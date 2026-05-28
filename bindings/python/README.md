# pdb-branch Python Binding

Python control-plane binding for `pdb-branch`.

Install from this directory:

```bash
python -m pip install -e '.[dev]'
python -m pytest
```

Use `pdb_branch.BranchClient` with a privileged connection to `CDB$ROOT`.

```python
branches.create_branch("AGENT_RAG_042", from_pdb="GOLDEN_MASTER")

branches.clone_branch_from_remote(
    "AGENT_RAG_042",
    source_pdb="AGENT_RAG_042",
    source_db_link="PDB_BRANCH_ORIGIN",
    clone_mode="AUTO",  # FULL, AUTO, or SNAPSHOT
)
```

Remote clone calls run in the target CDB. The database link must already exist
there and point back to the source CDB. Use
`clone_branch_from_remote_with_result` when callers need to inspect whether
`AUTO` fell back to a full clone.

Run the Oracle Free integration harness from the repo root:

```bash
scripts/run-python-oracle-free-integration.sh
```

The harness creates a repo-local `.venv-integration` virtualenv and reuses the
named Oracle Free container by default.
