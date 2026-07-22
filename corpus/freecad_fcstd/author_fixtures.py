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
import zlib

import FreeCAD as App
import Fem
import Mesh
import Part
import Path as PathModule
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
        persistent_ids = {}
        history_tags = {}
        design_tags = {}
        element_map_child_ids = {}
        object_ids = {}

        def stable_persistent_id(match):
            source_id = match.group(1)
            target_id = persistent_ids.setdefault(
                source_id, f"{len(persistent_ids) + 1:x}".encode()
            )
            return b":H" + target_id

        def stable_history_tag(match):
            source_tag = match.group(2)
            target_tag = history_tags.setdefault(
                source_tag, f"{len(history_tags) + 1:x}".encode()
            )
            return match.group(1) + b":" + target_tag

        def stable_design_tag(match):
            source_tag = match.group(1)
            target_tag = design_tags.setdefault(
                source_tag, f"{len(design_tags) + 1:x}".encode()
            )
            return b";D" + target_tag + b";"

        def stable_element_map_child_id(match):
            source_id = match.group(2)
            target_id = element_map_child_ids.setdefault(
                source_id, str(len(element_map_child_ids) + 1).encode()
            )
            return match.group(1) + target_id + match.group(3)

        def stable_link_owner_id(match):
            source_id = match.group(2)
            target_id = object_ids.get(source_id, source_id)
            return match.group(1) + target_id + match.group(3)

        def stable_object_id(match):
            source_id = match.group(2)
            target_id = str(len(object_ids) + 1).encode()
            object_ids[source_id] = target_id
            return match.group(1) + target_id + match.group(3)

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
                declarations = re.sub(
                    rb'( id=")([0-9]+)(")',
                    stable_object_id,
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
                data = re.sub(
                    rb'(<Property name="_LinkOwner"[^>]*>\s*<Integer value=")([0-9]+)("/>)',
                    stable_link_owner_id,
                    data,
                )
                data = re.sub(
                    rb'(<XLink\b[^>]*\bstamp=")[^"]*(")',
                    rb"\g<1>2026-01-01T00:00:00Z\2",
                    data,
                )
            if source_info.filename == "GuiDocument.xml":
                next_tree_rank = iter(range(1, 1000000))
                data = re.sub(
                    rb' treeRank="[0-9]+"',
                    lambda _: f' treeRank="{next(next_tree_rank)}"'.encode(),
                    data,
                )
                data = re.sub(
                    rb'<Camera settings="[^"]*"/>',
                    (
                        b'<Camera settings="OrthographicCamera {&#10;'
                        b'  viewportMapping ADJUST_CAMERA&#10;'
                        b'  position 17.311642 -2.3116379 17.811638&#10;'
                        b'  orientation 0.74290609 0.30772209 0.59447283  1.2171158&#10;'
                        b'  nearDistance 0&#10;'
                        b'  farDistance 28.79236&#10;'
                        b'  aspectRatio 1&#10;'
                        b'  focalDistance 14.39618&#10;'
                        b'  height 32.432148&#10;&#10;}&#10;"/>'
                    ),
                    data,
                )
            if source_info.filename == "thumbnails/Thumbnail.png":
                data = canonical_thumbnail()
            if source_info.filename == "Document.xml" or source_info.filename.endswith(
                ".txt"
            ):
                data = re.sub(rb":H(-?[0-9a-fA-F]+)", stable_persistent_id, data)
                data = re.sub(
                    rb"(:H[0-9a-fA-F]+):([0-9a-fA-F]+)",
                    stable_history_tag,
                    data,
                )
                data = re.sub(rb";D([0-9a-fA-F]+);", stable_design_tag, data)
                if source_info.filename.endswith(".Map.txt"):
                    data = re.sub(
                        rb"(?m)^(\d+ \d+ \d+ )(-?\d+)( \d+ ;:H)",
                        stable_element_map_child_id,
                        data,
                    )
            info = zipfile.ZipInfo(source_info.filename, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            output.writestr(info, data)
    normalized.replace(target)


def canonical_thumbnail():
    """Return an original deterministic PNG illustrating the appearance fixture."""
    width = 256
    height = 256
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            background = (244, 247, 250)
            body = 48 <= x < 208 and 74 <= y < 184
            top = 72 <= x < 184 and 48 <= y < 78
            accent = (x - 148) ** 2 + (y - 76) ** 2 < 30**2
            if accent:
                color = (230, 112, 65)
            elif top:
                color = (85, 156, 218)
            elif body:
                color = (59, 132, 196)
            else:
                color = background
            row.extend(color)
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, payload):
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


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
    metadata(
        document,
        "Mesh, points, FEM, CAM, embedded bytes, and inert extension data",
    )

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

    fem_analysis = document.addObject("Fem::FemAnalysis", "FemAnalysis")
    temperature = document.addObject(
        "Fem::ConstraintTemperature", "TemperatureConstraint"
    )
    temperature.Temperature = 80.0
    temperature.Suppressed = False
    fem_analysis.addObject(temperature)

    toolpath = document.addObject("Path::Feature", "Toolpath")
    toolpath.Path = PathModule.Path(
        [
            PathModule.Command("G0", {"X": 0.0, "Y": 0.0, "Z": 5.0}),
            PathModule.Command(
                "G1", {"X": 10.0, "Y": 0.0, "Z": 0.0, "F": 120.0}
            ),
            PathModule.Command("G1", {"X": 10.0, "Y": 10.0, "Z": 0.0}),
        ]
    )

    extension = document.addObject("App::FeaturePython", "ExtensionPayload")
    extension.addProperty("App::PropertyLinkList", "Members", "Application")
    extension.Members = [mesh, cloud, fem_analysis, toolpath]
    extension.addProperty("App::PropertyString", "Domain", "Application")
    extension.Domain = "Extension retention fixture"
    extension.addProperty("App::PropertyPythonObject", "SerializedState", "Application")
    extension.SerializedState = {"iterations": 3, "command": None}
    payload_path = OUTPUT / "cc0_payload.bin"
    payload_path.write_bytes(struct.pack("<4I", 1, 2, 3, 4))
    extension.addProperty("App::PropertyFileIncluded", "Payload", "Application")
    extension.Payload = str(payload_path)

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


def design_history_document():
    document = App.newDocument("DesignHistory")
    metadata(document, "Parametric PartDesign additive, subtractive, and dress-up history")
    body = document.addObject("PartDesign::Body", "Body")

    pad_sketch = body.newObject("Sketcher::SketchObject", "PadSketch")
    pad_points = [(0, 0), (24, 0), (24, 16), (0, 16)]
    for index, point in enumerate(pad_points):
        next_point = pad_points[(index + 1) % len(pad_points)]
        pad_sketch.addGeometry(
            Part.LineSegment(App.Vector(*point, 0), App.Vector(*next_point, 0)), False
        )
    pad_sketch.addConstraint(Sketcher.Constraint("Horizontal", 0))
    pad_sketch.addConstraint(Sketcher.Constraint("Vertical", 1))
    pad_sketch.addConstraint(Sketcher.Constraint("Distance", 0, 24.0))
    pad_sketch.addConstraint(Sketcher.Constraint("Distance", 1, 16.0))
    pad = body.newObject("PartDesign::Pad", "Pad")
    pad.Profile = pad_sketch
    pad.Length = 10
    document.recompute()

    pocket_sketch = body.newObject("Sketcher::SketchObject", "PocketSketch")
    pocket_sketch.Placement.Base = App.Vector(0, 0, 10)
    pocket_sketch.addGeometry(Part.Circle(App.Vector(12, 8, 0), App.Vector(0, 0, 1), 3), False)
    pocket_sketch.addConstraint(Sketcher.Constraint("Diameter", 0, 6.0))
    pocket = body.newObject("PartDesign::Pocket", "Pocket")
    pocket.Profile = pocket_sketch
    pocket.Length = 6
    document.recompute()

    fillet = body.newObject("PartDesign::Fillet", "Fillet")
    fillet.Base = (pocket, ["Edge1"])
    fillet.Radius = 1
    document.recompute()

    save(document, "design_history.FCStd")
    App.closeDocument(document.Name)


def techdraw_document():
    document = App.newDocument("DrawingAnnotations")
    metadata(document, "TechDraw page, view, dimension, note, symbol, and template")

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
    dimension = document.addObject("TechDraw::DrawViewDimension", "Dimension")
    dimension.Type = "Distance"
    dimension.References2D = [(view, "Edge1")]
    dimension.FormatSpec = "%.2w"
    dimension.X = 80
    dimension.Y = 125
    page.addView(dimension)
    note = document.addObject("TechDraw::DrawViewAnnotation", "Note")
    note.Text = ["CC0 INSPECTION NOTE"]
    page.addView(note)
    symbol = document.addObject("TechDraw::DrawViewSymbol", "Symbol")
    symbol.Symbol = str(template_path)
    page.addView(symbol)

    document.recompute()
    save(document, "techdraw_annotations.FCStd")
    App.closeDocument(document.Name)


if __name__ == "__main__":
    core_document()
    application_document()
    geometry_topology_document()
    binary_shape_document()
    design_history_document()
    techdraw_document()
    for temporary_asset in OUTPUT.glob("cc0_*"):
        temporary_asset.unlink()
