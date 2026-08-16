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

## 1. Scalar and array encoding

### SE-01. Custom relation units

**Question.** How does each custom unit symbol convert a curve-relation value to canonical units?

**Known.** `creo_prt.md` §8.3 "Square brackets following a numeric literal or parameter expression contain a" defines the built-in unit symbols, their dimensions, and their canonical conversions. Celsius and Fahrenheit use affine conversions.

**Need.** We must know each custom conversion to evaluate a relation that uses the symbol.

### SE-02. Custom affine units

**Question.** Which custom unit definitions use an affine offset, and what offset does each definition use?

**Known.** `creo_prt.md` §8.3 "Square brackets following a numeric literal or parameter expression contain a" states that an affine unit cannot be part of a compound unit.

**Need.** We must know the offset to normalize temperature-like custom values.

### SE-03. Other curve-equation frame transitions

**Question.** What slot transition does each other stateful `local_sys f9 04 03` token encode?

**Known.** `creo_prt.md` §8.3 "The identifiers `r`, `theta`, and `z` define cylindrical curve coordinates" defines the twelve explicit frame slots. `creo_prt.md` §8.3 "A curve-equation entity carries its placement" assigns the rank-two image to the shared plane-frame production.

**Need.** We must know the other transitions to decode a complete curve-equation placement.

### SE-04. Other pcurve array transitions

**Question.** What slot transition does each other stateful `crv_pnt_arr f9 02 04` token encode?

**Known.** `creo_prt.md` §4.1 "A direct curve body consisting of exactly eight scalar slots and no references" defines the direct eight-slot pcurve body and its endpoint order.

**Need.** We must know the other transitions to decode all pcurve endpoint arrays.

### SE-05. Other positive DICT prefixes

**Question.** What value does each positive DICT prefix outside a defined scalar lane encode?

**Known.** `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" defines the DICT reconstruction rule and the lane-specific prefix tables. `creo_prt.md` §2.3 "Each record grammar defines the DICT lane for its scalar slots." gives the lane-specific priority rule.

**Need.** We must know the remaining prefixes to decode scalar values without using a value from the wrong lane.

### SE-06. Negative DICT prefixes

**Question.** What value does each undefined negative DICT prefix encode in its scalar lane?

**Known.** `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" defines the negative prefixes that have a complete reconstruction rule and the byte widths of unresolved forms. `creo_prt.md` §5 "`feat_outl_info.outline f9 02 03` stores six sequential feature-local scalar" states that an undefined prefix does not remove a scalar slot.

**Need.** We must know the remaining prefixes to decode geometry records that contain negative values.

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

### SE-10. Six-scalar terminal frames

**Question.** What role does each value in an unsuffixed terminal six-scalar frame have?

**Known.** `creo_prt.md` §8.5 "`MdlRefInfo`" defines six-scalar `MdlRefInfo` line rows as `end1.xyz` followed by `end2.xyz`. Equality of one coordinate pair does not identify that grammar in another record family.

**Need.** We must know the roles to distinguish endpoints, frame vectors, plane data, and trailers.

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

## 2. Curves and surfaces

### GS-01. Cone half-angle overrides

**Question.** What grammar and value rule apply to a per-instance cone half-angle that is not the terminal positive-DICT form?

**Known.** `creo_prt.md` §3.2 "A positional cone suffix consists of exactly one complete nine-slot support" defines the terminal positive-DICT half-angle form and the support-frame construction.

**Need.** We must know the override to construct the cone carrier.

### GS-02. Torus and sphere radius overrides

**Question.** What grammar and value rule apply to a `geom_type = 26` radius body that is not a tagged radius trailer or terminal prototype-minor-radius replay?

**Known.** `creo_prt.md` §3.3 "A `srf_prim_ptr(torus)` prototype stores" through `creo_prt.md` §3.3 "In named `radius`, `radius1`, and `radius2` fields" define the recognized torus and sphere radius forms and their carrier invariants.

**Need.** We must know the override to construct the torus or sphere carrier.

### GS-03. Later spline prototype joins

**Question.** Which field joins a later positional spline row to its prototype?

**Known.** `creo_prt.md` §3.2 "`srf_prim_ptr` records contain the surface prototype fields" through `creo_prt.md` §3.2 "Cylinder and cone prototype local systems are parameter templates" define named surface prototypes and the positional replay forms that have a proven join.

**Need.** We must know the join to apply the correct spline degree, knots, control points, and weights.

### GS-04. Spline intersection-curve joins

**Question.** Which field joins a spline surface to each surface-intersection curve on that surface?

**Known.** `creo_prt.md` §5 "An analytic" defines intersection-curve transfer when a surface pair and its endpoint witnesses select one candidate.

**Need.** We must know the join to bind the trim curve to the spline surface.

### GS-05. Prototype-adjacent `tab_cyl` points

**Question.** What geometric role does each point in a prototype-adjacent `tab_cyl` instance row have?

**Known.** `creo_prt.md` §3.2 "A `tab_cyl`" defines the prototype fields. `creo_prt.md` §3.2 "A repeated `tab_cyl` cubic-curve replay has this structure:" defines the separate repeated cubic replay.

**Need.** We must know the point roles to construct the ruled surface.

### GS-06. Prototype-adjacent `tab_cyl` parameters

**Question.** What does each parameter in a prototype-adjacent `tab_cyl` instance row control?

**Known.** `creo_prt.md` §3.2 "A `tab_cyl`" defines the bounded `params` field and its relationship to the prototype.

**Need.** We must know the parameter roles to define the surface chart and domain.

### GS-07. Ambiguous `tab_cyl` placement

**Question.** How does a replay-bound `tab_cyl` select its placement when its axis span matches neither directrix-coordinate range uniquely?

**Known.** `creo_prt.md` §3.2 "A repeated `tab_cyl` cubic-curve replay has this structure:" defines the repeated cubic replay and the unique axis-span placement cases.

**Need.** We must know the selection rule to place the surface in model space.

### GS-08. Other `fc 02` slots

**Question.** What role does each slot in an `fc 02` body have outside the defined short pcurve form?

**Known.** `creo_prt.md` §4.2 "Non-eight-slot curve bodies begin with `fc <subtype>`. The subtype selects a body-grammar class." identifies `fc 02` as a short pcurve-style endpoint family.

**Need.** We must know the roles to construct its curve and endpoints.

### GS-09. Other `fc 05` variants

**Question.** What grammar does an `fc 05` body use when it does not satisfy the defined cap-circle production?

**Known.** `creo_prt.md` §4.2 "`fc 05` records store cap-circle control points in the order `A`, `B`, `t`, `C`, where `A` and" through `creo_prt.md` §4.2 "One `fc 05`" define complete point groups, scalar lanes, termination, cylinder binding, placement, and circle construction for cap-circle bodies.

**Need.** We must know the other grammar to construct or reject the curve correctly.

### GS-10. `fc 08` grammar

**Question.** What is the complete body grammar for `fc 08`?

**Known.** `creo_prt.md` §4.2 "Non-eight-slot curve bodies begin with `fc <subtype>`. The subtype selects a body-grammar class." identifies `fc 08` as a world-coordinate control-polyline family. Recognized coordinate tokens and opaque spans partition its retained body.

**Need.** We must know the grammar to construct the control polyline.

### GS-11. `fc 13` full sample fields

**Question.** What role does each field in a full `fc 13` sample group have?

**Known.** `creo_prt.md` §4.2 "Non-eight-slot curve bodies begin with `fc <subtype>`. The subtype selects a body-grammar class." identifies `fc 13` as a held-cap-ordinate control polyline.

**Need.** We must know the roles to construct the control polyline.

### GS-12. `fc 13` terminal form

**Question.** Is the shortened held-coordinate-plus-two-field form in `fc 13` a final sample or a trailer?

**Known.** The decoder retains the repeated full groups and the shortened terminal form as separate bounded data.

**Need.** We must know the form to determine the final control-point count.

### GS-13. Other `fc` subtypes

**Question.** What body grammar does each `fc 04`, `fc 07`, `fc 09`, and `fc 0a` subtype use?

**Known.** `creo_prt.md` §4.2 "Non-eight-slot curve bodies begin with `fc <subtype>`. The subtype selects a body-grammar class." defines the common `fc <subtype>` opener and the recognized subtype families.

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

**Question.** What model-space equation does each positional cylinder body outside the defined local-system, compact axis-aligned, referenced planar-envelope, held-axis axial/radial, and repeated-diameter forms encode?

**Known.** `creo_prt.md` §3.2 "`srf_prim_ptr` records contain the surface prototype fields" through `creo_prt.md` §3.2 "Positional cylinder rows store cap-plane point data rather than a `local_sys` replay." define the recognized cylinder row families and their placement invariants.

**Need.** We must know the equation to construct the cylinder carrier.

### GS-18. Other positional cones

**Question.** What model-space equation does each positional cone body outside the support-apex suffix and planar-envelope forms encode?

**Known.** `creo_prt.md` §3.2 "A repeated-diameter type-24 round body stores two scalar diameter endpoints" through `creo_prt.md` §3.2 "Positional cylinder rows store cap-plane point data rather than a `local_sys` replay. Their" define the recognized cone support, apex, axis, radial ratio, and half-angle construction.

**Need.** We must know the equation to construct the cone carrier.

### GS-19. Positional cone station token

**Question.** What scalar value does the three-byte station token between a positional cone's model-reference token and half-angle encode?

**Known.** `creo_prt.md` §3.2 "A positional cone suffix consists of exactly one complete nine-slot support" states that the support frame and half-angle define the exact cone independently of this token.

**Need.** We must know the value to preserve the native cone parameters.

### GS-20. Other non-plane surface rows

**Question.** What model-space equation does each non-plane surface row outside the defined analytic and spline families encode?

**Known.** `creo_prt.md` §3.1 "A decoder must not infer the kind of a row without" defines the normalized surface-family mapping. `creo_prt.md` §3.2 "`srf_prim_ptr` records contain the surface prototype fields" through `creo_prt.md` §3.4 "A standard positional envelope is exactly ten contiguous scalar slots" define the surface prototypes and the recognized positional constructions.

**Need.** We must know the equation to construct the remaining surface carriers.

### GS-21. Non-prismatic round radii

**Question.** Which fields define the varying radius of a non-prismatic round?

**Known.** `creo_prt.md` §6 "Classes 913" through `creo_prt.md` §6 "For a class-913 cylindrical slot fillet, the first two `geoms_affected`" define the recognized edge-treatment schemas, positional replay, and resolved constant-radius forms.

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

### SP-01. Nonlinear simultaneous equations

**Question.** How must a nonlinear curve-equation `SOLVE` block be solved?

**Known.** `creo_prt.md` §8.3 "`SOLVE` opens a simultaneous-equation block and `FOR` followed by one or more" defines the block framing. The decoder solves complete, dimensionally valid affine systems and smooth numeric nonlinear systems when one finite full-rank root is established, and retains other blocks.

**Need.** The neutral format model does not yet define a transfer for non-smooth, piecewise, or discrete function forms whose affine reduction does not resolve them.

### SP-02. Other simultaneous-solve states

**Question.** How must a simultaneous-solve block evaluate when an unknown does not have a previous numeric value?

**Known.** The defined affine solver uses the ordered equations, declared unknowns, and previous or uniquely constrained physical dimensions. An underdetermined dimension system remains unresolved. A nonlinear block without a preceding finite numeric value for every unknown remains unresolved.

**Need.** We must know the initialization rule to evaluate the block deterministically.

### SP-03. Section-to-datum joins

**Question.** Which additional fields join a section definition to its sketch datum when no unique bounded-source owner chain or generated-datum parent remains?

**Known.** `creo_prt.md` §6 "`DEPDB_DATA` and each complete bounded `AllFeatur` feature row store an" defines the recipe, consecutive datum, and `gsec3d_ptr.sketch_plane` join. The definition is eligible only inside its complete source range. The same section-plane entity used by multiple definitions remains ambiguous. `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "When the sketch plane resolves to a placed plane carrier" define the unique generated-datum parent-table join and the `ActDatums` geometric identifiers.

**Need.** We must define a transfer for definitions that have no unique bounded-source chain and no unique generated-datum parent without inferring a feature owner or model-space frame.

### SP-04. Other relation equations

**Question.** What equation does each relation type outside signed type 0, type 5, and type 14 encode?

**Known.** `creo_prt.md` §5 "Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer" through `creo_prt.md` §5 "A positive-ratio elliptical cone uses local frame coordinates" define the recognized linear, radius, incidence, and entity-geometry relations. Complete `eqtn_arr` function-0, function-2, function-3, and function-35 rows define radial endpoint, scalar equality, unsigned coordinate distance, radius binding, and point-on-line equations when their positional row grammars are complete.
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
and other function-43 forms remain native.

**Need.** We must know each equation to solve the section geometry.

### SP-05. Type-1 direction selectors

**Question.** What direction does each selector in a type-1 angular relation specify?

**Known.** The decoder retains the relation type, operands, direction selector, and value as separate fields.

**Need.** We must know the direction to select the correct signed angle.

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

**Question.** What geometric role does each feature-definition `transf` frame have when one definition contains more than one frame?

**Known.** `creo_prt.md` §6 "Feature-definition `local_sys f9 04 03` and `transf f9 04 03` bodies use the" defines the twelve-slot transform body. A unique feature-bound section transform selects its section definition.

**Need.** We must know each role to select the section-to-model transform.

### SP-10. Frame selection order

**Question.** Which field selects one frame when a feature definition contains multiple complete `local_sys` or `transf` frames?

**Known.** Frame order alone does not override the unique owner and feature-bound transform rules.

**Need.** We must know the selector to avoid an arbitrary placement.

### SP-11. `relat_ptr` operand-vector roles

**Question.** What entity or locus does each of the three four-slot `relat_ptr` operand vectors identify?

**Known.** The decoder preserves the three vectors independently and does not combine their namespaces.

**Need.** We must know the roles to bind the correct relation operands.

### SP-12. Dimension-driven variable join

**Question.** Which fields join a dimension-driven `var_arr` value to the relation dimension that drives it?

**Known.** `creo_prt.md` §5 "A `segtab` line whose two endpoint identifiers each have complete type-1 and" identifies the dimension-driven `var_arr` state. `uvar_id`, point key, relation identifier, relation dimension selector, and external dimension identifier are distinct identities.
The named `dimtab_ptr` prototype may also carry `dim_ref` rows with nullable
`item_id`, `sense`, and two nullable point slots. Those rows are distinct from
the `var_arr` solver-variable identity. A complete `eqtn_arr` uses zero-based
`var_arr` row ordinals for its argument slots. Function `2` transfers scalar
equality between two referenced rows; for two type-1 rows or two type-2 rows,
this transfers equality between the corresponding point coordinates. Function
`3` transfers a complete non-negative linear dimension into an unsigned
coordinate-difference constraint when its inline type-0 scalar agrees with the
selected dimension row, its scalar-equality component resolves to that value,
or its type-0 value is the dimension-driven sentinel. The selected complete
dimension supplies that sentinel's resolved scalar. A function-2 type-3/type-0
pair binds a positive type-3 radius row to a type-3 dimension row when the
resolved scalar values agree or the type-0 row is dimension-driven; the
selected dimension supplies the resolved scalar and radius value. A function-5
type-6/type-6/type-5 row with a zero type-5 selector transfers direct equality
between the two type-6 scalars; either finite scalar supplies the other row's
dimension-driven sentinel.

**Need.** We must know the remaining non-equality equation and relation joins
that assign a dimension value to a dimension-driven solver variable.

### SP-13. `relat_ptr.used` states

**Question.** What solver state does each value of `relat_ptr.used` represent?

**Known.** The field has more than two values and is not a Boolean constraint-activation flag.

**Need.** We must know the states to decide how the solver uses the relation.

### SP-14. Unary point incidence

**Question.** What neutral constraint does a unary type-1 or type-2 `skamp_ptr` incidence represent when its sense-0 operand is a `segtab` point and it has no matching `verhor` or type-14 axis line?

**Known.** `creo_prt.md` §5 "Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer" through `creo_prt.md` §5 "A positive-ratio elliptical cone uses local frame coordinates" define the incidence forms that have a proven point, line, or axis structure.

**Need.** We must know the constraint to transfer it without inventing an axis.

### SP-15. Unary type-33 incidence

**Question.** What neutral constraint does a unary type-33 `skamp_ptr` incidence with flags 34 and a sense-10 bounded-curve operand represent?

**Known.** The decoder retains the type, flags, sense, and bounded-curve identity.

**Need.** We must know the constraint to transfer its design intent.

### SP-16. Other `skamp_ptr` geometry families

**Question.** What geometry family does each `skamp_ptr` entity code outside the defined point, endpoint-bearing curve, line, arc, and circle roles identify?

**Known.** The defined roles use incidence structure and section entities to prove their geometry family.

**Need.** We must know the mapping to create the correct neutral constraint operands.

### SP-17. Solver-only external references

**Question.** Which record binds a solver-only `skamp_ptr` entity identifier to its external geometry?

**Known.** A solver-only identifier does not bind through the local `segtab` geometry join.

**Need.** We must know the binding to evaluate constraints on external geometry.

### SP-18. `exists()` model-item joins

**Question.** Which owner and namespace joins expose model, feature, component, and scoped dimension items to a curve-expression `exists()` query?

**Known.** `creo_prt.md` §8.3 "A curve-from-equation entity stores `expression f8 <count>` followed by exactly" defines the `exists()` query's complete-program identifier and decoded-section-dimension namespaces. Other item classes retain their complete source identity.

**Need.** We must know the joins to return the correct query result.

### SP-19. Sweep axis without `ActDatums`

**Question.** Which DEPDB relation supplies the `protextrude` or `protrevolve` axis when the part has no `ActDatums` section?

**Known.** `creo_prt.md` §8.3 "A `protextrude` or `protrevolve` operation references its sweep axis through" states that the operation references its axis through `gsec3d_ptr` placement fields, not through an inline vector.

**Need.** We must know the relation to construct the sweep direction or revolution axis.

### SP-20. Default sweep datums

**Question.** Which feature-definition datum default or standard-datum convention supplies a sweep axis when no explicit datum joins to the section?

**Known.** Datum names do not define geometric orientation. A referenced datum can orient an in-plane axis only when it is perpendicular to the sketch-plane normal.

**Need.** We must know the convention to place the sweep without guessing from a name.

### SP-21. Competing regeneration snapshots

**Question.** Which field selects the current regeneration snapshot when several section definitions select the same internal sketch-plane entity?

**Known.** `creo_prt.md` §6 "`DEPDB_DATA` and each complete bounded `AllFeatur` feature row store an" states that the immediate feature-state chain does not select a snapshot when more than one definition uses that entity.

**Need.** We must know the selector to bind the feature to one section definition.

### SP-22. Ambiguous sketch-datum parent

**Question.** How does a section select its sketch datum when the generated-datum parent-table remainder is not unique?

**Known.** `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "When the sketch plane resolves to a placed plane carrier" define the unique remainder rule and the nested `plane_id` join.

**Need.** We must know the selection rule to place the sketch.

### SP-23. Parallel orienting datum

**Question.** Which datum supplies the in-plane orientation when the nested reference datum is parallel to the sketch normal?

**Known.** `creo_prt.md` §8.1 "`ref_planes`" states that the nested datum orients an in-plane axis only when it is perpendicular to the sketch-plane normal.

**Need.** We must know the alternate datum to complete the sketch frame.

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

### SP-39. Simple-drilled template selection and depth endpoint

**Question.** Which replay identity distinguishes class-911 three-row
simple-drilled dimension tables that have the same bore-radius and blind-depth
envelope, and does the blind depth terminate at the cylindrical shoulder or
the conical tip?

**Known.** `creo_prt.md` §6 "A class-911 table-class-29 simple-drilled recipe"
defines the generated-surface recipe and the bore-radius, included-angle, and
blind-depth dimension roles. The paired cylinder parameter records provide
per-axis common and adjacent-union spans. Bore diameter and blind depth select
the complete tables that match those spans on distinct axes. All matching
tables must define one equal tuple. Two rowless materialization-source pairs
select the external-ID-2 depth family; three select the external-ID-4 depth
family. Complementary envelopes define placement
directly. A one-sided envelope pair defines the second radial coordinate when
the patches share exactly one normalized bound and their non-shared bounds
differ by the bore diameter. A clipped envelope pair defines its missing radial
coordinate when the corresponding seven-token compound-close cone bodies
cross-bind the cylinder corners in generated order. The neutral hole bottom
retains no depth-to-tip state.

**Need.** We must identify the per-feature replay join when competing tuples
have equal bore-radius and blind-depth envelopes, and identify the depth
endpoint to set `HoleBottom::Angled`.

## 4. Topology and appearance

### TP-01. DEPDB recipe-to-body binding

**Question.** Which DEPDB fields bind a feature recipe to the body topology that it changes?

**Known.** `creo_prt.md` §7 "DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They" states that sparse DEPDB curve rows do not encode final loops or trims. Feature identifiers bind materialized surface carriers to generating features.

**Need.** We must know the binding to apply the feature to the correct body.

### TP-02. Sparse-edge topology binding

**Question.** Which DEPDB fields bind a sparse edge record to its final vertices, faces, and loops?

**Known.** Sparse DEPDB curve rows are one-sided topology views and retain their identifiers and suffix fields.

**Need.** We must know the binding to reconstruct the final B-rep.

### TP-03. Multi-loop classification

**Question.** Which byte-backed field identifies an outer loop or an inner loop on a multi-loop face?

**Known.** Parameter-space containment can classify loops only when complete pcurves and a surface chart are available.

**Need.** We must know the field to classify loops when containment is unavailable.

**Note.** The closure commit derives a common plane from solved boundary vertices and uses geometric containment. It does not identify the byte-backed field asked by this item, and its hand-built point test does not establish that coplanarity is the native ownership rule. Reopen until a native topology witness settles loop ownership.

### TP-04. Vertex-coordinate binding

**Question.** Which fields bind a topology vertex identifier to its XYZ coordinates?

**Known.** `creo_prt.md` §8.2 "`vert_tab` chains bind a solved trim-vertex identifier to its incident `segtab` external" through `creo_prt.md` §8.2 "`vert_tab` chains bind a solved trim-vertex identifier to its incident `segtab` external" define coordinate recovery from unique incident analytic carriers.

**Need.** We must know the stored binding to place vertices that carrier intersection cannot resolve uniquely.

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

**Known.** Face adjacency gives connected shell components. A body-count field gives the expected body cardinality but not shell ownership.

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

**Need.** We must know the initial state to decompress the table deterministically.

**Note.** The closure tests generate compressed streams with the implementation's chosen dictionary and width rules and decode them with the same implementation. No native `DispDataTable` byte stream or independent compressor/parser evidence is recorded. Passing those tests cannot verify the initial dictionary or width transition. Reopen until the framing rule is evidenced.

### PP-07. Compressed `DispDataTable` geometry binding

**Question.** Which fields bind decompressed `DispDataTable` rows to model geometry?

**Known.** The compressed stream contains display data in a namespace separate from material face instances.

**Need.** We must know the binding to apply display data to the correct entities.

### PP-08. Configuration driver-table traversal

**Question.** How do references traverse the configuration driver table selected by a non-null `FamilyInf.drv_tbl_ptr`?

**Known.** `creo_prt.md` §8.3 "`FamilyInf.Sld_FamilyInfo.drv_tbl_ptr` is the configuration driver-table" defines the null and referenced pointer forms and the configuration-root identity. A null pointer means that the part has no family-table configurations.

**Need.** We must know the traversal to enumerate all configuration rows.

### PP-09. Configuration driver-table rows

**Question.** What does each row and field in the configuration driver table represent?

**Known.** A non-null pointer preserves the canonical driver-table entity identifier.

**Need.** We must know the row semantics to transfer configuration parameters and values.

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

**Need.** We must know the semantic axis order of multidimensional type-2
arrays, the character-set selection for non-UTF-8 type-10 strings, type-10
continuation semantics,
geometry and design-history graph joins, and non-attribute section grammar to
transfer the rest of legacy persistence.
