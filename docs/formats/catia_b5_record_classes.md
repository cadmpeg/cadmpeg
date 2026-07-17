<!-- Generated from crates/cadmpeg-codec-catia/src/b5_record_class.rs. Do not edit by hand; run `cargo test -p cadmpeg-codec-catia`. -->

# CATIA `b5 03` record classes

Each `b5 03` record's third header byte is its type/class code. The stream walk dispatches on this byte to resolve topology and geometry nodes. The surface column marks classes the topology binder accepts where a surface reference is required.

| Code | Name | Role | Surface | Description |
|---|---|---|---|---|
| `0x0e` | profile_line | profile | no | straight-line trim profile (point + direction) |
| `0x0f` | profile_arc | profile | no | circular-arc trim profile (center, two axes, radius) |
| `0x18` | line_pcurve | loop_member | no | straight-line pcurve loop member |
| `0x21` | pcurve | pcurve | no | parameter-space NURBS pcurve lifted onto its surface |
| `0x27` | surface_plane | surface | yes | planar surface (origin + two directions) |
| `0x28` | surface_cylinder | surface | yes | cylindrical surface (origin, axis, radius) |
| `0x2d` | surface_revolution | surface | yes | surface of revolution (profile curve, axis, gauge radius) |
| `0x34` | surface_nurbs | surface | yes | NURBS surface; geometry sourced from the a8 03 stream |
| `0x5e` | edge | edge | no | edge node binding two vertex points |
| `0x5f` | face | face | no | face node referencing one surface and its loops |
| `0x62` | loop | loop | no | loop node: (pcurve edge)* pairs plus a surface reference |
