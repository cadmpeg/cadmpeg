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
    for (retained_limit, item_limit, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::RetainedBytes,
            "creo saved spline loss text",
        ),
        (
            u64::MAX,
            0,
            ResourceDimension::CollectionItems,
            "creo saved spline losses",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained_limit;
        policy.limits.max_collection_items = item_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = push_saved_spline_loss(
            &ctx,
            &mut Vec::new(),
            format_args!(
                "Saved section spline at offset 7 cannot form a NURBS curve: {}",
                JoinedLaneRecords(&records)
            ),
        )
        .expect_err("below-need limit");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation));
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
    let arena = DecodeArena::new();
    for (limit, operation) in [
        (0, "creo revolved NURBS pole rows"),
        (1, "creo revolved NURBS weight rows"),
        (2, "creo revolved NURBS poles"),
        (11, "creo revolved NURBS weights"),
        (40, "creo revolved NURBS u knots"),
        (44, "creo revolved NURBS v knots"),
        (56, "IR NURBS paired grid rows"),
        (65, "IR NURBS paired poles"),
        (76, "IR NURBS admitted grid rows"),
        (85, "IR NURBS admitted poles"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::revolved_nurbs_surface(
            &ctx,
            &directrix,
            &axis,
            &"revolved NURBS fixture",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect_err("one collection boundary exceeds its named limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }
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
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        8,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(!unique_feature_surface_row(
        &[row.clone(), row],
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = super::extrusion_solved_segment_ids(&ctx, &definition)
        .expect_err("one solved ID exceeds zero nodes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo extrusion solved segment ID nodes"),
        "{error:?}"
    );
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        super::extrusion_solved_segment_ids(ctx, &definition)
    })
    .expect("service solved segment ID");
    assert_eq!(ids, std::collections::BTreeSet::from([7]));
}

#[test]
fn malformed_saved_spline_reports_transfer_loss() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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

#[test]
fn saved_spline_extrusion_refuses_construction_identity_copies() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
        }],
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
