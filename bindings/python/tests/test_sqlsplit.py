from pdb_branch.installer import read_script
from pdb_branch.sqlsplit import split_sqlplus_script


def test_split_sqlplus_script_uses_slash_terminators() -> None:
    script = """
CREATE TABLE demo (id NUMBER)
/

BEGIN
  NULL;
END;
/
"""

    assert split_sqlplus_script(script) == [
        "CREATE TABLE demo (id NUMBER)",
        "BEGIN\n  NULL;\nEND;",
    ]


def test_bundled_scripts_are_split_into_statements() -> None:
    for script_name in ("001_tables.sql", "002_package.sql"):
        script = read_script(script_name)
        statements = split_sqlplus_script(script)

        assert statements
        assert all("/" not in statement.splitlines() for statement in statements)
