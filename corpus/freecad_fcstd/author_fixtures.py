# SPDX-License-Identifier: CC0-1.0
"""Author the cadmpeg FCStd public corpus from blank FreeCAD 1.1.1 documents.

The generated models and this authoring script are dedicated to the public
domain under CC0-1.0. They use no external model, template, or library content.
"""

from pathlib import Path
import os
import struct

import FreeCAD as App
import Mesh
import Part
import Points
import Sketcher


OUTPUT = Path(os.environ["CADMPEG_FCSTD_OUTPUT"]).resolve()
OUTPUT.mkdir(parents=True, exist_ok=True)


def save(document, filename):
    target = OUTPUT / filename
    target.unlink(missing_ok=True)
    for backup in OUTPUT.glob(f"{target.stem}*.FCBak"):
        backup.unlink()
    document.saveAs(str(target))


def metadata(document, purpose):
    document.addObject("App::FeaturePython", "CorpusMetadata")
    record = document.CorpusMetadata
    record.addProperty("App::PropertyString", "License", "Corpus")
    record.License = "CC0-1.0"
    record.addProperty("App::PropertyString", "Purpose", "Corpus")
    record.Purpose = purpose
    record.addProperty("App::PropertyPythonObject", "InertValue", "Corpus")
    record.InertValue = {"safe": True, "execute": False}


def core_document():
    document = App.newDocument("CoreDesignProduct")
    metadata(document, "Core geometry, sketches, design history, links, and spreadsheet")

    group = document.addObject("App::Part", "Product")
    group.Label = "CC0 Product"
    box = document.addObject("Part::Box", "Box")
    box.Length = 30
    box.Width = 20
    box.Height = 10
    cylinder = document.addObject("Part::Cylinder", "Cylinder")
    cylinder.Radius = 4
    cylinder.Height = 16
    cylinder.Placement.Base = App.Vector(15, 10, 0)
    cut = document.addObject("Part::Cut", "Cut")
    cut.Base = box
    cut.Tool = cylinder
    group.addObject(cut)

    sketch = document.addObject("Sketcher::SketchObject", "Profile")
    points = [(0, 0), (12, 0), (12, 8), (0, 8)]
    for index, point in enumerate(points):
        next_point = points[(index + 1) % len(points)]
        sketch.addGeometry(
            Part.LineSegment(App.Vector(*point, 0), App.Vector(*next_point, 0)), False
        )
    sketch.addConstraint(Sketcher.Constraint("Horizontal", 0))
    sketch.addConstraint(Sketcher.Constraint("Vertical", 1))
    sketch.addConstraint(Sketcher.Constraint("Distance", 0, 12.0))
    sketch.addConstraint(Sketcher.Constraint("Distance", 1, 8.0))
    group.addObject(sketch)

    feature = document.addObject("PartDesign::Feature", "DesignedFeature")
    feature.Shape = Part.makeBox(12, 8, 5)
    feature.addProperty("App::PropertyLink", "Profile", "Design")
    feature.Profile = sketch
    feature.addProperty("App::PropertyLength", "Length", "Design")
    feature.Length = 5
    group.addObject(feature)

    link = document.addObject("App::Link", "Occurrence")
    link.LinkedObject = feature
    link.LinkTransform = True
    link.Placement.Base = App.Vector(40, 0, 0)

    sheet = document.addObject("Spreadsheet::Sheet", "Parameters")
    sheet.set("A1", "Length")
    sheet.set("B1", "30 mm")
    sheet.setAlias("B1", "ProductLength")
    sheet.set("A2", "Width")
    sheet.set("B2", "20 mm")

    document.recompute()
    save(document, "core_design_product.FCStd")
    App.closeDocument(document.Name)


def application_document():
    document = App.newDocument("ApplicationPayloads")
    metadata(document, "Mesh, points, embedded bytes, and inert extension data")

    mesh = document.addObject("Mesh::Feature", "Mesh")
    mesh.Mesh = Mesh.Mesh(
        [
            (App.Vector(0, 0, 0), App.Vector(10, 0, 0), App.Vector(0, 10, 0)),
            (App.Vector(10, 0, 0), App.Vector(10, 10, 0), App.Vector(0, 10, 0)),
        ]
    )
    cloud = document.addObject("Points::Feature", "PointCloud")
    cloud.Points = Points.Points(
        [App.Vector(1, 2, 3), App.Vector(4, 5, 6), App.Vector(-1, 0, 2)]
    )

    analysis = document.addObject("App::FeaturePython", "Analysis")
    analysis.addProperty("App::PropertyLinkList", "Members", "Application")
    analysis.Members = [mesh, cloud]
    analysis.addProperty("App::PropertyString", "Domain", "Application")
    analysis.Domain = "FEM/CAM retention fixture"
    analysis.addProperty("App::PropertyPythonObject", "SerializedState", "Application")
    analysis.SerializedState = {"iterations": 3, "command": None}
    payload_path = OUTPUT / "cc0_payload.bin"
    payload_path.write_bytes(struct.pack("<4I", 1, 2, 3, 4))
    analysis.addProperty("App::PropertyFileIncluded", "Payload", "Application")
    analysis.Payload = str(payload_path)

    document.recompute()
    save(document, "application_payloads.FCStd")
    App.closeDocument(document.Name)


def techdraw_document():
    document = App.newDocument("DrawingAnnotations")
    metadata(document, "TechDraw page, view, dimension-like note, symbol, and template")

    model = document.addObject("Part::Box", "Model")
    model.Length = 25
    model.Width = 15
    model.Height = 8

    template_path = OUTPUT / "cc0_template.svg"
    template_path.write_text(
        """<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297"><rect x="5" y="5" width="200" height="287" fill="none" stroke="black"/><text x="10" y="288">cadmpeg CC0</text></svg>""",
        encoding="utf-8",
    )
    page = document.addObject("TechDraw::DrawPage", "Page")
    template = document.addObject("TechDraw::DrawSVGTemplate", "Template")
    template.Template = str(template_path)
    page.Template = template
    view = document.addObject("TechDraw::DrawViewPart", "View")
    view.Source = [model]
    view.X = 80
    view.Y = 100
    view.Scale = 1.5
    page.addView(view)
    note = document.addObject("TechDraw::DrawViewAnnotation", "Note")
    note.Text = ["CC0 INSPECTION NOTE"]
    page.addView(note)
    symbol = document.addObject("TechDraw::DrawViewSymbol", "Symbol")
    symbol.Symbol = str(template_path)
    page.addView(symbol)

    document.recompute()
    save(document, "techdraw_annotations.FCStd")
    App.closeDocument(document.Name)


core_document()
application_document()
techdraw_document()
for temporary_asset in OUTPUT.glob("cc0_*"):
    temporary_asset.unlink()
