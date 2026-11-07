/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The guard dialog for a program about to start (guard-dialog.mjs loads this for the
 * kinds "protocol" and "native-messaging"; WhisperProtocolGuard.sys.mjs opens it).
 *
 * Shows who asks (the exact site, or the add-on), the program's name and full path
 * (or that its path is unknown) and that Whisper cannot control what the program does.
 * Cancel has the focus; Open only works one second after the dialog appears, and again
 * one second after the window gets the focus back, so a click meant for the page cannot
 * land on it. "Always allow" stores the choice for that exact origin (or add-on). Every
 * value from outside is set as text or as a Fluent argument.
 */

const ENABLE_DELAY_MS = 1000;

export function renderProgramDialog({ args, info, el, button, $ }) {
  const nm = args.kind == "native-messaging";
  const text = v => String(v ?? "");
  document.l10n.setAttributes(
    $("guard-title"),
    nm ? "whisper-program-nm-title" : "whisper-program-title"
  );
  if (nm) {
    document.l10n.setAttributes($("guard-lead"), "whisper-program-nm-lead", {
      addon: text(info.addon),
      host: text(info.host),
    });
  } else {
    document.l10n.setAttributes(
      $("guard-lead"),
      info.program ? "whisper-program-lead" : "whisper-program-lead-unnamed",
      { origin: text(info.origin), scheme: text(info.scheme), program: text(info.program) }
    );
  }

  const body = $("guard-body");
  const path = el("div", null, null, "address");
  path.append(el("p", "whisper-program-path-label", null, "address-label"));
  const value = el(
    "p",
    info.path ? null : "whisper-program-path-unknown",
    null,
    "address-value"
  );
  if (info.path) {
    value.textContent = text(info.path);
  }
  value.setAttribute("dir", "ltr");
  path.append(value);
  body.append(path);
  if (nm && info.description) {
    body.append(
      el("p", "whisper-program-nm-description", { description: text(info.description) })
    );
  }
  body.append(el("p", nm ? "whisper-program-nm-warning" : "whisper-program-warning"));

  let remember = null;
  if (info.canRemember) {
    const label = el("label", null, null, "remember");
    remember = el("input");
    remember.type = "checkbox";
    label.append(
      remember,
      el(
        "span",
        nm ? "whisper-program-nm-remember" : "whisper-program-remember",
        nm ? { addon: text(info.addon) } : { origin: text(info.origin), scheme: text(info.scheme) }
      )
    );
    body.append(label);
  }

  const cancel = button("whisper-program-cancel", null, { primary: true });
  const open = el("button", nm ? "whisper-program-nm-allow" : "whisper-program-open");
  open.type = "button";
  open.addEventListener("click", () => {
    args.result = remember?.checked ? "open-always" : "open";
    window.close();
  });
  $("guard-actions").append(open);

  // Open is disabled for a second after the dialog appears or the window comes back.
  let timer = null;
  const hold = () => {
    open.disabled = true;
    clearTimeout(timer);
    timer = setTimeout(() => (open.disabled = false), ENABLE_DELAY_MS);
  };
  hold();
  window.addEventListener("focus", hold);
  window.addEventListener("blur", () => {
    clearTimeout(timer);
    open.disabled = true;
  });
  return cancel;
}
