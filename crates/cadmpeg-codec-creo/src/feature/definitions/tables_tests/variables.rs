// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn positional_variable_row_capacity_refuses_before_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        positional_variable_rows_with_limits(u64::MAX, u64::MAX)
            .expect("two rows admitted")
            .rows
            .len(),
        2
    );
    let error = positional_variable_rows_with_limits(
        crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo variable rows"),
            |cap| positional_variable_rows_with_limits(cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("two slots need two items");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo variable rows"));
}

#[test]
fn positional_variable_value_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = positional_variable_rows_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo variable value body"),
            |cap| positional_variable_rows_with_limits(u64::MAX, cap),
        ),
    )
    .expect_err("value needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable value body"));
}

#[test]
fn positional_variable_guess_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = positional_variable_rows_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo variable guess body"),
            |cap| positional_variable_rows_with_limits(u64::MAX, cap),
        ),
    )
    .expect_err("guess needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable guess body"));
}

#[test]
fn positional_variable_table_joins_coordinate_rows() {
    let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
            \x02\x07\x18\x18\x01\x00\x0a";
    let cache = scalar::ScalarCache::from_section(payload);

    let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
        .expect("positional var_arr");

    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert_eq!(variables.rows.len(), 2);
    assert!(variables.is_complete());
    assert_eq!(variables.rows[0].value_body, [0x18]);
    assert_eq!(variables.rows[0].guess_body, [0x18]);
    assert_eq!(
        variables.rows[0].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(0));
    assert_eq!(variables.rows[0].uvar_id, Some(9));
    assert_eq!(
        variables.rows[1].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[1].known, Some(1));
    assert_eq!(variables.rows[1].homogeneity, Some(0));
    assert_eq!(variables.rows[1].uvar_id, Some(10));
    assert_eq!(variables.points().len(), 1);
    assert_eq!(variables.points()[0].point_id, 7);
    assert_eq!(variables.points()[0].u, Some(0.0));
    assert_eq!(variables.points()[0].v, Some(0.0));
}

#[test]
fn positional_variable_table_rejects_duplicate_table_headers() {
    let payload = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2
            \x02\x07\x18\x18\x01\x00\x0a
            \xf8\x02\xf7\x77\xfb\xe2\xf7\x78
            \x01\x08\x18\x18\x01\x00\x0b\xf1\xf7\x77\xe2
            \x02\x08\x18\x18\x01\x00\x0c";
    let cache = scalar::ScalarCache::from_section(payload);

    assert!(positional_variable_table(payload, 0, payload.len(), 119, &cache).is_none());
}

#[test]
fn positional_variable_guess_zero_preserves_compact_trailing_fields_at_table_boundary() {
    let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x07\x00\x18\x18\x01\x01\x0f\xf1\xf7\x77\xe2\
            \x07\x01\x18\x18\x00\x01\x07\xf2next_table\0";
    let cache = scalar::ScalarCache::from_section(payload);

    let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
        .expect("positional var_arr");

    assert!(variables.is_complete());
    assert_eq!(variables.rows.len(), 2);
    assert_eq!(
        variables.rows[0].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(1));
    assert_eq!(variables.rows[0].uvar_id, Some(15));
    assert_eq!(
        variables.rows[1].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[1].known, Some(0));
    assert_eq!(variables.rows[1].homogeneity, Some(1));
    assert_eq!(variables.rows[1].uvar_id, Some(7));
}

#[test]
fn variable_tables_retain_extents_without_decoded_rows() {
    let named = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2";
    let cache = scalar::ScalarCache::from_section(named);
    let variables = variable_table(named, 0, named.len(), &cache).expect("named var_arr header");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert!(variables.rows.is_empty());
    assert!(variables.points().is_empty());
    assert!(!variables.is_complete());

    let positional = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78";
    let cache = scalar::ScalarCache::from_section(positional);
    let variables = positional_variable_table(positional, 0, positional.len(), 119, &cache)
        .expect("positional var_arr header");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert!(variables.rows.is_empty());
    assert!(variables.points().is_empty());
    assert!(!variables.is_complete());
}

#[test]
fn variable_table_withholds_duplicate_coordinate_identities() {
    let row = |variable_type, value, offset| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::from(variable_type),
        key: 7,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset,
    };
    let table = FeatureVariableTable {
        declared_count: 3,
        entity_ref: Some(119),
        rows: vec![row(1, 2.0, 10), row(1, 2.0, 20), row(2, 3.0, 30)],
        offset: 5,
    };

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.points().len(), 1);
    assert_eq!(table.points()[0].point_id, 7);
    assert_eq!(table.points()[0].u, None);
    assert_eq!(table.points()[0].v, Some(3.0));
}

#[test]
fn radius_variables_do_not_create_section_points() {
    let row = |variable_type, key, value, offset| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::from(variable_type),
        key,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset,
    };
    let table = FeatureVariableTable {
        declared_count: 3,
        entity_ref: Some(119),
        rows: vec![row(1, 7, 2.0, 10), row(2, 7, 3.0, 20), row(3, 99, 4.0, 30)],
        offset: 5,
    };

    assert_eq!(table.points().len(), 1);
    assert_eq!(table.points()[0].point_id, 7);
    let (points, ambiguous) = reconciled_points(&table);
    assert_eq!(points.get(&7), Some(&[Some(2.0), Some(3.0)]));
    assert!(!points.contains_key(&99));
    assert!(ambiguous.is_empty());
}

#[test]
fn reconciled_points_refuses_before_point_id_node() {
    let table = with_points(
        FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![FeatureSectionPoint {
            point_id: 7,
            u: Some(2.0),
            v: Some(3.0),
        }],
    );
    assert!(
        matches!(crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo reconciled point ID nodes", |ctx| table.reconciled_points(ctx)),
CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo reconciled point ID nodes")
    );
    assert_eq!(
        reconciled_points(&table).0.get(&7),
        Some(&[Some(2.0), Some(3.0)])
    );
}

#[test]
fn variable_coordinate_7e_and_c6_are_the_f3_dict_sign_pair() {
    let positive = [0x7e, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let negative = [0xc6, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let cache = scalar::ScalarCache::from_section(&positive);

    assert_eq!(
        decode_variable_scalar(&positive, 0, positive.len(), &cache),
        (
            ScalarLane::Value(f64::from_be_bytes([
                0x3f, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7
        )
    );
    assert_eq!(
        decode_variable_scalar(&negative, 0, negative.len(), &cache),
        (
            ScalarLane::Value(f64::from_be_bytes([
                0xbf, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7
        )
    );
}
