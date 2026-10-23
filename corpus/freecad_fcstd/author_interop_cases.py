# SPDX-License-Identifier: CC0-1.0
"""Author native interoperability regression inputs from blank documents."""

import json
import os
from pathlib import Path

import FreeCAD as App
import FreeCADGui as Gui
import Mesh
import Part


OUTPUT = Path(os.environ["CADMPEG_FCSTD_REFERENCE_DIR"])
OUTPUT.mkdir(parents=True, exist_ok=True)
Gui.showMainWindow()


def save(document, name):
    if document.recompute() < 0:
        raise RuntimeError(f"recompute failed for {name}")
    if any("Invalid" in obj.State for obj in document.Objects):
        raise RuntimeError(f"invalid object in {name}")
    document.saveAs(str(OUTPUT / f"{name}.FCStd"))
    App.closeDocument(document.Name)


document = App.newDocument("RestorationOrder")
for name, translation in (("ZFirst", (0, 0, 0)), ("ASecond", (40, 50, 60))):
    obj = document.addObject("Part::Feature", name)
    obj.Shape = Part.makeBox(10, 20, 30)
    obj.Placement.Base = App.Vector(*translation)
    obj.ViewObject.ShapeColor = (0.2, 0.4, 0.6)
    obj.ViewObject.LineColor = (0.7, 0.2, 0.1)
    obj.ViewObject.PointColor = (0.1, 0.8, 0.3)
save(document, "restoration_two_boxes")

document = App.newDocument("ParametricEdit")
obj = document.addObject("Part::Box", "Box")
obj.Length, obj.Width, obj.Height = 10, 20, 30
save(document, "parametric_box")

document = App.newDocument("ReplicaPcurves")
obj = document.addObject("Part::Feature", "FilletedBox")
box = Part.makeBox(10, 20, 30)
obj.Shape = box.makeFillet(1, box.Edges)
assert len(obj.Shape.Faces) == 26 and obj.Shape.isValid()
save(document, "filleted_box")


def triangle(document, name):
    obj = document.addObject("Mesh::Feature", name)
    obj.Mesh = Mesh.Mesh([[(0, 0, 0), (2, 0, 0), (0, 3, 0)]])
    obj.Placement = App.Placement(
        App.Vector(100, 200, 300), App.Rotation(App.Vector(0, 0, 1), 90)
    )
    return obj


document = App.newDocument("MeshPlacement")
triangle(document, "Triangle")
save(document, "placed_mesh")

document = App.newDocument("NestedMeshPlacement")
outer = document.addObject("App::Part", "Outer")
outer.Placement = App.Placement(
    App.Vector(10, 20, 30), App.Rotation(App.Vector(1, 0, 0), 90)
)
inner = document.addObject("App::Part", "Inner")
inner.Placement = App.Placement(
    App.Vector(-5, 7, 11), App.Rotation(App.Vector(0, 1, 0), 90)
)
outer.addObject(inner)
inner.addObject(triangle(document, "Triangle"))
save(document, "nested_mesh")

document = App.newDocument("MeshLinks")
mesh = triangle(document, "Triangle")
link = document.addObject("App::Link", "LinkedTriangle")
link.setLink(mesh)
link.LinkPlacement = App.Placement(
    App.Vector(-20, 40, 80), App.Rotation(App.Vector(0, 1, 0), 90)
)
array = document.addObject("App::Link", "MeshArray")
array.setLink(mesh)
array.ElementCount = 2
array.ShowElement = False
array.PlacementList = [
    App.Placement(App.Vector(5, 10, 15), App.Rotation()),
    App.Placement(App.Vector(25, 30, 35), App.Rotation(App.Vector(1, 0, 0), 90)),
]
save(document, "linked_mesh")

(OUTPUT / "expectations.json").write_text(
    json.dumps(
        {
            "parametric_box_edit.FCStd": {
                "reference": "parametric_box.FCStd",
                "edits": [{"object": "Box", "property": "Length", "value": 15}],
            },
            "restoration_two_boxes_edit.FCStd": {
                "reference": "restoration_two_boxes.FCStd",
                "edits": [
                    {"object": "ZFirst", "property": "Label", "value": "Edited first box"}
                ],
            },
        },
        indent=2,
        sort_keys=True,
    )
)
