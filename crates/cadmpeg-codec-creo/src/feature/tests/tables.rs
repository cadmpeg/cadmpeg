// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::definitions::*;
use crate::psb;
use crate::scalar;

#[test]
fn positional_dimension_table_uses_the_inherited_table_class() {
    let mut payload = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 2);
    assert_eq!(dimensions.rows[0].value, Some(3.0));
    assert_eq!(
        dimensions.rows[0].value_body,
        [0x46, 0x08, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(dimensions.rows[0].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[0].external_id, 43);
    assert_eq!(dimensions.rows[1].dimension_type, 10);
    assert_eq!(
        dimensions.rows[1].value_body,
        [0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f]
    );
    assert_eq!(dimensions.rows[1].external_id, 44);
}

#[test]
fn named_dimension_retains_nested_dimension_references() {
    let payload = b"dimtab_ptr\0\xf3\xf8\x01\xf7\x58\xfb\xe2\
            \xe0\x01type\0\x02\xe0\x02value\0\x18\xe0\x01direct\0\x00\
            \xe0\x02aux_value\0\x18\xe0\x01ext_id\0\x02\
            dim_ref\0\xf1\xf8\x02\xf7\x60\xfb\xe2\
            \xe0\x01item_id\0\x0d\xe0\x01sense\0\x00\
            \xe0\x01point\0\xf8\x02\x03\xe4\
            \xf1\xf7\x60\xe2\x02\x02\x14\xe4\xf3\xf7\x58\xe2";
    let cache = scalar::ScalarCache::from_section(payload);

    let dimensions =
        dimension_table(payload, 0, payload.len(), &cache).expect("named dimension table");
    let references = dimensions.rows[0]
        .references
        .as_ref()
        .expect("nested dimension references");

    assert_eq!(references.declared_count, 2);
    assert_eq!(references.entity_ref, Some(0x60));
    assert_eq!(references.rows.len(), 2);
    assert_eq!(
        references.rows[0],
        FeatureDimensionReference {
            item_id: Some(13),
            sense: Some(0),
            point: [Some(3), Some(1)],
            offset: payload
                .windows(b"item_id\0".len())
                .position(|window| window == b"item_id\0")
                .expect("item_id offset"),
        }
    );
    assert_eq!(references.rows[1].item_id, Some(2));
    assert_eq!(references.rows[1].sense, Some(2));
    assert_eq!(references.rows[1].point, [Some(20), Some(1)]);
}

#[test]
fn positional_dimension_table_is_self_describing_when_multiple_rows_close() {
    let mut payload = b"prefix\xf8\x04\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    for (index, row) in [
        [1, 0xe4, 0, 0x18, 2],
        [2, 0x0e, 0, 0x18, 0],
        [2, 0xe4, 0, 0x18, 3],
        [2, 0xe4, 0, 0x18, 1],
    ]
    .into_iter()
    .enumerate()
    {
        payload.extend_from_slice(&row);
        if index < 3 {
            payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        }
    }
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = self_described_positional_dimension_table(&payload, 0, payload.len(), &cache)
        .expect("self-described dimension table");

    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 4);
    assert_eq!(dimensions.rows[0].external_id, 2);
    assert_eq!(dimensions.rows[1].value, Some(-0.5));
}

#[test]
fn one_row_positional_table_does_not_self_identify_as_dimensions() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\xe4\x00\x18\x02";
    assert_eq!(
        self_described_positional_dimension_table(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
        ),
        None
    );
}

#[test]
fn positional_dimension_table_retains_bounded_opaque_values() {
    let mut payload = b"prefix\xf8\x03\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[1, 0x00, 0x04, 0xa6, 0, 0x18, 44]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[5, 0x0d, 0, 0x18, 45]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.rows.len(), 3);
    assert_eq!(dimensions.rows[1].value, None);
    assert_eq!(
        dimensions.rows[1].unresolved_value_token.as_deref(),
        Some(&[0x00, 0x04, 0xa6][..])
    );
    assert_eq!(dimensions.rows[1].value_body, [0x00, 0x04, 0xa6]);
    assert_eq!(dimensions.rows[1].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[1].external_id, 44);
    assert_eq!(dimensions.rows[2].value, Some(-1.0));
    assert_eq!(dimensions.rows[2].external_id, 45);
}

#[test]
fn positional_dimensions_decode_the_positive_dict_lattice_and_bounded_opaque_forms() {
    let positive = [1, 0x53, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f, 0, 0x18, 46];
    let opaque_three = [1, 0x00, 0x04, 0xa6, 0, 0x18, 47];
    let opaque_four = [1, 0x01, 0x04, 0xfe, 0xf2, 0, 0x18, 48];
    let zero = [2, 0x18, 0, 0x18, 49];
    let negative_half = [1, 0x0e, 0, 0x18, 50];
    let cache = scalar::ScalarCache::default();

    let positive_row = positional_dimension(&positive, 0, positive.len(), &cache)
        .expect("positive dictionary dimension");
    assert_eq!(
        positive_row.value,
        Some(f64::from_be_bytes([
            0x3f, 0xc8, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f,
        ]))
    );
    assert_eq!(positive_row.direction_byte, 0);
    assert_eq!(positive_row.auxiliary_value, Some(0.0));
    assert_eq!(positive_row.value_body, positive[1..8]);
    assert_eq!(positive_row.auxiliary_body, [0x18]);
    assert_eq!(positive_row.external_id, 46);
    for (body, external_id, token) in [
        (&opaque_three[..], 47, &[0x00, 0x04, 0xa6][..]),
        (&opaque_four[..], 48, &[0x01, 0x04, 0xfe, 0xf2][..]),
    ] {
        let row =
            positional_dimension(body, 0, body.len(), &cache).expect("bounded opaque dimension");
        assert_eq!(row.value, None);
        assert_eq!(row.unresolved_value_token.as_deref(), Some(token));
        assert_eq!(row.external_id, external_id);
    }
    let zero_row = positional_dimension(&zero, 0, zero.len(), &cache).expect("zero dimension");
    assert_eq!(zero_row.value, Some(0.0));
    assert_eq!(zero_row.external_id, 49);
    let negative_half_row = positional_dimension(&negative_half, 0, negative_half.len(), &cache)
        .expect("negative half dimension");
    assert_eq!(negative_half_row.value, Some(-0.5));
    assert_eq!(negative_half_row.external_id, 50);
}

#[test]
fn positional_dimension_seven_byte_positive_value_preserves_field_alignment() {
    let body = [2, 0x31, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0, 0x18, 27];
    let row = positional_dimension(&body, 0, body.len(), &scalar::ScalarCache::default())
        .expect("seven-byte positive dimension");

    assert_eq!(
        row.value,
        Some(f64::from_be_bytes([
            0x40, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0,
        ]))
    );
    assert_eq!(row.direction_byte, 0);
    assert_eq!(row.auxiliary_value, Some(0.0));
    assert_eq!(row.external_id, 27);
}

#[test]
fn dimension_tables_retain_extents_without_decoded_rows() {
    let named = b"dimtab_ptr\0\xf8\x02\xf7\x58\xfb\xe2";
    let cache = scalar::ScalarCache::from_section(named);
    let dimensions = dimension_table(named, 0, named.len(), &cache).expect("named dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());

    let positional = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59";
    let cache = scalar::ScalarCache::from_section(positional);
    let dimensions = positional_dimension_table(positional, 0, positional.len(), 88, &cache)
        .expect("positional dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());
}

#[test]
fn positional_definition_inherits_the_labeled_dimension_table_class() {
    let mut payload = b"feat_defs_917\0dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0004\0\
            \xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = definitions(&payload);
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded[1].owner_feature_id, Some(42));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value, Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
}

#[test]
fn depdb_gsec2d_definition_anchors_positional_table_replay() {
    let mut payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
            dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe3S2D0003\0\xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = depdb_definitions(&payload);
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].id, 2);
    assert_eq!(decoded[1].id, 2);
    assert!(decoded
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value, Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
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
    assert_eq!(variables.rows[0].guess, Some(0.0));
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(0));
    assert_eq!(variables.rows[0].uvar_id, Some(9));
    assert_eq!(variables.rows[1].guess, Some(0.0));
    assert_eq!(variables.rows[1].known, Some(1));
    assert_eq!(variables.rows[1].homogeneity, Some(0));
    assert_eq!(variables.rows[1].uvar_id, Some(10));
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(0.0));
    assert_eq!(variables.points[0].v, Some(0.0));
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
    assert_eq!(variables.rows[0].guess, Some(0.0));
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(1));
    assert_eq!(variables.rows[0].uvar_id, Some(15));
    assert_eq!(variables.rows[1].guess, Some(0.0));
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
    assert!(variables.points.is_empty());
    assert!(!variables.is_complete());

    let positional = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78";
    let cache = scalar::ScalarCache::from_section(positional);
    let variables = positional_variable_table(positional, 0, positional.len(), 119, &cache)
        .expect("positional var_arr header");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert!(variables.rows.is_empty());
    assert!(variables.points.is_empty());
    assert!(!variables.is_complete());
}

#[test]
fn variable_table_withholds_duplicate_coordinate_identities() {
    let row = |variable_type, value, offset| FeatureVariableRow {
        variable_type,
        key: 7,
        value: Some(value),
        value_body: Vec::new(),
        guess: None,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: None,
        homogeneity: None,
        uvar_id: None,
        dimension_driven: false,
        offset,
    };
    let table = variable_table_from_rows(
        3,
        Some(119),
        vec![row(1, 2.0, 10), row(1, 2.0, 20), row(2, 3.0, 30)],
        5,
    );

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.points.len(), 1);
    assert_eq!(table.points[0].point_id, 7);
    assert_eq!(table.points[0].u, None);
    assert_eq!(table.points[0].v, Some(3.0));
}

#[test]
fn radius_variables_do_not_create_section_points() {
    let row = |variable_type, key, value, offset| FeatureVariableRow {
        variable_type,
        key,
        value: Some(value),
        value_body: Vec::new(),
        guess: None,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: None,
        homogeneity: None,
        uvar_id: None,
        dimension_driven: false,
        offset,
    };
    let table = variable_table_from_rows(
        3,
        Some(119),
        vec![row(1, 7, 2.0, 10), row(2, 7, 3.0, 20), row(3, 99, 4.0, 30)],
        5,
    );

    assert_eq!(table.points.len(), 1);
    assert_eq!(table.points[0].point_id, 7);
    let (points, ambiguous) = table.reconciled_points();
    assert_eq!(points.get(&7), Some(&[Some(2.0), Some(3.0)]));
    assert!(!points.contains_key(&99));
    assert!(ambiguous.is_empty());
}

#[test]
fn variable_coordinate_7e_and_c6_are_the_f3_dict_sign_pair() {
    let positive = [0x7e, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let negative = [0xc6, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let cache = scalar::ScalarCache::from_section(&positive);

    assert_eq!(
        decode_variable_scalar(&positive, 0, positive.len(), &cache),
        (
            Some(f64::from_be_bytes([
                0x3f, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7,
            false
        )
    );
    assert_eq!(
        decode_variable_scalar(&negative, 0, negative.len(), &cache),
        (
            Some(f64::from_be_bytes([
                0xbf, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7,
            false
        )
    );
}

#[test]
fn positional_gsec3d_decodes_placement_and_reference_rows() {
    let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
            \x07\x05\xf6\x04\xf6\x01";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(513));
    assert_eq!(section.sketch_plane_flip, None);
    assert_eq!(section.reference_plane_entity_ids, vec![6, 7]);
    assert_eq!(section.reference_plane_rows.len(), 2);
    assert_eq!(section.reference_plane_rows[0].plane_entity_id, 6);
    assert_eq!(section.reference_plane_rows[0].reference_type, Some(5));
    assert_eq!(section.reference_plane_rows[0].external_reference_id, None);
    assert_eq!(section.reference_plane_rows[0].segment_id, Some(3));
    assert_eq!(section.reference_plane_rows[0].sub_index, None);
    assert_eq!(
        section.reference_plane_rows[0].reference_flip,
        Some(BinaryFlag::Clear)
    );
    assert_eq!(section.reference_plane_rows[1].plane_entity_id, 7);
    assert_eq!(section.reference_plane_rows[1].reference_type, Some(5));
    assert_eq!(section.reference_plane_rows[1].external_reference_id, None);
    assert_eq!(section.reference_plane_rows[1].segment_id, Some(4));
    assert_eq!(section.reference_plane_rows[1].sub_index, None);
    assert_eq!(
        section.reference_plane_rows[1].reference_flip,
        Some(BinaryFlag::Set)
    );
    assert_eq!(section.reference_plane_datum_geometry_id, None);
    assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
    assert_eq!(section.orientation.reference_type, None);
    assert_eq!(section.orientation.segment_id, None);
    assert_eq!(section.orientation.reference_flip, None);
}

#[test]
fn positional_gsec3d_retains_its_header_without_a_body() {
    let payload = b"prefix\x07S2D0004\0";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.offset, 6);
    assert_eq!(section.sketch_plane_entity_id, None);
    assert!(section.reference_plane_entity_ids.is_empty());
    assert_eq!(section.orientation, FeatureSectionOrientation::default());
}

#[test]
fn positional_gsec3d_retains_placement_and_complete_reference_prefix() {
    let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\x07";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(513));
    assert_eq!(section.reference_plane_entity_ids, [6]);
    assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
    assert_eq!(section.orientation.reference_type, None);
    assert_eq!(section.orientation.segment_id, None);
    assert_eq!(section.orientation.reference_flip, None);
}

#[test]
fn named_gsec3d_uses_the_outer_plane_id_before_reference_rows() {
    let payload = b"\xe0\x00gsec3d_ptr\0\
            \xe0\x01plane_id\0\x2a\
            \xe0\x01plane_flip\0\xf6\
            \xe0\x00ref_planes\0\xf8\x01\xf7\x80\x8c\xfb\xe2\
            \xe0\x01plane_id\0\x06\
            \xe0\x01ref_type\0\x05\
            \xe0\x01ext_ref_id\0\xf6\
            \xe0\x01seg_id\0\x02\
            \xe0\x01sub_index\0\xf6\
            \xe0\x01flip_flag\0\x00\
            \xe0\x00p_saved_result\0";

    let definitions = definitions_in_ranges(&payload[..], &[(0, 1, None, false)]);
    let section = definitions[0].section_3d.as_ref().expect("named gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(42));
    assert_eq!(section.reference_plane_datum_geometry_id, Some(6));
    assert_eq!(section.sketch_plane_flip, None);
}

#[test]
fn equation_table_replays_direct_and_counted_rows() {
    let payload = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\
            \xe0\x05fcn_id\0\x02\
            \xe0\x08arg_arr\0\xf8\x02\x2f\x08\
            \xe0\x01aux_data\0\xf6\
            \xf1\xf7\x80\x9f\xe2\
            \x01\x04\x11\x12\xf6\xe2\
            \x02\x05\xf8\x04\x13\xe4\xe5\xf6\xe2\
            \x03\x06\xf8\x02\xf6\x14\xf6\xe2\
            \xe0\x02scale\0\x99\x88"
        .to_vec();

    let table = equation_table(&payload, 0, payload.len()).expect("eqtn_arr table");

    assert_eq!(table.declared_count, 4);
    assert_eq!(table.entity_ref, Some(159));
    assert_eq!(table.offset, 0);
    assert_eq!(table.rows.len(), 3);
    assert!(table.prototype_body.starts_with(b"\xe0\x01id\0"));
    assert!(table.prototype_body.ends_with(b"\xf1\xf7\x80\x9f\xe2"));

    assert_eq!(table.rows[0].equation_id, 1);
    assert_eq!(table.rows[0].function_id, 4);
    assert_eq!(table.rows[0].explicit_argument_count, None);
    assert_eq!(table.rows[0].arguments, [Some(17), Some(18)]);
    assert_eq!(table.rows[0].arguments_body, [0x11, 0x12]);
    assert_eq!(table.rows[0].auxiliary_body, [0xf6]);
    assert_eq!(table.rows[0].body, [1, 4, 0x11, 0x12, 0xf6, 0xe2]);

    assert_eq!(table.rows[1].equation_id, 2);
    assert_eq!(table.rows[1].function_id, 5);
    assert_eq!(table.rows[1].explicit_argument_count, Some(4));
    assert_eq!(
        table.rows[1].arguments,
        [Some(19), Some(1), Some(0), Some(0)]
    );
    assert_eq!(table.rows[1].arguments_body, [0x13, 0xe4, 0xe5]);
    assert_eq!(table.rows[1].auxiliary_body, [0xf6]);
    assert!(table.rows[1].body.ends_with(&[0xf6, 0xe2]));

    assert_eq!(table.rows[2].equation_id, 3);
    assert_eq!(table.rows[2].function_id, 6);
    assert_eq!(table.rows[2].explicit_argument_count, Some(2));
    assert_eq!(table.rows[2].arguments, [None, Some(20)]);
    assert_eq!(table.rows[2].arguments_body, [0xf6, 0x14]);
    assert_eq!(table.rows[2].auxiliary_body, [0xf6]);
}

#[test]
fn positional_relation_table_replays_rows_after_its_prototype() {
    let payload = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
            prototype\xf1\xf7\x64\xe2\
            \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

    let relations =
        positional_relation_table(payload, 0, payload.len(), 100).expect("positional relat_ptr");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert_eq!(relations.rows.len(), 1);
    assert_eq!(relations.rows[0].relation_id, 8);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(relations.rows[0].sign, 0);
    assert_eq!(relations.rows[0].dimension_id, 246);
    assert_eq!(relations.rows[0].relation_type, 0);
    assert!(relations.rows[0].operand_vectors.is_some());
}

#[test]
fn relation_table_retains_solver_children_after_an_invalid_row() {
    let payload = b"relat_ptr\0\xf4\x04\xf8\x03\xf7\x6a\xfb\xe2\
            schema\xf1\xf7\x6a\xe2invalid\
            skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";

    let relations = relation_table(payload, 0, payload.len()).expect("relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(106));
    assert!(relations.rows.is_empty());
    assert_eq!(relations.skamps.len(), 1);
    assert_eq!(relations.skamps[0].id, 5);
}

#[test]
fn relation_tables_retain_extents_without_their_prototypes() {
    let named = b"relat_ptr\0\xf8\x03\xf7\x64\xfb\xe2";
    let relations = relation_table(named, 0, named.len()).expect("named relat_ptr header");
    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());

    let positional = b"\xf8\x03\xf7\x64\xfb\xe2";

    let relations = positional_relation_table(positional, 0, positional.len(), 100)
        .expect("positional relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());
}

#[test]
fn positional_skamp_table_replays_counted_nested_items() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x01\xea\x22\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 2);
    assert_eq!(skamps[0].id, 1);
    assert_eq!(skamps[0].kind, 0);
    assert_eq!(skamps[0].items.len(), 2);
    assert_eq!(skamps[0].items[0].entity_id, 6);
    assert_eq!(skamps[0].items[1].sense, 2);
    assert_eq!(skamps[1].kind, 1);
    assert_eq!(skamps[1].flags, 34);
    assert_eq!(skamps[1].status, 35);
    assert_eq!(skamps[1].items[0].entity_id, 8);
}

#[test]
fn positional_skamp_table_skips_row_auxiliary_frames() {
    let payload = b"\xf8\x03\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x04\x00\x22\xe0\x02aux\0\xf8\x02\x0a\x0b\xf7\x60\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00\xf3\xf7\x58\xe2\
            \x03\x02\x00\x23\xf7\x60\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x09\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 3);
    assert_eq!(skamps[1].id, 2);
    assert_eq!(skamps[1].kind, 4);
    assert_eq!(skamps[1].items[0].entity_id, 8);
    assert_eq!(skamps[2].id, 3);
    assert_eq!(skamps[2].kind, 2);
    assert_eq!(skamps[2].items[0].entity_id, 9);
}

#[test]
fn positional_skamp_table_rejects_ambiguous_nested_item_arrays() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf3\xf7\x58\xe2\x02\x04\x00\x22\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x07\x00\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].id, 1);
}

#[test]
fn positional_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2";
    let rows = positional_feature_skamps(skamps, 0, skamps.len(), 88);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 1);

    let triples = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2";
    let rows = positional_relation_triples(triples, 0, triples.len(), 100);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(1));
}

#[test]
fn solver_header_does_not_adopt_a_later_array() {
    let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";

    assert!(named_solver_table_header(payload, b"skamp_ptr\0", 0, payload.len()).is_none());
}

#[test]
fn named_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2invalid";
    let rows = feature_skamps(skamps, 0, skamps.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 5);

    let triples = b"triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2\x01\x02\x03";
    let rows = feature_relation_triples(triples, 0, triples.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(7));
}

#[test]
fn positional_definition_preserves_its_named_solver_tables() {
    let solver_tables = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2\
            triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
    let mut payload = b"relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
    payload.extend_from_slice(solver_tables);
    let positional_start = payload.len();
    payload.extend_from_slice(solver_tables);
    payload.extend_from_slice(b"\xf8\x02\xf7\x6a\xfb\xe2");
    let prototype_offset = payload.len() + 3;
    assert!((128..=16_383).contains(&prototype_offset));
    payload.extend_from_slice(&[
        psb::token::ENTITY_REF,
        0x80 + u8::try_from(prototype_offset >> 8).expect("prototype offset high byte"),
        u8::try_from(prototype_offset & 0xff).expect("prototype offset low byte"),
    ]);
    payload.extend_from_slice(b"\xf1\xf7\x6a\xe2");

    let definitions = definitions_in_ranges(
        &payload,
        &[(0, 1, None, false), (positional_start, 2, None, true)],
    );
    let relations = definitions[1].relations.as_ref().expect("relations");

    assert_eq!(relations.skamps.len(), 1);
    assert_eq!(relations.skamps[0].id, 5);
    assert_eq!(
        relations
            .skamp_header
            .as_ref()
            .expect("skamp header")
            .declared_count,
        1
    );
    assert_eq!(relations.triples.len(), 1);
    assert_eq!(relations.triples[0].relation_id, Some(7));
    assert_eq!(
        relations
            .triples_header
            .as_ref()
            .expect("triples header")
            .declared_count,
        1
    );
}

#[test]
fn positional_triples_replay_nullable_relation_joins() {
    let payload = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2\x02\xf6\x05";

    let triples = positional_relation_triples(payload, 0, payload.len(), 100);

    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].relation_id, Some(1));
    assert_eq!(triples[0].equation_id, None);
    assert_eq!(triples[0].skamp_id, Some(4));
    assert_eq!(triples[1].relation_id, Some(2));
    assert_eq!(triples[1].skamp_id, Some(5));
}

#[test]
fn positional_trim_entity_table_decodes_without_segments() {
    let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        Some(68),
    )
    .expect("positional ent_tab");

    assert_eq!(entities.declared_count, Some(7));
    assert_eq!(entities.entity_ref, Some(66));
    assert_eq!(entities.entry_ref, Some(67));
    assert_eq!(entities.solved_external_ids, vec![9]);
    assert_eq!(entities.rows[0].vertices, [3, 4]);
    assert_eq!(entities.rows[0].kind, TrimEntityKind::Line);
}

#[test]
fn positional_trim_entity_table_retains_an_empty_extent() {
    let payload = b"prefix\xf8\x00\xf7\x42\xfb\xe2\
            \xf8\x01\xf7\x44\xfb\xe2";

    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        Some(68),
    )
    .expect("empty positional ent_tab");

    assert_eq!(entities.declared_count, Some(0));
    assert_eq!(entities.entity_ref, Some(66));
    assert_eq!(entities.entry_ref, Some(67));
    assert!(entities.rows.is_empty());
    assert!(entities.solved_external_ids.is_empty());
}

#[test]
fn positional_trim_entity_table_withholds_rows_without_the_entry_class() {
    let payload = b"prefix\xf8\x01\xf7\x42\xfb\xe2\
            \x00\xe3\x09\x00\x03\x04\xf6\x00";

    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        None,
    )
    .expect("positional ent_tab header");

    assert_eq!(entities.declared_count, Some(1));
    assert!(entities.rows.is_empty());
    assert!(entities.solved_external_ids.is_empty());
}

#[test]
fn positional_order_table_replays_prototype_and_following_rows() {
    let payload = b"prefix\xf8\x03\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

    let order =
        positional_order_table(payload, 0, payload.len(), 66).expect("positional order_table");

    assert_eq!(order.declared_count, 3);
    assert!(order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.rows[0].external_id, 10);
    assert_eq!(order.rows[0].internal_id, 2);
    assert_eq!(order.rows[0].bitmask, 1);
    assert_eq!(order.rows[1].external_id, 11);
    assert_eq!(order.internal_id(10), Some(2));
    assert_eq!(order.external_id(2), Some(10));

    let mut duplicate_external = order.clone();
    duplicate_external.declared_count += 1;
    duplicate_external.rows.push(FeatureOrderRow {
        external_id: 10,
        internal_id: 4,
        bitmask: 0,
        offset: 20,
    });
    assert_eq!(duplicate_external.internal_id(10), None);
    assert_eq!(duplicate_external.external_id(2), None);
    let mut duplicate_internal = order;
    duplicate_internal.declared_count += 1;
    duplicate_internal.rows.push(FeatureOrderRow {
        external_id: 12,
        internal_id: 2,
        bitmask: 0,
        offset: 21,
    });
    assert_eq!(duplicate_internal.external_id(2), None);
    assert_eq!(duplicate_internal.internal_id(10), None);
}

#[test]
fn named_order_table_replays_prototype_and_following_rows() {
    let payload = b"order_table\0\xf8\x03\xf7\x42\xfb\xe2\
            \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
            \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

    let order = order_table(payload, 0, payload.len()).expect("named order_table");

    assert_eq!(order.declared_count, 3);
    assert!(order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.external_id(2), Some(10));
    assert_eq!(order.internal_id(11), Some(3));
}

#[test]
fn order_tables_retain_extents_without_decoded_rows() {
    let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\xf1\xf7\x42\xe2";
    let order = order_table(named, 0, named.len()).expect("named order_table header");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(!order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert!(order.rows.is_empty());

    let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
    let order = positional_order_table(positional, 0, positional.len(), 66)
        .expect("positional order_table header");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(!order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert!(order.rows.is_empty());
}

#[test]
fn incomplete_order_tables_do_not_resolve_identifiers() {
    let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
            \xf1\xf7\x42\xe2\x0a\x02\x00";
    let order = order_table(named, 0, named.len()).expect("named order_table");
    assert_eq!(order.rows.len(), 1);
    assert!(!order.is_complete());
    assert_eq!(order.internal_id(10), None);
    assert_eq!(order.external_id(2), None);

    let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
    let order = positional_order_table(positional, 0, positional.len(), 66)
        .expect("positional order_table");
    assert!(!order.is_complete());
    assert_eq!(order.internal_id(10), None);
}

#[test]
fn positional_trim_vertex_table_is_independent_of_entity_rows() {
    let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x01\x02\x03\x00\xe2";
    let vertices = positional_trim_vertex_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 68,
            bucket: 69,
            entry: 69,
        },
        None,
        None,
    )
    .expect("positional vert_tab");

    assert_eq!(vertices.declared_count, Some(19));
    assert_eq!(vertices.entity_ref, Some(68));
    assert_eq!(vertices.entry_ref, Some(69));
    assert_eq!(vertices.rows.len(), 1);
    assert_eq!(vertices.rows[0].vertex_id, 3);
    assert_eq!(vertices.rows[0].entities, [1, 2]);
}

#[test]
fn positional_trim_vertex_table_retains_an_empty_extent() {
    let payload = b"prefix\xf8\x00\xf7\x44\xfb\xe2";

    let vertices = positional_trim_vertex_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 68,
            bucket: 69,
            entry: 69,
        },
        None,
        None,
    )
    .expect("empty positional vert_tab");

    assert_eq!(vertices.declared_count, Some(0));
    assert_eq!(vertices.entity_ref, Some(68));
    assert_eq!(vertices.entry_ref, Some(69));
    assert!(vertices.rows.is_empty());
}

#[test]
fn trim_vertex_uses_unique_shared_point_for_mixed_curves() {
    let segment = |kind, point_ids, external_id| FeatureSegment {
        kind,
        directions: [None; 3],
        point_ids,
        center_id: (kind == FeatureSegmentKind::Arc).then_some(4),
        arc_orientation: (kind == FeatureSegmentKind::Arc).then_some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![
            segment(FeatureSegmentKind::Line, [1, 2], 9),
            segment(FeatureSegmentKind::Arc, [2, 3], 10),
        ],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    };
    let variables = FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![FeatureSectionPoint {
            point_id: 2,
            u: Some(3.0),
            v: Some(4.0),
        }],
        offset: 0,
    };

    assert_eq!(
        entity_intersection([9, 10], Some(&segments), Some(&variables)),
        Some([3.0, 4.0])
    );

    let mut duplicate_segments = segments.clone();
    duplicate_segments.rows.push(segments.rows[0].clone());
    assert!(duplicate_segments.segment(9).is_none());
    assert!(entity_intersection([9, 10], Some(&duplicate_segments), Some(&variables)).is_none());

    let mut duplicate_points = variables.clone();
    duplicate_points.points.push(variables.points[0].clone());
    assert_eq!(
        duplicate_points.reconciled_points().0.get(&2),
        Some(&[Some(3.0), Some(4.0)])
    );
    assert_eq!(
        entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)),
        Some([3.0, 4.0])
    );
    duplicate_points.points[1].u = Some(5.0);
    assert!(duplicate_points.reconciled_points().1.contains(&2));
    assert!(entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)).is_none());
    let row = |variable_type, value, offset| FeatureVariableRow {
        variable_type,
        key: 2,
        value: Some(value),
        value_body: Vec::new(),
        guess: None,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: None,
        homogeneity: None,
        uvar_id: None,
        dimension_driven: false,
        offset,
    };
    let mut repeated_raw = variables.clone();
    repeated_raw.points[0] = FeatureSectionPoint {
        point_id: 2,
        u: None,
        v: None,
    };
    repeated_raw.rows = vec![row(1, 3.0, 30), row(1, 3.0, 31), row(2, 4.0, 32)];
    assert_eq!(
        repeated_raw.reconciled_points().0.get(&2),
        Some(&[Some(3.0), Some(4.0)])
    );
    repeated_raw.rows[1].value = Some(5.0);
    assert!(repeated_raw.reconciled_points().1.contains(&2));
}

#[test]
fn trim_vertex_template_identifies_table_and_entry_classes() {
    let payload = b"vert_tab\0\xf8\x13\xf7\x44\xfb\xe2\
            attrs\0\xf1\xf7\x46\xe3bucket_xar\0\xf8\x01\xf7\x46\xfb\xe3\
            \xf7\x45\x09\x0a\x03\x00";

    assert_eq!(
        trim_table_header(payload, b"vert_tab\0", 0, payload.len()),
        Some(TrimTableHeader {
            declared_count: 19,
            classes: TrimTableClasses {
                table: 68,
                bucket: 70,
                entry: 69,
            },
        })
    );
}

#[test]
fn trim_buckets_require_the_complete_declared_sequence_and_counts() {
    let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x01\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x02\xf1\xf7\x42\xe2\x03\xe2\
            \x04\xf0\xf7\x43\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0b\x0c\
            \x05\x00\xe2\x05\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0d\x0e\
            \x06\x00\xe2\x06\xe0\x00next\0";
    let header = TrimTableHeader {
        declared_count: 7,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };

    assert_eq!(
        trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Vertex)
            .iter()
            .map(|bucket| (
                bucket.index,
                bucket.declared_entry_count,
                bucket.decoded_entry_count
            ))
            .collect::<Vec<_>>(),
        (0..7)
            .zip([1, 1, 0, 0, 1, 1, 0])
            .map(|(index, count)| (index, count, count))
            .collect::<Vec<_>>()
    );
    let truncated = payload
        .windows(2)
        .position(|bytes| bytes == [0xe2, 0x06])
        .expect("last bucket index");
    assert_eq!(
        trim_buckets(payload, 0, truncated, header, TrimEntryKind::Vertex)
            .iter()
            .map(|bucket| bucket.index)
            .collect::<Vec<_>>(),
        (0..6).collect::<Vec<_>>()
    );
}

#[test]
fn trim_bucket_completeness_rejects_missing_and_extra_vertex_entries() {
    let header = TrimTableHeader {
        declared_count: 1,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };
    let missing = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe0";
    let buckets = trim_buckets(missing, 0, missing.len(), header, TrimEntryKind::Vertex);
    assert_eq!(buckets[0].declared_entry_count, 2);
    assert_eq!(buckets[0].decoded_entry_count, 1);
    assert!(!buckets[0].is_complete());

    let extra = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe3\x04\x05\x06\x00\xe0";
    let buckets = trim_buckets(extra, 0, extra.len(), header, TrimEntryKind::Vertex);
    assert_eq!(buckets[0].declared_entry_count, 1);
    assert_eq!(buckets[0].decoded_entry_count, 2);
    assert!(!buckets[0].is_complete());
}

#[test]
fn trim_vertex_entries_retain_variable_incident_entity_counts() {
    let counted = b"\xf8\x03\x0a\x0b\x0c\x07\x00";
    assert_eq!(
        trim_vertex_entry(counted, 0, counted.len()),
        Some((vec![10, 11, 12], 7, counted.len()))
    );
    let direct = b"\x0a\x0b\x0c\x07\x00";
    assert_eq!(
        trim_vertex_entry(direct, 0, direct.len()),
        Some((vec![10, 11, 12], 7, direct.len()))
    );
}

#[test]
fn trim_entity_bucket_counts_the_named_prototype_and_complete_bodies() {
    let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            entry_ptr(entity_entry)\0\xe3xid\0\x00ent_mode\0\x00start_vtx\0\xf6\
            end_vtx\0\xf6center_vtx\0\xf6pers_attribs\0\x00\
            \xf4\x04\xf7\x42\xe2\xe3\
            \x09\x00\x03\x04\xf6\x00\xe0";
    let header = TrimTableHeader {
        declared_count: 1,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };
    let buckets = trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Entity);
    assert_eq!(buckets[0].decoded_entry_count, 2);
    assert!(buckets[0].is_complete());

    let truncated = payload.len() - 2;
    let buckets = trim_buckets(payload, 0, truncated, header, TrimEntryKind::Entity);
    assert_eq!(buckets[0].decoded_entry_count, 1);
    assert!(!buckets[0].is_complete());
}
