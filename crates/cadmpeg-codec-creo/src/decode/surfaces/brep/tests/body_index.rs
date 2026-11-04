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
    let eligible_faces = BTreeMap::from([(5, Vec::new())]);
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
            faces: vec![5],
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
fn compact_frame_component_requires_every_source_face_and_reports_refusal() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.topology.face_components.push(
        crate::topology::FaceComponent::new_for_test(vec![5, 6], vec![10])
            .expect("connected source component"),
    );
    let mut faces = BTreeMap::from([(5, Vec::new())]);
    let mut losses = Vec::new();
    super::super::retain_complete_compact_frame_components(&scan, &mut faces, &mut losses);
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
    let mut complete = BTreeMap::from([(5, Vec::new()), (6, Vec::new())]);
    super::super::retain_complete_compact_frame_components(&scan, &mut complete, &mut losses);
    assert_eq!(complete.len(), 2);
    assert!(losses.is_empty());
    super::super::retain_complete_compact_frame_components(&scan, &mut faces, &mut losses);
    assert!(faces.is_empty());
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
                faces: vec![1, 2],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: vec![5],
                wire_curves: BTreeSet::from([20]),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::new(),
            },
        ];
        super::super::surface_owned_body_components(&scan, &admitted, &specs, Some(selected))
    };
    let body = SurfaceOwner::Body(368);
    let grouped = grouping(&[(1, body), (2, body)], 4).expect("one owning body");
    assert_eq!(
        grouped.bodies,
        vec![NeutralShellSpec {
            faces: vec![1, 2],
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
