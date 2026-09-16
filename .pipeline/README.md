# Project pipeline

Run `scripts/project-pipeline doctor`, `status`, `check-fast`, `check-full`, or `resume` from any directory. Python 3.11+ and Git are required. Linux advisory locks protect mutations; no AI dependency. `check-full --component rust|frontend|android|pipeline` separates expensive suites. Full checks install locked frontend dependencies; they can require network. Tests never grant physical readiness.

Checkpoint example:

```sh
scripts/project-pipeline checkpoint --message "pipeline: phase complete" --next "Exact next command or action" --paths .pipeline scripts/project-pipeline
scripts/project-pipeline resume
```

Checkpoint refuses main/master, detached HEAD, mismatched writer branch and pre-existing staging. Paths are explicit; it stages state plus selected paths, scans the index contents and staged patch with Gitleaks, checks whitespace, commits, then pushes without force. On scan failure the index is preserved for inspection; unstage only the selected files after review. On push failure stop new work and run `git push -u origin HEAD` on the same branch, then `resume`; do not create replacement commits. Never use a force push.

STATE records the parent HEAD because a commit cannot contain its own hash. `resume` resolves the exact containing checkpoint commit from Git and compares live HEAD to origin; offline push status is unknown. An interrupted state write is atomic. In a crash before checkpoint, run status, record blockers/next action in STATE, scan/checkpoint the explicit paths, push and stop. Uncommitted work is not a durable remote backup.

Canonical paths can be registered in AUTHORITY.json with accepted SHA-256 hashes. Resume computes current hashes and reports changes without accepting them automatically. Originals not present are external/not-local. Unchanged documents do not trigger rereads or repeat audits.

Frontend has no dedicated lint script/config at the official checkpoint; build uses existing CRA/CRACO lint integration. Tests are required (no passWithNoTests). Android uses its existing AGP/SDK/NDK versions; no physical test scripts are invoked. No runtime files may be edited to green CI.
