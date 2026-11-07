/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef mozilla_ClipboardGuardSource_h
#define mozilla_ClipboardGuardSource_h

// Shared between the content process (dom/base/nsCopySupport.cpp, which labels a
// clipboard write) and the parent (toolkit/components/clipguard, which reads the label).
// The label is the nsITransferable's whisperCopySource attribute; it travels to the
// parent as a field of IPCTransferable (dom/ipc/IPCTransferable.ipdlh), separate from
// the data, so it never reaches the native clipboard.

#include <cstdint>

namespace mozilla {

// Who put the text on the clipboard. Matches clipguard::Source in the Rust crate.
enum class ClipguardSource : uint8_t {
  Unknown = 0,
  // The person copied a selection (Ctrl+C, the context menu) with no page handler, or a
  // handler that did not change the data.
  UserCopy = 1,
  // The person copied, and the page's "copy" handler replaced the data.
  PageReplacedCopy = 2,
  // document.execCommand("copy").
  ExecCommand = 3,
  // navigator.clipboard.write() / writeText().
  AsyncClipboard = 4,
  // From a content process with no label.
  ContentUnlabeled = 5,
};

// What the browser does with a write. Matches clipguard::Action in the Rust crate.
enum class ClipguardAction : uint8_t {
  Ignore = 0,
  Allow = 1,
  CheckVisibility = 2,
  Block = 3,
};

}  // namespace mozilla

#endif  // mozilla_ClipboardGuardSource_h
