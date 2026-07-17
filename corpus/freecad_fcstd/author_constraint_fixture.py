# SPDX-License-Identifier: CC0-1.0
"""Author the deterministic CC0 Sketcher constraint-family FCStd fixture."""

import os
from pathlib import Path

import FreeCAD as App
import Part
import Sketcher

from author_fixtures import metadata, normalize_fcstd


output = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
output.mkdir(parents=True, exist_ok=True)
target = output / "sketch_constraints.FCStd"
target.unlink(missing_ok=True)

document = App.newDocument("SketchConstraints")
metadata(document, "Independent Sketcher geometry and constraint families")


def sketch(name):
    return document.addObject("Sketcher::SketchObject", name)


def line(target_sketch, start=(0, 0), end=(10, 4)):
    return target_sketch.addGeometry(
        Part.LineSegment(App.Vector(*start, 0), App.Vector(*end, 0)), False
    )


relations = sketch("GeometricRelations")
for start, end in [
    ((0, 0), (10, 0)),
    ((15, 0), (15, 10)),
    ((20, 0), (30, 4)),
    ((20, 8), (30, 12)),
    ((35, 0), (45, 4)),
    ((39, -5), (35, 5)),
    ((50, 0), (60, 0)),
    ((60, 0), (65, 4)),
    ((70, 0), (80, 3)),
    ((80, 3), (90, 6)),
    ((95, 0), (105, 4)),
    ((95, 0), (105, -4)),
    ((110, 0), (120, 4)),
]:
    line(relations, start, end)
circle_a = relations.addGeometry(
    Part.Circle(App.Vector(55, 0, 0), App.Vector(0, 0, 1), 5), False
)
circle_b = relations.addGeometry(
    Part.Circle(App.Vector(55, 12, 0), App.Vector(0, 0, 1), 5), False
)
relations.addConstraint(Sketcher.Constraint("Horizontal", 0))
relations.addConstraint(Sketcher.Constraint("Vertical", 1))
relations.addConstraint(Sketcher.Constraint("Parallel", 2, 3))
relations.addConstraint(Sketcher.Constraint("Perpendicular", 4, 5))
relations.addConstraint(Sketcher.Constraint("Tangent", 6, 7))
relations.addConstraint(Sketcher.Constraint("Equal", circle_a, circle_b))
relations.addConstraint(Sketcher.Constraint("Coincident", 8, 2, 9, 1))
relations.addConstraint(Sketcher.Constraint("PointOnObject", 10, 1, 11))
relations.addConstraint(Sketcher.Constraint("Block", 12))

dimensions = sketch("DimensionalRelations")
distance_line = line(dimensions, (0, 0), (8, 4))
first = line(dimensions, (15, 0), (18, 3))
second = line(dimensions, (25, 5), (28, 9))
x_line = line(dimensions, (35, 0), (43, 4))
y_line = line(dimensions, (50, 0), (58, 4))
angle_a = line(dimensions, (65, 0), (75, 0))
angle_b = line(dimensions, (65, 0), (72, 7))
radius_circle = dimensions.addGeometry(
    Part.Circle(App.Vector(85, 0, 0), App.Vector(0, 0, 1), 4), False
)
diameter_circle = dimensions.addGeometry(
    Part.Circle(App.Vector(100, 0, 0), App.Vector(0, 0, 1), 4), False
)
dimensions.addConstraint(Sketcher.Constraint("Distance", distance_line, 9.0))
dimensions.addConstraint(Sketcher.Constraint("Distance", first, 1, second, 1, 11.0))
dimensions.addConstraint(Sketcher.Constraint("DistanceX", x_line, 1, x_line, 2, 8.0))
dimensions.addConstraint(Sketcher.Constraint("DistanceY", y_line, 1, y_line, 2, 4.0))
dimensions.addConstraint(Sketcher.Constraint("Angle", angle_a, angle_b, 0.7853981633974483))
dimensions.addConstraint(Sketcher.Constraint("Radius", radius_circle, 4.0))
dimensions.addConstraint(Sketcher.Constraint("Diameter", diameter_circle, 8.0))

symmetry = sketch("SymmetryRelation")
left = line(symmetry, (-8, 3), (-6, 5))
right = line(symmetry, (8, 3), (6, 5))
axis = line(symmetry, (0, -10), (0, 10))
symmetry.addConstraint(Sketcher.Constraint("Symmetric", left, 1, right, 1, axis))

inactive = sketch("InactiveRelation")
inactive_line = line(inactive, (0, 0), (10, 3))
inactive_index = inactive.addConstraint(Sketcher.Constraint("Horizontal", inactive_line))
inactive.setActive(inactive_index, False)

conic_helpers = sketch("InternalAlignmentRelations")
ellipse = conic_helpers.addGeometry(
    Part.Ellipse(App.Vector(0, 0, 0), 12, 5), False
)
conic_helpers.exposeInternalGeometry(ellipse)

optical = sketch("OpticalRelation")
incident = line(optical, (-10, -5), (0, 0))
refracted = line(optical, (0, 0), (8, 7))
interface = line(optical, (-12, 0), (12, 0))
optical.addConstraint(
    Sketcher.Constraint("SnellsLaw", incident, 2, refracted, 1, interface, 1.25)
)

weighted = sketch("SplineWeightRelation")
spline = Part.BSplineCurve()
spline.buildFromPolesMultsKnots(
    [
        App.Vector(0, 0, 0),
        App.Vector(4, 8, 0),
        App.Vector(8, 7, 0),
        App.Vector(12, 2, 0),
    ],
    [4, 4],
    [0.0, 1.0],
    False,
    3,
    [1.0, 1.2, 1.0, 1.0],
)
spline_index = weighted.addGeometry(spline, False)
weighted.exposeInternalGeometry(spline_index)

document.recompute()
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
