# PhoneBoost A4 Runtime Scaffolding

This repository is in the A4 scaffolding phase for the non-production General
POC. The imported A1-A3 crates and protocol fixtures are frozen and must remain
byte-identical to `inputs/phoneboost_fixture_bootstrap_v1_1.zip`.

Pass 1 contains only the frozen baseline, repository structure, dependency
provenance, and the low-level Unix API micro-gate. It does not implement
`HOST_STARTUP`, `LOCAL_CLIENT_ACCEPT`, or a functional C12 server.
