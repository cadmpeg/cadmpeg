// SPDX-License-Identifier: Apache-2.0
//! Circular, rectangular, and mirror pattern projection.

use super::{feature_stream_matches,unique_feature_match_for_decode};
use crate::design::face_resolve::resolved_historical_face_group;
use crate::ids;
use crate::records::{feature::scope::DesignParameterScope,
    topology::{construction::DesignConstructionOperandGroup,extrude_selection::DesignOperandRole,
    face::DesignFaceOperand}};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3,Vector3};

pub(super) fn project_circular_pattern(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    let Some(construction) = scope.circular_pattern_construction() else {
        return Ok(None);
    };
    let Some((axis_origin, axis_dir)) = circular_pattern_axis(&construction.axis) else {
        return Ok(None);
    };
    let Some(stream) =
        ids::native_stream_charged(ctx, &scope.id, "resolve F3D unique feature stream")?
    else {
        return Ok(None);
    };
    let Some(group) = unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && group.scope_record_index == scope.record_index
                && matches!(
                    group.role(),
                    DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                )
                && !group.members().is_empty())
        },
        "visit F3D unique feature source",
    )?
    else {
        return Ok(None);
    };
    let seed = if group.role() == DesignOperandRole::BODIES_A {
        PatternSeed::Faces(
            match resolved_historical_face_group(
                ctx,
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            )? {
                Some(selection) => selection,
                None => cadmpeg_ir::features::FaceSelection::Native(
                    ctx.copy_retained_text(&group.id, "f3d circular face seed id")?,
                ),
            },
        )
    } else {
        PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
            ctx.copy_retained_text(&group.id, "f3d circular body seed id")?,
        ))
    };
    let Some(axis_origin) = cadmpeg_ir::features::FinitePoint3::new(axis_origin) else {
        return Ok(None);
    };
    let Some(axis_dir) = cadmpeg_ir::features::FeatureDirection3::new(axis_dir) else {
        return Ok(None);
    };
    let Some(pattern) = PatternKind::new(PatternTransform::Circular {
        axis_origin,
        axis_dir,
        angle: construction.angle,
        count: construction.count.get(),
    })
    .ok() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: vec![seed],
            pattern,
        },
    )))
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
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let Some(construction) = scope.rectangular_pattern_construction() else {
        return Ok(None);
    };
    let mut active = [
        (
            construction.u_count(),
            construction.u_extent(),
            construction.v_count(),
        ),
        (
            construction.v_count(),
            construction.v_extent(),
            construction.u_count(),
        ),
    ]
    .into_iter()
    .filter_map(|(count, extent, inactive_count)| {
        Some((count, PatternIntervals::new(count)?, extent, inactive_count))
    });
    let Some((count, intervals, extent, inactive_count)) = active.next() else {
        return Ok(None);
    };
    if active.next().is_some() || inactive_count != 1 {
        return Ok(None);
    }
    let direction = construction.instances().and_then(|instances| {
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
    let group_seed = if let Some(stream) = ids::native_stream_charged(ctx, &scope.id, "f3d rectangular pattern scope stream")? {
        let mut source = groups.iter();
        let mut matching = || ctx.find_by(&mut source, |group| {
            Ok(feature_stream_matches(ctx, &group.id, Some(stream), "f3d rectangular pattern group stream")?
                && group.scope_record_index == scope.record_index
                && matches!(group.role(), DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B)
                && !group.members().is_empty())
        }, "f3d rectangular pattern group source");
        let first = matching()?;
        let second = matching()?;
        match (first, second) {
            (Some(group), None) => Some(if group.role() == DesignOperandRole::BODIES_A {
                PatternSeed::Faces(
                    match resolved_historical_face_group(
                        ctx,
                        scope,
                        scope.previous_history_state_id(),
                        group,
                        face_operands,
                    )? {
                        Some(selection) => selection,
                        None => cadmpeg_ir::features::FaceSelection::Native(
                            ctx.copy_retained_text(&group.id, "f3d rectangular face seed id")?,
                        ),
                    },
                )
            } else {
                PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
                    ctx.copy_retained_text(&group.id, "f3d rectangular body seed id")?,
                ))
            }),
            _ => None,
        }
    } else {
        None
    };
    let mut seeds = Vec::new();
    if let Some(seed) = component_seed.or(group_seed) {
        ctx.push_vec(&mut seeds, seed, "f3d rectangular pattern seeds")?;
    }
    let direction = direction.map(cadmpeg_ir::features::FeatureDirection3::from);
    let Some(spacing) = PositiveLength::new(extent.abs() * 10.0 / f64::from(intervals.get()))
    else {
        return Ok(None);
    };
    let Some(pattern) = PatternKind::new(PatternTransform::Linear {
        direction,
        spacing,
        count,
        second: None,
    })
    .ok() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Pattern { seeds, pattern },
    )))
}

pub(super) fn project_mirror(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    scopes: &[DesignParameterScope],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };

    let Some(construction) = scope.mirror_construction() else {
        return Ok(None);
    };
    let Some(stream) =
        ids::native_stream_charged(ctx, &scope.id, "resolve F3D unique feature stream")?
    else {
        return Ok(None);
    };
    let seed_group = unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && group.scope_record_index == scope.record_index
                && group.record_index == construction.seed_group_record_index
                && matches!(
                    group.role(),
                    DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                )
                && !group.members().is_empty())
        },
        "visit F3D unique feature source",
    )?;
    let plane_group = unique_feature_match_for_decode(
        ctx,
        groups.iter(),
        |group| {
            Ok(feature_stream_matches(
                ctx,
                &group.id,
                Some(stream),
                "compare F3D unique feature stream",
            )? && group.scope_record_index == scope.record_index
                && group.record_index == construction.plane_group_record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.members().len() == 1)
        },
        "visit F3D unique feature source",
    )?;
    let (Some(seed_group), Some(_plane_group)) = (seed_group, plane_group) else {
        return Ok(None);
    };
    let seed = if let Some(record_index) = construction
        .seed_feature_scope_record_index
        .map(|reference| reference.value)
    {
        let seed_scope = unique_feature_match_for_decode(
            ctx,
            scopes.iter(),
            |candidate| {
                Ok(feature_stream_matches(
                    ctx,
                    &candidate.id,
                    Some(stream),
                    "compare F3D unique feature stream",
                )? && candidate.record_index == record_index)
            },
            "visit F3D unique feature source",
        )?;
        let Some(seed_scope) = seed_scope else {
            return Ok(None);
        };
        PatternSeed::Feature(crate::design::identity::neutral_feature_id(
            ctx, seed_scope,
        )?)
    } else if seed_group.role() == DesignOperandRole::BODIES_B {
        PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
            ctx.copy_retained_text(&seed_group.id, "f3d mirror body seed id")?,
        ))
    } else {
        PatternSeed::Faces(
            match resolved_historical_face_group(
                ctx,
                scope,
                scope.previous_history_state_id(),
                seed_group,
                face_operands,
            )? {
                Some(selection) => selection,
                None => cadmpeg_ir::features::FaceSelection::Native(
                    ctx.copy_retained_text(&seed_group.id, "f3d mirror face seed id")?,
                ),
            },
        )
    };
    let (plane_origin, plane_normal, scale_origin) = match construction.plane {
        Some(plane) => (plane.origin().get(), *plane.normal().as_raw(), false),
        None => {
            let Some(plane_scope_record_index) = construction
                .plane_scope_record_index
                .map(|value| value.value)
            else {
                return Ok(None);
            };
            let plane = unique_feature_match_for_decode(
                ctx,
                scopes.iter(),
                |candidate| {
                    Ok(feature_stream_matches(
                        ctx,
                        &candidate.id,
                        Some(stream),
                        "compare F3D unique feature stream",
                    )? && candidate.record_index == plane_scope_record_index
                        && matches!(
                            candidate.payload(),
                            crate::records::feature::scope::DesignScopePayload::WorkPlane(_)
                        )
                        && candidate.work_plane_transform().is_some())
                },
                "visit F3D unique feature source",
            )?;
            let Some(plane) = plane else {
                return Ok(None);
            };
            let Some(transform) = plane.work_plane_transform() else {
                return Ok(None);
            };
            (
                Point3::new(transform[0][3], transform[1][3], transform[2][3]),
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                true,
            )
        }
    };
    let origin_scale = if scale_origin { 10.0 } else { 1.0 };
    let Some(plane_origin) = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
        plane_origin.x * origin_scale,
        plane_origin.y * origin_scale,
        plane_origin.z * origin_scale,
    )) else {
        return Ok(None);
    };
    let Some(plane_normal) = cadmpeg_ir::features::FeatureDirection3::new(plane_normal) else {
        return Ok(None);
    };
    let Some(pattern) = PatternKind::new(PatternTransform::Mirror {
        plane_origin,
        plane_normal,
    })
    .ok() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: vec![seed],
            pattern,
        },
    )))
}

