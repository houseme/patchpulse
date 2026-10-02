# Task 12: Patch Baseline Comparison

Requirements: approved v0.5 roadmap; depends on Tasks 02, 04, 05, 10.

## Implementation

1. Add disabled-by-default named KB baseline configuration with strict numeric
   KB normalization and deterministic deduplication; reject an enabled empty list.
2. GET `/patches/baseline` compares the configured requirement against installed
   and pending inventory in one publication. Report installed, missing and pending
   KBs, counts, observation time, stale state and an explicit compliance result.
3. Return unknown compliance before readiness or when source data is stale; never
   claim compliance from retained or incomplete inventory. Comparison is exact KB
   membership and does not infer supersedence, vulnerability or security coverage.
4. Hub per-agent and fleet comparison preserves machine identity and accounts for
   unreachable/missing agents as unknown, with no request-driven baseline mutation.
5. Validate compliant/non-compliant/unknown states, pending distinction,
   normalization, disabled routes, writes and partial-failure retention.

## Acceptance

Every conclusion identifies the named baseline and input freshness. Existing
inventory fields are unchanged. No baseline action installs or approves updates.

## Implementation record

Agent configuration/domain/router comparison is implemented and real-router
contracts cover unknown, pending/non-compliant, fresh compliant and retained
stale states plus disabled/write rejection. Task 10 adds per-agent/fleet reports,
including transport-failure and missing-agent unknown states.
