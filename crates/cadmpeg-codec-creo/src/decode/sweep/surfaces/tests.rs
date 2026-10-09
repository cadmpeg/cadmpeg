// SPDX-License-Identifier: Apache-2.0

use super::{
    push_saved_spline_loss, transfer_saved_spline_curves, unique_feature_surface_row,
    JoinedLaneRecords,
};
use crate::decode::tests::surface_row;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn saved_spline_loss_refuses_text_and_slot_below_limits() {
    let records = ["first".to_owned(), "second".to_owned()];
    for (dimension, operation) in [
        (
            ResourceDimension::RetainedBytes,
            "creo saved spline loss text",
        ),
        (
            ResourceDimension::CollectionItems,
            "creo saved spline losses",
        ),
    ] {
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::RetainedBytes {
                policy.limits.max_retained_bytes = cap;
            } else {
                policy.limits.max_collection_items = cap;
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            push_saved_spline_loss(
                &ctx,
                &mut Vec::new(),
                format_args!(
                    "Saved section spline at offset 7 cannot form a NURBS curve: {}",
                    JoinedLaneRecords(&records)
                ),
            )
        };
        let cap = crate::test_support::allocation_limit_at(dimension, Some(operation), run);
        let error = run(cap).expect_err("below-need limit");
        assert!(
            matches!(error, CodecError::ResourceLimit(resource) if resource.dimension == dimension && resource.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut losses = Vec::new();
    push_saved_spline_loss(
        &ctx,
        &mut losses,
        format_args!(
            "Saved section spline at offset 7 cannot form a NURBS curve: {}",
            JoinedLaneRecords(&records)
        ),
    )
    .expect("service loss");
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "Saved section spline at offset 7 cannot form a NURBS curve: first; second"
    );
}

#[test]
fn revolved_nurbs_surface_refuses_each_collection_boundary() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3, RevolutionAxis};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::{Point3, Vector3};

    let directrix = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid directrix");
    let axis = RevolutionAxis {
        origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite origin"),
        direction: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).expect("axis direction"),
        reference: None,
    };
    crate::test_support::assert_refusal_order(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        &[
            "creo revolved NURBS pole rows",
            "creo revolved NURBS weight rows",
            "creo revolved NURBS poles",
            "creo revolved NURBS weights",
            "creo revolved NURBS u knots",
            "creo revolved NURBS v knots",
            "IR NURBS paired grid rows",
            "IR NURBS paired poles",
            "IR NURBS admitted grid rows",
            "IR NURBS admitted poles",
        ],
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            super::revolved_nurbs_surface(
                &ctx,
                &directrix,
                &axis,
                &"revolved NURBS fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        },
    );
    let surface = crate::decode::with_test_decode_ctx(|ctx| {
        super::revolved_nurbs_surface(
            ctx,
            &directrix,
            &axis,
            &"revolved NURBS fixture",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
    })
    .expect("service allocation")
    .expect("valid revolution surface");
    assert_eq!((surface.u_count(), surface.v_count()), (2, 9));
    assert_eq!(surface.u_knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn generated_surface_binding_requires_one_matching_row() {
    let row = surface_row(31, 7, crate::surface::SurfaceKind::Plane);
    assert!(unique_feature_surface_row(
        &crate::surface::unique_rows::UniqueIdRows::from_rows(std::slice::from_ref(&row).to_vec()),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        &crate::surface::unique_rows::UniqueIdRows::from_rows(std::slice::from_ref(&row).to_vec()),
        31,
        8,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        &crate::surface::unique_rows::UniqueIdRows::from_rows(std::slice::from_ref(&row).to_vec()),
        31,
        7,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(!unique_feature_surface_row(
        &crate::surface::unique_rows::UniqueIdRows::from_rows([row.clone(), row].to_vec()),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
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
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
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
                        value: Vec::new(),
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

#[test]
fn extrusion_solved_segment_ids_refuse_before_tree_node() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut definition = saved_spline_definition();
    definition.trim_entities = Some(crate::feature::definitions::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::definitions::FeatureTrimEntity {
            external_id: 7,
            mode: None,
            vertices: [1, 2],
            kind: crate::feature::definitions::TrimEntityKind::Line,
            offset: 0,
        }],
        solved_external_ids: vec![7],
        offset: 0,
    });
    let arena = DecodeArena::new();
    crate::test_support::assert_refusal_order(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        &["creo extrusion solved segment ID nodes"],
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
            super::extrusion_solved_segment_ids(&ctx, &definition)
        },
    );
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        super::extrusion_solved_segment_ids(ctx, &definition)
    })
    .expect("service solved segment ID");
    assert_eq!(ids, std::collections::BTreeSet::from([7]));
}

#[test]
fn malformed_saved_spline_reports_transfer_loss() {
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
        .expect("section frame"),
    );
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| transfer_saved_spline_curves(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("transfer"),
        0
    );
    assert!(ir.model.curves.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::SectionSplineUnresolved.kind()
    );
}

#[test]
fn numerical_followup_revolution_refuses_skew_line_specialization() {
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3, RevolutionAxis};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
    for radius in [2e-10, 2.] {
        let axis = RevolutionAxis {
            origin: FinitePoint3::new(Point3::new(0., 0., 0.))
                .expect("valid finite regression fixture"),
            direction: FeatureDirection3::new(Vector3::new(0., 0., 1.))
                .expect("valid finite regression fixture"),
            reference: None,
        };
        let transform = crate::placement::FeatureSectionTransform::new(
            1,
            Some(1),
            [radius, 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            0,
        )
        .expect("valid finite regression fixture");
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(radius, radius),
        })
        .expect("valid finite regression fixture");
        assert!(super::revolved_section_surface(&transform, &line, &axis).is_none());
        let generator = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(0., radius),
        })
        .expect("valid finite regression fixture");
        assert!(super::revolved_section_surface(&transform, &generator, &axis).is_some());
    }
}

fn saved_spline_extrusion_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let mut definition = saved_spline_definition();
    let Some(crate::feature::definitions::FeatureSavedEntity::Spline(spline)) = definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .first_mut()
    else {
        panic!("saved spline");
    };
    spline.parameters.as_mut().expect("parameters").value = vec![0.0, 1.0];
    definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
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
    });
    scan.features.definitions.push(definition);
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section transform"),
    );
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(surface_row(
        20,
        40,
        crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
    ));
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
    for (id, z) in [(21, -1.0), (22, 1.0)] {
        scan.surfaces
            .rows
            .push(surface_row(id, 40, crate::surface::SurfaceKind::Plane));
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, 0.0, z],
            normal: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .expect("normal"),
            u_axis: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .expect("axis"),
            offset: 0,
        });
    }
    scan
}

#[test]
fn saved_spline_extrusion_refuses_construction_identity_copies() {
    let scan = saved_spline_extrusion_scan();
    let count = crate::test_support::assert_retained_boundaries(
        &[
            "creo construction curve identity copy",
            "creo construction surface identity copy",
        ],
        |ctx| {
            super::transfer_feature_extrusion_surfaces(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert_eq!(count, 1);
}

#[test]
fn saved_spline_translated_curve_promotes_only_on_new_curve_transfer() {
    let scan = saved_spline_extrusion_scan();
    let (count, mut ir) = crate::test_support::assert_retained_boundaries(
        &["creo saved extrusion translated curve"],
        |ctx| {
            let mut ir = CadIr::empty();
            let count = super::transfer_feature_extrusion_surfaces(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            Ok((count, ir))
        },
    );
    assert_eq!(count, 1);
    assert_eq!(ir.model.curves.len(), 1);
    let cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve),
    ) = &ir.model.curves[0].geometry else {
        panic!("translated spline directrix");
    };
    assert_eq!(curve.pole_rows().point_at(0).expect("first pole").get(),
        cadmpeg_ir::math::Point3::new(2.0, 0.0, -1.0));
    assert_eq!(curve.pole_rows().point_at(curve.pole_count() - 1).expect("last pole").get(),
        cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0));
    let ids = ir.model.curves.iter().map(|curve| curve.id.clone()).collect::<Vec<_>>();
    let mut losses = Vec::new();
    let repeated = crate::decode::with_test_decode_ctx(|ctx| {
        super::transfer_feature_extrusion_surfaces(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    }).expect("duplicate spline route");
    assert_eq!(repeated, 0);
    assert_eq!(ir.model.curves.iter().map(|curve| curve.id.clone()).collect::<Vec<_>>(), ids);
    assert_eq!(ir.model.surfaces.len(), 1);
    assert_eq!(ir.model.procedural_surfaces.len(), 1);
    assert!(losses.is_empty());
}

#[test]
fn absent_placed_curve_references_are_free_and_preserve_original_refusal() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};
    let transform = crate::placement::FeatureSectionTransform::new(
        5, Some(5), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
    ).expect("section frame");
    let sketch = SketchId::mint("creo:model:sketch#5").expect("sketch identity");
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0),
    }).expect("line");
    let point = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(0.0, 0.0),
    }).expect("point");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for (placement, geometry) in [(None, &line), (Some(&transform), &point)] {
        assert_eq!(super::placed_sketch_curve_ref(&ctx, placement, &sketch, 3, geometry)
            .expect("fixed absent reference"), None);
    }
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed absent curve reference").expect_err("zero work");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    for (placement, geometry) in [(None, &line), (Some(&transform), &point), (Some(&transform), &line)] {
        assert!(matches!(super::placed_sketch_curve_ref(&ctx, placement, &sketch, 3, geometry),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn short_revolution_axis_is_free_and_preserves_original_refusal() {
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3, RevolutionAxis};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::{Point3, Vector3};
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)], None, false,
    ).expect("structural admission").expect("finite directrix");
    // FeatureDirection3 admits finite positive squared norm. The codec's
    // normalization requires length greater than its near-zero threshold.
    let axis = RevolutionAxis {
        origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
        direction: FeatureDirection3::new(Vector3::new(0.0, 0.0, 5.0e-13)).expect("finite nonzero norm"),
        reference: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    assert!(super::revolved_nurbs_surface(&ctx, &curve, &axis, &"short axis", &mut refusal)
        .expect("fixed absent surface").is_none());
    assert!(refusal.take_records_checked().expect("no diagnostic").is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed short axis").expect_err("zero work");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    for _ in 0..2 {
        assert!(matches!(super::revolved_nurbs_surface(&ctx, &curve, &axis, &"short axis", &mut refusal),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}
