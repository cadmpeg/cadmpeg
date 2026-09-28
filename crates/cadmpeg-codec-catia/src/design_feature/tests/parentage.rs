// SPDX-License-Identifier: Apache-2.0

use crate::design_feature::tests::design_object;
use crate::design_feature::tests::feature;
use crate::design_feature::DesignFeatureTransfer;
use crate::native::CatiaNative;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use std::collections::HashMap;

#[test]
fn feature_parent_lookup_refuses_collection_limit() {
    let native = CatiaNative {
        design_objects: vec![design_object("synthetic:test:object#one", None)],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    ir.model.features.push(feature("one", "synthetic:test:object#one"));
    let transfer = DesignFeatureTransfer::default();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        transfer.assign_feature_parents(ctx, &mut ir, &native)
    });
    assert!(matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_feature_parent_objects"));
    crate::test_support::with_service_context(|ctx| {
        transfer.assign_feature_parents(ctx, &mut ir, &native)
    }).expect("service profile admits parent indexes");
    assert!(ir.model.feature_parent(&ir.model.features[0].id).is_none());
}

#[test]
fn assigns_parent_from_an_exact_transferred_owner_chain() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("synthetic:test:object#parent-object", None),
            design_object(
                "synthetic:test:object#child-object",
                Some("synthetic:test:object#parent-object"),
            ),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "synthetic:test:object#parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "synthetic:test:object#child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "synthetic:test:object#parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| transfer.assign_feature_parents(ctx, &mut ir, &native)).unwrap();

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
            design_object("synthetic:test:object#parent-object", None),
            design_object(
                "synthetic:test:object#group-object",
                Some("synthetic:test:object#parent-object"),
            ),
            design_object(
                "synthetic:test:object#child-object",
                Some("synthetic:test:object#group-object"),
            ),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "synthetic:test:object#parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "synthetic:test:object#child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "synthetic:test:object#parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| transfer.assign_feature_parents(ctx, &mut ir, &native)).unwrap();

    assert_eq!(
        ir.model.feature_parent(&ir.model.features[1].id),
        Some(&FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"))
    );
}

#[test]
fn rejects_a_parent_that_does_not_precede_its_child() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("synthetic:test:object#parent-object", None),
            design_object(
                "synthetic:test:object#child-object",
                Some("synthetic:test:object#parent-object"),
            ),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    let mut parent_feature = feature("parent-feature", "synthetic:test:object#parent-object");
    parent_feature.ordinal = 20;
    let mut child_feature = feature("child-feature", "synthetic:test:object#child-object");
    child_feature.ordinal = 10;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "synthetic:test:object#parent-object".to_string(),
                FeatureId::mint("synthetic:test:id#parent-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#child-object".to_string(),
                FeatureId::mint("synthetic:test:id#child-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| transfer.assign_feature_parents(ctx, &mut ir, &native)).unwrap();

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| ir.model.feature_parent(&feature.id).is_none()));
}

#[test]
fn does_not_assign_a_self_parent() {
    let native = CatiaNative {
        design_objects: vec![design_object(
            "synthetic:test:object#feature-object",
            Some("synthetic:test:object#feature-object"),
        )],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    ir.model
        .features
        .push(feature("feature", "synthetic:test:object#feature-object"));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([(
            "synthetic:test:object#feature-object".to_string(),
            FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        )]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| transfer.assign_feature_parents(ctx, &mut ir, &native)).unwrap();

    assert!(ir.model.feature_parent(&ir.model.features[0].id).is_none());
}

#[test]
fn omits_all_parents_in_an_owner_cycle() {
    let native = CatiaNative {
        design_objects: vec![
            design_object(
                "synthetic:test:object#first-object",
                Some("synthetic:test:object#second-object"),
            ),
            design_object(
                "synthetic:test:object#second-object",
                Some("synthetic:test:object#first-object"),
            ),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    ir.model.features.push(feature(
        "first-feature",
        "synthetic:test:object#first-object",
    ));
    ir.model.features.push(feature(
        "second-feature",
        "synthetic:test:object#second-object",
    ));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "synthetic:test:object#first-object".to_string(),
                FeatureId::mint("synthetic:test:id#first-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#second-object".to_string(),
                FeatureId::mint("synthetic:test:id#second-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| transfer.assign_feature_parents(ctx, &mut ir, &native)).unwrap();

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| ir.model.feature_parent(&feature.id).is_none()));
}
