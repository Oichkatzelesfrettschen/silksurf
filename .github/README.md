# CI Policy

silksurf runs NO automatic cloud CI on push or pull_request. The merge
gate is local (AD-009, `docs/design/ARCHITECTURE-DECISIONS.md`):
`scripts/local_gate.sh full`, wired into the pre-push git hook by
`scripts/install-git-hooks.sh`.

`ci.yml` is the only workflow. It is `workflow_dispatch`-only and
exists as a discoverability surface: running it prints the local-gate
commands. It gates nothing.

Run the gate yourself:

```sh
scripts/install-git-hooks.sh        # one-time hook setup
scripts/local_gate.sh fast          # pre-commit equivalent (make check)
scripts/local_gate.sh full          # pre-push equivalent (make full)
MIRI=1 scripts/local_gate.sh full   # add miri smoke
FUZZ=1 scripts/local_gate.sh full   # add fuzz smoke (30s per target)
```

Reference: `docs/development/LOCAL-GATE.md`.
