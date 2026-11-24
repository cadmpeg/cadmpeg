#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``generate-annex-a.py`` on a synthesized specification and schema."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("generate-annex-a.py")
SPEC = importlib.util.spec_from_file_location("generate_annex_a", SCRIPT)
assert SPEC and SPEC.loader
annex = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = annex
SPEC.loader.exec_module(annex)

DOCUMENT = """\
# Spec

```rust
pub struct Face { pub id: FaceId, pub surface: SolvedSurfaceGeometry, pub spans: Members<Span, Ordered>, pub line: LineCurve }
pub enum Span { Line(FinitePoint3), Native(NonBlankString) }
pub struct Wrapper<T: Clone + Debug>(Vec<T>);
```

  ```rust
  pub struct Sheet { pub address: CellAddress, pub tolerance: Option<PositiveLength> }
  ```

## Annex A. Record types stated by schema

The opening paragraph, which the generator keeps.

### `Stale`

## Annex B. Sidecar types stated by schema

The sidecar paragraph, which the generator keeps.

### `Old`
"""

SCHEMA = {
    "$defs": {
        "SolvedSurfaceGeometry": {"oneOf": [{"$ref": "#/$defs/PolygonalSurface"}, {"$ref": "#/$defs/Span"}]},
        "PolygonalSurface": {"type": "object", "properties": {"origin": {"$ref": "#/$defs/FinitePoint3"},
                                                          "id": {"$ref": "#/$defs/FaceId"}}},
        "FinitePoint3": {"type": "array", "items": {"type": "number"}},
        "Span": {"type": "string"},
        "FaceId": {"type": "string"},
        "Unused": {"type": "null"},
    }
}

SIDECAR = {
    "$defs": {
        "SourceFidelity": {"type": "object", "properties": {"digest": {"$ref": "#/$defs/Digest"},
                                                            "at": {"$ref": "#/$defs/FinitePoint3"},
                                                            "kind": {"$ref": "#/$defs/Span"}}},
        "Digest": {"type": "string"},
        "FinitePoint3": {"type": "array", "items": {"type": "number"}},
        "Span": {"type": "string"},
        "Report": {"type": "object"},
    }
}

PLAIN = DOCUMENT.replace("NonBlankString", "String").replace("PositiveLength", "u32")


class GenerateAnnexA(unittest.TestCase):
    def test_stated_and_named_types_come_from_the_rust_sketches(self):
        stated, named = annex.stated_and_named(DOCUMENT)
        self.assertEqual(stated, {"Face", "Span", "Wrapper", "Sheet"})
        self.assertEqual(named, {"FaceId", "SolvedSurfaceGeometry", "Members", "Span", "Ordered", "FinitePoint3",
                                 "NonBlankString", "CellAddress", "PositiveLength", "LineCurve"})

    def test_the_annex_holds_unstated_types_and_the_unstated_types_they_reach(self):
        names = annex.annex_types(DOCUMENT.replace("NonBlankString", "String")
                                  .replace("PositiveLength", "u32"), SCHEMA["$defs"])
        self.assertEqual(names, ["FinitePoint3", "PolygonalSurface", "SolvedSurfaceGeometry"])

    def test_a_type_awaiting_the_implementation_is_left_out_until_the_schema_defines_it(self):
        document = DOCUMENT.replace("NonBlankString", "String").replace("PositiveLength", "u32")
        self.assertNotIn("LineCurve", annex.annex_types(document, SCHEMA["$defs"]))
        defined = {**SCHEMA["$defs"], "LineCurve": {"type": "object"}}
        with self.assertRaisesRegex(annex.ShapeError, "now defines LineCurve; remove them from AWAITING"):
            annex.annex_types(document, defined)

    def test_a_named_type_nothing_defines_is_an_error(self):
        with self.assertRaisesRegex(annex.ShapeError, "NonBlankString, PositiveLength"):
            annex.annex_types(DOCUMENT, SCHEMA["$defs"])

    def test_annex_b_holds_the_sidecar_roots_and_the_types_no_other_part_states(self):
        annex_a = annex.annex_types(PLAIN, SCHEMA["$defs"])
        self.assertEqual(annex.sidecar_types(PLAIN, annex_a, SCHEMA["$defs"], SIDECAR["$defs"]),
                         ["Digest", "SourceFidelity"])

    def test_a_type_both_annexes_reach_has_one_schema(self):
        sidecar = {**SIDECAR["$defs"], "FinitePoint3": {"type": "string"}}
        annex_a = annex.annex_types(PLAIN, SCHEMA["$defs"])
        with self.assertRaisesRegex(annex.ShapeError, "define `FinitePoint3` differently"):
            annex.sidecar_types(PLAIN, annex_a, SCHEMA["$defs"], sidecar)

    def test_generation_replaces_everything_after_each_opening_paragraph(self):
        generated = annex.generate(PLAIN, SCHEMA, SIDECAR)
        head = PLAIN[: PLAIN.index("### `Stale`")]
        self.assertTrue(generated.startswith(head))
        self.assertNotIn("Stale", generated)
        self.assertNotIn("Old", generated)
        self.assertIn('### `PolygonalSurface`\n\n```json\n{\n  "type": "object"', generated)
        sidecar = generated[generated.index("## Annex B."):]
        self.assertIn("The sidecar paragraph, which the generator keeps.\n\n### `Digest`", sidecar)
        self.assertNotIn("Report", sidecar)
        self.assertEqual(annex.generate(generated, SCHEMA, SIDECAR), generated)

    def test_check_and_write(self):
        document = PLAIN
        with tempfile.TemporaryDirectory() as directory:
            spec, schema = Path(directory, "cadir-v11.md"), Path(directory, "schema.json")
            sidecar = Path(directory, "sidecar.json")
            spec.write_text(document, encoding="utf-8")
            schema.write_text(json.dumps(SCHEMA), encoding="utf-8")
            sidecar.write_text(json.dumps(SIDECAR), encoding="utf-8")
            (original, original_sidecar), annex.SPEC, annex.SIDECAR_SCHEMA = (annex.SPEC, annex.SIDECAR_SCHEMA), spec, sidecar
            try:
                with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(annex.main(["--schema", str(schema)]), 1)
                    self.assertEqual(annex.main(["--schema", str(schema), "--write"]), 0)
                    self.assertEqual(annex.main(["--schema", str(schema), "--check"]), 0)
                    spec.write_text(document.replace("## Annex A.", "## Annex B."), encoding="utf-8")
                    self.assertEqual(annex.main(["--schema", str(schema)]), 2)
                    spec.write_text(document.replace("## Annex B.", "## Annex C."), encoding="utf-8")
                    self.assertEqual(annex.main(["--schema", str(schema)]), 2)
            finally:
                annex.SPEC, annex.SIDECAR_SCHEMA = original, original_sidecar


if __name__ == "__main__":
    unittest.main()
