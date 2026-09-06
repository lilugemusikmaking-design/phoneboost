#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'PHONEBOOST_USER_UNINSTALL FAIL: %s\n' "$1" >&2
    exit 1
}

[[ "$#" -eq 0 ]] \
    || fail "usage: scripts/uninstall_phoneboost_control_center_user.sh"
[[ -n "${HOME:-}" && "$HOME" == /* ]] || fail "HOME must be an absolute path"
install_prefix=${PHONEBOOST_INSTALL_PREFIX:-"$HOME/.local"}
[[ "$install_prefix" == /* ]] || fail "installation prefix must be absolute"
case "$install_prefix" in
    *[!A-Za-z0-9_./-]*)
        fail "installation prefix contains unsupported characters"
        ;;
esac
command -v realpath >/dev/null 2>&1 \
    || fail "required command unavailable: realpath"
canonical_home="$(realpath -e -- "$HOME")" \
    || fail "HOME cannot be resolved"
install_prefix="$(realpath -m -- "$install_prefix")" \
    || fail "installation prefix cannot be resolved"
[[ "$install_prefix" != "/" && "$install_prefix" != "$canonical_home" ]] \
    || fail "installation prefix is too broad"

application_root="$install_prefix/share/phoneboost"
launcher="$install_prefix/bin/phoneboost-control-center"
desktop_entry="$install_prefix/share/applications/org.phoneboost.ControlCenter.desktop"

rm -f -- "$launcher" "$desktop_entry"
if [[ -d "$application_root" ]]; then
    [[ "$application_root" == "$install_prefix/share/phoneboost" ]] \
        || fail "refusing to remove an unexpected application path"
    rm -rf -- "$application_root"
fi

rmdir -- "$install_prefix/share/applications" 2>/dev/null || true
rmdir -- "$install_prefix/share" 2>/dev/null || true
rmdir -- "$install_prefix/bin" 2>/dev/null || true
rmdir -- "$install_prefix" 2>/dev/null || true

printf '%s\n' \
    'PHONEBOOST_USER_UNINSTALL PASS' \
    'Installed application files were removed.' \
    'Pairing state, configuration, logs, and recorded evidence were preserved.'
