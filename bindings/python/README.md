# pdb-branch Python Binding

Python control-plane binding for `pdb-branch`.

Install from this directory:

```bash
python -m pip install -e '.[dev]'
python -m pytest
```

Use `pdb_branch.BranchClient` with a privileged connection to `CDB$ROOT`.

Run the Oracle Free integration harness from the repo root:

```bash
scripts/run-oracle-free-integration.sh
```

The harness creates a repo-local `.venv-integration` virtualenv and reuses the
named Oracle Free container by default.
