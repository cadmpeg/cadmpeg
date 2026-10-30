// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::BrepComponentTopology;

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
    let mut eligible_faces = BTreeMap::from([(5, vec![&first_loop])]);
    if two_faces {
        eligible_faces.insert(6, vec![&second_loop]);
    }
    let faces: &[u32] = if two_faces { &[5, 6] } else { &[5] };
    BrepComponentTopology::from_component(
        &BTreeSet::from([10, 11]),
        &BTreeSet::from([10]),
        faces,
        &eligible_faces,
        &BTreeMap::from([(10, [1, 2])]),
    )
}

#[test]
fn brep_component_topology_preserves_service_incidence() {
    let topology = topology_result(true);
    assert_eq!(topology.component_face_curves, BTreeSet::from([10]));
    assert_eq!(topology.wire_curves, BTreeSet::from([11]));
    assert_eq!(topology.face_adjacency[&5], BTreeSet::from([6]));
    assert_eq!(topology.face_adjacency[&6], BTreeSet::from([5]));
    assert_eq!(topology.face_vertices[&5], BTreeSet::from([1, 2]));
    assert_eq!(topology.face_vertices[&6], BTreeSet::from([1, 2]));
}

#[test]
fn compact_component_requires_every_boundary_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut scan = crate::test_support::empty_container_scan();
    scan.topology.face_components.push(
        crate::topology::FaceComponent::new_for_test(vec![5, 6], vec![10])
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
        let mut faces = BTreeMap::from([(5, vec![&loops[0]]), (6, vec![&loops[1]])]);
        let mut losses = Vec::new();
        super::super::retain_complete_compact_frame_boundary_components(
            &scan,
            &mut faces,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &mut losses,
        );
        if resolved {
            assert_eq!(faces.len(), 2);
            assert!(losses.is_empty());
        } else {
            assert!(faces.is_empty());
            assert_eq!(losses.len(), 2);
            for (loss, face) in losses.iter().zip([5, 6]) {
                assert_eq!(
                    loss.code,
                    crate::loss::CreoLossCode::BrepCompactFrameBoundaryUnresolved.kind()
                );
                assert!(loss.message.contains(&format!("source face {face}")));
                assert!(loss.message.contains("10"));
            }
        }
    }
}
