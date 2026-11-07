#!/usr/bin/env python3
"""Whisper's end-to-end update test: Firefox's own updater, Whisper's keys, real builds.

Two builds of the same source are needed: A (the package people download) and B, the
same source built again later, so B has a newer build ID. The test installs A, lets
its update service find B on a local update server, download it, check it and stage
it, presses "Restart to update" and checks that B is running; then it tries the
updates that must be refused.

The real signing key never comes near this test. A throwaway key pair is made for
each run, and the updater of the installed copy is changed to trust the throwaway
certificates instead of Whisper's: the certificates are compiled into the updater as
plain bytes, so the test swaps Whisper's two certificates for throwaway ones of the
same length (`patch`). Nothing a build produces ever contains a throwaway
certificate; only the copies this test installs do.

  prepare  (Linux) unpack A and B, make the throwaway certificates, change B's updater,
           make and sign the MARs, write update.xml for each case.
             update_test.py prepare --a PKG_A --b PKG_B --bundle DIR [--port 8111]
             environment: MAR_TOOLS and NSS_LIBS, as for make-mar.sh and sign-mar.sh
           The private keys exist only while prepare runs; the bundle holds
           certificates, MARs and update.xml files only.
  patch    change the updater of an installed or unpacked A to trust the bundle's
           throwaway certificates.
             update_test.py patch --bundle DIR --app APP_DIR
  run      start the local update server and go through the cases.
             update_test.py run --bundle DIR --app APP_DIR --work DIR --out DIR [--cases a,b]

Every case has a fresh profile, its own Marionette port and its own browser: "update"
runs in APP_DIR itself (the installed A); no-downgrade and replay-refused run on a copy
of what "update" left there (B), or, if it left no B, on B unpacked from its MAR; every
other case runs on a copy of A. Whatever is left of a case's browser is ended before
the next case starts, and a failed case never stops the others.
  update          A finds B (signed with the primary key), downloads, verifies, stages
                  it; "Restart to update" -> B runs.
  no-downgrade    B is offered A again (older build ID): not offered, not downloaded.
  replay-refused  B is offered A's validly signed MAR under a newer build ID: the
                  updater refuses it (VERSION_DOWNGRADE_ERROR, 23); B keeps running.
  rotation-key    B signed with the secondary (rotation) key: verified and staged.
  untrusted-key   B signed with a key the updater does not trust: refused (19).
  tampered        one byte of a signed MAR changed, update.xml hash fits the changed
                  file: the signature check refuses it (19).
  tampered-hash   the same changed MAR under the size and hash of the real one. Firefox
                  159 checks only the size of a download (Downloader._verifyDownload in
                  UpdateService.sys.mjs), not the hash update.xml gives, so it is the
                  updater's signature check that refuses it (19). A refusal of the
                  download itself is accepted too; the report says which one happened.
  wrong-channel   a MAR for another MAR channel (Firefox's), signed with the trusted
                  key: refused (MAR_CHANNEL_MISMATCH_ERROR, 22).
  unsigned        a MAR without a signature: refused (19).
With a Whisper Canary build (prepare --canary-a, run --canary-app):
  canary-update          the canary finds its next build (the same files under a newer
                         build ID, set_build_id.py), signed with the canary key: applied.
  canary-mar-on-release  Whisper is offered that canary MAR: refused (19), Whisper
                         trusts only its own two certificates.
  release-mar-on-canary  the canary is offered Whisper's B (primary key): refused (19),
                         the canary trusts only the canary certificate.
"""

import argparse
import datetime
import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "..", "smoke"))
from marionette import Marionette, MarionetteError  # noqa: E402
from updatelib import (GECKO, MarError, app_ini, extract_mar, product_version, read_env,  # noqa: E402
                       read_info, sha512, update_xml)
from set_build_id import set_build_id  # noqa: E402

CERTS = os.path.join(GECKO, "overlay", "toolkit", "mozapps", "update", "updater")
WINDOWS = platform.system() == "Windows"
CERT_VERIFY_ERROR = 19
MAR_CHANNEL_MISMATCH_ERROR = 22
VERSION_DOWNGRADE_ERROR = 23

ALL_CASES = ["update", "no-downgrade", "replay-refused", "rotation-key", "untrusted-key", "tampered",
             "tampered-hash", "wrong-channel", "unsigned", "canary-update", "canary-mar-on-release",
             "release-mar-on-canary"]
# Cases that run on the canary (the installed canary A, or a copy of it).
CANARY = ["canary-update", "release-mar-on-canary"]
# Cases that run on B (the browser the update case updates to), and on a copy of A.
FROM_B = ["no-downgrade", "replay-refused"]
FROM_A = ["rotation-key", "untrusted-key", "tampered", "tampered-hash", "wrong-channel", "unsigned"]
# Each case gets its own Marionette port, from this one up.
MARIONETTE_BASE_PORT = 2830


def log(*args):
    print(*args, flush=True)


def run_cmd(cmd, env=None):
    log("+", " ".join(cmd))
    subprocess.run(cmd, check=True, env=env)


# --- packages and certificates ------------------------------------------------------

def unpack(package, dest):
    os.makedirs(dest)
    if package.endswith(".tar.xz"):
        with tarfile.open(package) as t:
            t.extractall(dest, filter="data")
    elif package.endswith(".zip"):
        with zipfile.ZipFile(package) as z:
            z.extractall(dest)
    else:
        raise SystemExit(f"{package}: not a .tar.xz or .zip")
    (top,) = os.listdir(dest)
    return os.path.join(dest, top)


def updater_path(app):
    for name in ("updater.exe", "updater"):
        if os.path.isfile(os.path.join(app, name)):
            return os.path.join(app, name)
    raise SystemExit(f"no updater in {app}")


def make_cert(keys, name, cn, length=None):
    """A throwaway RSA-4096 key and self-signed certificate (DER). With `length`, the
    certificate is made exactly that many bytes long, so it can take the place of a
    real one inside the updater."""
    key = os.path.join(keys, f"{name}.key")
    der = os.path.join(keys, f"{name}.der")
    run_cmd(["openssl", "genpkey", "-quiet", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:4096", "-out", key])
    # Each character of the name adds two bytes (subject and issuer), each byte of the
    # serial number one, so some combination hits the length exactly.
    for pad in range(0, 40):
        for serial_bytes in (20, 19, 18, 17):
            subject = f"/CN={cn}{'.' * pad}/O=Whisper"
            serial = "0x4" + os.urandom(serial_bytes).hex()[1:]
            subprocess.run(["openssl", "req", "-x509", "-new", "-key", key, "-sha384", "-days", "3650",
                            "-subj", subject, "-set_serial", serial, "-outform", "DER", "-out", der],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if length is None or os.path.getsize(der) == length:
                return key, der
    raise SystemExit(f"could not make a {length}-byte certificate for {name}")


def patch_updater(path, swaps, times=(1,)):
    """Replaces each real certificate inside the updater binary with a test one of the
    same length. Each real certificate must be there a number of times in `times` (the
    canary's updater holds its one certificate as primary and secondary: twice, or once
    if the linker merged the identical copies)."""
    data = open(path, "rb").read()
    for real, test in swaps:
        if len(real) != len(test):
            raise SystemExit("certificates differ in length")
        found = data.count(real)
        if found not in times:
            raise SystemExit(f"{path}: Whisper's certificate is in the updater {found} times, expected {times}")
        data = data.replace(real, test)
    with open(path, "wb") as f:
        f.write(data)
    log(f"patched {path}: {len(swaps)} certificates swapped for throwaway ones")


def real_certs():
    return {w: open(os.path.join(CERTS, f"whisper_{w}.der"), "rb").read()
            for w in ("primary", "secondary", "canary")}


# --- prepare ------------------------------------------------------------------------

def prepare(a):
    release = read_env("release.env")
    bundle = os.path.abspath(a.bundle)
    if os.path.exists(bundle):
        raise SystemExit(f"{bundle} exists")
    for d in ("certs", "srv/files"):
        os.makedirs(os.path.join(bundle, d))
    work = tempfile.mkdtemp(prefix="whisper-update-prep-")
    keys = tempfile.mkdtemp(prefix="whisper-update-keys-")
    os.chmod(keys, 0o700)
    try:
        app_a = unpack(a.a, os.path.join(work, "a"))
        app_b = unpack(a.b, os.path.join(work, "b"))
        version, build_a = app_ini(os.path.join(app_a, "application.ini"))
        version_b, build_b = app_ini(os.path.join(app_b, "application.ini"))
        if version != version_b or not build_b > build_a:
            raise SystemExit(f"B ({version_b}, {build_b}) must be the same version as A ({version}, {build_a}) "
                             "with a newer build ID")
        log(f"A: Firefox {version}, build {build_a}\nB: Firefox {version_b}, build {build_b}")

        real = real_certs()
        test = {}
        signing = {}
        for which in ("primary", "secondary"):
            key, der = make_cert(keys, f"test-{which}", f"Whisper MAR test ({which})", len(real[which]))
            signing[which] = (key, der)
            test[which] = open(der, "rb").read()
            shutil.copy(der, os.path.join(bundle, "certs", f"test-{which}.der"))
        signing["untrusted"] = make_cert(keys, "untrusted", "Whisper MAR untrusted test key")
        for which in ("primary", "secondary"):
            shutil.copy(os.path.join(CERTS, f"whisper_{which}.der"), os.path.join(bundle, "certs", f"whisper-{which}.der"))
        swaps = [(real[w], test[w]) for w in ("primary", "secondary")]
        # B's updater trusts the throwaway certificates too, so the copy that was updated
        # to B can be offered further (refused) updates.
        patch_updater(updater_path(app_b), swaps)
        patch_updater(updater_path(app_a), swaps)

        mars = os.path.join(work, "mars")
        os.makedirs(mars)
        make_mar = os.path.join(HERE, "make-mar.sh")
        sign_mar = os.path.join(HERE, "sign-mar.sh")
        b_unsigned = os.path.join(mars, "b-unsigned.mar")
        a_unsigned = os.path.join(mars, "a-unsigned.mar")
        run_cmd([make_mar, app_b, b_unsigned])
        run_cmd([make_mar, app_a, a_unsigned])
        wrong = os.path.join(mars, "b-wrong-channel-unsigned.mar")
        shutil.copy(b_unsigned, wrong)
        run_cmd([os.path.join(os.environ["MAR_TOOLS"], "mar"), "-H", "firefox-mozilla-release",
                 "-V", product_version(version, build_b), "-i", wrong])
        files = os.path.join(bundle, "srv", "files")
        run_cmd([sign_mar, *signing["primary"],
                 b_unsigned, os.path.join(files, "b-primary.mar"),
                 wrong, os.path.join(files, "b-wrong-channel.mar"),
                 a_unsigned, os.path.join(files, "a-replay.mar")])
        run_cmd([sign_mar, *signing["secondary"], b_unsigned, os.path.join(files, "b-secondary.mar")])
        run_cmd([sign_mar, *signing["untrusted"], b_unsigned, os.path.join(files, "b-untrusted.mar")])
        shutil.copy(b_unsigned, os.path.join(files, "b-unsigned.mar"))
        tampered = os.path.join(files, "b-tampered.mar")
        shutil.copy(os.path.join(files, "b-primary.mar"), tampered)
        info = read_info(tampered)
        offset = info["content_offset"] + (info["index_offset"] - info["content_offset"]) // 2
        with open(tampered, "r+b") as f:
            f.seek(offset)
            byte = f.read(1)
            f.seek(offset)
            f.write(bytes([byte[0] ^ 0x01]))
        log(f"b-tampered.mar: byte {offset} of {info['size']} flipped (inside the packed files)")

        if a.canary_a:
            # Whisper Canary: A as built, its next build (the same files under a newer
            # build ID, as the quick canary path makes them), and a throwaway canary key.
            app_c = unpack(a.canary_a, os.path.join(work, "c"))
            version_c, build_c = app_ini(os.path.join(app_c, "application.ini"))
            key, der = make_cert(keys, "test-canary", "Whisper MAR test (canary)", len(real["canary"]))
            shutil.copy(der, os.path.join(bundle, "certs", "test-canary.der"))
            patch_updater(updater_path(app_c), [(real["canary"], open(der, "rb").read())], times=(1, 2))
            later = [datetime.datetime.strptime(x, "%Y%m%d%H%M%S") + datetime.timedelta(minutes=1)
                     for x in (build_c, build_b)]
            now = datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None, microsecond=0)
            build_cb = max([now] + later).strftime("%Y%m%d%H%M%S")
            app_cb = os.path.join(work, "cb")
            shutil.copytree(app_c, app_cb, symlinks=True)
            set_build_id(app_cb, build_cb)
            c_unsigned = os.path.join(mars, "c-unsigned.mar")
            run_cmd([make_mar, app_cb, c_unsigned, release["CANARY_MAR_CHANNEL_ID"]])
            run_cmd([sign_mar, key, der, c_unsigned, os.path.join(files, "c-canary.mar")])
            log(f"canary A: Firefox {version_c}, build {build_c}; canary B: build {build_cb}")
    finally:
        shutil.rmtree(keys, ignore_errors=True)
        shutil.rmtree(work, ignore_errors=True)

    newer = (datetime.datetime.strptime(build_b, "%Y%m%d%H%M%S") + datetime.timedelta(minutes=1)).strftime(
        "%Y%m%d%H%M%S")
    base = f"http://127.0.0.1:{a.port}"
    cases = {
        "update": {"mar": "b-primary.mar", "build": build_b, "expect": "applied", "then": build_b},
        "no-downgrade": {"mar": "a-replay.mar", "build": build_a, "expect": "not-offered", "then": build_b},
        "replay-refused": {"mar": "a-replay.mar", "build": newer, "expect": VERSION_DOWNGRADE_ERROR,
                           "then": build_b},
        "rotation-key": {"mar": "b-secondary.mar", "build": build_b, "expect": "applied", "then": build_b},
        "untrusted-key": {"mar": "b-untrusted.mar", "build": build_b, "expect": CERT_VERIFY_ERROR, "then": build_a},
        "tampered": {"mar": "b-tampered.mar", "build": build_b, "expect": CERT_VERIFY_ERROR, "then": build_a},
        "tampered-hash": {"mar": "b-tampered.mar", "hash_of": "b-primary.mar", "build": build_b,
                          "expect": "refused", "then": build_a},
        "wrong-channel": {"mar": "b-wrong-channel.mar", "build": build_b, "expect": MAR_CHANNEL_MISMATCH_ERROR,
                          "then": build_a},
        "unsigned": {"mar": "b-unsigned.mar", "build": build_b, "expect": CERT_VERIFY_ERROR, "then": build_a},
    }
    if a.canary_a:
        newest = (datetime.datetime.strptime(build_cb, "%Y%m%d%H%M%S") + datetime.timedelta(minutes=1)).strftime(
            "%Y%m%d%H%M%S")
        cases.update({
            "canary-update": {"mar": "c-canary.mar", "build": build_cb, "expect": "applied", "then": build_cb,
                              "app": "canary"},
            "canary-mar-on-release": {"mar": "c-canary.mar", "build": build_cb, "expect": CERT_VERIFY_ERROR,
                                      "then": build_a},
            "release-mar-on-canary": {"mar": "b-primary.mar", "build": newest, "expect": CERT_VERIFY_ERROR,
                                      "then": build_c, "app": "canary"},
        })
    for name, c in cases.items():
        served = os.path.join(files, c["mar"])
        described = os.path.join(files, c.get("hash_of", c["mar"]))
        xml = update_xml(version, c["build"], release["WHISPER_VERSION"], f"{base}/download/{c['mar']}",
                         os.path.getsize(described), sha512(described))
        os.makedirs(os.path.join(bundle, "srv", name))
        with open(os.path.join(bundle, "srv", name, "update.xml"), "w", encoding="utf-8") as f:
            f.write(xml)
        c["url"] = f"{base}/{name}/update.xml"
        c["mar_info"] = read_info(served)
    meta = {"version": version, "build_a": build_a, "build_b": build_b, "port": a.port, "cases": cases,
            "build_canary_a": build_c if a.canary_a else None, "build_canary_b": build_cb if a.canary_a else None,
            "mar_sizes": {n: os.path.getsize(os.path.join(files, n)) for n in sorted(os.listdir(files))}}
    with open(os.path.join(bundle, "cases.json"), "w") as f:
        json.dump(meta, f, indent=2)
    log(json.dumps(meta, indent=2))


def patch(a):
    bundle = os.path.abspath(a.bundle)
    real = real_certs()
    names = ("canary",) if a.canary else ("primary", "secondary")
    test = {w: open(os.path.join(bundle, "certs", f"test-{w}.der"), "rb").read() for w in names}
    patch_updater(updater_path(a.app), [(real[w], test[w]) for w in names], times=(1, 2) if a.canary else (1,))


# --- run ----------------------------------------------------------------------------

OBSERVE_JS = r"""
// Records the update service's notifications from now on (window.whisperUpdateEvents).
if (!window.whisperUpdateEvents) {
  window.whisperUpdateEvents = [];
  for (const topic of ["update-available", "update-downloading", "update-downloaded", "update-staged",
                       "update-error", "update-swap"]) {
    Services.obs.addObserver((subject, t, data) => {
      let u = null;
      try {
        u = subject && subject.QueryInterface(Ci.nsIUpdate);
      } catch (e) {}
      window.whisperUpdateEvents.push({
        t: Date.now(), topic: t, data,
        update: u && { state: u.state, errorCode: u.errorCode, buildID: u.buildID },
      });
    }, topic);
  }
}
return true;
"""

SNAPSHOT_JS = r"""
const aus = Cc["@mozilla.org/updates/update-service;1"].getService(Ci.nsIApplicationUpdateService);
const um = Cc["@mozilla.org/updates/update-manager;1"].getService(Ci.nsIUpdateManager);
const { AppMenuNotifications } = ChromeUtils.importESModule("resource://gre/modules/AppMenuNotifications.sys.mjs");
const { UpdateUtils } = ChromeUtils.importESModule("resource://gre/modules/UpdateUtils.sys.mjs");
const upd = u => u && {
  state: u.state, errorCode: u.errorCode, buildID: u.buildID, appVersion: u.appVersion,
  displayVersion: u.displayVersion,
  patch: u.selectedPatch && { type: u.selectedPatch.type, state: u.selectedPatch.state, size: u.selectedPatch.size },
};
const root = Services.dirsvc.get("UpdRootD", Ci.nsIFile).path;
const read = async p => { try { return (await IOUtils.readUTF8(p)).trim(); } catch (e) { return null; } };
const checker = Cc["@mozilla.org/updates/update-checker;1"].getService(Ci.nsIUpdateChecker);
const n = AppMenuNotifications.activeNotification;
return {
  buildID: Services.appinfo.appBuildID,
  platformBuildID: Services.appinfo.platformBuildID,
  version: Services.appinfo.version,
  whisperVersion: Services.prefs.getDefaultBranch("").getStringPref("whisper.version", ""),
  channel: UpdateUtils.UpdateChannel,
  builtInURL: await UpdateUtils.formatUpdateURL(Services.appinfo.updateURL),
  checkURL: await checker.getUpdateURL(Ci.nsIUpdateChecker.BACKGROUND_CHECK),
  canApply: aus.canApplyUpdates, canStage: aus.canStageUpdates, disabled: aus.disabled,
  state: aus.getStateName(aus.currentState),
  ready: upd(await um.getReadyUpdate()),
  downloading: upd(await um.getDownloadingUpdate()),
  history: (await um.getHistory()).map(upd),
  status: await read(PathUtils.join(root, "updates", "0", "update.status")),
  // What the update service has written to disk about the update it holds
  activeUpdate: await read(PathUtils.join(root, "active-update.xml")),
  events: window.whisperUpdateEvents || [],
  notification: n ? { id: n.id, dismissed: n.dismissed } : null,
  lastCheck: Services.prefs.getIntPref("app.update.lastUpdateTime.background-update-timer", 0),
  updRoot: root,
};
"""

INSTALLED_JS = r"""
const um = Cc["@mozilla.org/updates/update-manager;1"].getService(Ci.nsIUpdateManager);
const u = await um.updateInstalledAtStartup();
return u && { state: u.state, buildID: u.buildID, appVersion: u.appVersion, displayVersion: u.displayVersion };
"""

RESTART_JS = r"""
// Press "Restart to update" in the app menu, as a user would: run the main action of
// the update notification. The browser quits right after this script has answered.
const { AppMenuNotifications } = ChromeUtils.importESModule("resource://gre/modules/AppMenuNotifications.sys.mjs");
const n = AppMenuNotifications.activeNotification;
if (!n || n.id != "update-restart") {
  return { pressed: false, notification: n && n.id };
}
setTimeout(() => n.mainAction.callback(window, false), 500);
return { pressed: true, notification: n.id };
"""

OFFERED_JS = r"""
// Ask the update server what it offers and which update the browser would pick.
const aus = Cc["@mozilla.org/updates/update-service;1"].getService(Ci.nsIApplicationUpdateService);
const checker = Cc["@mozilla.org/updates/update-checker;1"].getService(Ci.nsIUpdateChecker);
const result = await checker.checkForUpdates(Ci.nsIUpdateChecker.BACKGROUND_CHECK).result;
const picked = await aus.selectUpdate(result.updates);
return {
  succeeded: result.succeeded,
  offered: result.updates.map(u => ({ buildID: u.buildID, appVersion: u.appVersion })),
  picked: picked && picked.buildID,
};
"""

TEST_PREFS = """user_pref("remote.prefs.recommended", false);
user_pref("app.update.disabledForTesting", false);
user_pref("whisper.update.testURL", "{url}");
user_pref("app.update.lastUpdateTime.background-update-timer", 0);
user_pref("app.update.timerFirstInterval", 20000);
user_pref("app.update.log", true);
user_pref("browser.dom.window.dump.enabled", true);
user_pref("devtools.console.stdout.chrome", true);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("marionette.port", {port});
user_pref("whisper.import.offerOnFirstRun", false);
"""


def port_open(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=1):
            return True
    except OSError:
        return False


def wait_port(port, want_open, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if port_open(port) == want_open:
            return True
        time.sleep(0.5)
    return False


def port_owners(port):
    """Ids of the processes listening on TCP `port` (Linux, from /proc)."""
    inodes = set()
    for table in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            lines = open(table).read().splitlines()[1:]
        except OSError:
            continue
        for line in lines:
            fields = line.split()
            if len(fields) > 9 and fields[3] == "0A" and int(fields[1].rsplit(":", 1)[1], 16) == port:
                inodes.add(fields[9])
    found = set()
    if not inodes:
        return found
    for pid in filter(str.isdigit, os.listdir("/proc")):
        try:
            for fd in os.listdir(f"/proc/{pid}/fd"):
                link = os.readlink(f"/proc/{pid}/fd/{fd}")
                if link.startswith("socket:[") and link[8:-1] in inodes:
                    found.add(int(pid))
                    break
        except OSError:
            continue
    return found


class Browser:
    """One Whisper process (and the process it restarts into), driven over Marionette.

    Every case has its own profile, its own copy of the browser and its own Marionette
    port, so a case that goes wrong cannot disturb the next one."""

    headless = False

    def __init__(self, app, profile, logfile, port):
        self.app = app
        self.profile = profile
        self.logfile = logfile
        self.port = port
        self.proc = None
        self.m = None

    def start(self, url):
        with open(os.path.join(self.profile, "user.js"), "w") as f:
            f.write(TEST_PREFS.format(url=url, port=self.port))
        if port_open(self.port):
            self.kill()
            if port_open(self.port):
                raise RuntimeError(f"something already listens on Marionette port {self.port}")
        exe = os.path.join(self.app, "whisper.exe" if WINDOWS else "whisper")
        # MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: Marionette may run scripts in the browser's own
        # (chrome) context only with this; the test reads the update service there.
        env = dict(os.environ, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
        if self.headless:
            env["MOZ_HEADLESS"] = "1"
        out = open(self.logfile, "a")
        out.write(f"\n===== start {time.strftime('%H:%M:%S')} {url} (Marionette port {self.port})\n")
        out.flush()
        # A process group of its own, so the browser and whatever it starts (content
        # processes, the restarted browser, the updater) can be ended together.
        group = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if WINDOWS
                 else {"start_new_session": True})
        self.proc = subprocess.Popen([exe, "--marionette", "--no-remote", "--profile", self.profile],
                                     stdout=out, stderr=subprocess.STDOUT, env=env, **group)
        # On Linux the started process is the browser; on Windows it is the launcher,
        # which exits as soon as it has started the browser.
        proc = self.proc
        self.connect(alive=(lambda: True) if WINDOWS else (lambda: proc.poll() is None))

    def processes(self):
        """Ids of this browser's processes (Linux; None on Windows), found by what stays
        the same across "Restart to update" rather than by process id: the restart is a
        new process (AppShutdown::MaybeDoRestart -> LaunchChild), which gets the profile
        through XRE_PROFILE_PATH instead of --profile, execs the updater to swap in the
        staged update, which execs the browser again (nsUpdateDriver ApplyUpdate,
        updater LaunchCallbackApp). So: --profile on the command line, XRE_PROFILE_PATH
        in the environment, the process id in the profile's lock, the process group, and
        whoever listens on this browser's Marionette port."""
        if WINDOWS:
            return None
        want = self.profile.encode()
        env_want = b"XRE_PROFILE_PATH=" + want
        found = set(port_owners(self.port))
        try:
            target = os.readlink(os.path.join(self.profile, "lock"))  # "127.0.0.1:+<pid>"
            pid = int(target.rsplit("+", 1)[1])
            if os.path.exists(f"/proc/{pid}"):
                found.add(pid)
        except (OSError, ValueError, IndexError):
            pass
        group = self.proc.pid if self.proc is not None else None
        for pid in filter(str.isdigit, os.listdir("/proc")):
            try:
                with open(f"/proc/{pid}/cmdline", "rb") as f:
                    args = f.read().split(b"\0")
                if not args or args == [b""]:
                    continue  # a zombie
                if want in args:
                    found.add(int(pid))
                    continue
                if group is not None and os.getpgid(int(pid)) == group:
                    found.add(int(pid))
                    continue
                with open(f"/proc/{pid}/environ", "rb") as f:
                    if env_want in f.read().split(b"\0"):
                        found.add(int(pid))
            except (OSError, ProcessLookupError):
                continue
        found.discard(os.getpid())
        return sorted(found)

    def alive(self):
        """False once no process of this browser is left (always True on Windows)."""
        procs = self.processes()
        return procs is None or bool(procs)

    def log_tail(self, lines=25):
        try:
            with open(self.logfile, errors="replace") as f:
                return "".join(f.readlines()[-lines:])
        except OSError:
            return ""

    def connect(self, seconds=120, alive=lambda: True):
        self.m = Marionette(port=self.port, timeout=seconds, alive=alive)
        self.m.start_session()
        self.m.run(OBSERVE_JS)

    def drop_connection(self):
        if self.m is not None:
            try:
                self.m.sock.close()
            except OSError:
                pass
            self.m = None

    def kill(self):
        """Ends everything that is left of this browser, then waits until its Marionette
        port is free. Called after every case, passed or not."""
        self.drop_connection()
        if WINDOWS:
            if self.proc is not None:
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(self.proc.pid)], capture_output=True)
            # The launcher's browser is not in the launcher's process tree once the
            # launcher has exited; the test is the only thing running Whisper here.
            for image in ("whisper.exe", "updater.exe"):
                subprocess.run(["taskkill", "/F", "/T", "/IM", image], capture_output=True)
        else:
            for _ in range(3):
                if self.proc is not None:
                    try:
                        os.killpg(self.proc.pid, 9)
                    except OSError:
                        pass
                left = self.processes()
                for pid in left:
                    try:
                        os.kill(pid, 9)
                    except OSError:
                        pass
                if not left:
                    break
                time.sleep(1)
        if self.proc is not None:
            try:
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                pass
        if not wait_port(self.port, False, 30):
            log(f"warning: Marionette port {self.port} is still in use")

    def run(self, js, *args):
        return self.m.run(js, *args)

    def snapshot(self):
        return self.m.run(SNAPSHOT_JS)

    def quit(self):
        if self.m is not None:
            try:
                self.m.quit()
            except (MarionetteError, OSError):
                pass
            self.m = None
        wait_port(self.port, False, 60)
        if self.proc:
            try:
                self.proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                pass

    def restart_to_update(self):
        """Presses "Restart to update"; returns once the restarted browser is reachable."""
        # Who listens on the Marionette port now (Linux): the browser that is to quit.
        before = set(port_owners(self.port)) if not WINDOWS else set()
        pressed = self.m.run(RESTART_JS)
        if not pressed["pressed"]:
            return pressed
        # The old session ends with the old browser; probing the port while it lives
        # only makes Marionette log "an active session has been found".
        self.drop_connection()
        t0 = time.monotonic()
        # The restarted browser can listen on the same port again within a few hundred
        # milliseconds (0.4 s for the canary in CI), faster than a poll of the port sees
        # it closed. On Linux the quit is therefore "none of the old listeners listens
        # any more" (closed, or a new process has it); elsewhere the port is polled often.
        if before:
            quit_seen = lambda: not (before & set(port_owners(self.port)))  # noqa: E731
        else:
            quit_seen = lambda: not port_open(self.port)  # noqa: E731
        deadline = time.monotonic() + 120
        while not quit_seen():
            if time.monotonic() > deadline:
                raise RuntimeError("the browser did not quit within 120 s after Restart to update")
            time.sleep(0.1)
        # The browser now runs the updater, which swaps in the staged update and starts
        # the browser again with the same arguments (Marionette and its port included).
        deadline = time.monotonic() + 300
        gone_since = None
        while not port_open(self.port):
            if self.alive():
                gone_since = None
            elif gone_since is None:
                gone_since = time.monotonic()
            elif time.monotonic() - gone_since > 30:
                raise RuntimeError("no process of the browser is left after Restart to update; "
                                   f"its log ends:\n{self.log_tail()}")
            if time.monotonic() > deadline:
                raise RuntimeError("the browser did not come back within 300 s after Restart to update; "
                                   f"processes {self.processes()}; its log ends:\n{self.log_tail()}")
            time.sleep(0.5)
        log(f"restarted browser listens again after {time.monotonic() - t0:.1f} s "
            f"(processes {self.processes()})")
        # Marionette may take a moment after the port opens; keep trying for 120 s.
        self.connect(seconds=120)
        return pressed


def server_requests(path):
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def wait_for(browser, done, seconds):
    deadline = time.monotonic() + seconds
    snap = browser.snapshot()
    while time.monotonic() < deadline:
        if done(snap):
            return snap, True
        time.sleep(2)
        snap = browser.snapshot()
    return snap, done(snap)


def staged_event(snap):
    return next((e for e in snap["events"] if e["topic"] == "update-staged"), None)


def copy_logs(snap, app, dest):
    os.makedirs(dest, exist_ok=True)
    roots = {snap.get("updRoot"), app}
    for root in filter(None, roots):
        for rel in ("updates/last-update.log", "updates/backup-update.log", "updates/0/update.log",
                    "updates/0/update.status"):
            src = os.path.join(root, *rel.split("/"))
            if os.path.isfile(src):
                shutil.copy(src, os.path.join(dest, rel.replace("/", "_")))


def run_case(name, case, browser, app, out, server_log, results):
    log(f"\n=== {name}: expect {case['expect']}")
    before_requests = len(server_requests(server_log))
    browser.start(case["url"])
    r = {"case": name, "expect": case["expect"], "served": case["mar"], "claimed_build": case["build"]}
    start = browser.snapshot()
    r["before"] = {k: start[k] for k in ("buildID", "platformBuildID", "version", "whisperVersion", "channel",
                                         "builtInURL", "checkURL", "canApply", "canStage", "disabled")}
    expect = case["expect"]
    ok = False
    if expect == "applied":
        snap, _ = wait_for(browser, lambda s: (staged_event(s) is not None) or
                           (s["ready"] or {}).get("state") == "applied", 240)
        # The menu notification follows the staging right away (badge wait time 0).
        snap, _ = wait_for(browser, lambda s: (s["notification"] or {}).get("id") == "update-restart", 15)
        r["staged"] = {k: snap[k] for k in ("state", "ready", "status", "events", "notification")}
        staged_ok = (snap["ready"] or {}).get("state") == "applied" and snap["status"] == "applied"
        copy_logs(snap, app, os.path.join(out, name))
        if staged_ok:
            # The update service writes the staged update to active-update.xml after
            # staging; a restart before that write is on disk leaves the history entry
            # without its details (seen once in CI: B running, but no
            # updateInstalledAtStartup). Restart once it is written, as a user would.
            written = lambda s: (f'buildID="{case["build"]}"' in (s.get("activeUpdate") or "")  # noqa: E731
                                 and 'state="applied"' in (s.get("activeUpdate") or ""))
            snap, r["active_update_written"] = wait_for(browser, written, 30)
            try:
                r["restart"] = browser.restart_to_update()
            except (RuntimeError, MarionetteError, OSError) as e:
                # Keep what is known (staging, the updater's logs) in the report.
                r["error"] = f"{e}"
                copy_logs(snap, app, os.path.join(out, name, "after-restart"))
                browser.kill()
                r["ok"] = False
                log(json.dumps(r, indent=2)[:8000])
                log(f"FAIL  {name}")
                results.append(r)
                return False
            copy_logs(snap, app, os.path.join(out, name, "after-restart"))
            # The update service finishes its start-up work (moving the installed
            # update into the history) some time after the window is there; Marionette
            # can be faster. Wait until it has, for up to 60 s.
            deadline = time.monotonic() + 60
            while True:
                after = browser.snapshot()
                installed = browser.run(INSTALLED_JS)
                history = [h for h in after["history"] if h]
                if (installed and history and history[0].get("buildID")) or time.monotonic() > deadline:
                    break
                time.sleep(2)
            r["after"] = {k: after[k] for k in ("buildID", "platformBuildID", "version", "whisperVersion",
                                                "history", "ready", "status")}
            r["installed_at_startup"] = installed
            ini_version, ini_build = app_ini(os.path.join(app, "application.ini"))
            r["application_ini_build"] = ini_build
            ok = (r["restart"].get("pressed") and after["buildID"] == case["then"]
                  and ini_build == case["then"]
                  and (r["installed_at_startup"] or {}).get("buildID") == case["then"]
                  and after["ready"] is None)
        browser.quit()
    elif expect == "not-offered":
        r["offered"] = browser.run(OFFERED_JS)
        # Also let the automatic background check (20 s after start) run and finish.
        snap, _ = wait_for(browser, lambda s: s["lastCheck"] > 0, 90)
        time.sleep(15)
        snap = browser.snapshot()
        r["after"] = {k: snap[k] for k in ("buildID", "state", "ready", "downloading", "events", "lastCheck")}
        reqs = server_requests(server_log)[before_requests:]
        r["requests"] = [q["path"] for q in reqs]
        asked = any(q["path"].startswith(f"/{name}/update.xml") for q in reqs)
        fetched = any(case["mar"] in q["path"] for q in reqs)
        ok = (asked and not fetched and r["offered"]["picked"] is None and r["offered"]["offered"]
              and snap["ready"] is None and snap["downloading"] is None and snap["lastCheck"] > 0
              and not any(e["topic"] in ("update-downloading", "update-staged") for e in snap["events"]))
        browser.quit()
    else:
        def download_refused(s):
            return any(e["topic"] == "update-error" and str(e["data"]).startswith("download-attempt")
                       for e in s["events"])
        if expect == "refused":
            # Refused when downloaded, or downloaded and then refused by the updater.
            done = lambda s: download_refused(s) or staged_event(s) is not None  # noqa: E731
        else:
            done = lambda s: staged_event(s) is not None  # noqa: E731
        snap, _ = wait_for(browser, done, 240)
        ev = staged_event(snap)
        r["outcome"] = {k: snap[k] for k in ("state", "ready", "downloading", "history", "status", "events")}
        copy_logs(snap, app, os.path.join(out, name))
        reqs = server_requests(server_log)[before_requests:]
        r["requests"] = [f"{q['path']} {q['status']} {q['bytes']}" for q in reqs]
        r["errors"] = [e["data"] for e in snap["events"] if e["topic"] == "update-error"]
        code = ev and ev["update"] and ev["update"]["errorCode"]
        r["error_code"] = code
        fetched = any(case["mar"] in q["path"] and q["status"] in (200, 206) for q in reqs)
        # The refused update ends in the failed state in the update history ...
        failed = [u for u in snap["history"] if u and u["buildID"] == case["build"]]
        r["history_state"] = failed[0]["state"] if failed else None
        refused_by_updater = (ev is not None and ev["data"] == "failed" and bool(failed)
                              and failed[0]["state"] == "failed")
        if expect == "refused":
            if ev is None and download_refused(snap):
                r["refused_at"] = "download"
                ok = fetched
            elif refused_by_updater and code == CERT_VERIFY_ERROR and failed[0]["errorCode"] == code:
                r["refused_at"] = f"updater (error {code})"
                ok = fetched
        else:
            ok = refused_by_updater and code == expect and failed[0]["errorCode"] == expect
            if ok:
                r["refused_at"] = f"updater (error {code})"
        browser.quit()
        # ... and on the next start nothing is applied: the build that ran before the
        # case is still the one running.
        browser.start(f"http://127.0.0.1:{case.get('port', 8111)}/none/update.xml")
        check = browser.snapshot()
        r["next_start"] = {"buildID": check["buildID"], "installed_at_startup": browser.run(INSTALLED_JS)}
        browser.quit()
        ok = (ok and check["buildID"] == r["before"]["buildID"]
              and r["next_start"]["installed_at_startup"] is None)
    r["ok"] = bool(ok)
    log(json.dumps(r, indent=2)[:6000])
    log(f"{'PASS' if ok else 'FAIL'}  {name}")
    results.append(r)
    return ok


def b_installed(app, build_b):
    try:
        return app_ini(os.path.join(app, "application.ini"))[1] == build_b
    except Exception:  # noqa: BLE001  no or broken application.ini: not B
        return False


def run(a):
    Browser.headless = a.headless
    bundle = os.path.abspath(a.bundle)
    meta = json.load(open(os.path.join(bundle, "cases.json")))
    out = os.path.abspath(a.out)
    work = os.path.abspath(a.work)
    os.makedirs(out, exist_ok=True)
    os.makedirs(work, exist_ok=True)
    wanted = a.cases.split(",") if a.cases else [c for c in ALL_CASES if c in meta["cases"]]
    unknown = [c for c in wanted if c not in ALL_CASES]
    if unknown:
        raise SystemExit(f"unknown cases {unknown}")
    cases = [c for c in ALL_CASES if c in wanted]
    server_log = os.path.join(out, "requests.jsonl")
    server = subprocess.Popen([sys.executable, os.path.join(HERE, "updateserver.py"), "--root",
                               os.path.join(bundle, "srv"), "--port", str(meta["port"]), "--log", server_log])
    if not wait_port(meta["port"], True, 30):
        raise SystemExit("the update server did not start")
    results = []
    main_app = os.path.abspath(a.app)
    canary_app = os.path.abspath(a.canary_app) if a.canary_app else None
    try:
        canary_template = None
        if canary_app and "release-mar-on-canary" in cases:
            canary_template = os.path.join(work, "canary-template", os.path.basename(canary_app))
            shutil.copytree(canary_app, canary_template, symlinks=True)
        template = a.template
        if not template and any(c in FROM_A or c == "canary-mar-on-release" for c in cases):
            # A clean copy of A for the cases that start from A, made before "update"
            # turns main_app into B.
            template = os.path.join(work, "template", os.path.basename(main_app))
            shutil.copytree(main_app, template, symlinks=True)
        for name in cases:
            if name not in meta["cases"]:
                results.append({"case": name, "ok": False,
                                "error": f"the bundle has no case {name} (made by prepare without --canary-a?)"})
                log(f"FAIL  {name}: not in the bundle")
                continue
            case = dict(meta["cases"][name], port=meta["port"])
            # Every case: its own copy of the browser, its own profile, its own
            # Marionette port.
            app, source = main_app, "the installed A"
            try:
                if name in CANARY and not canary_app:
                    raise MarError("no canary build given (--canary-app)")
                if name == "canary-update":
                    app, source = canary_app, "the installed canary A"
                elif name in CANARY:
                    app = os.path.join(work, name, os.path.basename(canary_app))
                    shutil.copytree(canary_template, app, symlinks=True)
                    source = "a copy of canary A"
                elif name in FROM_B:
                    app = os.path.join(work, name, os.path.basename(main_app))
                    if b_installed(main_app, meta["build_b"]):
                        shutil.copytree(main_app, app, symlinks=True)
                        source = "a copy of the installation the update case updated to B"
                    else:
                        count = extract_mar(os.path.join(bundle, "srv", "files", "b-primary.mar"), app)
                        source = f"B unpacked from b-primary.mar ({count} files); the update case left no B"
                elif name != "update":
                    app = os.path.join(work, name, os.path.basename(main_app))
                    shutil.copytree(template, app, symlinks=True)
                    source = "a copy of A"
            except (OSError, MarError) as e:
                results.append({"case": name, "expect": case["expect"], "ok": False,
                                "error": f"could not set up the browser for the case: {e!r}"})
                log(f"FAIL  {name}: {e!r}")
                continue
            profile = tempfile.mkdtemp(prefix=f"profile-{name}-", dir=work)
            browser = Browser(app, profile, os.path.join(out, f"browser-{name}.log"),
                              MARIONETTE_BASE_PORT + ALL_CASES.index(name))
            log(f"\n--- {name}: {source}, Marionette port {browser.port}")
            try:
                run_case(name, case, browser, app, out, server_log, results)
                results[-1]["browser"] = source
            except (MarionetteError, RuntimeError, OSError) as e:
                results.append({"case": name, "expect": case["expect"], "ok": False, "error": repr(e),
                                "browser": source, "browser_log_tail": browser.log_tail()})
                log(f"FAIL  {name}: {e!r}")
            finally:
                browser.kill()
    finally:
        server.terminate()
    with open(os.path.join(out, "report.json"), "w") as f:
        json.dump({"meta": meta, "results": results}, f, indent=2, default=str)
    try:
        write_markdown(meta, results, os.path.join(out, "report.md"))
    except Exception as e:  # noqa: BLE001  the verdict below must still be printed
        log(f"report.md could not be written: {e!r}")
    failed = [r["case"] for r in results if not r.get("ok")]
    missing = [c for c in cases if c not in {r["case"] for r in results}]
    log(f"\n{len(results) - len(failed)}/{len(cases)} cases passed" + (f"; failed: {failed}" if failed else "")
        + (f"; not run: {missing}" if missing else ""))
    return 1 if failed or missing else 0


def describe(r):
    """One line: what a case saw (also for a case that broke off half way)."""
    expect = r.get("expect")
    nxt = (r.get("next_start") or {}).get("buildID")
    if "error" in r:
        seen = r["error"]
        if r.get("staged"):
            seen = f"staged: {r['staged'].get('status')}; then {seen}"
        return seen
    if expect == "applied":
        return (f"staged: {(r.get('staged') or {}).get('status')}; after restart running build "
                f"{(r.get('after') or {}).get('buildID')} (application.ini {r.get('application_ini_build')})")
    if expect == "not-offered":
        offered = r.get("offered") or {}
        return f"offered {offered.get('offered')}, picked {offered.get('picked')}; requests {r.get('requests')}"
    if r.get("refused_at"):
        return f"refused at {r['refused_at']}; next start build {nxt}"
    return (f"not refused as expected (updater error {r.get('error_code')}, update-error {r.get('errors')}); "
            f"next start build {nxt}")


def write_markdown(meta, results, path):
    a, b = meta.get("build_a"), meta.get("build_b")
    lines = [f"Firefox {meta.get('version')}: A = build {a}, B = build {b}\n",
             "| | Case | Expected | Observed |", "|---|---|---|---|"]
    for r in results:
        try:
            observed = describe(r)
        except Exception as e:  # noqa: BLE001  one odd result must not lose the others
            observed = f"(could not describe: {e!r})"
        expect = r.get("expect")
        expected = {"refused": "refused (at download, or by the updater: 19)"}.get(expect, expect) \
            if isinstance(expect, str) or expect is None else f"updater error {expect}"
        observed = " ".join(str(observed).split())
        if len(observed) > 600:
            observed = observed[:600] + " ..."
        lines.append(f"| {'✅' if r.get('ok') else '❌'} | {r.get('case')} | {expected} | {observed} |")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--a", required=True)
    p.add_argument("--b", required=True)
    p.add_argument("--bundle", required=True)
    p.add_argument("--port", type=int, default=8111)
    p.add_argument("--canary-a", default=None, help="the Whisper Canary package built with A")
    p = sub.add_parser("patch")
    p.add_argument("--bundle", required=True)
    p.add_argument("--app", required=True)
    p.add_argument("--canary", action="store_true", help="APP is Whisper Canary")
    p = sub.add_parser("run")
    p.add_argument("--bundle", required=True)
    p.add_argument("--app", required=True, help="installed (or unpacked) A, already patched")
    p.add_argument("--template", default=None, help="a clean patched copy of A for the other cases")
    p.add_argument("--canary-app", default=None, help="installed (or unpacked) canary A, already patched")
    p.add_argument("--work", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--cases", default="")
    p.add_argument("--headless", action="store_true", help="run the browser without a display")
    a = ap.parse_args()
    if a.cmd == "prepare":
        prepare(a)
    elif a.cmd == "patch":
        patch(a)
    else:
        sys.exit(run(a))


if __name__ == "__main__":
    main()
