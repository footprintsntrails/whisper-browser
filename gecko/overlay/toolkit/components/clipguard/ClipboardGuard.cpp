/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "ClipboardGuard.h"

#include <type_traits>

#include "mozilla/AutoRestore.h"
#include "mozilla/ClearOnShutdown.h"
#include "mozilla/Clipguard.h"  // the clipguard Rust crate (cbindgen)
#include "mozilla/ClipguardEvent.h"
#include "mozilla/MoveOnlyFunction.h"
#include "mozilla/RefPtr.h"
#include "mozilla/Services.h"
#include "mozilla/StaticPrefs_clipguard.h"
#include "mozilla/StaticPtr.h"
#include "mozilla/TimeStamp.h"
#include "mozilla/dom/BrowsingContext.h"
#include "mozilla/dom/WindowContext.h"
#include "mozilla/dom/WindowGlobalParent.h"
#include "nsComponentManagerUtils.h"
#include "nsIClipboardOwner.h"
#include "nsIContentAnalysis.h"
#include "nsIObserverService.h"
#include "nsIPermissionManager.h"
#include "nsIPrincipal.h"
#include "nsISupportsPrimitives.h"
#include "nsITransferable.h"
#include "nsServiceManagerUtils.h"
#include "nsThreadUtils.h"
#include "nsWidgetsCID.h"

namespace mozilla {

using dom::WindowContext;
using dom::WindowGlobalParent;

// Contract with nsBaseClipboard (patches/0042-clipguard-hold.patch). The guard reaches
// into private clipboard internals, so a Firefox rebase can silently break it. On every
// engine update re-check, and keep this list in step with the patch header:
//   1. nsBaseClipboard::SetDataImpl still takes aCheckContentAnalysis and, when it is
//      false, neither re-runs this guard nor the content-analysis copy check, and commits
//      synchronously. (This guard calls that private method via friendship.)
//   2. The write completion runs exactly once; passing it to the guard and NOT calling
//      finish() on the Handled path still resolves the page's promise once.
//   3. PendingCopy / CancelPendingCopy and the mMutatingNativeClipboard re-entrancy guard
//      still exist and behave as in 159.
//   4. The read methods the guard hooks (GetDataIfSmallerThanNative, GetDataSnapshot,
//      GetDataSnapshotSync) still carry the requesting window/principal.
// (1) and the completion type are checked at compile time here; (2)/(3) by the
// re-entrancy assertion in Guard() and the smoke test; a signature change breaks the
// build at the call sites.
static_assert(
    std::is_same_v<nsBaseClipboard::SetDataCompletion,
                   mozilla::MoveOnlyFunction<void(nsresult)>>,
    "nsBaseClipboard::SetDataCompletion changed; re-check the clipguard contract above "
    "(patches/0042-clipguard-hold.patch) before updating this assertion.");

// Set while the guard runs a commit that must skip the clipguard re-check but still go
// through Firefox's content-analysis copy interception (F6). Main-thread only: all
// clipboard writes run on the parent main thread.
static bool sSkipGuardOnce = false;

namespace {

// clipguard.mode: 0 block, 1 warn, 2 off.
bool GuardOff() { return StaticPrefs::clipguard_mode() == 2; }

// The text/plain of a transferable, or its text/html if there is no plain text.
bool GetClipboardText(nsITransferable* aTransferable, nsAString& aText,
                      bool& aIsHtml) {
  aIsHtml = false;
  auto read = [&](const char* aFlavor, nsAString& aOut) -> bool {
    nsCOMPtr<nsISupports> data;
    if (NS_FAILED(aTransferable->GetTransferData(aFlavor, getter_AddRefs(data))) ||
        !data) {
      return false;
    }
    nsCOMPtr<nsISupportsString> str = do_QueryInterface(data);
    if (!str) {
      return false;
    }
    str->GetData(aOut);
    return !aOut.IsEmpty();
  };
  if (read(kTextMime, aText)) {
    return true;
  }
  if (read(kHTMLMime, aText)) {
    aIsHtml = true;
    return true;
  }
  return false;
}

// Removes a trailing line break or ESC from every text flavor, so pasting the text into
// a terminal does not run it on its own. Returns whether anything changed.
bool StripTrailing(nsITransferable* aTransferable) {
  bool changed = false;
  for (const char* flavor : {kTextMime, kHTMLMime}) {
    nsCOMPtr<nsISupports> data;
    if (NS_FAILED(aTransferable->GetTransferData(flavor, getter_AddRefs(data))) ||
        !data) {
      continue;
    }
    nsCOMPtr<nsISupportsString> str = do_QueryInterface(data);
    if (!str) {
      continue;
    }
    nsAutoString text;
    str->GetData(text);
    int64_t keep = clipguard::clipguard_trailing_strip_len(&text);
    if (keep >= 0 && static_cast<uint64_t>(keep) < text.Length()) {
      text.Truncate(static_cast<uint32_t>(keep));
      str->SetData(text);
      aTransferable->SetTransferData(flavor, str);
      changed = true;
    }
  }
  return changed;
}

nsCOMPtr<nsITransferable> MakePlaceholderTransferable() {
  nsCOMPtr<nsITransferable> trans =
      do_CreateInstance("@mozilla.org/widget/transferable;1");
  nsCOMPtr<nsISupportsString> data =
      do_CreateInstance("@mozilla.org/supports-string;1");
  if (!trans || !data) {
    return nullptr;
  }
  trans->Init(nullptr);
  nsAutoString placeholder;
  clipguard::clipguard_placeholder(&placeholder);
  if (NS_FAILED(trans->AddDataFlavor(kTextMime)) ||
      NS_FAILED(data->SetData(placeholder)) ||
      NS_FAILED(trans->SetTransferData(kTextMime, data))) {
    return nullptr;
  }
  return trans;
}

// Whether the person chose "Always allow commands from this site" this session, for the
// window that wrote. The permission is session-scoped, so it is gone when the browser
// closes and is never written to permissions.sqlite.
bool SiteAllowed(WindowContext* aWindowContext) {
  if (!aWindowContext) {
    return false;
  }
  RefPtr<WindowGlobalParent> wgp = aWindowContext->Canonical();
  nsCOMPtr<nsIPrincipal> principal = wgp ? wgp->DocumentPrincipal() : nullptr;
  if (!principal || !principal->GetIsContentPrincipal()) {
    return false;
  }
  nsCOMPtr<nsIPermissionManager> perms =
      do_GetService("@mozilla.org/permissionmanager;1");
  if (!perms) {
    return false;
  }
  uint32_t permission = nsIPermissionManager::UNKNOWN_ACTION;
  if (NS_FAILED(perms->TestPermissionFromPrincipal(principal, "clipguard-allow"_ns,
                                                   &permission))) {
    return false;
  }
  return permission == nsIPermissionManager::ALLOW_ACTION;
}

// Copies every text flavor aTo can import from aFrom (text/plain, text/html).
void CopyTextFlavors(nsITransferable* aFrom, nsITransferable* aTo) {
  for (const char* flavor : {kTextMime, kHTMLMime}) {
    nsCOMPtr<nsISupports> data;
    if (NS_SUCCEEDED(aFrom->GetTransferData(flavor, getter_AddRefs(data))) && data) {
      aTo->AddDataFlavor(flavor);
      aTo->SetTransferData(flavor, data);
    }
  }
}

// A fresh transferable holding a copy of aSource's text flavors (for the read-back store,
// so it is not the same object that later has its trailing break cut for the clipboard).
nsCOMPtr<nsITransferable> CloneTextTransferable(nsITransferable* aSource,
                                                nsIPrincipal* aPrincipal) {
  nsCOMPtr<nsITransferable> trans =
      do_CreateInstance("@mozilla.org/widget/transferable;1");
  if (!trans) {
    return nullptr;
  }
  trans->Init(nullptr);
  trans->SetDataPrincipal(aPrincipal);
  CopyTextFlavors(aSource, trans);
  return trans;
}

// One held page's own text, for same-origin read-back while the block is in effect.
struct ReadbackEntry {
  nsIClipboard::ClipboardType mType = nsIClipboard::kGlobalClipboard;
  nsCOMPtr<nsIPrincipal> mPrincipal;
  nsCOMPtr<nsITransferable> mOwnText;
  mozilla::TimeStamp mExpiry;
};

// At most one entry per clipboard type; the global clipboard is the only one guarded.
mozilla::StaticAutoPtr<nsTArray<ReadbackEntry>> sReadback;

nsTArray<ReadbackEntry>& Readback() {
  if (!sReadback) {
    sReadback = new nsTArray<ReadbackEntry>();
    ClearOnShutdown(&sReadback);
  }
  return *sReadback;
}

// The content principal for a window context, or null.
already_AddRefed<nsIPrincipal> ContentPrincipalOf(WindowContext* aWindowContext) {
  if (!aWindowContext) {
    return nullptr;
  }
  RefPtr<WindowGlobalParent> wgp = aWindowContext->Canonical();
  nsCOMPtr<nsIPrincipal> principal = wgp ? wgp->DocumentPrincipal() : nullptr;
  if (!principal || !principal->GetIsContentPrincipal()) {
    return nullptr;
  }
  return principal.forget();
}

// The read-back entry for this type, matched to a reader of the SAME ORIGIN as the writer
// (strict principal Equals), whatever browsing context it is in, and not expired; prunes
// expired entries. Keying on the origin, not the writer's top browsing context, means a
// same-origin popup or tab opened by the page also reads the page's own text, so the page
// cannot use a popup as an oracle for "was I blocked". A different origin never matches, so
// it still gets the placeholder.
ReadbackEntry* MatchReadback(nsIClipboard::ClipboardType aWhichClipboard,
                             WindowContext* aReaderWindowContext,
                             nsIPrincipal* aReaderPrincipal) {
  if (!sReadback) {
    return nullptr;
  }
  // The snapshot read path passes the reader principal explicitly; the sync read path
  // takes it from the reader window.
  nsCOMPtr<nsIPrincipal> readerPrincipal = aReaderPrincipal;
  if (!readerPrincipal) {
    readerPrincipal = ContentPrincipalOf(aReaderWindowContext);
  }
  if (!readerPrincipal || !readerPrincipal->GetIsContentPrincipal()) {
    return nullptr;
  }
  auto& entries = Readback();
  mozilla::TimeStamp now = mozilla::TimeStamp::Now();
  for (size_t i = entries.Length(); i > 0; --i) {
    ReadbackEntry& e = entries[i - 1];
    if (now > e.mExpiry) {
      entries.RemoveElementAt(i - 1);
      continue;
    }
    if (e.mType == aWhichClipboard && e.mPrincipal &&
        e.mPrincipal->Equals(readerPrincipal)) {
      return &e;
    }
  }
  return nullptr;
}

}  // namespace

/* static */ void ClipboardGuard::RememberReadback(
    nsIClipboard::ClipboardType aWhichClipboard, WindowContext* aWriterWindowContext,
    nsITransferable* aOwnText) {
  nsCOMPtr<nsIPrincipal> principal = ContentPrincipalOf(aWriterWindowContext);
  if (!principal || !aOwnText) {
    return;
  }
  ForgetReadback(aWhichClipboard);
  ReadbackEntry entry;
  entry.mType = aWhichClipboard;
  entry.mPrincipal = principal;
  entry.mOwnText = aOwnText;
  entry.mExpiry =
      mozilla::TimeStamp::Now() + mozilla::TimeDuration::FromMilliseconds(kClipguardHoldMs);
  Readback().AppendElement(std::move(entry));
}

/* static */ void ClipboardGuard::ForgetReadback(
    nsIClipboard::ClipboardType aWhichClipboard) {
  if (!sReadback) {
    return;
  }
  sReadback->RemoveElementsBy(
      [&](const ReadbackEntry& e) { return e.mType == aWhichClipboard; });
}

/* static */ bool ClipboardGuard::FillReadbackIfSameOrigin(
    nsIClipboard::ClipboardType aWhichClipboard,
    WindowContext* aReaderWindowContext, nsIPrincipal* aReaderPrincipal,
    nsITransferable* aReaderTransferable) {
  ReadbackEntry* e =
      MatchReadback(aWhichClipboard, aReaderWindowContext, aReaderPrincipal);
  if (!e || !aReaderTransferable) {
    return false;
  }
  CopyTextFlavors(e->mOwnText, aReaderTransferable);
  return true;
}

/* static */ already_AddRefed<nsITransferable>
ClipboardGuard::TakeReadbackTransferable(nsIClipboard::ClipboardType aWhichClipboard,
                                         WindowContext* aReaderWindowContext,
                                         nsIPrincipal* aReaderPrincipal) {
  ReadbackEntry* e =
      MatchReadback(aWhichClipboard, aReaderWindowContext, aReaderPrincipal);
  if (!e) {
    return nullptr;
  }
  nsCOMPtr<nsITransferable> copy =
      CloneTextTransferable(e->mOwnText, e->mPrincipal);
  return copy.forget();
}

// Fills a ClipguardContext from the prefs and the source, then asks the crate.
static clipguard::ClipguardDecision Decide(const clipguard::ClipguardVerdict& aVerdict,
                                           ClipguardSource aSource, bool aSiteAllowed,
                                           bool aSubstituted, uint8_t aVisibility) {
  clipguard::ClipguardContext ctx{};
  ctx.source = static_cast<uint8_t>(aSource);
  ctx.mode = static_cast<uint8_t>(StaticPrefs::clipguard_mode());
  ctx.strict_script_copy = StaticPrefs::clipguard_strict_script_copy();
  ctx.user_copy_notice = StaticPrefs::clipguard_user_copy_notice();
  ctx.site_allowed = aSiteAllowed;
  ctx.substituted = aSubstituted;
  return clipguard::clipguard_decide(&aVerdict, &ctx, aVisibility);
}

/* static */ bool ClipboardGuard::IsEnabled() { return !GuardOff(); }

// Whisper's swapped-address guard (browser/components/whisper/security) remembers the
// last cryptocurrency address copied in Whisper. A page's navigator.clipboard write fires
// no "copy" event for it to see, so the parent tells it: observers of
// "whisper-script-copy" get the text. Short texts only (an address is far shorter);
// parent process only, nothing is kept or logged here.
static void NotifyScriptCopy(const nsAString& aText) {
  if (aText.IsEmpty() || aText.Length() > 256) {
    return;
  }
  nsCOMPtr<nsIObserverService> obs = services::GetObserverService();
  if (obs) {
    obs->NotifyObservers(nullptr, "whisper-script-copy",
                         PromiseFlatString(aText).get());
  }
}

/* static */ void ClipboardGuard::CommitAllowedWrite(
    nsBaseClipboard* aClipboard, nsITransferable* aTransferable,
    nsIClipboardOwner* aOwner, nsIClipboard::ClipboardType aWhichClipboard,
    WindowContext* aWindowContext) {
  // The person chose "Copy anyway", or the command turned out to be on screen: put the
  // real text on the clipboard. Skip the clipguard re-check (the decision is already
  // made), but if enterprise Content Analysis is active, still route the write through it
  // -- a normal copy would be checked, so "Copy anyway" must be too (F6). When Content
  // Analysis is not active (the usual case) this is the plain, synchronous commit.
  const bool contentAnalysis = nsIContentAnalysis::MightBeActive();
  if (contentAnalysis) {
    sSkipGuardOnce = true;
  }
  aClipboard->SetDataImpl(aTransferable, aOwner, aWhichClipboard, aWindowContext,
                          /* aCheckContentAnalysis */ contentAnalysis);
  sSkipGuardOnce = false;
}

/* static */ ClipboardGuard::Result ClipboardGuard::Guard(
    nsBaseClipboard* aClipboard, nsITransferable* aTransferable,
    nsIClipboardOwner* aOwner, nsIClipboard::ClipboardType aWhichClipboard,
    WindowContext* aWindowContext, nsBaseClipboard::SetDataCompletion& aCompletion) {
  // A guard-initiated commit that is going through content analysis (see
  // CommitAllowedWrite): skip the clipguard re-check, but let the rest of SetDataImpl,
  // including the content-analysis copy check, run.
  if (sSkipGuardOnce) {
    sSkipGuardOnce = false;
    return Result::NotGuarded;
  }

  // The guard must never be entered re-entrantly: the placeholder and commit writes pass
  // aCheckContentAnalysis = false precisely so SetDataImpl does not call back into here.
  // If a rebase breaks that, this fires loudly in debug builds (see the contract above).
  static bool sInGuard = false;
  MOZ_DIAGNOSTIC_ASSERT(
      !sInGuard,
      "clipguard re-entered SetDataImpl's guarded pass; the aCheckContentAnalysis=false "
      "contract (patches/0042-clipguard-hold.patch) may have changed on a Firefox rebase.");
  mozilla::AutoRestore<bool> restoreInGuard(sInGuard);
  sInGuard = true;

  // Only content writes to the global clipboard are checked. A null or chrome window is
  // the browser itself (the URL bar, an about: page, the placeholder write below) and is
  // left alone -- the same gate content analysis uses in NeedsCopyContentAnalysis.
  if (aWhichClipboard != nsIClipboard::kGlobalClipboard || !aTransferable ||
      !aWindowContext || aWindowContext->IsInProcess() ||
      !aWindowContext->GetBrowsingContext() ||
      aWindowContext->GetBrowsingContext()->IsChrome()) {
    return Result::NotGuarded;
  }

  nsAutoString text;
  bool isHtml = false;
  if (!GetClipboardText(aTransferable, text, isHtml)) {
    return Result::NotGuarded;
  }

  // Who wrote it (the content process set this in nsCopySupport). The field rides on
  // IPCTransferable, never on the native clipboard.
  ClipguardSource source =
      static_cast<ClipguardSource>(aTransferable->GetWhisperCopySource());
  if (source == ClipguardSource::AsyncClipboard) {
    NotifyScriptCopy(text);
  }
  if (GuardOff()) {
    return Result::NotGuarded;
  }

  clipguard::ClipguardVerdict verdict{};
  clipguard::clipguard_classify(&text, isHtml, &verdict);
  if (verdict.level == 0) {
    return Result::NotGuarded;  // not a command
  }

  // A page "copy" handler that replaced the person's selection (P2). It counts as a
  // substitution -- and so is treated like a script write -- only when the replacement is
  // more dangerous than what the person selected (clipguard_is_substitution). A handler
  // that tidies the selection, or appends something harmless like "Read more at <url>",
  // does not, so an ordinary copy button is left alone. With no selection text to compare
  // (unusual for P2), err toward treating it as a substitution.
  bool substituted = (source == ClipguardSource::PageReplacedCopy);
  if (substituted) {
    nsAutoString selection;
    selection = aTransferable->GetWhisperSelection();
    if (!selection.IsEmpty()) {
      substituted =
          clipguard::clipguard_is_substitution(&selection, &text, verdict.level);
    }
  }

  // "Always allow commands from this site" (this session) is a session permission the
  // doorhanger adds for the writing origin (WhisperClipguard.sys.mjs). Check it here.
  const bool siteAllowed = SiteAllowed(aWindowContext);

  clipguard::ClipguardDecision decision =
      Decide(verdict, source, siteAllowed, substituted, /* Unknown */ 0);

  switch (static_cast<ClipguardAction>(decision.action)) {
    case ClipguardAction::Ignore:
      return Result::NotGuarded;

    case ClipguardAction::Allow: {
      // Write the page's text as usual; just remove a trailing line break first and
      // tell the interface. The page's completion runs in the normal flow.
      if (decision.strip_trailing) {
        StripTrailing(aTransferable);
      }
      ClipguardEvent::DispatchNotice(aClipboard, aWindowContext, source, verdict,
                                     decision.notice, text);
      return Result::NotGuarded;
    }

    case ClipguardAction::CheckVisibility:
    case ClipguardAction::Block: {
      // Copy the page's own text, as written, before the trailing break is cut, so that
      // same-origin read-back can return exactly what the page wrote. It is registered
      // only after the placeholder write below, which clears nothing (that write passes
      // aCheckContentAnalysis = false, so SetDataImpl does not forget read-back for it).
      RefPtr<WindowGlobalParent> wgp = aWindowContext->Canonical();
      nsCOMPtr<nsIPrincipal> writer = wgp ? wgp->DocumentPrincipal() : nullptr;
      nsCOMPtr<nsITransferable> ownText =
          writer ? CloneTextTransferable(aTransferable, writer) : nullptr;

      // Put a harmless placeholder on the clipboard now, so nothing dangerous is ever
      // there, and hold the real text. The page's write "succeeds" at once: run its
      // completion with NS_OK and commit the placeholder with no completion.
      if (decision.strip_trailing) {
        StripTrailing(aTransferable);
      }
      RefPtr<ClipboardGuardHold> hold = new ClipboardGuardHold();
      hold->mClipboard = aClipboard;
      hold->mTransferable = aTransferable;
      hold->mOwner = aOwner;
      hold->mWindowContext = aWindowContext;
      hold->mWhichClipboard = aWhichClipboard;

      if (nsCOMPtr<nsITransferable> placeholder = MakePlaceholderTransferable()) {
        aClipboard->SetDataImpl(placeholder, nullptr, aWhichClipboard, nullptr,
                                /* aCheckContentAnalysis */ false);
      }
      if (ownText) {
        RememberReadback(aWhichClipboard, aWindowContext, ownText);
      }

      nsBaseClipboard::SetDataCompletion completion = std::move(aCompletion);
      if (completion) {
        completion(NS_OK);
      }

      ClipguardEvent::Start(hold, source, verdict, text,
                            static_cast<ClipguardAction>(decision.action),
                            decision.notice);
      return Result::Handled;
    }
  }
  return Result::NotGuarded;
}

}  // namespace mozilla
