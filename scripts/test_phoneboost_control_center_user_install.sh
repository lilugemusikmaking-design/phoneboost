#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'PHONEBOOST_USER_INSTALL_TEST FAIL: %s\n' "$1" >&2
    exit 1
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repository_root"

temporary_root="$(mktemp -d)"
trap 'rm -rf -- "$temporary_root"' EXIT
test_home="$temporary_root/home"
test_prefix="$test_home/.local"
mkdir -p -- "$test_home"

HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="$test_prefix" \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null

# Exercise the update path as well as a first installation. The installed
# command and payload must remain complete, with no transaction debris.
HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="$test_prefix" \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null

for relative in \
    bin/phoneboost-control-center \
    share/applications/org.phoneboost.ControlCenter.desktop \
    share/phoneboost/libexec/phoneboostctl \
    share/phoneboost/libexec/phoneboostd \
    share/phoneboost/libexec/phoneboost-web-bridge \
    share/phoneboost/scripts/run_phoneboost_control_center.sh \
    share/phoneboost/frontend/build/index.html; do
    [[ -e "$test_prefix/$relative" ]] || fail "installed file missing: $relative"
done

cmp target/release/phoneboostctl "$test_prefix/share/phoneboost/libexec/phoneboostctl"
cmp target/release/phoneboostd "$test_prefix/share/phoneboost/libexec/phoneboostd"
cmp target/release/phoneboost-web-bridge \
    "$test_prefix/share/phoneboost/libexec/phoneboost-web-bridge"
cmp frontend/build/index.html "$test_prefix/share/phoneboost/frontend/build/index.html"

grep -Fqx "Exec=$test_prefix/bin/phoneboost-control-center" \
    "$test_prefix/share/applications/org.phoneboost.ControlCenter.desktop"
grep -Fqx 'Terminal=true' \
    "$test_prefix/share/applications/org.phoneboost.ControlCenter.desktop"

if find "$test_prefix/share" -maxdepth 1 \
    \( -name '.phoneboost-install.*' -o -name '.phoneboost-backup.*' \) \
    -print -quit | grep -q .; then
    fail "installation transaction debris remains"
fi

if grep -ERn '#token=|[[:xdigit:]]{64}' \
    "$test_prefix/bin/phoneboost-control-center" \
    "$test_prefix/share/applications/org.phoneboost.ControlCenter.desktop" \
    "$test_prefix/share/phoneboost/scripts/run_phoneboost_control_center.sh" >/dev/null; then
    fail "installed launch metadata contains capability-like material"
fi

rollback_marker="$test_prefix/share/phoneboost/rollback-marker"
printf '%s\n' 'preserve-the-previous-installation' >"$rollback_marker"
fake_bin="$temporary_root/fake-bin"
mkdir -p -- "$fake_bin"
cat >"$fake_bin/mv" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
count=0
[[ ! -f "$PHONEBOOST_TEST_MV_COUNT" ]] \
    || read -r count <"$PHONEBOOST_TEST_MV_COUNT"
count=$((count + 1))
printf '%s\n' "$count" >"$PHONEBOOST_TEST_MV_COUNT"
if [[ "$count" -eq 6 ]]; then
    exit 73
fi
exec /usr/bin/mv "$@"
EOF
chmod 0755 "$fake_bin/mv"
if HOME="$test_home" \
    PHONEBOOST_INSTALL_PREFIX="$test_prefix" \
    PHONEBOOST_TEST_MV_COUNT="$temporary_root/mv-count" \
    PATH="$fake_bin:$PATH" \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null 2>&1; then
    fail "simulated interrupted update unexpectedly succeeded"
fi
grep -Fqx 'preserve-the-previous-installation' "$rollback_marker" \
    || fail "previous application payload was not restored after failure"
[[ -x "$test_prefix/bin/phoneboost-control-center" ]] \
    || fail "previous launcher was not restored after failure"
[[ -f "$test_prefix/share/applications/org.phoneboost.ControlCenter.desktop" ]] \
    || fail "previous desktop entry was not restored after failure"
if find "$test_prefix/share" -maxdepth 1 \
    \( -name '.phoneboost-install.*' -o -name '.phoneboost-backup.*' \) \
    -print -quit | grep -q .; then
    fail "failed update left transaction debris"
fi

pairing_marker="$test_home/.local/state/phoneboost/peers/preserved.json"
mkdir -p -- "$(dirname -- "$pairing_marker")"
printf '%s\n' '{"preserved":true}' >"$pairing_marker"

HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="$test_prefix" \
    scripts/uninstall_phoneboost_control_center_user.sh >/dev/null
[[ ! -e "$test_prefix/bin/phoneboost-control-center" ]] \
    || fail "launcher remains after uninstall"
[[ ! -e "$test_prefix/share/applications/org.phoneboost.ControlCenter.desktop" ]] \
    || fail "desktop entry remains after uninstall"
[[ ! -e "$test_prefix/share/phoneboost" ]] \
    || fail "application payload remains after uninstall"
grep -Fqx '{"preserved":true}' "$pairing_marker" \
    || fail "pairing state was not preserved by uninstall"

if HOME="$test_home" PHONEBOOST_INSTALL_PREFIX=/ \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null 2>&1; then
    fail "installer accepted root as its prefix"
fi
if HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="$test_home" \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null 2>&1; then
    fail "installer accepted HOME as its prefix"
fi
if HOME="$test_home" PHONEBOOST_INSTALL_PREFIX=relative \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null 2>&1; then
    fail "installer accepted a relative prefix"
fi
if HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="$test_home/.local/.." \
    scripts/install_phoneboost_control_center_user.sh --no-build >/dev/null 2>&1; then
    fail "installer accepted a prefix resolving to HOME"
fi
if HOME="$test_home" PHONEBOOST_INSTALL_PREFIX="/tmp/phoneboost/../../.." \
    scripts/uninstall_phoneboost_control_center_user.sh >/dev/null 2>&1; then
    fail "uninstaller accepted a prefix resolving to root"
fi

printf '%s\n' 'PHONEBOOST_USER_INSTALL_TEST PASS'
