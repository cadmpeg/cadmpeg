# cadmpeg-codec-rhino

`cadmpeg-codec-rhino` is the Rhino `.3dm` codec for cadmpeg. It detects 3DM
input, inspects chunk and table structure, transfers supported model data into
CADIR version 4, and writes selected neutral model families as native 3DM.

Support level: [L9](../../docs/format-support.md#support-ladder) for archive
versions 50, 60, 70, and 80.

Archive versions 50, 60, 70, and 80 decode points, point clouds, line, arc,
polyline, polycurve, curve-on-surface, persistent polyedge-reference, and NURBS
curves; plane, clipping-plane, NURBS, revolution, and sum surfaces; meshes;
connected Brep topology; SubD control cages; extrusions; hatches; detail views;
NURBS cages and morph controls; modern and legacy V5 dimensions; center marks;
and recursively expanded instance definitions and references. Units,
tolerances, layers, object identity, names, effective object and face color,
visibility, and source instance paths transfer into the neutral model. Lengths
and length-valued tolerances transfer in millimeters; angles, unit vectors,
knot values, UV values, relative tolerances, and hatch pattern scale are not
scaled.

Built-in history records transfer as ordered native operations with command
identity, record mode, object dependencies, object selections, scalar and
transform values, persistent polyedge and SubD edge-chain constructions, and
embedded typed geometry. The native operation parameters preserve the complete
built-in command-value map without assigning application-specific meanings to
numeric parameter identifiers.

Product definitions, occurrences, placements, linked-file identities, layers,
object display attributes, materials, texture slots and mappings, bitmaps,
groups, lights, linetypes, hatch patterns, text styles, dimension styles,
general annotations, document render and drafting settings, saved views,
cameras, construction planes, page settings, wallpaper, trace images, notes,
revisions, previews, and application identity transfer into typed native
arenas. Third-party classes and userdata remain named exact records with their
class, item, or record identity. The source-fidelity sidecar classifies every
source byte as typed header data, structural framing, or part of a named opaque
record. Complete record bytes are retained within bounded limits; larger
records retain exact length and SHA-256 identity.

V3 and V4 archives support full container inspection and metadata decoding;
their object geometry remains unknown. V1, V2, and archive version 5 support
header-only inspection, and normal decode reports `NotImplemented`.

The native writer targets archive 50, 60, 70, or 80 explicitly. Source-less
generation supports point objects, grouped point clouds represented as
free-vertex bodies, circles, canonical rational or non-rational NURBS curves,
planes, canonical rational or non-rational NURBS surfaces, and standalone
triangle meshes with normals and recognized native auxiliary channels. Archive
60, 70, and 80 meshes preserve double vertices; archive 50 reports any vertex
quantization. B-rep generation supports connected planar faces with holes,
closed planar solids, and mixed planar and bounded nonperiodic NURBS faces with
outer and inner loops, exact line or NURBS edges, and parameter-space trim
curves. Multiple B-reps and free geometry may coexist in one generated archive.
Decoded archives produced by this writer can be edited and regenerated when
their retained namespace contains only generated accounting records and default
presentation state. Additional native records, unsupported topology, geometry,
presentation state, and noncanonical NURBS contracts are rejected before
output.

```sh
cadmpeg inspect model.3dm
cadmpeg decode model.3dm -o model.cadir.json
cadmpeg convert model.cadir.json -o model.3dm
cadmpeg convert model.cadir.json -o model-v6.3dm --rhino-version 60
```

Requires Rust 1.88 or later. Licensed under Apache-2.0.
