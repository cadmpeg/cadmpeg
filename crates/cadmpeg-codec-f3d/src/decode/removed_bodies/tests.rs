// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, DistinctMembers, Feature, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};

fn body(id: &str) -> Body {
    Body {
        id: BodyId::mint(id).unwrap(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
        removed_by: None,
    }
}

fn remove(id: &str, bodies: &[&str], mode: BodyRetentionMode, suppressed: Option<bool>) -> Feature {
    let bodies = DistinctMembers::try_from(
        bodies
            .iter()
            .map(|body| BodyId::mint(*body).unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::DeleteBody {
                bodies: BodySelection::Bodies(bodies),
                mode,
            },
        )),
        native_ref: None,
    }
}

fn marks(features: Vec<Feature>) -> Vec<Option<String>> {
    let mut model = cadmpeg_ir::document::Model::default();
    model.bodies = vec![body("f3d:brep:entity#1"), body("f3d:brep:entity#2")];
    model.features = features;
    super::mark(&mut model);
    model
        .bodies
        .iter()
        .map(|body| body.removed_by.as_ref().map(|id| id.as_str().to_owned()))
        .collect()
}

#[test]
fn an_active_remove_marks_the_bodies_it_deletes() {
    assert_eq!(
        marks(vec![remove(
            "f3d:model:feature#remove",
            &["f3d:brep:entity#2"],
            BodyRetentionMode::DeleteSelected,
            Some(false),
        )]),
        [None, Some("f3d:model:feature#remove".to_owned())]
    );
}

#[test]
fn a_suppressed_or_keep_selected_remove_marks_nothing() {
    for (mode, suppressed) in [
        (BodyRetentionMode::DeleteSelected, Some(true)),
        (BodyRetentionMode::KeepSelected, None),
        (BodyRetentionMode::Unresolved, None),
    ] {
        assert_eq!(
            marks(vec![remove(
                "f3d:model:feature#remove",
                &["f3d:brep:entity#2"],
                mode,
                suppressed,
            )]),
            [None, None]
        );
    }
}

#[test]
fn a_body_that_two_removes_name_keeps_no_mark() {
    assert_eq!(
        marks(vec![
            remove(
                "f3d:model:feature#first",
                &["f3d:brep:entity#2"],
                BodyRetentionMode::DeleteSelected,
                None,
            ),
            remove(
                "f3d:model:feature#second",
                &["f3d:brep:entity#2"],
                BodyRetentionMode::DeleteSelected,
                None,
            ),
        ]),
        [None, None]
    );
}
