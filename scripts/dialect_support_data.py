#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Load and join the dialect identity, support, and evaluation registries."""

from __future__ import annotations

import tomllib
from pathlib import Path

IDENTITY_REL = Path("docs") / "dialects.toml"
SUPPORT_REL = Path("docs") / "dialect-support.toml"
EVALUATIONS_REL = Path("docs") / "evaluations.toml"


class RegistryDataError(ValueError):
    """The registry documents cannot form one identity/support table."""


def load_documents(root: Path) -> tuple[dict, dict, dict]:
    """Parse the three registry documents under ``root``."""
    documents = []
    for relative in (IDENTITY_REL, SUPPORT_REL, EVALUATIONS_REL):
        path = root / relative
        try:
            with path.open("rb") as handle:
                documents.append(tomllib.load(handle))
        except FileNotFoundError as error:
            raise RegistryDataError(f"{relative.name}: not found") from error
        except tomllib.TOMLDecodeError as error:
            raise RegistryDataError(f"{relative.name}: parse error: {error}") from error
        except OSError as error:
            raise RegistryDataError(f"{relative.name}: {error}") from error
    return tuple(documents)


def joined_rows(identity: dict, support: dict) -> tuple[dict, list[tuple[dict, dict]]]:
    """Return declared formats and the total identity/support join in identity order."""
    declared = identity.get("format")
    identities = identity.get("dialect")
    supports = support.get("support")
    if not isinstance(declared, dict) or not declared:
        raise RegistryDataError(f"{IDENTITY_REL}: no [format.<id>] entries")
    if not isinstance(identities, list) or not identities:
        raise RegistryDataError(f"{IDENTITY_REL}: no [[dialect]] rows")
    if not isinstance(supports, list) or not supports:
        raise RegistryDataError(f"{SUPPORT_REL}: no [[support]] rows")

    by_id: dict[str, dict] = {}
    for row in supports:
        dialect = row.get("dialect") if isinstance(row, dict) else None
        if not isinstance(dialect, str):
            raise RegistryDataError(f"{SUPPORT_REL}: a [[support]] row has no dialect id")
        if dialect in by_id:
            raise RegistryDataError(f"{SUPPORT_REL}: duplicate support row for {dialect}")
        by_id[dialect] = row

    joined = []
    for identity_row in identities:
        dialect = identity_row.get("id") if isinstance(identity_row, dict) else None
        if not isinstance(dialect, str) or ":" not in dialect:
            raise RegistryDataError(f"{IDENTITY_REL}: bad dialect id {dialect!r}")
        format_id = dialect.partition(":")[0]
        if format_id not in declared:
            raise RegistryDataError(f"{IDENTITY_REL}: {dialect} names undeclared format {format_id}")
        support_row = by_id.pop(dialect, None)
        if support_row is None:
            raise RegistryDataError(f"{SUPPORT_REL}: no support row for {dialect}")
        joined.append((identity_row, support_row))
    if by_id:
        raise RegistryDataError(
            f"{SUPPORT_REL}: support rows name no identity row: {', '.join(sorted(by_id))}"
        )
    return declared, joined
