# CATIA V5 `.CATPart`: Open Items

This document records `.CATPart` semantics that remain unresolved. The format specification contains only asserted rules.

## Container and roster

- Vertex-roster cardinality. The relationship between the `54` roster and `05 08 01` vertex-table cardinality has an unresolved exceptional case.
- The non-surface grammar and role of outer `01 00 04 00 <tag>` rows are unresolved. A literal marker scan does not establish that each row belongs to the freeform-surface alias roster or a vertex-registration roster.

## Container and roster (decoded-but-unresolved fields)

- The extent-struct `flags` word is carried raw; its bit assignments are unresolved.

## Design records

- The relation from grouped `7C0B` stored values to `7C09` design-object fields is unresolved.

## Standard nested `V5_CFV2`

- The `a5 03 32` header token byte at `record + 7` is a small repeating type code; its value space and semantics are unresolved.
- The numeric continuation following the three aligned `a5 03 32` jet blocks has multiple length classes. Its lane counts, terminal fields, and relationship to the rolling-ball definition are unresolved.
- The semantic assignments of the width-coded `b2/b3/b4 03 5e` header token and terminal byte are unresolved.
- The field semantics of class-`0x18` descriptors, the operands and individual eight-scalar lanes in analytic-circle class-`0x23` edge definitions, the corresponding roles in standalone class-`0x24` records, and the class-`0x25` scalar lanes are unresolved.
- The internal coding of the sampled-cache lane in `a8 03 25` extrusion directrices is unresolved. Its enclosing references, solved parameter interval, and fit tolerance are defined independently of that cache.

- The byte relation assigning logical vertex components to `05 08 01` allocation rows is unspecified.
- Standard spline cache poles, knots, and native parameterization. The exact two-surface intersection construction and endpoint trim are resolved, but the serialized cache referenced by the standard row remains unresolved.
- `op1` and persistent-tag resolution. The mapping from absolute persistent CGM tags to serialized records remains unresolved for the consolidated `a5` family.
- The mapping from a standard `0x60` row's local allocation tag to its native edge record remains unresolved when no edge node carries the same curve identity.
- Standard-path topology membership across multiple separate FBB face groups.
- The standard-path arc branch is unspecified for arcs with no witnessed adjacent face.
- The `a5 03 20` `op1` or persistent-tag reference to serialized-record mapping is unspecified.
- The field split within the 62-byte numeric tail of `b2/b3/b4 03 62` owner packets and the owner packet's binding to a face record are unspecified.
- The `b2 03 28` layout-`0x62` token-to-3D cylinder frame mapping is unspecified.
- The semantic roles of counted `b2/b3/b4 03 61` references and tails, and of the long-form `61` prefix, members, references, and scalar, are unspecified.
- The higher-level object role of each `b2/b3/b4 03 5f` → `62` allocation-linked owner remains unspecified.

## Object stream

- Multi-surface `b5 03 5f` face semantics.
- The parameter equations and scalar roles of `b5 03 1a` and `1d` conic pcurves are unresolved.
- The field or relation fixing each `b5 03 5f` face's normal sense against its surface frame is unresolved. Closed endpoint chains determine coedge traversal but not this face-level sign.
- The object-stream body-kind and outward-shell sign fields are unresolved; one-body ownership and incidence determine a stable topology gauge but do not identify the source sign bytes.
- `b5 03 2d` bytes `+29..+76`.
- The operation names and semantic roles of the six control bytes and two scalars in `b5 03 37/3b` support-bound surface constructions are unresolved.

## Zero-entity `a9 03`

- The reference-lane rule associating a `0638` oriented use with its owner-local `21xx` support and `05xx` incidence record is unspecified.
- The fields that bind each `05 0b`/`05 10`/`05 15` incidence lane to physical-edge endpoints are unspecified.
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

- Binding the quotient of `u24be` endpoints by native identity to the counted coordinate rows.
- The record-family discriminator and following byte grammar are unspecified when a nested file contains an FBB-like run but lacks one or more required edge or vertex populations.
- Variant loop-node payloads outside the length-framed `b5 03 62` and `a8 03 62` forms are unspecified.
- The delimiter grammar of the marker-only `00 33 3X` surface path is unspecified.
