// SPDX-License-Identifier: Apache-2.0
use crate::decode::coverage::{
    curve_transfer_coverage, design_constraint_transfer_coverage, surface_transfer_coverage,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::SourceObjectAssociation;

fn source() -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_core::text::NonBlankString::try_from("VisibGeom:41")
            .expect("source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

fn curve(geometry: CurveGeometry) -> Curve {
    Curve {
        id: CurveId::mint("test:model:entity#prefix-curve".to_string()).expect("curve identity"),
        geometry,
        source_object: Some(source()),
    }
}

fn surface(geometry: SurfaceGeometry) -> Surface {
    Surface {
        id: SurfaceId::mint("test:model:surface#prefix-surface".to_string())
            .expect("surface identity"),
        geometry,
        source_object: Some(source()),
    }
}

#[test]
fn transferred_curve_identity_prefix_refuses_work() {
    let curve = curve(CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid line"),
    )));
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo coverage identity prefix",
        |ctx| curve_transfer_coverage(ctx, &[], std::slice::from_ref(&curve)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo coverage identity prefix"));
}

#[test]
fn unknown_curve_identity_prefix_refuses_work() {
    let curve = curve(CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
        record: None,
    }));
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo coverage identity prefix",
        |ctx| curve_transfer_coverage(ctx, &[], std::slice::from_ref(&curve)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo coverage identity prefix"));
}

#[test]
fn transferred_surface_identity_prefix_refuses_work() {
    let surface = surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid plane"),
    )));
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo coverage identity prefix",
        |ctx| surface_transfer_coverage(ctx, &[], std::slice::from_ref(&surface), &[]),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo coverage identity prefix"));
}

#[test]
fn unknown_surface_identity_prefix_refuses_work() {
    let surface = surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
        record: None,
    }));
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo coverage identity prefix",
        |ctx| surface_transfer_coverage(ctx, &[], std::slice::from_ref(&surface), &[]),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo coverage identity prefix"));
}

#[test]
fn native_constraint_kind_prefix_refuses_work() {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntityId,
        SketchId,
    };
    use std::collections::BTreeMap;

    let sketch =
        SketchId::mint("synthetic:test:id#coverage-sketch".to_string()).expect("sketch identity");
    let entity = SketchEntityId::mint("synthetic:test:id#coverage-entity".to_string())
        .expect("entity identity");
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:id#sketch:relation:coverage".to_string())
            .expect("constraint identity"),
        sketch,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_core::text::NonBlankString::try_from("creo:relation:9")
                    .expect("native kind"),
                entities: vec![entity],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
                native_flags: None,
                native_properties: BTreeMap::new(),
            },
        )
        .expect("native constraint"),
        name: None,
        driving: None,
        active: Some(true),
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo native constraint kind prefix",
        |ctx| {
            design_constraint_transfer_coverage(
                ctx,
                std::slice::from_ref(&constraint),
                ":relation:",
                "creo:relation:",
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo native constraint kind prefix"));
}

#[test]
fn curve_coverage_admits_model_traversal_and_uses_scoped_lookup_storage() {
    let curve = curve(CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }));
    crate::test_support::assert_work_boundaries(
        &["creo curve coverage traversal", "creo coverage identity prefix"],
        |ctx| curve_transfer_coverage(ctx, &[], std::slice::from_ref(&curve)),
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo unknown curve ID nodes",
        |ctx| curve_transfer_coverage(ctx, &[], std::slice::from_ref(&curve)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn surface_coverage_admits_model_traversal_and_uses_scoped_lookup_storage() {
    let surface = surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }));
    crate::test_support::assert_work_boundaries(
        &["creo surface coverage traversal", "creo coverage identity prefix"],
        |ctx| surface_transfer_coverage(ctx, &[], std::slice::from_ref(&surface), &[]),
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo unknown surface ID nodes",
        |ctx| surface_transfer_coverage(ctx, &[], std::slice::from_ref(&surface), &[]),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn surface_coverage_shares_extrusion_evidence_across_equal_neutral_ids() {
    use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
    use cadmpeg_ir::ids::ProceduralSurfaceId;
    let plane = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
    ).expect("plane");
    let known = surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)));
    let construction = ProceduralSurfaceId::mint("test:model:surface#extrusion").expect("identity");
    let mut alias = surface(SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    });
    alias.source_object = None;
    let procedural = ProceduralSurface::new(construction,
        ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#directrix").expect("identity"), None,
                Vector3::new(0.0, 0.0, 1.0), None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            ).expect("extrusion"),
        ), None,
    );
    let kind = crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear);
    let row = crate::surface::SurfaceRow { id: 41, kind, feature_id: 0, reversed: false, boundary_type: crate::surface::BoundaryType::Code00, next_surface: 0, offset: 0 };
    let result = crate::decode::with_test_decode_ctx(|ctx| {
        surface_transfer_coverage(ctx, &[row], &[known, alias], &[procedural])
    }).expect("coverage");
    assert_eq!(result.family(kind), (1, 1));
    assert_eq!(result.transferred_rows(), 1);
}
