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
for backup in output.glob("product_assembly*.FCBak"):
    backup.unlink()

external_target = output / "external_component.FCStd"
external_target.unlink(missing_ok=True)
external_document = App.newDocument("ExternalComponent")
metadata(external_document, "Externally linked product component")
external_part = external_document.addObject("Part::Feature", "ExternalPrototype")
external_part.Label = "External spacer"
external_part.Shape = Part.makeCylinder(5, 4)
external_document.recompute()
external_document.saveAs(str(external_target))
normalize_fcstd(external_target)

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

prototype_group = document.addObject("App::DocumentObjectGroup", "PrototypeGroup")
prototype_group.Label = "Reusable prototypes"
prototype_group.addObject(bracket)
prototype_group.addObject(pin)

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

link_group = document.addObject("App::LinkGroup", "OccurrenceGroup")
link_group.Label = "Nested, array, and external occurrences"

nested_occurrence = document.addObject("App::Link", "NestedBracketOccurrence")
nested_occurrence.LinkedObject = bracket_occurrence
nested_occurrence.LinkTransform = True
nested_occurrence.LinkPlacement.Base = App.Vector(0, 20, 0)

pin_array = document.addObject("App::Link", "PinArray")
pin_array.LinkedObject = pin
pin_array.LinkTransform = True
pin_array.ElementCount = 3
pin_array.PlacementList = [
    App.Placement(App.Vector(0, 0, 0), App.Rotation()),
    App.Placement(App.Vector(0, 10, 0), App.Rotation()),
    App.Placement(App.Vector(0, 20, 0), App.Rotation()),
]
pin_array.ScaleList = [(1.0, 1.0, 1.0), (1.0, 1.25, 1.0), (1.0, 1.5, 1.0)]

document.recompute()
document.saveAs(str(target))
external_occurrence = document.addObject("App::Link", "ExternalOccurrence")
external_occurrence.LinkedObject = external_part
external_occurrence.LinkTransform = True
external_occurrence.LinkPlacement.Base = App.Vector(40, 0, 0)
link_group.ElementList = [nested_occurrence, pin_array, external_occurrence]
assembly.addObject(link_group)

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
for backup in output.glob("product_assembly*.FCBak"):
    backup.unlink()
App.closeDocument(document.Name)
App.closeDocument(external_document.Name)
