# SPDX-License-Identifier: Apache-2.0
"""Validate cadmpeg-written FCStd files inside a native FreeCAD process."""

import json
import os
from pathlib import Path

import FreeCAD as App


input_directory = Path(os.environ["CADMPEG_FCSTD_INPUT_DIR"])
output_directory = Path(os.environ["CADMPEG_FCSTD_OUTPUT_DIR"])
require_native_resave = os.environ.get("CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE") == "1"
output_directory.mkdir(parents=True, exist_ok=True)
results = []

for input_path in sorted(input_directory.glob("*.FCStd")):
    document = App.openDocument(str(input_path))
    if document is None:
        raise RuntimeError(f"FreeCAD refused {input_path.name}")
    source_types = [(obj.Name, obj.TypeId) for obj in document.Objects]
    native_resave_accepted = False
    if require_native_resave:
        document.recompute()
        output_path = output_directory / input_path.name
        document.saveAs(str(output_path))
        App.closeDocument(document.Name)

        reopened = App.openDocument(str(output_path))
        if reopened is None:
            raise RuntimeError(f"FreeCAD refused its resave of {input_path.name}")
        output_types = [(obj.Name, obj.TypeId) for obj in reopened.Objects]
        if output_types != source_types:
            raise RuntimeError(
                f"object identity/type drift for {input_path.name}: "
                f"{source_types!r} != {output_types!r}"
            )
        native_resave_accepted = True
        App.closeDocument(reopened.Name)
    else:
        App.closeDocument(document.Name)
    results.append(
        {
            "filename": input_path.name,
            "objects": len(source_types),
            "accepted": True,
            "native_resave_accepted": native_resave_accepted,
        }
    )

if not results:
    raise RuntimeError(f"no FCStd files found in {input_directory}")
print(json.dumps(results, indent=2, sort_keys=True), flush=True)
