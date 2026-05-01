# Maintenance Cron Suggestions

> silksurf has no cloud CI by policy (ADR-009). The local-gate runs on
> every push; the recurring sweeps below are the contributor's
> responsibility. Configure them as a personal cron (or systemd-timer)
> on your dev machine. Skipping them means dependency advisories and
> stale lockfiles will only surface during the next push -- catching
> them on a weekly cadence keeps the merge gate green.

## Weekly: dependency advisories + bans + outdated

```cron
# Every Monday at 09:00 local time.
0 9 * * 1 cd /path/to/silksurf && \
    cargo deny check advisories bans licenses sources 2>&1 | \
    tee -a ~/.silksurf-maint.log && \
    cargo audit 2>&1 | tee -a ~/.silksurf-maint.log && \
    cargo outdated --workspace --depth 1 2>&1 | \
    tee -a ~/.silksurf-maint.log
```

Read the log; bump deps via `cargo update -p <crate>` and re-run
`scripts/local_gate.sh full` before pushing.

## Nightly: bench history append (when P3.S2 history runner lands)

```cron
# Every night at 03:00.
0 3 * * * cd /path/to/silksurf && \
    cargo run --release --quiet \
        -p silksurf-engine --bin bench_pipeline >> ~/.silksurf-bench.log 2>&1
```

The current `bench_pipeline` output is human-readable; once the rolling
NDJSON history schema lands (P3.S2), append into
`perf/history.ndjson` instead so trend analysis is possible.

## Monthly: conformance re-run + corpus refresh

```cron
# First day of the month at 09:00.
0 9 1 * * cd /path/to/silksurf && \
    scripts/conformance_run.sh 2>&1 | tee -a ~/.silksurf-conformance.log
```

Diff the test262 scorecard against the prior month's run; investigate
any drop in pass rate.

## Quarterly: rotate + verify tokens

  * Verify any local cargo registry tokens, GitHub PATs, signing keys
    for cargo-dist (P9) are still valid.
  * Re-read `docs/design/THREAT-MODEL.md` for any subsystem rows that
    have changed.

## Ad-hoc

  * `cargo update --dry-run` -- preview minor / patch bumps without
    applying. Useful before a roadmap-wave boundary.
  * `cargo tree -d` -- enumerate duplicate dep versions (the
    `cargo-deny bans` policy currently allows duplicates with `warn`;
    review periodically).
  * `cargo bloat --release` -- per-crate code-size attribution. Useful
    when binary size grows unexpectedly.

## Why no cloud CI cron

ADR-009 captures the strict-local-only CI policy. The same reasoning
applies to scheduled jobs: cron-on-cloud is invisible to the developer
and fails open (jobs silently get skipped). Local cron is your machine,
your responsibility, your timezone, and the failures show up next
morning in `~/.silksurf-maint.log` where you can act on them
immediately.

If a contributor wants to surface aggregate maintenance metrics across
the team, the right path is to publish a tiny dashboard from each
contributor's `~/.silksurf-*.log`, not to add cron-on-GitHub.
