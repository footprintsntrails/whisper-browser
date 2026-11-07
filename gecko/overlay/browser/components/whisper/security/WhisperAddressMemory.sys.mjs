/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The last cryptocurrency address the user copied in Whisper, kept in the parent
 * process, in memory only, for whisper.security.addressguard.minutes.
 *
 * Content processes must compare a pasted address at once (before the page sees the
 * paste), so they get a salted SHA-256 of the remembered address through
 * Services.ppmm.sharedData, never the address itself; the salt is new each run. When
 * the hashes differ, the content process asks the parent, which shows both addresses.
 *
 * Nothing is written to disk. Panic and Burn call forget().
 */

import {
  hex,
  sha256,
  utf8,
} from "moz-src:///browser/components/whisper/security/WhisperHash.sys.mjs";

const SHARED_KEY = "whisper:addressGuard";
const MINUTES_PREF = "whisper.security.addressguard.minutes";

let gRemembered = null; // {address, family, normalized, expires}
let gTimer = null;
let gSalt = null;

function salt() {
  if (!gSalt) {
    gSalt = hex(crypto.getRandomValues(new Uint8Array(16)));
  }
  return gSalt;
}

/** The hash content processes compare with (WhisperInputGuardChild does the same). */
export function addressHash(saltHex, normalized) {
  return hex(sha256(utf8(`${saltHex}:${normalized}`)));
}

function publish() {
  const shared = Services.ppmm.sharedData;
  if (gRemembered) {
    shared.set(SHARED_KEY, {
      family: gRemembered.family,
      salt: salt(),
      hash: addressHash(salt(), gRemembered.normalized),
      expires: gRemembered.expires,
    });
  } else {
    shared.delete(SHARED_KEY);
  }
  shared.flush();
}

export const WhisperAddressMemory = {
  SHARED_KEY,

  /**
   * @param {{address: string, family: string, normalized: string}} found
   *   CryptoAddress.classifyAddress of the copied text, run again in the parent
   */
  remember(found) {
    const minutes = Math.max(1, Math.min(120, Services.prefs.getIntPref(MINUTES_PREF, 10)));
    gRemembered = {
      address: found.address,
      family: found.family,
      normalized: found.normalized,
      expires: Date.now() + minutes * 60000,
    };
    publish();
    gTimer?.cancel();
    gTimer = Cc["@mozilla.org/timer;1"].createInstance(Ci.nsITimer);
    gTimer.initWithCallback(() => this.forget(), minutes * 60000, Ci.nsITimer.TYPE_ONE_SHOT);
  },

  /** The remembered address, unless it has expired. */
  current() {
    if (gRemembered && Date.now() > gRemembered.expires) {
      this.forget();
    }
    return gRemembered;
  },

  /** A copy in Whisper that is not an address replaces the remembered one. */
  forget() {
    const had = !!gRemembered;
    gRemembered = null;
    gTimer?.cancel();
    gTimer = null;
    if (had) {
      publish();
    }
  },
};
