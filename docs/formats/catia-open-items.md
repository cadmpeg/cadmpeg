# CATIA V5 `.CATPart`: Open Items

This document records `.CATPart` semantics that remain unresolved. The format specification contains only asserted rules.

## Container and roster

- Outer-directory stream-name extraction. The outer UTF-16LE runs do not establish a stream-name layout.
- Vertex-roster cardinality. The relationship between the `54` roster and `05 08 01` vertex-table cardinality has an unresolved exceptional case.
- The non-surface grammar and role of outer `01 00 04 00 <tag>` rows are unresolved. A literal marker scan does not establish that each row belongs to the freeform-surface alias roster or a vertex-registration roster.

## Standard nested `V5_CFV2`

- The byte relation assigning logical vertex components to `05 08 01` allocation rows is unspecified.
- Spline-region edge and vertex binding. A B-spline carrier lacks a closed-form locus for assigning an `a5 03 32` curve or pcurve to an edge and binding its endpoints.
- High-degeneracy analytic intersections. Collinear candidate sets can contain more endpoints than the physical edge uses.
- `op1` and persistent-tag resolution. The mapping from absolute persistent CGM tags to serialized records remains unresolved for the consolidated `a5` family.
- Standard-path line equations, multi-body shell membership, annular-cap orientation, and plane-normal sign.
- The standard-path arc branch is unspecified for torus-witnessed arcs and arcs with no witnessed adjacent face.
- The `a5 03 20` `op1` or persistent-tag reference to serialized-record mapping is unspecified.

## Object stream

- Multi-surface `b5 03 5f` face semantics.
- `b5 03 2d` bytes `+29..+76`.
- Referenced pole grids in non-inline-pole `a8 03 34` records.

## Zero-entity `a9 03`

- The reference-lane rule associating a `0638` oriented use with its owner-local `21xx` support and `05xx` incidence record is unspecified.
- The fields that bind each `05 0b`/`05 10`/`05 15` incidence lane to physical-edge endpoints are unspecified.
- The pole-reference program in non-inline `2145`, `2172`, and `219f` support records is unspecified.
- The byte-semantic distinction between non-inner loop classes `0x41` and `0xc1` is unspecified.
- The records or fields encoding body and shell membership are unspecified.

## E5 `0D 03`

- `0xa0` circle branch selection.
- `0xa0` wrapper-to-primitive co-parametric mapping. The cone subset uses `q_circle = (R/ca_q_scale) * q_ca`; the general mapping remains unresolved.
- Plane-cap digon orientation and rank-deficient plane frames.
- The two root `extra_orientation_signs`.
- The E5 body and shell orientation equation remains incomplete because the two root `extra_orientation_signs` lack assigned roles.

## FBB-only and float-packed variants

- `u24be` endpoint-port to logical-vertex collapse.
- The record-family discriminator and following byte grammar are unspecified when a nested file contains an FBB-like run but lacks one or more required edge or vertex populations.
- Float-packed loop records, markerless loops, the `b5 03 18` pcurve variant, and multi-loop inner loops.
- `b5 03 62` variant loop-node payloads, loop records without a `b5 03 62` marker, and inner-loop binding on multi-loop float-packed faces are unspecified.
- The delimiter grammar of the marker-only `00 33 3X` surface path is unspecified.
