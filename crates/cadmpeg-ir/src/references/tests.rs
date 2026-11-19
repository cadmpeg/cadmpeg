// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::{DrawingPart, InstanceTarget, ReferenceEntity, ReferenceSelection, ReferenceTarget};
use crate::attributes::AttributeTarget;
use crate::design::refs::{EntityRef, Evaluated, Member, Position, StateRef};
use crate::drawings::DrawingId;
use crate::features::{ConfigurationId, FeatureId};
use crate::ids::{FaceId, PmiId};
use crate::semantic_annotations::SemanticAnnotationId;

fn face() -> FaceId {
    FaceId::mint("test:model:face#1").unwrap()
}

fn state() -> StateRef {
    StateRef {
        configuration: Some(ConfigurationId::mint("test:design:configuration#large").unwrap()),
        of: Evaluated::State(Position::After(Member::Feature(
            FeatureId::mint("test:design:feature#3").unwrap(),
        ))),
    }
}

fn round_trip<T>(value: &T, wire: serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).unwrap(), wire);
    assert_eq!(&serde_json::from_value::<T>(wire).unwrap(), value);
}

#[test]
fn an_instance_target_writes_its_entity_and_only_the_path_and_state_it_states() {
    round_trip(
        &InstanceTarget::new(AttributeTarget::Face(face())),
        json!({"entity": {"kind": "face", "value": "test:model:face#1"}}),
    );
    round_trip(
        &InstanceTarget {
            entity: AttributeTarget::Document,
            path: None,
            state: Some(state()),
        },
        json!({
            "entity": {"kind": "document"},
            "state": {
                "configuration": "test:design:configuration#large",
                "of": {"kind": "state", "value": {"kind": "after", "value": {"kind": "feature", "value": "test:design:feature#3"}}}
            }
        }),
    );
}

#[test]
fn an_instance_target_refuses_a_second_spelling() {
    for wire in [
        json!({"entity": {"kind": "document"}, "path": null}),
        json!({"entity": {"kind": "document"}, "state": null}),
        json!({"entity": {"kind": "document"}, "extra": 1}),
        json!({"path": []}),
        json!({"entity": {"kind": "document", "value": null}}),
    ] {
        assert!(
            serde_json::from_value::<InstanceTarget<AttributeTarget>>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn a_reference_target_is_adjacently_tagged() {
    let entity = ReferenceTarget::Entity(InstanceTarget::new(ReferenceEntity::Topology(
        EntityRef::Face(face()),
    )));
    let cases = [
        (ReferenceTarget::Null, json!({"kind": "null"})),
        (ReferenceTarget::Unresolved, json!({"kind": "unresolved"})),
        (
            entity,
            json!({"kind": "entity", "value": {"entity": {"kind": "topology", "value": {"kind": "face", "value": "test:model:face#1"}}}}),
        ),
        (
            ReferenceTarget::Feature(FeatureId::mint("test:design:feature#3").unwrap()),
            json!({"kind": "feature", "value": "test:design:feature#3"}),
        ),
        (
            ReferenceTarget::External {
                document: "other.FCStd".into(),
                object: "Box".into(),
            },
            json!({"kind": "external", "value": {"document": "other.FCStd", "object": "Box"}}),
        ),
    ];
    for (target, wire) in cases {
        round_trip(&target, wire);
    }
}

#[test]
fn a_drawing_annotation_or_pmi_target_names_its_record() {
    let drawing = || DrawingId::mint("test:model:drawing#front").unwrap();
    let cases = [
        (
            ReferenceTarget::Drawing {
                drawing: drawing(),
                part: None,
            },
            json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front"}}),
        ),
        (
            ReferenceTarget::Drawing {
                drawing: drawing(),
                part: Some(DrawingPart::Curve(3)),
            },
            json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "part": {"kind": "curve", "value": 3}}}),
        ),
        (
            ReferenceTarget::Drawing {
                drawing: drawing(),
                part: Some(DrawingPart::Unread),
            },
            json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "part": {"kind": "unread"}}}),
        ),
        (
            ReferenceTarget::Annotation(
                SemanticAnnotationId::mint("test:model:semantic-annotation#note").unwrap(),
            ),
            json!({"kind": "annotation", "value": "test:model:semantic-annotation#note"}),
        ),
        (
            ReferenceTarget::Pmi(PmiId::mint("test:model:pmi#flatness").unwrap()),
            json!({"kind": "pmi", "value": "test:model:pmi#flatness"}),
        ),
    ];
    for (target, wire) in cases {
        assert!(!target.reads_design());
        round_trip(&target, wire);
    }
    for wire in [
        json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "part": null}}),
        json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "curve": 3}}),
        json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "part": {"kind": "unread", "value": null}}}),
        json!({"kind": "drawing", "value": {"drawing": "test:model:drawing#front", "part": {"kind": "curve", "value": -1}}}),
        json!({"kind": "drawing", "value": {"part": {"kind": "unread"}}}),
        json!({"kind": "drawing", "value": "test:model:drawing#front"}),
        json!({"kind": "pmi", "value": "bad id"}),
    ] {
        assert!(
            serde_json::from_value::<ReferenceTarget>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn a_reference_target_refuses_a_value_beside_a_bare_tag() {
    for wire in [
        json!({"kind": "null", "value": null}),
        json!({"kind": "unresolved", "value": null}),
        json!({"kind": "unresolved", "value": {}}),
        json!({"kind": "local", "value": "id"}),
        json!({"kind": "external", "document": "d", "object": "o"}),
    ] {
        assert!(
            serde_json::from_value::<ReferenceTarget>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn a_reference_selection_keeps_the_source_token_of_an_unresolved_target() {
    let native: crate::design::names::NativeName = serde_json::from_value(
        json!({"codec": "fcstd", "value": {"kind": "text", "value": "Front.Edge1"}}),
    )
    .unwrap();
    round_trip(
        &ReferenceSelection::unresolved(native),
        json!({
            "target": {"kind": "unresolved"},
            "native": {"codec": "fcstd", "value": {"kind": "text", "value": "Front.Edge1"}}
        }),
    );
    round_trip(
        &ReferenceSelection::new(ReferenceTarget::Null),
        json!({"target": {"kind": "null"}}),
    );
    for wire in [
        json!({"target": {"kind": "null"}, "native": null}),
        json!({"target": {"kind": "null"}, "subelements": []}),
        json!({"native": {"codec": "fcstd", "value": {"kind": "text", "value": "Box"}}}),
    ] {
        assert!(
            serde_json::from_value::<ReferenceSelection>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn only_a_state_a_local_path_configuration_or_a_feature_reads_the_design() {
    let mut target = InstanceTarget::new(ReferenceEntity::Topology(EntityRef::Face(face())));
    assert!(!ReferenceTarget::Entity(target.clone()).reads_design());
    target.state = Some(state());
    assert!(ReferenceTarget::Entity(target.clone()).reads_design());
    target.state = None;
    target.path = Some(
        serde_json::from_value(json!([{
            "occurrence": {"kind": "occurrence", "value": "test:model:occurrence#1"},
            "configuration": {"kind": "named", "value": "Large"}
        }]))
        .unwrap(),
    );
    assert!(!target.reads_design());
    target.path = Some(
        serde_json::from_value(json!([{
            "occurrence": {"kind": "occurrence", "value": "test:model:occurrence#1"},
            "configuration": {"kind": "local", "value": "test:design:configuration#large"}
        }]))
        .unwrap(),
    );
    assert!(target.reads_design());
    assert!(
        ReferenceTarget::Feature(FeatureId::mint("test:design:feature#3").unwrap()).reads_design()
    );
    assert!(!ReferenceTarget::Unresolved.reads_design());
}
