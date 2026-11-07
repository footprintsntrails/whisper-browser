/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "ClipguardEvent.h"

#include "mozilla/Services.h"
#include "mozilla/dom/WindowContext.h"
#include "mozilla/dom/WindowGlobalParent.h"
#include "nsIObserverService.h"
#include "mozilla/StaticPrefs_clipguard.h"
#include "nsISupportsPrimitives.h"
#include "nsITransferable.h"
#include "nsThreadUtils.h"

namespace mozilla {

using dom::WindowContext;
using dom::WindowGlobalParent;

NS_IMPL_ISUPPORTS(ClipguardEvent, nsIClipguardEvent, nsITimerCallback)

ClipguardEvent::~ClipguardEvent() { DropHold(); }

static void FillDisplay(const nsAString& aText, nsAString& aDisplay,
                        nsACString& aHosts) {
  clipguard::clipguard_display_text(&aText, &aDisplay);
  clipguard::clipguard_hosts(&aText, &aHosts);
}

/* static */ void ClipguardEvent::DispatchNotice(
    nsBaseClipboard*, WindowContext* aWindowContext, ClipguardSource aSource,
    const clipguard::ClipguardVerdict& aVerdict, uint8_t aNotice,
    const nsAString& aText) {
  if (aNotice == nsIClipguardEvent::NOTICE_NONE) {
    return;
  }
  RefPtr<ClipguardEvent> event = new ClipguardEvent();
  event->mKind = nsIClipguardEvent::KIND_NOTICE;
  event->mNotice = aNotice;
  event->mLevel = aVerdict.level;
  event->mSource = static_cast<uint32_t>(aSource);
  event->mReasons = aVerdict.reasons;
  event->mText = aText;
  event->mWindow = aWindowContext;
  FillDisplay(aText, event->mDisplayText, event->mHosts);
  event->NotifyObservers();
}

/* static */ void ClipguardEvent::Start(ClipboardGuardHold* aHold,
                                        ClipguardSource aSource,
                                        const clipguard::ClipguardVerdict& aVerdict,
                                        const nsAString& aText,
                                        ClipguardAction aAction, uint8_t aNotice) {
  RefPtr<ClipguardEvent> event = new ClipguardEvent();
  event->mHold = aHold;
  event->mWindow = aHold->mWindowContext;
  event->mSource = static_cast<uint32_t>(aSource);
  event->mLevel = aVerdict.level;
  event->mReasons = aVerdict.reasons;
  event->mText = aText;
  event->mNotice = aNotice;
  FillDisplay(aText, event->mDisplayText, event->mHosts);

  if (aAction == ClipguardAction::CheckVisibility) {
    event->mKind = nsIClipguardEvent::KIND_CHECK_VISIBILITY;
  } else {
    event->mKind = nsIClipguardEvent::KIND_BLOCKED;
  }

  NS_NewTimerWithCallback(getter_AddRefs(event->mTimer), event, kClipguardHoldMs,
                          nsITimer::TYPE_ONE_SHOT);
  event->NotifyObservers();
}

void ClipguardEvent::NotifyObservers() {
  if (nsCOMPtr<nsIObserverService> obs = services::GetObserverService()) {
    obs->NotifyObservers(static_cast<nsIClipguardEvent*>(this), "clipguard-event",
                         nullptr);
  }
}

void ClipguardEvent::CommitHeldText() {
  if (!mHold || !mHold->mLive || !mHold->mTransferable || !mHold->mClipboard) {
    return;
  }
  mHold->mLive = false;
  // The real text is going on the clipboard with its own origin; same-origin read-back no
  // longer needs the override.
  ClipboardGuard::ForgetReadback(mHold->mWhichClipboard);
  // Commit the page's real text: skip the clipguard re-check (already allowed), but go
  // through enterprise Content Analysis if it is active (F6).
  ClipboardGuard::CommitAllowedWrite(mHold->mClipboard, mHold->mTransferable,
                                     mHold->mOwner, mHold->mWhichClipboard,
                                     mHold->mWindowContext);
}

void ClipguardEvent::DropHold() {
  if (mTimer) {
    mTimer->Cancel();
    mTimer = nullptr;
  }
  if (mHold) {
    mHold->mLive = false;
    mHold->mTransferable = nullptr;  // let the held text go
    // The hold window is over ("Keep blocked", timeout): stop answering read-back too.
    ClipboardGuard::ForgetReadback(mHold->mWhichClipboard);
  }
}

NS_IMETHODIMP ClipguardEvent::Notify(nsITimer*) {
  DropHold();
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::GetKind(uint32_t* aKind) {
  *aKind = mKind;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetNotice(uint32_t* aNotice) {
  *aNotice = mNotice;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetLevel(uint32_t* aLevel) {
  *aLevel = mLevel;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetSource(uint32_t* aSource) {
  *aSource = mSource;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetReasons(uint32_t* aReasons) {
  *aReasons = mReasons;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetDisplayText(nsAString& aText) {
  aText = mDisplayText;
  return NS_OK;
}
NS_IMETHODIMP ClipguardEvent::GetHosts(nsACString& aHosts) {
  aHosts = mHosts;
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::GetPrincipal(nsIPrincipal** aPrincipal) {
  *aPrincipal = nullptr;
  if (mWindow && !mWindow->IsDiscarded()) {
    if (RefPtr<WindowGlobalParent> wgp = mWindow->Canonical()) {
      nsCOMPtr<nsIPrincipal> principal = wgp->DocumentPrincipal();
      principal.forget(aPrincipal);
    }
  }
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::GetWindowGlobal(WindowGlobalParent** aWgp) {
  *aWgp = nullptr;
  if (mWindow && !mWindow->IsDiscarded()) {
    RefPtr<WindowGlobalParent> wgp = mWindow->Canonical();
    wgp.forget(aWgp);
  }
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::GetVisibilityProbes(nsTArray<nsString>& aProbes) {
  aProbes.Clear();
  clipguard::clipguard_visibility_probes(&mText, &aProbes);
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::ResolveVisibility(
    const nsTArray<nsString>& aVisibleTexts) {
  if (mResolved || mKind != nsIClipguardEvent::KIND_CHECK_VISIBILITY || !mHold ||
      !mHold->mLive) {
    return NS_OK;
  }
  bool visible = clipguard::clipguard_is_visible(&mText, &aVisibleTexts);
  Resolve(visible ? /* Visible */ 1 : /* NotVisible */ 2);
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::ResolveVisibilityUnchecked() {
  if (mResolved || mKind != nsIClipguardEvent::KIND_CHECK_VISIBILITY || !mHold ||
      !mHold->mLive) {
    return NS_OK;
  }
  Resolve(/* Unchecked */ 3);
  return NS_OK;
}

void ClipguardEvent::Resolve(uint8_t aVisibility) {
  mResolved = true;

  clipguard::ClipguardVerdict verdict{};
  verdict.level = static_cast<uint8_t>(mLevel);
  verdict.reasons = mReasons;
  // Whether every address is a known installer is recomputed from the text.
  clipguard::ClipguardVerdict reclassified{};
  clipguard::clipguard_classify(&mText, false, &reclassified);
  verdict.known_installer = reclassified.known_installer;

  clipguard::ClipguardContext ctx{};
  ctx.source = static_cast<uint8_t>(mSource);
  ctx.mode = static_cast<uint8_t>(StaticPrefs::clipguard_mode());
  ctx.strict_script_copy = StaticPrefs::clipguard_strict_script_copy();
  ctx.user_copy_notice = StaticPrefs::clipguard_user_copy_notice();
  ctx.site_allowed = false;
  ctx.substituted = (mSource == static_cast<uint32_t>(ClipguardSource::PageReplacedCopy));
  clipguard::ClipguardDecision decision =
      clipguard::clipguard_decide(&verdict, &ctx, aVisibility);

  if (static_cast<ClipguardAction>(decision.action) == ClipguardAction::Allow) {
    // On screen after all: replace the placeholder with the page's text, then show
    // whatever notice the table calls for.
    CommitHeldText();
    mKind = nsIClipguardEvent::KIND_NOTICE;
    mNotice = decision.notice;
    DropHold();
    if (mNotice != nsIClipguardEvent::NOTICE_NONE) {
      NotifyObservers();
    }
  } else {
    // Not on screen: the placeholder stays. Become a blocked event so the person can
    // still choose "Copy anyway".
    mKind = nsIClipguardEvent::KIND_BLOCKED;
    mNotice = decision.notice;
    NotifyObservers();
  }
}

NS_IMETHODIMP ClipguardEvent::GetCanCopyAnyway(bool* aCan) {
  *aCan = mHold && mHold->mLive && mHold->mTransferable;
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::CopyAnyway(bool* aDone) {
  *aDone = false;
  if (mHold && mHold->mLive && mHold->mTransferable) {
    CommitHeldText();
    DropHold();
    *aDone = true;
  }
  return NS_OK;
}

NS_IMETHODIMP ClipguardEvent::Discard() {
  DropHold();
  return NS_OK;
}

}  // namespace mozilla
