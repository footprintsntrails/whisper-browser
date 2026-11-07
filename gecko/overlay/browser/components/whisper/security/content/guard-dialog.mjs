/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Fills in Whisper's guard dialog from window.arguments[0] = {kind, info, result}
 * (WhisperGuardDialog.sys.mjs) and writes the user's answer into `result`.
 *
 * Rules for every kind: the safe choice has the focus and Escape means it; words come
 * from whisper-security.ftl; values from outside (an address, a site, a program path)
 * are only set with textContent or as Fluent arguments; nowhere a "you're protected".
 */

import { HoldButton } from "chrome://browser/content/whisper/security/hold-button.mjs";

const args = window.arguments?.[0]?.wrappedJSObject ?? window.arguments?.[0] ?? {};
const info = args.info ?? {};
const $ = id => document.getElementById(id);

function el(tag, l10nId, l10nArgs, className) {
  const node = document.createElementNS("http://www.w3.org/1999/xhtml", tag);
  if (l10nId) {
    document.l10n.setAttributes(node, l10nId, l10nArgs);
  }
  if (className) {
    node.className = className;
  }
  return node;
}

function finish(result) {
  args.result = result;
  window.close();
}

function button(l10nId, result, { primary = false, l10nArgs } = {}) {
  const b = el("button", l10nId, l10nArgs, primary ? "primary" : "");
  b.type = "button";
  b.addEventListener("click", () => finish(result));
  $("guard-actions").append(b);
  return b;
}

/** The deliberate way past a warning: hold for 3 seconds. */
function holdButton(l10nId, result, hintId) {
  const b = el("button", null, null, "hold");
  b.type = "button";
  const track = el("span", null, null, "hold-track");
  track.append(el("span", null, null, "hold-progress"));
  b.append(el("span", l10nId, null, "hold-label"), track);
  const hint = el("p", hintId, null, "hold-hint");
  hint.id = "hold-hint";
  b.setAttribute("aria-describedby", "hold-hint");
  $("guard-actions").append(b);
  $("guard-body").append(hint);
  new HoldButton(b, {
    duration: 3000,
    onComplete: () => finish(result),
    onProgress: left => {
      const status = $("guard-status");
      if (left === null) {
        status.removeAttribute("data-l10n-id");
        status.textContent = "";
      } else {
        document.l10n.setAttributes(status, "whisper-hold-status", { seconds: left });
      }
    },
  });
  return b;
}

/** An address with the characters that differ marked (text nodes and <mark> only). */
function addressBlock(labelId, address, marks) {
  const wrap = el("div", null, null, "address");
  wrap.append(el("p", labelId, null, "address-label"));
  const code = el("p", null, null, "address-value");
  code.setAttribute("dir", "ltr");
  for (let i = 0; i < address.length; ) {
    let j = i;
    while (j < address.length && marks[j] == marks[i]) {
      j++;
    }
    const part = address.slice(i, j);
    if (marks[i]) {
      const m = el("mark");
      m.textContent = part;
      code.append(m);
    } else {
      code.append(document.createTextNode(part));
    }
    i = j;
  }
  wrap.append(code);
  return wrap;
}

function seed() {
  document.l10n.setAttributes($("guard-title"), "whisper-seed-title");
  document.l10n.setAttributes($("guard-lead"), "whisper-seed-lead");
  const body = $("guard-body");
  if (info.kind == "typed") {
    body.append(
      el("p", info.complete ? "whisper-seed-typed-complete" : "whisper-seed-typed-start", {
        count: info.count,
      })
    );
  } else {
    body.append(el("p", info.kind == "drop" ? "whisper-seed-dropped" : "whisper-seed-pasted"));
  }
  body.append(el("p", "whisper-seed-advice"));
  const back = button("whisper-seed-go-back", "back", { primary: true });
  holdButton(
    "whisper-seed-hold",
    "allow",
    info.kind == "typed" ? "whisper-seed-hold-hint-typed" : "whisper-seed-hold-hint-paste"
  );
  return back;
}

function address() {
  const { diffAddresses } = ChromeUtils.importESModule(
    "moz-src:///browser/components/whisper/security/CryptoAddress.sys.mjs"
  );
  const copied = String(info.copied ?? "");
  const pasted = String(info.pasted ?? "");
  const diff = diffAddresses(copied, pasted);
  document.l10n.setAttributes($("guard-title"), "whisper-address-title");
  document.l10n.setAttributes($("guard-lead"), "whisper-address-lead");
  $("guard-body").append(
    addressBlock("whisper-address-copied", copied, diff.a),
    addressBlock("whisper-address-pasted", pasted, diff.b),
    el("p", "whisper-address-difference", { count: diff.changed }),
    el("p", "whisper-address-scope", null, "note")
  );
  const safe = button("whisper-address-use-copied", "copied", { primary: true });
  button("whisper-address-use-pasted", "pasted");
  button("whisper-address-cancel", null);
  return safe;
}

async function main() {
  // Built at once (module scripts run after parsing, before the load event), so the
  // content is there when the dialog box first measures it.
  let focus = null;
  if (args.kind == "seed") {
    focus = seed();
  } else if (args.kind == "address") {
    focus = address();
  } else {
    const { renderProgramDialog } = await import(
      "chrome://browser/content/whisper/security/program-dialog.mjs"
    );
    focus = renderProgramDialog({ args, info, el, button, holdButton, $ });
  }
  await document.l10n.translateRoots();
  // SubDialog gives the frame window resizeDialog(): measure again now that the text
  // is translated.
  await window.resizeDialog?.();
  focus?.focus();
}

main().catch(e => console.error("Whisper guard dialog:", e));
