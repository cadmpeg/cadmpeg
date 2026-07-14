# SPDX-License-Identifier: CC0-1.0
"""Author the deterministic CC0 product, occurrence, and assembly-joint fixture."""

import os
from pathlib import Path

import FreeCAD as App
import Part
import Assembly  # noqa: F401 - registers Assembly document object types

from author_fixtures import metadata, normalize_fcstd


output = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
output.mkdir(parents=True, exist_ok=True)
target = output / "product_assembly.FCStd"
target.unlink(missing_ok=True)

document = App.newDocument("ProductAssembly")
metadata(document, "Components, occurrences, grounding, and revolute assembly joint")

assembly = document.addObject("Assembly::AssemblyObject", "Assembly")
assembly.Label = "CC0 linkage assembly"

bracket = document.addObject("Part::Feature", "BracketPrototype")
bracket.Label = "Bracket"
bracket.Shape = Part.makeBox(20, 12, 6)
bracket.addProperty("App::PropertyString", "PartNumber", "Product")
bracket.PartNumber = "CC0-BRACKET-01"

pin = document.addObject("Part::Feature", "PinPrototype")
pin.Label = "Pin"
pin.Shape = Part.makeCylinder(3, 20)
pin.addProperty("App::PropertyString", "PartNumber", "Product")
pin.PartNumber = "CC0-PIN-01"

bracket_occurrence = document.addObject("App::Link", "BracketOccurrence")
bracket_occurrence.LinkedObject = bracket
bracket_occurrence.LinkTransform = True
bracket_occurrence.LinkPlacement.Base = App.Vector(10, 0, 0)

pin_occurrence = document.addObject("App::Link", "PinOccurrence")
pin_occurrence.LinkedObject = pin
pin_occurrence.LinkTransform = True
pin_occurrence.LinkPlacement.Base = App.Vector(20, 6, 3)
pin_occurrence.LinkPlacement.Rotation = App.Rotation(App.Vector(0, 1, 0), 90)

assembly.addObject(bracket_occurrence)
assembly.addObject(pin_occurrence)

joint = document.addObject("App::FeaturePython", "RevoluteJoint")
joint.addProperty("App::PropertyEnumeration", "JointType", "Assembly")
joint.JointType = ["Fixed", "Revolute", "Slider"]
joint.JointType = "Revolute"
joint.addProperty("App::PropertyXLinkSubHidden", "Reference1", "Assembly")
joint.Reference1 = (bracket_occurrence, ["Face1", "Edge1"])
joint.addProperty("App::PropertyXLinkSubHidden", "Reference2", "Assembly")
joint.Reference2 = (pin_occurrence, ["Face2", "Edge3"])
joint.addProperty("App::PropertyPlacement", "Placement1", "Assembly")
joint.Placement1 = App.Placement(App.Vector(20, 6, 3), App.Rotation())
joint.addProperty("App::PropertyPlacement", "Placement2", "Assembly")
joint.Placement2 = App.Placement(App.Vector(20, 6, 3), App.Rotation())
joint.addProperty("App::PropertyPlacement", "Offset1", "Assembly")
joint.Offset1 = App.Placement(App.Vector(0, 0, 0.5), App.Rotation())
joint.addProperty("App::PropertyPlacement", "Offset2", "Assembly")
joint.Offset2 = App.Placement(App.Vector(0, 0, -0.5), App.Rotation())
joint.addProperty("App::PropertyAngle", "Angle", "Assembly")
joint.Angle = 12
joint.addProperty("App::PropertyAngle", "AngleMin", "Assembly")
joint.AngleMin = -35
joint.addProperty("App::PropertyAngle", "AngleMax", "Assembly")
joint.AngleMax = 55
joint.addProperty("App::PropertyBool", "EnableAngleMin", "Assembly")
joint.EnableAngleMin = True
joint.addProperty("App::PropertyBool", "EnableAngleMax", "Assembly")
joint.EnableAngleMax = True
joint.addProperty("App::PropertyBool", "Detach1", "Assembly")
joint.Detach1 = True
joint.addProperty("App::PropertyBool", "Suppressed", "Assembly")
joint.Suppressed = False
assembly.addObject(joint)

ground = document.addObject("App::FeaturePython", "GroundBracket")
ground.addProperty("App::PropertyLinkSub", "ObjectToGround", "Assembly")
ground.ObjectToGround = (bracket_occurrence, [""])
ground.addProperty("App::PropertyPlacement", "Placement", "Assembly")
ground.Placement = bracket_occurrence.LinkPlacement
assembly.addObject(ground)

document.recompute()
document.saveAs(str(target))
normalize_fcstd(target)
App.closeDocument(document.Name)
