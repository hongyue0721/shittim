#!/usr/bin/env bash
# Deterministic schema generation and contract checks (Rust-only; no Node).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_MANIFEST="${ROOT}/rust/Cargo.toml"
TOOL=(cargo run --manifest-path "${CARGO_MANIFEST}" -p schema-tool --)

echo "==> schema-tool generate (1/2)"
"${TOOL[@]}" --repo-root "${ROOT}" generate

echo "==> schema-tool generate (2/2)"
"${TOOL[@]}" --repo-root "${ROOT}" generate

echo "==> schema-tool check"
"${TOOL[@]}" --repo-root "${ROOT}" check

echo "==> cargo fmt --check"
cargo fmt --manifest-path "${CARGO_MANIFEST}" --all -- --check

echo "==> cargo clippy -D warnings"
cargo clippy --manifest-path "${CARGO_MANIFEST}" --workspace --all-targets -- -D warnings

echo "==> cargo test"
cargo test --manifest-path "${CARGO_MANIFEST}" --workspace

echo "==> generated tree drift"
if ! git -C "${ROOT}" diff --exit-code -- rust/crates/kernel-contracts/src/generated; then
  echo "generated Rust files drifted; run schema-tool generate and commit the result" >&2
  exit 1
fi

echo "check-schema: ok"
