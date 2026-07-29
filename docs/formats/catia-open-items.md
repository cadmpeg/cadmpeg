# CATIA V5 `.CATPart`: Open Items

This document records `.CATPart` semantics that remain unresolved. The format specification contains only asserted rules.

## Container and roster

- The non-surface grammar and role of outer `01 00 04 00 <tag>` rows are unresolved. A literal marker scan does not establish that each row belongs to the freeform-surface alias roster or a vertex-registration roster.

## Container and roster (decoded-but-unresolved fields)

- The extent-struct `flags` word is carried raw; its bit assignments are unresolved.

## Design intent

- The relation between application-specific `CATFeatCont` object graphs and the
  core `CATPrtCont` design history is unresolved. Container class names and
  structural owner groups do not establish shared feature authorship.
- The semantic roles of the references, fixed-width words, and control bytes in inline `7C09` bodies are unresolved.
- The entity-identity binding from compound formula-instance roles such as `expression`, ordered inputs, and `paramout` to parser-version relation-expression and parameter entities is unresolved. Record adjacency and repeated compact value-packet selectors do not establish incidence.
- The referents of nonlocal `body` and `param` selectors on legacy typed relations are unresolved. The selectors do not use the containing legacy run's entity-identity space.
- The value production and entity binding for typed `String` relation inputs and results are unresolved. Schema-catalog type signatures alone do not carry a named parameter value.
- The value production and entity binding for typed `Boolean` parameters and active configuration state are unresolved. Boolean- and activity-named field classes include compound object payloads and do not identify scalar values.
- The instance grammar binding numeric tuples, `2DPoint`, `PRTSketch`, and `Sketch` schema fields into sketch identity, coordinates, geometry, and membership is unresolved. Field-class labels, structural ownership, catalog-entry equality, empty declarations, and inter-object references do not establish those semantics.
- The semantic roles of `PRTSketch` and `Sketch` atom, list, and reference payloads are unresolved.
- The incidence from complete `Range`/`CstAttr_Dimension` and `Range`/`ComplexCst` values to individual constraints is unresolved. Incoming `ListAggregator` references mix these identities with unrelated graph members and repeat identities outside the counted list, so they do not establish a constraint owner or operand relation.
- Constructed and support-face sketch placement frames, construction state, profile membership, sketch-geometry classes, dimensional constraints, and non-dimensional constraints are unresolved.
- The feature-instance grammar that binds empty mainstream operation fields, definition-bound values, and structurally owned operand objects into one ordered operation is unresolved. Operation-named field classes, definition schema names, and structural ownership alone do not establish feature identity, operands, outputs, or replay order.

## Standard nested `V5_CFV2`

- The semantic assignments of the four admitted `a5 03 32` header type codes are unresolved.
- The numeric continuation following the three aligned `a5 03 32` jet blocks has multiple length classes. Its lane counts, terminal fields, and relationship to the rolling-ball definition are unresolved.
- The semantic assignments of the width-coded `b2/b3/b4 03 5e` header token and terminal byte are unresolved.
- The field semantics of class-`0x18` descriptors, the operands and individual eight-scalar lanes in analytic-circle class-`0x23` edge definitions, the corresponding roles in standalone class-`0x24` records, and the class-`0x25` scalar lanes are unresolved.
- The internal coding of the sampled-cache lane in `a8 03 25` extrusion directrices is unresolved. Its enclosing references, solved parameter interval, and fit tolerance are defined independently of that cache.
- The byte relation assigning logical vertex components to `05 08 01` allocation rows is unspecified.
- Standard 3D spline cache poles and knots. Exact two-surface constructions use their native class-`20` pcurve jets and shared parameter interval when the standard tag closes through a class-`5e`/`23` dependency chain; the separate serialized 3D cache remains unresolved.
- `op1` and persistent-tag resolution outside the exact class-`19` analytic-circle identity binding. The mapping from absolute persistent CGM tags to other serialized records remains unresolved for the consolidated `a5` family.
- The mapping from a standard `0x60` row's local allocation tag to its native edge record remains unresolved when no edge node carries the same curve identity.
- Standard-path topology membership across multiple separate FBB face groups.
- The standard-path arc branch is unspecified when neither an adjacent face witness nor an exact two-support object-stream pcurve witness is present.
- The `a5 03 20` `op1` or persistent-tag reference to serialized-record mapping is unspecified.
- The semantic coordinate roles of the binary64 box and three binary32 bounds in the fixed `b2/b3/b4 03 62` owner tail, the five-byte header, and the owner packet's binding to a face record are unspecified.
- The semantic role of `pre_range_scalar` immediately before the active angular range in `b2 03 29` and `b5 03 29` cone records is unspecified.
- The semantic roles of the four `b2/b3/b4 03 18` parameter-point prefix selectors are unresolved.
- The internal roles of the reference-and-control program in `b2 03 3b` cone-face chart records are unresolved.
- The semantic roles of class-`0x60` group types `2` and `12..=21` are unspecified. Type `3` opens a cylinder chain.
- The semantic roles of the structurally typed counted `b2/b3/b4 03 61` references and tails, and of the long-form `61` prefix, monotone members, five persistent references, and scalar, are unspecified.
- The higher-level object role of each `b2/b3/b4 03 5f` → `62` allocation-linked owner remains unspecified.
- The semantic role of the `b2 03 2d` revolution record's `u16le` profile allocation identity remains unresolved independently of the exact unique profile-interval binding.

## Object stream

- Multi-surface `b5 03 5f` face semantics.
- The semantic distinction between `b5 03 5f` terminal controls `03` and `05` is unresolved.
- The field or relation fixing each `b5 03 5f` face's normal sense against its surface frame is unresolved. Closed endpoint chains determine coedge traversal but not this face-level sign.
- The object-stream body-kind and outward-shell sign fields are unresolved; one-body ownership and incidence determine a stable topology gauge but do not identify the source sign bytes.
- The semantic distinction among object-stream edge terminal controls `01`, `02`, `21`, `22`, `25`, `26`, `29`, and `2a` is unresolved.
- The semantic roles of the class-`62` secondary framing control and odd extended-metadata control are unresolved.
- The semantic distinction between object-stream vertex-incidence terminal controls `00` and `04` is unresolved.
- The semantic role of the positive scalar in the exact `b5 03 21` pcurve suffix is unresolved.
- The individual roles of the two decreasing auxiliary scalars in class-`2c` terminal forms `01 09` and `01 15` are unresolved. Their enclosing class-`30` construction supplies the exact result chart independently.
- The operation name and semantic roles of the six control bytes in `b5 03 37` support-bound surface constructions are unresolved.
- The operation name and semantic roles of the six controls and first scalar in `b5 03 3b` support-bound surface constructions are unresolved.
- The construction semantics of `b5 03 30` carrier kind `0x11` are unresolved. Its result reference may carry cone geometry, but the referenced cones do not satisfy the analytic parallel-offset distance equation.

## Zero-entity `a9 03`

- The semantic role of the terminal control byte in each `5fxx` face record is unresolved.
- The allocation-lane rule associating a `0638` oriented use with its owner-local `21xx` support and `05xx` incidence record is unspecified.
- The fields that bind each `05 0b`/`05 10`/`05 15` incidence allocation lane to physical-edge endpoints are unspecified.
- The semantic roles and namespace of the independent `T`, `X`, and `Y` allocations in each `5e 1a` tuple `[T,X,Y,T−1,T−2]` remain unresolved.

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
- The roles of the object-stream loop control, the second signed control per edge, and the ten optional numeric metadata fields are unresolved.
- The delimiter grammar of the marker-only `00 33 3X` surface path is unspecified.
