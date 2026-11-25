// SPDX-License-Identifier: Apache-2.0
//! Instance indices against the coordinate ranges of their runs (§6.5,
//! part 10 "Patterns" and "Sketches").

use super::super::super::fixture::{
    self, assert_finding, assert_no_design_finding, design_findings, id,
};
use super::{
    constraint, length, linear, linear_layout, linear_stage, pattern, pattern_of, point,
    sketch_entity, sketch_findings, sketched, skipped, with,
};
use crate::design::dimension::Dimension;
use crate::design::names::InstanceIndex;
use crate::design::ops::shared::{CompositeStage, PatternLayout, RunSides, Spacing};
use crate::design::ops::sketch::{PlanarCurve, SketchRelation};
use crate::document::CadIr;

/// Part 10: an override's instance index lies in each run's coordinate
/// range, and under a literal `seed_only: true` of a two-run linear stage
/// it has a zero coordinate in that stage (§6.5).
#[test]
fn an_override_index_lies_in_its_runs_and_a_seed_only_stage() {
    const OUTSIDE: &str = "lies outside a run's coordinate range";
    const SEED_ONLY: &str = "has no zero coordinate under seed_only";
    let indices = |index: Vec<i32>| {
        let mut ir = fixture::document();
        ir.design.features[1].definition.operation =
            pattern_of("extrude", linear(None, 3), vec![skipped(index)]);
        ir
    };
    assert_no_design_finding(&indices(vec![2]));
    assert_finding(&indices(vec![3]), OUTSIDE);

    let two_runs = || {
        let mut layout = linear_layout(None, 2);
        layout.second = Some(layout.first.clone());
        layout.seed_only = Some(Dimension::Literal(true));
        layout
    };
    let staged = |index: Vec<i32>| {
        with(pattern(
            PatternLayout::Composite(vec![CompositeStage::Linear(two_runs()), linear_stage(2)]),
            vec![skipped(index)],
        ))
    };
    let found = |ir: &CadIr| {
        design_findings(ir)
            .into_iter()
            .any(|finding| finding.message.contains(SEED_ONLY))
    };
    assert!(!found(&staged(vec![1, 0, 1])));
    assert!(found(&staged(vec![1, 1, 0])));
}

/// Part 10: a sketch pattern's instance indices lie in its runs' coordinate
/// ranges (§6.5). A run of two instances, the seed first, on one side holds
/// coordinates 0 and 1.
#[test]
fn a_sketch_pattern_instance_lies_in_its_runs() {
    use crate::design::ops::sketch::{
        SketchPattern, SketchPatternAxis, SketchPatternInstance, SketchPatternLayout,
    };
    const OUTSIDE: &str = "lies outside its layout's runs";
    let patterned = |coordinate: i32| {
        let mut ir = sketched();
        ir.design.sketch_entities.push(sketch_entity(
            "copy",
            PlanarCurve::Line {
                start: point(0.0, 1.0),
                end: point(1.0, 1.0),
            },
            Vec::new(),
        ));
        ir.design.sketch_constraints.push(constraint(
            "pattern",
            SketchRelation::Pattern(Box::new(SketchPattern {
                seeds: vec![id("sketch-entity", "line")],
                layout: SketchPatternLayout::Linear {
                    directions: vec![SketchPatternAxis {
                        direction: None,
                        spacing: Spacing::Pitch {
                            count: Dimension::Literal(2),
                            pitch: Dimension::Literal(length(1.0)),
                        },
                        sides: Some(RunSides::One),
                    }],
                },
                instances: vec![SketchPatternInstance {
                    index: InstanceIndex(vec![coordinate]),
                    entities: vec![id("sketch-entity", "copy")],
                    suppressed: None,
                }],
            })),
        ));
        ir
    };
    let outside = |ir: &CadIr| {
        sketch_findings(ir)
            .iter()
            .any(|message| message.contains(OUTSIDE))
    };
    assert!(!outside(&patterned(1)));
    assert!(outside(&patterned(2)));
    assert!(outside(&patterned(-1)));
}
