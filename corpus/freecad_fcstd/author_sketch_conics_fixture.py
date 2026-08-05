# SPDX-License-Identifier: CC0-1.0
"""Author `sketch_conics.FCStd`, an FCStd parser fixture written as XML.

Every other fixture under `fixtures/` is saved by FreeCAD from a blank
document. This one is not: it is a `Document.xml` authored directly, because
the geometry it carries -- a circular arc and an elliptical arc with nonzero
sweep, and an ellipse whose major axis is not axis aligned -- is what reaches
the decoder's arc and ellipse trigonometry, and no FreeCAD-saved fixture in
this corpus carries it.

It is therefore a parser input only. It is not evidence about what FreeCAD
writes, and it must not be used to support any format-support claim.

Each arc is preceded and followed by a line segment whose endpoint is the arc
endpoint evaluated from the standard parameterizations

    circle(t)  = center + radius * (cos t, sin t)
    ellipse(t) = center + major * cos t * (cos a, sin a)
                        + minor * sin t * (-sin a, cos a)

so the decoder must reproduce both endpoints of both arcs, to within the 1e-9
chaining tolerance, for the profiles to chain. The order matters: the leading
line is chained first, so the arc joins it through the arc's *start* endpoint
and the trailing line joins the arc through its *end* endpoint. Putting the arc
first instead would leave its start endpoint unused and unpinned.

A wrong endpoint arm leaves seven single-entity profiles instead of two
three-entity chains and one unbounded ellipse.
"""

import math
import sys
import zipfile
from pathlib import Path


def number(value):
    return f"{value:.16f}"


ARC_CENTER = (0.0, 0.0)
ARC_RADIUS = 10.0
ARC_START = 0.4
ARC_END = 2.1

ELLIPSE_ARC_CENTER = (30.0, 0.0)
# Deliberately not a multiple of pi/4: at pi/4 the cosine and the sine are
# equal, so an axis frame built the wrong way round produces the same points and
# the fixture would pin nothing.
ELLIPSE_ARC_ANGLE = 0.7
ELLIPSE_ARC_MAJOR = 9.0
ELLIPSE_ARC_MINOR = 4.0
ELLIPSE_ARC_START = 0.3
ELLIPSE_ARC_END = 2.4

ELLIPSE_CENTER = (60.0, -6.0)
ELLIPSE_ANGLE = 1.1
ELLIPSE_MAJOR = 12.0
ELLIPSE_MINOR = 5.0


def circle_point(parameter):
    return (
        ARC_CENTER[0] + ARC_RADIUS * math.cos(parameter),
        ARC_CENTER[1] + ARC_RADIUS * math.sin(parameter),
    )


def ellipse_point(parameter):
    major = (math.cos(ELLIPSE_ARC_ANGLE), math.sin(ELLIPSE_ARC_ANGLE))
    minor = (-major[1], major[0])
    along_major = math.cos(parameter)
    along_minor = math.sin(parameter)
    return (
        ELLIPSE_ARC_CENTER[0]
        + ELLIPSE_ARC_MAJOR * along_major * major[0]
        + ELLIPSE_ARC_MINOR * along_minor * minor[0],
        ELLIPSE_ARC_CENTER[1]
        + ELLIPSE_ARC_MAJOR * along_major * major[1]
        + ELLIPSE_ARC_MINOR * along_minor * minor[1],
    )


def geometry(index, kind, carrier, construction=0):
    return f"""                        <Geometry type="Part::Geom{kind}" id="{index}" migrated="1">
                            <GeoExtensions count="1">
                                <GeoExtension type="Sketcher::SketchGeometryExtension" id="{index}" internalGeometryType="0" geometryModeFlags="00000000000000000000000000000000" geometryLayer="0"/>
                            </GeoExtensions>
                            {carrier}
                            <Construction value="{construction}"/>
                        </Geometry>"""


def line(start, end):
    return (
        f'<LineSegment StartX="{number(start[0])}" StartY="{number(start[1])}"'
        f' StartZ="0.0000000000000000" EndX="{number(end[0])}" EndY="{number(end[1])}"'
        f' EndZ="0.0000000000000000"/>'
    )


def geometry_list():
    arc_start = circle_point(ARC_START)
    arc_end = circle_point(ARC_END)
    ellipse_arc_start = ellipse_point(ELLIPSE_ARC_START)
    ellipse_arc_end = ellipse_point(ELLIPSE_ARC_END)
    carriers = [
        ("LineSegment", line(ARC_CENTER, arc_start)),
        (
            "ArcOfCircle",
            f'<ArcOfCircle CenterX="{number(ARC_CENTER[0])}" CenterY="{number(ARC_CENTER[1])}"'
            ' CenterZ="0.0000000000000000" NormalX="0.0000000000000000"'
            ' NormalY="0.0000000000000000" NormalZ="1.0000000000000000"'
            ' AngleXU="0.0000000000000000"'
            f' Radius="{number(ARC_RADIUS)}" StartAngle="{number(ARC_START)}"'
            f' EndAngle="{number(ARC_END)}"/>',
        ),
        ("LineSegment", line(arc_end, ARC_CENTER)),
        ("LineSegment", line(ELLIPSE_ARC_CENTER, ellipse_arc_start)),
        (
            "ArcOfEllipse",
            f'<ArcOfEllipse CenterX="{number(ELLIPSE_ARC_CENTER[0])}"'
            f' CenterY="{number(ELLIPSE_ARC_CENTER[1])}" CenterZ="0.0000000000000000"'
            ' NormalX="0.0000000000000000" NormalY="0.0000000000000000"'
            ' NormalZ="1.0000000000000000"'
            f' AngleXU="{number(ELLIPSE_ARC_ANGLE)}"'
            f' MajorRadius="{number(ELLIPSE_ARC_MAJOR)}"'
            f' MinorRadius="{number(ELLIPSE_ARC_MINOR)}"'
            f' StartAngle="{number(ELLIPSE_ARC_START)}"'
            f' EndAngle="{number(ELLIPSE_ARC_END)}"/>',
        ),
        ("LineSegment", line(ellipse_arc_end, ELLIPSE_ARC_CENTER)),
        (
            "Ellipse",
            f'<Ellipse CenterX="{number(ELLIPSE_CENTER[0])}" CenterY="{number(ELLIPSE_CENTER[1])}"'
            ' CenterZ="0.0000000000000000" NormalX="0.0000000000000000"'
            ' NormalY="0.0000000000000000" NormalZ="1.0000000000000000"'
            f' MajorRadius="{number(ELLIPSE_MAJOR)}" MinorRadius="{number(ELLIPSE_MINOR)}"'
            f' AngleXU="{number(ELLIPSE_ANGLE)}"/>',
        ),
    ]
    body = "\n".join(
        geometry(index + 1, kind, carrier)
        for index, (kind, carrier) in enumerate(carriers)
    )
    return f'<GeometryList count="{len(carriers)}">\n{body}\n                    </GeometryList>'


DOCUMENT = """<?xml version='1.0' encoding='utf-8'?>
<!--
 FreeCAD Document, see https://www.freecad.org for more information...
-->
<Document SchemaVersion="4" ProgramVersion="1.1R20260414 (Git shallow)" FileVersion="1" StringHasher="1">
    <StringHasher saveall="0" threshold="0" count="0"></StringHasher>
    <Properties Count="6" TransientCount="0">
        <Property name="Comment" type="App::PropertyString">
            <String value="Sketch conics parser fixture; authored as XML, not saved by FreeCAD"/>
        </Property>
        <Property name="CreationDate" type="App::PropertyString" status="16777217">
            <String value="2026-01-01T00:00:00Z"/>
        </Property>
        <Property name="Label" type="App::PropertyString" status="16777217">
            <String value="sketch_conics"/>
        </Property>
        <Property name="LastModifiedDate" type="App::PropertyString" status="16777217">
            <String value="2026-01-01T00:00:00Z"/>
        </Property>
        <Property name="License" type="App::PropertyString" status="1">
            <String value="CC0-1.0"/>
        </Property>
        <Property name="Uid" type="App::PropertyUUID" status="16777217">
            <Uuid value="00000000-0000-0000-0000-000000000001"/>
        </Property>
    </Properties>
    <Objects Count="1" Dependencies="0">
        <ObjectDeps Name="ConicArcs" Count="0"/>
        <Object type="Sketcher::SketchObject" name="ConicArcs" id="1" />
    </Objects>
    <ObjectData Count="1">
        <Object name="ConicArcs">
            <Properties Count="3" TransientCount="0">
                <Property name="Geometry" type="Part::PropertyGeometryList" status="8192">
                    {geometry}
                </Property>
                <Property name="Label" type="App::PropertyString" status="134217728">
                    <String value="ConicArcs"/>
                </Property>
                <Property name="Placement" type="App::PropertyPlacement" status="16777216">
                    <PropertyPlacement px="0.0000000000000000" py="0.0000000000000000" pz="0.0000000000000000" q0="0.0000000000000000" q1="0.0000000000000000" q2="0.0000000000000000" q3="1.0000000000000000" a="0.0000000000000000" ox="0.0000000000000000" oy="0.0000000000000000" oz="1.0000000000000000"/>
                </Property>
            </Properties>
        </Object>
    </ObjectData>
</Document>
"""


def main(target):
    """Writes the fixture, replacing any earlier copy."""
    document = DOCUMENT.format(geometry=geometry_list()).encode()
    target.unlink(missing_ok=True)
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.comment = b"cadmpeg CC0 FCStd fixture"
        info = zipfile.ZipInfo("Document.xml", date_time=(1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o600 << 16
        archive.writestr(info, document)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
