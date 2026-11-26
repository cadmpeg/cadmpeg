// SPDX-License-Identifier: Apache-2.0
//! Typed circular feature patterns (sldprt.md §2.3): a pattern of features
//! that turns a full turn about its axis spreads its count over the turn.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use cadmpeg_ir::design::construction::{AxisValue, Construction};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::body::{Pattern, PatternInstances, PatternSeeds};
use cadmpeg_ir::design::ops::shared::{CircularLayout, PatternLayout, PatternRun, Spacing};
use cadmpeg_ir::design::selection::PickTarget;
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::patterns::{PatternSeed, PatternTransform};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::units::UnitVector3;

use super::sketch::{pick, selection};
use super::swept::{self, Context};

/// How far a circular pattern's angle may lie from a full turn.
const EPS_FULL_TURN_RAD: f64 = 1e-9;

/// Type each unresolved pattern of `features` whose decoded record is a
/// full-turn circular pattern of features.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) {
    swept::walk(features, OperationFamily::Pattern, |feature, _| {
        if let Ok(pattern) = circular(&feature.id, context) {
            feature.definition.operation = FeatureOperation::Pattern(Box::new(pattern));
        }
    });
}

fn circular(id: &FeatureId, context: &Context<'_>) -> Result<Pattern, &'static str> {
    let source = context
        .model
        .features
        .iter()
        .find(|candidate| &candidate.id == id)
        .ok_or("the pattern has no decoded record")?;
    let FeatureDefinition::Operation(v10::FeatureOperation::Pattern { seeds, pattern }) =
        source.evaluation.definition()
    else {
        return Err("the pattern's decoded record is no pattern");
    };
    let PatternTransform::Circular {
        axis_origin,
        axis_dir,
        angle,
        count,
    } = pattern.definition()
    else {
        return Err("the pattern is not circular");
    };
    if (angle.get() - TAU).abs() > EPS_FULL_TURN_RAD {
        return Err("the circular pattern turns less than a full turn");
    }
    let picks = seeds
        .iter()
        .map(|seed| match seed {
            PatternSeed::Feature(feature) => Ok(pick(PickTarget::FeatureResult(feature.clone()))),
            _ => Err("a seed of the pattern is not a feature"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if picks.is_empty() {
        return Err("the pattern has no seed");
    }
    let direction = UnitVector3::normalized_nonzero((*axis_dir).into())
        .ok_or("the pattern's axis is degenerate")?;
    Ok(Pattern {
        seeds: PatternSeeds::Features {
            features: selection(picks)?,
            method: None,
        },
        layout: PatternLayout::Circular(CircularLayout {
            axis: Construction {
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: Some(AxisValue {
                    origin: *axis_origin,
                    direction,
                }),
                resolved_in: BTreeMap::new(),
            },
            angular: PatternRun {
                spacing: Spacing::Fit {
                    count: Dimension::Literal(*count),
                },
                sides: None,
                seed: None,
                increments: Vec::new(),
            },
            radial: None,
            orientation: None,
        }),
        instances: PatternInstances::Implicit(Vec::new()),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })
}
