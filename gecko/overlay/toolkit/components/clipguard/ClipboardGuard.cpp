/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "ClipboardGuard.h"

#include "mozilla/ClearOnShutdown.h"
#include "mozilla/Clipguard.h"  // the clipguard Rust crate (cbindgen)
#include "mozilla/ClipguardEvent.h"
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
  uint64_t mTopBrowsingContextId = 0;
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

// The top browsing context id and content principal for a window context, or false.
bool WriterIdentity(WindowContext* aWindowContext, uint64_t& aTopBcId,
                    nsCOMPtr<nsIPrincipal>& aPrincipal) {
  if (!aWindowContext) {
    return false;
  }
  dom::BrowsingContext* bc = aWindowContext->GetBrowsingContext();
  RefPtr<WindowGlobalParent> wgp = aWindowContext->Canonical();
  if (!bc || !wgp) {
    return false;
  }
  aPrincipal = wgp->DocumentPrincipal();
  if (!aPrincipal || !aPrincipal->GetIsContentPrincipal()) {
    return false;
  }
  aTopBcId = bc->Top()->Id();
  return true;
}

// The read-back entry for this type, matched to a reader that is the same page (same top
// browsing context and the same origin) and not expired; prunes expired entries.
ReadbackEntry* MatchReadback(nsIClipboard::ClipboardType aWhichClipboard,
                             WindowContext* aReaderWindowContext,
                             nsIPrincipal* aReaderPrincipal) {
  if (!sReadback || !aReaderWindowContext) {
    return nullptr;
  }
  dom::BrowsingContext* bc = aReaderWindowContext->GetBrowsingContext();
  RefPtr<WindowGlobalParent> wgp = aReaderWindowContext->Canonical();
  if (!bc || !wgp) {
    return nullptr;
  }
  uint64_t readerTopBc = bc->Top()->Id();
  // The snapshot read path passes the reader principal explicitly; the sync read path
  // takes it from the reader window.
  nsCOMPtr<nsIPrincipal> readerPrincipal =
      aReaderPrincipal ? aReaderPrincipal : wgp->DocumentPrincipal();
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
    if (e.mType == aWhichClipboard && e.mTopBrowsingContextId == readerTopBc &&
        e.mPrincipal && e.mPrincipal->Equals(readerPrincipal)) {
      return &e;
    }
  }
  return nullptr;
}

}  // namespace

/* static */ void ClipboardGuard::RememberReadback(
    nsIClipboard::ClipboardType aWhichClipboard, WindowContext* aWriterWindowContext,
    nsITransferable* aOwnText) {
  uint64_t topBc = 0;
  nsCOMPtr<nsIPrincipal> principal;
  if (!WriterIdentity(aWriterWindowContext, topBc, principal) || !aOwnText) {
    return;
  }
  ForgetReadback(aWhichClipboard);
  ReadbackEntry entry;
  entry.mType = aWhichClipboard;
  entry.mTopBrowsingContextId = topBc;
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

/* static */ ClipboardGuard::Result ClipboardGuard::Guard(
    nsBaseClipboard* aClipboard, nsITransferable* aTransferable,
    nsIClipboardOwner* aOwner, nsIClipboard::ClipboardType aWhichClipboard,
    WindowContext* aWindowContext, nsBaseClipboard::SetDataCompletion& aCompletion) {
  // Only content writes to the global clipboard are checked. A null or chrome window is
  // the browser itself (the URL bar, an about: page, the placeholder write below) and is
  // left alone -- the same gate content analysis uses in NeedsCopyContentAnalysis.
  if (GuardOff() || aWhichClipboard != nsIClipboard::kGlobalClipboard ||
      !aTransferable || !aWindowContext || aWindowContext->IsInProcess() ||
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
    aTransferable->GetWhisperSelection(selection);
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
