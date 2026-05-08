from __future__ import annotations


def split_sqlplus_script(script: str) -> list[str]:
    """Split a small SQL*Plus-style script on slash-only block terminators."""

    statements: list[str] = []
    current: list[str] = []
    for raw_line in script.splitlines():
        if raw_line.strip() == "/":
            statement = "\n".join(current).strip()
            if statement:
                statements.append(statement)
            current = []
        else:
            current.append(raw_line.rstrip())

    trailing = "\n".join(current).strip()
    if trailing:
        statements.append(trailing)
    return statements
