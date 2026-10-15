// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn circular_entity_selection_stops_after_second_match() {
    let definition = blank_definition();
    let table = FeatureEntityTable::new(
        42,
        0,
        [204, 203, 200, 200]
            .map(|class_id| FeatureEntityTableEntry {
                entity_id: 1,
                payload: crate::feature::entity::entry_payload(class_id, Some(2), None, None),
                prefixed: false,
                end_offset: 1,
                offset: 0,
            })
            .to_vec(),
        &std::collections::BTreeSet::default(),
        0,
    );
    let short = [table.clone(), table.clone()];
    let mut long = short.to_vec();
    long.extend(std::iter::repeat_n(table, 64));
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = PlacementSources {
        datums: &[],
        surface_rows: &rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &[],
        affected_ids: &[],
    };
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, tables: &[FeatureEntityTable]| {
        super::super::circular_profile_aligned_origin(
            ctx,
            &definition,
            42,
            super::super::SectionPlaneAxes {
                plane: SignedPlaneEquation {
                    normal: [0.0, 0.0, 1.0],
                    offset: 0.0,
                },
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
            &mut super::super::PlacementLookup::new(ctx, &sources, &[])?,
            tables,
        )
    };
    crate::test_support::assert_work_boundaries(
        &["creo circular profile entity table selection"],
        |ctx| run(ctx, &short),
    );
    let refusal = |tables: &[FeatureEntityTable]| {
        crate::test_support::last_refusal_at(
            b"",
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "creo circular profile entity table selection",
            |ctx| run(ctx, tables),
        )
    };
    let cadmpeg_core::CodecError::ResourceLimit(short_refusal) = refusal(&short) else {
        panic!("work refusal")
    };
    let cadmpeg_core::CodecError::ResourceLimit(long_refusal) = refusal(&long) else {
        panic!("work refusal")
    };
    assert_eq!(short_refusal, long_refusal);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| run(ctx, &long)).expect("ambiguous selection"),
        None
    );
}

#[test]
fn local_frame_selection_stops_after_second_complete_match() {
    let frame = FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: Some(finite_frame([0.0; 12])),
        offset: 1,
    };
    let mut short = blank_definition();
    short.parameter_frames = vec![frame.clone(), frame.clone()];
    let mut long = short.clone();
    long.parameter_frames.extend(std::iter::repeat_n(frame, 64));
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, definition: &FeatureDefinition| {
        super::super::unique_complete_local_system(ctx, definition)
    };
    crate::test_support::assert_work_boundaries(&["creo complete local frame selection"], |ctx| {
        run(ctx, &short)
    });
    let refusal = |definition: &FeatureDefinition| {
        crate::test_support::last_refusal_at(
            b"",
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "creo complete local frame selection",
            |ctx| run(ctx, definition),
        )
    };
    let cadmpeg_core::CodecError::ResourceLimit(short_refusal) = refusal(&short) else {
        panic!("work refusal")
    };
    let cadmpeg_core::CodecError::ResourceLimit(long_refusal) = refusal(&long) else {
        panic!("work refusal")
    };
    assert_eq!(short_refusal, long_refusal);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| run(ctx, &long)).expect("ambiguous frame"),
        None
    );
}

#[test]
fn carrier_index_reuses_queries_and_skips_unused_envelopes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let outlines = [OutlinePlane {
        surface_id: 7,
        origin: [0.0, 0.0, 3.0],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 1,
    }];
    let envelope = PlaneEnvelopeRecord {
        surface_id: 7,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [[None; 3]; 2],
        },
        corner_coordinate_equal: [None; 3],
        scalar_tokens: Vec::new(),
        row_offset: 0,
        offset: 0,
    };
    let unused = vec![envelope; 64];
    let run = |cap, envelopes: &[PlaneEnvelopeRecord], repeat: bool| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
        let sources = PlacementSources {
            datums: &[],
            surface_rows: &rows,
            model_planes: &[],
            outline_planes: &outlines,
            plane_envelopes: envelopes,
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        };
        let mut lookup = super::super::PlacementLookup::new(&ctx, &sources, &[])?;
        let equation = lookup.generated_equation(7)?;
        assert_eq!(equation, Some(([0.0, 0.0, 1.0], 3.0)));
        if repeat {
            assert_eq!(lookup.generated_equation(7)?, equation);
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
        run(cap, &[], false)
    });
    assert_eq!(
        cap,
        crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| run(
            cap, &unused, true
        ))
    );
    run(cap, &unused, true).expect("unchanged work admits repeated indexed queries");
}

#[test]
fn duplicate_outline_blocks_generated_envelope_fallback() {
    let outline = OutlinePlane {
        surface_id: 7,
        origin: [0.0, 0.0, 3.0],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 1,
    };
    let outlines = [outline.clone(), outline];
    let envelopes = [PlaneEnvelopeRecord {
        surface_id: 7,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [[Some(2.0), None, None]; 2],
        },
        corner_coordinate_equal: [Some(true), None, None],
        scalar_tokens: Vec::new(),
        row_offset: 0,
        offset: 0,
    }];
    crate::decode::with_test_decode_ctx(|ctx| {
        let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
        let sources = PlacementSources {
            datums: &[],
            surface_rows: &rows,
            model_planes: &[],
            outline_planes: &outlines,
            plane_envelopes: &envelopes,
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        };
        let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
        assert_eq!(lookup.generated_equation(7)?, None);
        let absent_outlines = PlacementSources {
            outline_planes: &[],
            ..sources
        };
        let mut lookup = super::super::PlacementLookup::new(ctx, &absent_outlines, &[])?;
        assert_eq!(lookup.generated_equation(7)?, Some(([1.0, 0.0, 0.0], 2.0)));
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("generated plane fallback");
}

#[test]
fn generated_parent_selection_keeps_equation_across_nonmatching_tail() {
    let datums = [
        datum(2, crate::axis::Axis::X, 0.0),
        datum(4, crate::axis::Axis::Y, 0.0),
    ];
    let tables = [FeatureGeometryTable {
        feature_id: 40,
        kind: FeatureGeometryTableKind::DatumIds(Some(vec![42])),
        count: 1,
        entity_class: 87,
        offset: 20,
    }];
    let parents = [
        FeatureAffectedIds {
            feature_id: 40,
            kind: AffectedIdKind::Parents,
            ids: vec![1, 3],
            offset: 40,
        },
        FeatureAffectedIds {
            feature_id: 41,
            kind: AffectedIdKind::Parents,
            ids: vec![1, 9],
            offset: 50,
        },
    ];
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = PlacementSources {
        datums: &datums,
        surface_rows: &rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &tables,
        affected_ids: &parents,
    };
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
        let equation =
            super::super::generated_datum_plane_equation(ctx, 42, 2, [1.0, 0.0, 0.0], &mut lookup)?;
        assert_eq!(
            equation,
            Some(SignedPlaneEquation {
                normal: [0.0, 1.0, 0.0],
                offset: 0.0
            })
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    crate::decode::with_test_decode_ctx(run).expect("generated parent plane");
    crate::test_support::assert_work_boundaries(
        &[
            "creo generated datum table scan",
            "creo generated datum ID count",
            "creo placement datum index traversal",
            "creo generated datum parent selection",
            "creo generated datum parent membership",
            "creo generated datum other parent selection",
            "creo placement feature datum index traversal",
            "creo generated datum feature datum traversal",
        ],
        run,
    );
}

#[test]
fn generated_plane_index_ignores_same_id_nonplane_rows() {
    let plane = SurfaceRow {
        id: 7,
        kind: SurfaceKind::Plane,
        feature_id: 42,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 1,
    };
    let cylinder = SurfaceRow {
        kind: SurfaceKind::Cylinder,
        offset: 2,
        ..plane.clone()
    };
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![plane, cylinder]);
    assert!(rows.unique(7).is_none());
    let sources = PlacementSources {
        datums: &[],
        surface_rows: &rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &[],
        affected_ids: &[],
    };
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
        assert_eq!(
            lookup.plane_row(7)?.map(|row| row.kind),
            Some(SurfaceKind::Plane)
        );
        assert_eq!(lookup.plane_row(7)?.map(|row| row.feature_id), Some(42));
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("plane-only identity selection");
}

#[test]
fn placement_transform_index_tracks_appended_owner_ambiguity() {
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = PlacementSources {
        datums: &[],
        surface_rows: &rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &[],
        affected_ids: &[],
    };
    let frame = |feature_id, offset| {
        FeatureSectionTransform::new(
            42,
            Some(feature_id),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            offset,
        )
        .expect("valid frame")
    };
    let frames = [frame(42, 1), frame(42, 2), frame(43, 3)];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut lookup = super::super::PlacementLookup::new(ctx, &sources, &[])?;
        assert_eq!(lookup.transform_position(42, &frames[..1])?, Some(0));
        assert_eq!(lookup.transform_position(42, &frames[..1])?, Some(0));
        assert_eq!(lookup.transform_position(42, &frames[..2])?, None);
        assert_eq!(lookup.transform_position(43, &frames)?, Some(2));
        assert_eq!(lookup.transform_position(42, &frames)?, None);
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    crate::decode::with_test_decode_ctx(run).expect("append-only transform index");
    crate::test_support::assert_work_boundaries(&["creo placement transform index traversal"], run);
}
