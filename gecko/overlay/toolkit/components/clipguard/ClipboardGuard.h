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
class nsIPrincipal;

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

  // --- Read-back: the page that was blocked must not learn it, so when the same page
  // (same top-level browsing context and the same origin) reads the clipboard while the
  // block is in effect, it gets its own written text, not the placeholder. Every other
  // reader -- another origin, another tab, the OS, other apps -- gets the placeholder.
  // nsBaseClipboard's read paths call these; they do nothing when nothing is held.

  // Remembers the page's own text (before the trailing break was cut, so the page reads
  // back exactly what it wrote) for same-origin read-back, keyed by the writing window.
  static void RememberReadback(nsIClipboard::ClipboardType aWhichClipboard,
                               mozilla::dom::WindowContext* aWriterWindowContext,
                               nsITransferable* aOwnText);

  // Drops the read-back entry for a clipboard type (a new write superseded it, the hold
  // was committed or discarded, or it expired).
  static void ForgetReadback(nsIClipboard::ClipboardType aWhichClipboard);

  // If the reader is the same page that was blocked, fills aReaderTransferable with the
  // page's own text and returns true. aReaderPrincipal may be null (then it is taken from
  // the reader window). Used by the synchronous read path (GetData).
  static bool FillReadbackIfSameOrigin(
      nsIClipboard::ClipboardType aWhichClipboard,
      mozilla::dom::WindowContext* aReaderWindowContext,
      nsIPrincipal* aReaderPrincipal, nsITransferable* aReaderTransferable);

  // Same match, for the async/snapshot read path: returns the page's own text as a
  // transferable to wrap in a snapshot, or null.
  static already_AddRefed<nsITransferable> TakeReadbackTransferable(
      nsIClipboard::ClipboardType aWhichClipboard,
      mozilla::dom::WindowContext* aReaderWindowContext,
      nsIPrincipal* aReaderPrincipal);
};

}  // namespace mozilla

#endif  // mozilla_ClipboardGuard_h
