/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Links from web pages that open another program (external protocols such as
 * `zoommtg:` or `search-ms:`), and add-ons that start a program (native messaging).
 *
 * Called first by Firefox's ContentDispatchChooser.handleURI (patches/0064) for every
 * external link a page opens. Firefox's own block list (network.protocol-handler.
 * external.*) still stops its schemes earlier, in C++; this adds the rest:
 *   - blocked: the schemes crates/proto_guard names (every `ms-*` handler, `search`,
 *     help viewers, file sharing, script runners, ...). Nothing happens.
 *   - a scheme no program on this computer handles: nothing happens either. And with
 *     no click on the page (no user activation), nothing happens for any scheme. So a
 *     page that tries many schemes to learn which programs are installed ("scheme
 *     flooding") gets the same response for a blocked scheme, an unknown one and an
 *     installed one; with a click it can test one scheme, and the user sees the dialog.
 *   - otherwise a dialog over the tab: the exact site, the scheme, the program's name
 *     and full path, "Whisper can't control what the program does next"; Cancel has the
 *     focus and Open works after a second. "Always allow" is stored for that exact
 *     origin (scheme, host and port), not for the whole site.
 * Links the user opens himself (typed, or from another program) keep Firefox's own
 * flow. whisper.security.protocols.enabled off gives Firefox's behaviour back.
 *
 * Native messaging (patches/0065): before Firefox starts the program of an add-on's
 * native messaging host, a dialog names the add-on, the host and the program's full
 * path; "Always allow" is stored for that add-on and host.
 */

import { AppConstants } from "resource://gre/modules/AppConstants.sys.mjs";

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  WhisperGuardDialog:
    "moz-src:///browser/components/whisper/security/WhisperGuardDialog.sys.mjs",
  WhisperGuardEngine:
    "moz-src:///browser/components/whisper/security/WhisperGuardEngine.sys.mjs",
  WindowsRegistry: "resource://gre/modules/WindowsRegistry.sys.mjs",
});

const ENABLED_PREF = "whisper.security.protocols.enabled";
const ALLOWED_MS_PREF = "whisper.security.protocols.allowed_ms_schemes";
const NM_PREF = "whisper.security.nativemessaging.ask";
const PROTOCOL_PERMISSION = "whisper-open-program^";
const NM_PERMISSION = "whisper-native-messaging^";

// One question at a time for the same thing.
const gPending = new Map();

function once(key, fn) {
  if (!gPending.has(key)) {
    gPending.set(
      key,
      fn().finally(() => gPending.delete(key))
    );
  }
  return gPending.get(key);
}

/** What to show for an origin: scheme, host and port, never a path. */
function originText(principal) {
  try {
    return principal.originNoSuffix || principal.origin;
  } catch (e) {
    return "";
  }
}

/**
 * The program that opens a scheme: {name, path}. `path` is null when it cannot be
 * found (the dialog says so).
 */
async function programFor(handler, scheme) {
  const preferred = handler.preferredApplicationHandler;
  if (
    handler.preferredAction == Ci.nsIHandlerInfo.useHelperApp &&
    preferred instanceof Ci.nsILocalHandlerApp
  ) {
    return { name: preferred.name, path: preferred.executable?.path ?? null };
  }
  let name = handler.defaultDescription || null;
  let path = null;
  let command = "";
  try {
    if (AppConstants.platform == "win") {
      const shell = Cc["@mozilla.org/browser/shell-service;1"].getService(
        Ci.nsIWindowsShellService
      );
      const progId = shell.queryCurrentDefaultHandlerFor(scheme);
      if (progId) {
        command =
          lazy.WindowsRegistry.readRegKey(
            Ci.nsIWindowsRegKey.ROOT_KEY_CLASSES_ROOT,
            `${progId}\\shell\\open\\command`,
            ""
          ) ?? "";
        if (command) {
          // As Windows reads it (quotes, spaces, environment variables).
          const file = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsILocalFileWin);
          file.initWithCommandLine(command);
          path = file.path;
        }
      }
    } else if ("@mozilla.org/gio-service;1" in Cc) {
      const app = Cc["@mozilla.org/gio-service;1"]
        .getService(Ci.nsIGIOService)
        .getAppForURIScheme(scheme);
      name ||= app?.name ?? null;
      command = app?.command ?? "";
    }
  } catch (e) {
    // Not found: shown as unknown.
  }
  if (!path && command) {
    try {
      path = (await lazy.WhisperGuardEngine.proto(scheme, { command })).program;
    } catch (e) {}
  }
  return { name, path };
}

export const WhisperProtocolGuard = {
  /**
   * Returns true when Whisper handled the link (blocked, asked, opened or let it
   * drop), false to let Firefox go on as usual.
   */
  async handleURI(
    handler,
    uri,
    principal,
    browsingContext,
    triggeredExternally,
    hasUserActivation
  ) {
    if (!Services.prefs.getBoolPref(ENABLED_PREF, true)) {
      return false;
    }
    // Typed by the user, or passed in by another program: Firefox's own flow.
    if (triggeredExternally || !principal || principal.isSystemPrincipal) {
      return false;
    }
    let tier = "ask";
    let scheme = uri.scheme;
    try {
      const answer = await lazy.WhisperGuardEngine.proto(uri.scheme, {
        allowedMs: Services.prefs.getStringPref(ALLOWED_MS_PREF, ""),
      });
      tier = answer.tier;
      scheme = answer.scheme ?? scheme;
    } catch (e) {
      // Without the engine every scheme still needs the dialog below.
      console.error("Whisper external programs:", e.message);
    }
    if (tier == "block") {
      return true;
    }
    const hasProgram =
      handler.hasDefaultHandler ||
      handler.preferredApplicationHandler instanceof Ci.nsILocalHandlerApp;
    if (!hasProgram || !hasUserActivation) {
      // The same nothing as for a blocked scheme (see the comment at the top).
      return true;
    }
    const permission = PROTOCOL_PERMISSION + scheme;
    if (
      Services.perms.testExactPermissionFromPrincipal(principal, permission) ==
      Services.perms.ALLOW_ACTION
    ) {
      handler.launchWithURI(uri, browsingContext);
      return true;
    }
    const origin = originText(principal);
    const result = await once(`${origin} ${scheme}`, async () => {
      const program = await programFor(handler, scheme);
      return lazy.WhisperGuardDialog.open(browsingContext, "protocol", {
        origin,
        scheme,
        program: program.name,
        path: program.path,
        canRemember: true,
      });
    });
    if (result == "open-always") {
      // In private windows the permission manager keeps it for the session only.
      Services.perms.addFromPrincipal(
        principal,
        permission,
        Services.perms.ALLOW_ACTION,
        Services.perms.EXPIRE_NEVER
      );
    }
    if (result == "open" || result == "open-always") {
      handler.launchWithURI(uri, browsingContext);
    }
    return true;
  },

  /**
   * Before an add-on's native messaging host starts (patches/0065). Throws when the
   * user does not allow it, and the program is not started.
   *
   * @param {object} context the add-on's ProxyContextParent
   * @param {string} application the host's name
   * @param {{path: string, manifest: object}} hostInfo its manifest
   * @param {string} command the program's full path
   */
  async checkNativeMessaging(context, application, hostInfo, command) {
    if (!Services.prefs.getBoolPref(NM_PREF, true)) {
      return;
    }
    const extension = context.extension;
    const principal = extension.principal;
    const permission = NM_PERMISSION + application;
    if (
      Services.perms.testExactPermissionFromPrincipal(principal, permission) ==
      Services.perms.ALLOW_ACTION
    ) {
      return;
    }
    const result = await once(`${extension.id} ${application}`, () =>
      lazy.WhisperGuardDialog.open(null, "native-messaging", {
        addon: extension.name,
        addonId: extension.id,
        host: application,
        description: String(hostInfo.manifest?.description ?? "").slice(0, 200),
        path: command,
        manifest: hostInfo.path,
        canRemember: true,
      })
    );
    if (result == "open-always") {
      Services.perms.addFromPrincipal(
        principal,
        permission,
        Services.perms.ALLOW_ACTION,
        Services.perms.EXPIRE_NEVER
      );
    }
    if (result != "open" && result != "open-always") {
      throw new Error(`The user did not allow ${application} to start`);
    }
  },
};
