use super::owned_relation_parameters;
use crate::records::relation_scalars::RelationScalars;
use crate::records::{FeatureInputLane, FeatureInputRelationFamily, FeatureInputRelationInstance};
use cadmpeg_ir::features::ParameterId;
use std::collections::HashMap;

pub(super) fn relation_lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalars: RelationScalars::from_refs(vec!["scalar".into()], Some("scalar".into()), None)
                .unwrap(),
            operands: Vec::new(),
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn project_with_policy() -> HashMap<String, Option<ParameterId>> {
    let lane = relation_lane();
    owned_relation_parameters(&[], &[], &[lane])
}

#[test]
fn relation_ownership_keeps_unmatched_driver_unowned() {
    let ownership = project_with_policy();
    assert_eq!(ownership.get("relation"), Some(&None));
}
