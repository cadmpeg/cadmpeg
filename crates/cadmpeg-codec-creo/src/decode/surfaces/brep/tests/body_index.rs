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
