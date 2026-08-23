#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_tooling="/home/mimir/phoneboost_ftest02_android_v1_work/.tooling"
android_sdk="${shared_tooling}/android-sdk"
ndk_root="${android_sdk}/ndk/29.0.14206865"
java_root="${shared_tooling}/java"
rust_toolchain="${shared_tooling}/rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu"
cargo_bin="${shared_tooling}/cargo/bin/cargo"
gradle_bin="${shared_tooling}/gradle/gradle-9.5.0/bin/gradle"
native_output="${workspace_root}/.work/a5/cargo-target/aarch64-linux-android/release/libphoneboost_core_jni.so"
jni_libs="${workspace_root}/.work/a5/jniLibs/arm64-v8a"

export JAVA_HOME="${java_root}"
export ANDROID_HOME="${android_sdk}"
export ANDROID_SDK_ROOT="${android_sdk}"
export ANDROID_USER_HOME="${workspace_root}/.tooling/android-user-home"
export GRADLE_USER_HOME="${shared_tooling}/gradle-user-home"
export RUSTUP_HOME="${shared_tooling}/rustup"
export CARGO_HOME="${workspace_root}/.tooling/cargo"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="${workspace_root}/.work/a5/cargo-target"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android29-clang"
export CC_aarch64_linux_android="${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER}"
export AR_aarch64_linux_android="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
export PATH="${java_root}/bin:${rust_toolchain}/bin:/usr/bin:/bin"

mkdir -p "${jni_libs}" "${ANDROID_USER_HOME}"
"${cargo_bin}" build --locked --release --target aarch64-linux-android \
    -p phoneboost-core-jni --features jni-test-probes
cp "${native_output}" "${jni_libs}/libphoneboost_core_jni.so"

"${gradle_bin}" --offline --no-daemon -p "${workspace_root}/android" :app:assembleDebug

test -f "${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk"
printf '%s\n' "A5 Android build PASS"

