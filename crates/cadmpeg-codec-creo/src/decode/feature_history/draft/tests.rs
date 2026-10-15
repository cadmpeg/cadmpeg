// SPDX-License-Identifier: Apache-2.0

use super::{
    hole_face_selection, schema_feature_definition, thicken_feature_definition,
    unbounded_feature_plane_definition,
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

#[test]
fn hole_resolved_face_keeps_its_native_reference() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.faces.push(resolved_hole_face());
    let selection = ({
        hole_face_selection(
            &scan,
            &ir,
            9,
            11,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeSet::new(),
        )
    })
    .expect("resolved hole face");
    assert!(
        matches!(selection, cadmpeg_ir::features::FaceSelection::Resolved { faces, native }
        if faces == vec![resolved_hole_face().id]
            && native == "creo:visibgeom:surface#11")
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
    let selection = ({
        hole_face_selection(
            &scan,
            &CadIr::empty(),
            9,
            11,
            &result_surface_ids,
            &available_features,
        )
    })
    .expect("generated hole face");
    assert!(
        matches!(selection, cadmpeg_ir::features::FaceSelection::Generated { native, .. }
        if native == "creo:visibgeom:surface#11")
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
    scan.surfaces.rows = vec![row(11, 3), row(201, 17)];
    scan
}

#[test]
fn thicken_fixture_generates_a_face() {
    let scan = thicken_scan();
    let definition = ({ thicken_feature_definition(&scan, &CadIr::empty(), 17) })
        .expect("generated thicken face");
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
        crate::decode::with_service_context(|ctx| schema_feature_definition(
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
        crate::decode::with_service_context(|ctx| schema_feature_definition(
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
        unbounded_feature_plane_definition(
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        )
        .expect("unbounded plane admitted"),
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
        unbounded_feature_plane_definition(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        )
        .expect("unbounded plane admitted"),
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

    assert!(unbounded_feature_plane_definition(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        5
    )
    .expect("unbounded plane admitted")
    .is_none());
    assert!(matches!(
        crate::decode::with_service_context(|ctx| schema_feature_definition(
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
