# SPDX-License-Identifier: CC0-1.0
# Original blank-document fixtures dedicated to CC0-1.0.
# Synthetic fixture, authored for Dutchman using FreeCAD's public scripting API.
# Run: flatpak run --command=FreeCADCmd org.freecad.FreeCAD author_techdraw_fixture.py
# Output folder: CADMPEG_FCSTD_OUTPUT (or DUTCHMAN_FIXTURE_OUT). No installed template is copied.
import os
import FreeCAD as App
import Part
import TechDraw

out = os.environ.get("CADMPEG_FCSTD_OUTPUT", os.environ.get("DUTCHMAN_FIXTURE_OUT", os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")))
os.makedirs(out, exist_ok=True)
svg = os.path.join(out, "techdraw-template.svg")
with open(svg, "w") as f:
    f.write('''<svg xmlns="http://www.w3.org/2000/svg" xmlns:freecad="http://www.freecadweb.org/wiki/index.php?title=Svg_Namespace" width="210mm" height="297mm" viewBox="0 0 210 297">
<rect x="10" y="10" width="190" height="277" fill="none" stroke="black" stroke-width="0.5"/>
<path d="M110 267 H200 M110 267 V287" fill="none" stroke="black"/>
<text id="TitleText" x="115" y="278" font-size="4" freecad:editable="Title"><tspan id="TitleValue">Synthetic box</tspan></text></svg>''')
doc = App.newDocument("TechDrawFixture")
box = doc.addObject("Part::Box", "Box")
box.Length, box.Width, box.Height = 30, 20, 10
page = doc.addObject("TechDraw::DrawPage", "Page")
template = doc.addObject("TechDraw::DrawSVGTemplate", "Template")
template.Template = svg
page.Template = template
for name, direction, x, y, scale in [("Front", (0,-1,0), 65,180,2), ("Top", (0,0,1), 65,230,1)]:
    view = doc.addObject("TechDraw::DrawViewPart", name)
    view.Source = [box]
    view.Direction = App.Vector(*direction)
    page.addView(view)
    view.ScaleType = "Custom"
    view.Scale = scale
    view.X, view.Y = x,y
doc.recompute()
dimension = doc.addObject("TechDraw::DrawViewDimension", "Length")
dimension.Type = "DistanceX"
dimension.References2D = [(doc.getObject("Front"), ("Edge3",))]
page.addView(dimension)
dimension.X, dimension.Y = 65,160
note = doc.addObject("TechDraw::DrawViewAnnotation", "Note")
note.Text = ["Synthetic box", "30 x 20 x 10 mm"]
page.addView(note)
note.X, note.Y = 120,100
doc.recompute()
if abs(dimension.getRawValue() - 30.0) > 1e-7:
    raise RuntimeError("Expected a 30 mm horizontal dimension; check Front edge numbering")
if template.EditableTexts.get("Title") != "Synthetic box":
    raise RuntimeError("The authored template title was not resolved")
doc.saveAs(os.path.join(out, "techdraw.FCStd"))
# Extend the same original document without loading installed patterns or templates.
for typ, name in [("TechDraw::DrawProjGroup", "Projection"), ("TechDraw::DrawViewSection", "Section"), ("TechDraw::DrawViewDetail", "Detail")]:
    try:
        view = doc.addObject(typ, name)
    except RuntimeError:
        # The runtime can create the object before it refuses its default pattern path.
        view = doc.getObject(name)
        if view is None:
            raise
    if name == "Projection":
        view.Source = [box]
        page.addView(view)
        view.addProjection("Front")
    elif name == "Section":
        view.FileHatchPattern = svg
        view.FileGeomPattern = ""
        view.PatIncluded = ""
        view.SvgIncluded = svg
        view.BaseView = doc.Front
        view.Source = [box]
        view.SectionNormal = App.Vector(1, 0, 0)
        view.SectionOrigin = App.Vector(15, 10, 5)
        page.addView(view)
    else:
        view.BaseView = doc.Front
        view.AnchorPoint = App.Vector(0, 0, 0)
        view.Radius = 5
        page.addView(view)
    view.X, view.Y = 120, 220
    doc.recompute()
for name, kind, edge, expected in [("Euclidean", "Distance", "Edge3", 30), ("Vertical", "DistanceY", "Edge1", 10)]:
    dimension = doc.addObject("TechDraw::DrawViewDimension", name)
    dimension.Type = kind
    dimension.References2D = [(doc.Front, (edge,))]
    page.addView(dimension)
    dimension.X, dimension.Y = 90, 160
    doc.recompute()
    if abs(dimension.getRawValue() - expected) > 1e-7:
        raise RuntimeError("Unexpected dimension value")
triangle = doc.addObject("Part::Feature", "Triangle")
triangle.Shape = Part.makePolygon([App.Vector(10,20,0), App.Vector(50,20,0), App.Vector(10,30,0), App.Vector(10,20,0)])
view = doc.addObject("TechDraw::DrawViewPart", "TriangleView")
view.Source = [triangle]
view.Direction = App.Vector(0,0,1)
view.XDirection = App.Vector(1,0,0)
view.ScaleType = "Custom"
view.Scale = 2
view.Rotation = 30
view.X, view.Y = 100,100
page.addView(view)
curves_svg = os.path.join(out, "techdraw-curves.svg")
with open(svg) as source, open(curves_svg, "w") as target:
    target.write(source.read().replace("</svg>", '<circle cx="50" cy="50" r="10"/><path d="M20 20 A10 10 0 0 1 30 30"/></svg>'))
template.Template = curves_svg
template.EditableTexts = {"Title": "Edited title"}
doc.recompute()
doc.saveAs(os.path.join(out, "techdraw-views.FCStd"))
cylinder = doc.addObject("Part::Cylinder", "Cylinder")
cylinder.Radius, cylinder.Height = 7, 10
circular = doc.addObject("TechDraw::DrawViewPart", "Circular")
circular.Source = [cylinder]
circular.Direction = App.Vector(0,0,1)
circular.ScaleType = "Custom"
circular.Scale = 1
circular.X, circular.Y = 140,220
page.addView(circular)
doc.recompute()
for kind, expected in [("Radius",7), ("Diameter",14), ("Angle",90)]:
    dimension = doc.addObject("TechDraw::DrawViewDimension", kind)
    dimension.Type = kind
    dimension.References2D = [(circular,("Edge0",))] if kind != "Angle" else [(doc.Front,("Edge1","Edge3"))]
    page.addView(dimension)
    dimension.X, dimension.Y = 140,200
    doc.recompute()
    if abs(dimension.getRawValue() - expected) > 1e-7:
        raise RuntimeError("Unexpected radial or angular value")
doc.saveAs(os.path.join(out, "techdraw-dimensions.FCStd"))
box.Length = 42
cylinder.Radius = 11
doc.recompute()
for name in ["Length", "Euclidean", "Vertical", "Radius", "Diameter", "Angle"]:
    dimension = doc.getObject(name)
    references = dimension.References2D
    dimension.References2D = []
    doc.recompute()
    dimension.References2D = references
    doc.recompute()
for name, expected in [("Length",42), ("Euclidean",42), ("Vertical",10), ("Radius",11), ("Diameter",22), ("Angle",90)]:
    if abs(doc.getObject(name).getRawValue() - expected) > 1e-7:
        raise RuntimeError("Unexpected variant dimension value")
doc.TriangleView.Rotation = 0
doc.recompute()
dimension = doc.addObject("TechDraw::DrawViewDimension", "TriangleBase")
dimension.Type = "Distance"
dimension.References2D = [(doc.TriangleView,("Edge0",))]
page.addView(dimension)
dimension.X, dimension.Y = 80,100
doc.recompute()
if abs(dimension.getRawValue() - 40) > 1e-7:
    raise RuntimeError("Unexpected asymmetric reference length")
doc.saveAs(os.path.join(out, "techdraw-variants.FCStd"))
App.closeDocument(doc.Name)
# Also exercise multiple pages and files containing pages without any 3D geometry.
doc = App.newDocument("DrawingOnly")
for name, x in [("First",20), ("Second",40)]:
    page = doc.addObject("TechDraw::DrawPage", name)
    template = doc.addObject("TechDraw::DrawSVGTemplate", name + "Template")
    template.Template = svg
    page.Template = template
    note = doc.addObject("TechDraw::DrawViewAnnotation", name + "Note")
    note.Text = [name]
    page.addView(note)
    note.X, note.Y = x,30
doc.recompute()
doc.saveAs(os.path.join(out, "techdraw-pages.FCStd"))
App.closeDocument(doc.Name)
