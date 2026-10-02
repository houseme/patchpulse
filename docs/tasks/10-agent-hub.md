# Task 10: Agent and Hub Aggregation

Requirements: approved v0.3 roadmap; depends on Tasks 03-06.

## Implementation

1. Add an explicit agent/hub mode with agent as the default. Hub mode does not
   run local Windows collectors. Validate configured unique agent IDs, HTTP(S)
   origins, polling durations, response limits and bounded concurrency.
2. Agent GET `/snapshot` returns one versioned coherent inventory with backend
   diagnostics, observation time and the configured freshness threshold.
3. Poll only administrator-configured URLs on a skip-on-missed-tick schedule.
   Reuse one HTTP client, enforce request/body limits, disable redirects and
   retain each failed agent's last successful snapshot and error independently.
4. Publish immutable fleet state. GET `/agents`, `/agents/{id}/snapshot` and
   `/fleet/summary` retain machine identity; never deduplicate KBs across hosts.
5. Hub readiness follows the first genuine remote snapshot. Fleet health exposes
   missing, transport-failed and source-stale agents. Cancellation stops polling.
6. Test real loopback agents, coherent wire parsing, partial/all failures,
   recovery, readiness, invalid configuration, limits and HTTP read-only behavior.

## Acceptance

No HTTP request can configure targets or trigger collection. Agent data remains
scoped to its configured ID. HTTPS validates certificates; credentials come from
configured environment variable names and never appear in responses or logs.
The agent remains usable independently. Live Windows/remote acceptance is separate.
