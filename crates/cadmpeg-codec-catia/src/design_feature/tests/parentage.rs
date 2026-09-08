// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn assigns_parent_from_an_exact_transferred_owner_chain() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("child-object", Some("parent-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir.model.feature_parent(&ir.model.features[0].id).is_none());
    assert_eq!(
        ir.model.feature_parent(&ir.model.features[1].id),
        Some(&FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"))
    );
}

#[test]
fn assigns_parent_from_the_nearest_transferred_ancestor() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("group-object", Some("parent-object")),
            design_object("child-object", Some("group-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert_eq!(
        ir.model.feature_parent(&ir.model.features[1].id),
        Some(&FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"))
    );
}

#[test]
fn rejects_a_parent_that_does_not_precede_its_child() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("child-object", Some("parent-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 20;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 10;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| ir.model.feature_parent(&feature.id).is_none()));
}

#[test]
fn does_not_assign_a_self_parent() {
    let native = CatiaNative {
        design_objects: vec![design_object("feature-object", Some("feature-object"))],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    ir.model.features.push(feature("feature", "feature-object"));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([(
            "feature-object".to_string(),
            FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        )]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir.model.feature_parent(&ir.model.features[0].id).is_none());
}

#[test]
fn omits_all_parents_in_an_owner_cycle() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("first-object", Some("second-object")),
            design_object("second-object", Some("first-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    ir.model
        .features
        .push(feature("first-feature", "first-object"));
    ir.model
        .features
        .push(feature("second-feature", "second-object"));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "first-object".to_string(),
                FeatureId::mint("synthetic:test:id#first-feature").expect("identity grammar"),
            ),
            (
                "second-object".to_string(),
                FeatureId::mint("synthetic:test:id#second-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| ir.model.feature_parent(&feature.id).is_none()));
}
