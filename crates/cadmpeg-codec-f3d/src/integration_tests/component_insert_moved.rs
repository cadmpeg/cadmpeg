// SPDX-License-Identifier: Apache-2.0
//! A Component Insert whose occurrence a joint moved after the insertion.

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use super::decode;
use crate::loss::F3dLossCode;

const EPS_POSE_MM: f64 = 1.0e-9;

#[test]
fn component_insert_binds_the_occurrence_a_joint_moved() {
    let result = decode(
        include_bytes!("../../../../corpus/f3d/component_insert_moved_by_joint.f3z").to_vec(),
    );
    let ir = result.ir();
    // The plate and three screws; the insert adds no unresolved occurrence of its own.
    assert_eq!(ir.model.occurrences.len(), 4);
    assert!(ir.model.occurrences.iter().all(|occurrence| occurrence
        .native_ref
        .as_deref()
        .is_some_and(|r| r.starts_with("f3d:xref:"))));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::FeatureComponentInsertMembershipAssumed.kind()));
    let insert = ir
        .model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Component Insert"))
        .unwrap();
    let plate = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.native_ref.as_deref() == Some("f3d:xref:reference#1-occurrence-0")
        })
        .unwrap();
    assert_eq!(
        insert.evaluation.definition(),
        &FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: plate.id.clone(),
        })
    );
    // The joint's pose, not the insertion matrix (5 mm along y): the plate's
    // corner at x = 1.0425 mm in its own design sits on the origin.
    let rows = plate.transform.unwrap().rows();
    assert!(
        (rows[0][3] + 1.042_546_720_383_512_6).abs() < EPS_POSE_MM,
        "{rows:?}"
    );
    assert!(
        rows[1][3].abs() < EPS_POSE_MM && rows[2][3].abs() < EPS_POSE_MM,
        "{rows:?}"
    );
    // The screws stay where their Screw features seat them.
    let mut screws = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.id != plate.id)
        .map(|occurrence| {
            let rows = occurrence.transform.unwrap().rows();
            [rows[0][3], rows[1][3], rows[2][3]]
        })
        .collect::<Vec<_>>();
    screws.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for (got, want) in
        screws
            .iter()
            .zip([[10.0, 10.0, 10.0], [30.0, 30.0, 10.0], [50.0, 10.0, 10.0]])
    {
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < EPS_POSE_MM, "{screws:?}");
        }
    }
}
