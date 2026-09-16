# Invariants

One writer per branch/worktree. CLI mutations hold an advisory Git-directory lock; all writers must honor this policy. Never work on main/master or merge automatically. Never change PhoneBoost runtime, canonical invariants, UI ZIPs or physical evidence for tooling convenience.

SOFTWARE/CI PROOF and PHYSICAL HARDWARE PROOF are independent. CI/checkpoints cannot assign READY/LIVE/PASS to hardware evidence. Missing tools or failed checks are failures, not passes. Checkpoints can preserve failing work and are never approval to merge.

No mandatory AI service. No secrets in state, logs, commits or review requests. Gitleaks redacts output and fails closed. Checkpoint paths are explicit; ignored files are never force-added. A failed push leaves a recoverable local commit. Resume is read-only.
