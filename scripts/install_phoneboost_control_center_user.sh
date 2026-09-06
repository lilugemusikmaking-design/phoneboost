#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'PHONEBOOST_USER_INSTALL FAIL: %s\n' "$1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command unavailable: $1"
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repository_root"

build=true
if [[ "${1:-}" == "--no-build" ]]; then
    build=false
    shift
fi
[[ "$#" -eq 0 ]] \
    || fail "usage: scripts/install_phoneboost_control_center_user.sh [--no-build]"

[[ -n "${HOME:-}" && "$HOME" == /* ]] || fail "HOME must be an absolute path"
install_prefix=${PHONEBOOST_INSTALL_PREFIX:-"$HOME/.local"}
[[ "$install_prefix" == /* ]] || fail "installation prefix must be absolute"
case "$install_prefix" in
    *[!A-Za-z0-9_./-]*)
        fail "installation prefix contains unsupported characters"
        ;;
esac
require_command realpath
canonical_home="$(realpath -e -- "$HOME")" \
    || fail "HOME cannot be resolved"
install_prefix="$(realpath -m -- "$install_prefix")" \
    || fail "installation prefix cannot be resolved"
[[ "$install_prefix" != "/" && "$install_prefix" != "$canonical_home" ]] \
    || fail "installation prefix is too broad"

rust_198_bin="$HOME/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin"
if [[ "$build" == true ]]; then
    [[ -x "$rust_198_bin/cargo" ]] \
        || fail "Rust 1.98 cargo unavailable at $rust_198_bin/cargo"
    PATH="$rust_198_bin:$PATH"
    export PATH
    require_command cargo
    require_command yarn

    printf '%s\n' 'Building the checked-in production frontend and PhoneBoost binaries...'
    REACT_APP_BACKEND_URL= yarn --cwd frontend build
    cargo build --release \
        -p pb-host --bin phoneboostd \
        -p pb-cli --bin phoneboostctl \
        -p pb-web-bridge --bin phoneboost-web-bridge
fi

for binary in phoneboostctl phoneboostd phoneboost-web-bridge; do
    [[ -x "target/release/$binary" ]] \
        || fail "production binary is unavailable: $binary"
done
[[ -f frontend/build/index.html ]] || fail "production frontend build is unavailable"
[[ -x scripts/run_phoneboost_control_center.sh ]] \
    || fail "repository Control Center launcher is unavailable"
[[ -x packaging/linux/phoneboost-control-center ]] \
    || fail "installed-launcher source is unavailable"
[[ -f packaging/linux/org.phoneboost.ControlCenter.desktop.in ]] \
    || fail "desktop-entry template is unavailable"

bin_dir="$install_prefix/bin"
data_dir="$install_prefix/share"
applications_dir="$data_dir/applications"
application_root="$data_dir/phoneboost"

mkdir -p -- "$bin_dir" "$applications_dir"
staging_root=""
backup_root=""
launcher_temp=""
desktop_temp=""
install_committed=false
application_install_attempted=false
launcher_install_attempted=false
desktop_install_attempted=false

cleanup() {
    local rollback_failed=false
    set +e
    [[ -z "$staging_root" || ! -e "$staging_root" ]] || rm -rf -- "$staging_root"
    if [[ "$install_committed" != true ]]; then
        if [[ -n "$backup_root" && -e "$backup_root/application" ]]; then
            rm -rf -- "$application_root" \
                && mv -- "$backup_root/application" "$application_root" \
                || rollback_failed=true
        elif [[ "$application_install_attempted" == true ]]; then
            rm -rf -- "$application_root"
        fi
        if [[ -n "$backup_root" && -e "$backup_root/launcher" ]]; then
            rm -f -- "$bin_dir/phoneboost-control-center" \
                && mv -- "$backup_root/launcher" "$bin_dir/phoneboost-control-center" \
                || rollback_failed=true
        elif [[ "$launcher_install_attempted" == true ]]; then
            rm -f -- "$bin_dir/phoneboost-control-center"
        fi
        if [[ -n "$backup_root" && -e "$backup_root/desktop" ]]; then
            rm -f -- "$applications_dir/org.phoneboost.ControlCenter.desktop" \
                && mv -- "$backup_root/desktop" \
                    "$applications_dir/org.phoneboost.ControlCenter.desktop" \
                || rollback_failed=true
        elif [[ "$desktop_install_attempted" == true ]]; then
            rm -f -- "$applications_dir/org.phoneboost.ControlCenter.desktop"
        fi
    fi
    if [[ "$rollback_failed" == true ]]; then
        printf 'PHONEBOOST_USER_INSTALL WARNING: rollback backup retained at %s\n' \
            "$backup_root" >&2
    elif [[ -n "$backup_root" && -e "$backup_root" ]]; then
        rm -rf -- "$backup_root"
    fi
}
trap cleanup EXIT

staging_root="$(mktemp -d "$data_dir/.phoneboost-install.XXXXXX")"
backup_root="$(mktemp -d "$data_dir/.phoneboost-backup.XXXXXX")"

mkdir -p -- \
    "$staging_root/libexec" \
    "$staging_root/scripts" \
    "$staging_root/frontend/build"
install -m 0755 target/release/phoneboostctl "$staging_root/libexec/phoneboostctl"
install -m 0755 target/release/phoneboostd "$staging_root/libexec/phoneboostd"
install -m 0755 target/release/phoneboost-web-bridge \
    "$staging_root/libexec/phoneboost-web-bridge"
install -m 0755 scripts/run_phoneboost_control_center.sh \
    "$staging_root/scripts/run_phoneboost_control_center.sh"
cp -a frontend/build/. "$staging_root/frontend/build/"

launcher_temp="$backup_root/new-launcher"
desktop_temp="$backup_root/new-desktop-entry"
trap 'rm -f -- "$launcher_temp" "$desktop_temp"; cleanup' EXIT
install -m 0755 packaging/linux/phoneboost-control-center "$launcher_temp"
sed "s|@PHONEBOOST_CONTROL_CENTER@|$bin_dir/phoneboost-control-center|g" \
    packaging/linux/org.phoneboost.ControlCenter.desktop.in >"$desktop_temp"
chmod 0644 "$desktop_temp"

[[ ! -e "$application_root" ]] \
    || mv -- "$application_root" "$backup_root/application"
[[ ! -e "$bin_dir/phoneboost-control-center" ]] \
    || mv -- "$bin_dir/phoneboost-control-center" "$backup_root/launcher"
[[ ! -e "$applications_dir/org.phoneboost.ControlCenter.desktop" ]] \
    || mv -- "$applications_dir/org.phoneboost.ControlCenter.desktop" \
        "$backup_root/desktop"

application_install_attempted=true
mv -- "$staging_root" "$application_root"
staging_root=""
launcher_install_attempted=true
mv -- "$launcher_temp" "$bin_dir/phoneboost-control-center"
desktop_install_attempted=true
mv -- "$desktop_temp" "$applications_dir/org.phoneboost.ControlCenter.desktop"

install_committed=true
rm -rf -- "$backup_root"
trap - EXIT

printf '%s\n' \
    'PHONEBOOST_USER_INSTALL PASS' \
    "Command: $bin_dir/phoneboost-control-center" \
    "Menu entry: $applications_dir/org.phoneboost.ControlCenter.desktop" \
    'No daemon or Control Center was started. No autostart service was installed.'
