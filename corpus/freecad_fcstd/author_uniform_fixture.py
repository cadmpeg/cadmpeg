# SPDX-License-Identifier: CC0-1.0
"""Author the CC0 whole-body appearance fixture from a blank native document."""

import os
from pathlib import Path
import sys

import FreeCAD as App
import FreeCADGui as Gui
import Part

sys.path.insert(0, str(Path(__file__).resolve().parent))
from author_fixtures import normalize_fcstd


output = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
output.mkdir(parents=True, exist_ok=True)
target = output / "uniform_appearance.FCStd"
target.unlink(missing_ok=True)
Gui.showMainWindow()
App.ParamGet("User parameter:BaseApp/Preferences/Document").SetBool("SaveThumbnail", False)
document = App.newDocument("UniformAppearance")
document.Comment = "Original blank-document model dedicated to CC0-1.0."
feature = document.addObject("Part::Feature", "Box")
feature.Shape = Part.makeBox(10, 20, 30)
feature.ViewObject.ShapeColor = (0.2, 0.4, 0.6)
feature.ViewObject.LineColor = (0.7, 0.2, 0.1)
feature.ViewObject.PointColor = (0.1, 0.8, 0.3)
feature.ViewObject.LineWidth = 3.0
feature.ViewObject.PointSize = 5.0
if document.recompute() < 0 or not feature.Shape.isValid():
    raise RuntimeError("invalid authored appearance fixture")
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
print(f"Authored {target.name} with FreeCAD {App.Version()}", flush=True)
