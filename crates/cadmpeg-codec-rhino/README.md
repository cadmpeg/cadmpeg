# cadmpeg-codec-rhino

`cadmpeg-codec-rhino` is the read-only Rhino `.3dm` codec for cadmpeg. It
detects 3DM input, inspects chunk and table structure, decodes document
metadata, and transfers supported model data into CADIR version 5.

Archive versions 50, 60, 70, and 80 decode points, point clouds, line, arc,
polyline, polycurve, and NURBS curves; plane, NURBS, revolution, and sum
surfaces; meshes; connected Brep topology; SubD control cages; extrusions; and
recursively expanded instance definitions and references. Units, tolerances,
layers, object identity, names, effective color, visibility, and source
instance paths are retained in the neutral model. Lengths and length-valued
tolerances transfer in millimeters; angles, unit vectors, knot values, UV
values, and relative tolerances are not scaled.

Unsupported classes, future payload versions, plugin data, dimensions,
hatches, annotations, rendering details, and other unmapped records remain
native unknown records. Rendering-attribute and nested material-reference
records are structurally framed but are not transferred as typed appearance
data. Complete record bytes are retained within per-record and per-document
limits; larger records retain their exact length and SHA-256 digest. A
truncated prefix is never retained as a complete record. Checksum failures,
invalid compressed channels, and invalid Brep, extrusion, SubD, or instance
candidates stay within their bounded parent and do not commit partial
geometry or topology.

V3 and V4 archives support full container inspection and metadata decoding;
their object geometry remains unknown. V1, V2, and archive version 5 support
header-only inspection, and normal decode reports `NotImplemented`.

```sh
cadmpeg inspect model.3dm
cadmpeg decode model.3dm -o model.cadir.json
```

Requires Rust 1.88 or later. Licensed under Apache-2.0.
