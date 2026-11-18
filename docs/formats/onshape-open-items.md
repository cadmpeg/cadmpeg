# Onshape bundle open items

## ONSHAPE-001 — Evaluated quantity slots

Question: Which numeric quantity slots establish a length or angle unit?

Known: Dimensioned expressions carry unit suffixes. Coordinate fields use metres. A quantity with an explicit supported `units` string has a scale.

Need: The numeric-slot unit encoding and its relation to expression text.

Conflict: An omitted quantity unit permits several scales.

Note: Unresolved quantities retain their feature as Native with `history.not-supported`.

## ONSHAPE-002 — Face sketch attachment frames

Question: Which source fields establish a sketch attachment origin and in-plane axis on a historical face or extrusion cap?

Known: A plane establishes a normal and a normal offset. A built-in plane establishes a complete sketch frame.

Need: The source attachment translation, rotation and sense rules.

Conflict: Translated and rotated sketch frames can share one plane.

Note: Unresolved attachments retain their feature as Native with `history.not-supported`.

## ONSHAPE-003 — Exact next-face coverage

Question: Which source topology forms permit an exact common first-intersection proof for `UP_TO_NEXT`?

Known: A next-face termination needs coverage of the complete selected region and the first face along every travel ray.

Need: Exact region coverage and intersection predicates for the admitted analytic carriers.

Conflict: Finite boundary samples can miss a narrow recess or an uncovered interior.

Note: `UP_TO_NEXT` retains its feature as Native with `history.not-supported`.

## ONSHAPE-004 — Thin wall sides and scope

Question: How do directed source curves and `flipWall` establish wall sides and a complete design scope?

Known: `midplane` selects a symmetric thickness. `thickness1` and `thickness2` carry two offsets. Open curves require directed left and right sides. A design feature belongs to one container in a product scope.

Need: The directed side mapping and complete design references for open and closed profiles.

Conflict: Inside and outside require a closed material boundary.

Note: Thin extrusions retain their feature as Native with `extrude.thin-not-supported`.

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
