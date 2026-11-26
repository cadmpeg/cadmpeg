// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use crate::annotations::{AnnotationBuilder, AnnotationKey, AnnotationTarget, StreamHandle};
use crate::design::ids::{EvaluationId, ProcessStateId};
use crate::provenance::Exactness;

const EVALUATION: &str = "test:design:evaluation#1";
const STATE: &str = "test:process:state#1";
const FACE: &str = "test:brep:face#2";

fn evaluation() -> EvaluationId {
    EvaluationId::mint(EVALUATION).unwrap()
}

#[test]
fn a_key_with_a_space_names_a_record_of_a_delta() {
    let key = AnnotationKey::in_evaluation(&evaluation(), FACE);
    assert_eq!(key.as_str(), "test:design:evaluation#1 test:brep:face#2");
    assert_eq!(
        key.target(),
        AnnotationTarget::Delta {
            qualifier: EVALUATION,
            record: FACE
        }
    );
    assert_eq!(key.owner(), EVALUATION);
    let state = AnnotationKey::in_process_state(&ProcessStateId::mint(STATE).unwrap(), FACE);
    assert_eq!(
        state.target(),
        AnnotationTarget::Delta {
            qualifier: STATE,
            record: FACE
        }
    );
    assert_eq!(
        AnnotationKey::from(FACE).target(),
        AnnotationTarget::Entity(FACE)
    );
}

#[test]
fn keys_in_increasing_order_give_nondecreasing_owners() {
    let keys = [
        "test:brep:face#1",
        "test:brep:face#1 test:brep:face#9",
        "test:brep:face#1 test:brep:face#0",
        "test:brep:face#1!",
        "test:brep:face#10",
        "test:brep:face#1-",
    ]
    .map(AnnotationKey::from)
    .into_iter()
    .collect::<BTreeSet<_>>();
    let owners = keys.iter().map(AnnotationKey::owner).collect::<Vec<_>>();
    assert!(owners.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn a_delta_key_is_annotated_retained_by_its_qualifier_and_remapped_per_id() {
    let key = AnnotationKey::in_evaluation(&evaluation(), FACE);
    let stream = StreamHandle::new(crate::stream_name!("source"));
    let mut builder = AnnotationBuilder::new();
    builder.note(&key, &stream, 7, Some("face"));
    builder.derived(&key, "tolerance").unwrap();
    builder.exactness(FACE, Exactness::Inferred);
    builder.retain_exactness(|owner| owner == EVALUATION);
    let mut annotations = builder.build();
    assert_eq!(annotations.provenance[key.as_str()].offset, 7);
    assert_eq!(
        annotations.exactness()[key.as_str()].fields()["tolerance"],
        Exactness::Derived
    );
    assert!(!annotations.exactness().contains_key(FACE));

    let mut calls = Vec::new();
    annotations
        .map_ids(|id| {
            calls.push(id.to_owned());
            Ok(format!("{id}-mapped"))
        })
        .unwrap()
        .unwrap();
    assert_eq!(calls, [FACE, EVALUATION]);
    let mapped = "test:design:evaluation#1-mapped test:brep:face#2-mapped";
    assert_eq!(annotations.provenance[mapped].offset, 7);
    assert!(annotations.exactness().contains_key(mapped));

    annotations.retain_entities(|owner| owner != "test:design:evaluation#1-mapped");
    assert!(annotations.provenance.is_empty());
    assert!(annotations.exactness().is_empty());
}

#[test]
fn copying_an_entity_annotates_its_delta_record_as_the_entity() {
    let stream = StreamHandle::new(crate::stream_name!("source"));
    let mut builder = AnnotationBuilder::new();
    builder.note(FACE, &stream, 11, None);
    builder.derived(FACE, "tolerance").unwrap();
    let mut annotations = builder.build();
    let key = AnnotationKey::in_evaluation(&evaluation(), FACE);
    annotations.copy_entity(FACE, key.clone());
    assert_eq!(annotations.provenance[&key], annotations.provenance[FACE]);
    assert_eq!(annotations.exactness()[&key], annotations.exactness()[FACE]);

    annotations.copy_entity("test:brep:face#unannotated", key.clone());
    assert!(!annotations.provenance.contains_key(&key));
    assert!(!annotations.exactness().contains_key(&key));
}
