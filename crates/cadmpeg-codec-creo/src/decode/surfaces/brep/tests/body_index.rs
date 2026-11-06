// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::{used_brep_vertices, BrepBodyIndexes, NeutralShellSpec};

fn index_result() -> BrepBodyIndexes {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    let component = crate::topology::FaceComponent::new_for_test(vec![5], vec![10])
        .expect("valid component fixture");
    let admitted_components = [&component];
    let admitted_edge_curves = BTreeSet::from([10]);
    let eligible_faces = BTreeMap::from([(5.into(), Vec::new())]);
    let eligible_face_ids = BTreeSet::from([5]);
    let curve_faces = BTreeMap::new();
    BrepBodyIndexes::from_components(
        &scan,
        &admitted_components,
        &admitted_edge_curves,
        &eligible_faces,
        &eligible_face_ids,
        &curve_faces,
    )
}

#[test]
fn brep_body_indexes_preserve_service_values() {
    let indexes = index_result();
    assert_eq!(indexes.neutral_edge_curves, BTreeSet::from([10]));
    assert_eq!(
        indexes.body_components,
        vec![NeutralShellSpec {
            faces: vec![5.into()],
            wire_curves: BTreeSet::from([10]),
        }]
    );
    let vertices = {
        used_brep_vertices(
            &indexes.neutral_edge_curves,
            &BTreeMap::from([(10, [1, 2])]),
        )
    };
    assert_eq!(vertices, BTreeSet::from([1, 2]));
}

#[test]
fn compact_frame_component_retains_proved_faces_and_reports_missing_faces() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.topology.face_components.push(
        crate::topology::FaceComponent::new_for_test(vec![5, 6], vec![10])
            .expect("connected source component"),
    );
    let faces = BTreeMap::from([(5, Vec::new())]);
    let mut losses = Vec::new();
    super::super::report_incomplete_compact_frame_components(&scan, &faces, &mut losses);
    assert!(faces.contains_key(&5));
    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 5,
            body: Vec::new(),
            slots: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 4.0, 8.0].map(Some),
            layout: Some(crate::scalar::PlaneSupportFrameLayout::CompactRotation),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 0,
            offset: 0,
        });
    let complete = BTreeMap::from([(5, Vec::new()), (6, Vec::new())]);
    super::super::report_incomplete_compact_frame_components(&scan, &complete, &mut losses);
    assert_eq!(complete.len(), 2);
    assert!(losses.is_empty());
    super::super::report_incomplete_compact_frame_components(&scan, &faces, &mut losses);
    assert!(faces.contains_key(&5));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::BrepCompactFrameComponentUnresolved.kind()
    );
}

#[test]
fn surface_owners_group_components_and_leave_unowned_ones_native() {
    use crate::surface::owners::{SurfaceOwner, SurfaceRowOwner};

    let grouping = |owners: &[(u32, SurfaceOwner)], selected: usize| {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.layout = crate::container::Layout::Nd;
        for (index, (surface_id, owner)) in owners.iter().enumerate() {
            scan.surfaces.rows.push(crate::surface::SurfaceRow {
                id: *surface_id,
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 4,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: index,
            });
            scan.surfaces.row_owners.push(SurfaceRowOwner {
                surface_id: *surface_id,
                owner: *owner,
                offset: index,
            });
        }
        let components = [
            crate::topology::FaceComponent::new_for_test(vec![1, 2], vec![10, 11])
                .expect("owned component"),
            crate::topology::FaceComponent::new_for_test(vec![5, 6], vec![20])
                .expect("unowned component"),
            crate::topology::FaceComponent::new_for_test(vec![7], vec![30])
                .expect("unowned empty component"),
        ];
        let admitted: Vec<_> = components.iter().collect();
        let specs = [
            NeutralShellSpec {
                faces: vec![1.into(), 2.into()],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: vec![5.into()],
                wire_curves: BTreeSet::from([20]),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::new(),
            },
        ];
        super::super::surface_owned_body_components(
            &scan,
            &admitted,
            &specs,
            Some(selected),
            &BTreeSet::new(),
        )
    };
    let body = SurfaceOwner::Body(368);
    let grouped = grouping(&[(1, body), (2, body)], 4).expect("one owning body");
    assert_eq!(
        grouped.bodies,
        vec![NeutralShellSpec {
            faces: vec![1.into(), 2.into()],
            wire_curves: BTreeSet::new(),
        }]
    );
    assert_eq!(grouped.unowned_faces, vec![5]);
    let losses: Vec<_> = grouped.unowned_face_losses().collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::TopologyUnownedComponent.kind()
    );
    assert!(losses[0].message.contains("source face 5 "));
    assert_eq!(grouped.owned_curves, BTreeSet::from([10, 11]));
    assert!(grouping(&[(1, body), (2, body)], 0).is_none());
    assert!(grouping(&[(1, body)], 4).is_none());
    assert!(grouping(&[(1, body), (2, SurfaceOwner::Body(425))], 4).is_none());
    assert!(grouping(&[(1, body), (2, SurfaceOwner::Quilt(808))], 4).is_none());
    assert!(grouping(&[(1, body), (2, SurfaceOwner::Unowned)], 4).is_none());
    assert!(grouping(&[(1, body), (2, body), (5, body)], 4).is_none());
}

#[test]
fn owner_grouping_skips_previously_excluded_null_owned_faces() {
    use crate::surface::owners::{SurfaceOwner, SurfaceRowOwner};
    use crate::topology::FaceComponent;

    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    for (index, (surface_id, owner)) in [(1, SurfaceOwner::Body(368)), (2, SurfaceOwner::Unowned)]
        .into_iter()
        .enumerate()
    {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: surface_id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: index,
        });
        scan.surfaces.row_owners.push(SurfaceRowOwner {
            surface_id,
            owner,
            offset: index,
        });
    }
    let component =
        FaceComponent::new_for_test(vec![1, 2], vec![10, 11]).expect("mixed-owner component");
    let grouped = super::super::surface_owned_body_components(
        &scan,
        &[&component],
        &[NeutralShellSpec {
            faces: vec![super::super::FaceRegionKey {
                source: 1,
                outer: None,
            }],
            wire_curves: BTreeSet::from([10]),
        }],
        Some(1),
        &[2].into(),
    )
    .expect("body-owned component after null-owned face removal");
    assert_eq!(
        grouped.bodies,
        [NeutralShellSpec {
            faces: vec![super::super::FaceRegionKey {
                source: 1,
                outer: None,
            }],
            wire_curves: BTreeSet::from([10]),
        }]
    );
}

#[test]
fn explicit_null_owners_join_visible_rows_by_identifier_and_offset() {
    use crate::surface::owners::{SurfaceOwner, SurfaceRowOwner};

    let mut scan = crate::test_support::empty_container_scan();
    for (id, offset) in [(1, 10), (2, 20), (3, 30), (3, 31), (4, 40), (5, 50)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset,
        });
    }
    for (id, offset, owner) in [
        (1, 10, SurfaceOwner::Unowned),
        (2, 21, SurfaceOwner::Unowned),
        (3, 30, SurfaceOwner::Unowned),
        (3, 31, SurfaceOwner::Unowned),
        (4, 40, SurfaceOwner::Body(368)),
    ] {
        scan.surfaces.row_owners.push(SurfaceRowOwner {
            surface_id: id,
            owner,
            offset,
        });
    }
    assert_eq!(
        super::super::explicit_unowned_faces(&scan, &[1, 2, 3, 4, 5].into()),
        [1].into()
    );
}

#[test]
fn null_owned_faces_and_their_exclusive_curves_leave_body_topology() {
    use crate::surface::owners::{SurfaceOwner, SurfaceRowOwner};
    use crate::topology::FaceComponent;

    let mut scan = crate::test_support::empty_container_scan();
    let loops = (1..=4)
        .map(|face| {
            crate::test_support::closed_loop(
                std::num::NonZeroU32::new(face),
                vec![crate::topology::HalfEdgeId {
                    curve_id: face + 9,
                    side: crate::topology::Side::Zero,
                }],
            )
        })
        .collect::<Vec<_>>();
    for (index, (face_id, owner)) in [
        (1, SurfaceOwner::Body(368)),
        (2, SurfaceOwner::Unowned),
        (3, SurfaceOwner::Quilt(808)),
        (4, SurfaceOwner::Unowned),
    ]
    .into_iter()
    .enumerate()
    {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: face_id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: index,
        });
        scan.surfaces.row_owners.push(SurfaceRowOwner {
            surface_id: face_id,
            owner,
            offset: index,
        });
    }
    let components = [
        FaceComponent::new_for_test(vec![1, 2, 3], vec![10, 11, 12])
            .expect("mixed native component"),
        FaceComponent::new_for_test(vec![4], vec![13]).expect("unowned-only component"),
    ];
    let mut admitted: Vec<_> = components.iter().collect();
    let mut eligible_faces = loops
        .iter()
        .zip(1_u32..)
        .map(|(lp, face_id)| (face_id, vec![lp]))
        .collect();
    let mut face_regions = BTreeMap::from([(2, vec![vec![&loops[1]]]), (4, vec![vec![&loops[3]]])]);
    let mut losses = Vec::new();
    let (unowned_faces, unowned_loop_curves, body_loop_curves) =
        super::super::exclude_explicit_unowned_faces(
            &scan,
            &mut eligible_faces,
            &mut face_regions,
            &mut losses,
        );
    assert_eq!(unowned_faces, BTreeSet::from([2, 4]));
    assert_eq!(eligible_faces.keys().copied().collect::<Vec<_>>(), [1, 3]);
    assert!(face_regions.is_empty());
    assert_eq!(unowned_loop_curves, BTreeSet::from([11, 13]));
    assert_eq!(body_loop_curves, BTreeSet::from([10, 12]));
    assert_eq!(losses.len(), 2);
    assert!(losses.iter().all(|loss| {
        loss.code == crate::loss::CreoLossCode::TopologyExplicitUnownedSurface.kind()
    }));
    let face = |source| super::super::FaceRegionKey {
        source,
        outer: None,
    };
    let mut specs = vec![
        NeutralShellSpec {
            faces: vec![face(1), face(3)],
            wire_curves: BTreeSet::from([10, 11, 12, 13]),
        },
        NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: BTreeSet::from([13]),
        },
    ];
    let mut curves = BTreeSet::from([10, 11, 12, 13]);
    let curve_faces = BTreeMap::from([(10, [2, 2]), (11, [1, 2]), (12, [1, 0]), (13, [4, 4])]);
    super::super::retain_body_topology(
        &mut admitted,
        &mut specs,
        &mut curves,
        &curve_faces,
        &unowned_loop_curves,
        &body_loop_curves,
        &unowned_faces,
    );
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs,
        [NeutralShellSpec {
            faces: vec![face(1), face(3)],
            wire_curves: BTreeSet::from([10, 12])
        }]
    );
    assert_eq!(curves, BTreeSet::from([10, 12]));
}

#[test]
fn explicit_unowned_component_keeps_distinct_wires_and_unresolved_components() {
    use crate::topology::FaceComponent;

    let components = [
        FaceComponent::new_for_test(vec![1], vec![10]).expect("owned component"),
        FaceComponent::new_for_test(vec![2], vec![20]).expect("unowned wire component"),
        FaceComponent::new_for_test(vec![3], vec![30]).expect("unresolved component"),
    ];
    let mut admitted: Vec<_> = components.iter().collect();
    let mut specs = vec![
        NeutralShellSpec {
            faces: vec![super::super::FaceRegionKey {
                source: 1,
                outer: None,
            }],
            wire_curves: BTreeSet::new(),
        },
        NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: BTreeSet::from([20]),
        },
        NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: BTreeSet::new(),
        },
    ];
    let mut curves = BTreeSet::from([20]);
    super::super::retain_body_topology(
        &mut admitted,
        &mut specs,
        &mut curves,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &[2].into(),
    );
    assert_eq!(admitted.len(), 3);
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[1].wire_curves, BTreeSet::from([20]));
    assert!(specs[2].faces.is_empty());
    assert!(specs[2].wire_curves.is_empty());
    assert_eq!(curves, BTreeSet::from([20]));
}

#[test]
fn a_retained_body_loop_keeps_its_curve_when_owner_sides_are_null() {
    let mut specs = vec![NeutralShellSpec {
        faces: vec![super::super::FaceRegionKey {
            source: 1,
            outer: None,
        }],
        wire_curves: BTreeSet::new(),
    }];
    let mut curves = BTreeSet::from([10]);
    let component =
        crate::topology::FaceComponent::new_for_test(vec![1, 2], vec![10]).expect("face component");
    let mut admitted = vec![&component];
    super::super::retain_body_topology(
        &mut admitted,
        &mut specs,
        &mut curves,
        &BTreeMap::from([(10, [2, 2])]),
        &[10].into(),
        &[10].into(),
        &[2].into(),
    );
    assert_eq!(curves, BTreeSet::from([10]));
    assert_eq!(specs[0].faces.len(), 1);
}

#[test]
fn an_unowned_loop_removes_its_curve_when_side_incidence_is_missing() {
    let mut specs = vec![NeutralShellSpec {
        faces: vec![super::super::FaceRegionKey {
            source: 1,
            outer: None,
        }],
        wire_curves: BTreeSet::from([10]),
    }];
    let mut curves = BTreeSet::from([10]);
    let component =
        crate::topology::FaceComponent::new_for_test(vec![1, 2], vec![10]).expect("face component");
    let mut admitted = vec![&component];
    super::super::retain_body_topology(
        &mut admitted,
        &mut specs,
        &mut curves,
        &BTreeMap::new(),
        &[10].into(),
        &BTreeSet::new(),
        &[2].into(),
    );
    assert!(specs[0].wire_curves.is_empty());
    assert!(curves.is_empty());
}
