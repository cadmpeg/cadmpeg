// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn section(reference_planes: ReferencePlanes) -> FeatureSection3d {
    FeatureSection3d {
        sketch_plane_entity_id: None,
        sketch_plane_flip: None,
        reference_planes,
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 17,
    }
}

fn empty_sources(rows: &crate::surface::SurfaceRows) -> PlacementSources<'_> {
    PlacementSources {
        datums: &[], surface_rows: rows, model_planes: &[], outline_planes: &[],
        plane_envelopes: &[], surface_parameters: &[], geometry_tables: &[], affected_ids: &[],
    }
}

#[test]
fn fixed_and_empty_carrier_reference_selection_is_free_and_keeps_original_refusal() {
    let mut explicit = section(ReferencePlanes::Named(vec![7, 8]));
    explicit.reference_plane_datum_geometry_id = Some(9);
    let cases = [
        (section(ReferencePlanes::Named(Vec::new())), None),
        (section(ReferencePlanes::Positional(Vec::new())), None),
        (explicit, Some(9)),
    ];
    for (section, expected) in &cases {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::super::unique_carrier_reference_id(&ctx, section)
            .expect("fixed or empty selection"), *expected);
        let original = ctx.charge_work_limit(1, "seed carrier reference refusal")
            .expect_err("zero work cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(super::super::unique_carrier_reference_id(&ctx, section),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn carrier_reference_selection_admits_present_ids_and_stops_at_first_conflict() {
    for (ids, expected) in [
        (vec![7], Some(7)),
        (vec![7, 7], Some(7)),
        (vec![7, 8, 7, 7], None),
    ] {
        let positional = ids.iter().map(|id| FeatureSectionReferencePlane {
            plane_entity_id: *id, reference_type: None, external_reference_id: None,
            segment_id: None, sub_index: None, reference_flip: None,
        }).collect();
        let cases = [section(ReferencePlanes::Named(ids)), section(ReferencePlanes::Positional(positional))];
        for section in &cases {
            let actual = crate::test_support::assert_work_boundaries(
                &["creo carrier reference ID selection"],
                |ctx| super::super::unique_carrier_reference_id(ctx, section),
            );
            assert_eq!(actual, expected);
            if expected.is_none() {
                let short = match &section.reference_planes {
                    ReferencePlanes::Named(ids) => self::section(ReferencePlanes::Named(ids[..2].to_vec())),
                    ReferencePlanes::Positional(ids) => self::section(ReferencePlanes::Positional(ids[..2].to_vec())),
                };
                let refusal = |section: &FeatureSection3d| {
                    let error = crate::test_support::last_refusal_at(&[],
                        ResourceDimension::WorkUnits, "creo carrier reference ID selection",
                        |ctx| super::super::unique_carrier_reference_id(ctx, section));
                    let CodecError::ResourceLimit(refusal) = error else {
                        panic!("carrier reference work boundary");
                    };
                    refusal
                };
                assert_eq!(refusal(&short), refusal(section), "conflict ends visits before the tail");
            }
        }
    }
}

#[test]
fn empty_generated_parent_visits_and_cached_placement_queries_are_free_and_fused() {
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = empty_sources(&rows);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut lookup = super::super::PlacementLookup::new(&ctx, &sources, &[]).expect("empty lookup");
    assert_eq!(super::super::generated_parent_plane_equation(&ctx, 42, [1.0, 0.0, 0.0], &mut lookup)
        .expect("empty parent rows"), None);
    assert!(lookup.plane_row(7).expect("empty plane row").is_none());
    assert!(lookup.definition(7).expect("empty definition").is_none());
    assert!(matches!(lookup.datum(7).expect("empty datum"), super::super::RowLookup::Missing));
    assert!(matches!(lookup.model(7).expect("empty model"), super::super::RowLookup::Missing));
    assert!(matches!(lookup.outline(7).expect("empty outline"), super::super::RowLookup::Missing));
    assert!(lookup.envelope_rows(7).expect("empty envelopes").is_empty());
    assert!(!lookup.is_generated_datum(7).expect("empty generated datum"));
    assert!(lookup.feature_datums(7).expect("empty feature datums").is_empty());
    assert!(lookup.feature_planes(7).expect("empty feature planes").is_empty());
    assert!(lookup.parameter(7).expect("empty parameters").is_none());
    assert_eq!(lookup.equation(7).expect("empty equation"), None);
    assert_eq!(lookup.generated_equation(7).expect("empty generated equation"), None);
    assert_eq!(lookup.transform_position(7, &[]).expect("empty transforms"), None);
    let original = ctx.charge_work_limit(1, "seed cached placement refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(lookup.plane_row(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.definition(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.datum(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.model(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.outline(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.envelope_rows(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.is_generated_datum(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.feature_datums(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.feature_planes(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.parameter(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.equation(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.generated_equation(7), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(lookup.transform_position(7, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::generated_parent_plane_equation(&ctx, 42, [1.0, 0.0, 0.0], &mut lookup),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(lookup.indexed_transforms, 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn fixed_placement_routes_keep_original_refusal_before_early_return_or_orientation() {
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = empty_sources(&rows);
    let mut definition = blank_definition();
    definition.identity = crate::feature::definitions::DefinitionIdentity::Parsed {
        schema_id: std::num::NonZeroU32::new(42), owner_feature_id: None,
    };
    let section = section(ReferencePlanes::Named(Vec::new()));
    let table = FeatureEntityTable::new(42, 0, Vec::new(), &std::collections::BTreeSet::new(), 17);
    let transform = FeatureSectionTransform::new(42, None, [1.0, 2.0, 3.0],
        [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 17).expect("fixed transform");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut lookup = super::super::PlacementLookup::new(&ctx, &sources, &[]).expect("empty lookup");
    assert_eq!(parse_generated_cylinder_section_transform(&ctx, &definition, &[], &mut lookup)
        .expect("no owning feature"), None);
    assert_eq!(parse_generated_planar_section_transform(&ctx, &definition, &[], &mut lookup)
        .expect("no owning feature"), None);
    assert!(!generated_planar_table_shape(&ctx, &table).expect("empty table"));
    assert_eq!(super::super::reference_flip_for_reference(&ctx, &section, None)
        .expect("fixed reference flip"), None);
    assert_eq!(super::super::apply_section_orientation(&ctx, transform.clone(), &section)
        .expect("fixed orientation"), transform);
    assert_eq!(super::super::definition_local_frame_transform(&ctx, &definition, &section)
        .expect("no owning feature"), None);
    assert_eq!(super::super::generated_cap_pair_plane_equation(&table, &mut lookup)
        .expect("empty cap table"), None);
    let reference = (7, SignedPlaneEquation { normal: [1.0, 0.0, 0.0], offset: 0.0 });
    assert_eq!(super::super::zero_offset_standard_section_plane_equation(&ctx, &definition,
        &section, reference, &[], &mut lookup).expect("no owning feature"), None);
    let original = ctx.charge_work_limit(1, "seed fixed placement refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(parse_generated_cylinder_section_transform(&ctx, &definition, &[], &mut lookup),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(parse_generated_planar_section_transform(&ctx, &definition, &[], &mut lookup),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(generated_planar_table_shape(&ctx, &table),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::reference_flip_for_reference(&ctx, &section, None),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::apply_section_orientation(&ctx, transform, &section),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::definition_local_frame_transform(&ctx, &definition, &section),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::generated_cap_pair_plane_equation(&table, &mut lookup),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::zero_offset_standard_section_plane_equation(&ctx, &definition,
        &section, reference, &[], &mut lookup), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn generated_parent_admits_each_present_datum_plane_and_envelope_without_terminal_fee() {
    let datums = [datum(2, crate::axis::Axis::X, 2.0)];
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![SurfaceRow {
        id: 7, kind: SurfaceKind::Plane, feature_id: 1, reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00, next_surface: 0, offset: 17,
    }]);
    let empty_rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let envelopes = [PlaneEnvelopeRecord {
        surface_id: 7, body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2], corners_3d: [[Some(2.0), None, None]; 2],
        },
        corner_coordinate_equal: [Some(true), None, None], scalar_tokens: Vec::new(),
        row_offset: 17, offset: 18,
    }];
    let equation = Some(SignedPlaneEquation { normal: [1.0, 0.0, 0.0], offset: 2.0 });
    let cases = [
        (PlacementSources { datums: &datums, ..empty_sources(&empty_rows) },
            "creo generated datum feature datum traversal", equation),
        (empty_sources(&rows),
            "creo generated datum feature plane traversal", None),
        (PlacementSources { plane_envelopes: &envelopes, ..empty_sources(&rows) },
            "creo generated datum envelope traversal", equation),
    ];
    for (sources, operation, expected) in cases {
        let actual = crate::test_support::assert_work_boundaries(&[operation], |ctx| {
            let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
            super::super::generated_parent_plane_equation(ctx, 1, [0.0, 1.0, 0.0], &mut lookup)
        });
        assert_eq!(actual, expected);
    }
}

#[test]
fn unique_placement_plane_row_fast_path_is_free_and_keeps_original_refusal() {
    for kind in [SurfaceKind::Plane, SurfaceKind::Cylinder] {
        let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![SurfaceRow {
            id: 7, kind, feature_id: 42, reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00, next_surface: 0, offset: 17,
        }]);
        let sources = empty_sources(&rows);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut lookup = super::super::PlacementLookup::new(&ctx, &sources, &[])
            .expect("empty lookup");
        assert_eq!(lookup.plane_row(7).expect("unique row").map(|row| (row.id, row.feature_id, row.offset)),
            (kind == SurfaceKind::Plane).then_some((7, 42, 17)));
        let original = ctx.charge_work_limit(1, "seed unique plane refusal")
            .expect_err("zero work cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(lookup.plane_row(7),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
