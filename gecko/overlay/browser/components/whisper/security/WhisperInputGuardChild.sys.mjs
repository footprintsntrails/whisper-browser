/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The page-input guard, in the content process: a recovery phrase never reaches a web
 * page's field unnoticed, and a pasted cryptocurrency address that differs from the one
 * the user copied is stopped.
 *
 * Event order, and why the page cannot see the text first. The actor's listeners sit
 * on the window root, the outermost target of every event in this document (also for
 * same-process frames), above the page's window and document. Capture runs from the
 * outermost target inwards, so our capturing listener runs before every listener the
 * page can add, capturing or not. When we stop an event there (preventDefault and
 * stopImmediatePropagation), no page listener for it runs, and the editor, which only
 * inserts after the event if it was not cancelled (nsCopySupport::FireClipboardEvent,
 * EditorBase), leaves the field unchanged. For a paste or drop that means the page
 * never receives the text: it is only readable from that event's clipboardData or
 * dataTransfer, which Firefox clears after the event. Cross-process frames have their
 * own window root and actor.
 *
 * Typing is different, and the guard says so: a page sees every keydown before the
 * text is inserted, so it can have recorded what was typed. Two checks run on
 * beforeinput (after keydown, before the text is inserted and before any input
 * event): the whole field would become a phrase (exact count, one BIP39 list, valid
 * checksum), or the field already holds only list words and reaches
 * whisper.security.seedguard.pause_after_words (6) complete words. At 6 of 12 words,
 * the 6 unknown words (62 bits after the checksum) are far beyond guessing, so pausing
 * there still protects a real phrase. The field is emptied while the warning is open
 * and filled again only if the user allows it.
 *
 * Not covered, by design or because the page can do it another way:
 *   - text the page reads with navigator.clipboard.readText() (Firefox first asks the
 *     user with its own Paste button);
 *   - typing into rich-text editors (contenteditable); pastes there are checked;
 *   - a page that records keystrokes sees the words typed before the pause.
 *
 * Nothing is logged or stored: the text is read from the event, checked and dropped.
 * When a typed field is paused, its text is kept in this actor only until the user
 * decides, then released (JavaScript strings cannot be wiped; the last reference is
 * dropped). The parent only learns that a phrase was seen and how many words it had,
 * never the words.
 */

const lazy = {};
// CryptoAddress.sys.mjs exports functions, not an object of that name: the module itself.
ChromeUtils.defineLazyGetter(lazy, "CryptoAddress", () =>
  ChromeUtils.importESModule("moz-src:///browser/components/whisper/security/CryptoAddress.sys.mjs")
);
// SeedPhrase.sys.mjs exports functions, not an object of that name: the module itself.
ChromeUtils.defineLazyGetter(lazy, "SeedPhrase", () =>
  ChromeUtils.importESModule("moz-src:///browser/components/whisper/security/SeedPhrase.sys.mjs")
);
// WhisperHash.sys.mjs exports functions, not an object of that name: the module itself.
ChromeUtils.defineLazyGetter(lazy, "WhisperHash", () =>
  ChromeUtils.importESModule("moz-src:///browser/components/whisper/security/WhisperHash.sys.mjs")
);

const SEED_PREF = "whisper.security.seedguard.enabled";
const PAUSE_PREF = "whisper.security.seedguard.pause_after_words";
const ADDRESS_PREF = "whisper.security.addressguard.enabled";
const ADDRESS_KEY = "whisper:addressGuard";
const TEXT_INPUT_TYPES = new Set(["", "text", "search", "password", "email", "tel", "url"]);
// Fields of a form looked at for a phrase typed one word per field.
const MAX_FORM_FIELDS = 40;

function ensureLists() {
  const SP = lazy.SeedPhrase;
  if (!SP.haveLists()) {
    const { WORDLISTS } = ChromeUtils.importESModule(
      "moz-src:///browser/components/whisper/security/SeedWordlists.sys.mjs"
    );
    SP.setLists(SP.buildLists(WORDLISTS));
  }
}

export class WhisperInputGuardChild extends JSWindowActorChild {
  // Fields the user allowed with the hold button, for as long as the page lives.
  #allowed = new WeakSet();
  #dialogOpen = false;
  #guarded = null;

  /** Web pages only: not the browser's own pages, not extension pages (wallets). */
  #isGuarded() {
    if (this.#guarded === null) {
      const principal = this.document?.nodePrincipal;
      this.#guarded =
        !!principal &&
        !principal.isSystemPrincipal &&
        !principal.isAddonOrExpandedAddonPrincipal &&
        !["about", "moz-extension", "chrome", "resource"].some(s => principal.schemeIs(s));
    }
    return this.#guarded;
  }

  handleEvent(event) {
    if (!event.isTrusted || !this.#isGuarded()) {
      return;
    }
    try {
      switch (event.type) {
        case "paste":
          this.#onPaste(event, event.clipboardData);
          break;
        case "drop":
          this.#onPaste(event, event.dataTransfer);
          break;
        case "beforeinput":
          this.#onBeforeInput(event);
          break;
        case "copy":
        case "cut":
          this.#onCopy(event);
          break;
      }
    } catch (e) {
      // Never the text: only that the guard failed, and where. (Cu.reportError
      // reaches the parent's console from a content process.)
      Cu.reportError(`Whisper input guard: ${event.type} check failed: ${e.name}: ${e.message} (${e.lineNumber})`);
    }
  }

  // --- Recovery phrases ------------------------------------------------------------

  #seedEnabled() {
    return Services.prefs.getBoolPref(SEED_PREF, true);
  }

  #pauseAfter() {
    return Math.max(0, Services.prefs.getIntPref(PAUSE_PREF, 6));
  }

  /**
   * The editable element an event is for: a text field, a textarea or an editor.
   * Seen from the window root, event.target is the field itself, or the host when the
   * field is inside a shadow root; then composedTarget is the node inside, which can
   * be the field's own internal (native anonymous) editor node, so walk up from there.
   */
  #target(event) {
    for (let el of [event.target, event.composedTarget]) {
      for (let depth = 0; el && depth < 8; depth++) {
        if (el.nodeType == 1 && (this.#isTextField(el) || el.isContentEditable)) {
          return el;
        }
        el = el.parentNode ?? el.host ?? null;
      }
    }
    return null;
  }

  #isTextField(el) {
    if (el.namespaceURI != "http://www.w3.org/1999/xhtml") {
      return false;
    }
    if (el.localName == "textarea") {
      return true;
    }
    return (
      el.localName == "input" &&
      TEXT_INPUT_TYPES.has(el.getAttribute("type")?.toLowerCase() ?? "")
    );
  }

  /** The field's value as it would be with `text` in place of the selection. */
  #prospective(el, text) {
    if (!this.#isTextField(el)) {
      return text;
    }
    const value = el.value;
    let start = null;
    let end = null;
    try {
      start = el.selectionStart;
      end = el.selectionEnd;
    } catch (e) {
      // Types without a selection (email): the text goes at the end.
    }
    if (start === null || end === null) {
      return value + text;
    }
    return value.slice(0, start) + text + value.slice(end);
  }

  #onPaste(event, data) {
    const target = this.#target(event);
    if (!target || !data || this.#allowed.has(target)) {
      return;
    }
    const text = data.getData("text/plain");
    if (!text) {
      return;
    }
    if (this.#seedEnabled() && this.#checkSeed(event, target, text)) {
      return;
    }
    if (Services.prefs.getBoolPref(ADDRESS_PREF, true)) {
      this.#checkAddress(event, target, text);
    }
  }

  /**
   * Stops a paste or drop and warns when its text, or the field with the text in it,
   * is a phrase or the start of one. Returns true when it did.
   */
  #checkSeed(event, target, text) {
    const SP = lazy.SeedPhrase;
    const pause = this.#pauseAfter();
    const prospective = this.#prospective(target, text);
    if (!SP.mightBePhrase(text, pause) && !SP.mightBePhrase(prospective, pause)) {
      return false;
    }
    ensureLists();
    const found = SP.detectPhrase(text) ?? SP.detectPhrase(prospective);
    const start = found ? null : SP.phraseStart(prospective + " ", pause);
    if (!found && !start) {
      return false;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    this.#warn(target, {
      kind: event.type,
      complete: !!found,
      count: found?.count ?? start.count,
      scheme: found?.scheme ?? null,
      language: found?.language ?? start.language,
    });
    return true;
  }

  #onBeforeInput(event) {
    if (!this.#seedEnabled() || !event.inputType.startsWith("insert")) {
      return;
    }
    if (event.inputType == "insertFromPaste" || event.inputType == "insertFromDrop") {
      // Checked on the paste or drop event itself.
      return;
    }
    const target = this.#target(event);
    if (!target || !this.#isTextField(target) || this.#allowed.has(target)) {
      return;
    }
    const data = event.data ?? (event.inputType == "insertLineBreak" ? "\n" : "");
    const SP = lazy.SeedPhrase;
    const pause = this.#pauseAfter();
    const prospective = this.#prospective(target, data);
    let found = null;
    let start = null;
    let fields = null;
    if (SP.mightBePhrase(prospective, pause)) {
      ensureLists();
      found = SP.detectPhrase(prospective);
      start = found ? null : SP.phraseStart(prospective, pause);
    } else if (pause && /^\s*\S*\s?$/u.test(prospective)) {
      // One word per field (as wallet import forms do): typing into a field while
      // the form's other fields already hold single list words.
      ensureLists();
      fields = this.#oneWordFields(target, prospective);
      if (fields) {
        start = { count: fields.length, language: fields.language };
      }
    }
    if (!found && !start) {
      return;
    }
    // The page may have recorded the keys (keydown comes first); it gets neither this
    // input nor the text in the field while the user decides.
    if (event.cancelable) {
      event.preventDefault();
    }
    event.stopImmediatePropagation();
    const restore = [];
    for (const el of fields ?? [target]) {
      restore.push([el, el == target ? prospective : el.value]);
      el.setUserInput("");
    }
    this.#warn(
      target,
      {
        kind: "typed",
        complete: !!found,
        count: found?.count ?? start.count,
        scheme: found?.scheme ?? null,
        language: found?.language ?? start.language,
      },
      restore
    );
  }

  /**
   * The form's text fields that each hold exactly one complete word, if they are at
   * least whisper.security.seedguard.pause_after_words and all words of one list.
   * Other fields count as complete (the user moved on); `target` counts once a space
   * follows its word.
   */
  #oneWordFields(target, prospective) {
    const SP = lazy.SeedPhrase;
    const pause = this.#pauseAfter();
    const scope = target.form ?? target.ownerDocument;
    const all = Array.from(scope.querySelectorAll("input, textarea"))
      .filter(el => this.#isTextField(el))
      .slice(0, MAX_FORM_FIELDS);
    const words = [];
    const fields = [];
    for (const el of all) {
      const complete = el == target ? /^\s*\S+\s+$/u : /^\s*\S+\s*$/u;
      const value = el == target ? prospective : el.value;
      if (complete.test(value)) {
        words.push(value.trim());
        fields.push(el);
      }
    }
    if (fields.length < pause) {
      return null;
    }
    const start = SP.phraseStart(words.join(" ") + " ", pause);
    if (!start) {
      return null;
    }
    fields.language = start.language;
    return fields;
  }

  /**
   * Shows the warning (in the parent, over this tab). `restore` holds the fields
   * emptied while it is open and their text, put back only if the user allows.
   */
  async #warn(target, info, restore = []) {
    if (this.#dialogOpen) {
      restore.length = 0;
      return;
    }
    this.#dialogOpen = true;
    let allowed = false;
    try {
      const reply = await this.sendQuery("Seed:Warn", info);
      allowed = reply?.allow === true;
    } catch (e) {
      // The tab went away: nothing to restore.
    } finally {
      this.#dialogOpen = false;
    }
    try {
      if (allowed) {
        this.#allowed.add(target);
        for (const [el, text] of restore) {
          this.#allowed.add(el);
          if (el.isConnected) {
            el.setUserInput(text);
          }
        }
        if (target.isConnected) {
          target.focus();
        }
      }
    } finally {
      restore.length = 0;
    }
  }

  // --- Swapped addresses -----------------------------------------------------------

  /** After the page's own copy handlers (system group): what goes to the clipboard. */
  #onCopy(event) {
    if (!Services.prefs.getBoolPref(ADDRESS_PREF, true)) {
      return;
    }
    let text = "";
    if (event.defaultPrevented) {
      // The page put its own data on the clipboard.
      text = event.clipboardData?.getData("text/plain") ?? "";
    } else {
      const el = this.document.activeElement;
      let start = null;
      try {
        start = el && this.#isTextField(el) ? el.selectionStart : null;
      } catch (e) {}
      text =
        start !== null
          ? el.value.slice(start, el.selectionEnd)
          : (this.contentWindow?.getSelection()?.toString() ?? "");
    }
    const found = lazy.CryptoAddress.classifyAddress(text);
    // A copy that is not an address replaces the remembered one, so a later paste is
    // only compared with the last thing copied in Whisper.
    this.sendAsyncMessage("Address:Copied", found ? { address: found.address } : {});
  }

  #checkAddress(event, target, text) {
    const memory = Services.cpmm.sharedData.get(ADDRESS_KEY);
    if (!memory || Date.now() > memory.expires) {
      return;
    }
    const found = lazy.CryptoAddress.classifyAddress(text);
    if (!found || found.family != memory.family) {
      return;
    }
    const H = lazy.WhisperHash;
    if (H.hex(H.sha256(H.utf8(`${memory.salt}:${found.normalized}`))) == memory.hash) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    this.#askAddress(target, found.address);
  }

  async #askAddress(target, pasted) {
    if (this.#dialogOpen) {
      return;
    }
    this.#dialogOpen = true;
    let reply = null;
    try {
      reply = await this.sendQuery("Address:Mismatch", { pasted });
    } catch (e) {
      // The tab went away.
    } finally {
      this.#dialogOpen = false;
    }
    if (typeof reply?.insert == "string" && target.isConnected) {
      this.#insert(target, reply.insert);
    }
  }

  /** Puts text where the paste would have gone, as if the user had typed it. */
  #insert(target, text) {
    target.focus();
    if (!this.#isTextField(target)) {
      this.document.execCommand("insertText", false, text);
      return;
    }
    let caret = null;
    try {
      caret = target.selectionStart === null ? null : target.selectionStart + text.length;
    } catch (e) {}
    target.setUserInput(this.#prospective(target, text));
    if (caret !== null) {
      try {
        target.setSelectionRange(caret, caret);
      } catch (e) {}
    }
  }
}
