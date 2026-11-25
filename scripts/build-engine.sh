#!/usr/bin/env bash
# Builds the Rust layout engine for the web app:
#   - the WebAssembly package    -> apps/web/src/lib/engine/pkg
#   - matching TypeScript types  -> apps/web/src/lib/engine/bindings
#     (for the engine and for the server's API)
# Both outputs are generated and git-ignored. Run via `npm run engine` in
# apps/web, which puts the project-local wasm-pack on PATH.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/apps/web/src/lib/engine"

# Prefer a rustup-managed toolchain when there is one. A Homebrew or distro
# Rust cannot install the wasm32 target.
if [ -x "$HOME/.cargo/bin/cargo" ]; then
	export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! rustc --print target-libdir --target wasm32-unknown-unknown 2>/dev/null | xargs test -d; then
	echo "The wasm32-unknown-unknown Rust target is missing." >&2
	echo "Install it with: rustup target add wasm32-unknown-unknown" >&2
	exit 1
fi

profile="${1:---release}"

rm -rf "$out/pkg" "$out/bindings"
wasm-pack build "$root/crates/wain-wasm" "$profile" \
	--target web --no-pack --out-dir "$out/pkg" --out-name wain_wasm

# ts-rs writes one .ts file per type from tests named export_bindings_*:
# the engine's types from wain-core and the API's from wain-server. Those
# tests touch no database, so none needs to be running.
TS_RS_EXPORT_DIR="$out/bindings" \
	cargo test --quiet --manifest-path "$root/Cargo.toml" \
	-p wain-core -p wain-server --features wain-core/ts,wain-server/ts export_bindings
