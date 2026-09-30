//! Omitted point-coordinate inference from distance constraints.

use super::super::inferred_point_coordinates_by_index;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use std::collections::HashMap;

fn lane(constraints: &[([u16; 2], f64)]) -> FeatureInputLane {
    let markers = [0.0, 1.0, 2.0]
        .into_iter()
        .enumerate()
        .map(|(ordinal, u)| {
            let mut marker = SketchInputEntity::new(
                format!("point-{ordinal}"),
                "lane",
                u32::try_from(ordinal).unwrap(),
                u64::try_from(ordinal).unwrap(),
                SketchInputKind::Point,
            );
            marker.feature_ref = Some("feature".into());
            marker.coordinates_m = FiniteVector::new([u, 0.0]);
            marker
        })
        .collect();
    let scalars = constraints
        .iter()
        .enumerate()
        .map(|(ordinal, (indices, distance))| FeatureInputScalar {
            id: format!("distance-{ordinal}"),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: u32::try_from(ordinal).unwrap(),
            offset: u64::try_from(ordinal).unwrap(),
            object_id: 0,
            name: "distance".into(),
            value: FiniteReal::new(*distance).unwrap(),
            role: FeatureInputScalarRole::Driving,
            operands: indices
                .iter()
                .map(|index| FeatureInputOperand {
                    offset: 0,
                    reference_ref: format!("reference-{index}"),
                    kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
                    entity_index: *index,
                    entity_ref: None,
                })
                .collect(),
        })
        .collect();
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars,
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    }
}

#[test]
fn omitted_point_coordinates_keep_unique_point_with_ambiguous_assignments() {
    // A distance-two pair must use 0 and 2; its shared distance-one neighbor is 1.
    let lane = lane(&[([10, 11], 1.0), ([10, 12], 1.0), ([11, 12], 2.0)]);
    let inferred = inferred_point_coordinates_by_index(
        &cadmpeg_test_support::service_decode_context(),
        &lane,
        "feature",
    )
    .unwrap();
    assert_eq!(inferred, HashMap::from([(10, [1.0, 0.0])]));
}

#[test]
fn omitted_point_coordinates_reject_inconsistent_component_after_domain_filtering() {
    // Three points cannot all be distance two apart in the candidate set {0,1,2}.
    // Domain filtering leaves the shared distance-one neighbor at 1, but no
    // assignment satisfies the complete component.
    let lane = lane(&[
        ([10, 11], 1.0),
        ([10, 12], 1.0),
        ([10, 13], 1.0),
        ([11, 12], 2.0),
        ([11, 13], 2.0),
        ([12, 13], 2.0),
    ]);
    let inferred = inferred_point_coordinates_by_index(
        &cadmpeg_test_support::service_decode_context(),
        &lane,
        "feature",
    )
    .unwrap();
    assert!(inferred.is_empty());
}
