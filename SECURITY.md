# Security Policy

> A web browser parses untrusted bytes from the public internet. silksurf takes
> security seriously even at this early stage.

## Reporting a vulnerability

Email: open an issue on GitHub with the `security` label OR contact the
maintainers privately if the vulnerability is exploitable in the wild. Do
not file public issues for unfixed exploitable bugs.

We aim to respond within 7 calendar days. Coordinated disclosure timelines
will be agreed case-by-case.

## Scope

In scope:

  * Memory safety violations in any silksurf-* crate.
  * TLS validation bypasses in `silksurf-tls`.
  * Cache poisoning or path-traversal in the persistent on-disk response
    cache (`silksurf-net::cache`).
  * Sandbox-escape paths from `silksurf-js` into host APIs not exposed
    through the explicit JS-DOM bridge.
  * Parser-level denial-of-service (algorithmic complexity, unbounded
    allocation) in `silksurf-html`, `silksurf-css`, `silksurf-js`.
  * Supply-chain attacks via dependency hijack.

Out of scope (for now):

  * Site-isolation gaps -- privacy/sandboxing is on the roadmap, not
    implemented yet (see SNAZZY-WAFFLE roadmap P8.S9).
  * Third-party-cookie policy -- no cookie subsystem yet.
  * Speculative-execution side channels.

## Configuration & operational notes

### TLS

  * `silksurf-tls` uses `rustls` exclusively. `openssl`/`native-tls` are
    banned by `deny.toml`.
  * Root store loading defaults to the platform verifier; supplemental CAs
    can be provided via the `--tls-ca-file <path>` CLI flag for
    `silksurf-app` (see `docs/development/RUNBOOK-TLS-PROBE.md` once
    landed).
  * Use `cargo run --release --bin tls-probe -- <hostname>` to diagnose
    handshake failures.

### Secrets

  * Never commit API tokens, cookies, or local test credentials.
  * Use environment variables for local testing inputs.
  * The persistent cache directory (`$XDG_CACHE_HOME/silksurf/http`) may
    contain response bodies; treat as sensitive for sites that set
    `Cache-Control: private`.

### Inputs

  * Treat HTML/CSS/JS as untrusted input. Fuzz targets live under `fuzz/`
    and should be exercised on any parser change (see
    `docs/development/LOCAL-GATE.md` -- `FUZZ=1` mode).
  * Resource bounds (max-rule-count, max-DOM-depth, parser fuel, TLS
    handshake timeouts, fetch size limits, JS stack-depth limits) are
    being audited in roadmap P8.S8.

## Supply-chain hygiene

  * `cargo deny check advisories bans licenses sources` runs in
    `local_gate.sh full`.
  * Banned crates: `openssl-sys`, `git2`, `atty`, `ansi_term` (see
    `deny.toml`).
  * SBOM (CycloneDX) generation is planned for P9 release work.

## Threat model

A formal STRIDE pass is being written (see roadmap P2.S5 / P8 -- target file
`docs/design/THREAT-MODEL.md`). Until it lands, treat this document as the
working summary.
