// SPDX-License-Identifier: Apache-2.0

use crate::history::feature_outputs::{omitted_copy_outputs, omitted_pattern_copies};
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::patterns::PatternKind;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use std::collections::BTreeSet;

fn feature(operation: FeatureOperation) -> Feature {
    Feature {
        id: FeatureId::mint("test:model:feature#mirror").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Mirror".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: None,
        product_definition: None,
    }
}

fn mirror() -> Feature {
    feature(FeatureOperation::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::UNRESOLVED_MIRROR,
    })
}

/// Input state `{1, 2}`, result state `{1, 2, 3, 4}`, affected slots `1, 3, 4`.
fn copies_omitted_by(feature: &Feature, omitted: impl Fn(i64) -> bool) -> Vec<i64> {
    let before = BTreeSet::from([1, 2]);
    let after = BTreeSet::from([1, 2, 3, 4]);
    omitted_pattern_copies(feature, &[4, 1, 3], (Some(&before), Some(&after)), omitted)
}

#[test]
fn a_pattern_copy_body_is_a_result_slot_that_the_input_state_does_not_hold() {
    assert_eq!(copies_omitted_by(&mirror(), |_| true), [3, 4]);
    assert_eq!(copies_omitted_by(&mirror(), |slot| slot == 4), [4]);
    assert!(copies_omitted_by(&mirror(), |_| false).is_empty());
}

#[test]
fn only_a_pattern_generates_copy_bodies() {
    let extrude = feature(FeatureOperation::Native {
        kind: "Extrude".into(),
        parameters: Default::default(),
    });
    assert!(copies_omitted_by(&extrude, |_| true).is_empty());
}

#[test]
fn copy_bodies_need_both_measured_states() {
    let after = BTreeSet::from([1, 3]);
    assert!(omitted_pattern_copies(&mirror(), &[3], (None, Some(&after)), |_| true).is_empty());
}

#[test]
fn an_omitted_copy_body_is_a_feature_local_body_of_the_result_state() {
    let mirror = mirror();
    let (note, result) = omitted_copy_outputs(&mirror, 7, &[4]);
    assert_eq!(note.code, F3dLossCode::FeaturePatternCopyBodyOmitted.kind());
    let result = result.expect("result state");
    assert_eq!(result.output_of, mirror.id);
    assert_eq!(
        result.id,
        crate::ids::history_result_state_id(&mirror.id, 7)
    );
    assert_eq!(
        result
            .bodies()
            .iter()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
        ["4"]
    );
}
