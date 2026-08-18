# IGES CADIR decisions

These records define CADIR choices where IGES defines the transfer data but does not define receiver behavior. They do not add IGES fields or validity rules. Each record names the format silence, the selected rule, the evidence, the charged cost, and an executable condition that reopens the decision.

## D-01. Noncanonical physical lines

**Question.** What survives when a pre-Terminate Fixed ASCII line is not 80 bytes?

**Silence.** IGES defines the fixed card shape and section order but does not define inspection or container-only salvage.

**Rule.** Retain the line candidate and opaque remainder as separate source spans for inspection and container-only decode. Refuse semantic projection before Directory or Parameter Data decoding. Retain records after Terminate as ignored trailing records.

**Ground.** IGES 5.3 §2.2; `card.rs::physical_lines`, `card.rs::container_summary`, and `reader.rs::decode_with_occurrence_limits`; tests `card::tests::over_width_lines_are_retained_for_inspection_and_rejected_semantically`, `card::tests::inspect_retains_short_and_extended_physical_records_before_terminate`, and `card::tests::decode_retains_post_terminate_physical_record`.

**Cost.** Semantic decode returns `CodecError::Malformed`; inspection exposes `noncanonical-physical-records` and `post-terminate` entries. No neutral geometry is produced.

**Reopens.** Reopen if the specification or an independently authored producer requires semantic recovery from a noncanonical pre-Terminate line. The executable check is the three named card tests.

## D-02. Zero Global resolution

**Question.** What does Global field 19 mean when its valid value is zero?

**Silence.** IGES defines positive minimum resolution as a discernibility distance but does not state a zero comparison predicate.

**Rule.** Use exact coordinate equality at zero. For a positive value, accept only Euclidean distance strictly less than the value.

**Ground.** IGES 5.3 §2.2.4.3.19; `global.rs::coincident_distance`; test `global::tests::minimum_resolution_uses_a_strict_positive_boundary_and_exact_zero`, plus `composite::tests::composite_zero_global_resolution_requires_exact_join`.

**Cost.** No loss is charged for the equality predicate. A failed join remains under the owning entity's existing projection or geometry loss.

**Reopens.** Reopen if an authoritative receiver rule establishes inclusive comparison or a non-exact zero rule. The executable check is the named Global and composite boundary tests.

## D-03. Generated Global identity

**Question.** Which Global identity fields can a semantic writer fill when the neutral model has no native sender metadata?

**Silence.** IGES defines the fields and defaults but does not supply values for a semantic model with no sender identity, author, receiver, or application protocol.

**Rule.** Keep unavailable native identity, author, organization, modification time, receiver, and application-protocol fields NULL. Use the fixed `cadmpeg` sender identity and package version for the generated sender fields.

**Ground.** IGES 5.3 §§2.2.4.3.3–26; `writer.rs::encode_file`; test `writer::tests::encode::encode_computes_global_metadata_and_defaults_unavailable_identity`.

**Cost.** No loss is charged. The emitted file identifies the semantic writer and does not invent native provenance.

**Reopens.** Reopen if a neutral IR field or an external writer profile supplies the missing native identity with a defined precedence. The executable check is the named writer test and the Global-field table assertion in that test module.

## D-04. Global flag-three unit names

**Question.** How does semantic projection handle a Global unit name with no known millimetre factor?

**Silence.** IGES delegates the flag-three name space to MIL-STD-12 or IEEE 260 and supplies no file-wide conversion factor for an unknown custom name.

**Rule.** Retain every nonempty flag-three name in inspection and native data. Project lengths only for an exact name with a known factor. Refuse semantic projection for an unknown factor. Type 316 remains scoped to its owning data entity.

**Ground.** IGES 5.3 §2.2.4.3.14–15 and §4.77; `global.rs::has_supported_length_factor`, `reader.rs::decode_with_occurrence_limits`, and `native.rs` Type 316 projection; tests `global::tests::unknown_flag_three_unit_name_is_inspectable_but_not_projected` and `reader::tests::type316_property_does_not_supply_a_global_flag_three_factor`.

**Cost.** Container-only decode retains the name. Semantic decode returns `CodecError::NotImplemented` before coordinate projection; no partial neutral geometry is emitted.

**Reopens.** Reopen if a cited external unit registry supplies a stable exact factor for the name or changes flag-three precedence. The executable check is the two named tests.

## D-05. Invalid Global string bytes

**Question.** How are non-ASCII or control bytes in Global Hollerith strings retained and admitted?

**Silence.** IGES defines the string field but does not define a byte-exact inspection representation or container-only behavior for invalid string bytes.

**Rule.** Preserve invalid bytes in the source image and expose them as lowercase hexadecimal attributes. Container-only decode retains the framed Global record. Semantic decode refuses before Directory or Parameter projection.

**Ground.** IGES 5.3 §2.2.2.3; `global.rs::invalid_string_fields`, `reader.rs::source_meta`, and the semantic barrier in `reader.rs`; tests `global::tests::global_strings_admit_printable_ascii_and_retain_invalid_bytes`, `global::tests::non_utf8_global_identifiers_are_preserved_as_exact_hex_attributes`, and `reader::tests::container_only_retains_invalid_global_units_name_as_hex`.

**Cost.** Semantic decode returns `CodecError::Malformed`. Container-only decode charges no geometry loss and exposes the exact bytes as inspection attributes.

**Reopens.** Reopen if the format specification defines a permitted non-ASCII encoding or a producer contract requires semantic replacement rather than refusal. The executable check is the named Global and reader tests.

## D-06. Type 124 interval admission

**Question.** What receiver comparison admits and canonicalizes a Type 124 linear part?

**Silence.** IGES 5.3 §4.21 defines the linear-part invariants but gives no receiver epsilon, matrix comparison algorithm, or repair algorithm.

**Rule.** Derive coefficient uncertainty from each real token's declared significance. Admit when interval norms contain one, interval dot products contain zero, and the determinant interval contains the required sign. Canonicalize admitted coefficients to an orthonormal frame and retain translation.

**Ground.** IGES 5.3 §§2.2.2.2, 2.2.4.3.8–11, and 4.21; `entities/geometry.rs::transform`; tests `geometry::tests::decode_accepts_rounded_transformed_circular_arc_frame`, `geometry::tests::decode_rejects_transform_roundoff_beyond_its_declared_precision`, `geometry::tests::decode_rejects_occt_rounded_transform_under_declared_single_precision`, `geometry::tests::decode_applies_declared_double_precision_to_transform_coefficients`, and `geometry::tests::decode_canonicalizes_a_rounded_left_handed_transform`.

**Cost.** A failed admission retains the native transform and charges the existing entity projection loss. No fixed receiver epsilon or pre-admission repair is applied.

**Reopens.** Reopen if IGES 5.3 or an independent producer witness defines a different receiver comparison or repair rule. The executable check is the transform canonicalization and rejection tests.

## D-07. Resource ceilings

**Question.** What happens when a valid IGES count requests more semantic work than the codec can safely materialize?

**Silence.** IGES defines counts and entities, not implementation resource limits or partial-result semantics.

**Rule.** Apply the named per-entity and session ceilings before the corresponding allocation or work. Return a terminal `CodecError::ResourceLimit`; do not return a partial neutral result. Local occurrence truncation retains native expansion data and emits its named loss.

**Ground.** `reader.rs`, `entities/splines.rs`, `entities/copious.rs`, `entities/composite.rs`, `entities/surfaces.rs`, and `native.rs` name the limits; tests `reader::tests::decode_enforces_each_iges_session_resource_dimension`, `splines::tests::decode_refuses_a_parametric_spline_segment_count_over_its_projection_limit`, `copious::tests::decode_refuses_a_copious_tuple_count_over_its_projection_limit`, `composite::tests::decode_refuses_a_composite_child_count_over_its_projection_limit`, and the surface and occurrence limit tests.

**Cost.** Session exhaustion returns `CodecError::ResourceLimit`. Local Form 63 and occurrence exhaustion retain native records and charge `entity.not-projected`, `occurrence.expansion-output-truncated`, or `occurrence.expansion-depth-truncated` as applicable.

**Reopens.** Reopen if a documented target profile changes a ceiling, requires a partial result, or proves a named bound unsafe. The executable check is the resource-limit test set and the occurrence-loss tests.

## D-08. Declared unit-vector admission

**Question.** What numeric rule admits fields that IGES describes as unit vectors?

**Silence.** IGES describes the vector semantics but gives no numeric deviation or receiver normalization tolerance.

**Rule.** Derive component intervals from each real token's significance and admit a declared unit vector when its squared-norm interval contains one. Admit a declared orthogonal pair when its dot-product interval contains zero. Retain source components and normalize only after admission. Type 123's separate nonzero rule remains unchanged.

**Ground.** IGES 5.3 §§4.20, 4.25, 4.30, and 4.37; `entities/geometry.rs::declared_unit_vector` and `declared_orthogonal_vectors`; tests `geometry::tests::declared_unit_vector_uses_real_token_significance_as_its_interval` and the primitive, surface, and offset owner tests.

**Cost.** A failed declared-vector admission charges the owning entity's existing `entity.not-projected` loss. No source vector is silently normalized before validation.

**Reopens.** Reopen if an authoritative application protocol defines an absolute tolerance or permits normalization of a failed declaration. The executable check is the named geometry test and owner test matrix.

## D-09. Type 102 join salvage

**Question.** What does the decoder do when a Type 102 composite cannot form one exact neutral carrier?

**Silence.** IGES requires ordered, physically dependent constituents and coincident joins but does not define a receiver's carrier-salvage representation.

**Rule.** Apply the Global-resolution predicate to adjacent transformed endpoints. Build the exact ordered carrier when all children are bounded and joins pass. Otherwise retain the ordered native composite carrier and do not invent a fallback neutral curve.

**Ground.** IGES 5.3 §§1.4.4.1 and 4.4; `entities/composite.rs::project` and `project_degraded_composite`; tests `composite::tests::composite_join_uses_global_resolution_and_reports_degradation`, `composite::tests::composite_join_at_positive_global_resolution_boundary_is_not_coincident`, and `composite::tests::composite_zero_global_resolution_requires_exact_join`.

**Cost.** Degraded exact-carrier projection charges `curve.composite-carrier-degraded`; an unconstructable native carrier charges the existing entity projection loss. Ordered native data remains available.

**Reopens.** Reopen if a cited receiver rule defines a different join metric, inclusive boundary, or fallback carrier. The executable check is the three named composite tests.

## D-10. Type 104 endpoint admission

**Question.** Which endpoint values survive when they disagree with the coefficient-defined conic carrier?

**Silence.** IGES supplies both coefficient and endpoint fields but does not state whether a receiver replaces, projects, or rejects a disagreeing endpoint.

**Rule.** After units, scale, and transform, require each endpoint to lie strictly below positive Global resolution from the evaluated carrier; zero requires exact equality. Keep the declared endpoint as the neutral vertex. Refuse projection when either endpoint fails.

**Ground.** IGES 5.3 §§2.2.4.3.19 and 4.5; `entities/conics.rs::project`; tests `conics::tests::decode_brackets_conic_endpoint_agreement_at_the_global_resolution` and `conics::tests::decode_preserves_declared_conic_endpoints_as_neutral_vertices`.

**Cost.** A failed endpoint check retains the Type 104 native record and charges `entity.not-projected`; it does not substitute an evaluated point.

**Reopens.** Reopen if the published format or an independent producer establishes endpoint replacement or an inclusive tolerance. The executable check is the two named conic tests.

## D-11. Angular normalization slack

**Question.** What receiver slack normalizes an angular seam or a revolution sweep?

**Silence.** IGES defines angular domains but supplies no angular equality tolerance.

**Rule.** Use an absolute `2π × 10^-12` radian implementation slack only for floating-point normalization of full turns. Do not use it for coordinate coincidence or other geometric admission.

**Ground.** IGES 5.3 §§4.3–4.5 and 4.13; `entities/conics.rs` and `entities/surfaces.rs`; tests `conics::tests::decode_canonicalizes_ellipse_arc_seam_noise` and `surfaces::tests::angular_basis_canonicalizes_a_full_sweep_with_decimal_roundoff`.

**Cost.** No loss is charged when normalization succeeds. A value outside the slack follows the owning entity's normal domain or projection rule.

**Reopens.** Reopen if an IGES application protocol defines angular equality or a producer witness requires a different normalization bound. The executable check is the two named angular tests.

## D-12. Malformed product-definition roots

**Question.** May root inference create an occurrence when a Type 308 or Type 320 member list is malformed?

**Silence.** IGES defines counted member lists but does not define safe root inference when a member may be an unrecognized instance.

**Rule.** Block document root inference when any definition member list has an invalid count or pointer span. Retain typed definitions and instances, emit `malformed_definition`, and emit no inferred occurrence from the blocked root.

**Ground.** IGES 5.3 §§4.73 and 4.78; `native.rs::product_occurrence_expansion` and `reader.rs`; tests `structure::tests::decode_does_not_infer_roots_from_malformed_definition_members` and `structure::tests::decode_does_not_infer_roots_from_malformed_network_definition_members`.

**Cost.** The occurrence report records `malformed_definition` and the decode report charges `occurrence.root-inference-blocked`. Native definition and instance records remain.

**Reopens.** Reopen if a source rule identifies a safe root independent of the malformed member or requires partial root inference. The executable check is the two named structure tests.

## D-13. Product-occurrence admission

**Question.** Which structure records may create semantic product occurrences?

**Silence.** IGES defines Type 308/320 definitions and Type 408/420 instances, but does not define a neutral occurrence admission boundary when structure validation rejects one record.

**Rule.** Expand only definitions and instances admitted by the structure validator. Keep rejected native records; continue valid roots; preserve complete ordered instance paths and report every local truncation or invalid-structure omission.

**Ground.** IGES 5.3 §§4.73, 4.78, 4.74, and 4.79; `native.rs::product_occurrences` and `reader.rs`; tests `structure::tests::decode_omits_occurrences_for_invalid_top_level_subfigures`, `structure::tests::decode_keeps_valid_occurrences_beside_invalid_top_level_subfigures`, and the output/depth limit tests.

**Cost.** Rejected structure charges `occurrence.invalid-structure`; output and depth limits charge their named occurrence losses. No occurrence is synthesized from an invalid record.

**Reopens.** Reopen if a validated product-profile rule permits a rejected native record to produce an occurrence or changes transform order. The executable check is the named occurrence test set.

## D-14. Type 102 invalid counted primary data

**Question.** May a pointer-shaped suffix be guessed when Type 102's declared count cannot establish its primary span?

**Silence.** IGES defines the count and list but does not define recovery from malformed count/list data.

**Rule.** Keep raw parameters and the entity loss. Do not reinterpret later pointer-shaped tokens as associativity or property groups.

**Ground.** IGES 5.3 §4.4; `parameter.rs::specified_parameter_end`; tests `parameter::tests::type102_invalid_count_suppresses_generic_suffix_candidate` and `parameter::tests::type102_wrong_typed_constituent_keeps_count_boundary`.

**Cost.** The Type 102 entity retains native data and charges its existing entity loss; no false graph links are created.

**Reopens.** Reopen if an entity-table rule defines a recoverable malformed count boundary. The executable check is the two named parameter tests.

## D-15. Type 106 malformed primary data

**Question.** May malformed IP, N, or tuple spans enable generic trailing-group recovery?

**Silence.** IGES defines form-specific tuple widths and cardinality constraints but does not define malformed-record recovery.

**Rule.** Retain raw Type 106 parameters and the entity loss. Suppress generic pointer-shaped suffix recovery. A form-defined width controls the boundary only when the required primary span is complete.

**Ground.** IGES 5.3 §§4.6–4.11; `parameter.rs::specified_parameter_end`; tests `parameter::tests::type106_form_ip_mismatch_suppresses_generic_suffix_candidate` and `parameter::tests::type106_nonpositive_count_suppresses_generic_suffix_candidate`.

**Cost.** The native Type 106 record remains and charges `entity.not-projected`; no guessed association or property links are emitted.

**Reopens.** Reopen if an application protocol defines a recoverable default for malformed IP or N. The executable check is the named parameter tests.

## D-16. Type 106 Form 63 projection

**Question.** Which Type 106 Form 63 records may become neutral simple closed curves?

**Silence.** IGES defines Form 63's simple-closed-planar semantics but does not define the neutral projection gate or work-exhaustion result.

**Rule.** Require IP=1, planar XY data, one repeated closure endpoint, and the simple-closed intersection constraints. Apply the Global-resolution predicate to closure. Retain native data when admission or bounded work fails.

**Ground.** IGES 5.3 §§4.11 and 4.68; NIST IGES application-protocol guidance; `entities/copious.rs::project_form_63`; tests `parameter::tests::type106_form63_rejects_nonplanar_ip_before_suffix_recovery` and `copious::tests::decode_closes_form_63_with_the_global_minimum_resolution`.

**Cost.** A failed projection charges `entity.not-projected`; no partially checked curve is emitted.

**Reopens.** Reopen if the application protocol permits another IP or intersection rule, or if a producer witness demonstrates a different closure metric. The executable check is the named Form 63 tests.

## D-17. Type 402 group admission

**Question.** What is retained when a Type 402 group count or member list is malformed?

**Silence.** IGES defines the counted member list and reverse-association requirements but does not define malformed-list recovery.

**Rule.** A nonnegative count establishes the arithmetic boundary, including zero. A malformed or truncated count/list does not establish a boundary. Retain raw parameters and suppress generic suffix recovery; resolved members project only when the form's ownership rules hold.

**Ground.** IGES 5.3 §§4.81, 4.85, 4.89, and 4.90; `parameter.rs::specified_parameter_end` and `entities/structure.rs`; tests `parameter::tests::type402_group_forms_share_count_driven_boundary`, `parameter::tests::type402_negative_count_suppresses_generic_suffix_candidate`, and `parameter::tests::type402_zero_count_keeps_the_count_defined_boundary`.

**Cost.** Malformed group data charges the entity projection loss; unresolved members retain reference-graph findings. No false group links are emitted.

**Reopens.** Reopen if a form table or producer witness establishes a different zero-count or malformed-list rule. The executable check is the named Type 402 parameter tests.

## D-18. Type 308 malformed member lists

**Question.** May a Type 308 member list with a malformed count or span be used for suffix recovery?

**Silence.** IGES defines the Type 308 count and ordered members but not receiver recovery.

**Rule.** A valid nonnegative count fixes the boundary, including zero. A missing, negative, or truncated count/list retains raw parameters, emits the Type 308 entity loss, and suppresses generic suffix recovery.

**Ground.** IGES 5.3 §4.73; `parameter.rs::specified_parameter_end`; tests `parameter::tests::type308_negative_count_suppresses_generic_suffix_candidate`, `parameter::tests::type308_zero_count_keeps_the_count_defined_boundary`, and `parameter::tests::type308_wrong_typed_member_keeps_count_boundary`.

**Cost.** The native definition remains; malformed primary data charges its entity loss and cannot create a product occurrence under D-12.

**Reopens.** Reopen if an application protocol supplies a safe recovery boundary for malformed Type 308 data. The executable check is the named Type 308 parameter tests.

## D-19. Type 504 malformed edge lists

**Question.** May a Type 504 edge list with a malformed tuple span be recovered as a suffix-bearing entity?

**Silence.** IGES defines five fields per edge tuple but not malformed-list recovery.

**Rule.** A complete positive count and tuple span fix the boundary even when a field is wrong-typed. A missing, nonpositive, wrong-typed, or truncated count/list retains raw parameters and suppresses generic suffix recovery.

**Ground.** IGES 5.3 §4.144; `parameter.rs::specified_parameter_end`; tests `parameter::tests::type504_count_driven_boundary_follows_one_and_two_edge_tuples`, `parameter::tests::type504_wrong_typed_tuple_field_keeps_count_boundary`, and `parameter::tests::type504_invalid_count_or_truncated_tuple_suppresses_generic_suffix_candidate`.

**Cost.** Invalid edge data retains native Type 504 data and charges the entity or reference loss supplied by the existing projection path.

**Reopens.** Reopen if the Type 504 table or independent producer evidence defines a different tuple recovery boundary. The executable check is the named Type 504 parameter tests.

## D-20. Unregistered trailing-group arbitration

**Question.** How is a suffix handled before an entity/form boundary is registered?

**Silence.** IGES says the entity table supplies `NV`, but the codec has no table entry for an unsupported pair.

**Rule.** Type a suffix only when exactly one complete candidate has nonnegative counts and all pointers resolve to allowed target classes. Preserve raw tokens and charge `parameter.ambiguous-trailing-pointer-groups` when multiple candidates are complete; never use pointer shape alone to select a boundary.

**Ground.** IGES 5.3 §2.2.4.5.1–2; `parameter.rs::ambiguous_trailing_pointer_group_count_with_records`; tests `parameter::tests::unknown_entity_with_ambiguous_suffix_is_not_guessed` and `parameter::tests::entity_table_boundary_beats_pointer_shaped_line_coordinates`.

**Cost.** Ambiguity remains in the native record and charges `parameter.ambiguous-trailing-pointer-groups`; unresolved chosen pointers retain graph findings.

**Reopens.** Reopen when every supported entity/form has a registered table boundary or a source rule defines a deterministic fallback for an unregistered pair. The executable check is the two named parameter tests.

## D-21. Type 402 Forms 3 and 4 empty display lists

**Question.** Does a Type 402 Form 3 or 4 record need an explicit displayed-entity count when the list is empty?

**Silence.** IGES defines the two counts but does not define omission of the second count as an empty list.

**Rule.** Require the `N2` token. Explicit `N2=0` is the empty displayed-entity class and fixes the boundary. Missing, wrong-typed, negative, or truncated counts do not establish a boundary.

**Ground.** IGES 5.3 §§4.82–4.83; `parameter.rs::specified_parameter_end` and `entities/drawing.rs`; tests `parameter::tests::type402_forms34_count_driven_boundary_follows_view_and_entity_lists`, `parameter::tests::type402_forms34_wrong_complete_field_keeps_count_boundary`, and `parameter::tests::type402_forms34_invalid_count_or_truncated_list_suppresses_generic_suffix_candidate`.

**Cost.** A malformed display list retains native parameters and charges the view-visibility projection loss. A wrong complete pointer keeps the boundary and its pointer loss.

**Reopens.** Reopen if the official table or producer evidence permits an omitted `N2` default. The executable check is the named Forms 3/4 parameter tests.

## D-22. Neutral parameter-domain fallback

**Question.** What happens when a native curve parameterization has no exact neutral IR range mapping?

**Silence.** IGES defines native parameter domains but does not define a receiver fallback for an IR carrier that cannot represent one exactly.

**Rule.** Preserve the native range. Use only exact equivalent neutral ranges for supported primitive, spline, path, NURBS, and composite carriers. Keep unsupported ray or unmappable base parameterizations native and emit `entity.not-projected`; do not invent a finite fallback range.

**Ground.** IGES 5.3 §§4.3–4.5, 4.7, 4.13–4.14, 4.23, and 4.25; `entities/geometry.rs`, `entities/composite.rs`, and `entities/trimming.rs`; tests `geometry::tests::decode_preserves_semi_bounded_and_unbounded_line_domains_natively`, `composite::tests::concatenated_range_is_exactly_the_canonical_knot_domain`, and `trimming::tests::decode_preserves_parameter_domain_as_implicit_outer_boundary`.

**Cost.** Unmapped carriers remain in native arenas and charge `entity.not-projected`; no fabricated edge range is emitted.

**Reopens.** Reopen if a neutral IR range is added or a published application protocol defines an exact mapping for a currently native-only domain. The executable check is the named domain tests.

## D-23. Type 126 property flags

**Question.** How does the decoder handle Type 126 property flags that contradict the serialized geometry?

**Silence.** IGES defines the flags as consistency claims but does not define a neutral receiver policy for contradictory claims or degenerate containing planes.

**Rule.** Validate `PROP1` through `PROP3` against transformed control points, Global resolution, and declared real precision. Treat a degenerate carrier as a containing-plane case and still validate its declared normal. Retain the native Type 126 record and refuse neutral projection on a contradiction. `PROP4` remains informational.

**Ground.** IGES 5.3 §§2.2.4.3.9–11, 2.2.4.3.19, 4.23, and Appendix B §B.4; `entities/splines.rs`; tests `geometry::tests::decode_accepts_a_declared_plane_for_a_degenerate_curve`, `geometry::tests::decode_applies_declared_real_significance_to_polynomial_weights`, and `geometry::tests::decode_clamps_bspline_parameter_range_within_declared_real_significance`.

**Cost.** A contradictory flag charges `entity.not-projected`; source weights, knots, flags, and tokens remain native.

**Reopens.** Reopen if an application protocol changes flag precedence, defines a different tolerance, or permits projection with contradictory claims. The executable check is the named Type 126 tests.

## D-24. Type 112 continuity claims

**Question.** Which Type 112 continuity claims use Global resolution, and which receive a derivative tolerance?

**Silence.** IGES defines positional, slope, and curvature continuity but does not define a receiver tolerance for the derivative claims.

**Rule.** Apply strict transformed Euclidean Global-resolution comparison only to positional joins. Retain `H` as native source data and do not invent slope or curvature tolerances or reject a coefficient carrier for those claims.

**Ground.** IGES 5.3 §4.14 and NIST IR 4600 RP253; `entities/splines.rs::project`; test `splines::tests::decode_uses_global_euclidean_resolution_for_parametric_spline_segments`.

**Cost.** A positional failure retains native Type 112 data and charges the existing spline projection loss. No derivative loss is invented.

**Reopens.** Reopen if NIST or IGES publishes a derivative tolerance or a producer witness requires derivative admission. The executable check is the named spline continuity test.

## D-25. Omitted Global sender-product field

**Question.** What does the reader do when a producer omits Global field 3 even though the IGES field is required?

**Silence.** IGES 5.3 marks field 3 as required-no-default; it does not define a receiver extension for producer files that omit the value.

**Rule.** Accept the omitted field during reader inspection and semantic decode, retain the sender-product value as NULL, and do not invent a default. Semantic writing always emits field 3.

**Ground.** IGES 5.3 §§2.2.3 and 2.2.4.3.3; the public Autodesk Inventor sample [Cube 10x10 IGES sample](https://raw.githubusercontent.com/kovacsv/occt-import-js/main/test/testfiles/cube-10x10mm/Cube%2010x10.igs), SHA-256 `8bcbc86a044f592ba2a1a18f89a905358a85aa5093c0dd0756771b1b022b4c6f`; `global.rs::Global::validate`; test `global::tests::omitted_sender_product_is_retained_as_null_for_reader_compatibility`; rebuilt service-profile `cadmpeg inspect`, `dump`, and `check` runs.

**Cost.** No loss is charged; the native source metadata retains a NULL sender-product value. A semantic writer supplies its own sender identity as required by its output profile.

**Reopens.** Reopen if an authoritative producer rule requires a different field-3 recovery or if the reader extension creates an ambiguity with a valid sender-product value. The executable check is the named Global test plus a fresh service-profile decode of the cited public witness.

## D-26. Type 216 fixed parameter boundary

**Question.** How does the reader separate Type 216 Forms 0 through 2 from trailing pointer groups when a primary witness field is malformed or incomplete?

**Silence.** IGES 5.3 defines the five Type 216 fields and the two nullable witness values, but it does not define malformed-record suffix recovery.

**Rule.** Register one fixed boundary at token index 6 for Forms 0 through 2. A complete five-slot primary span owns the suffix even when a primary field is wrong-typed, omitted, or points to the wrong entity class. If the record does not reach token index 6, retain the primary tokens and suppress generic pointer-shaped suffix recovery; pointer-shaped tokens before the registered boundary are never reinterpreted as a suffix.

**Ground.** IGES 5.3 §4.63; public Open CASCADE [`IGESDimen_ToolLinearDimension::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESDimen/IGESDimen_ToolLinearDimension.cxx), which read and write exactly five references and pass `Standard_True` only for the two witness references; public [`IGESData_ParamReader::ReadEntity`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESData/IGESData_ParamReader.cxx), which accepts zero only for a nullable reference; `parameter.rs::specified_parameter_end`; and tests `parameter::tests::type216_forms_share_fixed_boundary`, `parameter::tests::type216_complete_wrong_witness_keeps_fixed_boundary`, and `parameter::tests::type216_truncated_primary_suppresses_generic_suffix_candidate`. The controlled witnesses are `/home/pcurve/side2/tmp/freecad-l9/ph03-type216/ph03-type216-form0-valid.igs` (`be36bf65ecd0fd9533b1363bb310f65ff2583a9ab1a549c3efbf39ad817a4c54`), `ph03-type216-form1-valid.igs` (`ccf001fb57a04ef54a466038345684886035393e6485ebe8c90f952ff417a049`), `ph03-type216-form2-valid.igs` (`af19a47692fef79247d3133678f99c66415c1e5beac052a6630ab5e1e78ebd13`), `ph03-type216-wrong-witness-type.igs` (`525396fcdb4614d48854cf898dbde3dd1722f815a9d87ec29875dad582cc9231`), `ph03-type216-wrong-witness-pointer.igs` (`28ba27e1316333f01b7501e06f13ad262cc5abc67f3cc41a7d2e25c8aefd65fb`), `ph03-type216-truncated-primary.igs` (`6f8658167561e46fd10568ff53ec758192b45f83dd3cac9bef77198f0c18d4cc`), and `ph03-type216-omitted-witness-slot.igs` (`93290c5983425fce478304d16c8c484e8e6a688b1dca1dc93ea537e8464bdded`). Rebuilt service-profile `inspect`, `dump`, and `check` runs pass for all seven witnesses with zero check findings. Before registration the truncated witness falsely recovered a Type 212 association at parameter index 6; after registration it has no association link, while all complete witnesses retain the suffix at parameter index 7.

**Cost.** A malformed complete span retains the native Type 216 record and its primary reference findings. A truncated span cannot manufacture a trailing association or property group.

**Reopens.** Reopen if the official Type 216 table or an independent producer defines a different fixed span or nullable-slot encoding. The executable check is the named Type 216 parameter suite and the seven witness runs.
