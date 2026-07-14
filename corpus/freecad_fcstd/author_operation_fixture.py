# SPDX-License-Identifier: CC0-1.0
"""Author deterministic CC0 core Part and PartDesign operation histories."""

import os
from pathlib import Path

import FreeCAD as App
import Part
import Sketcher

from author_fixtures import metadata, normalize_fcstd


output = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
output.mkdir(parents=True, exist_ok=True)
target = output / "core_operations.FCStd"
target.unlink(missing_ok=True)

document = App.newDocument("CoreOperations")
metadata(document, "Core Part and PartDesign operation-family semantics")


def feature(name, shape):
    result = document.addObject("Part::Feature", name)
    result.Shape = shape
    return result


revolution_profile = document.addObject("Sketcher::SketchObject", "RevolutionProfile")
revolution_profile.addGeometry(
    Part.LineSegment(App.Vector(4, 0, 0), App.Vector(8, 0, 0)), False
)
revolution = document.addObject("Part::Revolution", "Revolution")
revolution.Source = revolution_profile
revolution.Axis = App.Vector(0, 1, 0)
revolution.Angle = 225
revolution.Symmetric = True
revolution.Solid = False

loft_section_a = feature(
    "LoftSectionA", Part.Wire(Part.makeCircle(4, App.Vector(0, 0, 0)))
)
loft_section_b = feature(
    "LoftSectionB", Part.Wire(Part.makeCircle(4, App.Vector(0, 0, 14)))
)
loft = document.addObject("Part::Loft", "Loft")
loft.Sections = [loft_section_a, loft_section_b]
loft.Solid = True
loft.Ruled = True
loft.Closed = False
loft.MaxDegree = 4

sweep_section = feature(
    "SweepSection", Part.Wire(Part.makeCircle(2, App.Vector(0, 0, 0)))
)
sweep_spine = feature(
    "SweepSpine",
    Part.makePolygon(
        [App.Vector(0, 0, 0), App.Vector(0, 0, 10), App.Vector(8, 0, 18)]
    ),
)
sweep = document.addObject("Part::Sweep", "Sweep")
sweep.Sections = [sweep_section]
sweep.Spine = (sweep_spine, ["Edge1", "Edge2"])
sweep.Solid = True
sweep.Frenet = True
sweep.Transition = "Transformed"

dressup_base = feature("DressupBase", Part.makeBox(20, 16, 10))
fillet = document.addObject("Part::Fillet", "PartFillet")
fillet.Base = dressup_base
fillet.Edges = [(1, 2.0, 2.0)]

chamfer_base = feature("ChamferBase", Part.makeBox(18, 12, 9))
chamfer = document.addObject("Part::Chamfer", "PartChamfer")
chamfer.Base = chamfer_base
chamfer.Edges = [(1, 1.5, 1.5)]

thickness_base = feature("ThicknessBase", Part.makeBox(16, 14, 8))
thickness = document.addObject("Part::Thickness", "Thickness")
thickness.Faces = (thickness_base, ["Face6"])
thickness.Value = 1.25
thickness.Mode = "Skin"
thickness.Join = "Arc"
thickness.Intersection = True

mirror_base = feature("MirrorBase", Part.makeCylinder(3, 10, App.Vector(12, 0, 0)))
mirror = document.addObject("Part::Mirroring", "Mirror")
mirror.Source = mirror_base
mirror.Normal = App.Vector(1, 0, 0)

for type_name, name, placement in [
    ("PartDesign::Plane", "DatumPlane", App.Vector(0, 0, 4)),
    ("PartDesign::Line", "DatumAxis", App.Vector(2, 3, 0)),
    ("PartDesign::Point", "DatumPoint", App.Vector(4, 5, 6)),
    ("PartDesign::CoordinateSystem", "DatumCoordinateSystem", App.Vector(7, 8, 9)),
]:
    datum = document.addObject(type_name, name)
    datum.Placement.Base = placement

document.recompute()
thickness.Shape = Part.makeBox(16, 14, 8).cut(
    Part.makeBox(13.5, 11.5, 8, App.Vector(1.25, 1.25, 1.25))
)
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
