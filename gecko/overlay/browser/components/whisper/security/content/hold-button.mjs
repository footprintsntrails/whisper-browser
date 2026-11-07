/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * A button that acts only after it has been held for a while (3 seconds): the
 * deliberate way past a warning, hard to hit by accident or by a page's trick.
 *
 * Works with a mouse, touch or pen (press and hold) and with the keyboard (hold Space
 * or Enter while the button has focus). Letting go early, moving the pointer off the
 * button, or the window losing focus starts over. Progress is a bar that only scales
 * (transform, in CSS: the "holding" class), and the status line counts down for screen
 * readers; with reduced motion the bar does not move and the countdown is the progress.
 *
 * The page behind a tab-modal dialog cannot send events into it, and synthetic events
 * are ignored here (isTrusted).
 */

export class HoldButton {
  /**
   * @param {HTMLButtonElement} button holds a child .hold-progress element
   * @param {object} options
   * @param {number} options.duration milliseconds to hold
   * @param {Function} options.onComplete called once, after a full hold
   * @param {Function} options.onProgress called with seconds left (for the status line)
   */
  constructor(button, { duration = 3000, onComplete, onProgress }) {
    this.button = button;
    this.win = button.ownerDocument.defaultView;
    this.duration = duration;
    this.onComplete = onComplete;
    this.onProgress = onProgress ?? (() => {});
    this.started = null;
    this.timer = null;
    this.ticker = null;
    this.done = false;

    const start = e => {
      if (!e.isTrusted || this.done) {
        return;
      }
      if (e.type == "keydown") {
        if ((e.key != " " && e.key != "Enter") || e.repeat) {
          if (e.key == " " || e.key == "Enter") {
            e.preventDefault();
          }
          return;
        }
        e.preventDefault();
      } else {
        if (e.button != 0) {
          return;
        }
        if (e.type == "pointerdown") {
          button.setPointerCapture(e.pointerId);
        }
      }
      this.#start();
    };
    const stop = e => {
      if (e.type == "keyup" && e.key != " " && e.key != "Enter") {
        return;
      }
      this.#stop();
    };
    // Mouse events as well as pointer events: whichever arrives first starts the hold
    // (starting twice is a no-op), and any release or cancel stops it.
    for (const type of ["pointerdown", "mousedown", "keydown"]) {
      button.addEventListener(type, start);
    }
    for (const type of ["pointerup", "mouseup", "pointercancel", "lostpointercapture", "keyup", "blur"]) {
      button.addEventListener(type, stop);
    }
    this.win.addEventListener("blur", () => this.#stop());
    // A plain click or Enter press does nothing by itself.
    button.addEventListener("click", e => e.preventDefault());
  }

  #start() {
    if (this.started !== null) {
      return;
    }
    this.started = Date.now();
    this.button.classList.add("holding");
    this.onProgress(Math.ceil(this.duration / 1000));
    this.ticker = this.win.setInterval(() => {
      this.onProgress(Math.max(1, Math.ceil((this.duration - (Date.now() - this.started)) / 1000)));
    }, 1000);
    this.timer = this.win.setTimeout(() => this.#complete(), this.duration);
  }

  #stop() {
    if (this.started === null || this.done) {
      return;
    }
    // Timers can run late in a window the system considers in the background; what
    // counts is how long the button was really held.
    if (Date.now() - this.started >= this.duration) {
      this.#complete();
      return;
    }
    this.started = null;
    this.button.classList.remove("holding");
    this.win.clearTimeout(this.timer);
    this.win.clearInterval(this.ticker);
    this.onProgress(null);
  }

  #complete() {
    this.done = true;
    this.win.clearInterval(this.ticker);
    this.button.classList.remove("holding");
    this.button.classList.add("held");
    this.onProgress(0);
    this.onComplete();
  }
}
