// SPDX-License-Identifier: Apache-2.0
//! Rectangular patterns in the v11 design layer (§6.5).

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{Construction, ConstructionInput, InputReference};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::body::{Pattern, PatternInstances, PatternMethod, PatternSeeds};
use cadmpeg_ir::design::ops::shared::{LinearRun, PatternLayout, PatternRun, RunSides, Spacing};
use cadmpeg_ir::design::options::{NativeOption, SymmetricMeasure};
use cadmpeg_ir::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use cadmpeg_ir::features::patterns::{PatternSeed, PatternTransform};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;

use cadmpeg_ir::units::UnitVector3;

use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;

pub(super) fn project(
    operation: &LegacyOperation,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Option<FeatureOperation> {
    let LegacyOperation::Pattern { seeds, pattern } = operation else {
        return None;
    };
    let PatternTransform::Linear {
        direction: Some(direction),
        count,
        second,
        symmetric,
        omitted_instances,
        ..
    } = pattern.definition()
    else {
        return None;
    };
    if !omitted_instances.is_empty() || seeds.is_empty() {
        return None;
    }
    let construction = scope.rectangular_pattern_construction()?;
    let distribution = construction.distribution();
    let source = [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ];
    if source
        .iter()
        .any(|(count, extent)| *count > 1 && *extent == 0.0)
    {
        return None;
    }
    let mut active = [(Some(direction.get()), *count, *symmetric)]
        .into_iter()
        .chain(
            second
                .iter()
                .map(|second| (Some(second.direction.get()), second.count, second.symmetric)),
        );
    let mut records = construction.directions().iter();
    let mut runs = Vec::with_capacity(2);
    for (axis, (source_count, extent)) in source.into_iter().enumerate() {
        let payload = axis == 1 && construction.secondary_direction().is_some();
        let is_active = source_count > 1;
        let record = if !payload
            && (is_active || (axis == 0 && construction.secondary_direction().is_some()))
        {
            records.next()
        } else {
            None
        };
        let (direction, symmetric) = if is_active {
            let (direction, count, symmetric) = active.next()?;
            if count != source_count {
                return None;
            }
            (direction, symmetric)
        } else {
            let direction = if payload {
                construction.secondary_direction().map(|value| {
                    cadmpeg_ir::math::Vector3::from(
                        value.value.map(cadmpeg_ir::scalar::FiniteReal::get),
                    )
                })
            } else {
                record
                    .and_then(crate::records::feature::patterns::DesignRectangularPatternDirection::direction)
                    .map(|value| *value.as_raw())
            };
            (
                direction.map(|value| {
                    if extent < 0.0 {
                        value.scale(-1.0)
                    } else {
                        value
                    }
                }),
                false,
            )
        };
        runs.push(run(
            scope,
            record,
            direction,
            extent,
            source_count,
            symmetric,
        )?);
    }
    if active.next().is_some() {
        return None;
    }
    let mut runs = runs.into_iter();
    let first = runs.next()?;
    let second = runs.next()?;
    let mut native_options = BTreeMap::new();
    let method = match distribution.map(|value| value.compute_option.value) {
        Some(1) => Some(PatternMethod::Identical),
        Some(2) => Some(PatternMethod::Adjust { against: None }),
        Some(value) => {
            native_options.insert(
                cadmpeg_core::nonblank_literal!("f3d:compute_option"),
                NativeOption::Integer(Dimension::Literal(i64::from(value))),
            );
            None
        }
        None => None,
    };
    let (seeds, native_seed) = project_seeds(seeds, method)?;
    let mut native_inputs = Vec::new();
    if native_seed {
        if let Some(distribution) = distribution {
            native_options.insert(
                cadmpeg_core::nonblank_literal!("f3d:compute_option"),
                NativeOption::Integer(Dimension::Literal(i64::from(
                    distribution.compute_option.value,
                ))),
            );
        }
        native_inputs.push("seed selections");
    }
    if distribution.is_none() {
        native_inputs.push("distribution flags");
    }
    if first.direction.resolved.is_none() || second.direction.resolved.is_none() {
        native_inputs.push("inactive directions");
    }
    if !first.direction.inputs.is_empty() || !second.direction.inputs.is_empty() {
        native_inputs.push("direction selections");
    }
    if !native_inputs.is_empty() {
        losses.push(F3dLossCode::FeatureRectangularPatternDesignNative.note(format!(
            "scope {} retains {} in its native record; its decoded directions, counts, and distances transfer to the v11 runs.",
            scope.id, native_inputs.join(", ")
        )));
    }
    Some(FeatureOperation::Pattern(Box::new(Pattern {
        seeds,
        layout: PatternLayout::Linear {
            first,
            seed_only: Some(Dimension::Literal(false)),
            second: Some(second),
        },
        instances: PatternInstances::Implicit(Vec::new()),
        native_options,
        source_settings: BTreeMap::new(),
    })))
}

fn project_seeds(
    seeds: &[PatternSeed],
    method: Option<PatternMethod>,
) -> Option<(PatternSeeds, bool)> {
    use cadmpeg_ir::features::selections::{BodySelection, FaceSelection};
    let source = match seeds {
        [PatternSeed::Faces(selection)] => {
            let native = match selection {
                FaceSelection::Native(native) | FaceSelection::Resolved { native, .. } => {
                    native.as_str()
                }
                FaceSelection::Historical { native, .. }
                | FaceSelection::HistoricalPartial { native, .. } => native.as_str(),
                FaceSelection::Generated { native, .. } => native.as_str(),
                _ => return None,
            };
            return Some((PatternSeeds::Faces(native_seeds(native)?), true));
        }
        [PatternSeed::Bodies(selection)] => match selection {
            BodySelection::Native(native) | BodySelection::Resolved { native, .. } => {
                native.as_str()
            }
            BodySelection::Historical { native, .. }
            | BodySelection::Generated { native, .. }
            | BodySelection::Local { native, .. } => native.as_str(),
            _ => return None,
        },
        _ => {
            let picks = seeds
                .iter()
                .map(|seed| {
                    let PatternSeed::Feature(feature) = seed else {
                        return None;
                    };
                    Some(Picked {
                        target: PickTarget::FeatureResult(feature.clone()),
                        context: None,
                        native: None,
                        domain: None,
                        witness: None,
                        resolved: None,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            return Some((
                PatternSeeds::Features {
                    features: Selection {
                        at: Anchor::Input,
                        picks: Picks::Named(Members::new(picks).ok()?),
                        native: None,
                    },
                    method,
                },
                false,
            ));
        }
    };
    Some((
        PatternSeeds::Bodies {
            bodies: native_seeds(source)?,
            join: Some(false),
        },
        true,
    ))
}

fn native_seeds(source: &str) -> Option<cadmpeg_ir::design::selection::SetSelection> {
    Some(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target: PickTarget::Native { within: None },
                context: None,
                native: Some(NativeName {
                    codec: cadmpeg_core::nonblank_literal!("f3d").into(),
                    value: NativeNameValue::Text(cadmpeg_core::text::NonBlankString::new(source)?),
                }),
                domain: None,
                witness: None,
                resolved: None,
            }])
            .ok()?,
        ),
        native: None,
    })
}

fn run(
    scope: &DesignParameterScope,
    record: Option<&crate::records::feature::patterns::DesignRectangularPatternDirection>,
    direction: Option<cadmpeg_ir::math::Vector3>,
    extent: f64,
    count: u32,
    symmetric: bool,
) -> Option<LinearRun> {
    let resolved = match direction {
        Some(direction) => Some(UnitVector3::normalized(direction)?),
        None => None,
    };
    let construction = scope.rectangular_pattern_construction()?;
    let distribution = construction.distribution();
    let adjacent = match distribution {
        Some(distribution) => !distribution.total_extent.value,
        None => match record
            .or_else(|| construction.directions().first())?
            .distance_type
        {
            7 => true,
            8 | 10 => false,
            _ => return None,
        },
    };
    let distance = Dimension::Literal(Length::new(extent.abs() * 10.0)?);
    let count = Dimension::Literal(count);
    let native = match record {
        Some(record) => Some(NativeNameValue::Integers(vec![i64::from(
            record.selection_record_index,
        )])),
        None if resolved.is_none() => Some(NativeNameValue::Text(
            cadmpeg_core::text::NonBlankString::new(scope.id.clone())?,
        )),
        None => None,
    };
    let inputs = native
        .and_then(|value| {
            Some(ConstructionInput {
                role: None,
                reference: Some(InputReference::Picks(Selection {
                    at: Anchor::Input,
                    picks: Picks::Named(
                        Members::new(vec![Picked {
                            target: PickTarget::Native { within: None },
                            context: None,
                            native: Some(NativeName {
                                codec: cadmpeg_core::nonblank_literal!("f3d").into(),
                                value,
                            }),
                            domain: None,
                            witness: None,
                            resolved: None,
                        }])
                        .ok()?,
                    ),
                    native: None,
                })),
                value: None,
                reversed: None,
            })
        })
        .into_iter()
        .collect();
    Some(LinearRun {
        direction: Construction {
            inputs,
            resolved,
            ..Construction::default()
        },
        run: PatternRun {
            spacing: if adjacent {
                Spacing::Pitch {
                    count,
                    pitch: distance,
                }
            } else {
                Spacing::Span {
                    count,
                    span: distance,
                }
            },
            sides: distribution.map(|_| {
                if symmetric {
                    RunSides::Symmetric {
                        count: Some(SymmetricMeasure::Total),
                        extent: Some(SymmetricMeasure::PerSide),
                    }
                } else {
                    RunSides::One
                }
            }),
            seed: None,
            increments: Vec::new(),
        },
    })
}

#[cfg(test)]
mod tests;
