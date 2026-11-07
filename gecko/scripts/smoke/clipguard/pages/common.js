/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/* Helpers shared by the ClickFix smoke-test pages. Each page reports what the clipboard
 * APIs returned to it, on window.__clipguardResult, so the test can check that the page
 * saw its write "succeed" whatever Whisper did behind it. No page ever runs a command;
 * the strings are inert and the addresses are *.invalid or documentation RFC 5737 IPs. */

window.__clipguardResult = { done: false, api: null, ok: null, error: null, read: null };

function report(api, ok, error) {
  window.__clipguardResult = {
    done: true,
    api,
    ok,
    error: error ? String(error) : null,
    read: window.__clipguardResult.read || null,
  };
}

// navigator.clipboard.writeText, reporting what the promise did.
async function writeText(text) {
  try {
    await navigator.clipboard.writeText(text);
    report("writeText", true, null);
  } catch (e) {
    report("writeText", false, e);
  }
}

// A ClipboardItem with a deferred text promise.
async function writeItem(text) {
  try {
    const item = new ClipboardItem({
      "text/plain": Promise.resolve(new Blob([text], { type: "text/plain" })),
    });
    await navigator.clipboard.write([item]);
    report("write", true, null);
  } catch (e) {
    report("write", false, e);
  }
}

// document.execCommand("copy") from a hidden textarea.
function execCopy(text) {
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.left = "-9999px";
  ta.setAttribute("aria-hidden", "true");
  document.body.appendChild(ta);
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
    report("execCommand", ok, null);
  } catch (e) {
    report("execCommand", false, e);
  } finally {
    ta.remove();
  }
  return ok;
}

// Install a copy handler that replaces whatever the person selected.
function installCopyHandler(text) {
  document.addEventListener(
    "copy",
    e => {
      e.preventDefault();
      e.clipboardData.setData("text/plain", text);
      report("copy-handler", true, null);
    },
    { once: true }
  );
}

// Read the clipboard back (the read-back probe): the page must not get its own command.
async function readBack() {
  try {
    const got = await navigator.clipboard.readText();
    window.__clipguardResult.read = got;
    return got;
  } catch (e) {
    window.__clipguardResult.read = "<error: " + e + ">";
    return null;
  }
}

window.__clipguard = { writeText, writeItem, execCopy, installCopyHandler, readBack, report };
