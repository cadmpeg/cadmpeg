// SPDX-License-Identifier: Apache-2.0
//! Rectangular and circular pattern projection.

use crate::design::feature_project::pattern_seeds;

use crate::design::feature_project::pattern_seeds::pattern_group_seeds;
use crate::design::feature_project::pattern_seeds::pattern_seed_group;

use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use std::collections::HashMap;

/// Largest difference, in radians, between a circular pattern's total angle
/// and a full turn for which the pattern still spans the full turn.
pub(super) const EPS_CIRCULAR_PATTERN_FULL_TURN: f64 = 1.0e-12;

/// Largest `1 - cos` between a rectangular-pattern direction record and the
/// instance-run direction that still names the same direction.
pub(super) const EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT: f64 = 1.0e-9;

pub(super) fn project_circular_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    let construction = scope.circular_pattern_construction()?;
    // The separator byte of a 130-byte axis record has no settled meaning
    // (DR-327); a full turn, whose instances do not depend on the direction
    // of travel, passes this gate and projects once the axis resolves.
    if construction.axis_separator_flag_offset.is_some()
        && (construction.angle.get() - std::f64::consts::TAU).abs() > EPS_CIRCULAR_PATTERN_FULL_TURN
    {
        return None;
    }
    let (axis_origin, axis_dir) = circular_pattern_axis(&construction.axis)?;
    let group = pattern_seed_group(scope, groups)?;
    let seeds = pattern_group_seeds(
        scope,
        group,
        face_operands,
        entity_selection_operands,
        scopes,
        authored_ordinals,
        losses,
    );
    let axis_origin = cadmpeg_ir::features::geometry::FinitePoint3::new(axis_origin)?;
    let suppressed = construction
        .suppressed
        .as_ref()
        .map_or(&[][..], |keys| keys.value.as_slice());
    let one_sided = construction
        .symmetric
        .is_some_and(|symmetric| !symmetric.value);
    let pattern = if suppressed.is_empty()
        || !one_sided
        || construction.axis_separator_flag_offset.is_some()
    {
        if !suppressed.is_empty() {
            let reason = if construction.axis_separator_flag_offset.is_some() {
                "the 130-byte axis record leaves the direction of travel unread"
            } else {
                "the instance order of a symmetric pattern is unread"
            };
            losses.push(
                crate::loss::F3dLossCode::FeatureCircularPatternSuppressionNotTransferred.note(
                    format!(
                        "CircularPattern scope {} keeps its suppressed instances {suppressed:?}: {reason}.",
                        scope.id
                    ),
                ),
            );
        }
        PatternKind::new(PatternTransform::Circular {
            axis_origin,
            axis_dir: cadmpeg_ir::features::geometry::FeatureDirection3::new(axis_dir)?,
            angle: construction.angle,
            count: construction.count.get(),
        })
        .ok()?
    } else {
        let count = construction.count.get();
        let full_turn = (construction.angle.get() - std::f64::consts::TAU).abs()
            <= EPS_CIRCULAR_PATTERN_FULL_TURN;
        let step = construction.angle.get()
            / f64::from(if full_turn {
                count
            } else {
                count.checked_sub(1)?
            });
        let angles = (0..count)
            .filter(|instance| !suppressed.contains(&(instance + 1)))
            .map(|instance| cadmpeg_ir::scalar::Angle::new(step * f64::from(instance)))
            .collect::<Option<Vec<_>>>()?;
        PatternKind::new(PatternTransform::CircularAngles {
            axis_origin,
            axis_dir: cadmpeg_ir::units::UnitVector3::normalized(axis_dir)?,
            angles,
        })
        .ok()?
    };
    losses.push(
        crate::loss::F3dLossCode::FeatureCircularPatternControlsAssumed.note(format!(
            "CircularPattern scope {} is read with equal spacing; its spacing control remains unread (DR-1030).",
            scope.id,
        )),
    );
    if (construction.angle.get() - std::f64::consts::TAU).abs() > EPS_CIRCULAR_PATTERN_FULL_TURN {
        match construction.symmetric.map(|symmetric| symmetric.value) {
            Some(true) => losses.push(
                crate::loss::F3dLossCode::FeatureCircularPatternPlacementNotTransferred.note(format!(
                    "CircularPattern scope {} drops the symmetric distribution: a circular pattern has no field for it.",
                    scope.id
                )),
            ),
            Some(false) => {}
            None => losses.push(
                crate::loss::F3dLossCode::FeatureCircularPatternDistributionAssumed.note(format!(
                    "CircularPattern scope {} is read as one-sided: its partial turn has no readable payload symmetric byte.",
                    scope.id
                )),
            ),
        }
    }
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
}

pub(super) fn circular_pattern_axis(
    axis: &crate::records::feature::patterns::DesignCircularPatternAxis,
) -> Option<(Point3, Vector3)> {
    use crate::records::feature::patterns::DesignCircularPatternAxis;

    match axis {
        DesignCircularPatternAxis::Inline {
            origin, direction, ..
        } => Some((
            Point3::new(
                origin[0].get() * 10.0,
                origin[1].get() * 10.0,
                origin[2].get() * 10.0,
            ),
            *direction.as_raw(),
        )),
        DesignCircularPatternAxis::HistoricalEdge {
            resolved: Some(axis),
            ..
        } => Some((axis.origin.get(), *axis.direction.as_raw())),
        DesignCircularPatternAxis::HistoricalEdge { .. } => None,
    }
}

/// The gap count of an active rectangular pattern axis: one less than the
/// instance count. An axis is active only when it states two or more
/// instances, so the gap count is never zero and always divides.
#[derive(Clone, Copy)]
pub(super) struct PatternIntervals(std::num::NonZeroU32);

impl PatternIntervals {
    /// Admit an active axis. An axis with fewer than two instances states no
    /// gap and is refused here.
    pub(super) fn new(count: u32) -> Option<Self> {
        std::num::NonZeroU32::new(count.checked_sub(1)?).map(Self)
    }

    /// The gap count.
    pub(super) const fn get(self) -> u32 {
        self.0.get()
    }
}

pub(super) fn project_rectangular_pattern_scalars(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let construction = scope.rectangular_pattern_construction()?;
    let mut direction_records = construction.directions().iter();
    let mut active = Vec::new();
    let mut dropped = Vec::new();
    let distribution = construction.distribution();
    let component_seed = construction
        .instances()
        .and_then(|instances| match instances {
            crate::records::feature::patterns::DesignRectangularPatternInstances::Bodies(_) => None,
            crate::records::feature::patterns::DesignRectangularPatternInstances::Components {
                seed,
                ..
            } => Some(PatternSeed::Occurrences(
                (vec![crate::ids::neutral_component_occurrence_id(
                    &seed.occurrence_guid,
                )])
                .try_into()
                .ok()?,
            )),
        });
    let seeds = component_seed
        .map(|seed| vec![seed])
        .or_else(|| {
            pattern_seed_group(scope, groups).map(|group| {
                pattern_group_seeds(
                    scope,
                    group,
                    face_operands,
                    entity_selection_operands,
                    scopes,
                    authored_ordinals,
                    losses,
                )
            })
        })
        .unwrap_or_default();
    if let Some(note) = distribution.and_then(|distribution| {
        pattern_seeds::distribution_seed_conflict(scope, distribution, &seeds)
    }) {
        losses.push(note);
    }
    let adjacent = distribution.map_or_else(
        || {
            construction
                .directions()
                .first()
                .is_some_and(|record| record.distance_type == 7)
        },
        |distribution| !distribution.total_extent.value,
    );
    let stored_symmetric = distribution.map_or([false; 2], |distribution| {
        [
            distribution.symmetric.value,
            distribution.symmetric_two.value,
        ]
    });
    let payload_names_direction_one = construction.secondary_direction().is_some();
    for (axis, ((count, extent), stored_symmetric)) in [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ]
    .into_iter()
    .zip(stored_symmetric)
    .enumerate()
    {
        let payload_direction = axis == 1 && construction.secondary_direction().is_some();
        let axis_active = count > 1 && extent != 0.0;
        let symmetric = stored_symmetric && axis_active;
        if stored_symmetric && !axis_active {
            losses.push(
                crate::loss::F3dLossCode::FeatureRectangularPatternInactiveSymmetricDropped.note(format!(
                    "RectangularPattern scope {} drops the symmetric flag of its inactive direction {}.",
                    scope.id,
                    ["one", "two"][axis]
                )),
            );
        }
        let record =
            if payload_direction || !(axis_active || (axis == 0 && payload_names_direction_one)) {
                None
            } else {
                direction_records.next()
            };
        if let (false, Some(record)) = (axis_active, record) {
            losses.push(
                crate::loss::F3dLossCode::FeatureRectangularPatternDirectionOneRecordAssumed.note(format!(
                    "RectangularPattern scope {} reads direction record {} as the record of its inactive direction one: the pattern payload names it as the U-direction record.",
                    scope.id, record.record_index
                )),
            );
        }
        let Some(intervals) = PatternIntervals::new(count) else {
            continue;
        };
        if extent == 0.0 {
            dropped.push(format!(
                "the {count} coincident instances of direction {}",
                ["one", "two"][axis]
            ));
            continue;
        }
        let axis_adjacent = match (distribution, record) {
            (None, Some(record)) => record.distance_type == 7,
            _ => adjacent,
        };
        let stored_direction = if payload_direction {
            construction.secondary_direction().and_then(|record| {
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::from(
                    record.value.map(cadmpeg_ir::scalar::FiniteReal::get),
                ))
            })
        } else {
            record.and_then(
                crate::records::feature::patterns::DesignRectangularPatternDirection::direction,
            )
        };
        let direction = stored_direction.map(|direction| {
            if extent < 0.0 {
                direction.negated()
            } else {
                direction
            }
        });
        let divisor = match (axis_adjacent, symmetric) {
            (true, _) => 1.0,
            // The extent runs from the seed to the final instance along the
            // direction, which holds `count / 2` instances.
            (false, true) => f64::from(count / 2),
            (false, false) => f64::from(intervals.get()),
        };
        let spacing = PositiveLength::new(extent.abs() * 10.0 / divisor)?;
        active.push((count, spacing, direction, symmetric));
    }
    let instance_direction = construction.instances().and_then(|instances| {
        let first = &instances.frames().next()?.transform.value;
        let last = &instances.frames().next_back()?.transform.value;
        let delta = Vector3::new(
            last[0][3] - first[0][3],
            last[1][3] - first[1][3],
            last[2][3] - first[2][3],
        );
        cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(delta)
    });
    let (count, spacing, direction, symmetric, second) = match active.as_slice() {
        // The instance run is the established direction of a single-direction
        // pattern; a direction record only fills it in when the run is absent
        // and must agree with it otherwise.
        [(count, spacing, direction, symmetric)] => {
            let direction = match (instance_direction, *direction) {
                (Some(run), Some(record))
                    if 1.0 - run.as_raw().dot(*record.as_raw())
                        > EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT =>
                {
                    losses.push(
                        crate::loss::F3dLossCode::FeatureRectangularPatternDirectionRecordDisagrees
                            .note(format!(
                                "RectangularPattern scope {} keeps the direction of its generated-instance run; its direction record states another direction.",
                                scope.id
                            )),
                    );
                    Some(run)
                }
                (run, record) => run.or(record),
            };
            (*count, *spacing, direction, *symmetric, None)
        }
        [(count, spacing, direction, symmetric), second] => {
            let (second_count, second_spacing, Some(second_direction), second_symmetric) = second
            else {
                return None;
            };
            (
                *count,
                *spacing,
                *direction,
                *symmetric,
                Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                    direction: cadmpeg_ir::features::geometry::FeatureDirection3::from(
                        *second_direction,
                    ),
                    spacing: *second_spacing,
                    count: *second_count,
                    symmetric: *second_symmetric,
                }),
            )
        }
        _ => return None,
    };
    let pattern = PatternKind::new(PatternTransform::Linear {
        direction: direction.map(cadmpeg_ir::features::geometry::FeatureDirection3::from),
        spacing,
        count,
        second,
        omitted_instances: construction.omitted_grid_positions()?,
        symmetric,
    })
    .ok()?;
    if let Some(record) = construction
        .directions()
        .iter()
        .find(|record| distribution.is_none() && record.distance_type == 10)
    {
        losses.push(crate::loss::F3dLossCode::FeatureRectangularPatternDistanceFormAssumed.note(format!(
            "RectangularPattern scope {} reads its distances as seed-to-final-instance extents: direction record {} stores distance type 10 without a readable distribution block.",
            scope.id, record.record_index
        )));
    }
    if direction.is_none() {
        losses.push(
            crate::loss::F3dLossCode::FeatureRectangularPatternDirectionUnresolved.note(format!(
                "RectangularPattern scope {} keeps its first direction unresolved: it has no admitted direction record or generated-instance run.",
                scope.id
            )),
        );
    }
    if !dropped.is_empty() {
        losses.push(
            crate::loss::F3dLossCode::FeatureRectangularPatternPlacementNotTransferred.note(
                format!(
                    "RectangularPattern scope {} drops {}: a linear pattern has no field for them.",
                    scope.id,
                    dropped.join(" and ")
                ),
            ),
        );
    }
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
}
