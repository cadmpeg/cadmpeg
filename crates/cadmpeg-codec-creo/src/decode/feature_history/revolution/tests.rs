// SPDX-License-Identifier: Apache-2.0

use super::super::revolution::transfer_resolved_revolution_surfaces;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn revolution_axis_error_refuses_retained_text_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let direction =
        cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(2.0, 0.0, 0.0))
            .expect("finite nonzero direction");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo revolution axis error text"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            match super::revolution_unit_axis(&ctx, 40, direction) {
                Err(CodecError::Malformed(_)) => Ok(()),
                value => value.map(|_| ()),
            }
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::revolution_unit_axis(&ctx, 40, direction)
        .expect_err("axis error text exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo revolution axis error text"));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = super::revolution_unit_axis(ctx, 40, direction)
            .expect_err("nonunit direction is malformed");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "feature 40 revolution axis direction does not have unit length"));
        Ok::<(), CodecError>(())
    })
    .expect("service error text admitted");
}

#[test]
fn revolution_knot_error_refuses_retained_text_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo revolution knot error text"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            match super::directrix_parameter_range(&ctx, 17, &[]) {
                Err(CodecError::Malformed(_)) => Ok(()),
                value => value.map(|_| ()),
            }
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::directrix_parameter_range(&ctx, 17, &[])
        .expect_err("knot error text exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo revolution knot error text"));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = super::directrix_parameter_range(ctx, 17, &[])
            .expect_err("empty knot list is malformed");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "FeatDefs saved spline at offset 17 has no knots"));
        assert_eq!(
            super::directrix_parameter_range(ctx, 17, &[0.0, 1.0])?,
            [0.0, 1.0]
        );
        Ok::<(), CodecError>(())
    })
    .expect("service error text admitted");
}

#[test]
fn revolved_saved_spline_loss_refuses_text_and_row_below_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for (dimension, operation) in [
        (
            ResourceDimension::RetainedBytes,
            "creo revolved saved spline loss text",
        ),
        (
            ResourceDimension::CollectionItems,
            "creo revolved saved spline losses",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            super::push_revolution_surface_loss(ctx, &mut Vec::new(), "saved spline refused")
        });
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut losses = Vec::new();
    super::push_revolution_surface_loss(&ctx, &mut losses, "saved spline refused")
        .expect("service loss");
    assert_eq!(losses[0].message, "saved spline refused");
}

fn saved_spline_definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::definitions::test_support::with_points(
            crate::feature::definitions::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 0,
            },
            vec![
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(-1.0),
                },
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(0.0),
                    v: Some(1.0),
                },
            ],
        )),
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![crate::feature::definitions::FeatureSegment {
                kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 99,
                body: Vec::new(),
                offset: 0,
            }])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 7,
                internal_id: 1,
                bitmask: 0,
                offset: 0,
            }]
            .into(),
            offset: 0,
        }),
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        }),
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Spline(
                crate::feature::definitions::FeatureSavedSpline {
                    entity_id: Some(1),
                    declared_point_count: Some(2),
                    interpolation_points: vec![[2.0, 0.0, 0.0], [2.0, 0.0, 1.0]],
                    interpolation_points_body: Vec::new(),
                    endpoint_tangents: Some(crate::feature::definitions::DecodedField {
                        value: [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                        body: Vec::new(),
                    }),
                    parameters: Some(crate::feature::definitions::DecodedField {
                        value: vec![0.0, 1.0],
                        body: Vec::new(),
                    }),
                    offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    }
}

fn saved_spline_curve() -> Curve {
    Curve {
        id: CurveId::mint("creo:featdefs:saved_spline_curve#40:1".to_string())
            .expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
                None,
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid saved-spline curve"),
        )),
        source_object: None,
    }
}

fn transfer_with_curve_count(curve_count: usize) -> (usize, CadIr) {
    transfer_with_curve_count_and_scale(curve_count, None)
}

fn transfer_with_curve_count_and_scale(
    curve_count: usize,
    length_scale_mm: Option<cadmpeg_ir::scalar::PositiveReal>,
) -> (usize, CadIr) {
    let scan = saved_spline_revolution_scan();

    let mut ir = CadIr::empty();
    let mut source_carriers =
        crate::decode::source_carriers::SourceUnitCarriers::new(length_scale_mm);
    for curve in (0..curve_count).map(|_| saved_spline_curve()) {
        if length_scale_mm.is_some() {
            crate::decode::with_test_decode_ctx(|ctx| {
                source_carriers.admit_curve(ctx, &mut ir, curve)
            })
            .expect("saved spline admission");
        } else {
            ir.model.curves.push(curve);
        }
    }
    let transferred = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_resolved_revolution_surfaces(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut source_carriers,
        )
    })
    .expect("valid source object identity");
    (transferred, ir)
}

#[test]
fn saved_spline_revolution_uses_source_directrix_after_mm_admission() {
    let scale = cadmpeg_ir::scalar::PositiveReal::new(25.4).expect("inch scale");
    let (transferred, ir) = transfer_with_curve_count_and_scale(1, Some(scale));
    assert_eq!(transferred, 1);
    let Some(SolvedCurveGeometry::Nurbs(directrix)) = ir.model.curves[0].geometry.solved() else {
        panic!("saved directrix changed family");
    };
    assert_eq!(
        directrix.control_points()[0].get(),
        Point3::new(50.8, 0.0, 0.0)
    );
    let Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(surface)) =
        ir.model.surfaces[0].geometry.solved()
    else {
        panic!("revolved surface changed family");
    };
    assert_eq!(surface.poles()[0].get(), Point3::new(50.8, 0.0, 0.0));
}

#[test]
fn saved_spline_revolution_rejects_duplicate_model_curve_ids() {
    let (transferred, ir) = transfer_with_curve_count(1);
    assert_eq!(transferred, 1);
    assert_eq!(ir.model.surfaces.len(), 1);
    assert_eq!(ir.model.procedural_surfaces.len(), 1);

    let (transferred, ir) = transfer_with_curve_count(2);
    assert_eq!(transferred, 0);
    assert!(ir.model.surfaces.is_empty());
    assert!(ir.model.procedural_surfaces.is_empty());
}

fn saved_spline_revolution_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.definitions.push(saved_spline_definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("valid section frame"),
    );
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Revolve,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeRevolve,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .revolution_extents
        .push(crate::feature::rows::FeatureRevolutionExtent {
            feature_id: 40,
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 20,
        kind: crate::surface::SurfaceKind::Spline,
        feature_id: 40,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            40,
            29,
            vec![crate::feature::entity::FeatureEntityTableEntry {
                entity_id: 20,
                payload: crate::feature::entity::entry_payload(200, Some(7), None, None),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            }],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([20]),
    );

    scan
}

#[test]
fn saved_spline_revolution_refuses_construction_surface_identity_copy() {
    let scan = saved_spline_revolution_scan();
    let count = crate::test_support::assert_retained_boundaries(
        &["creo construction surface identity copy"],
        |ctx| {
            let mut ir = CadIr::empty();
            ir.model.curves.push(saved_spline_curve());
            transfer_resolved_revolution_surfaces(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert_eq!(count, 1);
}

#[test]
fn duplicate_saved_spline_surface_releases_candidate_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scan = saved_spline_revolution_scan();
    let mut ir = CadIr::empty();
    ir.model.curves.push(saved_spline_curve());
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#20")
            .expect("existing surface"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
        ),
        source_object: None,
    });
    let expected = ir.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for _ in 0..3 {
        let mut annotations = AnnotationBuilder::new();
        let mut losses = Vec::new();
        assert_eq!(
            transfer_resolved_revolution_surfaces(
                &ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut losses,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
            .expect("duplicate candidate keeps no output storage"),
            0
        );
        assert!(losses.is_empty());
    }
    assert_eq!(ir, expected);
    let available = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "after duplicate surface candidate",
        )
        .expect("all candidate and lookup reservations ended");
    drop(available);
    let error = ctx
        .charge_retained(1, "after duplicate surface retention")
        .expect_err("zero retained cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.used == 0));
}

#[test]
fn duplicate_vertex_orbits_release_candidate_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchPlacement, SketchProfiles,
    };
    for extrusion in [false, true] {
        let mut scan = saved_spline_revolution_scan();
        if extrusion {
            scan.features.operations[0].kind = crate::feature::operations::OperationKind::Extrude;
            scan.features.operations[0].recipe =
                crate::feature::operations::RecipeResolution::Resolved(
                    crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
                );
            scan.features.revolution_extents.clear();
            scan.features.rows.push(crate::feature::rows::FeatureRow {
                feature_id: 40,
                root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
                body: vec![0; 2].try_into().expect("complete fixture row header"),
                stream_offset: 0,
                body_offset: 0,
                offset: 0,
            });
        }
        let mut ir = CadIr::empty();
        let sketch_id = crate::decode::with_test_decode_ctx(|ctx| {
            super::model_sketch_id(ctx, &scan, &scan.features.definitions[0])
        })
        .expect("sketch identity admission")
        .expect("fixture sketch");
        let entity_id = SketchEntityId::mint("test:sketch:line#1").expect("fixture entity");
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: SketchProfiles::try_from(vec![vec![SketchEntityUse {
                entity: entity_id.clone(),
                reversed: false,
            }]])
            .expect("fixture profile"),
            native_ref: None,
        });
        ir.model.sketch_entities.push(SketchEntity::new(
            entity_id,
            sketch_id,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(2.0, 1.0),
            })
            .expect("fixture line"),
        ));
        let transfer = |ctx: &DecodeContext<'_>, ir: &mut CadIr| {
            let mut annotations = AnnotationBuilder::new();
            let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
            if extrusion {
                super::transfer_resolved_extrusion_vertex_orbit_curves(
                    ctx,
                    &scan,
                    ir,
                    &mut annotations,
                    &mut source_carriers,
                )
            } else {
                super::transfer_resolved_revolution_vertex_orbit_curves(
                    ctx,
                    &scan,
                    ir,
                    &mut annotations,
                    &mut source_carriers,
                )
            }
        };
        let count = crate::decode::with_test_decode_ctx(|ctx| transfer(ctx, &mut ir))
            .expect("service orbit transfer");
        assert_eq!(count, 2, "both profile endpoints generate orbits");
        assert_eq!(ir.model.curves.len(), 2);
        let expected = ir.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1024 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        for _ in 0..3 {
            assert_eq!(
                transfer(&ctx, &mut ir).expect("duplicate orbit keeps no output storage"),
                0
            );
        }
        assert_eq!(ir, expected);
        let available = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "after duplicate orbit candidates",
            )
            .expect("all candidate reservations ended");
        drop(available);
    }
}
