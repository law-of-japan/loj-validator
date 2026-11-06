#!/usr/bin/env bash
# Run in an isolated official Rust 1.88 container, with /source read-only and
# /results writable. Build/test on the container's native filesystem for links.
set -euo pipefail
mkdir -p /work
tar -C /source --exclude=./target --exclude=./.git -cf - . | tar -C /work -xf -
cd /work
run_suite() {
    local toolchain="$1" name="$2"
    {
        rustc +"$toolchain" --version
        cargo +"$toolchain" --version
        LOJ_CONFORMANCE_REPORT="/results/${name}-conformance.json" \
            cargo +"$toolchain" test --workspace --locked
    } >"/results/${name}.log" 2>&1
}
run_suite 1.88.0 linux-msrv
rustup toolchain install stable --profile minimal > /results/linux-stable-install.log 2>&1
run_suite stable linux-stable
