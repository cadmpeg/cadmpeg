// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn positional_section_reference_planes_refuse_before_each_row() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_positional_section_3d(
                ctx,
                POSITIONAL_SECTION_LIMIT_INPUT,
                0,
                POSITIONAL_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    crate::test_support::assert_refusal_order(
        ResourceDimension::CollectionItems,
        &[
            "creo positional section reference planes",
            "creo positional section reference planes",
        ],
        run,
    );
    let section = run(u64::MAX)
        .expect("section admitted")
        .expect("section present");
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        [6, 7]
    );
}

#[test]
fn named_section_reference_plane_refuses_before_vec_growth() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_section_3d(
                ctx,
                NAMED_SECTION_LIMIT_INPUT,
                0,
                NAMED_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    let result = run(crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo named section reference planes"),
        run,
    ));
    assert!(matches!(result,
Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo named section reference planes"));
    assert_eq!(
        run(u64::MAX)
            .expect("section admitted")
            .expect("section present")
            .reference_planes
            .entity_ids()
            .collect::<Vec<_>>(),
        [1]
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
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        vec![6, 7]
    );
    let ReferencePlanes::Positional(rows) = &section.reference_planes else {
        panic!("positional reference planes");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].plane_entity_id, 6);
    assert_eq!(rows[0].reference_type, Some(5));
    assert_eq!(rows[0].external_reference_id, None);
    assert_eq!(rows[0].segment_id, Some(3));
    assert_eq!(rows[0].sub_index, None);
    assert_eq!(rows[0].reference_flip, Some(BinaryFlag::Clear));
    assert_eq!(rows[1].plane_entity_id, 7);
    assert_eq!(rows[1].reference_type, Some(5));
    assert_eq!(rows[1].external_reference_id, None);
    assert_eq!(rows[1].segment_id, Some(4));
    assert_eq!(rows[1].sub_index, None);
    assert_eq!(rows[1].reference_flip, Some(BinaryFlag::Set));
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
    assert!(section.reference_planes.entity_ids().next().is_none());
    assert_eq!(section.orientation, FeatureSectionOrientation::default());
}

#[test]
fn positional_gsec3d_retains_placement_and_complete_reference_prefix() {
    let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\x07";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(513));
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        [6]
    );
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

    let definitions = crate::decode::with_test_decode_ctx(|ctx| {
        definitions_in_ranges(
            ctx,
            &payload[..],
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: std::num::NonZeroU32::new(1),
                owner_override: None,
                positional: false,
            }],
            None,
        )
    })
    .expect("definitions admitted");
    let section = definitions[0].section_3d.as_ref().expect("named gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(42));
    assert_eq!(section.reference_plane_datum_geometry_id, Some(6));
    assert_eq!(section.sketch_plane_flip, None);
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

#[test]
fn named_section_reference_stops_before_saved_result() {
    let payload = b"\xe0\0gsec3d_ptr\0\xe0\0ref_planes\0\xf8\x01\xf7\x80\
        \xe0\0p_saved_result\0";
    let section = crate::decode::with_test_decode_ctx(|ctx| {
        crate::feature::definitions::section_3d(ctx, payload, 0, payload.len())
    })
    .expect("placement admitted")
    .expect("named placement");
    assert_eq!(section.reference_planes, ReferencePlanes::Named(Vec::new()));
}
