// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::{split_neutral_component_shells, BrepComponentTopology, FaceRegionKey};

fn topology_result(two_faces: bool) -> BrepComponentTopology {
    let first_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }],
    );
    let second_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(6),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::One,
        }],
    );
    let mut eligible_faces = BTreeMap::from([(5.into(), vec![&first_loop])]);
    if two_faces {
        eligible_faces.insert(6.into(), vec![&second_loop]);
    }
    let faces: Vec<_> = if two_faces {
        vec![5.into(), 6.into()]
    } else {
        vec![5.into()]
    };
    BrepComponentTopology::from_component(
        &BTreeSet::from([10, 11]),
        &BTreeSet::from([10]),
        &faces,
        &eligible_faces,
        &BTreeMap::from([(10, [1, 2])]),
    )
}

#[test]
fn brep_component_topology_preserves_service_incidence() {
    let topology = topology_result(true);
    assert_eq!(topology.component_face_curves, BTreeSet::from([10]));
    assert_eq!(topology.wire_curves, BTreeSet::from([11]));
    assert_eq!(
        topology.face_adjacency[&5.into()],
        BTreeSet::from([6.into()])
    );
    assert_eq!(
        topology.face_adjacency[&6.into()],
        BTreeSet::from([5.into()])
    );
    assert_eq!(topology.face_vertices[&5.into()], BTreeSet::from([1, 2]));
    assert_eq!(topology.face_vertices[&6.into()], BTreeSet::from([1, 2]));
}

#[test]
fn disconnected_regions_of_one_source_face_remain_separate_shells() {
    use crate::topology::{HalfEdgeId, Side};

    let edge_groups = [[10, 11, 12], [13, 14, 15]].map(|group| {
        group.map(|curve_id| HalfEdgeId {
            curve_id,
            side: Side::Zero,
        })
    });
    let keys = edge_groups.map(|group| FaceRegionKey {
        source: 5,
        outer: Some(group[0]),
    });
    let loops = edge_groups.map(|group| {
        crate::test_support::closed_loop(std::num::NonZeroU32::new(5), group.to_vec())
    });
    let eligible = BTreeMap::from([(keys[0], vec![&loops[0]]), (keys[1], vec![&loops[1]])]);
    let curves = BTreeSet::from([10, 11, 12, 13, 14, 15]);
    let vertices = BTreeMap::from([
        (10, [1, 2]),
        (11, [2, 3]),
        (12, [3, 1]),
        (13, [4, 5]),
        (14, [5, 6]),
        (15, [6, 4]),
    ]);
    let topology =
        BrepComponentTopology::from_component(&curves, &curves, &keys, &eligible, &vertices);
    assert!(topology.face_adjacency[&keys[0]].is_empty());
    assert!(topology.face_adjacency[&keys[1]].is_empty());
    let shells = split_neutral_component_shells(
        &keys,
        &topology.wire_curves,
        &topology.face_adjacency,
        &topology.face_vertices,
        &vertices,
    );
    assert_eq!(shells.len(), 2);
    assert_eq!(shells[0].faces, [keys[0]]);
    assert_eq!(shells[1].faces, [keys[1]]);
}

#[test]
fn unresolved_boundary_excludes_its_faces_and_keeps_other_faces() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut scan = crate::test_support::empty_container_scan();
    scan.topology.face_components.push(
        crate::topology::FaceComponent::new_for_test(vec![5, 6, 7], vec![10, 11])
            .expect("connected faces"),
    );
    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 5,
            body: Vec::new(),
            slots: [None; 12],
            layout: Some(crate::scalar::PlaneSupportFrameLayout::CompactRotation),
            classification: crate::surface::LocalSystemClassification::Unclassified,
            row_offset: 0,
            offset: 0,
        });
    let loops = [crate::topology::Side::Zero, crate::topology::Side::One]
        .into_iter()
        .zip([5, 6])
        .map(|(side, face)| {
            crate::test_support::closed_loop(
                std::num::NonZeroU32::new(face),
                vec![crate::topology::HalfEdgeId { curve_id: 10, side }],
            )
        })
        .collect::<Vec<_>>();
    let independent_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(7),
        vec![crate::topology::HalfEdgeId {
            curve_id: 11,
            side: crate::topology::Side::Zero,
        }],
    );
    for resolved in [false, true] {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.model.curves.push(Curve {
            id: cadmpeg_ir::ids::CurveId::compose(&crate::identity::VISIBGEOM_CURVE, 10),
            geometry: CurveGeometry::Solved(if resolved {
                SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0).unit().expect("unit direction"),
                    )
                    .expect("line carrier"),
                )
            } else {
                SolvedCurveGeometry::Unknown { record: None }
            }),
            source_object: None,
            parameter_range: None,
        });
        ir.model.curves.push(Curve {
            id: cadmpeg_ir::ids::CurveId::compose(&crate::identity::VISIBGEOM_CURVE, 11),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0).unit().expect("unit direction"),
                )
                .expect("line carrier"),
            )),
            source_object: None,
            parameter_range: None,
        });
        let mut faces = BTreeMap::from([
            (5, vec![&loops[0]]),
            (6, vec![&loops[1]]),
            (7, vec![&independent_loop]),
        ]);
        let mut losses = Vec::new();
        super::super::retain_complete_compact_frame_boundary_components(
            &scan,
            &mut faces,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &mut losses,
        );
        if resolved {
            assert_eq!(faces.len(), 3);
            assert!(losses.is_empty());
        } else {
            assert_eq!(faces.keys().copied().collect::<Vec<_>>(), [7]);
            assert_eq!(losses.len(), 2);
            for loss in &losses {
                assert_eq!(
                    loss.code,
                    crate::loss::CreoLossCode::BrepCompactFrameBoundaryUnresolved.kind()
                );
            }
        }
    }
}
