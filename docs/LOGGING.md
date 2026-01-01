# Logging & Error Reporting

## Crates
- Logging/tracing: `tracing`, `tracing-subscriber`.
- Error types: `thiserror` for structured errors, `anyhow` for fallible CLIs.

## Guidance
- Use `tracing::info!`/`warn!`/`error!` for pipeline milestones and failures.
- Prefer typed errors in libraries; use `anyhow::Result` only at binary edges.
- Include context (URL, node id, selector) in errors to help debugging.

## Runtime Configuration
```
RUST_LOG=silksurf_engine=info,silksurf_css=debug cargo run -p silksurf-app
```

## TODO
- Add structured error reporting hooks for the GUI shell.
