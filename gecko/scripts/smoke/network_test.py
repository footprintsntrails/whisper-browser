#!/usr/bin/env python3
"""Whisper network test: every host the browser itself contacts, with servers answering.

Usage: network_test.py --app DIR --out DIR --ca DIR --sink-log FILE --dns-log FILE
                       [--seconds 420] [--allowlist gecko/network-allowlist.txt]

Run by run-network.sh inside a network namespace with no way out, where dnsstub.py
answers every name with 127.0.0.1 and sink.py answers every connection there (TLS with a
certificate for any name, DNS over HTTPS, enough of Remote Settings to make the browser
go on). The first-start test (run-smoke.sh) answers "no such name" and so only sees the
names a browser looks up before any server replies; this one also sees what follows a
reply (an attachments host named by a server, a signature chain, a redirect).

What it does, with a fresh profile and no page loaded:
  1. trusts the sink's throw-away CA through an enterprise policy (Certificates.Install
     in distribution/policies.json of the unpacked test package, removed afterwards);
  2. makes the periodic jobs due at once instead of after hours or days: every update
     timer (Whisper's update check, add-on updates, Remote Settings, search engines,
     region, ...) is marked as last run in 1970 and the timer manager's delays are set
     to their minimum, then "idle-daily" is sent (daily maintenance);
  3. waits --seconds, quits, starts again with the same profile for a minute (some work
     happens only on a second start), quits;
  4. collects every host name: DNS queries, TLS server names, Host headers and DoH
     lookups the sink saw, and Whisper's own-connection log (about the browser's own
     requests, WhisperNetLog) when the build has it;
  5. fails when a host is not in the allowlist, and reports where each host was seen
     and whether the own-connection log listed it.

Writes OUT/network-report.{json,md} and OUT/network-hostnames.txt; exits 1 on failure.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from marionette import Marionette, MarionetteError  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_ALLOWLIST = os.path.join(HERE, "..", "..", "network-allowlist.txt")

# Timers registered with Firefox's timer manager in 159 (category update-timer and
# registerTimer() calls); FORCE_JS below also resets any other one it finds.
TIMERS = ["background-update-timer", "addon-background-update-timer",
          "services-settings-poll-changes", "xpi-signature-verification",
          "search-engine-update-timer", "region-update-timer", "browser-cleanup-thumbnails",
          "rs-experiment-loader-timer", "glean-addons-daily", "suggest-ingest",
          "content-relevancy-timer", "recipe-client-addon-run"]

USER_JS = """
user_pref("remote.prefs.recommended", false);
user_pref("whisper.import.offerOnFirstRun", false);
// The timer manager checks for due timers 10 s after start and then every 30 s (the
// smallest values it accepts), instead of after 30 s and 2 minutes.
user_pref("app.update.timerFirstInterval", 10000);
user_pref("app.update.timerMinimumDelay", 30);
"""

FORCE_JS = r"""
// Every update timer is due now: its last run is set to 1970 (the timer manager then
// runs one due timer per check, every 30 s here).
const names = Services.prefs.getChildList("app.update.lastUpdateTime.");
for (const name of names) {
  Services.prefs.setIntPref(name, 1);
}
// Daily maintenance normally waits for the user to be idle once a day.
Services.obs.notifyObservers(null, "idle-daily");
return names.map(n => n.replace("app.update.lastUpdateTime.", ""));
"""

NETLOG_JS = r"""
try {
  const { WhisperNetLog } = ChromeUtils.importESModule(
    "moz-src:///browser/components/whisper/security/WhisperNetLog.sys.mjs");
  return { entries: WhisperNetLog.entries(), summary: WhisperNetLog.summary() };
} catch (e) {
  return { error: String(e) };
}
"""


def load_allowlist(path):
    names = []
    for line in open(path, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#"):
            names.append(line.lower())
    return names


def run_browser(app, profile, out, name, seconds, force):
    log = open(os.path.join(out, f"{name}-browser.log"), "w")
    env = dict(os.environ, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
    proc = subprocess.Popen([os.path.join(app, "whisper"), "--marionette", "--no-remote", "--profile", profile],
                            stdout=log, stderr=subprocess.STDOUT, env=env)
    result = {"timers": [], "netlog": None}
    started = time.monotonic()
    try:
        m = Marionette(alive=lambda: proc.poll() is None)
        m.start_session()
        time.sleep(5)
        if force:
            result["timers"] = m.run(FORCE_JS)
        while time.monotonic() - started < seconds:
            time.sleep(5)
        result["netlog"] = m.run(NETLOG_JS)
        try:
            m.quit()
        except MarionetteError:
            pass
    except MarionetteError as e:
        result["error"] = str(e)
    try:
        result["exit"] = proc.wait(timeout=90)
    except subprocess.TimeoutExpired:
        proc.kill()
        result["exit"] = "killed"
    log.close()
    return result


def hosts_from_logs(dns_log, sink_log):
    seen = {}

    def add(host, how):
        host = (host or "").strip(".").lower()
        if host and not re.fullmatch(r"[\d.]+|localhost", host):
            seen.setdefault(host, set()).add(how)

    if os.path.exists(dns_log):
        for line in open(dns_log, encoding="utf-8", errors="replace"):
            parts = line.split()
            if len(parts) >= 3:
                add(parts[2], "dns")
    if os.path.exists(sink_log):
        for line in open(sink_log, encoding="utf-8"):
            try:
                e = json.loads(line)
            except ValueError:
                continue
            add(e.get("sni"), "tls")
            add(e.get("host"), "http")
            add(e.get("doh"), "doh")
    return seen


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--ca", required=True, help="the sink's CA folder (ca.pem)")
    ap.add_argument("--sink-log", required=True)
    ap.add_argument("--dns-log", required=True)
    ap.add_argument("--seconds", type=float, default=420)
    ap.add_argument("--allowlist", default=DEFAULT_ALLOWLIST)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)

    # The resolv.conf search domain makes the system resolver also ask for
    # "<name>.<domain>"; those copies are not names the browser chose.
    search = []
    if os.path.exists("/etc/resolv.conf"):
        for line in open("/etc/resolv.conf"):
            if line.startswith("search"):
                search += line.split()[1:]

    policies = os.path.join(a.app, "distribution", "policies.json")
    if os.path.exists(policies):
        raise SystemExit(f"{policies} exists; the test package must not have one")
    os.makedirs(os.path.dirname(policies), exist_ok=True)
    with open(policies, "w") as f:
        json.dump({"policies": {"Certificates": {"Install": [os.path.join(a.ca, "ca.pem")]}}}, f)
    profile = tempfile.mkdtemp(prefix="whisper-network-")
    with open(os.path.join(profile, "user.js"), "w") as f:
        f.write(USER_JS)
        for t in TIMERS:
            f.write(f'user_pref("app.update.lastUpdateTime.{t}", 1);\n')
    try:
        first = run_browser(a.app, profile, a.out, "first", a.seconds, force=True)
        second = run_browser(a.app, profile, a.out, "second", 60, force=False)
    finally:
        os.remove(policies)

    seen = hosts_from_logs(a.dns_log, a.sink_log)
    for host in list(seen):
        if any(host.endswith("." + d) for d in search):
            del seen[host]
    allow = set(load_allowlist(a.allowlist))
    netlog_hosts = set()
    for run in (first, second):
        for e in ((run.get("netlog") or {}).get("entries") or []):
            netlog_hosts.add(e.get("host", "").lower())
    has_netlog = any("entries" in (r.get("netlog") or {}) for r in (first, second))

    unexpected = sorted(h for h in seen if h not in allow)
    rows = []
    for host in sorted(seen):
        rows.append({"host": host, "seen": sorted(seen[host]), "allowed": host in allow,
                     "own_log": (host in netlog_hosts) if has_netlog else None})
    # The own-connection log must not list anything outside the allowlist either.
    netlog_unexpected = sorted(h for h in netlog_hosts if h and h not in allow)
    # Every host the browser sent an HTTP request to (not only a DNS lookup) should be
    # in the own-connection log; DNS-over-HTTPS queries are logged there separately.
    http_hosts = {h for h, how in seen.items() if how & {"http", "tls"}}
    missing_in_log = sorted(h for h in http_hosts if has_netlog and h not in netlog_hosts)

    report = {"first": first, "second": second, "hosts": rows, "unexpected": unexpected,
              "netlog_unexpected": netlog_unexpected, "missing_in_own_log": missing_in_log}
    with open(os.path.join(a.out, "network-report.json"), "w") as f:
        json.dump(report, f, indent=2, default=str)
    with open(os.path.join(a.out, "network-hostnames.txt"), "w") as f:
        for r in rows:
            f.write(f"{r['host']}\t{','.join(r['seen'])}\n")
    ok = not unexpected and not netlog_unexpected and not missing_in_log \
        and first.get("exit") == 0 and second.get("exit") == 0
    with open(os.path.join(a.out, "network-report.md"), "w") as f:
        f.write("| Host | Seen as | In allowlist | In own-connection log |\n|---|---|---|---|\n")
        for r in rows:
            own = "-" if r["own_log"] is None else ("yes" if r["own_log"] else "no")
            f.write(f"| {r['host']} | {', '.join(r['seen'])} | {'yes' if r['allowed'] else '**no**'} | {own} |\n")
        f.write(f"\nTimers made due: {', '.join(first.get('timers') or [])}\n")
        f.write(f"\nExit codes: first start {first.get('exit')}, second start {second.get('exit')}\n")
    print(open(os.path.join(a.out, "network-report.md")).read())
    if unexpected:
        print(f"::error::hosts outside the allowlist: {' '.join(unexpected)}")
    if netlog_unexpected:
        print(f"::error::the own-connection log lists hosts outside the allowlist: {' '.join(netlog_unexpected)}")
    if missing_in_log:
        print(f"::error::requests missing from the own-connection log: {' '.join(missing_in_log)}")
    shutil.rmtree(profile, ignore_errors=True)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
