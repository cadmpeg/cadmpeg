# SPDX-License-Identifier: CC0-1.0
"""Author the deterministic CC0 GUI, thumbnail, and appearance FCStd fixture."""

import os
from pathlib import Path

import FreeCAD as App
import FreeCADGui as Gui
import Part

from author_fixtures import metadata, normalize_fcstd


output = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
output.mkdir(parents=True, exist_ok=True)
target = output / "gui_appearance.FCStd"
target.unlink(missing_ok=True)

document = App.newDocument("GuiAppearance")
metadata(document, "GUI state, thumbnail, and object, face, edge, and vertex appearance")
feature = document.addObject("Part::Feature", "ColoredModel")
feature.Shape = Part.makeBox(18, 12, 7).fuse(
    Part.makeCylinder(3, 12, App.Vector(9, 6, 7))
)
document.recompute()

view = feature.ViewObject
view.ShapeColor = (0.20, 0.55, 0.85)
view.LineColor = (0.10, 0.10, 0.10)
view.PointColor = (0.95, 0.35, 0.15)
view.LineWidth = 2.5
view.PointSize = 4.0
view.Transparency = 12
face_colors = []
for index, _ in enumerate(feature.Shape.Faces):
    face_colors.append(
        (
            0.25 + 0.05 * (index % 4),
            0.35 + 0.04 * (index % 3),
            0.60 + 0.03 * (index % 5),
            0.88,
        )
    )
view.DiffuseColor = face_colors

App.ParamGet("User parameter:BaseApp/Preferences/Document").SetBool(
    "SaveThumbnail", True
)
active_view = Gui.activeDocument().activeView()
active_view.viewAxonometric()
active_view.fitAll()
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
Gui.getMainWindow().close()
