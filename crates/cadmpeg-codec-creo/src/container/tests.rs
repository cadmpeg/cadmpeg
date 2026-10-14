// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::container;

mod aggregation;
mod sections;

#[test]
fn topology_face_ids_deduplicate_repeated_nodes() {
    let ids = super::topology_face_ids([7_u32, 7, 8]);
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), [7, 8]);
}

#[test]
fn named_datum_plane_aggregates_one_plane() {
    let payload = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0";
    let section = super::Section::scan("ActDatums".to_string(), 0, payload.len(), None, payload)
        .expect("bounded datum section");
    let planes = super::datum_planes(std::slice::from_ref(&section)).expect("named plane");
    assert_eq!(planes.len(), 1);
}

fn feature_row_for_aggregate(body: &[u8]) -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 10,
        body: body.to_vec().try_into().expect("two-byte feature row"),
        body_offset: 100,
        offset: 98,
    }
}

#[test]
fn feature_geometry_table_aggregation_collects_one_table() {
    let body = b"\xe0\x00dtm_id_tab\0\xf2\xf8\x01\xf7\x57\xfb\xe2\
        \xe0\x01dtm_id\0\x2a\xe0\x01dim_id\0\xf6";
    let row = feature_row_for_aggregate(body);
    let tables =
        super::feature_geometry_tables(&[], std::slice::from_ref(&row)).expect("aggregate table");
    assert_eq!(tables.len(), 1);
}

#[test]
fn feature_affected_id_aggregation_collects_one_record() {
    let body = b"\xe0\x01geoms_affected\0\xf8\x01\x2a";
    let row = feature_row_for_aggregate(body);
    let records = super::feature_affected_ids(&[], std::slice::from_ref(&row));
    assert_eq!(records.len(), 1);
}

#[test]
fn revolution_extent_aggregation_collects_one_extent() {
    use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition};
    use crate::feature::operations::{
        FeatureOperation, FeatureRecipe, OperationKind, OperationName, RecipeResolution,
    };

    let body = [
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(247),
            owner_feature_id: Some(247),
        },
        body: body.to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let operation = FeatureOperation {
        feature_id: 247,
        kind: OperationKind::Revolve,
        name: OperationName::Derived,
        recipe: RecipeResolution::Resolved(FeatureRecipe::ProtrudeRevolve),
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    };
    let records = super::feature_revolution_extents(
        &[],
        std::slice::from_ref(&definition),
        std::slice::from_ref(&operation),
    );
    assert_eq!(records.len(), 1);
}

#[test]
fn feature_definition_aggregation_collects_one_definition() {
    let payload = b"feat_defs_917\0template\xe3S2D0004\0replay";
    let section = super::Section::scan("FeatDefs".to_string(), 0, payload.len(), None, payload)
        .expect("bounded feature section");
    let rows = super::feature_definitions(std::slice::from_ref(&section)).expect("definition");
    assert_eq!(rows.len(), 1);
}

#[test]
fn feature_row_aggregation_collects_one_row() {
    use std::collections::BTreeSet;

    let payload = crate::test_support::allfeatur_row(4, [0xeb, 0x04], 917, &[0xaa]);
    let section =
        container::Section::scan("AllFeatur".to_string(), 0, payload.len(), None, &payload)
            .expect("bounded AllFeatur section");
    let rows = super::feature_rows(std::slice::from_ref(&section), &BTreeSet::from([4]));
    assert_eq!(rows.len(), 1);
}

#[test]
fn section_result_collector_returns_decoded_records() {
    let bytes = [0u8];
    let section =
        super::Section::scan("body".to_string(), 0, 1, None, &bytes).expect("one bounded section");
    let sections = [section];
    let records = super::collect_section_records_result(
        sections.iter(),
        |_| Ok(vec![42u32]),
        |_, _| {},
        |_| 0,
    )
    .expect("one output item");
    assert_eq!(records, vec![42]);
}

#[test]
fn structural_feature_ids_include_surface_feature() {
    let row = crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let ids =
        super::structural_feature_ids(&[], std::slice::from_ref(&row), &[]).expect("structural ID");
    assert_eq!(ids.len(), 1);
}

#[test]
fn candidate_feature_ids_copy_structural_ids() {
    use std::collections::BTreeSet;

    assert_eq!(
        super::candidate_feature_ids(&BTreeSet::from([4]), std::iter::empty()),
        BTreeSet::from([4])
    );
}

#[test]
fn candidate_feature_ids_add_new_ids() {
    use std::collections::BTreeSet;

    assert_eq!(
        super::candidate_feature_ids(&BTreeSet::from([4]), [5]),
        BTreeSet::from([4, 5])
    );
}

#[test]
fn complete_feature_ids_merge_additions() {
    use std::collections::BTreeSet;

    assert_eq!(
        super::complete_feature_ids(BTreeSet::from([4]), [5]),
        vec![4, 5]
    );
}

#[test]
fn feature_row_definitions_collect_one_definition() {
    let row = feature_row_for_aggregate(b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0");
    let definitions =
        super::feature_row_definitions(std::slice::from_ref(&row)).expect("one definition");
    assert_eq!(definitions.len(), 1);
}

fn one_feature_definition() -> crate::feature::definitions::FeatureDefinition {
    {
        crate::feature::definitions::depdb_section_definition(
            b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0",
            None,
        )
    }
    .expect("section definition admitted")
    .expect("one bounded section definition")
}

#[test]
fn claimed_definition_owners_collect_parsed_owner() {
    let mut definition = one_feature_definition();
    definition.identity = crate::feature::definitions::DefinitionIdentity::Parsed {
        schema_id: None,
        owner_feature_id: Some(7),
    };
    let owners = super::claimed_definition_owners(std::slice::from_ref(&definition));
    assert_eq!(owners.len(), 1);
}

#[test]
fn version_line_preserves_lossy_unicode_trim_under_service_policy() {
    let data = b" \t#UGC:2 P \xff \t\n";
    assert_eq!(super::line_at(data, 0), "#UGC:2 P �");
    assert_eq!(super::line_at(b" \t\n", 0), "");
}

#[test]
fn two_chart_pcurves_collect_both_rows() {
    use std::collections::BTreeSet;

    let samples = [
        0x0f, 0xe4, 0x0d, 0x18, 0xe4, 0x0f, 0x18, 0x0d, 0x0d, 0x18, 0xe4, 0x0f,
    ];
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 3]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 8, 9, 0, 0, 0xe3, 0xe1, 0xe3]);
    payload.extend_from_slice(&[8, 0, 4, 0xf6, 1]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 9, 7, 0, 0, 0xe3, 0xe1, 0xe3]);
    let section = super::Section::scan("body".to_string(), 0, payload.len(), None, &payload)
        .expect("one bounded section");
    let sections = [section];
    let face_ids = BTreeSet::from([10, 11]);
    let rows = super::two_chart_pcurves(&sections, &face_ids).expect("two rows");
    assert_eq!(rows.len(), 2);
}

#[test]
fn feature_reference_names_collect_one_reference() {
    let payload = b"\xf7\x71\x01\x05\x02N\xff\0\x01\x01";
    let section = super::Section::scan("MdlRefInfo".to_string(), 0, payload.len(), None, payload)
        .expect("one bounded reference section");
    let sections = [section];
    let names = super::feature_reference_names(&sections);
    assert_eq!(names.len(), 1);
}

mod framing;

#[test]
fn duplicate_primitive_section_namespace_is_refused() {
    {
        let section = |source_offset| super::ExpandedSection {
            name: "SolidPrimdata".to_owned(),
            source_offset,
            compressed_length: 0,
            data: Vec::new(),
        };
        let error = super::scan_primitives(&[section(1), section(2)])
            .map(|scan| scan.scalar_arrays.len())
            .expect_err("distinct sections cannot share primitive identity");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed { .. }));
    };
}

#[test]
fn expanded_section_above_the_local_ceiling_stays_compressed() {
    let bytes = b"#Body\n\x1f\x9d\x10";
    let section = super::Section::scan(
        "Body".to_owned(),
        0,
        bytes.len(),
        Some(256 * 1024 * 1024 + 1),
        bytes,
    )
    .expect("section extent");
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut losses = Vec::new();
        let expanded = super::expanded_sections(ctx, bytes, &[section], &mut losses)
            .expect("the expansion ceiling skips the section");
        assert!(expanded.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].section_offset, 0);
        assert!(losses[0].message.contains("256 MiB section ceiling"));
        assert!(ctx.resource_refusal().is_none());
    });
}

mod unit_selection;

mod work_admission;
