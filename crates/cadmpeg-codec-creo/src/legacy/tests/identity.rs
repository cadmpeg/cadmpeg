// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn unknown_declaration_codes_retain_scope_identity() {
    let data = b"@future 1 8\n@future 1 12\n0 1 value\n@next 2 255\n0 2 value\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(persistence.counts.conflicting_declarations, 1);
    assert_eq!(persistence.counts.unresolved_values, 1);
    let scope = scope_fixture(data, 0..data.len());
    assert_eq!(scope.values.len(), 1);
    assert_eq!(scope.values[0].attribute_id, 2);
    assert!(matches!(
        scope.declarations[1].type_code,
        LegacyTypeCode::Other(_)
    ));
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| parse_declaration(ctx, b"@future 1 256"))
            .expect("service profile admits scalar parsing")
            .is_none()
    );
}

#[test]
fn scan_resolves_declarations_values_and_continuations() {
    let data = b"#P_OBJECT 6\n@root 1 0\n0 1 ->\n@matrix 2 2\n1 2 [2][2]\n\
                     $3FF,0\n$0,3FF\n#END_OF_UGC\n";

    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let scope = scope_fixture(data, 0..data.len());

    assert_eq!(scope.declarations.len(), 2);
    assert_eq!(scope.declarations[1].name, "matrix");
    assert_eq!(scope.declarations[1].type_code, LegacyTypeCode::Real);
    assert_eq!(scope.values.len(), 2);
    assert_eq!(&data[scope.values[0].payload.clone()], b"->");
    assert_eq!(&data[scope.values[1].payload.clone()], b"[2][2]");
    let continuation = scope.values[1]
        .continuation
        .as_ref()
        .expect("continuations");
    assert_eq!(continuation.count.get(), 2);
    assert_eq!(&data[continuation.rows.clone()], b"$3FF,0\n$0,3FF");
    assert_eq!(persistence.counts.unresolved_values, 0);
    assert_eq!(persistence.counts.conflicting_declarations, 0);
}

#[test]
fn scan_resolves_identifiers_within_independent_scopes() {
    let data = b"@field 7 1\n1 7 4\n@other 7 2\n2 7 5\n";
    let second = data
        .windows(b"@other".len())
        .position(|window| window == b"@other")
        .expect("second scope");

    let persistence = scan(data, [0..second, second..data.len()])
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.counts.scopes, 2);
    assert_eq!(persistence.counts.declarations, 2);
    assert_eq!(persistence.counts.values, 2);
    assert_eq!(persistence.counts.conflicting_declarations, 0);
    assert_eq!(persistence.real_values.rows.len(), 1);
    assert_eq!(persistence.real_values.rows[0].scope_offset, second);
}

#[test]
fn model_name_prefers_root_solid_over_null_view_placeholder() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 ROOT\n\
@View 3 0\n@model_name 4 10\n0 3 ->\n1 4 NULL\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let expected_offset = data
        .windows(b"1 2 ROOT".len())
        .position(|window| window == b"1 2 ROOT")
        .expect("model name value");

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| persistence.model_name(ctx))
            .expect("name resolution fits service limits"),
        Some(("ROOT".to_string(), expected_offset))
    );
}

#[test]
fn legacy_model_name_refuses_before_retained_copy() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 ROOT\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo legacy model name"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            persistence.model_name(&trial_ctx)
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = persistence
        .model_name(&ctx)
        .expect_err("four name bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo legacy model name")
    );
}

#[test]
fn legacy_model_name_refuses_before_object_index_node() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 ROOT\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = persistence
        .model_name(&ctx)
        .expect_err("one object requires one index node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo legacy model name object nodes")
    );
}

#[test]
fn model_name_withholds_conflicting_root_identities() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 FIRST\n\
@Solid 3 0\n@model_name 4 10\n0 3 ->\n1 4 SECOND\n";
    let second_scope = data
        .windows(b"@Solid 3".len())
        .position(|window| window == b"@Solid 3")
        .expect("second scope");
    let persistence = scan(data, [0..second_scope, second_scope..data.len()])
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| persistence.model_name(ctx))
            .expect("conflicting names need no allocation"),
        None
    );
}

#[test]
fn first_source_model_name_selects_root_row_for_scoped_sections() {
    let data = b"@model_name 1 10\n0 1 ROOT\n\
@model_name 2 10\n0 2 DEPENDENT\n";
    let second_scope = data
        .windows(b"@model_name 2".len())
        .position(|window| window == b"@model_name 2")
        .expect("second scope");
    let persistence = scan(data, [0..second_scope, second_scope..data.len()])
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| persistence.first_source_model_name(ctx))
            .expect("source name fits service limits"),
        Some((
            "ROOT".to_string(),
            data.windows(b"0 1 ROOT".len())
                .position(|window| window == b"0 1 ROOT")
                .expect("root value")
        ))
    );
}

#[test]
fn legacy_first_source_model_name_refuses_before_retained_copy() {
    let data = b"@model_name 1 10\n0 1 ROOT\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = persistence
        .first_source_model_name(&ctx)
        .expect_err("four source-name bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo legacy first source model name")
    );
}

#[test]
fn principal_unit_requires_one_complete_known_type_10_scalar() {
    let millimeter = b"@principal_sys_units 25 10\n2 25 millimeter Newton Second (mmNs)\n";
    let persistence = scan(millimeter, std::iter::once(0..millimeter.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(
        principal_unit_system(&persistence),
        Some(PrincipalUnitSystem::MillimeterNewtonSecond)
    );
    assert_eq!(
        principal_unit_system(&persistence)
            .and_then(PrincipalUnitSystem::length_scale_mm)
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(1.0)
    );

    let inch = b"@principal_sys_units 25 10\n2 25 Inch lbm Second (Pro/E Default)\n";
    let persistence = scan(inch, std::iter::once(0..inch.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(
        principal_unit_system(&persistence),
        Some(PrincipalUnitSystem::InchPoundMassSecond)
    );
    assert_eq!(
        principal_unit_system(&persistence)
            .and_then(PrincipalUnitSystem::length_scale_mm)
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(25.4)
    );

    let mut repeated = millimeter.to_vec();
    repeated.extend_from_slice(millimeter);
    let persistence = scan(&repeated, std::iter::once(0..repeated.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(principal_unit_system(&persistence), None);
}

#[test]
fn legacy_unit_array_supplies_length_scale_when_principal_scalar_is_absent() {
    let factor = 0.393_700_787_401_574_8_f64;
    let data = format!(
        r"@Solid 1 0
@unit_arr 2 0
@type 3 1
@unit_type 4 1
@factor 5 2
@name 6 10
0 1 ->
1 2 [1]
2 2 ->
3 3 11
3 4 0
3 5 {factor_bits:016X}
3 6 CM
",
        factor_bits = factor.to_bits()
    );
    let persistence = scan(data.as_bytes(), std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(
        principal_unit_system(&persistence)
            .and_then(PrincipalUnitSystem::length_scale_mm)
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(10.0)
    );
}

#[test]
fn legacy_unit_array_refuses_before_element_identity_node() {
    let factor = 0.393_700_787_401_574_8_f64;
    let data = format!(
        "@Solid 1 0\n@unit_arr 2 0\n@type 3 1\n@unit_type 4 1\n@factor 5 2\n@name 6 10\n0 1 ->\n1 2 [1]\n2 2 ->\n3 3 11\n3 4 0\n3 5 {factor_bits:016X}\n3 6 CM\n",
        factor_bits = factor.to_bits()
    );
    let persistence = scan(data.as_bytes(), std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = persistence
        .principal_unit_system(&ctx)
        .expect_err("one array element needs one identity node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo legacy unit array element identities")
    );
}

#[test]
fn legacy_unit_array_conflict_withholds_length_scale() {
    let factor = 0.393_700_787_401_574_8_f64;
    let data = format!(
        r"@Solid 1 0
@unit_arr 2 0
@type 3 1
@unit_type 4 1
@factor 5 2
@name 6 10
0 1 ->
1 2 [1]
2 2 ->
3 3 11
3 4 0
3 5 {factor_bits:016X}
3 5 {other_factor_bits:016X}
3 6 CM
",
        factor_bits = factor.to_bits(),
        other_factor_bits = (factor * 2.0).to_bits()
    );
    let persistence = scan(data.as_bytes(), std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(principal_unit_system(&persistence), None);
}

