# Task 04: Scheduling, Atomic Snapshots, and Failure Degradation

Requirements: F7. Dependencies: 02, 03.

## Implementation steps

1. Run collectors concurrently with independent timeouts and one scheduler as the only snapshot writer.
2. Retain per-backend last successful records and timestamps; a failed backend must never clear its previous records.
3. Accept successful empty collections as authoritative. Distinguish an empty success from a failed collection.
4. Publish an atomic aggregate snapshot after each cycle; partial successes update healthy sources while recording degradation.
5. Keep historical readiness after any successful collection; initial failure returns unavailable. Staleness includes retained failed backend data.
6. Collect immediately once, then use skip-on-missed-tick scheduling without a duplicate startup run. Drain HTTP and stop scheduling on shutdown.

## Acceptance criteria

- Tests cover all-failed, partial-failed, empty-success, first-failure, recovery, retained records, readiness, stale age, timeout, and single-flight behavior.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`src/cache/mod.rs`, `src/collector/orchestrator.rs`, `src/scheduler/mod.rs`, and `tests/snapshot_pipeline.rs`. Failure retention, recovery, valid empty results, reconciliation, startup scheduling, single-flight, and active-worker cancellation tests pass. Backend success timestamps reflect each collector finish time.
