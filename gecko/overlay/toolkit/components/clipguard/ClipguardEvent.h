/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef mozilla_ClipguardEvent_h
#define mozilla_ClipguardEvent_h

#include "ClipboardGuard.h"
#include "mozilla/Clipguard.h"
#include "nsCOMPtr.h"
#include "nsIClipguardEvent.h"
#include "nsITimer.h"
#include "nsString.h"

namespace mozilla {

class ClipboardGuardHold;

// The subject of a "clipguard-event" observer notification: what the guard did with one
// clipboard write, and the ways the person can respond (see nsIClipguardEvent.idl). It
// owns the held copy for a blocked or not-yet-checked write and drops it after 60 s.
class ClipguardEvent final : public nsIClipguardEvent, public nsITimerCallback {
 public:
  NS_DECL_ISUPPORTS
  NS_DECL_NSICLIPGUARDEVENT
  NS_DECL_NSITIMERCALLBACK

  // A write that was allowed: tell the interface what to show. Nothing is held.
  static void DispatchNotice(nsBaseClipboard* aClipboard,
                             dom::WindowContext* aWindowContext,
                             ClipguardSource aSource,
                             const clipguard::ClipguardVerdict& aVerdict,
                             uint8_t aNotice, const nsAString& aText);

  // A write that was held (blocked, or waiting on a visibility check). Starts the 60 s
  // timer and notifies the interface.
  static void Start(ClipboardGuardHold* aHold, ClipguardSource aSource,
                    const clipguard::ClipguardVerdict& aVerdict,
                    const nsAString& aText, ClipguardAction aAction,
                    uint8_t aNotice);

 private:
  ClipguardEvent() = default;
  ~ClipguardEvent();

  void NotifyObservers();  // notify "clipguard-event" observers (subject: this)
  void CommitHeldText();
  // Decide a held check with the page's answer (visibility 1, 2 or 3).
  void Resolve(uint8_t aVisibility);
  void DropHold();

  RefPtr<ClipboardGuardHold> mHold;  // null for a notice-only event
  RefPtr<dom::WindowContext> mWindow;  // the writing window, for the panel/indicator
  nsCOMPtr<nsITimer> mTimer;

  uint32_t mKind = 0;
  uint32_t mNotice = 0;
  uint32_t mLevel = 0;
  uint32_t mSource = 0;
  uint32_t mReasons = 0;
  nsString mText;  // the real text, held for display and "copy anyway"
  nsString mDisplayText;
  nsCString mHosts;
  bool mResolved = false;  // resolveVisibility has run
};

}  // namespace mozilla

#endif  // mozilla_ClipguardEvent_h
