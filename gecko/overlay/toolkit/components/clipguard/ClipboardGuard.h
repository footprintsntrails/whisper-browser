/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef mozilla_ClipboardGuard_h
#define mozilla_ClipboardGuard_h

// Whisper's ClickFix protection. nsBaseClipboard::SetDataImpl calls Guard() for every
// content write to the global clipboard; Guard() asks the clipguard Rust crate what to
// do and carries it out. Everything here runs on the parent process main thread.
//
// The page never learns the decision: its write always "succeeds". When a write is
// blocked, the clipboard is given a harmless placeholder instead, the real text is held
// in memory (at most 60 seconds, never written to disk) so the person can still choose
// "Copy anyway", and the browser interface is told through the "clipguard-event"
// observer topic (nsIClipguardEvent).

#include "mozilla/ClipboardGuardSource.h"  // ClipguardSource, the private flavors
#include "mozilla/RefPtr.h"
#include "nsBaseClipboard.h"
#include "nsCOMPtr.h"
#include "nsIClipboard.h"
#include "nsString.h"

class nsITransferable;
class nsIClipboardOwner;

namespace mozilla {
namespace dom {
class WindowContext;
}  // namespace dom

// A clipboard write that was blocked, or is waiting on a visibility check: the page's
// real text and everything needed to put it on the clipboard later, plus the window it
// came from. Held in memory only; ClipguardEvent drops it after 60 seconds.
class ClipboardGuardHold final {
 public:
  NS_INLINE_DECL_REFCOUNTING(ClipboardGuardHold)

  RefPtr<nsBaseClipboard> mClipboard;
  nsCOMPtr<nsITransferable> mTransferable;  // the page's real text (trailing break cut)
  nsCOMPtr<nsIClipboardOwner> mOwner;
  RefPtr<mozilla::dom::WindowContext> mWindowContext;
  nsIClipboard::ClipboardType mWhichClipboard = nsIClipboard::kGlobalClipboard;
  bool mLive = true;  // the held text is still available to commit

 private:
  ~ClipboardGuardHold() = default;
};

// The text of a blocked copy is held for this long so the person can still choose "Copy
// anyway", then dropped.
constexpr uint32_t kClipguardHoldMs = 60000;

class ClipboardGuard final {
 public:
  enum class Result : uint8_t {
    // SetDataImpl should carry on and write aTransferable as usual (which runs
    // aCompletion). A trailing line break may have been removed from it.
    NotGuarded,
    // The guard took over: it has written the placeholder or the page's text and run
    // aCompletion. SetDataImpl must return NS_OK and do nothing more.
    Handled,
  };

  // The only entry point. aCompletion is the page's write completion (it drives the
  // writeText promise); the guard either leaves it for SetDataImpl to run, or runs it
  // itself when it returns Handled.
  static Result Guard(nsBaseClipboard* aClipboard, nsITransferable* aTransferable,
                      nsIClipboardOwner* aOwner,
                      nsIClipboard::ClipboardType aWhichClipboard,
                      mozilla::dom::WindowContext* aWindowContext,
                      nsBaseClipboard::SetDataCompletion& aCompletion);

  // Whether the guard is on at all (clipguard.mode != off and the pref gate). A fast
  // check so SetDataImpl pays nothing when it is off.
  static bool IsEnabled();
};

}  // namespace mozilla

#endif  // mozilla_ClipboardGuard_h
