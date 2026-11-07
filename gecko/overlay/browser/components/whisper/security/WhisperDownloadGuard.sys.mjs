/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's download check (dl_guard): holds risky downloads until the user decides.
 *
 * It plugs into Firefox's own verdict step, the one Safe Browsing's remote check used
 * (Whisper turns that off): DownloadIntegration.shouldBlockForReputationCheck, through
 * Firefox's Integration overrides, so no Firefox file changes for the check itself.
 * When a file is held, Firefox keeps it as a ".part" file in the temporary folder and
 * shows it in the downloads panel with "Allow download" and "Remove file"; a held file
 * cannot be opened or started before the user allows it ("open when done" waits too),
 * and the mark of the web is written when it is allowed (patches/0062: always
 * ZoneId=3).
 *
 * What is held is decided by crates/dl_guard (run by WhisperGuardEngine): disc images,
 * shortcuts, scripts and app packages always; programs unless the user already allowed
 * one from that site in this session (in memory only); names that hide the real type;
 * content that does not match its name or the server's type; ZIP archives holding any
 * of these. The check reads the start and end of the file (and an archive's table of
 * contents) on this device; nothing is sent anywhere.
 *
 * If the engine fails, the file is held with "Whisper could not check this file": a
 * check that cannot run must not wave everything through silently.
 *
 * Save Link As and Save Page As write the file directly, with no ".part" file to hold,
 * so they are not checked; the mark of the web still applies.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  Downloads: "resource://gre/modules/Downloads.sys.mjs",
  Integration: "resource://gre/modules/Integration.sys.mjs",
  WhisperGuardEngine:
    "moz-src:///browser/components/whisper/security/WhisperGuardEngine.sys.mjs",
});
ChromeUtils.defineLazyGetter(
  lazy,
  "ORIGIN",
  () =>
    ChromeUtils.importESModule(
      "moz-src:///browser/components/whisper/security/WhisperGuardEngine.sys.mjs"
    ).ORIGIN
);
ChromeUtils.defineLazyGetter(
  lazy,
  "l10n",
  () => new Localization(["branding/brand.ftl", "whisper/whisper-security.ftl"], true)
);

const ENABLED_PREF = "whisper.security.downloads.enabled";
const PROGRAMS_PREF = "whisper.security.downloads.hold_programs";
export const VERDICT_PREFIX = "WhisperHeld:";
// An archive's table of contents larger than this is not read.
const MAX_EXTRA_BYTES = 4 * 1024 * 1024;

// Sites the user allowed a held program from, this session only.
const gAllowedSites = new Set();
// The reasons of a held download, this session (after a restart only its code is left).
const gReasons = new WeakMap();

function siteOf(url) {
  try {
    let uri = Services.io.newURI(url);
    if (uri.schemeIs("blob")) {
      uri = Services.io.newURI(new URL(url).origin);
    }
    return Services.eTLD.getSchemelessSite(uri);
  } catch (e) {
    return null;
  }
}

function originOf(download) {
  const ORIGIN = lazy.ORIGIN;
  let scheme = "";
  try {
    scheme = Services.io.newURI(download.source.url).scheme;
  } catch (e) {}
  const referrer = download.source.referrerInfo?.originalReferrer;
  if (scheme == "file" || referrer?.schemeIs("file")) {
    return ORIGIN.LOCAL;
  }
  if (scheme == "blob" || scheme == "data") {
    return ORIGIN.MADE_BY_PAGE;
  }
  return scheme == "https" ? ORIGIN.HTTPS : ORIGIN.INSECURE;
}

function urlName(url) {
  try {
    const path = new URL(url).pathname;
    return decodeURIComponent(path.slice(path.lastIndexOf("/") + 1)).slice(0, 300);
  } catch (e) {
    return "";
  }
}

async function readRanges(path, ranges, size, budget) {
  const chunks = [];
  for (const [offset, length] of ranges) {
    const len = Math.min(length, Math.max(0, size - offset));
    if (len <= 0 || budget.left < len) {
      continue;
    }
    budget.left -= len;
    chunks.push([offset, await IOUtils.read(path, { offset, maxBytes: len })]);
  }
  return chunks;
}

export const WhisperDownloadGuard = {
  VERDICT_PREFIX,

  init() {
    lazy.Integration.downloads.register(base => ({
      async shouldBlockForReputationCheck(download) {
        const held = await WhisperDownloadGuard.check(download);
        if (held) {
          return held;
        }
        return base.shouldBlockForReputationCheck.call(this, download);
      },
    }));
    this._watchUnblocks().catch(e => console.error("Whisper download check:", e));
  },

  /** Remembers the site of a program the user allowed, for this session. */
  async _watchUnblocks() {
    const list = await lazy.Downloads.getList(lazy.Downloads.ALL);
    await list.addView({
      onDownloadChanged(download) {
        const verdict = download.error?.reputationCheckVerdict ?? "";
        if (download.succeeded && verdict == `${VERDICT_PREFIX}program`) {
          const site = siteOf(download.source.url);
          if (site) {
            gAllowedSites.add(site);
          }
        }
      },
    });
  },

  /** For Panic and Burn. */
  forgetSession() {
    gAllowedSites.clear();
  },

  /**
   * @returns {Promise<null | {shouldBlock: true, verdict: string}>}
   */
  async check(download) {
    if (!Services.prefs.getBoolPref(ENABLED_PREF, true)) {
      return null;
    }
    const path = download.target.partFilePath;
    if (!path) {
      // Nothing to hold without a ".part" file (Save Link As writes in place).
      return null;
    }
    try {
      const { size } = await IOUtils.stat(path);
      const site = siteOf(download.source.url);
      const engine = lazy.WhisperGuardEngine;
      const request = {
        fileName: PathUtils.filename(download.target.path),
        urlName: urlName(download.source.url),
        mime: download.contentType ?? "",
        size,
        origin: originOf(download),
        siteAllowed:
          !Services.prefs.getBoolPref(PROGRAMS_PREF, true) ||
          (!!site && gAllowedSites.has(site)),
        chunks: [],
      };
      const budget = { left: 1024 * 1024 + MAX_EXTRA_BYTES };
      let ranges = await engine.dlFirstRanges(size);
      let answer = null;
      for (let round = 0; round < 4; round++) {
        request.chunks.push(...(await readRanges(path, ranges, size, budget)));
        answer = await engine.dlInspect(request);
        if (answer.verdict != "need") {
          break;
        }
        ranges = answer.ranges;
      }
      if (answer?.verdict != "hold") {
        return null;
      }
      const reasons = answer.reasons ?? [];
      gReasons.set(download, { reasons, kind: answer.kind });
      return { shouldBlock: true, verdict: VERDICT_PREFIX + (reasons[0]?.code ?? answer.kind) };
    } catch (e) {
      console.error("Whisper download check failed:", e.message);
      gReasons.set(download, { reasons: [{ code: "unchecked" }], kind: "other" });
      return { shouldBlock: true, verdict: `${VERDICT_PREFIX}unchecked` };
    }
  },

  /** Is this a verdict of ours? */
  isHeld(verdict) {
    return typeof verdict == "string" && verdict.startsWith(VERDICT_PREFIX);
  },

  /**
   * [title, [details, tip]] for the downloads panel and about:downloads
   * (patches/0063). Plain strings.
   */
  describe(download) {
    const verdict = download.error?.reputationCheckVerdict ?? "";
    const code = verdict.slice(VERDICT_PREFIX.length);
    const reasons = gReasons.get(download)?.reasons ?? [{ code }];
    const l10n = lazy.l10n;
    const say = reason =>
      l10n.formatValueSync(`whisper-download-reason-${reason.code}`, {
        ext: reason.ext ? `.${reason.ext}` : "",
        shown: reason.shown ? `.${reason.shown}` : "",
        real: reason.real ? `.${reason.real}` : "",
        claimed: reason.claimed ?? "",
        found: reason.found ?? "",
        entry: reason.entry ?? "",
      });
    // The main reason, and what an archive or disc image holds if that is another.
    const inside = reasons.find(r => r.entry);
    let detail = say(reasons[0]) || l10n.formatValueSync("whisper-download-reason-unchecked");
    if (inside && inside != reasons[0]) {
      detail += " " + say(inside);
    }
    return [
      l10n.formatValueSync("whisper-download-held-title"),
      [detail, l10n.formatValueSync("whisper-download-held-tip")],
    ];
  },

  /** The text of Firefox's "Are you sure?" dialog when a held file is allowed. */
  unblockMessage() {
    return lazy.l10n.formatValueSync("whisper-download-unblock-message");
  },
};
