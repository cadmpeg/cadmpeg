# Onshape bundle open items

## ONSHAPE-001 — Evaluated quantity slots

Question: Which numeric quantity slots establish a length or angle unit?

Known: Dimensioned expressions carry unit suffixes. Coordinate fields use metres. A quantity with an explicit supported `units` string has a scale.

Need: The numeric-slot unit encoding and its relation to expression text.

Conflict: An omitted quantity unit permits several scales.

Note: Unresolved quantities retain their feature as Native with `history.not-supported`.

## ONSHAPE-003 — Exact next-face coverage

Question: Which stored data establish the `UP_TO_NEXT` target for a `booleanScope` selection, and for non-planar or nearly parallel faces?

Known: Planar faces of live bodies with closed line boundaries establish exact first intersection and complete coverage for a closed convex line profile. Each body stores `type` and `consumedByComposite`.

Need: A stored statement of how a `booleanScope` selection limits the next face. Stored exact intersection data for a non-planar face, a loop that is not one closed chain of line edges, and a plane that is nearly parallel to the travel direction.

## ONSHAPE-004 — Thin sides of mixed curve groups

Question: Which source query fields establish the shared material side of a mixed curve group?

Known: Explicit wall flags and a closed carrier sense establish symmetric walls and one-curve sides, as does a stored line direction over an increasing parameter interval. A reversed interval is ONSHAPE-007.

Need: The selection order, orientation and join rules for mixed carrier groups.

Conflict: Different carrier directions can assign opposite material sides within one group.

Note: Unresolved thin forms retain their feature as Native with `extrude.thin-not-supported`.

## ONSHAPE-005 — Omitted hole construction facts

Question: Which source conventions establish an omitted drill-point angle, depth reference, thread hand and thread presentation?

Known: Explicit `tipAngle` and Boolean `depthToTip` establish an angled blind bottom. A standard value can carry `hand`, `modeled` and `cosmetic` facts.

Need: The omission rules and the source encodings for each hole standard family.

Conflict: Opposite hands and alternate depth references change the construction.

Note: Required unresolved construction facts retain the hole as Native with `history.not-supported`.

## ONSHAPE-006 — Negative revolve quantities

Question: How does a negative one-direction revolve quantity combine with the source axis and direction flag?

Known: Angular variables retain signed values in radians. A neutral one-sided revolve uses positive travel about its stated axis.

Need: The source rule for signed angular travel and its relation to `oppositeDirection`.

Conflict: An absolute parameter value contradicts its signed expression.

Note: A negative revolve quantity retains its feature as Native with `history.not-supported`.

## ONSHAPE-007 — Thin side of a reversed line interval

Question: Does a one-sided thin line use the directed profile or the stored `dirX` and `dirY` carrier when `endParam < startParam`?

Known: A decoded line runs from its `startParam` point to its `endParam` point. `WallSide::Right` is the right side of the open profile seen looking against its normal. The decoder maps a positive `thickness1`, zero `thickness2` and false `flipWall` to `right` for an open line.

Need: A stored one-sided thin extrude over a line with `endParam < startParam`, its sketch frame, wall flags and final STEP body. A matching increasing-interval case establishes the control.

Conflict: Reversing the parameter interval reverses the profile tangent relative to the stored carrier direction.

Note: The decoder retains its current side mapping until stored wall geometry establishes the source convention. A one-sided thin wall over a line with `endParam < startParam` reports `extrude.thin-not-supported` naming ONSHAPE-007 and the side assumed.

## ONSHAPE-010 — Sweep profile frame

Question: Which frame rule does `profileControl: "NONE"` apply along a curved sweep path, and which value applies when the parameter is omitted?

Known: A sweep stores cross-section queries in `profiles` and path queries in `path`. The `profileControl` enum has values `NONE`, `KEEP_ORIENTATION`, `LOCK_FACES`, and `LOCK_DIRECTION`. The lock controls carry query lists. The plain form stores false `hasTwist` and `hasScale` controls.

Need: Build an asymmetric profile and a path with bends in two planes. Save each profile-control value, then save the plain control with its parameter omitted. Compare the parameter rows and section frames in the final body.

Conflict: Different frame rules can rotate an asymmetric section about the path tangent.

Note: The decoder maps the plain form to a sweep operation.

## ONSHAPE-008 — Offset plane direction and alignment controls

Question: How do `oppositeDirection`, `flipNormal` and `flipAlignment` establish the offset plane frame?

Known: An `OFFSET` `cPlane` stores its support query in `entities` and its length in `offset`. Its controls include Boolean `oppositeDirection`, `flipNormal` and `flipAlignment`. With these controls false, the plane contains `o + d * n` and retains support normal `n`. `flipNormal` reverses the constructed normal. `flipAlignment` changes in-plane alignment, which the IR stores as `u_axis`.

Need: Build an offset plane on a default plane, place an asymmetric sketch on it, and save with each control changed separately. Compare the cPlane parameters, sketch attachment matrix and final geometry.

Conflict: Offset side, normal sense and in-plane alignment affect the placed sketch frame.

Note: A true direction or alignment control retains the feature as Native with `plane.not-supported`.

## ONSHAPE-009 — Offset plane on a planar face

Question: Which stored fields establish the oriented support frame of an `OFFSET` cPlane on a planar face?

Known: The `entities` query carries a deterministic face id. The rollback face stores `orientation` as `true` or `false` and a `PLANE` surface with `origin` in metres and a `normal` vector. A face sketch stores its attachment sense and in-plane axis in its `plane.normal` and `plane.x` fields. Coplanarity relates that attachment to the surface. The cPlane offset is a signed length.

Need: Build offset planes on planar faces with each orientation value, attach asymmetric sketches, and save with positive and negative offsets. Compare rollback surface normals, orientation values, sketch attachment matrices and final geometry.

Conflict: Reversing the support normal changes the offset side. The surface normal and orientation value leave the in-plane axis unresolved.

Note: A face-supported offset plane retains its feature as Native with `plane.not-supported` until its attachment frame is proved.

## ONSHAPE-040 — Face sketch attachment sense against face orientation

Question: Does the sign of the `sketchMatrix` normal of a face sketch follow the rollback face `orientation` value?

Known: The `sketchMatrix` normal sign and the face `orientation` value are both stored.

Need: Place sketches on faces whose `orientation` is `false` and on faces whose surface normal points into the solid. Compare the matrix normal with the surface normal and the orientation value.

## ONSHAPE-011 — Assembly mate frames and parameter quantities

Question: Which coordinate frame owns `matedCS` for a path through a
subassembly, and which feature parameters establish enabled mate limits,
offsets, and their signs relative to the evaluated connector frames?

Known: An assembly definition stores mate types and pairs of evaluated
connectors in `featureData.matedEntities`. `matedOccurrence` is an ordered
instance-ID path and can traverse a subassembly. Each `matedCS` stores
three-component `origin`, `xAxis`, `yAxis`, and `zAxis` arrays. Occurrence
transforms hold 16 row-major coefficients in the root assembly frame.
The feature response stores native mate parameters by `parameterId`.

Need: Save two instances of a subassembly with distinct rotations and a mate
to a child with an offset connector. Change the child's placement and compare
`matedCS` with the child and subassembly occurrence transforms. Save a two-part
revolute assembly with a lower angular limit, then an upper angular limit,
then an angular offset. Repeat with a slider's lower and upper travel limits.
Compare feature parameters and evaluated connector frames after each change.

Note: Nested frames use the part-local reading and charge
`assembly.mate-frame-assumed`. Unresolved solved sense charges
`assembly.mate-frame-unresolved`; a planar mate then retains its native
response. Enabled or unclassified parameters beyond the plain mate set charge
`assembly.mate-limits-unresolved` with their IDs. A boolean `enable` switch is
not charged. A switch set to `false` silences the limit or offset parameters
whose names carry its own qualifying words, and a switch with more qualifying
words than a parameter silences that parameter. Source responses remain assets.

## ONSHAPE-030 — Shell wall sense with opposite direction

Question: Does a stored `oppositeDirection` of `true` on a `shell` feature grow the wall outward from the original boundary, and which side of an opening face carries the wall?

Known: A `false` value places the wall inside the original boundary. The Onshape `shell` specification names the control "Opposite direction" and lists no other direction control.

Need: A stored shell with `oppositeDirection` set to `true`, its input body and its final STEP body.

Conflict: None.

Note: The decoder retains a shell with `oppositeDirection` set to `true`, the hollow form or an unlisted parameter as Native with `shell.not-supported` or `history.not-supported`.
