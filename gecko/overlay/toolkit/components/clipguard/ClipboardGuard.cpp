/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "ClipboardGuard.h"

#include "mozilla/Clipguard.h"  // the clipguard Rust crate (cbindgen)
#include "mozilla/ClipguardEvent.h"
#include "mozilla/RefPtr.h"
#include "mozilla/Services.h"
#include "mozilla/StaticPrefs_clipguard.h"
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

}  // namespace

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

  // v1: a page "copy" handler that replaced the selection is treated as a script write;
  // the visibility check still lets it through when the copied command is on screen (an
  // ordinary copy button that reformats the selection). Comparing the selection's own
  // danger level (clipguard_is_substitution) to spare an admin who copied a real install
  // command through such a button is left for v1.1, when the selection text is carried.
  const bool substituted = (source == ClipguardSource::PageReplacedCopy);

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
