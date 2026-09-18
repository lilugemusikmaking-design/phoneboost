# P1 physical validation — Galaxy A15

- Date: 2026-09-18T22:12:10.152469+00:00
- Branch: `chore/pipeline-p1-user-tests`
- Baseline HEAD: `9acdaea066699e0f0355dd0b1172d965e79bcf59`
- Device: `RZ8X500LL3A`, Samsung `SM-A155F`, ADB `device`

## Results

- Android targeted gate: PASS (`assembleDebug`, lease-state 5/5, SAS display 4/4, lint).
- Maestro physical: PASS 2/2 in 50 s.
- C05/C06: PASS; six-digit SAS matched with digits suppressed, mutual confirmation, persisted trust, ping/pong, restart reload, and IK reconnect.
- Remote regression: PASS; authenticated worker, active lease, admission/readiness proof, exact remote BLAKE3 result, controlled Wi-Fi loss, explicit local fallback without false remote success, authenticated reconnect, and second exact remote success.
- Wi-Fi restored and temporary daemon stopped.
- Blockers: none.

## Local evidence

- `.tooling/p1/maestro-results/junit.xml`
- `.tooling/p1/maestro-home/.maestro/tests/2026-09-19_000819/maestro.log`

The local evidence directory is ignored by Git. No SAS digits, private keys, raw peer state, or endpoint were committed.

## Next action

STOP. Do not start P2 or merge master without a new instruction.
