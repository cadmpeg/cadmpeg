# SPDX-License-Identifier: Apache-2.0
"""Restore the generated FCStd spreadsheet and record FreeCAD's runtime kinds.

Regenerate with FreeCAD 1.1.4 and freecad.cmd on PATH:
    python3 -B record_formula_oracle.py --scratch /path/to/scratch
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile


def record(archive, addresses, output):
    import FreeCAD as App

    doc = App.openDocument(archive)
    try:
        doc.recompute()
        results = {}
        for address in addresses:
            value = doc.Sheet.get(address)
            if type(value) is str and value.startswith("ERR:"):
                results[address] = ["error", None]
            elif isinstance(value, App.Units.Quantity):
                results[address] = ["Quantity", list(value.Unit.Signature)]
            else:
                results[address] = [type(value).__name__, None]
        Path(output).write_text(json.dumps({
            "freecad": App.Version(),
            "results": results,
        }), encoding="utf-8")
    finally:
        App.closeDocument(doc.Name)


def main():
    from generate_formula_cases import cases, document

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch", type=Path, required=True)
    args = parser.parse_args()
    rows = list(cases())
    addresses = [address for address, *_ in rows]
    if len(set(addresses)) != len(addresses):
        raise ValueError("duplicate generated spreadsheet address")
    xml = document(rows)
    script = Path(__file__).resolve()
    with tempfile.TemporaryDirectory(dir=args.scratch) as work:
        work = Path(work)
        archive = work / "FormulaOracle.FCStd"
        with zipfile.ZipFile(archive, "w") as fcstd:
            fcstd.writestr("Document.xml", xml)
        output = work / "results.json"
        # The interactive console can recover from Python exceptions and return success.
        # Exit explicitly so a partial or failed recording cannot replace the fixture.
        command = f"""
import os
import runpy
import traceback
try:
    runpy.run_path({str(script)!r})["record"](
        {str(archive)!r}, {addresses!r}, {str(output)!r})
except BaseException:
    traceback.print_exc()
    os._exit(1)
os._exit(0)
"""
        subprocess.run(["freecad.cmd", "-c"], input=f"exec({command!r})\n",
                       text=True, check=True, timeout=180)
        captured = json.loads(output.read_bytes())
    if set(captured["results"]) != set(addresses):
        raise ValueError("FreeCAD result addresses differ from the generated sheet")
    table = {
        "freecad": captured["freecad"],
        "rows": [
            [family, address, *captured["results"][address]]
            for address, family, *_ in rows
        ],
    }
    # Keep one row per line so changes to an individual observation are visible in a diff.
    encoded = ['{', '  "freecad": ' + json.dumps(table["freecad"]) + ',', '  "rows": [']
    encoded.extend(
        '    ' + json.dumps(row) + (',' if index < len(rows) - 1 else '')
        for index, row in enumerate(table["rows"])
    )
    encoded.extend(['  ]', '}', ''])
    script.with_name("formula_oracle.json").write_text("\n".join(encoded), encoding="utf-8")
    script.with_name("formula_oracle.xml").write_bytes(xml)


if __name__ == "__main__":
    main()
