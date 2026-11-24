# Onshape bundle open items

## ONSHAPE-001 — Evaluated quantity slots

Question: Which numeric quantity slots establish a length or angle unit?

Known: Dimensioned expressions carry unit suffixes. Coordinate fields use metres. A quantity with an explicit supported `units` string has a scale.

Need: The numeric-slot unit encoding and its relation to expression text.

Conflict: An omitted quantity unit permits several scales.

Note: Unresolved quantities retain their feature as Native with `history.not-supported`.

## ONSHAPE-003 — Exact next-face coverage

Question: Which stored source operands identify the resolved `UP_TO_NEXT` target for curved profiles and multiple candidate faces?

Known: One stored planar target with a closed convex line boundary establishes exact first intersection and complete coverage for a closed convex line profile.

Need: Stored target-identity and boundary encodings for curved carriers and competing faces.

Conflict: Finite boundary samples can miss a narrow recess or an uncovered interior.

Note: Unresolved `UP_TO_NEXT` forms retain their feature as Native with `history.not-supported`.

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
