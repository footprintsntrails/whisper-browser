# Contributing to Whisper

Thank you for helping. Bug reports, ideas and pull requests are all welcome.

- **Bugs and ideas:** open an issue with the bug report or feature request form.
- **Security problems:** please do not open an issue; see [SECURITY.md](SECURITY.md).

## Working on the code

You need [rustup](https://rustup.rs); `rust-toolchain.toml` pins the toolchain.

```sh
cargo test --workspace --features whisper/test-hooks   # unit and end-to-end tests
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask scan                                       # no secrets, e-mail addresses or local paths
```

Try your build against a scratch profile, so your real Whisper profile stays as it is:

```sh
cargo xtask bundle
WHISPER_SHARE=build/share WHISPER_DATA=/tmp/whisper-try cargo run -p whisper
```

The end-to-end tests never start a real browser: a copy of the test build named `firefox`
records the arguments it was given, and a small local server stands in for GitHub and
addons.mozilla.org.

## Ground rules

- **Nothing new phones home.** A change to `profile/user.js` must not make the browser contact a
  new server, and every pref gets a comment saying what it does.
- **Tests come with behaviour changes.** Profile setup, imports and updates are covered end to end
  in `crates/whisper/tests/`; please keep it that way.
- **Few, well-known dependencies.** Each new crate should earn its place; `cargo deny` checks
  advisories and licenses in CI.
- **Small, focused pull requests** with a clear description. CI has to be green before merging.

By contributing you agree that your contribution is licensed under the
[Mozilla Public License 2.0](LICENSE).
