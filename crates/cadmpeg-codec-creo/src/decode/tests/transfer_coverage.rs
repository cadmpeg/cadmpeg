// SPDX-License-Identifier: Apache-2.0

use crate::decode::coverage::{
    constraint_kind_breakdown, curve_transfer_coverage, design_constraint_transfer_coverage,
    surface_transfer_coverage,
};
use crate::decode::sketch_transfer::profiles::{
    normalize_section_incidence_curve_family_evidence, IncidenceEvidence,
    SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::sketch_constraint_loci_compatible;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntityId,
    SketchGeometry, SketchId, SketchLocus,
};
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::BTreeMap;

fn assert_collection_refusal<T>(result: &Result<T, CodecError>, operation: &'static str) {
    assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == operation));
}

fn curve_coverage_with_limit(
    limit: u64,
) -> Result<crate::decode::coverage::CurveTransferCoverage, CodecError> {
    let row = |id, type_byte| crate::curve::CurveTopologyRow {
        id,
        type_byte,
        feature_id: 17,
        directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
        next_edges: [id, id],
        offset: 0,
    };
    let rows = [row(41, 0x05), row(42, 0x13)];
    let source = |native_id| SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_core::text::NonBlankString::try_from(format!("VisibGeom:{native_id}"))
            .expect("source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let curves = [
        Curve {
            id: CurveId::mint("test:model:entity#coverage-known".to_string()).expect("identity"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid line"),
            )),
            source_object: Some(source(41)),
        },
        Curve {
            id: CurveId::mint("test:model:entity#coverage-unknown".to_string()).expect("identity"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: Some(source(42)),
        },
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    curve_transfer_coverage(&ctx, &rows, &curves)
}

#[test]
fn curve_coverage_refuses_unique_count_node() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unique-row count nodes"),
            curve_coverage_with_limit,
        )),
        "creo unique-row count nodes",
    );
}

#[test]
fn curve_coverage_refuses_unique_projection() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unique-row projection"),
            curve_coverage_with_limit,
        )),
        "creo unique-row projection",
    );
}

#[test]
fn curve_coverage_refuses_transferred_id_node() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo transferred curve ID nodes"),
            curve_coverage_with_limit,
        )),
        "creo transferred curve ID nodes",
    );
}

#[test]
fn curve_coverage_refuses_unknown_id_node() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unknown curve ID nodes"),
            curve_coverage_with_limit,
        )),
        "creo unknown curve ID nodes",
    );
}

#[test]
fn curve_coverage_refuses_type_node() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo curve coverage type nodes"),
            curve_coverage_with_limit,
        )),
        "creo curve coverage type nodes",
    );
}

#[test]
fn curve_coverage_refuses_unknown_type_node() {
    assert_collection_refusal(
        &curve_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo curve coverage unknown type nodes"),
            curve_coverage_with_limit,
        )),
        "creo curve coverage unknown type nodes",
    );
}

fn surface_coverage_with_limit(
    limit: u64,
) -> Result<crate::decode::coverage::SurfaceTransferCoverage, CodecError> {
    let row = |id, kind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = [
        row(
            44,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ),
        row(45, crate::surface::SurfaceKind::Plane),
    ];
    let construction_id =
        ProceduralSurfaceId::mint("test:model:entity#coverage-construction".to_string())
            .expect("construction identity");
    let plane = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("valid plane");
    let source = |native_id| SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_core::text::NonBlankString::try_from(format!("VisibGeom:{native_id}"))
            .expect("source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let surfaces = [
        Surface {
            id: SurfaceId::mint("test:model:surface#coverage-extrusion".to_string())
                .expect("surface identity"),
            geometry: SurfaceGeometry::Procedural {
                construction: construction_id.clone(),
                cache: Some(SolvedSurfaceGeometry::Plane(plane)),
            },
            source_object: Some(source(44)),
        },
        Surface {
            id: SurfaceId::mint("test:model:surface#coverage-unknown".to_string())
                .expect("surface identity"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: Some(source(45)),
        },
    ];
    let procedural_surfaces = [ProceduralSurface::new(
        construction_id,
        ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#coverage-directrix".to_string())
                    .expect("directrix identity"),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .expect("valid extrusion"),
        ),
        None,
    )];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    surface_transfer_coverage(&ctx, &rows, &surfaces, &procedural_surfaces)
}

#[test]
fn surface_coverage_refuses_unique_count_node() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unique-row count nodes"),
            surface_coverage_with_limit,
        )),
        "creo unique-row count nodes",
    );
}

#[test]
fn surface_coverage_refuses_unique_projection() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unique-row projection"),
            surface_coverage_with_limit,
        )),
        "creo unique-row projection",
    );
}

#[test]
fn surface_coverage_refuses_extrusion_construction_node() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo extrusion construction nodes"),
            surface_coverage_with_limit,
        )),
        "creo extrusion construction nodes",
    );
}

#[test]
fn surface_coverage_refuses_extrusion_surface_node() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo extrusion surface nodes"),
            surface_coverage_with_limit,
        )),
        "creo extrusion surface nodes",
    );
}

#[test]
fn surface_coverage_refuses_transferred_row() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo transferred surface rows"),
            surface_coverage_with_limit,
        )),
        "creo transferred surface rows",
    );
}

#[test]
fn surface_coverage_refuses_unknown_id_node() {
    assert_collection_refusal(
        &surface_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo unknown surface ID nodes"),
            surface_coverage_with_limit,
        )),
        "creo unknown surface ID nodes",
    );
}

fn constraint_coverage_with_limit(
    limit: u64,
) -> Result<crate::decode::coverage::DesignConstraintTransferCoverage, CodecError> {
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    design_constraint_transfer_coverage(&ctx, &[constraint], [(":relation:", "creo:relation:")])
        .map(|[coverage]| coverage)
}

#[test]
fn constraint_coverage_refuses_native_kind_node() {
    assert_collection_refusal(
        &constraint_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo native constraint kind nodes"),
            constraint_coverage_with_limit,
        )),
        "creo native constraint kind nodes",
    );
}

#[test]
fn constraint_coverage_refuses_active_native_kind_node() {
    assert_collection_refusal(
        &constraint_coverage_with_limit(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo active native constraint kind nodes"),
            constraint_coverage_with_limit,
        )),
        "creo active native constraint kind nodes",
    );
}

#[test]
// These checked constructors must accept the explicit test fixtures.
#[allow(clippy::unwrap_used)]
fn surface_coverage_separates_transferred_unique_rows_from_ambiguous_ids() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Plane),
        row(42, crate::surface::SurfaceKind::Cylinder),
        row(
            44,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ),
        row(43, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::Cone),
    ];
    let plane = |id: &str, native_id: u32| Surface {
        id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: Some(SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Creo,
            object_id: cadmpeg_core::text::NonBlankString::try_from(format!(
                "VisibGeom:{native_id}"
            ))
            .expect("nonempty source identity"),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    };
    let mut surfaces = vec![
        plane("derived-id-independent-of-native-id", 41),
        plane("wrong-family", 42),
        plane("extrusion-carrier", 44),
    ];
    let cache = surfaces[2].geometry.clone();
    surfaces[2].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint(
            "test:model:entity#extrusion-construction".to_string(),
        )
        .expect("identity grammar"),
        cache: Some(cache.solved().expect("solved carrier").clone()),
    };
    let procedural_surfaces = vec![ProceduralSurface::new(
        ProceduralSurfaceId::mint("test:model:entity#extrusion-construction".to_string())
            .expect("identity grammar"),
        ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#directrix".to_string()).expect("identity grammar"),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .unwrap(),
        ),
        None,
    )];

    let coverage = crate::decode::with_test_decode_ctx(|ctx| {
        surface_transfer_coverage(ctx, &rows, &surfaces, &procedural_surfaces)
    })
    .expect("service surface coverage");

    assert_eq!(coverage.unique_rows(), 3);
    assert_eq!(coverage.transferred_rows(), 2);
    assert_eq!(coverage.ambiguous_rows(), 2);
    assert_eq!(coverage.family(crate::surface::SurfaceKind::Plane), (1, 1));
    assert_eq!(
        coverage.family(crate::surface::SurfaceKind::Cylinder),
        (1, 0)
    );
    assert_eq!(coverage.family(crate::surface::SurfaceKind::Cone), (0, 0));
    assert_eq!(
        coverage.family(crate::surface::SurfaceKind::Extrusion(
            crate::surface::ExtrusionVariant::Linear
        )),
        (1, 1)
    );
}

#[test]
fn curve_coverage_excludes_unknown_carriers_and_ambiguous_ids() {
    let row = |id, type_byte| crate::curve::CurveTopologyRow {
        id,
        type_byte,
        feature_id: 17,
        directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
        next_edges: [id, id],
        offset: 0,
    };
    let rows = vec![row(41, 0x05), row(42, 0x13), row(43, 0x05), row(43, 0x05)];
    let source = |native_id| SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_core::text::NonBlankString::try_from(format!("VisibGeom:{native_id}"))
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let curves = vec![
        Curve {
            id: CurveId::mint("test:model:entity#typed".to_string()).expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid LineCurve fixture"),
            )),
            source_object: Some(source(41)),
        },
        Curve {
            id: CurveId::mint("test:model:entity#opaque".to_string()).expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: Some(source(42)),
        },
    ];

    let coverage =
        crate::decode::with_test_decode_ctx(|ctx| curve_transfer_coverage(ctx, &rows, &curves))
            .expect("service curve coverage");

    assert_eq!(coverage.unique_rows(), 2);
    assert_eq!(coverage.transferred_rows(), 1);
    assert_eq!(coverage.ambiguous_rows(), 2);
    assert_eq!(coverage.by_type()[&0x05], (1, 1));
    assert_eq!(coverage.by_type()[&0x13], (1, 0));
}

#[test]
fn design_constraint_coverage_separates_typed_and_native_constraints() {
    let sketch =
        SketchId::mint("synthetic:test:id#sketch".to_string()).expect("valid test fixture");
    let constraint = |id: &str, definition| SketchConstraint {
        id: SketchConstraintId::mint(format!("synthetic:test:id#{id}"))
            .expect("valid test fixture"),
        sketch: sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            .expect("valid test fixture"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let entity =
        SketchEntityId::mint("synthetic:test:id#entity".to_string()).expect("valid test fixture");
    let mut constraints = vec![
        constraint(
            "sketch:relation:1",
            SketchConstraintDefinitionInput::Fixed {
                entity: entity.clone(),
            },
        ),
        constraint(
            "sketch:relation:2",
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_core::text::NonBlankString::try_from("creo:relation:9")
                    .expect("nonempty native kind"),
                entities: vec![entity.clone()],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
            },
        ),
        constraint(
            "sketch:skamp:3",
            SketchConstraintDefinitionInput::Fixed { entity },
        ),
    ];
    constraints[0].active = Some(true);
    constraints[1].active = Some(true);
    constraints[2].active = Some(false);

    let coverage = crate::decode::with_test_decode_ctx(|ctx| {
        design_constraint_transfer_coverage(ctx, &constraints, [(":relation:", "creo:relation:")])
            .map(|[coverage]| coverage)
    })
    .expect("service constraint coverage");

    assert_eq!(coverage.transferred, 2);
    assert_eq!(coverage.native, 1);
    assert_eq!(
        coverage
            .typed()
            .expect("native constraints are a transferred subset"),
        1
    );
    assert_eq!(coverage.active, 2);
    assert_eq!(coverage.active_native, 1);
    assert_eq!(
        coverage
            .active_typed()
            .expect("native constraints are a transferred subset"),
        1
    );
    assert_eq!(coverage.native_by_kind, BTreeMap::from([(9, 1)]));
    assert_eq!(coverage.active_native_by_kind, BTreeMap::from([(9, 1)]));
    let report_coverage = crate::decode::with_test_decode_ctx(|ctx| {
        let mut report_coverage = cadmpeg_ir::report::decode::Coverage::default();
        report_coverage
            .record_indexed(
                ctx,
                crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
                1,
                2,
            )
            .expect("coverage entry");
        report_coverage
            .record_indexed(
                ctx,
                crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
                9,
                1,
            )
            .expect("coverage entry");
        report_coverage
            .record_indexed(
                ctx,
                crate::coverage::TRANSFERRED_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
                9,
                4,
            )
            .expect("coverage entry");
        report_coverage
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| constraint_kind_breakdown(
            ctx,
            &report_coverage,
            "active_native_feature_relation_type_"
        )
        .expect("breakdown")
        .to_string()),
        "type 1=2, type 9=1"
    );
}

#[test]
fn native_curve_families_accept_only_their_defined_loci() {
    let point =
        SketchEntityId::mint("synthetic:test:id#point".to_string()).expect("valid test fixture");
    let bounded =
        SketchEntityId::mint("synthetic:test:id#bounded".to_string()).expect("valid test fixture");
    let line =
        SketchEntityId::mint("synthetic:test:id#line".to_string()).expect("valid test fixture");
    let reference_line = SketchEntityId::mint("synthetic:test:id#reference_line".to_string())
        .expect("valid test fixture");
    let circle =
        SketchEntityId::mint("synthetic:test:id#circle".to_string()).expect("valid test fixture");
    let geometry = BTreeMap::from([
        (
            point.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("point")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            bounded.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("bounded_curve")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            line.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            reference_line.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("reference_line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            circle.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("circle")
                    .expect("nonempty source identity"),
            ),
        ),
    ]);
    let compatible = SketchConstraintDefinitionInput::CoincidentLoci {
        loci: vec![
            SketchLocus::Entity(point),
            SketchLocus::Start(bounded),
            SketchLocus::Center(circle.clone()),
        ],
    };
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| sketch_constraint_loci_compatible(
            ctx,
            &compatible,
            &geometry
        ))
        .expect("service locus compatibility admitted")
    );
    let incompatible = SketchConstraintDefinitionInput::CoincidentLoci {
        loci: vec![SketchLocus::Start(line), SketchLocus::Start(circle)],
    };
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| sketch_constraint_loci_compatible(
            ctx,
            &incompatible,
            &geometry
        ))
        .expect("service locus compatibility admitted")
    );
    let centered_midpoint = SketchConstraintDefinitionInput::Midpoint {
        point: SketchLocus::Center(
            SketchEntityId::mint("synthetic:test:id#line".to_string()).expect("valid test fixture"),
        ),
        entity: SketchEntityId::mint("synthetic:test:id#bounded".to_string())
            .expect("valid test fixture"),
    };
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| sketch_constraint_loci_compatible(
            ctx,
            &centered_midpoint,
            &geometry
        ))
        .expect("service locus compatibility admitted")
    );
    let incompatible_midpoint = SketchConstraintDefinitionInput::Midpoint {
        point: SketchLocus::Center(reference_line),
        entity: SketchEntityId::mint("synthetic:test:id#bounded".to_string())
            .expect("valid test fixture"),
    };
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| sketch_constraint_loci_compatible(
            ctx,
            &incompatible_midpoint,
            &geometry
        ))
        .expect("service locus compatibility admitted")
    );
}

#[test]
fn incidence_family_lattice_narrows_endpoint_evidence() {
    let mut line: IncidenceEvidence = [
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Line,
    ]
    .into_iter()
    .collect();
    normalize_section_incidence_curve_family_evidence(&mut line);
    assert_eq!(
        line,
        [SectionEntityIncidenceFamily::Line].into_iter().collect()
    );

    let mut arc: IncidenceEvidence = [
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Circular,
    ]
    .into_iter()
    .collect();
    normalize_section_incidence_curve_family_evidence(&mut arc);
    assert_eq!(
        arc,
        [SectionEntityIncidenceFamily::Arc].into_iter().collect()
    );

    let mut conflicting: IncidenceEvidence = [
        SectionEntityIncidenceFamily::Line,
        SectionEntityIncidenceFamily::Circular,
    ]
    .into_iter()
    .collect();
    normalize_section_incidence_curve_family_evidence(&mut conflicting);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| conflicting.len(ctx))
            .expect("admitted incidence family count"),
        2
    );
}
