# Security

## Reporting a problem

Please report security problems **privately** through GitHub's private vulnerability reporting:
open the [Security tab](https://github.com/footprintsntrails/whisper-browser/security) of this
repository and choose **Report a vulnerability**. Please do not open a public issue.

Helpful to include: what is affected, how to reproduce it, and what an attacker could do with it.
You will get an answer as soon as possible, and credit in the release notes if you want it.

## What is in scope

- Whisper's own build of Firefox: the patches in `patches/`, the files in `gecko/overlay/`
  (prefs, interface, import), and how it is built and released
- The update path: the update host, the MAR signing and its checks, the updater's certificates
- The installers and packages built by this repository

Problems in Firefox itself go to [Mozilla](https://www.mozilla.org/security/), and problems in
uBlock Origin to [its project](https://github.com/gorhill/uBlock).

## Supported versions

Only the latest release gets fixes. Installed copies check for updates every 6 hours while
they run.

## How updates are verified

Whisper updates itself with Firefox's own updater. An update is a complete MAR file from the
GitHub release; `update.xml` on GitHub Pages names it with its size and SHA-512, which the
browser checks after downloading. The updater then applies it only if:

- it carries exactly one RSA-4096 signature (PKCS#1 v1.5, SHA-384) that one of the two
  certificates compiled into the updater verifies: the primary one, which signs releases, or
  the secondary one, a rotation key kept apart. Mozilla's certificates are not in Whisper's
  updater;
- its MAR channel is `whisper-release`;
- its product version, which carries the build ID (`159.0.1.20261106.093414`), is not older
  than the running build, so an older signed release cannot be installed over a newer one.

The SHA-256 fingerprints of the two certificates are pinned in
[`gecko/release.env`](gecko/release.env) and checked when the source tree is prepared and when
a package is built:

```
primary    BB:DE:B2:1F:DE:EF:D6:1A:B9:B7:95:F3:91:FA:16:C0:6B:6A:0E:73:68:F9:4A:C8:F5:46:64:BC:FB:32:29:1F
secondary  25:96:8B:49:B6:86:D3:92:D2:E5:00:19:AC:77:89:CC:BF:44:74:7D:72:51:60:08:1C:E8:00:CD:25:93:1E:A5
```

Check them with
`openssl x509 -inform DER -in gecko/overlay/toolkit/mozapps/update/updater/whisper_primary.der -noout -fingerprint -sha256`
(and `whisper_secondary.der`). Every build is tested end to end: it updates itself to a newer
build and refuses older builds, MARs signed with another key, tampered or unsigned MARs and
MARs for another channel (`.github/workflows/gecko-update-test.yml`).

Whisper Canary, the test edition built from every green build, is kept apart: its updater
trusts only its own certificate, `whisper_canary.der`
(`D8:CC:17:51:B2:EE:1C:F3:43:2E:AB:C4:0C:DA:FC:84:3E:D3:7D:3A:CE:AC:65:E4:43:D8:8A:B4:0A:E7:C8:39`),
and its MAR channel is `whisper-canary`. Its key signs only canary builds, from the `canary`
environment. Whisper refuses canary MARs and the canary refuses Whisper's; the update test
checks both.
