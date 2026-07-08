# CATIA V5 `.CATPart`: Open Items

This document records `.CATPart` semantics that remain unresolved. The format specification contains only asserted rules.

## Container and roster

- Outer-directory stream-name extraction. The outer UTF-16LE runs do not establish a stream-name layout.
- Vertex-roster cardinality. The relationship between the `54` roster and `05 08 01` vertex-table cardinality has an unresolved exceptional case.
- The non-surface grammar and role of outer `01 00 04 00 <tag>` rows are unresolved. A literal marker scan does not establish that each row belongs to the freeform-surface alias roster or a vertex-registration roster.

## Standard nested `V5_CFV2`

- Logical-vertex assignment to `05 08 01` rows. This allocation-layer mapping is required for byte-faithful reserialization but not for analytic BREP reconstruction.
- Spline-region edge and vertex binding. A B-spline carrier lacks a closed-form locus for assigning an `a5 03 32` curve or pcurve to an edge and binding its endpoints.
- High-degeneracy analytic intersections. Collinear candidate sets can contain more endpoints than the physical edge uses.
- `op1` and persistent-tag resolution. The mapping from absolute persistent CGM tags to serialized records remains unresolved for the consolidated `a5` family.
- `b2 03` face, loop, and coedge graph reconstruction.
- Standard-path line equations, multi-body shell membership, annular-cap orientation, and plane-normal sign.
- The standard-path arc branch is unspecified for torus-witnessed arcs and arcs with no witnessed adjacent face.
- The `a5 03 20` `op1` or persistent-tag reference to serialized-record mapping is unspecified.

## Object stream

- Multi-surface `b5 03 5f` face semantics.
- Binding of `5f` rank to face declaration order beyond the entity-number proxy.
- `b5 03 2d` bytes `+29..+76`.
- Referenced pole grids in non-inline-pole `a8 03 34` records.

## Zero-entity `a9 03`

- Owner-local ordering that associates a `0638` oriented use with its `21xx` support and `05xx` vertex.
- Non-inline support programs `2145`, `2172`, and `219f` that reference poles.
- Periodic-face merge for duplicated curved carriers and a shared seam.
- Geometry-pool families beyond plane, cylinder, cone, torus, circle/arc, and B-spline carriers are unspecified, including the distinction between `0x41` and `0xc1` in non-inner loops.
- The zero-entity variant has no defined byte-backed body or shell membership signal.

## E5 `0D 03`

- `0xa0` circle branch selection.
- `0xa0` wrapper-to-primitive co-parametric mapping. The cone subset uses `q_circle = (R/ca_q_scale) * q_ca`; the general mapping remains unresolved.
- Plane-cap digon orientation and rank-deficient plane frames.
- The two root `extra_orientation_signs`.
- The E5 body and shell orientation equation remains incomplete because the two root `extra_orientation_signs` lack assigned roles.

## FBB-only and float-packed variants

- `u24be` endpoint-port to logical-vertex collapse.
- Files with incomplete or non-standard standard-looking spines have an unresolved geometry source. The missing populations may use a second storage segment, another spine grammar, an assembly/reference mechanism, or export-generated topology.
- Float-packed loop records, markerless loops, the `b5 03 18` pcurve variant, and multi-loop inner loops.
- `b5 03 62` variant loop-node payloads, loop records without a `b5 03 62` marker, and inner-loop binding on multi-loop float-packed faces are unspecified.
- The delimiter grammar of the marker-only `00 33 3X` surface path is unspecified.
