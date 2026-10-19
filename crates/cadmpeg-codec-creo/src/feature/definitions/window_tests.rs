// SPDX-License-Identifier: Apache-2.0
//! Stored field boundaries and explicit nulls.

use super::{
    decode_section_coordinate_scalar, decode_variable_scalar, feature_relation_triples,
    positional_relation_triples, positional_variable_table, variable_table, ScalarLane,
};
use super::{
    positional_section_3d, saved_arc_scalar, saved_section_scalar, saved_spline_entities,
    BinaryFlag, FeatureSavedEntity, ReferencePlanes,
};
use crate::scalar;

#[test]
fn variable_scalar_lookahead_stays_inside_its_window() {
    let payload = [0x18, 0xe0];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        decode_variable_scalar(&payload, 0, 0, &cache),
        (ScalarLane::Undefined, 0)
    );
    assert_eq!(
        decode_variable_scalar(&payload, 0, 1, &cache),
        (ScalarLane::Undefined, 1)
    );
    assert_eq!(
        decode_variable_scalar(&payload, 0, 2, &cache),
        (ScalarLane::Value(0.0), 1)
    );
}

#[test]
fn coordinate_scalar_lookahead_stays_inside_its_window() {
    let payload = [0x18, 0xe0];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        decode_section_coordinate_scalar(&payload, 0, 0, &cache),
        (ScalarLane::Undefined, 0)
    );
    assert_eq!(
        decode_section_coordinate_scalar(&payload, 0, 1, &cache),
        (ScalarLane::Undefined, 1)
    );
    assert_eq!(
        decode_section_coordinate_scalar(&payload, 0, 2, &cache),
        (ScalarLane::Value(0.0), 1)
    );
}

#[test]
fn parameter_scalar_lookahead_stays_inside_its_window() {
    let payload = [0x18, 0xe0];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        crate::feature::definitions::decode_parameter_scalar(&payload, 0, 1, &cache),
        None
    );
    assert_eq!(
        crate::feature::definitions::decode_parameter_scalar(&payload, 0, 2, &cache),
        Some((0.0, 1))
    );
}

#[test]
fn variable_guess_requires_a_complete_in_window_suffix() {
    let payload = [0x18, 1, 2, 0x81, 0, 0xe0];
    let cache = scalar::ScalarCache::default();
    for source in [&payload[..], &payload[..4]] {
        let guess = crate::feature::definitions::decode_variable_guess(source, 0, 4, &cache);
        assert_eq!(guess, (ScalarLane::Undefined, 1));
    }
    let guess = crate::feature::definitions::decode_variable_guess(&payload, 0, 5, &cache);
    assert_eq!(guess, (ScalarLane::Value(0.0), 1));
}

#[test]
fn positional_variable_rows_require_every_stored_field() {
    let header = b"\xf8\x01\xf7\x77\xfb\xe2\xf7\x78";
    let row = b"\x01\x80\x80\x0f\x0f\x01\0\x80\x81";
    let mut payload = header.to_vec();
    payload.extend_from_slice(row);
    payload.extend_from_slice(&[0xf1, 0xf7, 119, 0xe2]);
    let definition_end = payload.len();
    payload.extend_from_slice(b"feat_defs_2\0");
    for end in header.len()..definition_end {
        let table =
            positional_variable_table(&payload, 0, end, 119, &scalar::ScalarCache::default())
                .expect("declared header");
        assert!(table.rows.is_empty(), "incomplete row at {end}");
        assert!(!table.is_complete());
    }
    let table = positional_variable_table(
        &payload,
        0,
        definition_end,
        119,
        &scalar::ScalarCache::default(),
    )
    .expect("complete table");
    assert!(table.is_complete());
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].key, 128);
    assert_eq!(table.rows[0].value, ScalarLane::Value(0.0));
    assert_eq!(table.rows[0].guess, ScalarLane::Value(0.0));
    assert_eq!(
        (
            table.rows[0].known,
            table.rows[0].homogeneity,
            table.rows[0].uvar_id
        ),
        (Some(1), Some(0), Some(129))
    );
}

#[test]
fn named_variable_replay_requires_every_solver_state_field() {
    let prototype = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\
        \xe0\x01type\0\x01\xe0\x01key\0\x07\xe0\x01value\0\x0f\
        \xe0\x01guess\0\x0f\xe0\x01known\0\x01\
        \xe0\x01homogeneity\0\0\xe0\x01uvar_id\0\x09\xf1\xf7\x77\xe2";
    for states in [&[1][..], &[1, 0][..], &[1, 0, 0x80][..]] {
        let mut payload = prototype.to_vec();
        payload.extend_from_slice(&[2, 7, 0x0f, 0x0f]);
        payload.extend_from_slice(states);
        payload.push(0xe2);
        let table = variable_table(&payload, 0, payload.len(), &scalar::ScalarCache::default())
            .expect("named table");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].key, 7);
        assert!(!table.is_complete());
    }
    let mut payload = prototype.to_vec();
    payload.extend_from_slice(&[2, 7, 0x0f, 0x0f, 1, 0, 10, 0xe2]);
    let table = variable_table(&payload, 0, payload.len(), &scalar::ScalarCache::default())
        .expect("complete named table");
    assert!(table.is_complete());
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        (
            table.rows[1].known,
            table.rows[1].homogeneity,
            table.rows[1].uvar_id
        ),
        (Some(1), Some(0), Some(10))
    );
}

#[test]
fn positional_triples_do_not_use_row_boundaries_as_null_fields() {
    let header = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59";
    for row in [
        &[0xe2][..],
        &[1, 0xe2][..],
        &[1, 2, 0xe2][..],
        &[0xe2, 2, 3][..],
        &[1, 0xe2, 3][..],
    ] {
        let mut payload = header.to_vec();
        payload.extend_from_slice(row);
        payload.extend_from_slice(b"\xe0\0next_table\0");
        assert!(positional_relation_triples(&payload, 0, payload.len(), 88).is_empty());
    }
    let mut payload = header.to_vec();
    payload.extend_from_slice(&[1, 2, 0xf6]);
    let rows = positional_relation_triples(&payload, 0, payload.len(), 88);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].relation_id, rows[0].equation_id, rows[0].skamp_id),
        (Some(1), Some(2), None)
    );
}

#[test]
fn named_triples_do_not_use_row_boundaries_as_null_fields() {
    let prototype = b"triples_ptr\0\xf8\x02\xf7\x58\xfb\xe2\
        \xe0\x01rel_id\0\x01\xe0\x01eqn_id\0\x02\xe0\x01skamp_id\0\x03\
        \xf1\xf7\x58\xe2";
    for row in [
        &[0xe2][..],
        &[1, 0xe2][..],
        &[1, 2, 0xe2][..],
        &[0xe2, 2, 3][..],
        &[1, 0xe2, 3][..],
    ] {
        let mut payload = prototype.to_vec();
        payload.extend_from_slice(row);
        payload.extend_from_slice(b"\xe0\0next_table\0");
        let rows = feature_relation_triples(&payload, 0, payload.len());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].relation_id, rows[0].equation_id, rows[0].skamp_id),
            (Some(1), Some(2), Some(3))
        );
    }
    let mut payload = prototype.to_vec();
    payload.extend_from_slice(&[1, 2, 0xf6, 0xe2]);
    let rows = feature_relation_triples(&payload, 0, payload.len());
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[1].relation_id, rows[1].equation_id, rows[1].skamp_id),
        (Some(1), Some(2), None)
    );
}

#[test]
fn saved_section_scalar_lookahead_stops_at_its_end() {
    let cache = scalar::ScalarCache::default();
    assert_eq!(saved_section_scalar(&[0x18, 0xe0], 0, 1, &cache), (None, 1));
    assert_eq!(
        saved_section_scalar(&[0x18, 0xe0], 0, 2, &cache),
        (Some(0.0), 1)
    );
}

#[test]
fn saved_arc_scalar_stops_before_following_dictionary_bytes() {
    let cache = scalar::ScalarCache::default();
    let bytes = [0x9b, 0, 0, 0, 0, 0, 0];
    for end in 1..bytes.len() {
        assert_eq!(saved_arc_scalar(&bytes, 0, end, &cache), (None, 1));
    }
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(4.0), 7)
    );
    assert_eq!(saved_arc_scalar(&[0x18, 0x28], 0, 1, &cache), (None, 1));
    assert_eq!(
        saved_arc_scalar(&[0x18, 0x28], 0, 2, &cache),
        (Some(0.0), 1)
    );
}

#[test]
fn saved_named_vector_macro_stops_at_its_end() {
    let payload = b"\xe0\x02center\0\x18\xe5";
    let cache = scalar::ScalarCache::default();
    let parse = |end| {
        crate::feature::definitions::saved_named_scalars::<3>(payload, b"center", 0, end, &cache)
    };
    assert_eq!(parse(payload.len() - 1), None);
    assert_eq!(
        parse(payload.len()),
        Some([Some(0.0), Some(1.0), Some(0.0)])
    );
}

#[test]
fn saved_spline_parameter_lookahead_stops_at_its_end() {
    let payload = b"\xe0\x02params\0\xf8\x01\x18\x6d";
    let cache = scalar::ScalarCache::default();
    let parse =
        |end| crate::feature::definitions::saved_spline_parameters(payload, 0, end, 1, &cache);
    assert_eq!(parse(payload.len() - 1), None);
    let full = parse(payload.len()).expect("zero parameter before dictionary prefix");
    assert_eq!(full.value, vec![0.0]);
    assert_eq!(full.body, b"\xf8\x01\x18");
}

#[test]
fn saved_spline_count_header_stops_at_its_end() {
    let mut payload = b"\xe0\x00save_entity_ptr(spline)\0\xe0\x02i_pnts\0\xf9".to_vec();
    let end = payload.len();
    payload.extend_from_slice(&[1, 3, 0x0f, 0x0f, 0x0f]);
    let entities = saved_spline_entities(&payload, 0, end, &scalar::ScalarCache::default());
    let entities = entities.expect("saved spline framing");
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline header");
    };
    assert_eq!(spline.declared_point_count, None);
    assert!(spline.interpolation_points.is_empty());
    assert!(spline.interpolation_points_body.is_empty());
}

#[test]
fn named_section_reference_stops_before_saved_result() {
    let payload = b"\xe0\0gsec3d_ptr\0\xe0\0ref_planes\0\xf8\x01\xf7\x80\
        \xe0\0p_saved_result\0";
    let section = crate::feature::definitions::section_3d(payload, 0, payload.len())
        .expect("named placement");
    assert_eq!(section.reference_planes, ReferencePlanes::Named(Vec::new()));
}

#[test]
fn positional_section_rows_stop_at_definition_end() {
    let definition = b"\x07S2D1\0\x01\xf6\xe1\xf6\x02\0\xf8\x01\xf7\x39\xfb\xe2\xf7\x3a";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(b"feat_defs_2\0");
    let section = positional_section_3d(&payload, 0, definition.len()).expect("section header");
    assert_eq!(section.sketch_plane_entity_id, Some(2));
    assert_eq!(section.sketch_plane_flip, Some(BinaryFlag::Clear));
    assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
    assert!(
        matches!(section.reference_planes, ReferencePlanes::Positional(rows) if rows.is_empty())
    );
}

#[test]
fn positional_section_header_and_multibyte_rows_stay_in_definition() {
    let mut payload = b"\x07S2D1\0\x01\xf6\xe1\xf6\x80\x01\0\xf8\x01\xf7\x81\x39\xfb\xe2\xf7\x81\x3a\x80\x06\x05\xf6\x03\xf6\x01".to_vec();
    let definition_end = payload.len();
    payload.extend_from_slice(b"feat_defs_2\0");
    for end in 6..definition_end {
        let section = positional_section_3d(&payload, 0, end).expect("section name");
        assert!(
            matches!(section.reference_planes, ReferencePlanes::Positional(rows) if rows.is_empty()),
            "end {end}"
        );
        if end <= 11 {
            assert_eq!(section.sketch_plane_entity_id, None, "incomplete plane ID");
        }
    }
    let section = positional_section_3d(&payload, 0, definition_end).expect("complete section");
    assert_eq!(section.sketch_plane_entity_id, Some(1));
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        [6]
    );
}
