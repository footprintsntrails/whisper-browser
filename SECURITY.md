# Security

## Reporting a problem

Please report security problems **privately** through GitHub's private vulnerability reporting:
open the [Security tab](https://github.com/footprintsntrails/whisper-browser/security) of this
repository and choose **Report a vulnerability**. Please do not open a public issue.

Helpful to include: what is affected, how to reproduce it, and what an attacker could do with it.
You will get an answer as soon as possible, and credit in the release notes if you want it.

## What is in scope

- The `whisper` program: profile setup, imports, the self-updater and its signature checks
- The installers and packages built by this repository
- `profile/user.js` and the theme, where a setting weakens privacy or security

Problems in Firefox itself go to [Mozilla](https://www.mozilla.org/security/), and problems in
uBlock Origin to [its project](https://github.com/gorhill/uBlock).

## Supported versions

Only the latest release gets fixes. Installed copies update themselves once a day.

## How updates are verified

Each release has an `update.json` listing every file with its size and SHA-256, signed with
RSA-3072 (PKCS#1 v1.5, SHA-256). Whisper checks that signature with the public key compiled into
it, [`keys/update-signing.pub.pem`](keys/update-signing.pub.pem), before it downloads or installs
anything. The SHA-256 of that key in DER form is:

```
7be75d73e9b92bf53a0f6078850441aa4e9fe4b6fb7123c3fcd8c7492cc92e91
```

Check it with `openssl pkey -pubin -in keys/update-signing.pub.pem -outform DER | sha256sum`.
