# User-supplied normative excerpt — HOST_STARTUP

Source: operator message in F1/F2 session, 2026-09-16. This is a scoped excerpt, not a replacement for the external canonical originals. Authority remains SPEC V0.7 > Contract Set V1.3 > Tech Sheet V1.3 > Pseudocode V1.1 > Fixture Generation Spec V1.0.

Contract Set V1.3 — C02 Linux Host Runtime & Local Authority
- Stale socket : lstat via parent validé.
- non-socket / mauvais owner / symlink => STARTUP_REFUSED.
- socket same-UID : tentative de connexion.
- si connexion réussit => ALREADY_RUNNING.
- si connexion refusée et inode stable après recheck => unlinkat puis bind.
- stale recovery MUST revalider type / owner / inode avant unlink.
- socket stale same-UID récupéré uniquement après preuve ; une instance active ne doit jamais être supprimée.

Tech Sheet V1.3 §4.3 — Stale control.sock Recovery Algorithm
1. Valider XDG_RUNTIME_DIR et le parent phoneboost.
2. Ouvrir le parent sans suivre les symlinks et conserver son FD.
3. fstatat/lstat control.sock avec NOFOLLOW.
4. absent => bind normal.
5. non-socket / mauvais owner / symlink => STARTUP_REFUSED.
6. same-UID socket => tentative connect ; succès => ALREADY_RUNNING; refus => re-stat via le FD parent.
7. comparer type + owner + device/inode avec la première observation.
8. uniquement si inchangé, unlinkat(parent_fd,"control.sock"), puis bind frais.
9. chmod 0600, listen ; aucun fallback /tmp.
Décision d’implémentation : utiliser une famille Unix bas niveau (rustix ou nix) et tester le remplacement concurrent entre les deux stat.

Pseudocode V1.1 HOST_STARTUP
HS-004 OPEN parent directory FD without following symlinks
HS-005 STAT control.sock with NOFOLLOW
HS-008 ATTEMPT connect existing socket
HS-010 RESTAT via parent FD
HS-011 IF type/owner/dev/inode changed THEN STOP STARTUP_REFUSED
HS-012 IF stale socket proven unchanged THEN unlinkat(parent_fd,"control.sock")

Operator explicitly permits O_PATH as strengthening of the stability proof only if all canonical checks and authority semantics are retained. No filesystem masking, weakened tests, or alternative authority semantics permitted.
