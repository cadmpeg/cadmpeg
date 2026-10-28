# Creo Parametric `.prt` (PSB): Open Items

This document lists the parts of the Creo Parametric `.prt` format that we do not know. The specification `creo_prt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

**Note.** The ND release comparison establishes header token values and
directory forms, not the numeric meanings of every producer declaration or
trailing TOC field. Need independent field semantics before using those
values as format selectors or interpreting them as dates, capabilities, or
checksums.

## 1. Scalar and array encoding

### SE-01. Custom relation units

**Question.** How does each custom unit symbol convert a curve-relation value to canonical units?

**Known.** `creo_prt.md` §8.3 "Square brackets following a numeric literal or parameter expression contain a" defines the built-in unit symbols, their dimensions, and their canonical conversions. Celsius and Fahrenheit use affine conversions.

**Need.** We must know each custom conversion to evaluate a relation that uses the symbol.

### SE-02. Owned custom affine-unit definitions

**Question.** Which complete basic custom-unit record binds its quantity type,
reference unit, scale, and offset to one model-local unit name?

**Known.** `creo_prt.md` §8.3 defines the reference-to-actual conversion and
its inverse. A nonzero offset belongs only to basic temperature. The separate
`xp_custom_sys_unit._conv_factors` array holds system-conversion pairs and
cannot assign an offset to a named custom unit.

**Need.** We must know the complete nonempty definition grammar and reference
ownership before resolving model-local affine units, including missing,
ambiguous, or cyclic references.

### SE-03. Other curve-equation frame transitions

**Question.** Which additional stateful productions are owned by a
curve-equation `local_sys` body beyond the defined explicit slot language?

**Known.** `creo_prt.md` §8.3 defines the curve-owned wrapper and twelve frame
slots. The complete rank-two image `18 e4 0f e4 18 e5 0f 18 e6` consumes nine
bytes and supplies all twelve slots. Its middle triple is zero; splitting its
`e4` bytes into generic unit scalars changes that rank. The next named field
bounds the owned body. Active and backup images of one saved equation are
persistence copies, not distinct transition productions.

**Need.** We must know the complete curve-owned byte extent, slot transition,
and independent placement constraints before admitting another stateful
production. A common `f9 04 03` wrapper does not establish that feature,
surface, or pcurve transition semantics apply.

### SE-04. Other pcurve array transitions

**Question.** What slot transition does each other stateful `crv_pnt_arr f9 02 04` token encode?

**Known.** `creo_prt.md` §4.1 "A direct curve body consisting of exactly eight scalar slots and no references" defines the direct eight-slot pcurve body, its endpoint order, standalone zero slots, and the exact held-scalar wrapper.

**Need.** We must know the other transitions to decode all pcurve endpoint arrays.

**Conflict.** A replay reading and a fixed-value reading of `e5` disagree. The
named body `48 20 00 18 e5 48 20 00 46 13 1e b8 51 eb 85 1f 9e 1e b8 51 eb 85
1f 18` belongs to a straight edge of length 4.78 between two planar faces. In
the second face, endpoint B is `[4.78, 0]`. The replay reading gives endpoint
A `[0, 0]` and the chord length 4.78 that the plane chart preserves; a fixed
`e5 = [0, 1]` gives a chord of length 4.884. The positional rows
`2f 20 00 46 13 1e b8 51 eb 85 1f 18 e4 2f 20 00 18 9e 1e b8 51 eb 85 1f 18`
and `48 20 00 46 13 1e b8 51 eb 85 1f 18 e4 2f 20 00 46 13 1e b8 51 eb 85 1f
2f 30 00 18` of straight edges with constant second face coordinate likewise
need `e4` to replay the preceding zero, not to store one.

### SE-07. Additional model scalar-heap forms

**Question.** Which numeric images do the unresolved model-scale and bounding
fields of the `Model_L05P` and `Model_L05_PX` slot grammar use, and which width
and role do the `b7`, `da`, and `e4` bounding-field forms have?

**Known.** `creo_prt.md` §2.3 defines the owning model and unit-system heaps,
logical counts, physical widths, the eight-byte and seven-byte bounding-field
openers, two-value `e5` repetition, zero-slot model control, and following
`string_xar` boundary. The model-scale image is one slot and is independent of
a preceding repeat. A following named-record header is not a null entry. The
unit-system heap has its own ten-scalar lane. A table that contains a `b7` or
`da` field reaches its declared count at `string_xar` with a seven-byte or an
eight-byte width for that opener, so this boundary does not select the width.
A one-byte `e4` field also reaches the boundary, but the bounding lane does not
define it as a literal one or as a one-copy repeat.

**Need.** We must know the numeric reconstruction of each seven-byte and `41`
bounding image and of the model-scale image, the widths of `b7` and `da`, and
the role of `e4` before extending the model grammar. An absent width rule must
leave the table unresolved; generic scalar widths and values do not apply by
prefix alone.

**Note.** The former SE-08 variable-length `e5` question is tracked here.
The defined model-heap repetition rule does not settle additional heap forms
or scalar roles in other fields.

### SE-09. Remaining plane-envelope scalar forms

**Question.** Which complete encodings, numeric values, and state transitions
apply to the remaining positional plane-envelope and bounded outline forms?

**Known.** `creo_prt.md` §3.4 defines the standard, compact, held-coordinate,
and planar-envelope forms. Section 8.1 defines the bounded datum-outline
`73` and `bb` mappings to IEEE high words `3f e8` and `bf e8`. Those two
mappings do not define every scalar prefix or every envelope field.

**Need.** Establish each remaining field's complete owner, token width,
logical slot count, numeric high word, and transition state. Validate its
value against independently owned geometry or a controlled native output.
Resolve the meaning of `e4` in each bounded envelope lane before applying a
literal or repetition rule from another lane.

**Conflict.** A generic scalar reader accepting a prefix does not establish
that prefix's meaning in every plane-envelope field. Complete field
consumption, retained opaque bytes, and a numerically plausible value do not
by themselves prove the field's numeric interpretation.

**Note.** The settled `73`/`bb` rule remains in the specification. This
question retains the other scalar and envelope roles; it does not define a
replacement prefix map or a global `e4` repetition rule.

### SE-11. Unresolved `00 <tail2>` solver-body tail

**Question.** Which solver identity or state does the tail of the bounded `00 <tail2>` variable body retain?

**Known.** `creo_prt.md` §5 defines its exact width, independent value and guess lanes, and unresolved inline state. A complete enabled section graph can resolve a row value separately; equal short bodies do not establish equal numerical values across sections.

**Need.** Define the owned tail fields and their joins before assigning additional solver semantics or replacing the unresolved lane state.

### SE-12. Unresolved `01 <tail3>` solver-body tail

**Question.** Which solver identity or state does the tail of the bounded `01 <tail3>` variable body retain?

**Known.** `creo_prt.md` §5 defines its exact width, independent value and guess lanes, and unresolved inline state. A complete enabled section graph can resolve a row value separately; equal short bodies do not establish equal numerical values across sections.

**Need.** Define the owned tail fields and their joins before assigning additional solver semantics or replacing the unresolved lane state.

### SE-13. Unresolved `34 <tail2>` solver-body tail

**Question.** Which solver identity or state does the tail of the bounded `34 <tail2>` variable body retain?

**Known.** `creo_prt.md` §5 defines its exact width, independent value and guess lanes, and unresolved inline state. A complete enabled section graph can resolve a row value separately; equal short bodies do not establish equal numerical values across sections.

**Need.** Define the owned tail fields and their joins before assigning additional solver semantics or replacing the unresolved lane state.

### SE-14. Other principal unit selectors

**Question.** Which coordinate unit systems do unassigned compact selectors,
including `49`, `52`, and `53`, identify, and what complete model-local unit
records resolve a custom-system length reference?

**Known.** Selectors `50`, `51`, `54`, and `55` select CGS, mmNs, inLbmS, and
mmKs. CGS lengths scale by `10` and inch lengths by `25.4` to canonical
millimeters. A custom selector can reference a model-local system `_id`;
`a7 10` is the compact integer `10000`. The system's unit names and object
references are distinct from public API unit-enum ordinals. A configuration
option list does not define the integer serialization of its entries.

**Need.** We must resolve each active selector and its length definition before
converting coordinate lengths. Unassigned selectors and incomplete custom-unit
references must not select a scale from configuration-list order, a principal
display name, or an API-enum cast.

## 2. Curves and surfaces

### GS-01. Cone half-angle overrides

**Question.** What complete grammar, scalar roles, and placement apply to per-instance cone bodies outside the terminal positive-DICT, complete support-apex, and complete planar-envelope productions?

**Known.** `creo_prt.md` §3.2 defines the terminal positive-DICT half-angle, the complete support-apex operand whose angle need not terminate the row, and the planar-envelope distance/radial-bound construction. A lexical angle followed by `e3` does not select the support-apex form without its complete preceding fields.

**Need.** Establish each remaining body's complete boundaries, scalar roles, and unique carrier construction before deriving its angle or placement. Resolve the support-apex three-byte field before assigning it a geometric role. Agreement on an angle alone does not establish ownership or a frame.

### GS-02. Torus and sphere radius overrides

**Question.** What complete grammar and placement rule apply to a `geom_type = 26` body outside the tagged radius trailer, complete inline local-system plus radius pair, and terminal prototype-minor-radius replay forms?

**Known.** `creo_prt.md` §3.3 "A `srf_prim_ptr(torus)` prototype stores" through `creo_prt.md` §3.3 "In named `radius`, `radius1`, and `radius2` fields" define the recognized torus and sphere radius forms and their carrier invariants.

**Need.** Define the remaining body grammar and a unique placement witness before constructing a torus or sphere carrier from those forms. An agreeing inline radius pair does not resolve conflicting origin or direction readings.

**Conflict.** The decoder reads a type-26 radius slot with the `0d` and `2d` meanings only for the prototype-minor-radius replay. Its carrier path reads the generic suffix lane, so `0d` and `2d` rows get no torus carrier. With the radius-slot meanings, the CTC-04 rows with a `2d` major radius and the CTC-05 rows with a `0d` minor radius place tori that agree with the STEP siblings, but two rows place the origin with one coordinate of the wrong sign: CTC-04 surface 15784 reads origin `y` from `2d 77 dc a8 7a 1e 8f 83` as -381.79 (STEP +381.79), and headset-hook-holder surface 437 reads origin `z` from `0e` as -0.5 in (STEP +0.5 in). The same `2d` origin-`y` reading misplaces about thirty torus and cylinder carriers that the decoder already emits in NIST CTC-02, CTC-04, FTC-10, mic-clip, panel-lamp, resistor, and thyrister. Adding the positive `2d` reading as another origin candidate withholds correct carriers in heat-sink, mouse, and other parts, so the inline origin lane needs its own rule before the carrier path takes the radius-slot meanings.

### GS-06. Prototype-adjacent `tab_cyl` parameters

**Question.** What does each parameter in a prototype-adjacent `tab_cyl` instance row control?

**Known.** `creo_prt.md` §3.2 defines the complete type-19, degree-three, four-control-point knot production and the separate interpolation-point parameter production. The knot production fixes the directrix domain `[0, 1]`. It does not establish the roles of other instance parameters.

**Need.** Establish the remaining instance parameter roles and their ownership before deriving a surface chart or placement.

### GS-07. Ambiguous `tab_cyl` placement

**Question.** How does a replay-bound `tab_cyl` select its placement when its axis span matches neither directrix-coordinate range uniquely?

**Known.** `creo_prt.md` §3.2 "A repeated `tab_cyl` cubic-curve replay has this structure:" defines the repeated cubic replay, the unique axis-span placement cases, and the paired short second-coordinate form. A span mismatch caused by the unpaired reading of a paired short token is not a placement ambiguity.

**Need.** Establish whether each remaining span mismatch has a scalar cause before a selection rule is defined, and place the surface in model space only through a unique chart.

### GS-08. Unresolved `fc 02` bodies

**Question.** Which scalar images or additional body productions resolve an `fc 02` body that does not satisfy the complete counted or complete short production?

**Known.** The complete counted production has eight endpoint slots in two face charts. The complete short production supplies one face path. The bounded microphone and heat-sink verification resolves 72 counted bodies and two short bodies; 68 other bodies remain opaque. An opaque body can contain an undefined scalar in a known production. Opacity alone does not prove another grammar.

**Need.** Resolve each remaining body through complete scalar ownership and chart roles before it supplies curve or endpoint geometry.

### GS-09. Other `fc 05` variants

**Question.** What grammar does an `fc 05` body use when it does not satisfy the defined cap-circle production?

**Known.** `creo_prt.md` §4.2 "`fc 05` records store cap-circle control points in the order `A`, `B`, `t`, `C`, where `A` and" through `creo_prt.md` §4.2 "One `fc 05`" define complete point groups, scalar lanes, termination, cylinder binding, placement, and circle construction for cap-circle bodies.

**Need.** We must know the other grammar to construct or reject the curve correctly.

### GS-11. `fc 13` scalar signs and model-space charts

**Question.** How do the remaining undefined scalar images, signed coordinate branches, and native chart transforms define complete placed paths?

**Known.** The complete point-first and parameter-first productions associate one parameter with three coordinate roles. Their shortened terminal group completes the preceding parameter's sample. These roles do not prove a spatial control polyline, NURBS control poles, or a native-to-model transform. A radius identity alone does not select a signed branch. The panel 687 path retains unresolved branch signs; the CTC02 and microphone role proofs do not establish their complete native model-space placements. No named nineteen-by-four array declaration establishes a universal interpretation of `13` as a sample count.

**Need.** Resolve the undefined images and derive each complete signed chart from native ownership before constructing placed geometry.

### GS-13. Incomplete counted pcurve bodies

**Question.** Which remaining scalar and enclosing-row forms resolve a counted two-chart body when exact `4N` scalar consumption cannot be established, and which first-coordinate prefixes of a counted body read differently from the two-chart lane?

**Known.** `creo_prt.md` defines `fc <count>` with four finite chart scalars per sample and no trailer, and the `2d`, `46`, and positive-DICT readings of bodies with three or more samples. Counts `4`, `7`, `8`, `9`, and `10` do not identify separate curve families. A unique complete counted body fixes a competing topology-suffix boundary without requiring the referenced face to be materialized. In two-sample `fc 02` bodies the `46` sign is mixed, so those bodies keep the two-chart lane.

**Need.** Define the missing scalar and enclosing-row productions before assigning values or selecting a suffix from incomplete or conflicting body readings. Check the first-coordinate prefixes `31`, `45`, `4a`, `4e`, `4f`, `52`, `54`, `58..5a`, `71`, `9e`, `a3`, `af`, `b0`, `b1`, `b9`, `bf`, `d1`, `d3`, `de`, and `df`, where the first tabulated-cylinder lane and the two-chart lane differ.

**Note.** In resistor-100w-aluminum-finned, about 370 vertices that the decoder solves from two-chart samples lie off the STEP sibling in the model frame with either sign reading. They fit STEP only under an axis permutation and translation, and the finned body is symmetric enough that a mirrored set fits as well, so an alignment search does not decide the sample sign there.

### GS-16. Model-reference entity types and conic framing

**Question.** Which complete entity grammars own the unresolved positional
records after a conic declaration, and how do their types select carriers?

**Known.** Type 30 identifies a model-reference ellipse. A preceding conic-list
declaration does not identify a later row's type or grammar. The solid-entity
and sketch-entity type namespaces are distinct. An integer type alone does not
establish a parabola or hyperbola. Ellipse construction requires the complete
frame, coefficients, and endpoint invariants in `creo_prt.md` §8.5.

**Need.** Define each complete owned record production before assigning its
fields or carrier. Resolve positional schema references before the flip field
and the complete twelve-slot local system. Agreement of the center and radii
alone does not establish the frame or endpoints. Preserve unresolved rows
without inheriting a grammar from a preceding list declaration.

### GS-17. Other positional cylinders

**Question.** What model-space equation does each positional cylinder body outside the defined analytic and generated round-edge forms encode?

**Known.** `creo_prt.md` §3.2 "`srf_prim_ptr` records contain the surface prototype fields" through `creo_prt.md` §3.2 "Positional cylinder rows store cap-plane point data rather than a `local_sys` replay." define the recognized cylinder row families and their placement invariants, including the `11 10 13` placement-witnessed inline cylinder and generated type-24 round-edge endpoint bodies. The first directrix-coordinate positive DICT lattice covers prefixes `4b..a3`.

In the seven-scalar directrix-coordinate lane, the first scalar is positive. It
usually equals the axial corner span: example pairs include `1.7` and `1.7`,
and `0.74` and `0.7399999999999998`. It does not always equal that span: one
example has a first scalar of `1.78` and an axial corner span of `1.68`. Its
role is not known.

A fully framed cap-envelope body owned by a unique cylinder row and resolved
class 932 remains opaque. A bounded station/envelope and a radial suffix can
agree with stored circles without defining the chart direction, phase, or
initial control roles. The generic terminal-support frame does not establish
those roles. Incomplete centers remain unresolved; an absent center cannot be
replaced by an envelope midpoint. Raw body bytes retain the unsupported fields.

A second, independently placed chart can define an infinite line even when
the first chart remains opaque. Line agreement does not establish its finite
endpoints or a trim interval. The current curve-wide veto suppresses both
charts.

In the FTC-08 and STC-08 pocket feature, the cap-adjacent rulings are
two-sample two-chart rows on the end cylinders. Their errors do not depend on
the cap chart. A ruling has the correct line when its end cylinder has the
correct placement. The two mirrored end cylinders also displace rulings that
have no cap face. A ruling endpoint has the correct position when its
end-cylinder coordinate is the literal `18`. The image `4f 47 ae 14 7a e1 48`
reads as 0.1584375, but the pocket depth is 0.135. The same image puts the
pocket floor too deep on rows that have no cap face. The image
`41 c1 47 ae 14 7a e1 48` reads as 0.135 on the end cylinders whose decoded
axis is +z, but the endpoint is 0.135 below the top face. No property of a
cap-adjacent row separates its correct carriers and endpoints from its
incorrect ones.

The selector-corner interval family reconstructs its unique axis line, radius,
and extent from two directrix parameters, two XYZ corners, and the selector
pair. Its split-selector and one-placeholder variants use the same equation.
The four-bound inline family selects either the positional row outline lane or
the signed first-directrix outline lane from its first outline token. Both
forms reconstruct a complete carrier from the inline local-system suffix.
Class-913 axial-interval corner bodies without a quadrant selector retain four
axis-line candidates. A unique maximum count of adjacent tangent support planes
selects the carrier; a tied or zero maximum does not.
Their common transverse span is a rolling-radius sample. In a
round-edge endpoint body, the unique pair of equal nonzero endpoint-coordinate
deltas supplies the perpendicular-round radius. In a class-913 generated-round
context, axial-interval and endpoint bodies do not fall through to scalar-frame
or terminal-suffix radius grammars. Outside that context, a byte-compatible
body does not establish endpoint ownership.

Class-913 generated round-edge replays carry a radius candidate in a bounded
`cr_flags_xar` positional record. Its `f2 f7 80 a0` opener, following
`01 f6` compact-field terminator, first `0x29` short-form scalar, and next
`f3 f7 80 97 e2` boundary are defined in `creo_prt.md` §3.2. The candidate
is used only with agreeing observed rolling-radius and placed-cylinder
witnesses; it does not by itself define the remaining carrier equation.

**Need.** We must know the equation to construct the cylinder carrier. Separate
independent carrier evidence from endpoint and topology authority before
recovering a curve adjacent to a withheld cap chart. Prove the complete chart
record, its owned coordinate lanes, and each promoted quantity. Do not restore
finite vertices from agreement of infinite carriers. Resolve the `4f <tail6>`
and `41 <tail7>` second-coordinate images in pcurve chart slots, the
end-cylinder axis sign, and the mirrored end-cylinder placement before the
veto is narrowed.

### GS-19. Unresolved cone origin-coordinate images

**Question.** Which owned scalar or model-reference grammar resolves the remaining cone origin-coordinate images and establishes their placement?

**Known.** `creo_prt.md` §3.2 defines the twelve-slot owner and the zero/reference/short origin production. Its last image is `origin[2]`, not a station parameter. The bounded `2f3400` value is source-length 20; the CTC05 inch selector converts it to 508 mm. Independent resistor STEP apices constrain the checked `1b`/`34` instances to zero Z but supply no raw-only formula. The CTC05 raw origin is `18`, `19c60fafbfd97304`, `2f3400`; interpreting the first two as independent generic numeric lanes gives X/Y values that disagree with the sibling STEP. Complete seven-byte scalar owners also prohibit splitting their suffix bytes into another coordinate.

**Need.** Resolve the reference-backed origin through its complete native owner before admitting a placed cone. Preserve unresolved bytes and withhold dependent geometry. Do not map a prefix to zero from the checked resistor instances or use a STEP fit as the runtime decoder.

### GS-21. Non-prismatic round radii

**Question.** Which class-913 instance fields encode round sets, their shape options, ordered governing-point radius entries, and radius-leg values or references?

**Known.** `creo_prt.md` §6 defines the per-set variable-round element contract and the resolved constant-radius forms. The named `data(subft_rndset)` definition retains chain, spine, point-set, and subset fields. Its point-set fields do not establish native instance station ownership. Unequal generated carrier radii define neither set boundaries nor a variable-radius law.

**Need.** A native instance with independently known element-tree values must establish set-array boundaries, the shape-option selector, radius-entry order, governing-point references, and the two leg grammars.

### GS-22. Native fillet spline values and instance chart

**Question.** Which complete direct and shared field trees define numeric fillet spline values, parameter domains and instance charts?

**Known.** `creo_prt.md` §3.2 defines the published three-spline carrier equation and independent contour roles. The direct headset body bounds three arrays of 25 interpolation points, two endpoint tangents per spline, and byte-identical strictly increasing parameter arrays. The center and tangent bodies have no complete numeric projection. Both the signed-coordinate and sign-cleared rivals violate unit-tangent and perpendicular-radius invariants. Five checked shared-form prefixes start with `shrsrf_data`; they do not prove a complete shared-reference join or the absence of later fields. Current native faces 374 and 468 both have closed four-edge cycles and unknown carriers.

**Need.** Establish the complete positional scalar lanes and direct/shared ownership from native numeric evidence or a controlled asymmetric Toolkit oracle. Resolve the unique instance chart, reflection and domain before constructing the fillet. Do not select signs from continuity or a mirror-symmetric STEP fit.

### GS-23. Round carrier ownership and generated-entry extents

**Question.** Which complete native fields establish carrier/sense inheritance for a row-unresolved generated face, and which body grammar bounds the final plain generated-entity entry?

**Known.** `creo_prt.md` §6 defines complete generated-result identities through unique, same-owner surface rows across all owned tables. Current face-side identity and independent B-rep admission are separate joins. The bounded round example reproduces its complete nine-member result roster. This does not establish the general rowless-cylinder carrier claim or the final plain-entry grammar.

**Need.** Obtain complete source-owned rowless carrier/sense discriminators and plain-entry field consumption or a positively owned enclosing boundary. Preserve the observed overlap with a candidate following table header as an unresolved extent issue. A raw `f2 f7` search, a fixed byte subtraction, or table adjacency is not a boundary or carrier proof. Historical topology/census counts and inactive-item selection remain separate from this verified result-identity rule.

## 3. Section solving and feature placement

**Note.** The extended `order_table.rg_ref_id` form is defined only for its
null `f6` slot. Need the referenced namespace, owner, and non-null value
grammar before transferring a non-null operand. Need a complete ownership
and replay comparison to explain changes in generated-entity and
affected-geometry replay counts across the release saves. Equal neutral
feature definitions do not establish completeness of those native tables.

### SP-01. Nonlinear simultaneous equations

**Question.** How must a nonlinear curve-equation `SOLVE` block be solved?

**Known.** `creo_prt.md` §8.3 "`SOLVE` opens a simultaneous-equation block and `FOR` followed by one or more" defines the block framing. The decoder solves complete, dimensionally valid affine systems and smooth numeric nonlinear systems when one finite full-rank root is established, and retains other blocks.

**Need.** Obtain saved programs with regenerated numeric or geometry witnesses for non-smooth, piecewise, and discrete forms that do not reduce affinely. Vary preceding unknown values, equation order, and `FOR` variable order. The witnesses must distinguish the selected root or branch from nearest-seed and fixed-order rules.

**Conflict.** A simultaneous system can have more than one mathematical result set. The returned set does not by itself identify the initialization or branch rule.

**Note.** Preserve an unsolved block and invalidate its preceding unknown values until the complete source semantics supplies a unique transfer.

### SP-03. Typed section-plane identity and pointer ownership

**Question.** What complete typed pointer grammar and representation preserve a
section's direct model-plane identity independently of its owner and placement?

**Known.** A positional `gsec3d_ptr` stores its outer plane after the complete
owner, first-chain, and quilt fields. Inline chain data and a pointer's class
reference do not supply that plane ID. Five independently bounded residual
sections join one typed nonvisible model-plane row despite zero or four
complete generated-datum tables. Their incomplete plane parameters and absent
saved location leave placement unresolved. A same-integer non-plane row and
multiple typed-plane candidates do not define a unique binding.

**Need.** Establish complete inline and class-qualified pointer ownership before
reading their outer fields. Preserve a unique typed model-item identity in an
explicit representation without choosing a table owner, inferring an origin,
or converting that identity into a placement frame. A partial pointer parse
must not resume at a plausible interior integer.

### SP-04. Other relation equations

**Note.** The relation discriminator is not a universal equation opcode. Type
`9` can join distinct incidence kinds; type `12` can join two endpoint
tangencies. Type `22` can refer to a point whose carrier is supplied only by the
incidence join. Types `25` and `38` do not acquire the type-14 radius equation
from similar operand vectors. A type-26 mixed real operand does not establish
the full bounded-curve equation. Unresolved rows retain their raw body and
source identity.

**Question.** What complete operand roles and private scalar equations apply to
relation types `9`, `10`, `12`, `13`, `21`, `22`, `25`, `26`, `38`, and other
unassigned types when no complete incidence interpretation supplies a neutral
constraint?

**Known.** `creo_prt.md` §5 "Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer" through `creo_prt.md` §5 "A positive-ratio elliptical cone uses local frame coordinates" define the recognized linear, radius, incidence, and entity-geometry relations. Complete `eqtn_arr` function-0, function-2, function-3, and function-35 rows define radial endpoint, scalar equality, unsigned coordinate distance, radius binding, and point-on-line equations when their positional row grammars are complete.
The supported type-six relation form has sign `1`,
`a=[first_point,second_point,0,1]`, `b=[center_point,0,0,0]`, and a complete
four-slot `c` selector; it binds a complete linear dimension to the unique arc
with the matching endpoint pair, center, and radius dimension index. Type four
produces a diameter constraint; the other linear dimension types produce a
radius constraint. An incomplete or ambiguous form remains native.
Function-13 rows with two type-2 point ordinates and a zero type-7 auxiliary
row define a same-coordinate equation.
Function-33 rows with four type-1/type-2 coordinate pairs identifying two
endpoint pairs and a zero type-7 auxiliary row define equality of the two
squared endpoint-pair lengths.
Function-6 rows with two complete type-1/type-2 point pairs and a type-3
scalar define their positive Euclidean distance.
Function-42 rows define the arithmetic-mean relation between two same-axis
point coordinates and a type-6 scalar. Function-31 rows bind one type-1/type-2
point pair to two type-6 coordinate scalars. Function-43 rows define the
eight-slot axis-distance form with two point pairs, two type-4-or-type-5
auxiliary rows, a type-0 distance row, and a type-5 auxiliary row. The
non-negative type-0 value transfers when it agrees with exactly one absolute
coordinate difference; a missing value transfers only when exactly one
coordinate difference is non-zero. Function-16 direct rows with two type-4
angle rows, a type-0 result row, and a zero type-5 selector transfer the
non-negative first-minus-second angle difference when it is at most π; a
missing result transfers from the two finite angles. Other function-16 forms
and other function-43 forms remain native. Function-10 seven-slot rows with
same-type selected coordinates for two distinct points and a target, opposite
type coordinates for the two reference points, and zero type-7 auxiliaries
transfer the target's missing opposite ordinate to the shared reference
ordinate when the selected ordinates differ and all other point identities and
coordinates are complete and unambiguous. Other function-10 forms remain
native.

**Need.** We must know each equation to solve the section geometry.

### SP-05. Type-1 direction selectors

**Question.** How does the complete `b`, `c`, and `sign` selector tuple choose the rays and their order for a type-1 angular relation?

**Known.** The relation retains its operand vectors, complete raw body, selector, and selected angular dimension separately. The line operands bind through the section order table. A scalar projection of `c` does not establish its direction semantics.

**Need.** Obtain regenerated section saves with fixed ordered line entities and endpoint order. Change only the dimension label sector, then reverse each line independently. Retain the ordered dimension references, point types, label location, and value from the section API. Compare the complete source tuple for each save.

**Conflict.** The angular magnitude and the stored `sign` do not establish the selected rays or a direction convention. A rule for labelled legacy sections does not establish the binary selector codebook.

**Note.** Retain the complete selector tuple. Transfer a directed target constraint only when its ray and direction semantics are independently resolved.

### SP-06. Other type-35 operands

**Question.** What independent family proof distinguishes a point entity from
a circular carrier when one solver-only identity has both sense-zero and
sense-four point-on-object roles?

**Known.** Section-entity identifiers and relation identifiers occupy separate
namespaces. An identity absent from the complete segment table remains scoped
to its sketch. A unique unary type-one or type-two incidence proves a line
independently of activity. In a type-35 incidence, that line can be the
sense-zero midpoint target while sense four on a centered construction line
selects the point locus. The type-35 role does not prove the missing entity is
a point. Mixed sense-zero and sense-four incidence roles do not by themselves
establish a unique point-versus-circular carrier family.

**Need.** Establish the referent family independently. A centered line's point
locus also needs a compatible center representation. A resolved line does not
accept `Center`; a native line can retain that symbolic locus without numerical
coordinates. Neither case permits an endpoint substitution or an invented point.

### SP-07. Non-coordinate full-width solver roles

**Question.** What numerical or solver role does a complete `ed <tail8>` image have in a variable outside coordinate types `1` and `2`?

**Known.** `creo_prt.md` §5 "Within each" defines the complete nine-byte full-width image. Coordinate types `1` and `2` decode its finite binary64 payload; other variable types retain an opaque scalar role. The image alone does not establish a dimension-driven state.

**Need.** Define the complete owning field and variable-type contracts before numerically admitting an opaque full-width variable or assigning solver state to it.

### SP-08. Other `local_sys` owners

**Question.** What geometric role does a feature-definition `local_sys` frame
have when it is neither the placement-record frame nor the section-location
frame?

**Known.** `creo_prt.md` §6 "Feature-definition `local_sys f9 04 03` and
`transf f9 04 03` bodies use the" defines the twelve-slot feature frame, the
placement-record frame, and the section-location frame. A frame that follows a
saved section result and precedes `entities_to_hide` does not define the
section plane.

**Need.** Identify the enclosing record of each such frame before a consumer
uses it.

### SP-09. Multiple `transf` frames

**Question.** Which owner-specific role distinguishes complete `transf` frames if one bounded feature definition contains more than one?

**Known.** `creo_prt.md` §6 "Feature-definition `local_sys f9 04 03` and `transf f9 04 03` bodies use the" defines the twelve-slot body. A frame belongs to its bounded source definition and nested owner. A field with the same name outside that definition is not a competing frame.

**Need.** Obtain a definition that contains multiple complete transforms with independently identified owners and geometric results. Distinguish those roles before selecting a section-to-model transform.

**Note.** An absent, incomplete, or ambiguous owner-specific transform does not permit a file-wide lookup or selection by field order.

### SP-10. Selection within one frame owner

**Question.** If one parsed owning object contains multiple complete frames of
one kind, what instance identity, index, or selector distinguishes the field
required by its consumer?

**Known.** Complete `local_sys` and `transf` fields under different owners do
not form one selection array. The placement record, each transformation-array
entry, and the section saved-result retain separate owner/kind identities.
Order, equality, or completeness across owners cannot establish a section
location frame. A uniquely owned section frame is independent of unrelated
feature-placement and generated-carrier evidence.

**Need.** We must establish an actual within-owner multiple-frame grammar
before selecting one such instance. Missing, incomplete, or ambiguous evidence
in the required owner must remain unresolved; a complete sibling object is not
a fallback.

### SP-11. `relat_ptr` operand-vector roles

**Question.** Which remaining type-specific tuple shapes and selectors have transferable reference or scalar roles?

**Known.** The three tuples use relation-type-specific namespaces. Point keys, order-table internal identifiers, radius keys, selectors, nulls, and scalar coefficients are distinct roles. Complete source tuples remain native data. Only independently joined slots supply object references; joined `skamp_ptr` items supply entity and sense operands separately.

**Need.** Obtain complete tuple productions with independently bounded point, order, radius, and incidence joins for each unresolved shape. Preserve scalar coefficients and selector values without treating them as object indices.

**Note.** A tuple position or integer value alone does not establish a namespace. Identical tuple shapes under different relation types do not inherit each other's semantics.

### SP-12. Full-width non-coordinate variable roles

**Question.** Which independently owned scalar roles assign numeric semantics to full-width `var_arr` images outside point-coordinate types `1` and `2`?

**Known.** `creo_prt.md` §5 defines complete nine-byte framing and numeric coordinate roles. Other full-width variable fields retain their exact images as opaque values. A finite IEEE reconstruction alone does not establish a numeric role. Opaque values do not select dimensions or receive equation-derived replacements. Point keys, radius keys, variable ordinals, dimension selectors, and external identifiers remain distinct namespaces.

**Need.** Obtain complete source productions and independent saved-value or authored-property joins for each remaining variable role. Preserve physical framing and source values while those semantics remain unresolved.

### SP-14. Unary point incidence

**Question.** What independent geometric meaning applies to a unary type-one
or type-two sense-zero incidence on a type-5 `segtab` point when `verhor` is
absent or conflicts and no complete type-fourteen symmetry-axis role exists?

**Known.** A unary orientation incidence and agreeing point `verhor` can
persist without type-fourteen axis use. Such records retain the point and two
separate native constraints. A degenerate symmetry axis instead requires its
complete agreeing orientation and type-fourteen conjunction. Missing or
conflicting orientation evidence does not establish an implicit line or origin.
Entity-orientation and point-alignment constraint families are distinct.

**Need.** We must know the constraint to transfer it without inventing an axis.

### SP-16. Other `skamp_ptr` geometry families

**Question.** Which complete owner or incidence join establishes the family of an unresolved projected or solver-only section identity?

**Known.** Entity identifiers are sketch-local. A type-58 owner identifies conic family independently of its stored selector and locus sense. The defined point, endpoint-bearing curve, line, arc, and circle roles require their independent owner or incidence evidence.

**Need.** Establish complete copied-result ownership for projected identities and complete, compatible incidence roles for ownerless identities. A saved result with the same integer in another definition does not establish a join.

**Note.** A missing or ambiguous owner and an unproved locus sense retain native geometry. Family does not follow from the numeric entity identifier or a single center-like sense.

**Conflict.** `creo_prt.md` §5 states that sense `4` establishes a native
circular family, and a solver-only identity with only sense-four roles takes
that family. Sense four also selects a locus on entities that are not circular:
a type-0 incidence stores the `segtab` type-5 point row `05 00 00 f6 f6 11` as
item `11 04`, and a centered type-47 construction line has a sense-four center.
A solver-only identity that is the sense-zero point of a type-35 midpoint row
and also takes sense four therefore has both point and circular evidence and
remains unresolved.

### SP-17. Solver-only external references

**Question.** What complete declaration and replay grammar certifies
`ref_planes` rows and their projection, including mixed model-item namespaces,
a collection qualifier, or non-null `ext_ref_id` or `sub_index`?

**Known.** A complete `gsec3d_ptr.ref_planes` row binds its fourth field,
`known_ext_id` or `seg_id`, to the known section-entity identifier. The typed
external target is `(ref_type, plane_id)`. The six-field form adds `sub_index`
and `flip_flag`. Surface and edge references are distinct from table-class,
point-key, relation, and dimension namespaces. Missing local `segtab` geometry
does not prove an external binding. A counted class reference is not a
model-item identifier. The section location frame is a separate projection
input. A row declaration can continue after `flip_flag` with a
`save_entity_ptr(line)` object. A six-integer prefix and a declared row count
do not certify that complete declaration or its replay binding.

**Need.** Establish the selected row-class declaration, field order, complete
row boundaries, declared table cardinality, and all nested fields before
looking up and projecting the typed model item. Missing or multiple candidate
rows, unknown types, unhandled qualifiers, or an unresolved section frame must
retain native geometry instead of selecting a same-valued datum or class ID.

### SP-18. `exists()` serialized owner graph

**Question.** Which serialized owner records bind a relation context to complete model, feature, component, and section item namespaces?

**Known.** Exact complete-program targets establish existence even when their values are unresolved. A numeric model qualifier selects the zero-based `ref_part_table` row ordinal, not its stored `id`. An `FID_` qualifier selects a feature inside the selected model before the base item selects that feature's parameter or measurement. `CID_` selects a component occurrence by its internal component feature ID. Session and component identities are distinct. Qualifiers resolve from right to left. Section `sd#`, `rsd#`, and `kd#` identities belong to the current section. Parent-model dimensions use `d#` or `rd#`.

**Need.** Bind the current relation owner and referenced documents without filename guessing. Preserve model parameters, feature parameters and measurements, occurrence parameters, and dimension namespaces separately. Resolve name-based features only when unique. Record namespace completeness independently of values. A proved member returns one; a missing member returns zero only in a complete resolved namespace. Missing documents, ambiguous owners, unsupported qualifiers, and incomplete namespaces remain unresolved. Preserve exact program targets before external owner resolution.

### SP-19. External and designated sweep-axis references

**Question.** Which owned fields distinguish an explicitly designated or external revolution axis from the internal centerline, and retain its exact typed model-geometry reference?

**Known.** Absence of `ActDatums` does not select an operation axis. An independently owned placed section supplies the extrusion sweep line. The bounded internal revolution path requires complete tables and one nondegenerate resolved `u=0` line; the nested `gsec3d_ptr.ref_planes` section-entity identifier is not its axis selector. A resolved saved-section frame without an exact operation owner remains insufficient. PTC separately permits designated, geometry, construction, and external linear references; their selection priority does not establish their native field encoding.

**Need.** A positively owned designated/external-axis witness and independent geometry oracle must establish the selector and typed reference without assigning a nearby saved section or interpreting a reference-table identifier as a centerline.

### SP-20. Complete placement-instruction reconciliation

**Question.** Which complete bounded placement-instruction grammar preserves competing nonzero or unresolved offset states before the zero-offset standard-datum placement test?

**Known.** A type-20127 zero-offset instruction with the reference datum as its first geometry operand can select a sketch plane through the exact standard-datum guards in §6. This rule does not select a sweep axis or its sign. Section-plane orientation and section origin require their own geometric evidence. The interactive default orientation convention applies after selecting a sketch plane; it does not supply a missing native sweep direction.

**Need.** Bound all instantiated instructions to their declared class and owning table. Retain each complete row's offset state and reject this fallback when owned instructions disagree or a candidate's offset remains unresolved. Preserve independently proved section transforms and generated-cap-pair placement as separate mechanisms.

### SP-21. Physical row ownership of repeated section-plane records

**Question.** What complete boundary and root-schema grammar safely discovers every physical `AllFeatur` row before decoded feature-namespace admission, and bounds positional section definitions without extending an earlier owner through an omitted row?

**Known.** The bounded class-916 rows with a shared sketch-plane identifier are separate records. In the verified pattern-member form, independent audit rows retain a pattern header and five predecessor-linked members; each member has its own physical row header and positional section record. A shared datum identifier is therefore insufficient to group those records as alternative states. The current fixed-prefix probe and known-feature filter do not establish a general row grammar. A canonical row identifier, a model operation, and a stored section/schema identifier remain separate identities.

**Need.** A complete owned boundary/root grammar and positional section parser must preserve each row range independently of current-feature filtering. Repeated definitions within one genuine owner range, or unresolved `DEPDB_DATA` repeats, require their own selector evidence; first/last order and sketch-plane arithmetic supply none.

### SP-22. Ambiguous sketch-datum parent

**Question.** How does a section select its sketch datum when the generated-datum parent-table remainder is not unique?

**Known.** `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "When the sketch plane resolves to a placed plane carrier" define the unique remainder rule and the nested `plane_id` join.

**Need.** We must know the selection rule to place the sketch.

### SP-23. Owned fallback when the orientation reference is parallel

**Question.** Which complete owner-qualified frame or generated carrier places a section when its typed orientation reference is parallel to the sketch normal?

**Known.** A parallel plane normal has no nonzero projection into the sketch plane and supplies no in-plane direction. The existing resolver can use a fully established generated carrier or a unique complete definition local frame, with owner, finite/nonzero axes, and perpendicular-axis guards. The two bounded named reference controls are perpendicular, so they do not establish an authored parallel-case selector. Their direct placement origin differs from their saved local-frame origin; those frames are not interchangeable. `SP-08` retains the distinct placement/transform/saved-result owner problem, and `SP-17` retains qualified typed reference-row ownership.

**Need.** An authored parallel-reference witness with complete frame ownership and independent placed geometry must establish the eligible fallback. Do not select another table member, reinterpret a PSB table class as a model datum, or choose the first/last nearby local frame.

### SP-24. `ActDatums` outline tokens

**Question.** What scalar value does each `5c` and `45` token encode in an `ActDatums` outline?

**Known.** `creo_prt.md` §6 "`ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records. Each section" defines the two-corner outline and its held-coordinate plane rule. Named and positional outlines use the same bounded model-coordinate lane. A `5c` or `45` token consumes one seven-byte coordinate slot without supplying a numeric value.

**Need.** We must know the values to construct nonzero datum offsets and extents.

### SP-25. Partial revolution direction and side values

**Question.** Which stored field selects the standard direction of a partial
`angle_choice` revolution, and which dimensions hold the two side values of a
two-sided mode or the references of a reference mode?

**Known.** The angle value of a rotational section sweep is the feature's
`dim_type` `21` model dimension. A `FROM_NONE | TO_ANGLE` mode with one
360-degree angular dimension is a full turn. `eb 20 04 00 00` selects a
symmetric angle mode. A recipe, repeated mode, or matching full-turn carrier
geometry does not supply the trim magnitude.

**Need.** Identify the direction state and the side-value and reference
owners before constructing a partial angular interval. Missing values do not
imply zero, a partial turn, or a full turn. Preserve the complete raw choice
vectors.

### SP-38. Current recipe ownership across creation events

**Question.** Which complete current feature-owned production selects the sweep form when creation-event history disagrees with the current modeling structure?

**Known.** A `created_features` row binds its `feat_id`, `ft_type`, and `icon_name`. A group event can contain distinct child recipes with distinct owners. Class 916 removes material and class 917 adds material; neither class selects extrusion versus revolution. An incompatible creation recipe does not define current form, Boolean effect, or regeneration ancestry. Current parent tables and prototype references remain independent of creation history. Display names, previous-feature identifiers, and pattern membership do not select the current recipe.

**Need.** Establish complete named and positional creation-array and event ownership, including repeated feature identifiers in separate events. Bind the current form independently of first or last audit occurrence, display text, and other child rows. Keep historical fields distinct from current recipe and dependency authority.

### SP-39. Complete outer-entry ownership for simple-drilled dimensions

**Question.** Which complete PSB compound-entry grammar binds each class-911 dimension definition to its current outer feature key, including entries whose declared array count differs from the ordinary keyed-entry count?

**Known.** The current outer key and optional `feat_id / ref_model_info` replay-source reference are separate identities. A copied replay can retain an older source while belonging to a different current key. A complete three-row table still requires the recipe-selected external-ID/type roles. Equal envelopes, names, ordinals, or dimension tuples elsewhere do not establish that owner. A blind depth proved equal to the owned cylinder's axial span ends at its shoulder; the conical point extends beyond it. This independently earns `depth_to_tip = false` only with the cylinder-span and angle evidence.

**Need.** The parser must retain complete current-entry bounds and bind exactly one eligible table before model projection. Unknown compound/replay boundaries must abstain. A missing or invalid owned table must not borrow another feature's equal-radius/depth tuple. Schema-class identity and internal replay-source IDs do not provide a current outer-entry binding. Raw model-unit values and Native unit labels need an explicit conversion contract. Different point-angle controls and envelope-free negatives must preserve both the ownership and Shoulder/Tip boundary.

### SP-41. Native counterbore and countersink additions

**Question.** Which complete feature-owned parameter grammar binds the independent counterbore and countersink additions when entry-profile or transition topology is unavailable?

**Known.** Class-911 `feat_type_ptr.form` distinguishes straight/extruded construction from sketched/revolved construction. It does not declare the detailed entry treatment. `draft_choice` and `misc_choice` are separate fields; a bit position without the complete enclosing production does not identify an entry treatment. Generated conical bottom geometry alone does not identify a countersink. A positively owned entry profile can distinguish a planar shoulder from a conical entry transition.

**Need.** Bind the complete current feature parameter production and its source profile before interpreting packed choices. Preserve counterbore and countersink evidence independently. Bound each outer keyed definition before the next owner; a named sketch prototype boundary does not define the owner interval. Unresolved operands or ambiguous entry transitions must remain unresolved.

### SP-43. Structural `e3` separator

**Question.** Which enclosing table and row grammar distinguishes a structural
`e3` from the tail of a compact identifier or scalar payload?

**Known.** A compact integer head from `80` through `bf` owns the following
byte, including `e3`. A locally complete `ent_tab` replay entry has five
nullable compact fields followed by literal `00`; only `f6` spells an absent
field. A positional saved scalar body can begin immediately after `e2`, or
after `f7`, one complete compact reference, and `e2`. A nearby `e2` after an
unrelated field is not a saved header. Raw saved bytes remain available when
local scalar admission is refused.

**Need.** Prove the enclosing bucket, table, and entry boundaries before
admitting row starts. Establish any counted-reference header from a complete
owned production. Keep saved arc parameter roles separate from independently
resolved center, radius, and endpoints.

**Conflict.** The current saved-row candidate scanner still searches for `e3`
and a nearby `e2` before testing the exact local header. Its candidate set also
bounds retained row bodies. The trim scanner likewise considers separator-like
bytes without a complete enclosing state machine. Local header validation and
identifier joins do not establish those boundaries or positive ownership.

### SP-44. Cap tables with mixed profile entries

**Question.** Which complete owner-qualified grammar selects the current profile
and linear extent when a class-29 generated table contains source-bearing
class-200 entries and a source-less class-200 cylinder entry outside the defined
four-entry circular forms?

**Known.** `creo_prt.md` §6 defines the general cap-pair rule with unique
source-less class-204 and class-203 entries and populated source identifiers on
every remaining class-200 entry. The separate circular forms define exactly four
entries. Placed cap planes and a materialized cylinder do not select the current
source profile in a table outside those forms.

**Need.** Define current profile selection and the source-profile and cap-use
roles of all entries before deriving an operation span from the cap pair.
Reconcile the selected profile, section placement, and generated carriers without
discarding other owned entries or selecting a regeneration state by table order.

## 4. Topology and appearance

### TP-01. Current feature-to-body binding

**Question.** Which stored regime discriminator and complete current feature-local body references bind an admitted recipe to its input body?

**Known.** The recipe feature and preceding-feature fields in creation-event rows are feature identities, not body selectors. Modern named `body_tree`, `body_array`, and `used_bodies` records contain explicit `body_id` fields; `body_array.first_geom_ptr` supplies a separate material-geometry root. A body count does not identify a feature target. Current generated carriers identify feature outputs rather than input material.

**Need.** Bind complete body identities to the same model owner and current feature scope, then resolve their topology roster independently. Validate the part's material regime before applying a legacy single-material replay rule. Missing, duplicate, unresolved, or conflicting target identities must remain unresolved; neither chronology nor the first or sole decoded body selects a modern target. Creation history alone does not establish the current recipe or its body operands.

### TP-02. Cross-section parameter framing and contour ownership

**Question.** Which complete parameter grammar distinguishes structural row
closes from delimiter-like bytes inside opaque and scalar parameter data?

**Known.** `Sld_Xsections.xsec_geom.crv_array` has six-reference
`[0, P, 0, N, S, 0]` tails. Its plane, successor, and reference-surface
identities belong to the cross-section and generating-feature namespaces.
They do not identify final-solid vertices, faces, or loops.

**Need.** Bound every parameter form and each row close within its owning
array. Require unique same-feature surface and curve targets and a complete
successor permutation before constructing section contours. Preserve all
independently closed cycles; a feature does not imply one contour.

**Note.** A tail split alone does not establish the complete row grammar or
prove that a section contour is current material topology.

### TP-03. Remaining contour roles and incomplete joins

**Question.** Which roles do contour values `00`, `02`, and `03` encode, and
which source-owned reference resolves a contour whose head does not join one
unique closed loop in its topology namespace?

**Known.** Positional `trv = 01` identifies an outer boundary and `trv = f6`
identifies an inner boundary. The preceding complete two-byte curve-header
reference selects the unique same-surface closed successor loop containing
that header. A complete classification requires a contour-to-loop bijection.
The role is independent of chain order and surface-row orientation. The
remaining grammar values do not establish a role. Incomplete or ambiguous
joins do not establish loop ownership. Multiple outer loops require the
face-instance partition described by TP-06.

**Need.** We must know the remaining role values and owner-qualified joins to
classify loops that the complete stored role mapping does not cover.

### TP-04. Additional vertex-coordinate selectors

**Question.** Which source-owned coordinate witness selects one point for a
B-rep vertex orbit when directed pcurve endpoints, intrinsic edge endpoints,
and incident analytic carrier constraints leave several candidates or none?

**Known.** `crv_array` face and successor references define half-edge vertex
orbits. The orbit labels are derived from that graph; the six-reference suffix
does not store vertex identifiers or XYZ tuples. Complete agreeing directed
pcurve paths bind their mapped endpoints to the two oriented start orbits.
Intrinsic edge endpoints and incident carrier intersections supply further
constraints. A point requires one consistent candidate. Sketch
`vert_tab.vertex_id` belongs to a separate trim namespace and does not bind a
B-rep orbit. The defined suffix grammar does not establish a direct stored
vertex-coordinate fallback in another record family.

**Need.** We must know any additional owner-qualified coordinate selector to
place an orbit whose existing evidence does not select one point. For compact
plane frames, we must distinguish the stored carrier point from the native
parameter origin when mapped pcurve endpoints do not agree with adjacent
carriers.

**Conflict.** `placed_plane_surfaces` uses the complete local-system origin
as the plane chart origin. Agreement of its normal coordinate establishes the
carrier plane but does not establish the tangential parameter origin. The
compact frame productions do not define a separate parameter-origin selector.

### TP-05. General rowless face-use binding

**Question.** Which fields bind a rowless face-use reference to its face, loop, edge, and orientation? Which stored curve incidences belong to the complete current face boundary?

**Known.** `creo_prt.md` §6 "Classes 913" through `creo_prt.md` §6 "For a class-913 cylindrical slot fillet, the first two `geoms_affected`" define independently constrained cylindrical slot fillets. The four-entry round-table shape does not establish rowless carrier inheritance or face sense. Raw side rows and closed successor cycles are distinct retained facts. Neither a closed cycle nor coverage of every raw side proves current trim ownership.

**Need.** Establish that general binding with complete owner-qualified
incidence, a unique carrier, directed endpoint geometry, and independent
face-surface sense. Distinguish required current boundary uses from other
stored incidence before applying a completeness requirement. Resolve repeated
identities and unmatched chains with an independently mapped stored trim.

**Conflict.** Other stored rows whose two face references are both live can
name a face with a closed ring while remaining outside that ring. This alone
establishes neither a complete face nor the invalidity of that face. Legacy
rows beside a null face reference or a face without a visible surface row are
not required uses, as specified in `creo_prt.md` §4 "In the legacy ASCII layout, `VisibGeom` retains
curve rows of deleted faces".

**Note.** The runtime admission guard requires every side whose other face is
live and remains conservative. Its coverage result must not be used as proof
that a withheld face is invalid. The general current-boundary ownership rule
remains unresolved.

### TP-06. Shared-surface face partition

**Question.** Which source-owned reference assigns each inner loop of a surface
carrier to one of several outer-loop face instances?

**Known.** `creo_prt.md` §3 "A contour binds to the unique closed successor
loop" defines the outer and inner contour roles and one face instance per
outer contour of a complete classification. A surface carrier identity does not
identify one bounded face instance. A parameter-envelope containment test only
filters candidate owners: a circle inside a concave polygon's bounding box can
remain outside the polygon. Independent complete boundary containment can
corroborate an assignment when both boundaries share a proved chart.

**Need.** Establish a complete owner reference or common-chart geometric
containment for every inner-to-outer assignment. Preserve distinct outer
instances and all unmatched contours. Partial joins, bounding boxes, and
surface identity alone must not admit an assignment.

### TP-07. `lo_restore.direction`

**Question.** Which restoration state and face side determine the traversal
action of the primary `lo_restore.direction` edge reference?

**Known.** The primary integer binds an edge identity in its attached generated
edge object or same-owner affected-edge roster. It is distinct from the loop
identifier and is not a signed direction, angle, or extent. An edge can have
two current face-side successor cycles; the integer alone does not select one.
A historical edge can be absent from the current visible topology.

**Need.** Prove the complete restoration-state-to-current-boundary join and
face-side action with an independent restoration control. Rotating a cycle to
an included edge demonstrates graph algebra, not an authored cyclic-start
rule. Preserve the raw reference without choosing a side or order from roster
overlap.

### TP-08. `lo_restore.direction2`

**Question.** Which restoration state and side determine the action of the
secondary `lo_restore.direction2` edge reference, including explicit zero?

**Known.** The secondary lane binds an edge from its separate typed secondary
or old-secondary object roster. It need not equal the first roster member or
the primary edge. An old reference need not occur in current visible topology.
Explicit zero is distinct from a missing field; its restoration action is
unproved.

**Need.** Establish the same-owner restoration/current-boundary and face-side
join before applying traversal order. Neither a roster match nor an anchored
cycle rotation proves restoration semantics or persistence lifecycle.

### TP-09. Loop-history operand ownership

**Question.** Which source-owned namespace and complete history relation
resolve the `lo_hist` operands of subtypes without a defined operand
production, and the second operand of subtype 759?

**Known.** `creo_prt.md` §6 "Records of subtypes 750 through 757" defines the
operand forms of subtypes 750 through 757 and 759 and the subtype-759 join of
the first operand to an earlier type-700 loop. The subtype-767 first operand is
a class-212 loop-history head that resolves the loop's boundary surface. The joins do not establish a current boundary network
or restoration order.

**Need.** Establish the owning production and target namespace of the
subtype-759 second operand, of other subtypes, of the class-212 tail
identifier, and of the class-2006 body after its class-212 identifier before
applying history. Preserve opaque operands and nullable states.

### TP-10. Required final loop-history type

**Question.** Which subtype-specific semantics and current-state evidence
make the required final `lo_type` actionable?

**Known.** The final type follows all three logical operands in every complete
row, including the final row before a named header or before an
`f2 f7 <reference>` table separator. It is not an optional trailing field.
Row-boundary tokens follow all six logical slots.

**Need.** Bind type and subtype to their complete history production before
traversal or restoration. Retaining the raw enum values does not prove a
current-boundary selector, operand namespace, or face-side action.

### TP-12. Complete body-owned material rosters

**Question.** Which complete source-owned material roster assigns all shell
components to the same body identity and model scope?

**Known.** Connected face components are not material-body identities. A
solid-root `next_geom_ptr` chain can cross disconnected shell components.
Legacy `frst_geom_ptr` is separate from the quilt discriminator. Modern
`body_array` rows distinguish `body_id` from their first material-geometry
reference. An unmaterialized next target prevents a complete roster; equal
opaque body-ID images alone do not establish a general multibody grammar.

**Need.** Parse and bind complete body/root records, every unique chain target,
and the same-model material regime before assigning shells. Preserve missing
or conflicting membership and distinguish solids from nonsolid quilts.

**Conflict.** Current neutral grouping still uses declared body count,
`first_quilt_ptr`, and adjacency-component cardinality as ownership gates and
can merge nonempty components from a single-count discriminator. Those
heuristics do not supply the missing complete owned roster. A retained Native
graph or partial neutral output does not certify correct closed bodies.

### TP-13. Element-color side and instance ownership

**Question.** Which serialized slot convention binds the two `color_inds`
values to an oriented model-item side, and which owner-qualified reference
resolves item identifiers outside the established topology-reference match?

**Known.** Complete two-empty-property `element_colors` rows store one item
identifier, two ordered color indices, and two empty `surfprop_ptr` bodies.
The row contains no contour, loop, boundary-role, or bounded-face selector.
Exact item identifiers can occur in topology face-reference pairs without a
materialized surface row. Such a reference can support several bounded faces.
PTC's side-appearance API accepts a model item and side zero along the normal
or side one on the other side; that API alone does not establish serialized
array order. A selected-set graph with the same counts and degrees as an
exported appearance set does not prove unique individual face identity under
symmetry. Nonempty property pointers are outside the empty-body grammar.

**Need.** We must know the directional slot convention, additional item
namespaces, and complete carrier-to-instance ownership before transferring
these appearances onto bounded face sides.

### TP-14. Unresolved `NeuPrtSld` appearance references

**Question.** Which additional owner-qualified `NeuPrtSld` records, if any,
map unresolved appearance item identifiers to their native model-item class,
carrier, and oriented bounded-face uses?

**Known.** Complete two-empty-property `element_colors` rows contain one item
identifier and two ordered color/property slots without a contour or bounded
face selector. Exact topology-reference matches do not require a materialized
surface row. Other fully decoded tables can retain identifiers absent from
all tested visible and nonvisible surface, curve, topology-reference, and
feature-row sets. Those nonmatches establish unresolved references; they do
not prove a different model-item class or exclude an unmaterialized or stale
reference. The public appearance API supports multiple model-item classes,
but does not identify the class of an unmatched serialized value. Slot-to-side
ordering and nonempty property pointees remain separate unresolved contracts.

**Need.** We must know those identity and side contracts before promoting
unresolved native appearances onto model faces. A nearest ID, table ordinal,
or unique exported graph shape does not supply that join.

### TP-15. Display side and occurrence ownership

**Question.** Which serialized convention assigns ASCII display color and
placement slots to oriented surface sides, and which reference supplies an
assembly occurrence owner or a nonempty placement-property pointee?

**Known.** A complete uncompressed ASCII `surf_ind_tab` record stores one
`surf_id`, two ordered `surf_color_inds`, and two `placement_ptr` entries.
Declaration numbers and object depth are schema structure, not the carrier
identity. Exact stored IDs can match current topology references without a
materialized surface row. The complete two-empty-placement form has no
additional contour, loop, or bounded-face selector. Equal color pairs do not
identify slot-to-side direction. A topology-reference match does not prove a
complete bounded-face partition, and a coincident integer in another model
does not establish an assembly occurrence. Nonempty placements and compressed
display bodies require their own grammar.

**Need.** We must know those side, occurrence, and property contracts before
transferring display metadata onto final bounded face uses.

### TP-16. Unresolved RGB constants and appearance projection

**Question.** Which RGB-lane evidence fixes `0d` and `18`, and which owned
row grammar binds the channels to an appearance and its model-item uses?

**Known.** `creo_prt.md` §1.2 defines RGB `0f = 1` and bounded predecessor
replay within three channels. The non-RGB transmission field stores
`18 = 0`; that field does not define RGB `18`. Complete `NeuPrtSld` bytes
are retained without a typed `color_tab` parser or appearance-binding
projection. Explicit positive RGB values remain separate from unresolved
compact constants.

**Need.** Establish each unresolved constant with independently paired RGB
definitions or controlled RGB changes. Define complete named and positional
row ownership and model-item binding before emitting colors and their uses.

**Conflict.** STEP green `.4` covariance does not identify native RGB `0d`.
A component value in another lane does not establish the RGB value. A STEP
color fit cannot replace the native value without an independent row identity.

### TP-17. Other appearance component lanes

**Question.** What scalar value does each undefined token in a non-RGB appearance component lane encode?

**Known.** `creo_prt.md` §1.2 defines field-qualified `diffuse:0f=1`, `transmission:18=0`, and the highlight-vector repeat control. Those meanings do not transfer to other component fields. Complete `NeuPrtSld` bytes preserve the remaining scalar images without establishing their values, appearance ownership, or surface side.

**Need.** Define complete owned appearance rows, retain unresolved component bodies, and establish each value independently with an exact field/component oracle. API ranges and values in another lane do not identify a token. Keep appearance binding and side selection separate from scalar decoding.

### TP-18. Complete modification-audit event grammar

**Question.** Which complete named and positional grammar recovers each
`mtrack_record_ptr` action payload and its field ownership?

**Known.** `MdlStatus.mod_track_data.rec_arr` is a modification audit trail.
A positional `item_name` starts at the byte after the compact timestamp's
`tm_year`. Repeated same-ID event names do
not select a current native candidate; they project a common operation only when
family, recipe, root schema class, and parent identifier agree.

**Need.** Establish complete bounds and field types for each action-specific
payload and its references. Agreement among same-ID names projects a common
operation, not a current candidate; a current candidate requires separate
current-model ownership evidence.

**Note.** The named prototype and compact six-scalar timestamp form do not
define the complete heterogeneous event union.

### TP-19. Complete loop-array row framing and roster semantics

**Question.** Which enclosing object and auxiliary grammar bounds each
`lo_array` row, and which owned state selects its face-side traversal action?

**Known.** The positional prefix has seven compact integers, including
attributes. The local lane admits three-byte integer images as well as one-
and two-byte forms. Counted object and auxiliary bodies can contain nested
compound closes; the first close is not the enclosing row boundary. A typed
roster and a visible successor orbit remain separate evidence.

**Need.** Consume the complete counted object/auxiliary production and preserve
all complete rows and opaque fields before using a roster as an ownership
oracle. Establish state-to-current-boundary and traversal joins independently;
roster overlap, pointer ancestry, or a possible cyclic rotation is insufficient.

**Conflict.** The prefix reader consumes one- and two-byte attributes with
the same compact-integer grammar as its other fields, but still omits the
three-byte tier and ends a row at the first compound close. In
`headset-hook-holder.prt`, the prefix at `0xc183` has attributes `513`,
direction `84`, and next-loop pointer `196`; correcting those fields does
not resolve its nested auxiliary body or recover the full declared roster.
Bodies can remain truncated and later rows omitted. Typed row output must
not be treated as a complete framing or current-ownership proof. General row
repair remains blocked on the complete enclosing grammar, rather than a
global retokenization or guessed current-state selector.

## 5. Packed persistence data

### PP-01. Hybrid visibility geometry and scalar values

**Question.** What grammar defines the leading opaque region and the unresolved packed scalar values of a hybrid visibility section?

**Known.** The capacitor specimen has a clear prefix and an XOR-96 tail with 1,718 complete eight-slot pcurve rows. The 691 recovered face identities all occur in the independently expanded persistence roster. Complete successor joins preserve face ownership; endpoint continuity distinguishes packed `e4 = 0` from the ordinary lane's one. The `0a = 0`, `1`, and `-1` rivals produce identical continuity outcomes and do not select its value. The leading region and the undefined three-, seven-, and eight-byte scalar images do not define geometry. Opposite endpoint slots do not all have equal form populations, so that claim is not a field-role proof.

**Need.** Establish the Source-owned tail start and count, the leading-region record or compression grammar, and independent numeric or chart oracles for each undefined scalar form. Preserve the bytes and abstain from neutral geometry until those inputs are complete. A complete framed tail does not prove a corpus-wide packed grammar or an analytic face bijection.

### PP-02. `SolidPrimdata` positional strip replay

**Question.** Which complete positional class-84 body grammars and primitive scalar forms define each strip set?

**Known.** `creo_prt.md` §8.4 defines record-local cumulative intervals and winding reset. A strip does not continue across records. Compact and array-framed positional replays retain cumulative boundaries, fixed attribute fields, and XYZ position arrays. The position-only profile and the interleaved normal-position profile have different attribute tuples and size accounting. Neither `obj_type=84` nor `num_offsets` supplies the strip count. A bounded nine-slot position body can retain an unresolved three-byte scalar without defining its numeric value.

**Need.** Complete physical framing must establish each positional cumulative-array extent and position count, including multiple-strip forms, before numeric geometry admission. Unresolved primitive scalar values and display placement require independent definitions.

### PP-03. Complete positional primitive-owner framing

**Question.** Which complete nested field trees delimit positional `pgl_segref` rows and their primitive containers, and which independent ownership rule associates a resolved display object with analytical geometry?

**Known.** `creo_prt.md` §8.4 defines the named row's display reference and its table/class/ordinal domain. The primitive decoder has no typed persistent-reference resolver or owner attachment. The bounded raw reference permutation and identifier correspondences do not prove complete nested row boundaries or a general face join.

**Need.** Establish complete positional row and nested-array boundaries before primitives inherit display references. Resolve missing or ambiguous tables, handles and row ownership conservatively. Prove a unique same-model geometry association separately; reference-like bytes inside opaque primitive payloads and equal numeric identifiers do not supply it.

### PP-04. Expanded `SolidPersistTable` row bodies

**Question.** Which complete named-prototype field trees bound each persistent object row and its nested arrays, including polymorphic shader bodies?

**Known.** `creo_prt.md` §8.8 defines the bounded table/class/ordinal reference domain, named row zero, handle-name variants, and separate segment identifiers. Handle class differs from concrete row type. The container expansion retains the payload; the decoder has no typed PGL object graph or persistent-reference resolver. Recognized common headers alone do not prove full row boundaries.

**Need.** Establish complete field-tree grammars for the object classes and host-prefix variants, including nested reference ownership. Preserve ordered rows and duplicate stored IDs. Missing or ambiguous table/class selection, absent GPU membership, and unresolved model/occurrence/display placement must remain explicit. Original nonzero or multiple-table cases and the historical population totals remain unverified; a private bounded selector is not a production parser.

### PP-05. Physical `DEPDB_DATA` semantic ownership

**Question.** Which complete physical persistence bodies, beyond the observed empty roots, establish feature, section, unit or model-metadata ownership?

**Known.** Three independently indexed originals contain complete empty physical `DEPDB_DATA` roots. Their surface, curve, section and recipe labels belong to other indexed sections. This establishes those three boundaries, not a population-wide absence result or a replacement owner for every implemented value grammar. The visible-geometry and namespace-census readers do not borrow physical `DEPDB_DATA` when `VisibGeom` is absent or sparse; newly refused geometry-looking sections retain their complete physical bytes with an explicit untyped-owner warning. Failed expansion retains its existing precedence.

**Need.** Establish source-owned complete grammars and audit the remaining physical feature/operation, section, model-name and unit readers before further semantic admission or quarantine. Preserve unknown physical variants and original offsets. Authored composition fixtures and an encoding-family name are not native ownership evidence; fixture relocation requires a proved producer grammar.

### PP-06. Compressed `DispDataTable` substrate

**Question.** Which encoding and initial state expand the `DispDataTable`
bodies whose `1f 9d 10` prefix is followed by invalid ordinary LZW codes?

**Known.** A non-block `1f 9d 10` stream starts with a nine-bit literal code
and dictionary slots `0..255`. A first code of 480 violates that state; a later
code beyond the next free dictionary slot also violates it. These failures can
occur in complete TOC-bounded bodies whose two stored checksum lanes agree.
Successful ordinary streams in the same container do not select a different
dictionary for such a body. The TOC tag `c` is a candidate discriminator;
it does not define a compression algorithm or initial dictionary. Matching
payload checksums do not validate the declared expanded length. The stored
bytes remain opaque.

**Need.** Establish whether the TOC tag selects an encoding, define its
complete decoding state, then verify
the declared output length and a fully consumed record grammar with an
independent decoder. A partial output, valid magic prefix, or printable run
does not establish the substrate.

**Note.** One part re-saved in Creo 7 through 13 retains this gap in every
release. The Creo 7 through 11 streams fail with invalid or incomplete LZW
framing; the Creo 12 and 13 streams reach an undefined code. Other compressed
sections expand with the defined Unix-compress grammar. Need a complete
dictionary and width-state proof with synthetic streams before applying that
grammar to this variant or changing the common decoder.

### PP-07. Compressed `DispDataTable` geometry binding

**Question.** Which fields bind decompressed `DispDataTable` rows to model geometry?

**Known.** The compressed stream contains display data in a namespace separate from material face instances.

**Need.** We must know the binding to apply display data to the correct entities.

### PP-09. Neutral family-table semantics

**Question.** Which family-table item types represent neutral parameters, dimensions, feature states, active instances, and configuration values?

**Known.** `creo_prt.md` §8.3 defines the legacy object graph, ordered `items` and `instances` arrays, ordinal value join, complete row fields, and typed value forms 50, 51, and 52. The decoder retains a complete joined table in the native `configuration_driver_tables` arena.

**Need.** We must map the complete table columns and instance state to neutral configuration, parameter, feature-state, and active-configuration records without assigning a meaning to an item type or visibility flag that the source does not establish.

### PP-13. Legacy persistence bodies

**Question.** What type and value grammar does each legacy ASCII `@<name>`
field declaration select, and how do its object references and arrays compose
the geometry, topology, and design-history graphs? What record grammar applies
to named legacy sections that do not begin with an attribute declaration?

**Known.** `creo_prt.md` §1 defines the complete legacy ASCII layout
discriminator, its outer `P_OBJECT` boundary, its decimal schema token, its
product-release banner forms, and its monolithic and named-section forms. The
named-section directory defines banner-relative offsets and stored extents.
Attribute declarations and value rows use section-local identifiers; an
immediately following `$` row continues a value payload.
The unique type-10 `principal_sys_units` scalar selects either millimeter or
inch coordinate lengths; inch lengths scale by `25.4` to canonical
millimeters.
Type 1 selects signed decimal 32-bit integer scalars and dimensioned run-length
arrays. Type 2 selects finite IEEE-754 binary64 scalars and dimensioned
run-length arrays with the compact hexadecimal grammar in `creo_prt.md` §1.
Type 0 selects null, arrow, inline, and dimensioned-array object nodes; row
depth supplies their scoped ownership tree.
Type 10 selects null and byte-string scalars and direct-row arrays. The first
array extent gives the number of string rows; later extents do not multiply
that count. Valid UTF-8 strings transfer as text, and other encodings retain
their exact bytes.
Type 3 selects nullable byte-string scalars. Type 4 selects byte-string scalars
without a null token. Neither type uses continuation rows.
Type 6 uses the type-2 compact-real scalar and array grammar. Types 5, 7, 9,
and 11 use unsigned decimal 32-bit scalars and dimension-complete run-length
arrays; their one-element arrays can store the element in a direct child row.
In `Sld_VisGeom.active_geom`, a complete direct-child `crv_array` object has
one `crv_id`, `type`, `feat_id`, signed two-element `crv_pnt_dir`, two
`crv_hdr_geom_ptr` face references, and two `next_crv_hdr_ptr` successor
references. A complete `[N,4]` `crv_pnt_arr` type-2 array uses its first and
last rows as the two endpoint pairs in the adjacent face charts. These rows
join the native half-edge and face-loop graph when all carrier and vertex
admission invariants hold.

**Need.** We must know the semantic axis order of multidimensional type-2
arrays outside the settled geometry records, the character-set selection for
non-UTF-8 type-10 strings, type-10 continuation semantics, remaining geometry
and design-history graph joins, and non-attribute section grammar to
transfer the rest of legacy persistence.
