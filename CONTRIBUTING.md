# Contributing to Whisper

Thank you for helping. Bug reports, ideas and pull requests are all welcome.

- **Bugs and ideas:** open an issue with the bug report or feature request form.
- **Security problems:** please do not open an issue; see [SECURITY.md](SECURITY.md).

## Working on the code

Whisper is Firefox's source release with the patches in `patches/` and the new files in
`gecko/overlay/`; [Build from source](README.md#build-from-source) shows how to build it.
GitHub Actions builds the browser when a push changes it (`.github/workflows/gecko.yml`) and
tests each build: it starts with a fresh profile while every host name it looks up is
recorded, imports from other browsers, and updates itself. A change to Whisper's interface only
(`gecko/overlay/browser/components/whisper/`) is smoke-tested in minutes: the interface files
go into the last build (`gecko/scripts/repack-ui.py`, `.github/workflows/gecko-ui-quick.yml`).

The checks CI runs on every push need [rustup](https://rustup.rs) (`rust-toolchain.toml` pins
the toolchain of the build tooling in `crates/xtask`) and Python 3:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask scan                     # no secrets, e-mail addresses or local paths
python3 .github/check-scripts.py     # every script that is run directly can be run
```

## Ground rules

- **Nothing new phones home.** A change to the built-in prefs
  (`gecko/overlay/browser/components/whisper/0-whisper.js`) must not make the browser contact a
  new server, and every pref gets a comment saying what it does. A first start that looks up a
  host name outside `gecko/network-allowlist.txt` fails the build.
- **Tests come with behaviour changes.** The first start, imports and updates are covered end to
  end by the smoke, import and update tests in `gecko/scripts/smoke/` and
  `gecko/scripts/update/`; please keep it that way.
- **Few, well-known dependencies.** Each new crate should earn its place; `cargo deny` checks
  advisories and licenses in CI.
- **Small, focused pull requests** with a clear description. CI has to be green before merging.

By contributing you agree that your contribution is licensed under the
[Mozilla Public License 2.0](LICENSE).
