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

extrusion_profile = feature(
    "ExtrusionProfile",
    Part.makePolygon(
        [
            App.Vector(0, 0, 0),
            App.Vector(8, 0, 0),
            App.Vector(8, 6, 0),
            App.Vector(0, 6, 0),
            App.Vector(0, 0, 0),
        ]
    ),
)
extrusion = document.addObject("Part::Extrusion", "SymmetricExtrusion")
extrusion.Base = extrusion_profile
extrusion.Dir = App.Vector(0, 0, 12)
extrusion.Symmetric = True
extrusion.Solid = True
extrusion.TaperAngle = 3

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
chamfer.Edges = [(1, 1.5, 2.5)]

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

datums = {}
for type_name, name, placement in [
    ("PartDesign::Plane", "DatumPlane", App.Vector(0, 0, 4)),
    ("PartDesign::Line", "DatumAxis", App.Vector(2, 3, 0)),
    ("PartDesign::Point", "DatumPoint", App.Vector(4, 5, 6)),
    ("PartDesign::CoordinateSystem", "DatumCoordinateSystem", App.Vector(7, 8, 9)),
]:
    datum = document.addObject(type_name, name)
    datum.Placement.Base = placement
    datums[name] = datum

draft_body = document.addObject("PartDesign::Body", "DraftBody")
draft_base = draft_body.newObject("PartDesign::Feature", "DraftBase")
draft_base.Shape = Part.makeBox(20, 15, 10)
draft = draft_body.newObject("PartDesign::Draft", "Draft")
draft.Base = (draft_base, ["Face1", "Face2"])
draft.NeutralPlane = (datums["DatumPlane"], [""])
draft.PullDirection = (datums["DatumAxis"], [""])
draft.Angle = 5
draft.Reversed = True

hole_body = document.addObject("PartDesign::Body", "HoleBody")
hole_base = hole_body.newObject("PartDesign::Feature", "HoleBase")
hole_base.Shape = Part.makeBox(24, 18, 10)
hole_profile = hole_body.newObject("Sketcher::SketchObject", "HoleProfile")
hole_profile.Placement.Base = App.Vector(0, 0, 10)
hole_profile.addGeometry(
    Part.Circle(App.Vector(12, 9, 0), App.Vector(0, 0, 1), 3), False
)
hole = hole_body.newObject("PartDesign::Hole", "Hole")
hole.Profile = hole_profile
hole.Diameter = 6
hole.DepthType = "Dimension"
hole.Depth = 7
hole.HoleCutType = "Countersink"
hole.HoleCutDiameter = 10
hole.HoleCutCountersinkAngle = 82
hole.DrillPoint = "Angled"
hole.DrillPointAngle = 118

pattern_body = document.addObject("PartDesign::Body", "PatternBody")
pattern_base = pattern_body.newObject("PartDesign::AdditiveBox", "PatternBase")
pattern_base.Length = 4
pattern_base.Width = 4
pattern_base.Height = 8
pattern = pattern_body.newObject("PartDesign::LinearPattern", "LinearPattern")
pattern.Originals = [pattern_base]
pattern.Direction = (datums["DatumAxis"], [""])
pattern.Length = 24
pattern.Occurrences = 4
pattern.Reversed = True

document.recompute()
thickness.Shape = Part.makeBox(16, 14, 8).cut(
    Part.makeBox(13.5, 11.5, 8, App.Vector(1.25, 1.25, 1.25))
)
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
