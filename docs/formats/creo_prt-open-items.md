# Creo Parametric `.prt` (PSB): Open Items

This document lists the parts of the Creo Parametric `.prt` format that we do not know. The specification `creo_prt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

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

### SE-14. Paired cache tail collisions

**Question.** Which `46 <byte1> <tail6>` token supplies the leading byte for a `9e` or `a3` token when more than one cache token has the same six-byte tail and a different `<byte1>`?

**Known.** `creo_prt.md` §2.3 "Lane-specific seven-byte forms include" defines the paired reconstruction and names one source token for each tail. The section cache holds each distinct eight-byte `46` image, so two tokens can share a tail and differ in `<byte1>`. A round value has an all-zero tail, thus a collision is common.

**Conflict.** The specification names one source token. `scalar.rs` `ScalarCache::from_section` keeps the first token scanned for each tail through `or_insert` and discards the other tokens. It applies no uniqueness gate.

**Need.** We must know the selection rule to reconstruct the correct magnitude. A wrong selection gives a finite coordinate or radius with the exponent of a different value, and no gate rejects it.

### SE-15. `18` standalone-zero and cache-index boundary

**Question.** Which rule separates a standalone-zero `18` from an `18 <index>` cache reference?

**Known.** `creo_prt.md` §2.3 "`0d` encodes negative one" states that `18` followed by any defined scalar opener encodes a standalone zero, and that `18 <index>` indexes the section-local `46` cache.

**Need.** We must know the complete set of defined scalar openers to separate the two forms. `scalar.rs` `decode_in_lane` tests the following byte against the local `LANE_OPENERS` table. Each entry in that table removes one compact-integer head from the reachable index range, so every cache index whose head byte is in the table cannot be read. The table also holds bytes that `decode` does not define.

### SE-16. `MdlRefInfo` arc row scalar lane

**Question.** Which scalar lane do the coordinates of an `ent_list(arc_z)` row use?

**Known.** `creo_prt.md` §2.3 "Lane-specific seven-byte forms include" defines the lane-specific prefix tables. `creo_prt.md` §2.3 "`<prefix> <tail6>` uses the prefix" gives the lane-specific priority rule. The tabulated-cylinder first-coordinate lane and the model-reference lane assign opposite signs to prefixes `46` and `2d`, and different magnitudes to prefixes `af`, `b0`, and `b1`.

**Need.** We must know the lane to give the arc its correct sign. `reference.rs` `arc_z_coordinate` tries the tabulated-cylinder first-coordinate lane before the model-reference lane, where the sibling function `coordinate` uses the model-reference lane alone for line and conic rows of the same section. A sign inversion shared by every point of one arc keeps the circle invariants true, so no gate rejects it.

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

### GS-24. Prototype first-instance row selection

**Question.** Which condition makes the preceding adjacent surface row, and not the following adjacent row, the first instance of a named prototype?

**Known.** `creo_prt.md` §3.2 "Named prototype fields describe the first surface instance" states that the preceding adjacent row is the first instance when the prototype separates that row from replay rows, and that the following adjacent row is the first instance in other conditions.

**Conflict.** The specification makes the selection conditional. `decode.rs` `unique_surface_prototype_associations` selects the nearest preceding same-family row in all conditions and uses the following row only when the nearest preceding row has a different family. The decoder does not evaluate the separation condition.

**Need.** We must know the condition to apply prototype `local_sys`, radius, and spline fields to the correct surface row. A wrong selection gives one surface the geometry of a different surface of the same family. The per-row uniqueness filter rejects two prototypes that select one row; it does not reject two prototypes that each select a different wrong row.

### GS-25. Eight-slot type-24 terminal frame precedence

**Question.** Which rule separates the single-diameter round frame from the square-radial round frame when a type-24 terminal scalar frame has exactly eight slots?

**Known.** `creo_prt.md` §3.2 defines both forms. It gives a precedence rule against the repeated-diameter form only. At eight slots both grammars read the same six corner slots, so both are admissible.

**Need.** We must know the rule to build the correct cylinder axis and radius. `surface.rs` `type24_round_frame` accepts the single-diameter form first and does not test the square-radial form. A square-radial body whose auxiliary slot equals one span decodes as a cylinder with a diagonal axis and the wrong radius, and the face is placed on that carrier.

### GS-26. Positional cylinder terminal radius boundary

**Question.** Which token boundary separates the twelve-slot `local_sys` from the terminal radius in a positional cylinder body?

**Known.** `creo_prt.md` §3.2 states that the body holds exactly one complete twelve-slot positional `local_sys` and ends with one positive scalar radius. It gives no marker for the radius token start.

**Need.** We must know the boundary to read the radius and the local system. `surface.rs` `decode_local_system_cylinder_frame` and `decode_zero_support_cylinder_origin_radius` accept the lowest offset whose scalar decodes positive and ends at the body end. The sibling function `decode_positional_cylinder_origin_radius` collects every such offset and requires exactly one.

### GS-27. Named `srf_array` row discriminator defaults

**Question.** What surface-row state does a `boundary_type` byte outside `00`, `01`, `06`, and `f6`, or an `orient` byte outside `01` and `f6`, encode?

**Known.** `creo_prt.md` §3.1 defines the named-record row fields and the defined discriminator values.

**Need.** We must know the states to classify the row. `surface.rs` `rows_with_boundaries` gives a named row `boundary_type` zero when the byte is absent or undefined, and `reversed` false when `orient` is absent or undefined. It publishes both as byte-backed fields. The positional branch of the same function rejects the row in these conditions.

### GS-28. Curve parameter-record suffix boundary

**Question.** Which rule selects the body and suffix boundary of a curve parameter record when more than one four-reference suffix start is byte-valid?

**Known.** `creo_prt.md` §4.1 defines the suffix as four references before the record close.

**Need.** We must know the rule to bound the scalar lane. `curve.rs` `parameter_records` accepts the first candidate, which its `4..=11` scan makes the shortest suffix and the longest body. The record is marked ambiguous and its geometry consumers drop it, so the wrong split reaches the native arena only. The sibling function `topology_suffix` rejects the same condition.

### GS-29. `MdlRefInfo` positional row body extent

**Question.** What bounds the body of an `ent_list(line3d)` or `ent_list(arc_z)` row?

**Known.** `creo_prt.md` §8.5 states that exactly one seven-scalar run may satisfy the `line3d` endpoint and stored-length invariant, and that exactly one run may satisfy the `arc_z` circle invariant. The next row header or the block end bounds the row.

**Need.** We must know the extent to apply the uniqueness rule to the complete row. `reference.rs` `line3d_lines` and `arc_z_circles` compute the true bound and then reduce it to 384 and 256 bytes. Neither constant comes from the format. A competing run outside the window makes the decoder report one candidate where the specification requires a withhold.

### GS-30. `entity(line)` six-scalar suffix start

**Question.** Which byte offset begins the six-scalar endpoint suffix of an `entity(line)` row?

**Known.** `creo_prt.md` §8.5 states that a positional row defines a line only when exactly six finite scalars consume the complete suffix. It states no uniqueness rule for the start offset, where it states one for `line3d` and `arc_z`.

**Need.** We must know the start to read the endpoints. `reference.rs` `scalar_suffix` accepts the lowest qualifying offset. A qualifying run that begins inside the row header gives three header-derived coordinates, and the record carries no entity identifier for a cross-check.

### GS-31. Positional conic local-system boundary

**Question.** Which end offset bounds the twelve-slot local system of a positional conic row?

**Known.** `creo_prt.md` §8.5 states that a complete row consumes all twelve local-system slots before its trailing compound record. For the named conic row it requires the unique image that follows a complete twelve-slot frame.

**Need.** We must know the boundary to build the conic frame. `reference.rs` `positional_conic_body` accepts the shortest end offset that yields twelve slots. The sibling function `named_conic_local_system` tracks a competing frame and rejects the row when two boundaries both give a complete frame.

### GS-32. `tab_cyl` chart intercept magnitude

**Question.** Which field gives the first-axis intercept of a tabulated-cylinder directrix chart?

**Known.** `creo_prt.md` §3.2 states that layouts whose second and fifth scalar prefixes are `46` require a first-axis intercept magnitude of 30, and that every remaining complete replay-bound frame selects its chart from a zero-offset form or a form with a first-axis intercept magnitude of 30 and a reflected sweep-axis sign.

**Note.** The magnitude 30 is a length in model units. A length is not a container constant. The specification states the value as settled, so this item disputes the specification and the decoder together. `decode.rs` `placed_tabulated_cylinder_directrix` admits only the magnitudes 0 and 30. A directrix chart with any other intercept leaves the surface without a NURBS carrier.

**Need.** We must know the field to place every tabulated-cylinder directrix, and to know whether the two admitted magnitudes are a rule or a property of the parts that gave them.

## 3. Section solving and feature placement

### SP-01. Nonlinear simultaneous equations

**Question.** How must a nonlinear curve-equation `SOLVE` block be solved?

**Known.** `creo_prt.md` §8.3 "`SOLVE` opens a simultaneous-equation block and `FOR` followed by one or more" defines the block framing. The decoder solves complete, dimensionally valid affine systems over numeric unknowns and retains other blocks.

**Need.** We must know the nonlinear solve rules to evaluate all derived curve parameters.

### SP-02. Other simultaneous-solve states

**Question.** How must a simultaneous-solve block evaluate when an unknown does not have a previous numeric value?

**Known.** The defined affine solver uses the ordered equations, declared unknowns, dimensions, and previous numeric values.

**Need.** We must know the initialization rule to evaluate the block deterministically.

### SP-03. Section-to-datum joins

**Question.** Which fields join a section definition to its sketch datum when the defined owner and generated-datum joins do not select one datum?

**Known.** `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "n      = sketch_plane.normal" define the unique generated-datum parent-table join and the `ActDatums` geometric identifiers.

**Need.** We must know the join to place the sketch in model space.

### SP-04. Other relation equations

**Question.** What equation does each relation type outside signed type 0, type 5, and type 14 encode?

**Known.** `creo_prt.md` §5 "Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer" through `creo_prt.md` §5 "A positive-ratio elliptical cone uses local frame coordinates" define the recognized linear, radius, incidence, and entity-geometry relations.

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

**Need.** We must know the join to assign the dimension value to the solver variable.

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

**Known.** `creo_prt.md` §6 "DEPDB also stores an internal sketch-datum chain." states that the immediate feature-state chain does not select a snapshot when more than one definition uses that entity.

**Need.** We must know the selector to bind the feature to one section definition.

### SP-22. Ambiguous sketch-datum parent

**Question.** How does a section select its sketch datum when the generated-datum parent-table remainder is not unique?

**Known.** `creo_prt.md` §6 "`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly" through `creo_prt.md` §6 "n      = sketch_plane.normal" define the unique remainder rule and the nested `plane_id` join.

**Need.** We must know the selection rule to place the sketch.

### SP-23. Parallel orienting datum

**Question.** Which datum supplies the in-plane orientation when the nested reference datum is parallel to the sketch normal?

**Known.** `creo_prt.md` §8.1 "`ref_planes`" states that the nested datum orients an in-plane axis only when it is perpendicular to the sketch-plane normal.

**Need.** We must know the alternate datum to complete the sketch frame.

### SP-24. Named `ActDatums` outline tokens

**Question.** What scalar value does each `a5`, `9f`, `5c`, and `45` token encode in a named `ActDatums` outline?

**Known.** `creo_prt.md` §6 "`ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records. Each section" defines the two-corner outline and its held-coordinate plane rule.

**Need.** We must know the values to construct nonzero datum offsets and extents.

### SP-25. Other revolution termination selectors

**Question.** How does each rotational-sweep selector other than the full-turn `angle_choice` form define its angular interval?

**Known.** `creo_prt.md` §6 "In a class-916 or class-917 positional feature row, feature form `2` selects a" defines `ea 44 00 00` as a complete 360-degree revolution. Linear sweep extents include one-sided, symmetric, and two-sided spans.

**Need.** We must know the selector semantics to trim a one-sided, symmetric, or two-sided revolution.

### SP-26. `AllFeatur` row start grammar

**Question.** Which bytes begin an `AllFeatur` feature row, and what bounds the row?

**Known.** `creo_prt.md` §6 states that a feature owns each mixed generated-entity table bounded by its `AllFeatur` row, and that the fixed prefix contains `f6 <class> e1`. It states that the row's leading identifier occupies a row-local numeric namespace that can collide with model-feature identifiers, and that numeric equality alone does not establish ownership. The specification does not give the row-start grammar.

**Need.** We must know the grammar to bind every generated-entity table, affected-geometry array, and loop-history entry to its owning feature. `feature/rows.rs` accepts a start at a known feature identifier followed by one of the three constants `eb 04`, `90 01`, and `c8 10`, and ends the row at the next such start. The three constants appear in no specification or layout table.

**Note.** This grammar does not reach every feature. Many feature identifiers that carry a current operation state get no `AllFeatur` row, and a part can yield no row at all while its operation states name many features. One part uses one header constant for nearly every row, so the constant looks like a property of the writing generation rather than of the feature family. Both observations say the accepted set is a subset of the real row-start forms, so this item ranks above every other row-scoped item: each unreached row is a feature whose generated-entity tables and affected geometry are absent, not wrong.

### SP-28. Material feature precedence

**Question.** Which field orders two material features for the base-body selection?

**Known.** `creo_prt.md` §6 "For a linear section sweep, generated plane carriers" states that the first resolved section sweep in feature-definition order forms the base body. `creo_prt.md` §6 states that state ordinals are local to one feature identifier and increase in byte order from zero. The specification gives no byte order across features.

**Conflict.** The specification gives feature-definition order. `decode.rs` `feature_is_first_material_operation` compares the `state_offset` of current operation records. `container.rs` `feature_operations` collects those records from the `MdlStatus` and `DEPDB_DATA` sections and adds each section's base offset, so two features can carry offsets from two different sections. The comparison then measures the order of the sections, not the order of the features.

**Need.** We must know the field to select the base body. The comparison is the only gate between emitting a complete closed solid body and emitting none, in the revolution, extrusion, and circular-extrusion transfer paths.

### SP-29. Section reference-plane orientation rows

**Question.** Which reference-plane row supplies the `ref_type`, `seg_id`, and `flip_flag` that orient a section?

**Known.** `creo_prt.md` §8.2 states that each reference-plane row replays its own `plane_id`, `ref_type`, `ext_ref_id`, `seg_id`, `sub_index`, and `flip_flag`.

**Need.** We must know the row to apply the correct orientation sense. `feature/definitions.rs` `positional_section_3d` retains every `plane_id` but keeps the orientation fields of row zero alone. `placement.rs` then selects the orienting reference geometrically, as the unique referenced plane not parallel to the sketch plane, and applies row zero's `flip_flag` to it. When the selected row is not row zero, a missing negation mirrors the sketch and every surface swept from it.

### SP-32. `ActDatums` positional row acceptance

**Question.** Which bytes identify a positional `<gid> 22` datum row, and what bounds a datum geometry identifier?

**Known.** `creo_prt.md` §6 "`ActDatums` stores datum-plane geometry" states that `ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records, and that each section holds one named datum row and can hold positional `<gid> 22 ...` rows.

**Need.** We must know the acceptance rule to enumerate every datum plane. `datum.rs` `planes` scans the complete section, not the `srf_array` region, and accepts any offset whose identifier byte is nonzero and at most `0x40`, whose next byte is `22`, and whose bytes at `+3` and `+4` are in two local value sets. The `0x40` cap and both value sets come from no specification or layout table.

**Note.** The `0x40` cap has large headroom against the datum identifiers that standard datum planes use, so the cap is not the part of this item to answer first. The unanchored scan is: the function accepts a row anywhere in the section, and reads the identifier and the owning feature identifier as raw bytes at fixed offsets from the match.

### SP-33. Datum outline held-axis selection

**Question.** Which axis holds the plane equation when a named datum outline has multiple paired standalone-zero coordinates?

**Known.** `creo_prt.md` §3.4 "When exactly one coordinate is held constant across both corners" and §6 "`ActDatums` stores datum-plane geometry" require exactly one equal coordinate pair for a positional outline; zero or multiple equal pairs leave that plane unresolved. `creo_prt.md` §8.1 "In a named datum outline" gives paired standalone-zero slots precedence for a named plane at zero offset. `datum.rs` `planes` now rejects positional outlines with multiple held axes. `datum.rs` `named_plane` selects a paired standalone-zero axis before applying the unique-equal-pair rule to other named outlines.

**Conflict.** The specification does not state whether multiple paired standalone-zero coordinates are a valid named standard-plane form or how to rank them. `datum.rs` `named_plane` currently selects the first such axis in coordinate order.

**Need.** We must know whether to retain or withhold a named plane when more than one standalone-zero pair is present, and if it is valid, which axis supplies its normal. This affects the model-space normal and every sketch placement that references the datum.

### SP-34. Section line without an `order_table` row

**Question.** Which bytes give the geometry of a `segtab` line that has no `order_table` internal identifier?

**Known.** `creo_prt.md` §8.2 defines two recoveries for an omitted `order_table` row: the intervening internal identifier between adjacent stored rows, and the unique remaining pair of one unmatched saved entity and one unmatched solved entity of the same family. Neither recovery applies when the row is absent from the table.

**Need.** We must know the bytes to give the line its endpoints. `decode.rs` `saved_section_missing_line_geometry` takes the two unmated endpoints of the other evaluated entities, which assumes the profile is closed and that the missing line closes it. No byte of the missing line is read. An open profile with a dangling line gives a chord between the two free ends, and that geometry seeds trim-vertex coordinates and generated side surfaces.

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

**Question.** What stored-name state does each `MdlStatus` prefix `o`, `x`, `y`, and `z` represent?

**Known.** `creo_prt.md` §6 "Operation names end in" states that the prefix is not part of the operation-family name and does not select the current same-ID state. Byte order selects the current state.

**Need.** We must know the prefix meanings to preserve the native state semantics.

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

### PP-10. Compressed `THMB_IMG_MAIN` payload

**Question.** How does a decoder retain the JPEG payload of a `THMB_IMG_MAIN` section that uses Unix-compress framing?

**Known.** `creo_prt.md` §1.2 "| `THMB_IMG_MAIN`" states that the payload begins with `FF D8 FF` and holds no model geometry. `creo_prt.md` §1 "A section payload beginning" defines the Unix-compress framing and the expanded-length check. A `THMB_IMG_MAIN` payload takes either form: the marker can begin the payload directly, or the section can begin `1f 9d <flags>` and hold the marker only after expansion.

**Need.** We must know the retention rule to preserve the thumbnail of a compressed section. `decode.rs` `preserve_passthrough_sections` searches the raw section bytes for `FF D8 FF`. A compressed section holds no such window before expansion, so the function discards the section and emits no passthrough record. The `expanded_sections` arena then retains the section lengths and digest but no bytes, and no loss is reported. `container.rs` `has_thumbnail` searches the same raw bytes, so it reports the thumbnail as absent.

### PP-11. Layout family identification

**Question.** Which field gives the layout family of a part that carries no `ND:` section decoration?

**Known.** `creo_prt.md` §1.1 gives the section cardinality of each family as approximate: about 40 or more for ND, about 12 for DEPDB. The `ND:` section decoration identifies an ND part. No field states the family.

**Need.** We must know the field to select the layout-gated transfer paths. `container.rs` `identify_layout` uses the section counts 24 and 32 as cut-points. Neither constant comes from the specification. A DEPDB part with 32 or more enumerated sections is declared ND, which admits it to the first-instance prototype and paired-envelope-sphere transfers. An undecorated ND part with 25 through 31 sections is declared unknown, which withholds those transfers and reports no loss.
