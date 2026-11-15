# Android decrypt qualification

- Branch: spike/android-decrypt-v0-1
- Base: 5446240bedb630be93bdec4f619afee7a6b781c7 (#35 merge commit; merged verified).
- Only writer: root. Historical worktrees/branches preserved.
- Audit: Snow 0.9.6 / ChaCha20Poly1305 0.10.1 / ChaCha20 0.9.1 soft / Poly1305 0.8.0 soft on ARM64.
- Candidate: existing chacha20_force_neon cfg, benchmark only until physical validation.
- No product changes yet. No protocol, framing, polling, ResourceGuard, RAM, GPU/NPU or affinity changes.
- Physical initial Saver ON; operator manually OFF; fresh Health OFF confirmed. Restoration journal /tmp/phoneboost-android-decrypt-20261115/power-state.json. Must restore ON and verify Health at end.
- Host-generated fixture and standalone Android benchmark in preparation; no benchmark PASS claimed yet.
- NEXT_ACTION: build and measure isolated baseline; only then compare existing NEON backend.
