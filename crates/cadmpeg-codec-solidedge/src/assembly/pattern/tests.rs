// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation};
use cadmpeg_ir::design::ops::assembly::ComponentPattern;
use cadmpeg_ir::design::ops::shared::{LinearLayout, PatternLayout, Spacing};
use cadmpeg_ir::design::selection::{PickContext, PickTarget, Picks, StepRef};

use super::super::occurrence_id;
use crate::features::feature_id;
use crate::test_support::{
    assembly_pattern_document, AssemblyPatternSpec, Member2d, OccurrenceSpec, PatternSpec,
    ProfileFrame, PATTERN_OBJECT, PATTERN_SITE,
};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

/// A layout sketch on the XY plane with normal +Z.
const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn count(result: &DecodeResult, code: &str) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .count()
}

/// References 20 (the seed) and 21, 22, ... at `x` metres along X.
fn references(xs: &[f64]) -> Vec<OccurrenceSpec> {
    xs.iter()
        .enumerate()
        .map(|(index, x)| {
            let mut spec =
                OccurrenceSpec::at_identity(20 + u32::try_from(index).unwrap(), PATTERN_SITE);
            spec.translation = [*x, 0.0, 0.0];
            spec
        })
        .collect()
}

fn linear_spec(counts: [u32; 2]) -> PatternSpec<'static> {
    PatternSpec {
        frame: &XY,
        seed: 0,
        kind: 1,
        counts,
        steps: [0.04, 0.01],
        geometry: &[
            Member2d::Line([0.0, 0.0, 0.04, 0.0]),
            Member2d::Line([0.0, 0.0, 0.0, 0.01]),
        ],
        guid: true,
    }
}

fn pattern_feature(result: &DecodeResult) -> &Feature {
    let id = feature_id(PATTERN_OBJECT);
    result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.id == id)
        .unwrap()
}

fn component_pattern(result: &DecodeResult) -> &ComponentPattern {
    match &pattern_feature(result).definition.operation {
        FeatureOperation::ComponentPattern(pattern) => pattern,
        other => panic!("expected a component pattern, found {other:?}"),
    }
}

#[test]
fn rectangular_pattern_states_seed_layout_and_instances() {
    let pattern = linear_spec([3, 1]);
    let references = references(&[0.0, 0.04, 0.08]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 22)],
        layout: None,
    }));
    assert_eq!(count(&result, "assembly.pattern-unread"), 0);
    assert_eq!(result.ir().model.occurrences.len(), 3);
    let component = component_pattern(&result);
    let Picks::Named(members) = &component.seeds.picks else {
        panic!("named picks")
    };
    let [seed] = members.as_slice() else {
        panic!("one seed")
    };
    assert_eq!(seed.target, PickTarget::Component);
    let Some(PickContext::Occurrence(path)) = &seed.context else {
        panic!("an occurrence context")
    };
    assert_eq!(path.0[0].occurrence, StepRef::Occurrence(occurrence_id(20)));
    let PatternLayout::Linear(LinearLayout {
        first,
        second: Some(second),
        ..
    }) = &component.layout
    else {
        panic!("a linear layout")
    };
    for (run, want) in [(first, [3, 40]), (second, [1, 10])] {
        let Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(pitch),
        } = &run.run.spacing
        else {
            panic!("a literal pitch")
        };
        assert_eq!(*count, want[0]);
        assert!((pitch.get() - f64::from(want[1])).abs() <= EPS_VALUE);
    }
    let indices: Vec<_> = component
        .instances
        .iter()
        .map(|instance| (instance.index.0.clone(), instance.occurrences.clone()))
        .collect();
    assert_eq!(
        indices,
        vec![
            (vec![1, 0], vec![occurrence_id(21)]),
            (vec![2, 0], vec![occurrence_id(22)]),
        ]
    );
    assert_eq!(result.ir().design.scopes.len(), 1);
}

#[test]
fn second_run_index_is_the_second_coordinate() {
    let pattern = linear_spec([1, 3]);
    let references = references(&[0.0, 0.04]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(0, 1, 21)],
        layout: None,
    }));
    let component = component_pattern(&result);
    assert_eq!(component.instances[0].index.0, vec![0, 1]);
}

/// References 20 (the seed), the copies 21 and 22, the repeat 23 of copy 22 and
/// the stale clone 24, all copies with the copy role.
fn stale_references() -> Vec<OccurrenceSpec> {
    let mut references = references(&[0.0, 0.04, 0.08, 0.08, 0.04]);
    for spec in references.iter_mut().skip(1) {
        spec.role = 0x40;
    }
    references
}

#[test]
fn stale_copy_references_are_not_occurrences() {
    let pattern = linear_spec([3, 1]);
    let references = stale_references();
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 22), (2, 0, 23)],
        layout: None,
    }));
    let ids: Vec<_> = result
        .ir()
        .model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.id.clone())
        .collect();
    assert_eq!(
        ids,
        vec![occurrence_id(20), occurrence_id(21), occurrence_id(22)]
    );
    assert_eq!(component_pattern(&result).instances.len(), 2);
    // The repeated copy, the stale reference 23 and the stale clone 24.
    assert_eq!(count(&result, "assembly.pattern-unread"), 3);
}

#[test]
fn copy_reference_without_the_copy_role_stays_an_occurrence() {
    let pattern = linear_spec([3, 1]);
    let references = references(&[0.0, 0.04, 0.08, 0.08]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 22), (2, 0, 23)],
        layout: None,
    }));
    assert_eq!(result.ir().model.occurrences.len(), 4);
    assert_eq!(component_pattern(&result).instances.len(), 2);
    assert_eq!(count(&result, "assembly.pattern-unread"), 1);
}

#[test]
fn copy_with_a_seed_index_is_left_out() {
    let pattern = linear_spec([2, 1]);
    let references = references(&[0.0, 0.04]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(0, 0, 21)],
        layout: None,
    }));
    assert!(component_pattern(&result).instances.is_empty());
    assert_eq!(count(&result, "assembly.pattern-unread"), 1);
}

#[test]
fn circular_pattern_keeps_its_layout_native_with_count_and_step() {
    let pattern = PatternSpec {
        frame: &XY,
        seed: 0,
        kind: 2,
        counts: [3, 0],
        steps: [TAU / 3.0, 0.0],
        geometry: &[Member2d::Arc([0.0, 0.0, 0.01, 0.0, TAU])],
        guid: true,
    };
    let references = references(&[0.0, 0.04, 0.08]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 22)],
        layout: None,
    }));
    let component = component_pattern(&result);
    assert!(matches!(component.layout, PatternLayout::Native(_)));
    assert_eq!(component.native_options.len(), 2);
    let indices: Vec<_> = component
        .instances
        .iter()
        .map(|instance| instance.index.0.clone())
        .collect();
    assert_eq!(indices, vec![vec![1], vec![2]]);
}

#[test]
fn pattern_whose_layout_object_is_not_a_pattern_states_nothing() {
    let pattern = linear_spec([3, 1]);
    let references = references(&[0.0, 0.04, 0.08]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 22)],
        layout: Some(20),
    }));
    assert_eq!(count(&result, "assembly.pattern-unread"), 1);
    assert!(result.ir().design.features.is_empty());
    assert!(result.ir().design.scopes.is_empty());
    assert_eq!(result.ir().model.occurrences.len(), 3);
}

#[test]
fn copy_naming_an_unstated_occurrence_is_left_out() {
    let pattern = linear_spec([3, 1]);
    let references = references(&[0.0, 0.04]);
    let result = decode(assembly_pattern_document(&AssemblyPatternSpec {
        pattern: &pattern,
        references: &references,
        copies: &[(1, 0, 21), (2, 0, 99)],
        layout: None,
    }));
    assert_eq!(component_pattern(&result).instances.len(), 1);
    assert_eq!(count(&result, "assembly.pattern-unread"), 1);
}

/// A pattern object's tail: 20 bytes, a count of one, a gap byte, one item whose
/// identifier text is `text`, and the 12-byte trailer.
fn tail(text: &[u8]) -> Vec<u8> {
    let mut list = vec![0_u8; 20];
    list.extend_from_slice(&1_u32.to_le_bytes());
    list.push(0);
    list.extend_from_slice(&40_u32.to_le_bytes());
    list.push(0x10);
    list.extend_from_slice(text);
    list.push(0);
    list.extend_from_slice(&[0; 12]);
    list
}

#[test]
fn copy_list_reads_items_of_any_digit_count() {
    assert_eq!(super::copy_list(&tail(b"40")), Some(vec![40]));
    let mut long = vec![0_u8; 20];
    long.extend_from_slice(&2_u32.to_le_bytes());
    long.push(0);
    for id in [403_u32, 1_000_000] {
        long.extend_from_slice(&id.to_le_bytes());
        long.push(0x10);
        long.extend_from_slice(id.to_string().as_bytes());
        long.push(0);
    }
    long.extend_from_slice(&[0; 12]);
    assert_eq!(super::copy_list(&long), Some(vec![403, 1_000_000]));
}

#[test]
fn copy_list_rejects_text_that_is_not_the_identifier() {
    assert_eq!(super::copy_list(&tail(b"41")), None);
    assert_eq!(super::copy_list(&[0; 8]), None);
}
