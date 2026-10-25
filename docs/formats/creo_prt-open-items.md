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

### SE-07. Other `double_xar` slots

**Question.** What grammar does each `double_xar` slot use when it is not a defined literal, scalar, placeholder, or terminal slot?

**Known.** `creo_prt.md` §2.3 "An expanded" defines the counted array, literal-one slot, literal-zero slot, two recursive placeholder images, scalar slots, and terminal null slot.

**Need.** We must know the other grammars to keep the counted slot boundary aligned.

### SE-08. Variable-length `e5` slots

**Question.** What bounds and semantics apply to a variable-length `e5` slot in `double_xar`?

**Known.** `creo_prt.md` §2.3 "An expanded" defines `e5 07 23 11 2e` as one exact recursive placeholder image.

**Need.** We must know the bounds to distinguish one slot from following slots.

### SE-09. Positional plane-envelope prefixes

**Question.** What scalar value does each undefined prefix in a positional plane envelope encode?

**Known.** `creo_prt.md` §3.4 "Plane row bodies contain envelope/domain data" through `creo_prt.md` §3.4 "In the frame-bound held-coordinate outline form" define the standard, compact, held-coordinate, and planar-envelope plane forms.

**Need.** We must know the remaining prefixes to construct the plane envelope and domain.

### SE-11. Dimension-driven `00 XX YY` values

**Question.** What numeric value does a `var_arr` scalar of the form `00 XX YY` encode?

**Known.** `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" defines its three-byte boundary. `creo_prt.md` §5 "Within each" defines the separate nine-byte dimension-driven sentinel.

**Need.** We must know the value to solve the section variable.

### SE-12. Dimension-driven `01 XX YY ZZ` values

**Question.** What numeric value does a `var_arr` scalar of the form `01 XX YY ZZ` encode?

**Known.** `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" defines its four-byte boundary.

**Need.** We must know the value to solve the section variable.

### SE-13. Dimension-driven `34 XX YY` values

**Question.** What numeric value does a `var_arr` scalar of the form `34 XX YY` encode?

**Known.** `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" defines its three-byte boundary.

**Need.** We must know the value to solve the section variable.

### SE-14. Other principal unit selectors

**Question.** Which coordinate unit system does each `_principal_sys_units_id`
value outside `51`, `54`, and `55` select?

**Known.** `creo_prt.md` §1.3 "`_principal_sys_units_id` identifies the active
coordinate unit system." gives `51` for millimeter-Newton-Second, `54` for
inch-pound-mass-second, and `55` for millimeter-Kilogram-Second. Binary
selector `54` stores lengths in inches and the neutral model multiplies every
length-bearing value by `25.4` to produce canonical millimeters. `creo_prt.md`
§1.3 "In legacy ASCII persistence, the unique type-10
`principal_sys_units` scalar" gives the same inch system and conversion.

**Need.** We must know each selector to give model coordinates their canonical
length unit.

## 2. Curves and surfaces

### GS-01. Cone half-angle overrides

**Question.** What grammar and value rule apply to a per-instance cone half-angle that is not the terminal positive-DICT form?

**Known.** `creo_prt.md` §3.2 "A positional cone suffix consists of exactly one complete nine-slot support" defines the terminal positive-DICT half-angle form and the support-frame construction.

**Need.** We must know the override to construct the cone carrier.

### GS-02. Torus and sphere radius overrides

**Question.** What grammar and value rule apply to a `geom_type = 26` radius body that is not a tagged radius trailer or terminal prototype-minor-radius replay?

**Known.** `creo_prt.md` §3.3 "A `srf_prim_ptr(torus)` prototype stores" through `creo_prt.md` §3.3 "In named `radius`, `radius1`, and `radius2` fields" define the recognized torus and sphere radius forms and their carrier invariants.

**Need.** We must know the override to construct the torus or sphere carrier.

### GS-06. Prototype-adjacent `tab_cyl` parameters

**Question.** What does each parameter in a prototype-adjacent `tab_cyl` instance row control?

**Known.** `creo_prt.md` §3.2 "A `tab_cyl`" defines the bounded `params` field and its relationship to the prototype.

**Need.** We must know the parameter roles to define the surface chart and domain.

### GS-07. Ambiguous `tab_cyl` placement

**Question.** How does a replay-bound `tab_cyl` select its placement when its axis span matches neither directrix-coordinate range uniquely?

**Known.** `creo_prt.md` §3.2 "A repeated `tab_cyl` cubic-curve replay has this structure:" defines the repeated cubic replay and the unique axis-span placement cases.

**Need.** We must know the selection rule to place the surface in model space.

### GS-08. Unresolved `fc 02` bodies

**Question.** Which scalar images or additional body productions resolve an `fc 02` body that does not satisfy the complete counted or complete short production?

**Known.** The complete counted production has eight endpoint slots in two face charts. The complete short production supplies one face path. The bounded microphone and heat-sink verification resolves 72 counted bodies and two short bodies; 68 other bodies remain opaque. An opaque body can contain an undefined scalar in a known production. Opacity alone does not prove another grammar.

**Need.** Resolve each remaining body through complete scalar ownership and chart roles before it supplies curve or endpoint geometry.

### GS-09. Other `fc 05` variants

**Question.** What grammar does an `fc 05` body use when it does not satisfy the defined cap-circle production?

**Known.** `creo_prt.md` §4.2 "`fc 05` records store cap-circle control points in the order `A`, `B`, `t`, `C`, where `A` and" through `creo_prt.md` §4.2 "One `fc 05`" define complete point groups, scalar lanes, termination, cylinder binding, placement, and circle construction for cap-circle bodies.

**Need.** We must know the other grammar to construct or reject the curve correctly.

### GS-10. `fc 08` grammar

**Question.** What is the complete body grammar for `fc 08`?

**Known.** `creo_prt.md` §4.2 "An `fc` prefix is resolved by exact body grammar." identifies `fc 08` as a world-coordinate control-polyline family. Recognized coordinate tokens and opaque spans partition its retained body.

**Need.** We must know the grammar to construct the control polyline.

### GS-11. `fc 13` scalar signs and model-space charts

**Question.** How do the remaining undefined scalar images, signed coordinate branches, and native chart transforms define complete placed paths?

**Known.** The complete point-first and parameter-first productions associate one parameter with three coordinate roles. Their shortened terminal group completes the preceding parameter's sample. These roles do not prove a spatial control polyline, NURBS control poles, or a native-to-model transform. A radius identity alone does not select a signed branch. The panel 687 path retains unresolved branch signs; the CTC02 and microphone role proofs do not establish their complete native model-space placements. No named nineteen-by-four array declaration establishes a universal interpretation of `13` as a sample count.

**Need.** Resolve the undefined images and derive each complete signed chart from native ownership before constructing placed geometry.

### GS-13. Other `fc` subtypes

**Question.** What body grammar does each `fc 04`, `fc 07`, `fc 09`, and `fc 0a` subtype use?

**Known.** `creo_prt.md` §4.2 "An `fc` prefix is resolved by exact body grammar." defines the common `fc <subtype>` opener and the recognized subtype families.

**Need.** We must know each grammar to construct its curve family.

### GS-14. Parabola carrier equation

**Question.** How do a parabola's `type`, `t0`, `t1`, `c1`, `c2`, and `local_sys` fields define its carrier?

**Known.** `creo_prt.md` §8.5 "The named entity in `ent_list(conic)` declares compact `id`, `type`, and" defines the named and positional conic fields, frame grammar, and field invariants. `creo_prt.md` §8.5 "A type-30 conic record defines a complete ellipse carrier without interpreting" defines type 30 as an ellipse.

**Need.** We must know the equation to construct a parabola in model space.

### GS-15. Hyperbola carrier equation

**Question.** How do a hyperbola's `type`, `t0`, `t1`, `c1`, `c2`, and `local_sys` fields define its carrier?

**Known.** `creo_prt.md` §8.5 "The named entity in `ent_list(conic)` declares compact `id`, `type`, and" defines the named and positional conic fields, frame grammar, and field invariants. `creo_prt.md` §8.5 "A type-30 conic record defines a complete ellipse carrier without interpreting" defines type 30 as an ellipse.

**Need.** We must know the equation to construct a hyperbola in model space.

### GS-16. Other conic types

**Question.** What carrier does each `MdlRefInfo` conic type other than the defined ellipse, parabola, and hyperbola types represent?

**Known.** `creo_prt.md` §8.5 "The named entity in `ent_list(conic)` declares compact `id`, `type`, and" defines the common conic record grammar and retains all type and coefficient fields.

**Need.** We must know the type mapping to construct the correct carrier.

### GS-17. Other positional cylinders

**Question.** What model-space equation does each positional cylinder body outside the defined analytic and generated round-edge forms encode?

**Known.** `creo_prt.md` §3.2 "`srf_prim_ptr` records contain the surface prototype fields" through `creo_prt.md` §3.2 "Positional cylinder rows store cap-plane point data rather than a `local_sys` replay." define the recognized cylinder row families and their placement invariants, including the `11 10 13` placement-witnessed inline cylinder and generated type-24 round-edge endpoint bodies. The first directrix-coordinate positive DICT lattice covers prefixes `4b..a3`.

In the seven-scalar directrix-coordinate lane, the first scalar is positive. It
usually equals the axial corner span: example pairs include `1.7` and `1.7`,
and `0.74` and `0.7399999999999998`. It does not always equal that span: one
example has a first scalar of `1.78` and an axial corner span of `1.68`. Its
role is not known.

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

**Need.** We must know the equation to construct the cylinder carrier.

### GS-19. Positional cone station token

**Question.** What scalar value does the three-byte station token between a positional cone's model-reference token and half-angle encode?

**Known.** `creo_prt.md` §3.2 "A positional cone suffix consists of exactly one complete nine-slot support" states that the support frame and half-angle define the exact cone independently of this token.

**Need.** We must know the value to preserve the native cone parameters.

### GS-21. Non-prismatic round radii

**Question.** Which fields define the varying radius of a non-prismatic round?

**Known.** `creo_prt.md` §6 "Classes 913" through `creo_prt.md` §6 "For a class-913 cylindrical slot fillet, the first two `geoms_affected`" define the recognized edge-treatment schemas, positional replay, resolved constant-radius forms, and the legacy Sld_Features/Sld_FullData constant-radius join. That join does not define a varying-radius blend.

**Need.** We must know the fields to construct the varying-radius blend.

### GS-22. Round flank geometry

**Question.** Which fields define the flank geometry of a round or fillet?

**Known.** The feature recipe retains affected geometry, affected edges, contours, and generated entities as separate identifier arrays.

**Need.** We must know the fields to construct the blend surface and its trims.

### GS-23. Round generated-face bindings

**Question.** Which relation binds each round or fillet result to its generated face instances?

**Known.** `creo_prt.md` §6 "Classes 913" through `creo_prt.md` §6 "For a class-913 cylindrical slot fillet, the first two `geoms_affected`" define the generated-surface arrays and the rowless-cylinder special case.

**Need.** We must know the binding to add the generated faces to the body topology.

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

### SP-03. Section-to-datum joins

**Question.** Which additional fields join a section definition to its sketch datum when no unique bounded-source owner chain or generated-datum parent remains?

**Known.** `creo_prt.md` §6 "`DEPDB_DATA` and each complete bounded `AllFeatur` feature row store an" defines the recipe, consecutive datum, and `gsec3d_ptr.sketch_plane` join. The definition is eligible only inside its complete source range. The same section-plane entity used by multiple definitions remains ambiguous. `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "When the sketch plane resolves to a placed plane carrier" define the unique generated-datum parent-table join and the `ActDatums` geometric identifiers.

**Need.** We must define a transfer for definitions that have no unique bounded-source chain and no unique generated-datum parent without inferring a feature owner or model-space frame.

### SP-04. Other relation equations

**Question.** What equation does each relation type outside signed type 0, type 1 angular, type 5, type 6, and type 14 encode?

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

**Question.** What does a type-35 operand identify when it does not resolve through a section entity?

**Known.** Section-entity identifiers and relation identifiers occupy separate namespaces.

**Need.** We must know the referent to evaluate the relation.

### SP-07. Solver sentinel

**Question.** What solver state does `ed ba 10 0c 8d ee 90 b4 0c` encode?

**Known.** `creo_prt.md` §5 "Within each" defines the nine-byte `ed <tail8>` production as one dimension-driven sentinel slot.

**Need.** We must know the state to evaluate or reject the affected variable correctly.

### SP-08. Multiple `local_sys` frames

**Question.** What geometric role does each feature-definition `local_sys` frame have when one definition contains more than one frame?

**Known.** `creo_prt.md` §6 "Feature-definition `local_sys f9 04 03` and `transf f9 04 03` bodies use the" defines the twelve-slot feature frame and its complete rank-two form.

**Need.** We must know each role to select the feature placement.

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

### SP-12. Dimension-driven variable join

**Question.** Which fields join a dimension-driven `var_arr` value to the relation dimension that drives it?

**Known.** `creo_prt.md` §5 "A `segtab` line whose two endpoint identifiers each have complete type-1 and" identifies the dimension-driven `var_arr` state. `uvar_id`, point key, relation identifier, relation dimension selector, and external dimension identifier are distinct identities.
The named `dimtab_ptr` prototype may also carry `dim_ref` rows with nullable
`item_id`, `sense`, and two nullable point slots. Those rows are distinct from
the `var_arr` solver-variable identity. A complete `eqtn_arr` uses zero-based
`var_arr` row ordinals for its argument slots. Function `2` transfers scalar
equality between two referenced rows; for two type-1 rows or two type-2 rows,
this transfers equality between the corresponding point coordinates. Function
`3` transfers the magnitude of a complete linear dimension into an unsigned
coordinate-difference constraint when its inline type-0 scalar agrees with
that magnitude, its scalar-equality component resolves to that magnitude, or
its type-0 value is the dimension-driven sentinel. The selected complete
dimension supplies that sentinel's resolved magnitude. A function-2 type-3/type-0
pair binds a positive type-3 radius row to a type-3 dimension row when the
resolved scalar values agree or the type-0 row is dimension-driven; the
selected dimension supplies the resolved scalar and radius value. A function-5
type-6/type-6/type-5 row with a zero type-5 selector transfers direct equality
between the two type-6 scalars; either finite scalar supplies the other row's
dimension-driven sentinel.

**Need.** We must know the remaining non-equality equation and relation joins
that assign a dimension value to a dimension-driven solver variable.

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

### SP-17. Solver-only external references

**Question.** What complete schema and projection rules handle qualified
external `ref_planes` rows, including a collection qualifier or non-null
`ext_ref_id` or `sub_index`, while preserving their typed model-item namespace?

**Known.** A complete `gsec3d_ptr.ref_planes` row binds its fourth field,
`known_ext_id` or `seg_id`, to the known section-entity identifier. The typed
external target is `(ref_type, plane_id)`. The six-field form adds `sub_index`
and `flip_flag`. Surface and edge references are distinct from table-class,
point-key, relation, and dimension namespaces. Missing local `segtab` geometry
does not prove an external binding. A counted class reference is not a
model-item identifier. The section location frame is a separate projection
input.

**Need.** We must own the complete row schema and any qualifiers before
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

### SP-25. Other revolution termination selectors

**Question.** How does each rotational-sweep selector other than the full-turn `angle_choice` form define its angular interval?

**Known.** `creo_prt.md` §6 "In a class-916 or class-917 positional feature row, feature form `2` selects a" defines `ea 44 00 00` as a complete 360-degree revolution. Linear sweep extents include one-sided, symmetric, and two-sided spans.

**Need.** We must know the selector semantics to trim a one-sided, symmetric, or two-sided revolution.

### SP-38. Conflicting feature recipe candidates

**Question.** Which byte-backed field selects the procedural recipe when one feature-state record contains more than one complete recipe name?

**Known.** `protextrude`, `cutextrude`, `protrevolve`, and `cutrevolve` are recognized recipe names. A unique DEPDB recipe binding supplies the recipe and its feature identifier.

**Need.** We must know the recipe discriminator and conflict rule to assign the feature family and Boolean effect.

### SP-39. Complete outer-entry ownership for simple-drilled dimensions

**Question.** Which complete PSB compound-entry grammar binds each class-911 dimension definition to its current outer feature key, including entries whose declared array count differs from the ordinary keyed-entry count?

**Known.** The current outer key and optional `feat_id / ref_model_info` replay-source reference are separate identities. A copied replay can retain an older source while belonging to a different current key. A complete three-row table still requires the recipe-selected external-ID/type roles. Equal envelopes, names, ordinals, or dimension tuples elsewhere do not establish that owner. A blind depth proved equal to the owned cylinder's axial span ends at its shoulder; the conical point extends beyond it. This independently earns `depth_to_tip = false` only with the cylinder-span and angle evidence.

**Need.** The parser must retain complete current-entry bounds and bind exactly one eligible table before model projection. Unknown compound/replay boundaries must abstain. A missing or invalid owned table must not borrow another feature's equal-radius/depth tuple. Different point-angle controls and envelope-free negatives must preserve both the ownership and Shoulder/Tip boundary.

### SP-41. Native counterbore and countersink additions

**Question.** Which complete feature-owned parameter grammar binds the independent counterbore and countersink additions when entry-profile or transition topology is unavailable?

**Known.** Class-911 `feat_type_ptr.form` distinguishes straight/extruded construction from sketched/revolved construction. It does not declare the detailed entry treatment. `draft_choice` and `misc_choice` are separate fields; a bit position without the complete enclosing production does not identify an entry treatment. Generated conical bottom geometry alone does not identify a countersink. A positively owned entry profile can distinguish a planar shoulder from a conical entry transition.

**Need.** Bind the complete current feature parameter production and its source profile before interpreting packed choices. Preserve counterbore and countersink evidence independently. Bound each outer keyed definition before the next owner; a named sketch prototype boundary does not define the owner interval. Unresolved operands or ambiguous entry transitions must remain unresolved.

### SP-43. Structural `e3` separator

**Question.** What separates a structural `e3` that starts an `ent_tab` replay
row from an `e3` that is the tail byte of a two-byte compact identifier?

**Known.** `creo_prt.md` §8.2 "`segtab` and `ent_tab` compact identifiers may
use `e3` as the tail byte" gives both uses. It gives that the tail is data and
not a row delimiter, and that an `ent_tab` replay row begins after a structural
`e3`.

**Need.** We must separate the two uses to frame `ent_tab` replay rows without
a byte scan.

**Conflict.** `saved_positional_generated_entities` in
`src/feature/definitions.rs:5449-5471` accepts each `e3` byte as a row start
when the following identifier joins a generated segment and an `e2` byte occurs
in the next 24 bytes. The specification does not give this window.

## 4. Topology and appearance

### TP-01. Current feature-to-body binding

**Question.** Which stored regime discriminator and complete current feature-local body references bind an admitted recipe to its input body?

**Known.** The recipe feature and preceding-feature fields in creation-event rows are feature identities, not body selectors. Modern named `body_tree`, `body_array`, and `used_bodies` records contain explicit `body_id` fields; `body_array.first_geom_ptr` supplies a separate material-geometry root. A body count does not identify a feature target. Current generated carriers identify feature outputs rather than input material.

**Need.** Bind complete body identities to the same model owner and current feature scope, then resolve their topology roster independently. Validate the part's material regime before applying a legacy single-material replay rule. Missing, duplicate, unresolved, or conflicting target identities must remain unresolved; neither chronology nor the first or sole decoded body selects a modern target. Creation history alone does not establish the current recipe or its body operands.

### TP-02. Sparse-edge topology binding

**Question.** Which DEPDB fields bind a sparse edge record to its final vertices, faces, and loops?

**Known.** Sparse DEPDB curve rows are one-sided topology views and retain their identifiers and suffix fields.

**Need.** We must know the binding to reconstruct the final B-rep.

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

**Question.** Which fields bind a rowless face-use reference to its face, loop, edge, and orientation outside the round-feature rowless-cylinder table?

**Known.** `creo_prt.md` §6 "Classes 913" through `creo_prt.md` §6 "For a class-913 cylindrical slot fillet, the first two `geoms_affected`" define the round-feature rowless-cylinder special case.

**Need.** We must know the general binding to construct those face uses.

### TP-06. Shared-surface face partition

**Question.** Which field partitions uses of one surface reference into separate face instances?

**Known.** A surface carrier identity does not by itself identify one bounded face instance.

**Need.** We must know the partition to prevent distinct faces from collapsing into one face.

### TP-07. `lo_restore.direction`

**Question.** What does the `lo_restore.direction` compact integer refer to, and how does it control loop traversal?

**Known.** `creo_prt.md` §6 "Within an `AllFeatur` `lo_restore` body, named-record type-one fields" states that it belongs to a loop-restoration edge record and is not a sweep direction or extent.

**Need.** We must know its referent and sense to restore the loop order.

### TP-08. `lo_restore.direction2`

**Question.** What does the `lo_restore.direction2` compact integer refer to, and how does it control loop traversal?

**Known.** `creo_prt.md` §6 "Within an `AllFeatur` `lo_restore` body, named-record type-one fields" states that it belongs to a loop-restoration edge record and is not a sweep direction or extent.

**Need.** We must know its referent and sense to restore the loop order.

### TP-09. Required `lo_hist` fields

**Question.** What semantic role does each of the four required `lo_hist` fields have?

**Known.** `creo_prt.md` §6 "A named `lo_id_tab_ptr` table can be followed in the same feature row by" defines the six-entry stored loop-history frame and preserves all field identities.

**Need.** We must know the roles to reconstruct feature-local loop history.

### TP-10. Optional `lo_hist` field

**Question.** What semantic role does the optional final `lo_hist` field have?

**Known.** The decoder preserves the optional field separately from the four required fields.

**Need.** We must know the role to apply the complete loop-history record.

### TP-11. Loop-to-boundary-surface join

**Question.** Which field joins a feature-local loop identifier to its boundary-surface curve network?

**Known.** Loop-history identifiers and surface and curve identifiers occupy separate namespaces.

**Need.** We must know the join to bind the restored loop to geometric edges.

### TP-12. Shell-to-body assignment

**Question.** Which byte-backed relation assigns a shell to a body when face-adjacency components and body-count fields disagree?

**Known.** Face adjacency gives connected shell components. A body-count field gives the expected body cardinality but not shell ownership. Legacy ASCII admission can identify eligible visible faces and retain a single admitted component; multiple admitted components still lack a shell-to-body join.

**Need.** We must know the relation to construct the correct body membership.

### TP-13. `element_colors` face binding

**Question.** Which fields bind an `element_colors` entry to an exact face instance?

**Known.** Geometry identity and face-instance identity are separate because one surface can support more than one face.

**Need.** We must know the binding to apply the color to the correct face.

### TP-14. `NeuPrtSld` face binding

**Question.** Which fields bind a `NeuPrtSld` appearance entry to an exact face instance?

**Known.** `creo_prt.md` §1.2 "| `NeuPrtSld` and display sections |" identifies `NeuPrtSld` as material, appearance, display, and tessellation data.

**Need.** We must know the binding to apply the appearance to the correct face.

### TP-15. Display-table face binding

**Question.** Which fields bind a display-table element to an exact face instance?

**Known.** Embedded display streams and model topology use separate record namespaces.

**Need.** We must know the binding to transfer per-face display data.

### TP-16. Other RGB lanes

**Question.** What scalar value does each undefined token in an RGB appearance lane encode?

**Known.** The decoder preserves complete recognized RGB values and retains undefined token bodies.

**Need.** We must know the values to construct the stored color.

### TP-17. Other appearance component lanes

**Question.** What scalar value does each undefined token in a non-RGB appearance component lane encode?

**Known.** Appearance records keep color and other material components in separate scalar lanes.

**Need.** We must know the values to construct the complete material.

### TP-18. `MdlStatus` prefix meanings

**Question.** What stored-name state does each `MdlStatus` prefix `o`, `x`, `y`, and `z` represent, and which field selects the current same-ID candidate?

**Known.** `creo_prt.md` §6 "Operation names end in" states that the prefix is not part of the operation-family name. Same-ID state candidates retain their byte order and exact prefixes. No candidate is projected as current without a selector.

**Need.** We must know the prefix meanings and current-state selector to preserve the native state semantics and project one current candidate.

**Conflict.** None.

### TP-19. `lo_array` roster semantics

**Question.** Which fields in a positional `lo_array` row bind its native
roster entry to a face, contour chain, curve header, and traversal orientation?

**Known.** `creo_prt.md` §3.5 defines the `lo_array` frame header, named
prototype fields, seven-field positional prefix, and bounded row body. The
decoder retains complete frame and row bytes without assigning neutral loop or
face meaning.

**Need.** We must know the joins before `lo_array` rows can contribute to
neutral loop ownership or face admission.

## 5. Packed persistence data

### PP-01. Packed `VisibGeom` records

**Question.** What geometry record grammar does packed `VisibGeom` use outside the defined PSB rows?

**Known.** `creo_prt.md` §3 "`VisibGeom`" defines material `VisibGeom` namespaces that contain PSB `srf_array` and `crv_array` rows.

**Need.** We must know the packed grammar to construct its remaining geometry.

### PP-02. `SolidPrimdata` strip continuation

**Question.** How does `SolidPrimdata` continue one triangle strip across record boundaries?

**Known.** `creo_prt.md` §8.4 "`SolidPrimdata` is a PSB compound stream. The named fields `p1`, `p2`, `pts`," defines primitive scalar arrays, cumulative strip sizes, vertex and normal tuples, and alternating triangle winding.

**Need.** We must know the continuation rule to construct all strip triangles.

### PP-03. `SolidPrimdata` persistent-segment binding

**Question.** Which fields bind a `SolidPrimdata` triangle-strip segment to its persistent model entity?

**Known.** The primitive stream defines tessellation coordinates and topology but does not assign a model face by position alone.

**Need.** We must know the binding to attach tessellation to the correct entity.

### PP-04. Expanded `SolidPersistTable`

**Question.** What row grammar and reference semantics does expanded `SolidPersistTable` use?

**Known.** The decoder retains the expanded table boundary and its exact row data.

**Need.** We must know the semantics to resolve persistent geometry identities.

### PP-05. Other `DEPDB_DATA` bodies

**Question.** What grammar does each `DEPDB_DATA` body outside the defined surface rows, section definitions, and feature recipes use?

**Known.** `creo_prt.md` §7 "DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They" defines fixed-prefix surface rows in `DEPDB_DATA`. `creo_prt.md` §6 "Within one current-state record, `protextrude` identifies an additive linear" through `creo_prt.md` §6 "Classes 913" define section and feature-definition boundaries.

**Need.** We must know the other grammars to transfer their design data.

### PP-06. Compressed `DispDataTable` dictionary

**Question.** What initial dictionary and code-width state does the compressed `DispDataTable` variant use?

**Known.** `creo_prt.md` §7 "DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They" defines `1f 9d 10` Unix-compress streams. `creo_prt.md` §8.3 "Unix-compress streams with header `1f 9d 10` grow code width" states that code 256 is a literal dictionary entry and not a clear code.

**Need.** We must define the initial dictionary, first available code, initial code width, and width-transition rule for this stream variant.

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
