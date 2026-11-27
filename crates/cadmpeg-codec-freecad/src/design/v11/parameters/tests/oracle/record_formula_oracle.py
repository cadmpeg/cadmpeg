# SPDX-License-Identifier: Apache-2.0
"""Restore the generated FCStd spreadsheet and record FreeCAD's runtime kinds.

Regenerate with FreeCAD 1.1.4 and freecad.cmd on PATH:
    python3 -B record_formula_oracle.py --scratch /path/to/scratch
"""

import argparse
import json
import math
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
            sheet = doc.Sheet
            value = sheet.get(address)
            stored = sheet.getContents(address)
            result = type(value).__name__
            signature = None
            number = None
            text = None
            if type(value) is str:
                text = value
                if value.startswith("ERR:") and stored.startswith("="):
                    try:
                        sheet.evalExpression(stored[1:])
                    except Exception:
                        result = "error"
            elif isinstance(value, App.Units.Quantity):
                result = "Quantity"
                signature = list(value.Unit.Signature)
                number = value.Value
            elif type(value) in (int, float):
                number = float(value)
            if number is not None and not math.isfinite(number):
                number = str(number)
            results[address] = [result, signature, number, text, stored, sheet.getDisplayUnit(address)]
        Path(output).write_text(json.dumps({
            "freecad": App.Version(),
            "results": results,
        }), encoding="utf-8")
    finally:
        App.closeDocument(doc.Name)


def main():
    from generate_formula_cases import SEED, cases, document, grammar_cases

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch", type=Path, required=True)
    parser.add_argument("--freecad", default="freecad.cmd")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--seed", type=lambda value: int(value, 0), default=SEED)
    args = parser.parse_args()
    rows = list(cases(args.seed))
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
        subprocess.run([args.freecad, "-c"], input=f"exec({command!r})\n",
                       text=True, check=True, timeout=180)
        captured = json.loads(output.read_bytes())
    if set(captured["results"]) != set(addresses):
        raise ValueError("FreeCAD result addresses differ from the generated sheet")
    refs = {address: targets for address, _, _, _, targets in grammar_cases(args.seed)}
    table = {
        "seed": args.seed,
        "freecad": captured["freecad"],
        "rows": [
            [family, address, content, *captured["results"][address], refs.get(address)]
            for address, family, content, _ in rows
        ],
    }
    # Keep one row per line so changes to an individual observation are visible in a diff.
    encoded = ['{', f'  "seed": {args.seed},', '  "freecad": ' + json.dumps(table["freecad"]) + ',', '  "rows": [']
    encoded.extend(
        '    ' + json.dumps(row) + (',' if index < len(rows) - 1 else '')
        for index, row in enumerate(table["rows"])
    )
    encoded.extend(['  ]', '}', ''])
    (args.output / "formula_oracle.json").write_text("\n".join(encoded), encoding="utf-8")
    (args.output / "formula_oracle.xml").write_bytes(xml)


if __name__ == "__main__":
    main()
