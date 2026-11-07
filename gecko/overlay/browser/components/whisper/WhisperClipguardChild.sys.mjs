/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Content half of the WhisperClipguard window actor. Given short pieces of a command
 * (its longest plain words), it returns the text of the visible elements that contain
 * them, so the parent can tell whether the command is actually shown on the page.
 *
 * Only visible text is returned: an element is skipped when it or an ancestor is hidden
 * (display:none, visibility:hidden, content-visibility), fully transparent, zero-sized,
 * clipped away, or off the page. This is the same idea the parent's clipguard crate uses
 * to compare; the page is never told it was asked.
 */
export class WhisperClipguardChild extends JSWindowActorChild {
  async receiveMessage(message) {
    if (message.name == "FindVisibleText") {
      return this.findVisibleText(message.data.probes || []);
    }
    return null;
  }

  findVisibleText(probes) {
    const doc = this.document;
    if (!doc || !doc.body || !probes.length) {
      return [];
    }
    const win = doc.defaultView;
    const needles = probes.map(p => p.toLowerCase()).filter(p => p.length >= 3);
    if (!needles.length) {
      return [];
    }

    const texts = [];
    const MAX_ELEMENTS = 4000; // bound the scan on a huge page
    const MAX_HITS = 40;
    let scanned = 0;

    // Walk text-bearing elements; a NodeFilter keeps it cheap.
    const walker = doc.createTreeWalker(doc.body, win.NodeFilter.SHOW_ELEMENT);
    let node = walker.currentNode;
    while (node && scanned < MAX_ELEMENTS && texts.length < MAX_HITS) {
      scanned++;
      const text = this._directText(node);
      if (text && needles.some(n => text.toLowerCase().includes(n))) {
        if (this._isVisible(win, node)) {
          texts.push(text);
        }
      }
      node = walker.nextNode();
    }
    return texts;
  }

  /** The text directly inside an element (its own text nodes), trimmed. */
  _directText(el) {
    let s = "";
    for (const child of el.childNodes) {
      if (child.nodeType == child.TEXT_NODE) {
        s += child.nodeValue;
      }
    }
    s = s.trim();
    return s.length >= 3 ? s : "";
  }

  _isVisible(win, el) {
    let node = el;
    let depth = 0;
    while (node && node.nodeType == node.ELEMENT_NODE && depth++ < 60) {
      const style = win.getComputedStyle(node);
      if (
        style.display == "none" ||
        style.visibility == "hidden" ||
        style.visibility == "collapse" ||
        style.contentVisibility == "hidden" ||
        parseFloat(style.opacity) === 0
      ) {
        return false;
      }
      node = node.parentElement || node.parentNode?.host;
    }
    const rect = el.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      return false;
    }
    // On the page, not scrolled entirely out of the document.
    const w = win.innerWidth || 0;
    const h = win.innerHeight || 0;
    if (rect.bottom < 0 || rect.right < 0 || rect.left > w || rect.top > h + rect.height) {
      // Allow elements a little below the fold (the person can scroll to them), but not
      // ones pushed far off to the side or above the page (a common hiding trick).
      if (rect.right < -4 || rect.left > w + 4 || rect.bottom < -4) {
        return false;
      }
    }
    return true;
  }
}
