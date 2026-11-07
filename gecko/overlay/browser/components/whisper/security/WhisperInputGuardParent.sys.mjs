/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The parent side of the page-input guard: shows the warnings over the tab and keeps
 * the last copied address (WhisperAddressMemory). Messages come from a content process
 * and are not trusted: every field is checked for type and size, nothing in them is
 * shown as the page's identity, and an address is classified again here.
 */

const lazy = {};
// CryptoAddress.sys.mjs exports functions, not an object of that name: the module itself.
ChromeUtils.defineLazyGetter(lazy, "CryptoAddress", () =>
  ChromeUtils.importESModule("moz-src:///browser/components/whisper/security/CryptoAddress.sys.mjs")
);
ChromeUtils.defineESModuleGetters(lazy, {
  WhisperAddressMemory:
    "moz-src:///browser/components/whisper/security/WhisperAddressMemory.sys.mjs",
  WhisperGuardDialog:
    "moz-src:///browser/components/whisper/security/WhisperGuardDialog.sys.mjs",
});

const SEED_KINDS = new Set(["paste", "drop", "typed"]);

export class WhisperInputGuardParent extends JSWindowActorParent {
  async receiveMessage(message) {
    switch (message.name) {
      case "Seed:Warn":
        return this.#seedWarn(message.data);
      case "Address:Copied":
        this.#copied(message.data);
        return null;
      case "Address:Mismatch":
        return this.#mismatch(message.data);
    }
    return null;
  }

  async #seedWarn(data) {
    const count = data?.count;
    const info = {
      kind: SEED_KINDS.has(data?.kind) ? data.kind : "typed",
      complete: data?.complete === true,
      count: Number.isInteger(count) && count > 0 && count <= 48 ? count : 0,
    };
    const result = await lazy.WhisperGuardDialog.open(this.browsingContext, "seed", info);
    return { allow: result == "allow" };
  }

  #copied(data) {
    const memory = lazy.WhisperAddressMemory;
    const found =
      typeof data?.address == "string" ? lazy.CryptoAddress.classifyAddress(data.address) : null;
    if (found) {
      memory.remember(found);
    } else {
      memory.forget();
    }
  }

  async #mismatch(data) {
    const pasted =
      typeof data?.pasted == "string" ? lazy.CryptoAddress.classifyAddress(data.pasted) : null;
    const copied = lazy.WhisperAddressMemory.current();
    if (!pasted) {
      return { insert: null };
    }
    if (!copied || pasted.family != copied.family || pasted.normalized == copied.normalized) {
      // Nothing to compare after all (it expired meanwhile): paste as asked.
      return { insert: pasted.address };
    }
    const result = await lazy.WhisperGuardDialog.open(this.browsingContext, "address", {
      copied: copied.address,
      pasted: pasted.address,
    });
    if (result == "copied") {
      return { insert: copied.address };
    }
    if (result == "pasted") {
      return { insert: pasted.address };
    }
    return { insert: null };
  }
}
