use super::super::assembly_operand_paths::POSITION_PATH_WRAPPER_TYPE_GUID;
use super::*;
use crate::layout::{captured_position_v4_path_wrapper as wrapper, indexed_design_record_header};
use crate::test_support::position::{append_locator, fixture, OCCURRENCE_KEY};

fn read(
    bytes: &[u8],
    scope: &DesignParameterScope,
    types: &StreamTypes<'_>,
) -> Option<DesignCapturedPositions> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    crate::test_support::with_decode_context(|ctx| {
        decode(ctx, bytes, &records, scope, types).unwrap()
    })
}

#[test]
fn typed_localized_position_keeps_order_and_omitted_matrix_without_locators() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, -3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (bytes, scope, types) = fixture(&[Some(matrix), None]);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let admitted = super::super::parameter_scope::parse_parameter_scope_with_type_guid(
        &bytes,
        &records,
        500,
        &"257".to_owned().try_into().unwrap(),
        0,
        Some(TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    assert_eq!(admitted.kind_name(), "Position");
    assert_eq!(admitted.source_kind_name(), "Posición");
    assert_eq!(scope.kind_name(), "Position");
    assert_eq!(scope.source_kind_name(), "Posición");
    let positions = read(&bytes, &scope, &types).unwrap();
    assert_eq!(
        positions
            .rows
            .iter()
            .map(|row| row.target.value)
            .collect::<Vec<_>>(),
        [700, 703]
    );
    assert_eq!(positions.rows[0].target.offset, 35);
    assert_eq!(positions.rows[0].matrix.as_ref().unwrap().offset, 46);
    assert_eq!(
        positions.rows[0]
            .matrix
            .as_ref()
            .unwrap()
            .value
            .map(FiniteReal::get),
        [0.0, -1.0, 0.0, 2.0, 1.0, 0.0, 0.0, -3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0]
    );
    assert!(positions.rows[1].matrix.is_none());
    assert!(positions.rows.iter().all(|row| row.path_index.is_none()));
    assert!(positions.paths.is_empty());
    let (bytes, scope, types) = fixture(&[]);
    assert!(read(&bytes, &scope, &types).unwrap().rows.is_empty());
}

#[test]
fn position_refuses_bad_framing_truncation_counts_and_nonfinite_coefficients() {
    let (bytes, scope, types) = fixture(&[
        Some(cadmpeg_ir::transform::Transform::identity().rows()),
        None,
    ]);
    for offset in [11, 29, 33, 34, 43, 45, 184, 188, 192, 264] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        assert!(read(&changed, &scope, &types).is_none(), "offset {offset}");
    }
    for end in 0..usize::try_from(scope.reference_count_offset()).unwrap() {
        assert!(read(&bytes[..end], &scope, &types).is_none(), "end {end}");
    }
    let mut changed = bytes.clone();
    changed[29..33].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(read(&changed, &scope, &types).is_none());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut changed = bytes.clone();
        changed[46..54].copy_from_slice(&value.to_le_bytes());
        assert!(read(&changed, &scope, &types).is_none());
    }
    let mut changed_types = types.clone();
    changed_types.insert(500, (TYPE_GUID, 7));
    assert!(read(&bytes, &scope, &changed_types).is_none());
}

#[test]
fn version_four_position_preserves_its_single_matrix_row() {
    let matrix = [
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, -3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (modern, modern_scope, mut types) = fixture(&[Some(matrix)]);
    let rows_end = usize::try_from(modern_scope.reference_count_offset()).unwrap() - suffix::LEN;
    let mut bytes = modern[..11].to_vec();
    bytes.resize(95, 0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&modern[prefix::LEN..rows_end]);
    bytes.extend_from_slice(&modern[rows_end + 4..]);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut scope = super::super::parameter_scope::parse_parameter_scope_with_type_guid(
        &bytes,
        &records,
        500,
        &"257".to_owned().try_into().unwrap(),
        0,
        Some(TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    assert_eq!(scope.reference_count_offset(), 319);
    assert!(read(&bytes, &scope, &types).is_none());
    types.insert(500, (TYPE_GUID, 4));
    let positions = read(&bytes, &scope, &types).unwrap();
    let [position] = positions.rows.as_slice() else {
        panic!("expected one captured row");
    };
    assert_eq!(position.target.value, 700);
    assert_eq!(position.target.offset, 101);
    let saved = position.matrix.as_ref().unwrap();
    assert_eq!(saved.offset, 112);
    assert_eq!(
        saved.value,
        std::array::from_fn(|index| FiniteReal::new(matrix[index / 4][index % 4]).unwrap())
    );
    for offset in [95, 100, 109, 111, 240, 316] {
        let mut malformed = bytes.clone();
        malformed[offset] ^= 1;
        assert!(read(&malformed, &scope, &types).is_none(), "{offset}");
    }
    let locator_start = bytes.len();
    append_locator(&mut bytes, &mut types, 500);
    let locator_end =
        locator_start + crate::layout::assembly_wide_operand_path_identity_locator::LEN;
    let remainder = bytes.split_off(locator_end);
    bytes.truncate(locator_start);
    crate::test_support::indexed_header(&mut bytes, *b"259", 700);
    bytes.extend_from_slice(&[0; 10]);
    crate::test_support::push_marked_reference(&mut bytes, 900);
    bytes.extend_from_slice(&[1, 0]);
    crate::test_support::push_marked_reference(&mut bytes, 500);
    crate::test_support::push_marked_reference(&mut bytes, 702);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&remainder);
    assert!(read(&bytes, &scope, &types).unwrap().paths.is_empty());
    types.insert(
        700,
        (
            super::super::assembly_operand_paths::WIDE_PATH_LOCATOR_TYPE_GUID,
            3,
        ),
    );
    let positions = read(&bytes, &scope, &types).unwrap();
    assert_eq!(positions.rows[0].path_index, Some(0));
    assert_eq!(
        positions.paths[0].occurrence_guids()[0].value.as_str(),
        OCCURRENCE_KEY
    );
    let wrapper_start = bytes.len() - indexed_design_record_header::LEN - wrapper::LEN;
    bytes[wrapper_start + wrapper::MARKER] = wrapper::MARKER_VALUE;
    let unregistered = read(&bytes, &scope, &types).unwrap();
    assert_eq!(unregistered.rows[0].path_index, None);
    assert_eq!(unregistered.rows[0].matrix, positions.rows[0].matrix);

    types.insert(702, (POSITION_PATH_WRAPPER_TYPE_GUID, 2));
    let other_version = read(&bytes, &scope, &types).unwrap();
    assert_eq!(other_version, unregistered);

    types.insert(702, (POSITION_PATH_WRAPPER_TYPE_GUID, 1));
    let legacy = read(&bytes, &scope, &types).unwrap();
    assert_eq!(legacy, positions);
    scope.captured_positions = Some(legacy);
    let decoded: DesignParameterScope =
        serde_json::from_value(crate::json_value(&scope).unwrap()).unwrap();
    assert_eq!(decoded.captured_positions, scope.captured_positions);
}

#[test]
fn version_six_position_keeps_matrix_when_wrapper_marker_is_zero() {
    let (mut bytes, scope, mut types) =
        fixture(&[Some(cadmpeg_ir::transform::Transform::identity().rows())]);
    append_locator(&mut bytes, &mut types, 500);
    types.insert(702, (POSITION_PATH_WRAPPER_TYPE_GUID, 1));
    let ordinary = read(&bytes, &scope, &types).unwrap();
    assert_eq!(ordinary.rows[0].path_index, Some(0));

    let wrapper_start = bytes.len() - indexed_design_record_header::LEN - wrapper::LEN;
    bytes[wrapper_start + wrapper::MARKER] = wrapper::MARKER_VALUE;
    let alternate = read(&bytes, &scope, &types).unwrap();
    assert_eq!(alternate.rows[0].path_index, None);
    assert_eq!(alternate.rows[0].target, ordinary.rows[0].target);
    assert_eq!(alternate.rows[0].matrix, ordinary.rows[0].matrix);
}

#[test]
fn position_retains_finite_nonrigid_and_nonaffine_coefficients() {
    let matrix = [
        [-2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 2.0],
    ];
    let (bytes, scope, types) = fixture(&[Some(matrix)]);
    let positions = read(&bytes, &scope, &types).unwrap();
    assert_eq!(
        positions.rows[0].matrix.as_ref().unwrap().value[0].get(),
        -2.0
    );
    assert_eq!(
        positions.rows[0].matrix.as_ref().unwrap().value[15].get(),
        2.0
    );
}

#[test]
fn position_joins_typed_locator_and_keeps_rows_when_the_backlink_is_wrong() {
    for backlink in [500, 501] {
        let (mut bytes, scope, mut types) = fixture(&[None]);
        append_locator(&mut bytes, &mut types, backlink);
        let positions = read(&bytes, &scope, &types).unwrap();
        assert_eq!(positions.rows[0].target.value, 700);
        if backlink == 500 {
            let path = &positions.paths[positions.rows[0].path_index.unwrap()];
            assert_eq!(path.occurrence_guids()[0].value.as_str(), OCCURRENCE_KEY);
            assert_eq!(path.link().locator_reference_offset, 35);
        } else {
            assert!(positions.rows[0].path_index.is_none());
        }
    }
}

#[test]
fn position_retains_repeated_row_targets() {
    let (mut bytes, scope, types) = fixture(&[None, None]);
    bytes[48..56].copy_from_slice(&700_u64.to_le_bytes());
    let positions = read(&bytes, &scope, &types).unwrap();
    assert_eq!(
        positions
            .rows
            .iter()
            .map(|row| row.target.value)
            .collect::<Vec<_>>(),
        [700, 700]
    );
    assert_eq!(positions.rows[1].target.offset, 48);
}

#[test]
fn position_shares_large_repeated_paths_on_wire_and_enforces_aggregate_budget() {
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    let (mut bytes, mut scope, mut types) = fixture(&vec![None; 1024]);
    for ordinal in 0..1024 {
        let at = 33 + ordinal * 13 + 2;
        bytes[at..at + 8].copy_from_slice(&700_u64.to_le_bytes());
    }
    let keys = [OCCURRENCE_KEY; 64];
    crate::test_support::position::append_locator_path(
        &mut bytes,
        &mut types,
        500,
        &keys,
        &[keys.as_slice(); 64],
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 8 * 1024 * 1024;
    let positions = crate::test_support::with_decode_policy(&policy, |ctx| {
        decode(ctx, &bytes, &records, &scope, &types)
            .unwrap()
            .unwrap()
    });
    assert_eq!(positions.paths.len(), 1);
    assert_eq!(positions.paths[0].occurrence_guids().len(), 64);
    assert_eq!(positions.paths[0].occurrence_lists().len(), 64);
    assert_eq!(positions.paths[0].occurrence_lists()[63].len(), 64);
    assert_eq!(positions.paths[0].link().locator_reference_offset, 35);
    for (ordinal, row) in positions.rows.iter().enumerate() {
        assert_eq!(row.path_index, Some(0));
        assert_eq!(row.target.offset, u64::try_from(35 + ordinal * 13).unwrap());
    }
    policy.limits.max_retained_bytes = 512 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(matches!(
            decode(ctx, &bytes, &records, &scope, &types),
            Err(CodecError::ResourceLimit(_))
        ));
        assert!(ctx.resource_refusal().is_some());
    });
    scope.captured_positions = Some(positions);
    let wire = crate::json_value(&scope).unwrap();
    assert_eq!(
        wire["captured_positions"]["paths"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        serde_json::from_value::<DesignParameterScope>(wire.clone()).unwrap(),
        scope
    );
    for index in [serde_json::json!(1), serde_json::Value::Null] {
        let mut invalid = wire.clone();
        invalid["captured_positions"]["rows"][1]["path_index"] = index;
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
    }
    let mut rewritten = wire.clone();
    let mut identities = IdentityMap::new(|id: &str| Ok(id.to_owned()));
    DesignParameterScope::rewrite_native_value(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut rewritten,
        &mut identities,
    )
    .unwrap();
    identities.finish().unwrap();
    assert_eq!(rewritten, wire);
}
