// SPDX-License-Identifier: Apache-2.0
//! Rectangular and circular pattern projection.

use super::pattern_seeds::{pattern_group_seeds, pattern_seed_group};
use crate::records::{
    feature::scope::DesignParameterScope,
    topology::{construction::DesignConstructionOperandGroup, face::DesignFaceOperand},
};
use cadmpeg_ir::math::{Point3, Vector3};

/// Largest difference, in radians, between a circular pattern's total angle
/// and a full turn for which the pattern still spans the full turn.
const EPS_CIRCULAR_PATTERN_FULL_TURN: f64 = 1.0e-12;

/// Largest `1 - cos` between a rectangular-pattern direction record and the
/// instance-run direction that still names the same direction.
const EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT: f64 = 1.0e-9;

pub(super) fn project_circular_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    let construction = scope.circular_pattern_construction()?;
    // The separator byte of a 130-byte axis record has no settled meaning
    // (DR-327); only a full turn, whose instances do not depend on the
    // direction of travel, projects from it.
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
    );
    let axis_origin = cadmpeg_ir::features::FinitePoint3::new(axis_origin)?;
    let axis_dir = cadmpeg_ir::features::FeatureDirection3::new(axis_dir)?;
    let pattern = PatternKind::new(PatternTransform::Circular {
        axis_origin,
        axis_dir,
        angle: construction.angle,
        count: construction.count.get(),
    })
    .ok()?;
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

fn circular_pattern_axis(
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
struct PatternIntervals(std::num::NonZeroU32);

impl PatternIntervals {
    /// Admit an active axis. An axis with fewer than two instances states no
    /// gap and is refused here.
    fn new(count: u32) -> Option<Self> {
        std::num::NonZeroU32::new(count.checked_sub(1)?).map(Self)
    }

    /// The gap count.
    const fn get(self) -> u32 {
        self.0.get()
    }
}

pub(super) fn project_rectangular_pattern_scalars(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
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
    let adjacent = distribution.map_or_else(
        || {
            construction
                .directions()
                .first()
                .is_some_and(|record| record.distance_type == 7)
        },
        |distribution| !distribution.total_extent.value,
    );
    let symmetric = distribution.is_some_and(|distribution| distribution.symmetric.value);
    if symmetric {
        // The extent of a symmetric direction has no settled measurement.
        if !adjacent || construction.u_extent() == 0.0 {
            return None;
        }
    }
    for (axis, (count, extent)) in [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ]
    .into_iter()
    .enumerate()
    {
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
        let stored_direction = if axis == 1 && construction.secondary_direction().is_some() {
            construction.secondary_direction().and_then(|record| {
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::from(
                    record.value.map(cadmpeg_ir::scalar::FiniteReal::get),
                ))
            })
        } else {
            direction_records.next().and_then(
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
        let divisor = if adjacent {
            1.0
        } else {
            f64::from(intervals.get())
        };
        let spacing = PositiveLength::new(extent.abs() * 10.0 / divisor)?;
        active.push((count, spacing, direction));
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
    let group_seed = pattern_seed_group(scope, groups).map(|group| {
        pattern_group_seeds(
            scope,
            group,
            face_operands,
            entity_selection_operands,
            scopes,
        )
    });
    let seeds = component_seed
        .map(|seed| vec![seed])
        .or(group_seed)
        .unwrap_or_default();
    let (count, spacing, direction, second) = match active.as_slice() {
        // The instance run is the established direction of a single-direction
        // pattern; a direction record only fills it in when the run is absent
        // and must agree with it otherwise.
        [(count, spacing, direction)] => {
            let direction = match (instance_direction, *direction) {
                (Some(run), Some(record))
                    if 1.0 - run.as_raw().dot(*record.as_raw())
                        > EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT =>
                {
                    None
                }
                (run, record) => run.or(record),
            };
            (*count, *spacing, direction, None)
        }
        [(count, spacing, direction), (second_count, second_spacing, Some(second_direction))] => (
            *count,
            *spacing,
            *direction,
            Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                direction: cadmpeg_ir::features::FeatureDirection3::from(*second_direction),
                spacing: *second_spacing,
                count: *second_count,
                symmetric: false,
            }),
        ),
        _ => return None,
    };
    let pattern = PatternKind::new(PatternTransform::Linear {
        direction: direction.map(cadmpeg_ir::features::FeatureDirection3::from),
        spacing,
        count,
        second,
        omitted_instances: Vec::new(),
        symmetric,
    })
    .ok()?;
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
