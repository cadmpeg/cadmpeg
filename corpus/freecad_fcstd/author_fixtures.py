# SPDX-License-Identifier: CC0-1.0
"""Author the cadmpeg FCStd public corpus from blank FreeCAD 1.1.1 documents.

The generated models and this authoring script are dedicated to the public
domain under CC0-1.0. They use no external model, template, or library content.
"""

from pathlib import Path
import os
import re
import struct
import zipfile

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
    normalize_fcstd(target)


def normalize_fcstd(target):
    normalized = OUTPUT / "cc0_normalized.FCStd"
    with zipfile.ZipFile(target, "r") as archive, zipfile.ZipFile(
        normalized, "w", compression=zipfile.ZIP_DEFLATED
    ) as output:
        output.comment = b"cadmpeg CC0 FCStd fixture"
        for source_info in archive.infolist():
            data = archive.read(source_info.filename)
            if source_info.filename == "Document.xml":
                data = re.sub(
                    rb'(<Property name="(?:CreationDate|LastModifiedDate)"[^>]*>\s*<String value=")[^"]*("/>)',
                    rb"\g<1>2026-01-01T00:00:00Z\2",
                    data,
                )
                objects_start = data.index(b"<Objects ")
                objects_end = data.index(b"</Objects>", objects_start)
                declarations = data[objects_start:objects_end]
                next_object_id = iter(range(1, 1000000))
                declarations = re.sub(
                    rb' id="[0-9]+"',
                    lambda _: f' id="{next(next_object_id)}"'.encode(),
                    declarations,
                )
                data = data[:objects_start] + declarations + data[objects_end:]
                next_uuid = iter(range(1, 1000000))
                data = re.sub(
                    rb'(<Uuid value=")[^"]+("/>)',
                    lambda match: match.group(1)
                    + f"00000000-0000-0000-0000-{next(next_uuid):012d}".encode()
                    + match.group(2),
                    data,
                )
            elif source_info.filename.endswith(".Map.txt"):
                persistent_ids = {}

                def stable_persistent_id(match):
                    source_id = match.group(1)
                    target_id = persistent_ids.setdefault(
                        source_id, f"{len(persistent_ids) + 1:x}".encode()
                    )
                    return b":H" + target_id

                data = re.sub(rb":H([0-9a-fA-F]+)", stable_persistent_id, data)
                history_tags = {}

                def stable_history_tag(match):
                    source_tag = match.group(2)
                    target_tag = history_tags.setdefault(
                        source_tag, f"{len(history_tags) + 1:x}".encode()
                    )
                    return match.group(1) + b":" + target_tag

                data = re.sub(
                    rb"(:H[0-9a-fA-F]+):([0-9a-fA-F]+)",
                    stable_history_tag,
                    data,
                )
            info = zipfile.ZipInfo(source_info.filename, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            output.writestr(info, data)
    normalized.replace(target)


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


def geometry_topology_document():
    document = App.newDocument("GeometryTopology")
    metadata(document, "Geometry carrier and connected topology family coverage")

    shapes = {
        "Solid": Part.makeBox(20, 15, 10),
        "Void": Part.makeBox(20, 20, 20).cut(
            Part.makeBox(10, 10, 10, App.Vector(5, 5, 5))
        ),
        "Seam": Part.makeCylinder(6, 18),
        "Degenerate": Part.makeCone(8, 0, 15),
        "Sheet": Part.Face(
            Part.makePolygon(
                [
                    App.Vector(0, 0, 0),
                    App.Vector(15, 0, 0),
                    App.Vector(15, 10, 0),
                    App.Vector(0, 10, 0),
                    App.Vector(0, 0, 0),
                ]
            )
        ),
        "Wire": Part.makePolygon(
            [App.Vector(0, 0, 0), App.Vector(5, 7, 0), App.Vector(12, 2, 0)]
        ),
    }
    shapes["Compound"] = Part.makeCompound(
        [Part.makeSphere(4), Part.makeTorus(6, 2, App.Vector(15, 0, 0))]
    )
    shapes["MultiShell"] = Part.makeCompound(
        [
            Part.makeBox(5, 5, 5).Shells[0],
            Part.makeBox(5, 5, 5, App.Vector(10, 0, 0)).Shells[0],
        ]
    )
    for name, shape in shapes.items():
        feature = document.addObject("Part::Feature", name)
        feature.Shape = shape

    line = Part.makeLine(App.Vector(0, 0, 0), App.Vector(10, 3, 2))
    circle = Part.makeCircle(5)
    ellipse = Part.Ellipse(App.Vector(0, 0, 0), 8, 3).toShape()
    bezier = Part.BezierCurve()
    bezier.setPoles(
        [
            App.Vector(0, 0, 0),
            App.Vector(3, 8, 0),
            App.Vector(8, -2, 0),
            App.Vector(12, 4, 0),
        ]
    )
    spline = Part.BSplineCurve()
    spline.interpolate(
        [
            App.Vector(0, 0, 0),
            App.Vector(4, 5, 1),
            App.Vector(9, 1, 2),
            App.Vector(14, 6, 0),
        ]
    )
    curves = document.addObject("Part::Feature", "CurveCarriers")
    curves.Shape = Part.makeCompound(
        [line, circle, ellipse, bezier.toShape(), spline.toShape()]
    )

    bspline_surface = Part.BSplineSurface()
    bspline_surface.interpolate(
        [
            [App.Vector(0, 0, 0), App.Vector(0, 5, 2), App.Vector(0, 10, 0)],
            [App.Vector(5, 0, 1), App.Vector(5, 5, 4), App.Vector(5, 10, 1)],
            [App.Vector(10, 0, 0), App.Vector(10, 5, 2), App.Vector(10, 10, 0)],
        ]
    )
    freeform = document.addObject("Part::Feature", "FreeformSurface")
    freeform.Shape = bspline_surface.toShape()

    revolved_profile = Part.Face(
        Part.makePolygon(
            [
                App.Vector(4, 0, 0),
                App.Vector(7, 0, 0),
                App.Vector(7, 0, 8),
                App.Vector(4, 0, 8),
                App.Vector(4, 0, 0),
            ]
        )
    )
    revolved = document.addObject("Part::Feature", "Revolved")
    revolved.Shape = revolved_profile.revolve(
        App.Vector(0, 0, 0), App.Vector(0, 0, 1), 270
    )

    path = Part.Wire(Part.makeLine(App.Vector(0, 0, 0), App.Vector(0, 0, 20)))
    profile = Part.Wire(Part.makeCircle(2))
    swept = document.addObject("Part::Feature", "Swept")
    swept.Shape = path.makePipeShell([profile], True, False)

    document.recompute()
    save(document, "geometry_topology.FCStd")
    App.closeDocument(document.Name)


def binary_shape_document():
    document = App.newDocument("BinaryExactShape")
    metadata(document, "Binary exact-shape side-entry framing")
    carrier = document.addObject("Part::Feature", "BinaryCarrier")
    carrier.Shape = Part.makeCylinder(7, 12).fuse(
        Part.makeSphere(8, App.Vector(0, 0, 12))
    )
    document.recompute()
    save(document, "binary_exact_shape.FCStd")

    binary_path = OUTPUT / "cc0_binary_shape.bin"
    carrier.Shape.exportBinary(str(binary_path))
    source = OUTPUT / "binary_exact_shape.FCStd"
    rewritten = OUTPUT / "cc0_binary_exact_shape.FCStd"
    with zipfile.ZipFile(source, "r") as archive, zipfile.ZipFile(
        rewritten, "w", compression=zipfile.ZIP_DEFLATED
    ) as output:
        for info in archive.infolist():
            data = archive.read(info.filename)
            name = info.filename
            if name == "Document.xml":
                data = data.replace(
                    b"BinaryCarrier.Shape.brp", b"BinaryCarrier.Shape.bin"
                )
            elif name == "BinaryCarrier.Shape.brp":
                name = "BinaryCarrier.Shape.bin"
                data = binary_path.read_bytes()
            output.writestr(name, data)
    source.unlink()
    rewritten.rename(source)
    normalize_fcstd(source)
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
geometry_topology_document()
binary_shape_document()
techdraw_document()
for temporary_asset in OUTPUT.glob("cc0_*"):
    temporary_asset.unlink()
