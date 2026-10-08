//! Hole position graph and marker-pattern tests.

use super::{cylinder, lane};
use crate::records::{
    FeatureInputRelationFamily, SketchInputEntity, SketchInputKind, SketchRelationKind,
};
use crate::resolved_features::holes::{
    compact_position_loci, marker_pattern_bore_axes, HoleMarkers,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::holes::HolePlacement;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

#[test]
fn compact_position_graph_selects_the_unique_bore_loci() {
    use FeatureInputRelationFamily::{
        PointPointDistance, PointPointHorizontalDistance, PointPointVerticalDistance,
    };

    let loci = [
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 16.0),
        Point2::new(0.0, 41.0),
    ];
    let relations = [
        (PointPointDistance, 0, 2, 25.0),
        (PointPointVerticalDistance, 0, 5, 0.0),
        (PointPointHorizontalDistance, 0, 5, 16.0),
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let placement_loci = [1, 2].into_iter().collect();
    assert_eq!(
        compact_position_loci(&ctx, &loci, &placement_loci, &relations).unwrap(),
        Some(vec![1, 2])
    );

    let ambiguous = [loci[0], loci[1], loci[2], Point2::new(0.0, -9.0)];
    let ambiguous_placements = [1, 2, 3].into_iter().collect();
    assert_eq!(
        compact_position_loci(&ctx, &ambiguous, &ambiguous_placements, &relations).unwrap(),
        None
    );
}

#[test]
fn object_indexed_curve_markers_select_a_congruent_bore_pattern() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane();
    lane.sketch_entities = [(1, [0.013, 0.007]), (2, [-0.009, 0.007])]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (object_index, coordinates_m))| {
            let marker_id: String = format!("marker-{ordinal}");
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                u32::try_from(ordinal).expect("ordinal fits u32"),
                cadmpeg_core::decode::u64_from_index(ordinal),
                SketchInputKind::LineOrCircle,
            );
            constructed_marker.feature_ref = Some("position".into());
            constructed_marker = constructed_marker.with_test_identity(Some(object_index), None);
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates_m);
            constructed_marker.links = None;
            constructed_marker
        })
        .collect();
    let surface = |id, x| Surface {
        id: SurfaceId::mint(format!("test:model:entity#surface-{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(x, 7.0, 10.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.1,
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let mut surfaces = vec![surface(0, -9.0), surface(1, 13.0), surface(2, 100.0)];

    let placements = marker_pattern_bore_axes(
        &ctx,
        &HoleMarkers::new(&ctx, &lane).unwrap(),
        "position",
        2.1,
        &surfaces,
        None,
    )
    .unwrap()
    .expect("required invariant");
    assert_eq!(placements.len(), 2);
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::holes::HolePlacement::Axis { origin, .. }
            if origin.x == -9.0 && origin.y == 7.0 && origin.z == 10.0
    )));
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::holes::HolePlacement::Axis { origin, .. }
            if origin.x == 13.0 && origin.y == 7.0 && origin.z == 10.0
    )));

    for marker in &mut lane.sketch_entities {
        marker.reclassify(SketchInputKind::Arc);
    }
    lane.sketch_entities.extend([
        {
            let marker_id: String = "auxiliary-object-locus".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 2, 2, SketchInputKind::Point);
            constructed_marker.feature_ref = Some("position".into());
            constructed_marker = constructed_marker.with_test_identity(Some(3), None);
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 1.0]);
            constructed_marker.links = None;
            constructed_marker
        },
        {
            let marker_id: String = "auxiliary-anchor".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 3, 3, SketchInputKind::Point);
            constructed_marker.feature_ref = Some("position".into());
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
            constructed_marker.links = None;
            constructed_marker
        },
    ]);
    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            2.1,
            &surfaces,
            None
        )
        .unwrap()
        .expect("object-indexed arc centers form the exact position roster")
        .len(),
        2
    );

    let opposite_side = |id, x| Surface {
        id: SurfaceId::mint(format!("test:model:entity#surface-{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(x, 30.0, 10.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.1,
            )
            .unwrap(),
        )),
        source_object: None,
    };
    surfaces.extend([opposite_side(3, -9.0), opposite_side(4, 13.0)]);
    assert!(marker_pattern_bore_axes(
        &ctx,
        &HoleMarkers::new(&ctx, &lane).unwrap(),
        "position",
        2.1,
        &surfaces,
        None
    )
    .unwrap()
    .is_none());
    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .unwrap()
        .expect("required invariant")
        .len(),
        2
    );

    let mut opposite = surface(5, -9.0);
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
        &mut opposite.geometry
    else {
        unreachable!();
    };
    let origin = cylinder_surface.origin();
    let ref_direction = cylinder_surface.frame().reference().as_raw();
    let radius = cylinder_surface.radius().get();

    let axis = Vector3::new(0.0, 0.0, -1.0);
    *cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
        *origin,
        axis,
        *ref_direction,
        radius,
    )
    .unwrap();
    surfaces.push(opposite);
    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .unwrap()
        .expect("required invariant")
        .len(),
        2
    );
}

#[test]
fn curve_markers_can_contain_unmatched_construction_loci() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane();
    lane.sketch_entities = [[-0.07, 0.011], [0.07, 0.011], [0.0, -0.004], [0.0, 0.011]]
        .into_iter()
        .enumerate()
        .map(|(ordinal, coordinates_m)| {
            let marker_id: String = format!("curve-marker-{ordinal}");
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                u32::try_from(ordinal).expect("ordinal fits u32"),
                cadmpeg_core::decode::u64_from_index(ordinal),
                SketchInputKind::Arc,
            );
            constructed_marker.feature_ref = Some("position".into());
            constructed_marker = constructed_marker.with_test_identity(
                Some(u32::try_from(ordinal + 1).expect("ordinal fits u32")),
                None,
            );
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates_m);
            constructed_marker.links = None;
            constructed_marker
        })
        .collect();
    let surfaces = [-70.0, 70.0]
        .into_iter()
        .enumerate()
        .map(|(id, x)| Surface {
            id: SurfaceId::mint(format!("test:model:entity#carrier-{id}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(x, 11.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    3.0,
                )
                .unwrap(),
            )),
            source_object: None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            3.0,
            &surfaces,
            None
        )
        .unwrap(),
        Some(vec![
            HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(-70.0, 11.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .unwrap(),
            },
            HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(70.0, 11.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .unwrap(),
            },
        ])
    );
}

#[test]
fn paired_object_loci_select_a_congruent_bore_pattern() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let marker = |id: &str, ordinal, object_index, kind, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            ordinal,
            u64::from(ordinal) * 10,
            kind,
        );
        constructed_marker.feature_ref = Some("position".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let mut lane = lane();
    lane.sketch_entities = vec![
        marker(
            "first",
            0,
            Some(1),
            SketchInputKind::Arc,
            Some([0.013, 0.0]),
        ),
        marker(
            "first-origin",
            1,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "second",
            2,
            Some(2),
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
            Some([-0.009, 0.0]),
        ),
        marker(
            "second-origin",
            3,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "auxiliary",
            4,
            Some(3),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate",
            5,
            Some(4),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate-origin",
            6,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
    ];

    let markers = HoleMarkers::new(&ctx, &lane).unwrap();
    let paired = ctx
        .get_hash_map(&markers.paired, "position", "test paired roster")
        .unwrap()
        .unwrap()
        .iter()
        .map(|(marker, _)| marker.id())
        .collect::<Vec<_>>();
    assert_eq!(paired, ["first", "second", "paired-duplicate"]);

    let mut surfaces = vec![cylinder(0, -9.0), cylinder(1, 13.0), cylinder(2, 100.0)];
    let placements = marker_pattern_bore_axes(
        &ctx,
        &HoleMarkers::new(&ctx, &lane).unwrap(),
        "position",
        2.0,
        &surfaces,
        None,
    )
    .unwrap()
    .expect("unique congruent pattern");
    assert_eq!(placements.len(), 2);

    let opposite = surfaces[..2]
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, mut surface)| {
            surface.id = SurfaceId::mint(format!("test:model:entity#opposite-{index}"))
                .expect("identity grammar");
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
                &mut surface.geometry
            else {
                unreachable!();
            };
            let origin = cylinder_surface.origin();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            let mut origin = *origin;

            origin.z = 20.0;
            let axis = Vector3::new(0.0, 0.0, -1.0);
            *cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                origin,
                axis,
                *ref_direction,
                radius,
            )
            .unwrap();
            surface
        })
        .collect::<Vec<_>>();
    surfaces.extend(opposite);
    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            2.0,
            &surfaces,
            None
        )
        .unwrap()
        .expect("unoriented coincident axes")
        .len(),
        2
    );

    surfaces.push(Surface {
        id: SurfaceId::mint("test:model:entity#duplicate-locus-bore").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(1000.0, 1000.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    assert_eq!(
        marker_pattern_bore_axes(
            &ctx,
            &HoleMarkers::new(&ctx, &lane).unwrap(),
            "position",
            2.0,
            &surfaces,
            None
        )
        .unwrap()
        .expect("complete paired roster takes precedence")
        .len(),
        3
    );
}
