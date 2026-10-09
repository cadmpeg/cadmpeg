// SPDX-License-Identifier: Apache-2.0

use super::{
    admitted_hole_placements, hole_face_selection, schema_feature_definition,
    thicken_feature_definition, unbounded_feature_plane_definition,
};
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::UnresolvedFamily;
use cadmpeg_ir::features::{
    FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

fn service_unbounded_feature_plane_definition(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Option<IrFeatureDefinition> {
    crate::decode::with_test_decode_ctx(|ctx| {
        unbounded_feature_plane_definition(ctx, scan, ir, source_carriers, feature_id)
    })
    .expect("service unbounded plane admitted")
}

fn resolved_hole_face() -> Face {
    Face {
        id: FaceId::mint("creo:visibgeom:face#11").expect("identity grammar"),
        shell: ShellId::mint("creo:test:shell#1").expect("identity grammar"),
        surface: SurfaceId::mint("creo:visibgeom:surface#11").expect("identity grammar"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    }
}

fn hole_face_limit_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    resolved: bool,
    operation: &'static str,
) {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    if resolved {
        ir.model.faces.push(resolved_hole_face());
    }
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        hole_face_selection(
            ctx,
            &scan,
            &ir,
            9,
            11,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeSet::new(),
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn hole_native_face_selection_refuses_retained_limit() {
    hole_face_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
        "creo hole native face selection",
    );
}

#[test]
fn hole_resolved_face_id_refuses_retained_limit() {
    hole_face_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        true,
        "creo hole face IDs",
    );
}

#[test]
fn hole_resolved_face_vector_refuses_collection_limit() {
    hole_face_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        true,
        "creo hole face identities",
    );
}

#[test]
fn hole_resolved_face_keeps_its_native_reference() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.faces.push(resolved_hole_face());
    let selection = crate::decode::with_test_decode_ctx(|ctx| {
        hole_face_selection(
            ctx,
            &scan,
            &ir,
            9,
            11,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeSet::new(),
        )
    })
    .expect("service profile admits the resolved face");
    assert!(
        matches!(selection, cadmpeg_ir::features::FaceSelection::Resolved { faces, native }
        if faces == vec![resolved_hole_face().id]
            && native == "creo:visibgeom:surface#11")
    );
}

#[test]
fn hole_generated_native_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = thicken_scan();
    let result_surface_ids = std::collections::BTreeMap::from([(3, vec![11])]);
    let available_features =
        std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
            "creo:model:feature#3",
        )
        .expect("identity grammar")]);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo hole generated native selection"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            hole_face_selection(
                &trial_ctx,
                &scan,
                &CadIr::empty(),
                9,
                11,
                &result_surface_ids,
                &available_features,
            )
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = hole_face_selection(
        &ctx,
        &scan,
        &CadIr::empty(),
        9,
        11,
        &result_surface_ids,
        &available_features,
    )
    .expect_err("generated native copy exceeds the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo hole generated native selection"),
        "{error:?}"
    );
}

#[test]
fn hole_generated_face_keeps_its_native_reference() {
    let scan = thicken_scan();
    let result_surface_ids = std::collections::BTreeMap::from([(3, vec![11])]);
    let available_features =
        std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
            "creo:model:feature#3",
        )
        .expect("identity grammar")]);
    let selection = crate::decode::with_test_decode_ctx(|ctx| {
        hole_face_selection(
            ctx,
            &scan,
            &CadIr::empty(),
            9,
            11,
            &result_surface_ids,
            &available_features,
        )
    })
    .expect("service profile admits the generated face");
    assert!(
        matches!(selection, cadmpeg_ir::features::FaceSelection::Generated { native, .. }
        if native == "creo:visibgeom:surface#11")
    );
}

#[test]
fn hole_placements_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo hole placements"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            let placement = cadmpeg_ir::features::holes::HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("finite origin"),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .expect("finite direction"),
            };
            admitted_hole_placements(&ctx, [Some(placement), None, None]).map(|_| ())
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let placement = cadmpeg_ir::features::holes::HolePlacement::Axis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite origin"),
        axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .expect("finite direction"),
    };
    let error = admitted_hole_placements(&ctx, [Some(placement), None, None])
        .expect_err("one placement exceeds the resource limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo hole placements"),
        "{error:?}"
    );
}

fn thicken_scan() -> crate::container::ContainerScan<'static> {
    let entry =
        |entity_id, class_id, related_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(
                class_id,
                None,
                related_entity_id,
                related_entity_id.map(|_| 0),
            ),
            entity_id,
            prefixed: true,
            offset: usize::try_from(entity_id).expect("fixture index fits usize"),
            end_offset: usize::try_from(entity_id).expect("fixture index fits usize"),
        };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            17,
            80,
            vec![entry(101, 214, Some(11)), entry(201, 210, Some(101))],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([201]),
    );
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            3,
            67,
            vec![entry(11, 0, None)],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([11]),
    );
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    scan.surfaces.rows =
        crate::surface::unique_rows::UniqueIdRows::from_rows(vec![row(11, 3), row(201, 17)]);
    scan
}

fn thicken_resource_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    let scan = thicken_scan();
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        thicken_feature_definition(ctx, &scan, &CadIr::empty(), 17)
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn thicken_source_surface_ids_refuse_collection_limit() {
    thicken_resource_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo thicken source surface IDs",
    );
}

#[test]
fn thicken_native_selection_refuses_retained_limit() {
    thicken_resource_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo thicken native selection",
    );
}

#[test]
fn thicken_generated_native_copy_refuses_retained_limit() {
    thicken_resource_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo thicken generated native selection",
    );
}

#[test]
fn thicken_fixture_generates_a_face_under_service_policy() {
    let scan = thicken_scan();
    let definition = crate::decode::with_test_decode_ctx(|ctx| {
        thicken_feature_definition(ctx, &scan, &CadIr::empty(), 17)
    })
    .expect("service profile admits generated thicken face");
    assert!(matches!(
        definition,
        IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Generated { .. },
            ..
        })
    ));
}

#[test]
fn datum_feature_rejects_conflicting_local_and_transferred_plane_carriers() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 6,
            origin: [0.0, 1.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            offset: 1,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane { .. })
    ));

    match &mut ir.model.surfaces[0].geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let mut origin = *origin;
            origin.y = 2.0;
            *plane_surface =
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, *normal, *u_axis)
                    .expect("valid PlaneSurface fixture");
        }
        _ => panic!("transferred datum plane"),
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    );
}

fn unbounded_plane_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan
}

fn plane_surface(origin_y: f64) -> Surface {
    Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, origin_y, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    }
}

fn placed_plane() -> crate::surface::OutlinePlane {
    crate::surface::OutlinePlane {
        surface_id: 6,
        origin: [0.0, 1.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        offset: 1,
    }
}

#[test]
fn unbounded_plane_uses_its_placed_carrier_without_model_surface() {
    let mut scan = unbounded_plane_scan();
    scan.planes.positional_frames.push(placed_plane());

    assert_eq!(
        service_unbounded_feature_plane_definition(
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0)
                )
                .expect("valid test fixture"),
            }
        ))
    );
}

#[test]
fn unbounded_plane_uses_its_model_carrier_without_placed_surface() {
    let scan = unbounded_plane_scan();
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(plane_surface(1.0));

    assert_eq!(
        service_unbounded_feature_plane_definition(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0)
                )
                .expect("valid test fixture"),
            }
        ))
    );
}

#[test]
fn unbounded_plane_rejects_conflicting_carriers() {
    let mut scan = unbounded_plane_scan();
    scan.planes.positional_frames.push(placed_plane());
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(plane_surface(2.0));

    assert!(service_unbounded_feature_plane_definition(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        5
    )
    .is_none());
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            None,
            "Unbounded Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Native { .. })
    ));
}

#[test]
fn thicken_source_face_lookup_refuses_at_work_boundary() {
    let scan = thicken_scan();
    let definition =
        crate::test_support::assert_work_boundaries(&["creo thicken source face IDs"], |ctx| {
            thicken_feature_definition(ctx, &scan, &CadIr::empty(), 17)
        });
    assert!(matches!(
        definition,
        IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Generated { .. },
            ..
        })
    ));
}

#[test]
fn hole_face_copy_refuses_at_work_boundary() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.faces.push(resolved_hole_face());
    let selection = crate::test_support::assert_work_boundaries(
        &["creo hole face IDs"],
        |ctx| {
            hole_face_selection(
                ctx,
                &scan,
                &ir,
                9,
                11,
                &std::collections::BTreeMap::new(),
                &std::collections::BTreeSet::new(),
            )
        },
    );
    assert!(matches!(
        selection,
        cadmpeg_ir::features::FaceSelection::Resolved { faces, native }
            if faces == vec![resolved_hole_face().id]
                && native == "creo:visibgeom:surface#11"
    ));
}

#[test]
fn resolved_thicken_faces_copy_only_output_identities() {
    let scan = thicken_scan();
    let mut ir = CadIr::empty();
    let face = resolved_hole_face();
    let expected = face.id.clone();
    ir.model.faces.push(face);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        thicken_feature_definition(ctx, &scan, &ir, 17)
    };
    let definition = crate::test_support::assert_work_boundaries(&["creo thicken face IDs"], run);
    assert!(
        matches!(definition, IrFeatureDefinition::Operation(IrFeatureOperation::Thicken {
        faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native }, ..
    }) if faces == vec![expected] && native == "creo:allfeatur:thicken_source_surfaces#17:11")
    );
    for (dimension, operation) in [
        (
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "creo thicken face IDs",
        ),
        (
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo thicken face identities",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, run);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.operation == operation)
        );
    }
}

mod admission_recovery;
