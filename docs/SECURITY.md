# Security & Configuration Notes

## TLS
- `silksurf-tls` uses `rustls`; root store loading is pending.
- Do not hardcode cert bundles; load from OS trust store.

## Secrets
- Never commit API tokens, cookies, or local test credentials.
- Use environment variables for local testing inputs.

## Inputs
- Treat HTML/CSS/JS as untrusted input.
- Fuzz targets live under `fuzz/` and should be run on new parsers.
