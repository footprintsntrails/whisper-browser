#!/usr/bin/env bash
# Builds Whisper's guard engine (crates/wcore_wasm: the Rust download and protocol
# checks) as WebAssembly and copies it to OUT_FILE.
#
# prepare.sh puts it into the source tree for the build
# (browser/components/whisper/security/jar.mn packages it); repack-ui.py builds it for
# a repack. Uses the toolchain pinned in rust-toolchain.toml, with its wasm32 target
# (installed here when rustup is there and it is missing).
#
# Usage: build-wasm.sh OUT_FILE
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:?usage: build-wasm.sh OUT_FILE}
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")
target=wasm32-unknown-unknown

if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --toolchain "$channel" --installed 2>/dev/null | grep -qx "$target"; then
    rustup toolchain install "$channel" --profile minimal --no-self-update >&2
    rustup target add --toolchain "$channel" "$target" >&2
  fi
  cargo=(cargo "+$channel")
else
  cargo=(cargo)
fi
# Its own target folder, so that a build running inside mach's environment
# (CARGO_TARGET_DIR, RUSTFLAGS for the host) does not mix with this one.
target_dir="$repo/target/guard-engine"
(cd "$repo" && env -u RUSTFLAGS -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET \
  "${cargo[@]}" build --locked --release -p wcore_wasm --target "$target" --target-dir "$target_dir" >&2)
built="$target_dir/$target/release/wcore_wasm.wasm"
# The module must import nothing (no files, no network, no browser): it may not
# have an import section (section id 2).
python3 - "$built" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
assert data[:4] == b"\0asm", "not a WebAssembly module"
i = 8
while i < len(data):
    section = data[i]
    i += 1
    size, shift = 0, 0
    while True:
        b = data[i]
        i += 1
        size |= (b & 0x7F) << shift
        shift += 7
        if b < 0x80:
            break
    if section == 2:
        sys.exit("the guard engine imports something; it must import nothing")
    i += size
PY
mkdir -p "$(dirname "$out")"
cp "$built" "$out"
echo "guard engine: $(wc -c < "$out") bytes -> $out" >&2
