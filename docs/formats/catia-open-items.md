# CATIA V5 `.CATPart`: Open Items

This document records `.CATPart` semantics that remain unresolved. The format specification contains only asserted rules.

## Container and roster

- Outer-directory stream-name extraction. The outer UTF-16LE runs do not establish a stream-name layout.
- Vertex-roster cardinality. The relationship between the `54` roster and `05 08 01` vertex-table cardinality has an unresolved exceptional case.
- The non-surface grammar and role of outer `01 00 04 00 <tag>` rows are unresolved. A literal marker scan does not establish that each row belongs to the freeform-surface alias roster or a vertex-registration roster.

## Container and roster (decoded-but-unresolved fields)

- The extent-struct `flags` word is carried raw; its bit assignments are unresolved.

## Design records

- The field grammar inside `7C0B` value-block payloads and its binding to adjacent `7C02` class/field names are unspecified.

## Standard nested `V5_CFV2`

- The `a5 03 32` header token byte at `record + 7` is a small repeating type code; its value space and semantics are unresolved.

- The byte relation assigning logical vertex components to `05 08 01` allocation rows is unspecified.
- Standard spline edge carrier geometry. Native `b5 03 5e` endpoint identities bind the edge topology, but the solved 3D spline carrier referenced by the standard row remains unresolved.
- `op1` and persistent-tag resolution. The mapping from absolute persistent CGM tags to serialized records remains unresolved for the consolidated `a5` family.
- Standard-path line equations, multi-body shell membership, annular-cap orientation, and plane-normal sign.
- The standard-path arc branch is unspecified for torus-witnessed arcs and arcs with no witnessed adjacent face.
- The `a5 03 20` `op1` or persistent-tag reference to serialized-record mapping is unspecified.

## Object stream

- Multi-surface `b5 03 5f` face semantics.
- The field or relation fixing each `b5 03 5f` face's normal sense against its surface frame is unresolved. Closed endpoint chains determine coedge traversal but not this face-level sign.
- The object-stream body-kind and outward-shell sign fields are unresolved; one-body ownership and incidence determine a stable topology gauge but do not identify the source sign bytes.
- `b5 03 2d` bytes `+29..+76`.
- Referenced pole grids in non-inline-pole `a8 03 34` records.

## Zero-entity `a9 03`

- The reference-lane rule associating a `0638` oriented use with its owner-local `21xx` support and `05xx` incidence record is unspecified.
- The fields that bind each `05 0b`/`05 10`/`05 15` incidence lane to physical-edge endpoints are unspecified.
- The pole-reference program in non-inline `2145`, `2172`, and `219f` support records is unspecified.
- The records or fields encoding body and shell membership are unspecified.
- The six `0x10`-tagged `u32` reference tokens of the `5e 1a` edge-stride record (offsets `7, 12, 17, 22, 27, 32`) are carried raw; their referents are unresolved.

## E5 `0D 03`

- `0xa0` circle branch selection.
- `0xa0` wrapper-to-primitive co-parametric mapping. The cone subset uses `q_circle = (R/ca_q_scale) * q_ca`; the general mapping remains unresolved.
- Plane-cap digon orientation and rank-deficient plane frames.
- The two root `extra_orientation_signs`.
- The E5 body and shell orientation equation remains incomplete because the two root `extra_orientation_signs` lack assigned roles.
- Curve-support records: the mode byte following the pcurve reference lane and the bytes after the fixed header are carried raw; both are unresolved.
- Bounds records: the trailing `u32` code after each bound parameter is unresolved.
- Edge-use records: the bytes after the five counted reference fields are unresolved.

## FBB-only and float-packed variants

- `u24be` endpoint-port to logical-vertex collapse.
- The record-family discriminator and following byte grammar are unspecified when a nested file contains an FBB-like run but lacks one or more required edge or vertex populations.
- Variant loop-node payloads outside the length-framed `b5 03 62` and `a8 03 62` forms are unspecified.
- The delimiter grammar of the marker-only `00 33 3X` surface path is unspecified.
