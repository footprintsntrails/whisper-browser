/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Parent half of the WhisperClipguard window actor. It only exists so WhisperClipguard
 * can call sendQuery("FindVisibleText") on the child; there is no message the other way.
 */
export class WhisperClipguardParent extends JSWindowActorParent {}
