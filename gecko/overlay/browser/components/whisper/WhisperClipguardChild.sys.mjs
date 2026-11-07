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
    const own = win.getComputedStyle(el);

    // Text that is there but can't be read: a colour equal to the background, a colour
    // with no alpha, or a font too small to read. These render the exact command on
    // screen yet unreadable, which a plain visibility check would accept.
    if (this._colorIsInvisible(win, el, own)) {
      return false;
    }
    if (parseFloat(own.fontSize) < 6) {
      return false;
    }
    // A collapsed line box hides the text even at a normal font size.
    const lineHeight = parseFloat(own.lineHeight);
    if (own.lineHeight !== "normal" && !Number.isNaN(lineHeight) && lineHeight < 1) {
      return false;
    }
    // text-indent pushing the text out of its box (a classic off-screen trick).
    const indent = parseFloat(own.textIndent);
    if (!Number.isNaN(indent) && Math.abs(indent) > 2000) {
      return false;
    }

    // Walk ancestors for hiding set anywhere above, and for a clip that removes the box.
    let node = el;
    let depth = 0;
    while (node && node.nodeType == node.ELEMENT_NODE && depth++ < 60) {
      const style = node == el ? own : win.getComputedStyle(node);
      if (
        style.display == "none" ||
        style.visibility == "hidden" ||
        style.visibility == "collapse" ||
        style.contentVisibility == "hidden" ||
        parseFloat(style.opacity) === 0 ||
        this._clipHidesEverything(style)
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

    // Covered by a higher opaque layer (overlap hiding): sample a few points inside the
    // element and require at least one to hit the element or one of its descendants.
    if (!this._isOnTop(win, el, rect)) {
      return false;
    }
    return true;
  }

  /** The element's text colour is fully transparent, or equal to the colour painted
   * behind it (so the text, though present, cannot be seen). */
  _colorIsInvisible(win, el, style) {
    const fg = this._rgba(style.color);
    if (!fg) {
      return false;
    }
    if (fg.a === 0) {
      return true;
    }
    // The first non-transparent background colour up the ancestor chain.
    let node = el;
    let depth = 0;
    let bg = null;
    while (node && node.nodeType == node.ELEMENT_NODE && depth++ < 60) {
      const c = this._rgba(win.getComputedStyle(node).backgroundColor);
      if (c && c.a > 0) {
        bg = c;
        break;
      }
      node = node.parentElement || node.parentNode?.host;
    }
    if (!bg) {
      bg = { r: 255, g: 255, b: 255, a: 1 }; // the default canvas is white
    }
    return (
      Math.abs(fg.r - bg.r) <= 8 &&
      Math.abs(fg.g - bg.g) <= 8 &&
      Math.abs(fg.b - bg.b) <= 8
    );
  }

  _rgba(value) {
    const m = /rgba?\(([^)]+)\)/.exec(value || "");
    if (!m) {
      return null;
    }
    const parts = m[1].split(/[,/]/).map(s => parseFloat(s.trim()));
    return { r: parts[0], g: parts[1], b: parts[2], a: parts[3] === undefined ? 1 : parts[3] };
  }

  _clipHidesEverything(style) {
    // clip-path that collapses the box to nothing, or the legacy clip: rect(0,0,0,0).
    const cp = style.clipPath;
    if (cp && (cp.includes("inset(100%") || cp.includes("circle(0") || cp === "polygon(0px 0px)")) {
      return true;
    }
    const clip = style.clip;
    if (clip && /rect\(\s*0\w*[,\s]+0\w*[,\s]+0\w*[,\s]+0/.test(clip)) {
      return true;
    }
    return false;
  }

  _isOnTop(win, el, rect) {
    const xs = [0.5, 0.15, 0.85];
    const ys = [0.5, 0.15, 0.85];
    for (const fx of xs) {
      for (const fy of ys) {
        const x = rect.left + rect.width * fx;
        const y = rect.top + rect.height * fy;
        if (x < 0 || y < 0 || x > win.innerWidth || y > win.innerHeight) {
          continue;
        }
        const hit = el.ownerDocument.elementFromPoint(x, y);
        if (hit && (hit == el || el.contains(hit) || hit.contains(el))) {
          return true;
        }
      }
    }
    // No sample point reached the element (fully covered, or entirely off the viewport).
    return false;
  }
}
