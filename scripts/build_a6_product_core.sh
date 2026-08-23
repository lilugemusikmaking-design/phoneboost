#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_tooling="/home/mimir/phoneboost_ftest02_android_v1_work/.tooling"
android_sdk="${shared_tooling}/android-sdk"
ndk_root="${android_sdk}/ndk/29.0.14206865"
rust_toolchain="${shared_tooling}/rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu"
cargo_bin="${shared_tooling}/cargo/bin/cargo"
product_so="${workspace_root}/.work/a6-product/aarch64-linux-android/release/libphoneboost_core_jni.so"

export RUSTUP_HOME="${shared_tooling}/rustup"
export CARGO_HOME="${workspace_root}/.tooling/cargo"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="${workspace_root}/.work/a6-product"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android29-clang"
export CC_aarch64_linux_android="${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER}"
export AR_aarch64_linux_android="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
export PATH="${rust_toolchain}/bin:/usr/bin:/bin"

"${cargo_bin}" build --locked --release --target aarch64-linux-android \
    -p phoneboost-core-jni
"${workspace_root}/scripts/scan_a6_product.py" "${product_so}"
printf '%s\n' "A6 product core build PASS: ${product_so}"
