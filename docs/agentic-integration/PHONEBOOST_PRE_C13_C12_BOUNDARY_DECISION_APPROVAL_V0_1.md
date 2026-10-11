# PHONEBOOST — PRE-C13 / C12 BOUNDARY DECISION APPROVAL V0.1

**Status:** APPROVED DIRECTION — CONTRACT/TEST WORK AUTHORIZED
**Date:** 2026-10-11
**Approver:** PhoneBoost project owner

## 1. Decision record

The project owner delegated technical delivery authority to the principal
development agent after reviewing the D1-D4 proposal in
`PHONEBOOST_PRE_C13_C12_BOUNDARY_DECISION_DRAFT_V0_1.md`.

The following directions are approved:

- **D1:** use the four normalized link classes `wifi_lan`, `usb_network`,
  `ethernet_ip`, and `local_ip_unknown`; exact `pb-link-v1:<class>` equality is
  a performance-profile compatibility boundary, never a trust or priority
  signal;
- **D2:** use authenticated-session-bound live measurements, a strict
  six-second freshness ceiling, at least three RTT samples, and at least one
  second of completed active transfer for any goodput direction consumed by
  C11; missing or stale evidence fails SPEED closed;
- **D3:** use the reviewed version-1 C13 JSON shape, private atomic persistence,
  exact 24-hour expiry, complete-key invalidation, and separate requester-path
  benchmark provenance;
- **D4:** use descriptor-based local ingress followed by a bounded
  PhoneBoost-owned immutable import object as the preferred future direction;
  never introduce a caller-provided path or generic execution route.

## 2. Authority granted by this approval

This approval authorizes:

1. exact machine-readable contracts and deterministic positive/negative tests;
2. separate implementation increments for D1/D2 and then C13;
3. a later exact C12 ingress contract and implementation after its byte,
   object-count, aggregate-quota, TTL and cancellation limits are fixed;
4. ordinary branch, checkpoint, CI and pull-request work needed to prove those
   increments.

It does not authorize bypassing repository branch protection or silently
merging a protected/default branch.

## 3. Continuing invariants

- C11 remains the only placement authority.
- C08/ResourceGuard remains the admission authority.
- adapters remain requesters above C12.
- pairing and SAS confirmation remain human-presence actions.
- link class grants no trust and creates no fixed preference.
- no filesystem path, shell, generic RPC, arbitrary network target, provider or
  arbitrary code execution surface is introduced.
- no MCP implementation starts before D1/D2 and C13 have independent green
  evidence and the C12 ingress contract is exact.
- the separate licensing/IP hold remains unchanged.

## 4. Exact artifacts

The machine-readable C13 candidate is:

`schemas/phoneboost-perf-profile-v1.schema.json`

Its independent standard-library checker and fixtures are part of the same
increment. A green checker proves schema/fixture consistency only; it does not
claim product persistence or runtime behavior.
