// SPDX-License-Identifier: Apache-2.0
//! Resolve extrude profile selections against sketch regions.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::{entity_selection_matches_curve, parse_sketch_profile};
use crate::design::geometry::{
    arrangement_region_containing_points, historical_member_points_in_state, point_in_polygon,
    point_on_sketch_entity, point_segment_distance, profile_loops_are_independent,
    project_to_sketch, region_containing_points,
};
use crate::ids::native_stream_charged;
use crate::design::feature_project::unique_feature_match_for_decode;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    sketch_geometry::SketchCurveIdentity,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelationOperand,
    topology::{
        construction::DesignConstructionOperandGroup,
        entity_selection::DesignEntitySelectionOperand,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
        sketch_profile::DesignSketchProfileOperand,
        sketch_profile::DesignSketchProfileRegionMember,
    },
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
    pub(crate) arrangement_budget: &'a WorkBudget<'a>,
    pub(crate) ctx: &'a DecodeContext<'a>,
}

#[derive(Clone, Copy)]
pub(in crate::design) struct ScopedExtrudeProfileResolution<'a> {
    entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &'a [crate::history_records::AsmHistory],
    linear_tolerance: f64,
    angular_tolerance: f64,
    arrangement_budget: &'a WorkBudget<'a>,
    ctx: &'a DecodeContext<'a>,
}

impl<'a> ExtrudeProfileResolution<'a> {
    pub(super) fn scoped(
        self,
        histories: &'a [crate::history_records::AsmHistory],
    ) -> ScopedExtrudeProfileResolution<'a> {
        ScopedExtrudeProfileResolution {
            entities: self.entities,
            spatial_entities: self.spatial_entities,
            histories,
            linear_tolerance: self.linear_tolerance,
            angular_tolerance: self.angular_tolerance,
            arrangement_budget: self.arrangement_budget,
            ctx: self.ctx,
        }
    }
}

fn histories_for_scope<'a>(
    ctx: &DecodeContext<'_>,
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [crate::history_records::AsmHistory],
) -> Result<&'a [crate::history_records::AsmHistory], CodecError> {
    if ctx.contains_key_hash_map(scope_histories, scope_id, "f3d scope history membership")? {
        Ok(
            crate::history::bound_scope_history(ctx, scope_id, scope_histories, histories)?
                .map_or(&[], std::slice::from_ref),
        )
    } else {
        Ok(histories)
    }
}

/// Native and neutral arenas required to resolve curve selections in sketches.
pub(crate) struct SketchCurveSelectionResolution<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Counted construction-operand groups.
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    /// Nested entity-selection operands.
    pub(crate) operands: &'a [DesignEntitySelectionOperand],
    /// Projected Sketch placement carriers.
    pub(crate) placements: &'a [DesignSketchPlacement],
    /// Persistent Sketch curve identities.
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    /// Neutral planar Sketches.
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    /// Neutral planar Sketch entities.
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    /// Neutral model-space Sketches for non-planar sketch carriers.
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    /// Neutral model-space entities for non-planar sketch carriers.
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

#[derive(Clone, Copy)]
struct EntitySelectionPathResolution<'a> {
    operands: &'a [DesignEntitySelectionOperand],
    placements: &'a [DesignSketchPlacement],
    curve_identities: &'a [SketchCurveIdentity],
    sketches: &'a [cadmpeg_ir::sketches::Sketch],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

impl<'a> SketchCurveSelectionResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.operands,
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

/// Bind exact Sweep sketch-profile and sole-curve path carriers.
pub(crate) fn bind_sweep_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef};
    let SketchCurveSelectionResolution {
        scopes,
        groups,
        operands,
        placements,
        curve_identities,
        sketches,
        sketch_entities,
        ..
    } = resolution;
    let path_resolution = resolution.path_resolution();
    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(native_ref) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };

                    let Some(scope) = unique_feature_match_for_decode(ctx, scopes.iter(),
                        |scope| {
                            ctx.equal(
                                scope.id.as_str(),
                                native_ref.as_ref(),
                                "f3d profile candidate identity comparison",
                            )
                        },
                        "f3d profile candidate source search",
                    )?
                    else {
                        break 'feature_edit;
                    };

                    let Some(stream) = native_stream_charged(ctx, &scope.id, "f3d profile native stream")? else {
                        break 'feature_edit;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::Sweep {
                        shape,
                        path,
                        guide_rail,
                        ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    {
                        let section = &mut *shape;
                        if let (Some(PlanarProfileRef::Native(group_id)), Some(profile_operand)) =
                            (section.referenced_profile(), scope.sweep_profile())
                        {
                            let group_id = group_id.as_str();
                            let group_matches = {
                                unique_feature_match_for_decode(ctx, groups.iter(),
                                    |group| {
                                        Ok(ctx.equal(
                                            group.id.as_str(),
                                            group_id.as_ref(),
                                            "f3d profile candidate identity comparison",
                                        )? && group.scope_record_index == scope.record_index
                                            && group.role() == DesignOperandRole::PROFILE
                                            && group
                                                .members()
                                                .iter()
                                                .map(|member| member.value)
                                                .eq([profile_operand.record_index])
                                            && ctx.equal(
                                                &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
                                                &Some(stream),
                                                "f3d profile candidate stream comparison",
                                            )?)
                                    },
                                    "f3d profile candidate source search",
                                )?
                                .is_some()
                            };
                            if group_matches {
                                if let Some(placement) = unique_feature_match_for_decode(ctx, placements.iter(),
                                    |placement| {
                                        Ok(ctx.equal(
                                            &native_stream_charged(ctx, &placement.id, "f3d profile native stream")?,
                                            &Some(stream),
                                            "f3d profile candidate stream comparison",
                                        )? && placement.entity_id.suffix()
                                            == profile_operand.entity_id.suffix())
                                    },
                                    "f3d profile candidate source search",
                                )? {
                                    let mut sketch_storage =
                                        ctx.reserve_scoped(0, "f3d sweep sketch identity storage")?;
                                    let sketch = sketch_storage.with_storage(|| {
                                        crate::design::identity::neutral_sketch_id(ctx, placement)
                                    })?;
                                    if ctx.any_by(
                                        *sketches,
                                        |candidate| {
                                            ctx.equal(
                                                &candidate.id,
                                                &sketch,
                                                "f3d selected sketch comparison",
                                            )
                                        },
                                        "f3d selected sketch search",
                                    )? {
                                        sketch_storage.commit()?;
                                        section.set_referenced_profile((sketch).into());
                                    }
                                }
                            }
                        }
                        if let Some(PlanarProfileRef::Native(group_id)) =
                            section.referenced_profile()
                        {
                            let group_id = group_id.as_str();
                            let mut resolved_storage =
                                ctx.reserve_scoped(0, "f3d sweep resolved entity storage")?;
                            let resolved = resolved_storage.with_storage(
                                || -> Result<Option<_>, CodecError> {
                                    let group = or_none!(unique_feature_match_for_decode(ctx, groups.iter(),
                                        |group| Ok(ctx.equal(
                                            group.id.as_str(),
                                            group_id.as_ref(),
                                            "f3d profile candidate identity comparison"
                                        )? && group.scope_record_index
                                            == scope.record_index
                                            && group.role() == DesignOperandRole::PROFILE
                                            && group.members().len() == 1
                                            && ctx.equal(
                                                &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
                                                &Some(stream),
                                                "f3d profile candidate stream comparison"
                                            )?),
                                        "f3d profile candidate source search"
                                    )?);

                                    let operand = or_none!(unique_feature_match_for_decode(ctx, operands.iter(),
                                        |operand| Ok(operand.scope_record_index
                                            == scope.record_index
                                            && operand.group_record_index == group.record_index
                                            && operand.group_member_ordinal == 0
                                            && operand.record_index() == group.members()[0].value
                                            && ctx.equal(
                                                &native_stream_charged(ctx, &operand.id, "f3d profile native stream")?,
                                                &Some(stream),
                                                "f3d profile candidate stream comparison"
                                            )?),
                                        "f3d profile candidate source search"
                                    )?);

                                    let placement = or_none!(unique_feature_match_for_decode(ctx, placements.iter(),
                                        |placement| Ok(ctx.equal(
                                            &native_stream_charged(ctx, &placement.id, "f3d profile native stream")?,
                                            &Some(stream),
                                            "f3d profile candidate stream comparison"
                                        )? && placement.entity_id.suffix()
                                            == operand.primary_identity),
                                        "f3d profile candidate source search"
                                    )?);

                                    let sketch =
                                        crate::design::identity::neutral_sketch_id(ctx, placement)?;
                                    if !ctx.any_by(
                                        *sketches,
                                        |candidate| {
                                            ctx.equal(
                                                &candidate.id,
                                                &sketch,
                                                "f3d selected sketch comparison",
                                            )
                                        },
                                        "f3d selected sketch search",
                                    )? {
                                        return Ok(None);
                                    }
                                    let owner_reference =
                                        or_none!(u32::try_from(operand.primary_identity).ok());

                                    let curve = or_none!(unique_feature_match_for_decode(ctx, curve_identities.iter(),
                                        |curve| Ok(ctx.equal(
                                            &native_stream_charged(ctx, &curve.id, "f3d profile native stream")?,
                                            &Some(stream),
                                            "f3d profile candidate stream comparison"
                                        )? && curve.owner_reference
                                            == Some(owner_reference)
                                            && entity_selection_matches_curve(operand, curve)),
                                        "f3d profile candidate source search"
                                    )?);

                                    let selected =
                                        crate::design::identity::neutral_sketch_curve_id(
                                            ctx,
                                            &sketch,
                                            curve.primary_id.get(),
                                            curve.secondary_id,
                                        )?;
                                    Ok(ctx
                                        .any_by(
                                            *sketch_entities,
                                            |entity| {
                                                Ok(ctx.equal(
                                                    &entity.sketch,
                                                    &sketch,
                                                    "f3d sweep entity sketch comparison",
                                                )? && ctx.equal(
                                                    entity.id(),
                                                    &selected,
                                                    "f3d sweep entity identity comparison",
                                                )?)
                                            },
                                            "f3d sweep entity search",
                                        )?
                                        .then_some((sketch, selected)))
                                },
                            )?;
                            if let Some((sketch, selected)) = resolved {
                                let mut entities = Vec::new();
                                ctx.push_vec(&mut entities, selected, "f3d sweep selected entity")?;
                                let profile =
                                    PlanarProfileRef::sketch_entities(sketch, entities, ctx)?
                                        .map_err(CodecError::malformed)?;
                                resolved_storage.commit()?;
                                section.set_referenced_profile(profile);
                            }
                        }
                        let resolve_path = |path: &PathRef| -> Result<Option<PathRef>, CodecError> {
                            let PathRef::Native(group_id) = path else {
                                return Ok(None);
                            };

                            let Some(group) = unique_feature_match_for_decode(ctx, groups.iter(),
                                |group| {
                                    Ok(ctx.equal(
                                        group.id.as_str(),
                                        group_id.as_ref(),
                                        "f3d profile candidate identity comparison",
                                    )? && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && ctx.equal(
                                            &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
                                            &Some(stream),
                                            "f3d profile candidate stream comparison",
                                        )?)
                                },
                                "f3d profile candidate source search",
                            )?
                            else {
                                return Ok(None);
                            };
                            if group.members().len() != 1 {
                                return Ok(None);
                            }
                            resolve_entity_selection_path(group, &path_resolution, ctx)
                        };
                        if let Some(path) = path {
                            if let Some(resolved) = resolve_path(path)? {
                                *path = resolved;
                            }
                        }
                        if let Some(guide_rail) = guide_rail {
                            if let Some(resolved) = resolve_path(&guide_rail.path)? {
                                guide_rail.path = resolved;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve `SplitFace` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_split_face_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool};

    let path_resolution = resolution.path_resolution();
    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let FeatureDefinition::Operation(FeatureOperation::SplitFace { tool, .. }) =
                        definition
                    else {
                        break 'feature_edit;
                    };
                    let SplitFaceTool::Path(PathRef::Native(group_id)) = tool else {
                        break 'feature_edit;
                    };

                    let Some(group) = unique_feature_match_for_decode(ctx, resolution.groups.iter(),
                        |group| {
                            Ok(ctx.equal(
                                group.id.as_str(),
                                group_id.as_ref(),
                                "f3d profile candidate identity comparison",
                            )? && group.role() == DesignOperandRole::ROLE_0X21
                                && !group.members().is_empty())
                        },
                        "f3d profile candidate source search",
                    )?
                    else {
                        break 'feature_edit;
                    };

                    if let Some(path) = resolve_entity_selection_path(group, &path_resolution, ctx)?
                    {
                        *tool = SplitFaceTool::Path(path);
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve `SurfaceTrim` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_surface_trim_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef};

    let path_resolution = resolution.path_resolution();
    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                        tool, ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    let PathRef::Native(group_id) = tool else {
                        break 'feature_edit;
                    };

                    let Some(group) = unique_feature_match_for_decode(ctx, resolution.groups.iter(),
                        |group| {
                            Ok(ctx.equal(
                                group.id.as_str(),
                                group_id.as_ref(),
                                "f3d profile candidate identity comparison",
                            )? && group.role() == DesignOperandRole::ROLE_0X21
                                && !group.members().is_empty())
                        },
                        "f3d profile candidate source search",
                    )?
                    else {
                        break 'feature_edit;
                    };

                    if let Some(path) = resolve_entity_selection_path(group, &path_resolution, ctx)?
                    {
                        *tool = path;
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};
    let ctx = resolution.ctx;

    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(scope) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };
                    let Some(scope) = ctx.find_by(
                        scopes,
                        |candidate| {
                            ctx.equal(candidate.id.as_str(), scope, "f3d extrude scope comparison")
                        },
                        "f3d extrude scope search",
                    )?
                    else {
                        break 'feature_edit;
                    };
                    let scoped_histories = histories_for_scope(
                        resolution.ctx,
                        &scope.id,
                        resolution.scope_histories,
                        resolution.histories,
                    )?;
                    let scoped_resolution = resolution.scoped(scoped_histories);
                    let effective_previous_history_state_id =
                        crate::history::effective_scope_previous_history_state_id(
                            resolution.ctx,
                            scope,
                            scoped_histories,
                        )?;
                    let mut matching_groups_storage =
                        ctx.reserve_scoped(0, "f3d extrude group storage")?;
                    let mut matching_groups = Vec::new();
                    for group in ctx.admit_iter(groups, "f3d profile source group scan")? {
                        if !ctx.equal(
                            &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
                            &native_stream_charged(ctx, &scope.id, "f3d profile native stream")?,
                            "f3d extrude group stream comparison",
                        )? || group.scope_record_index != scope.record_index
                        {
                            continue;
                        }
                        matching_groups_storage.with_storage(|| {
                            (resolution.ctx).push_vec(
                                &mut matching_groups,
                                group,
                                "f3d extrude matching selection group",
                            )
                        })?;
                    }
                    resolution.ctx.stable_sort_by_key(
                        &mut matching_groups[..],
                        |value| {
                            let group = value;
                            group.scope_reference_ordinal
                        },
                        Ord::cmp,
                        "sort f3d design profile_select 1",
                    )?;
                    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                        definition
                    else {
                        break 'feature_edit;
                    };
                    if let ProfileRef::Planar(PlanarProfileRef::Native(native)) = profile {
                        if let Some(group) = unique_feature_match_for_decode(ctx, curve_resolution.groups.iter(),
                            |group| {
                                Ok(ctx.equal(
                                    group.id.as_str(),
                                    native.as_ref(),
                                    "f3d profile candidate identity comparison",
                                )? && ctx.equal(
                                    &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
                                    &native_stream_charged(ctx, &scope.id, "f3d profile native stream")?,
                                    "f3d profile candidate stream comparison",
                                )? && group.scope_record_index == scope.record_index)
                            },
                            "f3d profile candidate source search",
                        )? {
                            if let Some(selection) = resolve_entity_selection_profile(
                                group,
                                &curve_resolution.path_resolution(),
                                resolution.ctx,
                            )? {
                                *profile = selection;
                                break 'feature_edit;
                            }
                        }
                        if let Some(selection) = historical_face_profile_selection(
                            &matching_groups,
                            members,
                            effective_previous_history_state_id,
                            &feature.id,
                            scoped_histories,
                            resolution.ctx,
                        )? {
                            *profile = ProfileRef::Planar(selection);
                        }
                        break 'feature_edit;
                    }
                    let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) = profile else {
                        break 'feature_edit;
                    };
                    let Some(sketch) = ctx.find_by(
                        sketches,
                        |sketch| ctx.equal(&sketch.id, sketch_id, "f3d extrude sketch comparison"),
                        "f3d extrude sketch search",
                    )?
                    else {
                        if matching_groups.is_empty() {
                            break 'feature_edit;
                        }
                        let spatial_id_parts = resolution.ctx.split_once(
                            sketch_id.as_str(),
                            "f3d:model:sketch#",
                            "f3d spatial profile sketch identity split",
                        )?;
                        if let Some(spatial_sketch) = resolution.ctx.find_by(
                            resolution.spatial_sketches,
                            |candidate| match spatial_id_parts {
                                Some((before, after)) => {
                                    let Some(rest) = resolution.ctx.strip_prefix(
                                        candidate.id.as_str(),
                                        before,
                                        "f3d spatial profile identity prefix",
                                    )?
                                    else {
                                        return Ok(false);
                                    };
                                    match resolution.ctx.strip_prefix(
                                        rest,
                                        "f3d:model:spatial-sketch#",
                                        "f3d spatial profile namespace prefix",
                                    )? {
                                        Some(suffix) => resolution.ctx.equal(
                                            suffix,
                                            after,
                                            "f3d spatial profile identity suffix",
                                        ),
                                        None => Ok(false),
                                    }
                                }
                                None => resolution.ctx.equal(
                                    candidate.id.as_str(),
                                    sketch_id.as_str(),
                                    "f3d spatial profile identity",
                                ),
                            },
                            "f3d spatial profile sketch search",
                        )? {
                            let mut selection_storage =
                                ctx.reserve_scoped(0, "f3d extrude group selection storage")?;
                            let mut selections = Vec::new();
                            for group in
                                ctx.admit_iter(&matching_groups, "f3d extrude matching group scan")?
                            {
                                let selection = resolved_spatial_extrude_profile_selection(
                                    group,
                                    members,
                                    spatial_sketch,
                                    scoped_resolution.spatial_entities,
                                    scoped_resolution,
                                    scope.history_state_id(),
                                    effective_previous_history_state_id,
                                )?;
                                selection_storage.with_storage(|| {
                                    (scoped_resolution.ctx).push_vec(
                                        &mut selections,
                                        selection,
                                        "f3d spatial profile selection",
                                    )
                                })?;
                            }
                            let mut index_storage =
                                ctx.reserve_scoped(0, "f3d extrude spatial index storage")?;
                            let mut indices = Vec::new();
                            let mut all_resolved = true;
                            let mut selection_source = selections.iter();
                            while let Some(selection) = ctx
                                .next_charged(&mut selection_source, "f3d spatial selection scan")?
                            {
                                let Some(index) = selection else {
                                    all_resolved = false;
                                    break;
                                };
                                if !ctx.contains(
                                    &indices,
                                    index,
                                    "f3d spatial selected index duplicate search",
                                )? {
                                    index_storage.with_storage(|| {
                                        (resolution.ctx).push_vec(
                                            &mut indices,
                                            *index,
                                            "f3d spatial extrude profile index",
                                        )
                                    })?;
                                }
                            }
                            if all_resolved {
                                let sketch_id = index_storage.with_storage(|| {
                                    spatial_sketch.id.try_clone_for_decode(
                                        resolution.ctx,
                                        "f3d profile spatial sketch id",
                                    )
                                })?;
                                *profile = match index_storage.with_storage(
                                    || -> Result<_, CodecError> {
                                        Ok(ProfileRef::spatial_sketch_profiles(
                                            sketch_id,
                                            indices,
                                            resolution.ctx,
                                        )?)
                                    },
                                )? {
                                    Ok(profile) => {
                                        index_storage.commit()?;
                                        profile
                                    }
                                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                        (resolution.ctx).copy_retained_text(
                                            &scope.id,
                                            "f3d spatial extrude fallback scope id",
                                        )?,
                                    )),
                                };
                            } else {
                                let mut group_storage =
                                    ctx.reserve_scoped(0, "f3d extrude group output storage")?;
                                let mut group_ids = Vec::new();
                                for group in ctx.admit_iter(
                                    &matching_groups,
                                    "f3d extrude matching group scan",
                                )? {
                                    let id = group_storage.with_storage(|| {
                                        (resolution.ctx).copy_retained_text(
                                            &group.id,
                                            "f3d spatial extrude selection group id",
                                        )
                                    })?;
                                    group_storage.with_storage(|| {
                                        (resolution.ctx).push_vec(
                                            &mut group_ids,
                                            id,
                                            "f3d spatial extrude selection group",
                                        )
                                    })?;
                                }
                                let sketch_id = group_storage.with_storage(|| {
                                    spatial_sketch.id.try_clone_for_decode(
                                        resolution.ctx,
                                        "f3d profile spatial sketch id",
                                    )
                                })?;
                                *profile = match group_storage.with_storage(
                                    || -> Result<_, CodecError> {
                                        Ok(ProfileRef::spatial_sketch_selection(
                                            sketch_id,
                                            group_ids,
                                            resolution.ctx,
                                        )?)
                                    },
                                )? {
                                    Ok(profile) => {
                                        group_storage.commit()?;
                                        profile
                                    }
                                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                        (resolution.ctx).copy_retained_text(
                                            &scope.id,
                                            "f3d spatial extrude fallback scope id",
                                        )?,
                                    )),
                                };
                            }
                            break 'feature_edit;
                        }
                        let id = match matching_groups.as_slice() {
                            [group] => &group.id,
                            _ => &scope.id,
                        };
                        *profile = ProfileRef::Planar(PlanarProfileRef::Native(
                            (resolution.ctx)
                                .copy_retained_text(id, "f3d extrude unresolved selection id")?,
                        ));
                        break 'feature_edit;
                    };
                    if let (Some(profile_operand), Some(stream)) =
                        (scope.extrude_profile(), native_stream_charged(ctx, &scope.id, "f3d profile native stream")?)
                    {
                        if let Some(profiles) = resolved_sketch_profile_regions(
                            stream,
                            profile_operand,
                            sketch,
                            curve_resolution.curve_identities,
                            curve_resolution.sketch_entities,
                            resolution.ctx,
                        )? {
                            let sketch_id = (sketch_id)
                                .try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?;
                            *profile = ProfileRef::Planar(match PlanarProfileRef::sketch_profiles(
                                sketch_id,
                                profiles,
                                resolution.ctx,
                            )? {
                                Ok(profile) => profile,
                                Err(_) => {
                                    PlanarProfileRef::Native((resolution.ctx).copy_retained_text(
                                        &scope.id,
                                        "f3d extrude profile fallback scope id",
                                    )?)
                                }
                            });
                            break 'feature_edit;
                        }
                    }
                    if matching_groups.is_empty() {
                        break 'feature_edit;
                    }
                    let mut selection_storage =
                        ctx.reserve_scoped(0, "f3d extrude group selection storage")?;
                    let mut selections = Vec::new();
                    for group in
                        ctx.admit_iter(&matching_groups, "f3d extrude matching group scan")?
                    {
                        let selection = selection_storage.with_storage(|| {
                            resolved_extrude_profile_selection(
                                sketch_id,
                                group,
                                members,
                                sketch,
                                scoped_resolution,
                                scope.history_state_id(),
                                effective_previous_history_state_id,
                            )
                        })?;
                        selection_storage.with_storage(|| {
                            (resolution.ctx).push_vec(
                                &mut selections,
                                selection,
                                "f3d extrude resolved selection",
                            )
                        })?;
                    }
                    *profile = if let Some(merged) =
                        merge_resolved_profile_selections(sketch_id, &selections, resolution.ctx)?
                    {
                        merged
                    } else {
                        let mut group_storage =
                            ctx.reserve_scoped(0, "f3d extrude group output storage")?;
                        let mut group_ids = Vec::new();
                        for group in
                            ctx.admit_iter(&matching_groups, "f3d extrude matching group scan")?
                        {
                            let id = group_storage.with_storage(|| {
                                (resolution.ctx)
                                    .copy_retained_text(&group.id, "f3d extrude fallback group id")
                            })?;
                            group_storage.with_storage(|| {
                                (resolution.ctx).push_vec(
                                    &mut group_ids,
                                    id,
                                    "f3d extrude fallback group",
                                )
                            })?;
                        }
                        let sketch_id = group_storage.with_storage(|| {
                            (sketch_id)
                                .try_clone_for_decode(resolution.ctx, "f3d profile sketch id")
                        })?;
                        let fallback =
                            match group_storage.with_storage(|| -> Result<_, CodecError> {
                                Ok(PlanarProfileRef::sketch_selection(
                                    sketch_id,
                                    group_ids,
                                    resolution.ctx,
                                )?)
                            })? {
                                Ok(profile) => {
                                    group_storage.commit()?;
                                    profile
                                }
                                Err(_) => {
                                    PlanarProfileRef::Native((resolution.ctx).copy_retained_text(
                                        &scope.id,
                                        "f3d extrude fallback scope id",
                                    )?)
                                }
                            };
                        ProfileRef::Planar(fallback)
                    };
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn resolve_entity_selection_profile(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::{PathRef, PlanarProfileRef, ProfileRef};

    if group.role() != DesignOperandRole::PROFILE {
        return Ok(None);
    }
    let mut path_storage = ctx.reserve_scoped(0, "f3d entity profile path storage")?;
    let path =
        path_storage.with_storage(|| resolve_entity_selection_path(group, resolution, ctx))?;
    let Some(path) = path else {
        return Ok(None);
    };
    match path {
        PathRef::SketchCurves { sketch, curves } => {
            let Some(source) = ctx.find_by(
                resolution.sketches,
                |source| ctx.equal(&source.id, &sketch, "f3d entity profile sketch comparison"),
                "f3d entity profile sketch search",
            )?
            else {
                return Ok(None);
            };
            let mut selected_storage =
                ctx.reserve_scoped(0, "f3d entity selected planar profile storage")?;
            let mut selected_profiles = Vec::new();
            let mut has_unprofiled_entity = false;
            let mut curve_source = curves.iter();
            while let Some(curve) = ctx.next_charged(&mut curve_source, "f3d curve scan")? {
                let mut profiles = source.profiles.iter().enumerate();
                let matches_curve =
                    |(_, profile): (usize, &Vec<cadmpeg_ir::sketches::SketchEntityUse>)| {
                        ctx.any_by(
                            profile,
                            |use_| {
                                ctx.equal(
                                    &use_.entity,
                                    curve,
                                    "f3d profile candidate boundary identity",
                                )
                            },
                            "f3d profile candidate boundary search",
                        )
                    };
                let Some((profile_index, _)) = ctx.find_by(
                    &mut profiles,
                    |value| matches_curve(*value),
                    "f3d profile candidate source search",
                )?
                else {
                    has_unprofiled_entity = true;
                    continue;
                };
                if ctx.any_by(
                    profiles,
                    matches_curve,
                    "f3d profile candidate source search",
                )? {
                    return Ok(None);
                }
                let Ok(profile_index) = u32::try_from(profile_index) else {
                    return Ok(None);
                };
                if !ctx.contains(
                    &selected_profiles,
                    &profile_index,
                    "f3d selected planar profile duplicate search",
                )? {
                    selected_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut selected_profiles,
                            profile_index,
                            "f3d entity path selected planar profile",
                        )
                    })?;
                }
            }
            if has_unprofiled_entity {
                path_storage.commit()?;
                Ok(Some(ProfileRef::Planar(PlanarProfileRef::SketchEntities {
                    sketch,
                    entities: curves,
                })))
            } else {
                let selected = selected_storage.with_storage(|| -> Result<_, CodecError> {
                    let sketch =
                        sketch.try_clone_for_decode(ctx, "f3d entity profile selected sketch")?;
                    Ok(
                        PlanarProfileRef::sketch_profiles(sketch, selected_profiles, ctx)?
                            .ok()
                            .map(ProfileRef::Planar),
                    )
                })?;
                if selected.is_some() {
                    selected_storage.commit()?;
                }
                Ok(selected)
            }
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let Some(source) = ctx.find_by(
                resolution.spatial_sketches,
                |source| {
                    ctx.equal(
                        &source.id,
                        &sketch,
                        "f3d entity spatial profile sketch comparison",
                    )
                },
                "f3d entity spatial profile sketch search",
            )?
            else {
                return Ok(None);
            };
            let mut selected_storage =
                ctx.reserve_scoped(0, "f3d entity selected spatial profile storage")?;
            let mut profiles = Vec::new();
            let mut curve_source = curves.iter();
            while let Some(curve) = ctx.next_charged(&mut curve_source, "f3d curve scan")? {
                let Some((index, _)) = unique_feature_match_for_decode(ctx, source.profiles.iter().enumerate(),
                    |(_, profile)| {
                        ctx.any_by(
                            profile.boundary(),
                            |use_| {
                                ctx.equal(
                                    &use_.entity,
                                    curve,
                                    "f3d profile candidate boundary identity",
                                )
                            },
                            "f3d profile candidate boundary search",
                        )
                    },
                    "f3d profile candidate source search",
                )?
                else {
                    return Ok(None);
                };

                let Ok(index) = u32::try_from(index) else {
                    return Ok(None);
                };
                if !ctx.contains(
                    &profiles,
                    &index,
                    "f3d selected spatial profile duplicate search",
                )? {
                    selected_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut profiles,
                            index,
                            "f3d entity path selected spatial profile",
                        )
                    })?;
                }
            }
            if profiles.is_empty() {
                return Ok(None);
            }
            let selected = selected_storage.with_storage(|| -> Result<_, CodecError> {
                let sketch = sketch
                    .try_clone_for_decode(ctx, "f3d entity profile selected spatial sketch")?;
                Ok(ProfileRef::spatial_sketch_profiles(sketch, profiles, ctx)?.ok())
            })?;
            if selected.is_some() {
                selected_storage.commit()?;
            }
            Ok(selected)
        }
        _ => Ok(None),
    }
}

fn historical_face_profile_selection(
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scoped_histories: &[crate::history_records::AsmHistory],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PlanarProfileRef>, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    let mut histories = scoped_histories.iter();
    let mut states: std::slice::Iter<'_, crate::history_records::AsmDeltaState> = [].iter();
    let mut next_state =
        || -> Result<Option<&crate::history_records::AsmDeltaState>, CodecError> {
            loop {
                if let Some(state) = ctx.find_by(
                    &mut states,
                    |state| Ok(state.state_id == previous_state_id),
                    "f3d historical face state search",
                )? {
                    return Ok(Some(state));
                }
                let Some(history) =
                    ctx.next_charged(&mut histories, "f3d historical face history search")?
                else {
                    return Ok(None);
                };
                states = history.states.iter();
            }
        };
    let Some(topology) = next_state()?.and_then(|state| state.topology()) else {
        return Ok(None);
    };
    if next_state()?.is_some() {
        return Ok(None);
    }
    let Some(first_group) = groups.first() else { return Ok(None); };
    let Some(stream) = native_stream_charged(ctx, &first_group.id, "f3d profile native stream")? else {
        return Ok(None);
    };
    let mut selected_faces_storage =
        ctx.reserve_scoped(0, "f3d historical selected face storage")?;
    let mut selected_faces = Vec::new();
    let mut group_source = groups.iter();
    while let Some(group) = ctx.next_charged(&mut group_source, "f3d historical group scan")? {
        if !ctx.equal(
            &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
            &Some(stream),
            "f3d historical group stream comparison",
        )? {
            return Ok(None);
        }
        let mut group_members_storage =
            ctx.reserve_scoped(0, "f3d profile group member storage")?;
        let mut group_members = Vec::new();
        for member in ctx.admit_iter(members, "f3d profile source member scan")? {
            if !ctx.equal(
                &native_stream_charged(ctx, &member.id, "f3d profile native stream")?,
                &Some(stream),
                "f3d profile member stream comparison",
            )? || member.group_record_index != group.record_index
            {
                continue;
            }
            group_members_storage.with_storage(|| {
                ctx.push_vec(
                    &mut group_members,
                    member,
                    "f3d historical profile group member",
                )
            })?;
        }
        ctx.stable_sort_by_key(
            &mut group_members[..],
            |value| {
                let member = value;
                {
                    member.group_member_ordinal
                }
            },
            Ord::cmp,
            "sort f3d design profile_select 2",
        )?;
        if group_members.len() != group.members().len()
            || ctx.any_by(
                group_members.iter().zip(group.members()),
                |(member, record_index)| Ok(member.record_index() != record_index.value),
                "f3d historical member run comparison",
            )?
        {
            return Ok(None);
        }
        let mut candidates: Option<(BTreeSet<i64>, ScopedReservation<'_>)> = None;
        let mut member_source = group_members.into_iter();
        while let Some(member) =
            ctx.next_charged(&mut member_source, "f3d ordered group member scan")?
        {
            let (kind, entity_ref) = match &member.historical {
                Some(binding) => {
                    if !binding.state_ids.is_empty()
                        && !ctx.contains(
                            &binding.state_ids,
                            &previous_state_id,
                            "visit F3D historical profile binding state",
                        )?
                    {
                        return Ok(None);
                    }
                    (Some(binding.kind), binding.entity_ref)
                }
                None => {
                    let Ok(entity_ref) = i64::try_from(member.local_id) else {
                        return Ok(None);
                    };
                    (None, entity_ref)
                }
            };
            let mut member_face_storage =
                ctx.reserve_scoped(0, "f3d historical profile member face storage")?;
            let member_faces = member_face_storage.with_storage(|| {
                historical_profile_face_candidates(kind, entity_ref, topology, ctx)
            })?;
            if member_faces.is_empty() {
                return Ok(None);
            }
            candidates = Some(match candidates {
                None => (member_faces, member_face_storage),
                Some(mut candidates) => {
                    ctx.retain_btree_set(
                        &mut candidates.0,
                        |face| {
                            ctx.contains_btree_set(
                                &member_faces,
                                face,
                                "f3d historical profile candidate membership",
                            )
                        },
                        "f3d historical profile candidate intersection",
                    )?;
                    candidates
                }
            });
        }
        let Some(candidates) = candidates else {
            return Ok(None);
        };
        if candidates.0.len() != 1 {
            return Ok(None);
        }
        let mut candidate_faces = candidates.0.iter();
        let Some(&face) = ctx.next_charged(
            &mut candidate_faces,
            "f3d historical profile sole candidate",
        )?
        else {
            return Ok(None);
        };
        if !ctx.contains(
            &selected_faces,
            &face,
            "f3d historical selected face duplicate search",
        )? {
            selected_faces_storage.with_storage(|| {
                ctx.push_vec(
                    &mut selected_faces,
                    face,
                    "f3d historical profile selected face",
                )
            })?;
        }
    }
    if selected_faces.is_empty() {
        return Ok(None);
    }
    let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
    let mut temporary_prefix_storage =
        ctx.reserve_scoped(0, "f3d historical face prefix storage")?;
    let prefix = temporary_prefix_storage.with_storage(|| {
        crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id)
    })?;
    let mut output_storage = ctx.reserve_scoped(0, "f3d historical profile output storage")?;
    let selection = output_storage.with_storage(|| -> Result<_, CodecError> {
        let mut face_ids = Vec::new();
        for face in ctx.admit_iter(selected_faces, "f3d historical selected face scan")? {
            let id = crate::design::identity::history_input_face_id(
                ctx,
                &prefix,
                face,
                "f3d historical face identifier",
            )?;
            ctx.push_vec(&mut face_ids, id, "f3d historical profile face id")?;
        }
        let mut group_ids = Vec::new();
        for group in ctx.admit_iter(groups, "f3d historical group id scan")? {
            let id = ctx.copy_retained_text(&group.id, "f3d historical profile group id")?;
            ctx.push_vec(&mut group_ids, id, "f3d historical profile group id entry")?;
        }
        Ok(PlanarProfileRef::historical_faces(
            crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?,
            face_ids,
            group_ids,
            ctx,
        )?
        .ok())
    })?;
    if selection.is_some() {
        output_storage.commit()?;
    }
    Ok(selection)
}

fn historical_profile_face_candidates(
    kind: Option<crate::records::topology::body_recipe::AsmHistoricalEntityKind>,
    entity_ref: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<i64>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let all_kinds = [
        AsmHistoricalEntityKind::Face,
        AsmHistoricalEntityKind::Loop,
        AsmHistoricalEntityKind::Coedge,
        AsmHistoricalEntityKind::Edge,
        AsmHistoricalEntityKind::Pcurve,
        AsmHistoricalEntityKind::Curve,
        AsmHistoricalEntityKind::Vertex,
        AsmHistoricalEntityKind::Point,
        AsmHistoricalEntityKind::Surface,
    ];
    let kinds = kind
        .iter()
        .chain(all_kinds.iter().filter(|_| kind.is_none()));
    let loop_faces = |loop_ref| {
        let mut storage = ctx.reserve_scoped(0, "f3d historical face candidate storage")?;
        let mut faces = BTreeSet::new();
        for relation in ctx.admit_iter(&topology.face_loops, "f3d historical face loops scan")? {
            if !ctx.any_by(
                &relation.member_refs,
                |member| Ok(*member == loop_ref),
                "f3d historical loop membership",
            )? {
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut faces,
                    relation.owner_ref,
                    "f3d historical loop face candidate",
                )
            })?;
        }
        Ok::<_, CodecError>((faces, storage))
    };
    let coedge_faces = |coedge_ref| {
        let mut storage = ctx.reserve_scoped(0, "f3d historical face candidate storage")?;
        let mut faces = BTreeSet::new();
        for coedge in ctx
            .admit_iter(
                &topology.coedge_topology,
                "f3d historical coedge topology scan",
            )?
            .filter(|coedge| coedge.coedge == coedge_ref)
        {
            let query_faces = loop_faces(coedge.owner_loop)?;
            for &face in ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")? {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut faces, face, "f3d historical coedge face candidate")
                })?;
            }
        }
        Ok::<_, CodecError>((faces, storage))
    };
    let edge_faces = |edge_ref| {
        let mut storage = ctx.reserve_scoped(0, "f3d historical face candidate storage")?;
        let mut faces = BTreeSet::new();
        for coedge in ctx
            .admit_iter(
                &topology.coedge_topology,
                "f3d historical coedge topology scan",
            )?
            .filter(|coedge| coedge.edge == edge_ref)
        {
            let query_faces = loop_faces(coedge.owner_loop)?;
            for &face in ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")? {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut faces, face, "f3d historical edge face candidate")
                })?;
            }
        }
        Ok::<_, CodecError>((faces, storage))
    };
    let mut storage = ctx.reserve_scoped(0, "f3d historical face candidate storage")?;
    let mut faces = BTreeSet::new();
    for kind in kinds {
        match kind {
            AsmHistoricalEntityKind::Face => {
                if ctx.any_by(
                    &topology.faces,
                    |face| Ok(*face == entity_ref),
                    "f3d historical face membership",
                )? {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut faces,
                            entity_ref,
                            "f3d historical profile face candidate",
                        )
                    })?;
                }
            }
            AsmHistoricalEntityKind::Loop => {
                let query_faces = loop_faces(entity_ref)?;
                for &face in
                    ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )
                    })?;
                }
            }
            AsmHistoricalEntityKind::Coedge => {
                let query_faces = coedge_faces(entity_ref)?;
                for &face in
                    ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )
                    })?;
                }
            }
            AsmHistoricalEntityKind::Edge => {
                let query_faces = edge_faces(entity_ref)?;
                for &face in
                    ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )
                    })?;
                }
            }
            AsmHistoricalEntityKind::Pcurve => {
                for binding in ctx
                    .admit_iter(
                        &topology.coedge_pcurves,
                        "f3d historical coedge pcurves scan",
                    )?
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    let query_faces = coedge_faces(binding.entity)?;
                    for &face in
                        ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                    {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut faces,
                                face,
                                "f3d historical profile face candidate",
                            )
                        })?;
                    }
                }
            }
            AsmHistoricalEntityKind::Curve => {
                for binding in ctx
                    .admit_iter(&topology.edge_curves, "f3d historical edge curves scan")?
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    let query_faces = edge_faces(binding.entity)?;
                    for &face in
                        ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                    {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut faces,
                                face,
                                "f3d historical profile face candidate",
                            )
                        })?;
                    }
                }
            }
            AsmHistoricalEntityKind::Vertex => {
                for edge in ctx
                    .admit_iter(&topology.edge_vertices, "f3d historical edge vertices scan")?
                    .filter(|edge| edge.start_vertex == entity_ref || edge.end_vertex == entity_ref)
                {
                    let query_faces = edge_faces(edge.edge)?;
                    for &face in
                        ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                    {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut faces,
                                face,
                                "f3d historical profile face candidate",
                            )
                        })?;
                    }
                }
            }
            AsmHistoricalEntityKind::Point => {
                let mut vertex_storage =
                    ctx.reserve_scoped(0, "f3d historical point vertex storage")?;
                let mut vertices = HashSet::new();
                for binding in ctx
                    .admit_iter(&topology.vertex_points, "f3d historical vertex points scan")?
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    vertex_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut vertices,
                            binding.entity,
                            "f3d historical point vertex candidate",
                        )
                    })?;
                }
                for edge in ctx
                    .admit_iter(&topology.edge_vertices, "f3d historical edge vertices scan")?
                    .filter(|edge| {
                        vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                    })
                {
                    let query_faces = edge_faces(edge.edge)?;
                    for &face in
                        ctx.admit_iter(&query_faces.0, "f3d historical face candidate merge")?
                    {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut faces,
                                face,
                                "f3d historical profile face candidate",
                            )
                        })?;
                    }
                }
            }
            AsmHistoricalEntityKind::Surface => {
                for binding in ctx
                    .admit_iter(&topology.face_surfaces, "f3d historical face surfaces scan")?
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut faces,
                            binding.entity,
                            "f3d historical profile face candidate",
                        )
                    })?;
                }
            }
            AsmHistoricalEntityKind::Body
            | AsmHistoricalEntityKind::Region
            | AsmHistoricalEntityKind::Shell => {}
        }
    }
    storage.commit()?;
    Ok(faces)
}

#[derive(Debug, Clone, PartialEq)]
enum ResolvedProfileSelection {
    Loops(Vec<u32>),
    Regions(Vec<cadmpeg_ir::features::SketchProfileRegion>),
}

fn merge_resolved_profile_selections(
    sketch: &cadmpeg_ir::sketches::SketchId,
    selections: &[cadmpeg_ir::features::ProfileRef],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let mut profile_storage = ctx.reserve_scoped(0, "f3d merged profile index storage")?;
    let mut profiles = Vec::new();
    let mut region_storage = ctx.reserve_scoped(0, "f3d merged region reference storage")?;
    let mut regions = Vec::new();
    let mut selections = selections.iter();
    while let Some(selection) = ctx.next_charged(&mut selections, "f3d merged selection scan")? {
        match selection {
            ProfileRef::Planar(PlanarProfileRef::SketchProfiles {
                sketch: selected,
                profiles: selected_profiles,
            }) if regions.is_empty()
                && ctx.equal(selected, sketch, "f3d merged sketch comparison")? =>
            {
                for profile in
                    ctx.admit_iter(selected_profiles.as_slice(), "f3d merged profile scan")?
                {
                    if !ctx.contains(&profiles, profile, "f3d merged profile duplicate search")? {
                        profile_storage.with_storage(|| {
                            ctx.push_vec(&mut profiles, *profile, "f3d merged selected profile")
                        })?;
                    }
                }
            }
            ProfileRef::Planar(PlanarProfileRef::SketchRegions {
                sketch: selected,
                regions: selected_regions,
            }) if profiles.is_empty()
                && ctx.equal(selected, sketch, "f3d merged sketch comparison")? =>
            {
                for region in
                    ctx.admit_iter(selected_regions.as_slice(), "f3d merged region scan")?
                {
                    if !ctx.any_by(
                        &regions,
                        |previous| ctx.equal(*previous, region, "f3d merged region comparison"),
                        "f3d merged region duplicate search",
                    )? {
                        region_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut regions,
                                region,
                                "f3d merged selected region reference",
                            )
                        })?;
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    let selected = if !profiles.is_empty() {
        let sketch = sketch.try_clone_for_decode(ctx, "f3d profile sketch id")?;
        let selected = PlanarProfileRef::sketch_profiles(sketch, profiles, ctx)?.ok();
        if selected.is_some() {
            profile_storage.commit()?;
        }
        selected
    } else if !regions.is_empty() {
        let mut copies = Vec::new();
        for region in ctx.admit_iter(&regions, "f3d merged kept region scan")? {
            let copy = copy_profile_region(region, ctx)?;
            ctx.push_vec(&mut copies, copy, "f3d merged selected region")?;
        }
        let sketch = sketch.try_clone_for_decode(ctx, "f3d profile sketch id")?;
        PlanarProfileRef::sketch_regions(sketch, copies).ok()
    } else {
        return Ok(None);
    };
    Ok(selected.map(ProfileRef::Planar))
}

fn copy_bound_profile(
    profile: &cadmpeg_ir::features::ProfileRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Planar(planar) => {
            Ok(ProfileRef::Planar(copy_bound_planar_profile(planar, ctx)?))
        }
        ProfileRef::SpatialSketchProfiles { sketch, profiles } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut indices = Vec::new();
            for index in ctx.admit_iter(profiles.as_slice(), "f3d bound spatial profile scan")? {
                ctx.push_vec(&mut indices, *index, "f3d bound spatial profile index")?;
            }
            ProfileRef::spatial_sketch_profiles(sketch, indices, ctx)?
                .map_err(CodecError::malformed)
        }
        ProfileRef::SpatialSketchSelection { sketch, selections } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut ids = Vec::new();
            for selection in
                ctx.admit_iter(selections.as_slice(), "f3d bound spatial selection scan")?
            {
                let id = ctx.copy_retained_text(selection, "f3d bound spatial selection id")?;
                ctx.push_vec(&mut ids, id, "f3d bound spatial selection")?;
            }
            ProfileRef::spatial_sketch_selection(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
    }
}

fn copy_bound_planar_profile(
    profile: &cadmpeg_ir::features::PlanarProfileRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PlanarProfileRef, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    match profile {
        PlanarProfileRef::Sketch(sketch) => Ok(PlanarProfileRef::Sketch(
            (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?,
        )),
        PlanarProfileRef::Native(id) => Ok(PlanarProfileRef::Native(
            ctx.copy_retained_text(id, "f3d bound native profile id")?,
        )),
        _ => Err(CodecError::malformed(
            "bound planar profile has unsupported resolved form",
        )),
    }
}

fn copy_bound_path(
    path: &cadmpeg_ir::features::PathRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::SketchCurves { sketch, curves } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?;
            let mut ids = Vec::new();
            for curve in ctx.admit_iter(curves.as_slice(), "f3d bound path curve scan")? {
                let id = (curve).try_clone_for_decode(ctx, "f3d bound planar curve id")?;
                ctx.push_vec(&mut ids, id, "f3d bound planar path curve")?;
            }
            PathRef::sketch_curves(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut ids = Vec::new();
            for curve in ctx.admit_iter(curves.as_slice(), "f3d bound path curve scan")? {
                let id = (curve).try_clone_for_decode(ctx, "f3d bound spatial curve id")?;
                ctx.push_vec(&mut ids, id, "f3d bound spatial path curve")?;
            }
            PathRef::spatial_sketch_curves(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
        _ => Err(CodecError::malformed(
            "bound path has unsupported resolved form",
        )),
    }
}

pub(super) fn resolved_extrude_profile_selection(
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};
    let ctx = resolution.ctx;

    let mut selection_members_storage = ctx.reserve_scoped(0, "f3d extrude member storage")?;
    let mut selection_members = Vec::new();
    for member in ctx.admit_iter(members, "f3d profile source member scan")? {
        if !ctx.equal(
            &native_stream_charged(ctx, &member.id, "f3d profile native stream")?,
            &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
            "f3d profile member stream comparison",
        )? || member.group_record_index != group.record_index
        {
            continue;
        }
        selection_members_storage.with_storage(|| {
            (resolution.ctx).push_vec(
                &mut selection_members,
                member,
                "f3d extrude selection member",
            )
        })?;
    }
    resolution.ctx.stable_sort_by_key(
        &mut selection_members[..],
        |value| {
            let member = value;
            {
                member.group_member_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design profile_select 3",
    )?;
    let exact_member_run = selection_members.len() == group.members().len()
        && ctx.all_by(
            selection_members.iter().zip(group.members()),
            |(member, record_index)| Ok(member.record_index() == record_index.value),
            "f3d profile member run comparison",
        )?;
    let mut resolved_storage = ctx.reserve_scoped(0, "f3d extrude candidate storage")?;
    let resolved_profiles = resolved_storage.with_storage(
        || -> Result<Option<ResolvedProfileSelection>, CodecError> {
            let mut resolved_profiles = None;
            if exact_member_run {
                let mut selected_storage =
                    ctx.reserve_scoped(0, "f3d extrude exact selection storage")?;
                let mut selected = Vec::new();
                let mut complete = true;
                let mut member_source = selection_members.iter();
                while let Some(member) =
                    ctx.next_charged(&mut member_source, "f3d extrude member scan")?
                {
                    let Some(geometry) = member.resolved_geometry.as_ref() else {
                        complete = false;
                        break;
                    };
                    let SketchRelationOperand::Curve {
                        primary_id,
                        secondary_id,
                        ..
                    } = geometry
                    else {
                        complete = false;
                        break;
                    };
                    let mut temporary_entity_storage =
                        ctx.reserve_scoped(0, "f3d extrude curve query storage")?;
                    let entity = temporary_entity_storage.with_storage(|| {
                        crate::design::identity::neutral_sketch_curve_id(
                            resolution.ctx,
                            sketch_id,
                            *primary_id,
                            *secondary_id,
                        )
                    })?;

                    let Some((index, _)) = unique_feature_match_for_decode(ctx, sketch.profiles.iter().enumerate(),
                        |(_, profile)| {
                            ctx.any_by(
                                profile,
                                |use_| {
                                    ctx.equal(
                                        &use_.entity,
                                        &entity,
                                        "f3d profile candidate boundary identity",
                                    )
                                },
                                "f3d profile candidate boundary search",
                            )
                        },
                        "f3d profile candidate source search",
                    )?
                    else {
                        complete = false;
                        break;
                    };

                    let Ok(profile_index) = u32::try_from(index) else {
                        complete = false;
                        break;
                    };
                    if !ctx.contains(
                        &selected,
                        &profile_index,
                        "f3d extrude selected profile duplicate search",
                    )? {
                        selected_storage.with_storage(|| {
                            (resolution.ctx).push_vec(
                                &mut selected,
                                profile_index,
                                "f3d extrude selected profile",
                            )
                        })?;
                    }
                }
                if complete && !selected.is_empty() {
                    selected_storage.commit()?;
                    resolved_profiles = Some(ResolvedProfileSelection::Loops(selected));
                }
            }
            if resolved_profiles.is_none() && exact_member_run {
                resolved_profiles = historical_selection_regions(
                    &selection_members,
                    sketch,
                    resolution.entities,
                    resolution.histories,
                    resolution.linear_tolerance,
                    resolution.arrangement_budget,
                    resolution.ctx,
                )?;
            }
            if resolved_profiles.is_none() {
                if let (Some(state_id), Some(previous_state_id)) =
                    (history_state_id, previous_history_state_id)
                {
                    resolved_profiles = transition_profile_selection(
                        sketch,
                        resolution,
                        state_id,
                        previous_state_id,
                    )?;
                }
            }
            if resolved_profiles.is_none() && sketch.profiles.len() == 1 {
                let mut profiles = Vec::new();
                ctx.push_vec(&mut profiles, 0, "f3d extrude sole profile")?;
                resolved_profiles = Some(ResolvedProfileSelection::Loops(profiles));
            }
            Ok(resolved_profiles)
        },
    )?;
    let mut output_storage = ctx.reserve_scoped(0, "f3d extrude selected output storage")?;
    let mut kept_selection = false;
    let profile = match resolved_profiles {
        Some(ResolvedProfileSelection::Loops(profiles)) => {
            match output_storage.with_storage(|| -> Result<_, CodecError> {
                let sketch = sketch_id.try_clone_for_decode(ctx, "f3d profile sketch id")?;
                Ok(PlanarProfileRef::sketch_profiles(sketch, profiles, ctx)?)
            })? {
                Ok(profile) => {
                    kept_selection = true;
                    profile
                }
                Err(_) => PlanarProfileRef::Native(
                    (resolution.ctx)
                        .copy_retained_text(&group.id, "f3d extrude fallback group id")?,
                ),
            }
        }
        Some(ResolvedProfileSelection::Regions(regions)) => {
            match output_storage.with_storage(|| -> Result<_, CodecError> {
                let sketch = sketch_id.try_clone_for_decode(ctx, "f3d profile sketch id")?;
                Ok(PlanarProfileRef::sketch_regions(sketch, regions))
            })? {
                Ok(profile) => {
                    kept_selection = true;
                    profile
                }
                Err(_) => PlanarProfileRef::Native(
                    (resolution.ctx)
                        .copy_retained_text(&group.id, "f3d extrude fallback group id")?,
                ),
            }
        }
        None => {
            let id =
                (resolution.ctx).copy_retained_text(&group.id, "f3d extrude selection group id")?;
            let mut ids = Vec::new();
            (resolution.ctx).push_vec(&mut ids, id, "f3d extrude selection group")?;
            match PlanarProfileRef::sketch_selection(
                (sketch_id).try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?,
                ids,
                resolution.ctx,
            )? {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(
                    (resolution.ctx)
                        .copy_retained_text(&group.id, "f3d extrude fallback group id")?,
                ),
            }
        }
    };
    if kept_selection {
        resolved_storage.commit()?;
        output_storage.commit()?;
    }
    Ok(ProfileRef::Planar(profile))
}

fn unique_historical_state<'a>(
    histories: &'a [crate::history_records::AsmHistory],
    state_id: i64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<&'a crate::history_records::AsmDeltaState>, CodecError> {
    let mut selected = None;
    let mut histories = histories.iter();
    while let Some(history) = ctx.next_charged(&mut histories, "f3d selected history search")? {
        let mut states = history.states.iter();
        while let Some(state) =
            ctx.next_charged(&mut states, "f3d selected history state search")?
        {
            if state.state_id == state_id {
                if selected.is_some() {
                    return Ok(None);
                }
                selected = Some(state);
            }
        }
    }
    Ok(selected)
}

fn transition_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    macro_rules! geometric {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let entities = resolution.entities;
    let arrangement_budget = resolution.arrangement_budget;
    let state = geometric!(unique_historical_state(
        resolution.histories,
        state_id,
        resolution.ctx
    )?);
    if state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id)
        != Some(previous_state_id)
    {
        return Ok(None);
    }
    let topology = geometric!(state.topology());
    let inserted_faces = &geometric!(state.transition.as_ref())
        .topology
        .faces
        .inserted;
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = resolution.linear_tolerance;
    let mut inserted_storage = resolution
        .ctx
        .reserve_scoped(0, "f3d transition inserted selection storage")?;
    let mut inserted_selections = Vec::new();
    for face in resolution
        .ctx
        .admit_iter(inserted_faces, "f3d transition inserted face scan")?
    {
        let points = historical_face_points(*face, topology, resolution.ctx)?;
        let selection = inserted_storage.with_storage(|| match points {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                points.as_ref(),
                tolerance,
                arrangement_budget,
                resolution.ctx,
            ),
            None => Ok(None),
        })?;
        inserted_storage.with_storage(|| {
            resolution.ctx.push_vec(
                &mut inserted_selections,
                selection,
                "f3d transition inserted selection",
            )
        })?;
    }
    let inserted = transition_inserted_profile_selection(
        sketch,
        entities,
        tolerance,
        &inserted_selections,
        resolution.ctx,
    )?;
    drop((inserted_selections, inserted_storage));
    if inserted.is_some() {
        return Ok(inserted);
    }
    let mut cylindrical_storage = resolution
        .ctx
        .reserve_scoped(0, "f3d transition cylindrical selection storage")?;
    let mut cylindrical_selections = Vec::new();
    for face in resolution
        .ctx
        .admit_iter(inserted_faces, "f3d transition cylindrical face scan")?
    {
        let selection = cylindrical_storage.with_storage(|| {
            inserted_cylindrical_profile_selection(
                sketch,
                entities,
                topology,
                *face,
                tolerance,
                resolution.angular_tolerance,
                resolution.ctx,
            )
        })?;
        cylindrical_storage.with_storage(|| {
            resolution.ctx.push_vec(
                &mut cylindrical_selections,
                selection,
                "f3d transition cylindrical selection",
            )
        })?;
    }
    if let Some(selection) = unique_resolved_selection(
        cylindrical_selections.iter().map(Option::as_ref),
        resolution.ctx,
        |left, right| resolved_selections_equal(left, right, resolution.ctx),
    )? {
        return Ok(Some(copy_resolved_selection(selection, resolution.ctx)?));
    }
    drop((cylindrical_selections, cylindrical_storage));
    let previous = geometric!(unique_historical_state(
        resolution.histories,
        previous_state_id,
        resolution.ctx
    )?);
    let previous_topology = geometric!(previous.topology());
    let deleted = &geometric!(state.transition.as_ref()).topology.faces.deleted;
    let mut temporary_face_storage = resolution
        .ctx
        .reserve_scoped(0, "f3d transition deleted face storage")?;
    let faces = temporary_face_storage.with_storage(|| {
        unique_multi_face_deleted_carrier_family(deleted, previous_topology, resolution.ctx)
    })?;
    let faces = geometric!(faces);
    let mut selection_storage = resolution
        .ctx
        .reserve_scoped(0, "f3d transition deleted selection storage")?;
    let mut selections = Vec::new();
    for face in resolution
        .ctx
        .admit_iter(faces, "f3d transition deleted face scan")?
    {
        let points = historical_face_points(face, previous_topology, resolution.ctx)?;
        let selection = selection_storage.with_storage(|| match points {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                points.as_ref(),
                tolerance,
                arrangement_budget,
                resolution.ctx,
            ),
            None => Ok(None),
        })?;
        selection_storage.with_storage(|| {
            resolution.ctx.push_vec(
                &mut selections,
                selection,
                "f3d transition deleted selection",
            )
        })?;
    }
    ordered_unique_profile_selections(&selections, resolution.ctx)
}

fn inserted_cylindrical_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
    linear_tolerance: f64,
    angular_tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut carriers = topology.face_surfaces.iter();
    let Some(binding) = ctx.find_by(
        &mut carriers,
        |binding| Ok(binding.entity == face),
        "f3d cylindrical carrier search",
    )?
    else {
        return Ok(None);
    };
    let carrier = binding.carrier;
    if ctx.any_by(
        carriers,
        |binding| Ok(binding.entity == face),
        "f3d cylindrical duplicate carrier search",
    )? {
        return Ok(None);
    }
    let mut cylinders = topology.surface_cylinders.iter();
    let Some(cylinder) = ctx.find_by(
        &mut cylinders,
        |cylinder| Ok(cylinder.surface == carrier),
        "f3d cylindrical surface search",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        cylinders,
        |cylinder| Ok(cylinder.surface == carrier),
        "f3d cylindrical duplicate surface search",
    )? || !cylinder.radius.is_finite()
        || cylinder.radius <= 0.0
    {
        return Ok(None);
    }
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return Ok(None);
    };
    let alignment = cylinder.axis.x * sketch_normal.x
        + cylinder.axis.y * sketch_normal.y
        + cylinder.axis.z * sketch_normal.z;
    if alignment.abs() < angular_tolerance.cos() {
        return Ok(None);
    }
    let offset = (sketch_origin.x - cylinder.origin.x) * sketch_normal.x
        + (sketch_origin.y - cylinder.origin.y) * sketch_normal.y
        + (sketch_origin.z - cylinder.origin.z) * sketch_normal.z;
    let parameter = offset / alignment;
    let Some(center) = project_to_sketch(
        sketch,
        Point3::new(
            cylinder.origin.x + parameter * cylinder.axis.x,
            cylinder.origin.y + parameter * cylinder.axis.y,
            cylinder.origin.z + parameter * cylinder.axis.z,
        ),
    ) else {
        return Ok(None);
    };
    let Some(points) = historical_face_points(face, topology, ctx)? else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "f3d cylindrical projected point storage")?;
    let mut projected = Vec::new();
    let mut point_iter = points.as_ref().iter();
    while let Some(point) =
        ctx.next_charged(&mut point_iter, "f3d cylindrical projected point scan")?
    {
        let Some(point) = project_to_sketch(sketch, *point) else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut projected,
                point,
                "f3d cylindrical profile projected point",
            )
        })?;
    }
    drop(points);
    let candidate = |(index, profile): (usize, &Vec<cadmpeg_ir::sketches::SketchEntityUse>)| -> Result<Option<u32>, CodecError> {
        let [use_] = profile.as_slice() else { return Ok(None); };
        let Some(entity) = ctx.find_by(entities, |entity| Ok(
            ctx.equal(&entity.sketch, &sketch.id, "f3d cylindrical entity sketch comparison")?
                && ctx.equal(entity.id(), &use_.entity, "f3d cylindrical entity comparison")?
        ), "f3d cylindrical entity search")? else { return Ok(None); };
        let SketchGeometryDefinition::Circle { center: candidate_center, radius: candidate_radius } = *entity.geometry.definition() else { return Ok(None); };
        let matches = (candidate_center.u - center.u).hypot(candidate_center.v - center.v) <= linear_tolerance
            && (candidate_radius.get() - cylinder.radius).abs() <= linear_tolerance
            && ctx.all_by(&projected, |point| Ok(((point.u - candidate_center.u).hypot(point.v - candidate_center.v) - candidate_radius.get()).abs() <= linear_tolerance), "f3d cylindrical circle point search")?;
        Ok(matches.then(|| u32::try_from(index).ok()).flatten())
    };
    let mut profiles = sketch.profiles.iter().enumerate();
    let Some(profile) = ctx.find_map(&mut profiles, candidate, "f3d cylindrical profile search")?
    else {
        return Ok(None);
    };
    if ctx
        .find_map(
            profiles,
            candidate,
            "f3d cylindrical duplicate profile search",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let mut output = Vec::new();
    ctx.push_vec(&mut output, profile, "f3d cylindrical selected profile")?;
    Ok(Some(ResolvedProfileSelection::Loops(output)))
}

fn resolved_spatial_extrude_profile_selection(
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<Option<u32>, CodecError> {
    enum ExactSelection {
        Resolved(u32),
        Unavailable,
        Contradictory,
    }

    let ctx = resolution.ctx;
    let mut group_members_storage = ctx.reserve_scoped(0, "f3d profile group member storage")?;
    let mut group_members = Vec::new();
    for member in ctx.admit_iter(members, "f3d profile source member scan")? {
        if !ctx.equal(
            &native_stream_charged(ctx, &member.id, "f3d profile native stream")?,
            &native_stream_charged(ctx, &group.id, "f3d profile native stream")?,
            "f3d profile member stream comparison",
        )? || member.group_record_index != group.record_index
        {
            continue;
        }
        group_members_storage.with_storage(|| {
            (resolution.ctx).push_vec(
                &mut group_members,
                member,
                "f3d spatial profile group member",
            )
        })?;
    }
    resolution.ctx.stable_sort_by_key(
        &mut group_members[..],
        |value| {
            let member = value;
            {
                member.group_member_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design profile_select 4",
    )?;
    let exact_member_run = group_members.len() == group.members().len()
        && ctx.all_by(
            group_members.iter().zip(group.members()),
            |(member, record_index)| Ok(member.record_index() == record_index.value),
            "f3d profile member run comparison",
        )?;
    let exact_selection = (|| -> Result<ExactSelection, CodecError> {
        if !exact_member_run {
            return Ok(ExactSelection::Unavailable);
        }
        let mut selected = None;
        let mut member_source = group_members.into_iter();
        while let Some(member) =
            ctx.next_charged(&mut member_source, "f3d ordered group member scan")?
        {
            let Some(SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            }) = member.resolved_geometry.as_ref()
            else {
                return Ok(ExactSelection::Unavailable);
            };
            let mut temporary_entity_storage =
                ctx.reserve_scoped(0, "f3d spatial extrude curve query storage")?;
            let entity = temporary_entity_storage.with_storage(|| {
                crate::design::identity::neutral_spatial_sketch_curve_id(
                    resolution.ctx,
                    &sketch.id,
                    *primary_id,
                    *secondary_id,
                )
            })?;

            let Some((index, _)) = unique_feature_match_for_decode(ctx, sketch.profiles.iter().enumerate(),
                |(_, profile)| {
                    ctx.any_by(
                        profile.boundary(),
                        |use_| {
                            ctx.equal(
                                &use_.entity,
                                &entity,
                                "f3d profile candidate boundary identity",
                            )
                        },
                        "f3d profile candidate boundary search",
                    )
                },
                "f3d profile candidate source search",
            )?
            else {
                return Ok(ExactSelection::Unavailable);
            };

            let Ok(profile) = u32::try_from(index) else {
                return Ok(ExactSelection::Unavailable);
            };
            if selected
                .replace(profile)
                .is_some_and(|selected| selected != profile)
            {
                return Ok(ExactSelection::Contradictory);
            }
        }
        Ok(selected.map_or(ExactSelection::Unavailable, ExactSelection::Resolved))
    })()?;
    match exact_selection {
        ExactSelection::Resolved(selection) => Ok(Some(selection)),
        ExactSelection::Contradictory => Ok(None),
        ExactSelection::Unavailable => {
            let transition = if let Some((state_id, previous_state_id)) =
                history_state_id.zip(previous_history_state_id)
            {
                transition_spatial_profile_selection(
                    sketch,
                    entities,
                    resolution.histories,
                    state_id,
                    previous_state_id,
                    resolution.linear_tolerance,
                    resolution.ctx,
                )?
            } else {
                None
            };
            Ok(transition.or_else(|| (sketch.profiles.len() == 1).then_some(0)))
        }
    }
}

fn transition_spatial_profile_selection(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let Some(state) = unique_historical_state(histories, state_id, ctx)? else {
        return Ok(None);
    };
    if state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id)
        != Some(previous_state_id)
    {
        return Ok(None);
    }
    let Some(topology) = state.topology() else {
        return Ok(None);
    };
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let unique = |faces: &[i64],
                  topology: &crate::history_records::AsmHistoricalTopology|
     -> Result<Option<u32>, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "f3d spatial transition index storage")?;
        let mut indices = Vec::new();
        for face in ctx.admit_iter(faces, "f3d spatial transition face scan")? {
            if let Some(points) = historical_face_points(*face, topology, ctx)? {
                if let Some(index) = spatial_polyline_profile_containing_points(
                    sketch,
                    entities,
                    points.as_ref(),
                    tolerance,
                    ctx,
                )? {
                    storage.with_storage(|| {
                        ctx.push_vec(&mut indices, index, "f3d spatial transition profile index")
                    })?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut indices,
            |value| value,
            Ord::cmp,
            "sort f3d spatial transition profile indices",
        )?;
        ctx.dedup_vec(&mut indices, "f3d profile select indices dedup")?;
        Ok((indices.len() == 1).then(|| indices[0]))
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if let Some(index) = unique(&transition.topology.faces.inserted, topology)? {
        return Ok(Some(index));
    }
    let Some(previous) = unique_historical_state(histories, previous_state_id, ctx)? else {
        return Ok(None);
    };
    let Some(previous_topology) = previous.topology() else {
        return Ok(None);
    };
    unique(&transition.topology.faces.deleted, previous_topology)
}

fn spatial_polyline_profile_containing_points(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    points: &[Point3],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let mut selected = None;
    let mut profiles = sketch.profiles.iter().enumerate();
    while let Some((index, profile)) =
        ctx.next_charged(&mut profiles, "f3d spatial polygon profile scan")?
    {
        let mut offsets = points.iter().map(|point| {
            point
                .vector_from(profile.origin().get())
                .dot(profile.normal().into())
        });
        let Some(first) = ctx.next_charged(&mut offsets, "f3d spatial polygon first offset")?
        else {
            continue;
        };
        if !ctx.all_by(
            offsets,
            |offset| Ok((offset - first).abs() <= tolerance),
            "f3d spatial polygon offset scan",
        )? {
            continue;
        }
        let normal = Vector3::from(profile.normal());
        let u_axis = Vector3::from(profile.u_axis());
        let v_axis = normal.cross(u_axis);
        let project = |point: Point3| {
            let offset = point.vector_from(profile.origin().get());
            Point2::new(offset.dot(u_axis), offset.dot(v_axis))
        };
        let mut polygon_storage = ctx.reserve_scoped(0, "f3d spatial polygon storage")?;
        let mut polygon = Vec::new();
        let mut uses = profile.boundary().iter();
        while let Some(use_) = ctx.next_charged(&mut uses, "f3d spatial polygon use scan")? {
            let Some(entity) = ctx.find_by(
                entities,
                |entity| {
                    Ok(ctx.equal(
                        &entity.sketch,
                        &sketch.id,
                        "f3d spatial polygon sketch comparison",
                    )? && ctx.equal(
                        entity.id(),
                        &use_.entity,
                        "f3d spatial polygon entity comparison",
                    )?)
                },
                "f3d spatial polygon entity scan",
            )?
            else {
                return Ok(None);
            };
            let cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line { start, end } =
                entity.geometry.definition()
            else {
                return Ok(None);
            };
            let point = project(if use_.reversed {
                end.get()
            } else {
                start.get()
            });
            polygon_storage.with_storage(|| {
                ctx.push_vec(&mut polygon, point, "f3d spatial profile polygon point")
            })?;
        }
        if polygon.len() >= 3
            && ctx.all_by(
                points,
                |point| {
                    let point = project(*point);
                    Ok(point_in_polygon(point, &polygon, ctx)?
                        || ctx.any_by(
                            polygon.iter().enumerate(),
                            |(index, start)| {
                                let end = polygon[(index + 1) % polygon.len()];
                                Ok(point_segment_distance(point, (*start, end)) <= tolerance)
                            },
                            "f3d spatial polygon boundary scan",
                        )?)
                },
                "f3d spatial polygon point scan",
            )?
        {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            if selected.replace(index).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(selected)
}

fn unique_multi_face_deleted_carrier_family(
    deleted_faces: &[i64],
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut seen_storage = ctx.reserve_scoped(0, "f3d deleted face uniqueness storage")?;
    let mut seen = HashSet::new();
    let mut family_storage = ctx.reserve_scoped(0, "f3d deleted carrier family storage")?;
    let mut families = BTreeMap::<i64, Vec<i64>>::new();
    let mut deleted_faces = deleted_faces.iter().copied();
    while let Some(face) = ctx.next_charged(&mut deleted_faces, "f3d deleted face scan")? {
        if seen.contains(&face) {
            return Ok(None);
        }
        seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, face, "f3d deleted face uniqueness index")
        })?;
        let mut bindings = topology.face_surfaces.iter();
        let Some(carrier) = ctx
            .find_by(
                &mut bindings,
                |binding| Ok(binding.entity == face),
                "f3d deleted face carrier scan",
            )?
            .map(|binding| binding.carrier)
        else {
            return Ok(None);
        };
        if ctx
            .find_by(
                &mut bindings,
                |binding| Ok(binding.entity == face),
                "f3d deleted face carrier duplicate scan",
            )?
            .is_some()
        {
            return Ok(None);
        }
        family_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut families,
                carrier,
                face,
                "f3d deleted carrier family index",
                "f3d deleted carrier family face",
            )
        })?;
    }
    let mut candidates = families.iter();
    let Some((_, faces)) = ctx.find_by(
        &mut candidates,
        |(_, faces)| Ok(faces.len() > 1),
        "f3d deleted carrier family scan",
    )?
    else {
        return Ok(None);
    };
    if ctx
        .find_by(
            &mut candidates,
            |(_, faces)| Ok(faces.len() > 1),
            "f3d deleted carrier family duplicate scan",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let mut faces = ctx.copy_slice(faces, "f3d deleted selected carrier faces")?;
    ctx.sort_unstable_by(
        &mut faces,
        |value| value,
        Ord::cmp,
        "sort f3d deleted carrier family faces",
    )?;
    Ok(Some(faces))
}

fn unique_resolved_selection<T>(
    selections: impl IntoIterator<Item = Option<T>>,
    ctx: &DecodeContext<'_>,
    mut equal: impl FnMut(&T, &T) -> Result<bool, CodecError>,
) -> Result<Option<T>, CodecError> {
    let mut selections = selections.into_iter();
    let Some(first) = ctx.find_map(&mut selections, Ok, "f3d unique selection first search")?
    else {
        return Ok(None);
    };
    if ctx.all_by(
        selections,
        |selection| match selection {
            Some(selection) => equal(&selection, &first),
            None => Ok(true),
        },
        "f3d unique selection agreement search",
    )? {
        Ok(Some(first))
    } else {
        Ok(None)
    }
}

fn transition_inserted_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    selections: &[Option<ResolvedProfileSelection>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    fn nonempty_loops(selection: Option<&ResolvedProfileSelection>) -> Option<&[u32]> {
        match selection {
            Some(ResolvedProfileSelection::Loops(loops)) if !loops.is_empty() => {
                Some(loops.as_slice())
            }
            _ => None,
        }
    }
    fn hole_region(selection: Option<&ResolvedProfileSelection>) -> Option<(u32, &[u32])> {
        match selection {
            Some(ResolvedProfileSelection::Regions(regions)) => match regions.as_slice() {
                [SketchProfileRegion::Loops { loops }] if !loops.holes().is_empty() => {
                    Some((loops.outer(), loops.holes()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    let first = ctx.find_map(
        selections,
        |selection| Ok(selection.as_ref()),
        "f3d inserted first selection search",
    )?;
    if let Some(first) = first {
        if ctx.all_by(
            selections,
            |selection| match selection {
                Some(selection) => resolved_selections_equal(selection, first, ctx),
                None => Ok(true),
            },
            "f3d inserted selection agreement search",
        )? {
            return Ok(Some(copy_resolved_selection(first, ctx)?));
        }
    }
    let mut loop_selections = selections.iter();
    let first_loops = ctx.find_map(
        &mut loop_selections,
        |selection| Ok(nonempty_loops(selection.as_ref())),
        "f3d inserted first loop search",
    )?;
    if let Some(first) = first_loops {
        if ctx.all_by(
            loop_selections,
            |selection| match nonempty_loops(selection.as_ref()) {
                Some(candidate) => ctx.equal(candidate, first, "f3d inserted loop comparison"),
                None => Ok(true),
            },
            "f3d inserted loop agreement search",
        )? {
            return Ok(Some(ResolvedProfileSelection::Loops(
                ctx.copy_slice(first, "f3d inserted agreed loop copy")?,
            )));
        }
    }
    if ctx.all_by(
        selections,
        |selection| {
            Ok(match selection {
                None => true,
                Some(ResolvedProfileSelection::Loops(loops)) => !loops.is_empty(),
                Some(ResolvedProfileSelection::Regions(_)) => false,
            })
        },
        "f3d inserted loop kind search",
    )? {
        let mut storage = ctx.reserve_scoped(0, "f3d inserted combined loop storage")?;
        let mut loops = Vec::new();
        for selection in ctx.admit_iter(selections, "f3d inserted combined selection scan")? {
            let Some(ResolvedProfileSelection::Loops(selected)) = selection else {
                continue;
            };
            for profile in ctx.admit_iter(selected, "f3d inserted combined loop scan")? {
                if !ctx.contains(
                    &loops,
                    profile,
                    "f3d inserted combined loop duplicate search",
                )? {
                    storage.with_storage(|| {
                        ctx.push_vec(&mut loops, *profile, "f3d inserted transition profile loop")
                    })?;
                }
            }
        }
        if !loops.is_empty()
            && profile_loops_are_independent(sketch, entities, &loops, tolerance, ctx)?
        {
            storage.commit()?;
            return Ok(Some(ResolvedProfileSelection::Loops(loops)));
        }
    }
    let mut hole_selections = selections.iter();
    let Some((outer, holes)) = ctx.find_map(
        &mut hole_selections,
        |selection| Ok(hole_region(selection.as_ref())),
        "f3d inserted hole region search",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        hole_selections,
        |selection| match hole_region(selection.as_ref()) {
            Some((candidate_outer, candidate_holes)) => Ok(candidate_outer != outer
                || !ctx.equal(
                    candidate_holes,
                    holes,
                    "f3d inserted region hole comparison",
                )?),
            None => Ok(false),
        },
        "f3d inserted hole region conflict search",
    )? {
        return Ok(None);
    }
    let mut has_boundary_support = false;
    let mut selected = selections.iter();
    while let Some(selection) =
        ctx.next_charged(&mut selected, "f3d inserted boundary support scan")?
    {
        let Some(selection) = selection else {
            continue;
        };
        match selection {
            ResolvedProfileSelection::Regions(regions) => {
                let [SketchProfileRegion::Loops { loops }] = regions.as_slice() else {
                    return Ok(None);
                };
                if loops.outer() != outer
                    || !ctx.equal(loops.holes(), holes, "f3d inserted support hole comparison")?
                {
                    return Ok(None);
                }
            }
            ResolvedProfileSelection::Loops(loops) => {
                if loops.is_empty()
                    || !ctx.all_by(
                        loops,
                        |profile| {
                            Ok(*profile == outer
                                || ctx.contains(
                                    holes,
                                    profile,
                                    "f3d inserted support hole search",
                                )?)
                        },
                        "f3d inserted support loop search",
                    )?
                {
                    return Ok(None);
                }
                has_boundary_support = true;
            }
        }
    }
    if !has_boundary_support {
        return Ok(None);
    }
    let owned_holes = ctx.copy_slice(holes, "f3d inserted transition region hole")?;
    let Some(region) = SketchProfileRegion::loops(outer, owned_holes, ctx)?.ok() else {
        return Ok(None);
    };
    let mut output = Vec::new();
    ctx.push_vec(&mut output, region, "f3d inserted transition region")?;
    Ok(Some(ResolvedProfileSelection::Regions(output)))
}

pub(super) fn first_historical_index<'ctx, V>(
    entries: impl IntoIterator<Item = (i64, V)>,
    ctx: &'ctx DecodeContext<'_>,
    operation: &'static str,
) -> Result<(HashMap<i64, V>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut index = HashMap::new();
    let mut entries = entries.into_iter();
    while let Some((key, value)) = ctx.next_charged(&mut entries, operation)? {
        if !ctx.contains_key_hash_map(&index, &key, operation)? {
            // discarded-value: the first record owns this numeric key.
            let _ =
                storage.with_storage(|| ctx.insert_hash_map(&mut index, key, value, operation))?;
        }
    }
    Ok((index, storage))
}

pub(super) fn historical_face_points<'ctx>(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Option<crate::design::geometry::HistoricalPoints<'ctx>>, CodecError> {
    let Some(loops) = ctx.find_by(
        &topology.face_loops,
        |relation| Ok(relation.owner_ref == face),
        "f3d historical face loop search",
    )?
    else {
        return Ok(None);
    };
    if loops.member_refs.is_empty() {
        return Ok(None);
    }
    let scoped_loop_coedges = first_historical_index(
        topology
            .loop_coedges
            .iter()
            .map(|relation| (relation.owner_ref, &relation.member_refs)),
        ctx,
        "f3d historical loop coedge index",
    )?;
    let loop_coedges = &scoped_loop_coedges.0;
    let mut scoped_coedges = None;
    let mut scoped_edges = None;
    let mut scoped_vertices = None;
    let mut scoped_points = None;
    let mut storage = ctx.reserve_scoped(0, "f3d historical face position storage")?;
    let mut positions = Vec::new();
    let mut loops = loops.member_refs.iter();
    while let Some(loop_ref) = ctx.next_charged(&mut loops, "f3d historical face loop scan")? {
        let Some(members) =
            ctx.get_hash_map(loop_coedges, loop_ref, "f3d historical loop coedge lookup")?
        else {
            return Ok(None);
        };
        let mut members = members.iter();
        while let Some(coedge_ref) =
            ctx.next_charged(&mut members, "f3d historical face coedge scan")?
        {
            if scoped_coedges.is_none() {
                scoped_coedges = Some(first_historical_index(
                    topology
                        .coedge_topology
                        .iter()
                        .map(|coedge| (coedge.coedge, coedge)),
                    ctx,
                    "f3d historical coedge index",
                )?);
            }
            let (coedges, _storage) = scoped_coedges.as_ref().ok_or_else(||
                CodecError::malformed("Historical coedges index is absent"))?;
            let Some(coedge) =
                ctx.get_hash_map(coedges, coedge_ref, "f3d historical coedge lookup")?
            else {
                return Ok(None);
            };
            if scoped_edges.is_none() {
                scoped_edges = Some(first_historical_index(
                    topology.edge_vertices.iter().map(|edge| (edge.edge, edge)),
                    ctx,
                    "f3d historical face edge index",
                )?);
            }
            let (edges, _storage) = scoped_edges.as_ref().ok_or_else(||
                CodecError::malformed("Historical edges index is absent"))?;
            let Some(edge) =
                ctx.get_hash_map(edges, &coedge.edge, "f3d historical face edge lookup")?
            else {
                return Ok(None);
            };
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                if scoped_vertices.is_none() {
                    scoped_vertices = Some(first_historical_index(
                        topology
                            .vertex_points
                            .iter()
                            .map(|binding| (binding.entity, binding.carrier)),
                        ctx,
                        "f3d historical face vertex index",
                    )?);
                }
                let (vertices, _storage) = scoped_vertices.as_ref().ok_or_else(||
                    CodecError::malformed("Historical vertices index is absent"))?;
                let Some(point_ref) =
                    ctx.get_hash_map(vertices, &vertex_ref, "f3d historical face vertex lookup")?
                else {
                    return Ok(None);
                };
                if scoped_points.is_none() {
                    scoped_points = Some(first_historical_index(
                        topology
                            .point_positions
                            .iter()
                            .map(|point| (point.point, point.position)),
                        ctx,
                        "f3d historical face point index",
                    )?);
                }
                let (points, _storage) = scoped_points.as_ref().ok_or_else(||
                    CodecError::malformed("Historical points index is absent"))?;
                let Some(position) =
                    ctx.get_hash_map(points, point_ref, "f3d historical face point lookup")?
                else {
                    return Ok(None);
                };
                if !ctx.any_by(
                    &positions,
                    |previous| Ok(previous == position),
                    "f3d historical face point duplicate search",
                )? {
                    storage.with_storage(|| {
                        ctx.push_vec(&mut positions, *position, "f3d historical face point")
                    })?;
                }
            }
        }
    }
    if positions.len() < 3 {
        return Ok(None);
    }
    Ok(Some(crate::design::geometry::HistoricalPoints::new(
        positions, storage,
    )))
}

enum HistoricalMemberPointRun<'ctx> {
    Topology(crate::design::geometry::HistoricalPoints<'ctx>),
    Resolved([Point3; 1]),
}

impl AsRef<[Point3]> for HistoricalMemberPointRun<'_> {
    fn as_ref(&self) -> &[Point3] {
        match self {
            Self::Topology(points) => points.as_ref(),
            Self::Resolved(points) => points,
        }
    }
}

fn historical_selection_regions(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    linear_tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let scoped_states = ctx.unique_index(
        ctx.admit_iter(histories, "f3d historical selection history scan")?
            .flat_map(|history| history.states.iter().map(|state| (state.state_id, state))),
        "f3d historical selection state index",
    )?;
    let states = &scoped_states.0;
    let Some(first_member) = members.first() else {
        return Ok(None);
    };
    let mut state_storage = ctx.reserve_scoped(0, "hold F3D historical selection states")?;
    let mut state_ids = Vec::new();
    if let Some(binding) = first_member.historical.as_ref() {
        let mut source_states = binding.state_ids.iter().copied();
        while let Some(state_id) = ctx.next_charged(
            &mut source_states,
            "visit F3D historical selection first member state",
        )? {
            if state_ids.is_empty()
                || !ctx.any_by(
                    &state_ids,
                    |existing| Ok(*existing == state_id),
                    "visit F3D historical selection duplicate state",
                )?
            {
                ctx.push_scoped_vec(
                    &mut state_storage,
                    &mut state_ids,
                    state_id,
                    "f3d historical selection state id",
                )?;
            }
        }
    }
    let mut remaining_members = members[1..].iter();
    while !state_ids.is_empty() {
        let Some(member) = ctx.next_charged(
            &mut remaining_members,
            "visit F3D historical selection intersecting member",
        )?
        else {
            break;
        };
        let Some(binding) = member.historical.as_ref() else {
            state_ids.clear();
            break;
        };
        let mut kept = 0;
        let mut state_slots = 0..state_ids.len();
        while let Some(index) = ctx.next_charged(
            &mut state_slots,
            "visit F3D historical selection intersection state",
        )? {
            let state_id = state_ids[index];
            if !binding.state_ids.is_empty()
                && ctx.any_by(
                    &binding.state_ids,
                    |candidate| Ok(*candidate == state_id),
                    "visit F3D historical selection member binding state",
                )?
            {
                state_ids[kept] = state_id;
                kept += 1;
            }
        }
        state_ids.truncate(kept);
    }
    ctx.sort_unstable_by(
        &mut state_ids,
        |value| value,
        Ord::cmp,
        "sort f3d historical selection states",
    )?;
    let mut previous_member_points: Option<(
        Vec<HistoricalMemberPointRun<'_>>,
        ScopedReservation<'_>,
    )> = None;
    let mut state_selection = None;
    let mut conflicting_state_selections = false;
    let mut selected_states = state_ids.into_iter();
    while let Some(state_id) = ctx.next_charged(
        &mut selected_states, "visit F3D historical selection selected state",
    )? {
        let Some(topology) = ctx
            .get_hash_map(states, &state_id, "f3d historical state lookup")?
            .and_then(|state| state.as_ref())
            .and_then(|state| state.topology())
        else {
            continue;
        };
        let mut member_storage = ctx.reserve_scoped(0, "f3d historical member point storage")?;
        let mut member_points = Vec::new();
        let mut complete = true;
        let mut member_iter = members.iter();
        while let Some(member) =
            ctx.next_charged(&mut member_iter, "f3d historical member point scan")?
        {
            let points = match historical_member_points_in_state(member, topology, ctx)? {
                Some(points) => Some(HistoricalMemberPointRun::Topology(points)),
                None => resolved_selection_member_point(ctx, member, sketch, entities)?
                    .map(|point| HistoricalMemberPointRun::Resolved([point])),
            };
            let Some(points) = points else {
                complete = false;
                break;
            };
            member_storage.with_storage(|| {
                ctx.push_vec(
                    &mut member_points,
                    points,
                    "f3d historical selection member points",
                )
            })?;
        }
        if !complete {
            continue;
        }
        if let Some((previous, _storage)) = previous_member_points.as_ref() {
            if same_member_point_bits(previous, &member_points, ctx)? {
                continue;
            }
        }
        let mut selection_storage =
            ctx.reserve_scoped(0, "f3d historical state selection storage")?;
        let selection = selection_storage.with_storage(|| {
            selection_for_member_points(
                members,
                sketch,
                entities,
                &member_points,
                tolerance,
                arrangement_budget,
                ctx,
            )
        })?;
        previous_member_points = Some((member_points, member_storage));
        if let Some(selection) = selection {
            if let Some((previous, _storage)) = state_selection.as_ref() {
                conflicting_state_selections |=
                    !resolved_selections_equal(previous, &selection, ctx)?;
            } else {
                state_selection = Some((selection, selection_storage));
            }
        }
    }
    drop(previous_member_points);
    drop(selected_states);
    drop(state_storage);
    drop(scoped_states);
    if conflicting_state_selections {
        return Ok(None);
    }
    if let Some((selection, storage)) = state_selection {
        storage.commit()?;
        return Ok(Some(selection));
    }
    let mut fallback_storage = ctx.reserve_scoped(0, "f3d resolved fallback point storage")?;
    let mut member_points = Vec::new();
    let mut complete = true;
    let mut member_iter = members.iter();
    while let Some(member) =
        ctx.next_charged(&mut member_iter, "f3d resolved fallback member scan")?
    {
        let Some(point) = resolved_selection_member_point(ctx, member, sketch, entities)? else {
            complete = false;
            break;
        };
        let points = [point];
        fallback_storage.with_storage(|| {
            ctx.push_vec(
                &mut member_points,
                points,
                "f3d resolved fallback member points",
            )
        })?;
    }
    if complete {
        if let Some(selection) = selection_for_member_points(
            members,
            sketch,
            entities,
            &member_points,
            tolerance,
            arrangement_budget,
            ctx,
        )? {
            return Ok(Some(selection));
        }
    }
    drop((member_points, fallback_storage));
    let mut selection_storage =
        ctx.reserve_scoped(0, "f3d historical fallback selection storage")?;
    let mut selections = Vec::new();
    for member in ctx.admit_iter(members, "f3d historical fallback member scan")? {
        let selection = selection_storage.with_storage(
            || -> Result<Option<ResolvedProfileSelection>, CodecError> {
                if let Some(point) = resolved_selection_member_point(ctx, member, sketch, entities)?
                {
                    selection_containing_points(
                        sketch,
                        entities,
                        std::slice::from_ref(&point),
                        tolerance,
                        arrangement_budget,
                        ctx,
                    )
                } else {
                    Ok(resolved_selection_member_profiles(member, sketch, ctx)?
                        .map(ResolvedProfileSelection::Loops))
                }
            },
        )?;
        selection_storage.with_storage(|| {
            ctx.push_vec(
                &mut selections,
                selection,
                "f3d historical fallback selection",
            )
        })?;
    }
    if ordered_selection_has_value(&selections, ctx)? {
        return ordered_unique_profile_selections(&selections, ctx);
    }
    region_with_boundary_selection_members(members, sketch, &selections, ctx)
}

fn same_member_point_bits<L: AsRef<[Point3]>, R: AsRef<[Point3]>>(
    left: &[L],
    right: &[R],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    ctx.all_by(
        left.iter().zip(right),
        |(left, right)| {
            let (left, right) = (left.as_ref(), right.as_ref());
            if left.len() != right.len() {
                return Ok(false);
            }
            ctx.all_by(
                left.iter().zip(right),
                |(left, right)| {
                    Ok(left.x.to_bits() == right.x.to_bits()
                        && left.y.to_bits() == right.y.to_bits()
                        && left.z.to_bits() == right.z.to_bits())
                },
                "f3d historical point bit comparison",
            )
        },
        "f3d historical member bit comparison",
    )
}

fn selection_for_member_points<P: AsRef<[Point3]>>(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    member_points: &[P],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut point_storage = ctx.reserve_scoped(0, "f3d historical combined point storage")?;
    let mut all_points = Vec::new();
    for points in ctx.admit_iter(member_points, "f3d historical combined member scan")? {
        for point in ctx.admit_iter(points.as_ref(), "f3d historical combined point scan")? {
            point_storage.with_storage(|| {
                ctx.push_vec(
                    &mut all_points,
                    *point,
                    "f3d historical combined member point",
                )
            })?;
        }
    }
    if let Some(selection) = selection_containing_points(
        sketch,
        entities,
        &all_points,
        tolerance,
        arrangement_budget,
        ctx,
    )? {
        return Ok(Some(selection));
    }
    drop((all_points, point_storage));
    let mut selection_storage = ctx.reserve_scoped(0, "f3d historical member selection storage")?;
    let mut selections = Vec::new();
    for points in ctx.admit_iter(member_points, "f3d historical selection point lane scan")? {
        let selection = selection_storage.with_storage(|| {
            selection_containing_points(
                sketch,
                entities,
                points.as_ref(),
                tolerance,
                arrangement_budget,
                ctx,
            )
        })?;
        selection_storage.with_storage(|| {
            ctx.push_vec(
                &mut selections,
                selection,
                "f3d historical member selection",
            )
        })?;
    }
    if ordered_selection_has_value(&selections, ctx)? {
        return ordered_unique_profile_selections(&selections, ctx);
    }
    region_with_boundary_selection_members(members, sketch, &selections, ctx)
}

fn region_with_boundary_selection_members(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    selections: &[Option<ResolvedProfileSelection>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let mut selections_iter = selections.iter();
    let Some([region]) = ctx.find_map(
        &mut selections_iter,
        |selection| {
            Ok(match selection {
                Some(ResolvedProfileSelection::Regions(regions)) => Some(regions.as_slice()),
                _ => None,
            })
        },
        "f3d historical boundary region search",
    )?
    else {
        return Ok(None);
    };
    let SketchProfileRegion::Loops { loops } = region else {
        return Ok(None);
    };
    if ctx.any_by(
        selections_iter,
        |selection| match selection {
            Some(ResolvedProfileSelection::Regions(candidate)) => Ok(!ctx.equal(
                candidate.as_slice(),
                std::slice::from_ref(region),
                "f3d historical boundary region comparison",
            )?),
            _ => Ok(false),
        },
        "f3d historical boundary region conflict search",
    )? {
        return Ok(None);
    }
    let mut pairs = members.iter().zip(selections);
    while let Some((member, selection)) =
        ctx.next_charged(&mut pairs, "f3d historical boundary member scan")?
    {
        let matches = match selection {
            Some(ResolvedProfileSelection::Regions(candidate)) => ctx.equal(
                candidate.as_slice(),
                std::slice::from_ref(region),
                "f3d historical boundary member region comparison",
            )?,
            Some(ResolvedProfileSelection::Loops(selected)) => {
                !selected.is_empty()
                    && ctx.all_by(
                        selected,
                        |profile| {
                            Ok(*profile == loops.outer()
                                || ctx.contains(
                                    loops.holes(),
                                    profile,
                                    "f3d historical boundary hole search",
                                )?)
                        },
                        "f3d historical boundary selected loop search",
                    )?
            }
            None => {
                let mut temporary_storage =
                    ctx.reserve_scoped(0, "f3d historical boundary member profile storage")?;
                let profiles = temporary_storage
                    .with_storage(|| resolved_selection_member_profiles(member, sketch, ctx))?;
                match profiles {
                    Some(profiles) => {
                        !profiles.is_empty()
                            && ctx.all_by(
                                &profiles,
                                |profile| {
                                    Ok(*profile == loops.outer()
                                        || ctx.contains(
                                            loops.holes(),
                                            profile,
                                            "f3d historical boundary resolved hole search",
                                        )?)
                                },
                                "f3d historical boundary resolved loop search",
                            )?
                    }
                    None => false,
                }
            }
        };
        if !matches {
            return Ok(None);
        }
    }
    let owned_holes = ctx.copy_slice(loops.holes(), "f3d historical boundary region hole")?;

    let Some(region) = SketchProfileRegion::loops(loops.outer(), owned_holes, ctx)?.ok() else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    ctx.push_vec(&mut selected, region, "f3d historical boundary region")?;
    Ok(Some(ResolvedProfileSelection::Regions(selected)))
}

fn resolved_selection_member_profiles(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(geometry) = member.resolved_geometry.as_ref() else {
        return Ok(None);
    };
    let SketchRelationOperand::Curve {
        primary_id,
        secondary_id,
        ..
    } = geometry
    else {
        return Ok(None);
    };
    let mut temporary_entity_storage =
        ctx.reserve_scoped(0, "f3d resolved profile identity storage")?;
    let entity = temporary_entity_storage.with_storage(|| {
        crate::design::identity::neutral_sketch_curve_id(
            ctx,
            &sketch.id,
            *primary_id,
            *secondary_id,
        )
    })?;
    let mut profile_storage = ctx.reserve_scoped(0, "f3d resolved profile index storage")?;
    let mut profiles = Vec::new();
    let mut profile_iter = sketch.profiles.iter().enumerate();
    while let Some((index, profile)) =
        ctx.next_charged(&mut profile_iter, "f3d resolved profile source scan")?
    {
        if ctx.any_by(
            profile,
            |use_| {
                ctx.equal(
                    &use_.entity,
                    &entity,
                    "f3d resolved profile entity comparison",
                )
            },
            "f3d resolved profile member search",
        )? {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            profile_storage.with_storage(|| {
                ctx.push_vec(&mut profiles, index, "f3d resolved member profile")
            })?;
        }
    }
    profile_storage.commit()?;
    Ok(Some(profiles))
}

fn resolved_selection_member_point(
    ctx: &DecodeContext<'_>,
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<Point3>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchRelationOperand::Point {
        record_index,
        persistent_id,
    } = or_none!(member.resolved_geometry.as_ref())
    else {
        return Ok(None);
    };
    let mut temporary_entity_storage =
        ctx.reserve_scoped(0, "f3d resolved point identity storage")?;
    let entity_id = temporary_entity_storage.with_storage(|| {
        persistent_id.map_or_else(
            || crate::design::identity::neutral_sketch_record_id(ctx, &sketch.id, *record_index),
            |persistent_id| {
                crate::design::identity::neutral_sketch_point_id(ctx, &sketch.id, persistent_id)
            },
        )
    })?;
    let SketchGeometryDefinition::Point { position } = or_none!(ctx.find_by(
        entities,
        |entity| Ok(ctx.equal(
            entity.id(),
            &entity_id,
            "f3d resolved point entity comparison"
        )? && ctx.equal(
            &entity.sketch,
            &sketch.id,
            "f3d resolved point sketch comparison"
        )?),
        "f3d resolved point entity search",
    )?)
    .geometry
    .definition() else {
        return Ok(None);
    };
    let (origin, normal, u_axis) = or_none!(sketch.resolved_placement());
    let v_axis = normal.cross(u_axis.get());
    Ok(Some(
        origin
            .translated(u_axis.get(), position.u)
            .translated(v_axis, position.v),
    ))
}

fn resolved_selections_equal(
    left: &ResolvedProfileSelection,
    right: &ResolvedProfileSelection,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    match (left, right) {
        (ResolvedProfileSelection::Loops(left), ResolvedProfileSelection::Loops(right)) => {
            ctx.equal(left, right, "f3d selected loop comparison")
        }
        (ResolvedProfileSelection::Regions(left), ResolvedProfileSelection::Regions(right)) => {
            ctx.equal(left, right, "f3d selected region comparison")
        }
        _ => Ok(false),
    }
}

fn copy_profile_region(
    region: &cadmpeg_ir::features::SketchProfileRegion,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::SketchProfileRegion, CodecError> {
    use cadmpeg_ir::features::{NonEmptyMembers, SketchProfileBoundaryUse, SketchProfileRegion};
    let copy_boundary = |source: &[SketchProfileBoundaryUse],
                         operation: &'static str|
     -> Result<NonEmptyMembers<SketchProfileBoundaryUse>, CodecError> {
        let mut output = Vec::new();
        for use_ in ctx.admit_iter(source, "f3d selected boundary scan")? {
            let use_ = SketchProfileBoundaryUse {
                entity: use_
                    .entity
                    .try_clone_for_decode(ctx, "f3d selected boundary entity")?,
                parameter_range: use_.parameter_range,
                reversed: use_.reversed,
            };
            ctx.push_vec(&mut output, use_, operation)?;
        }
        output
            .try_into()
            .map_err(|_| CodecError::malformed("selected boundary must be nonempty"))
    };
    match region {
        SketchProfileRegion::Loops { loops } => {
            let holes = ctx.copy_slice(loops.holes(), "f3d selected region hole")?;
            SketchProfileRegion::loops(loops.outer(), holes, ctx)?.map_err(CodecError::malformed)
        }
        SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } => {
            let outer_boundary = copy_boundary(outer_boundary, "f3d selected outer boundary use")?;
            let mut holes = Vec::new();
            for source in
                ctx.admit_iter(hole_boundaries, "f3d selected region hole boundary scan")?
            {
                let hole = copy_boundary(source, "f3d selected hole boundary use")?;
                ctx.push_vec(&mut holes, hole, "f3d selected region hole boundary")?;
            }
            Ok(SketchProfileRegion::Trimmed {
                outer_boundary,
                hole_boundaries: holes,
            })
        }
    }
}

fn copy_resolved_selection(
    selection: &ResolvedProfileSelection,
    ctx: &DecodeContext<'_>,
) -> Result<ResolvedProfileSelection, CodecError> {
    match selection {
        ResolvedProfileSelection::Loops(loops) => Ok(ResolvedProfileSelection::Loops(
            ctx.copy_slice(loops, "f3d selected profile copy")?,
        )),
        ResolvedProfileSelection::Regions(regions) => {
            let mut output = Vec::new();
            for region in ctx.admit_iter(regions, "f3d selected region copy scan")? {
                let copy = copy_profile_region(region, ctx)?;
                ctx.push_vec(&mut output, copy, "f3d selected region copy")?;
            }
            Ok(ResolvedProfileSelection::Regions(output))
        }
    }
}

fn ordered_selection_has_value(
    selections: &[Option<ResolvedProfileSelection>],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut has_loops = false;
    let mut has_regions = false;
    let mut selections = selections.iter();
    while let Some(selection) =
        ctx.next_charged(&mut selections, "f3d ordered selection kind scan")?
    {
        match selection {
            Some(ResolvedProfileSelection::Loops(selected)) if !has_regions => {
                has_loops |= !selected.is_empty();
            }
            Some(ResolvedProfileSelection::Regions(selected)) if !has_loops => {
                has_regions |= !selected.is_empty();
            }
            _ => return Ok(false),
        }
    }
    Ok(has_loops || has_regions)
}

fn ordered_unique_profile_selections(
    matches: &[Option<ResolvedProfileSelection>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut loop_storage = ctx.reserve_scoped(0, "f3d ordered loop selection storage")?;
    let mut loops = Vec::new();
    let mut region_storage = ctx.reserve_scoped(0, "f3d ordered region selection storage")?;
    let mut regions = Vec::new();
    let mut matches = matches.iter();
    while let Some(selection) = ctx.next_charged(&mut matches, "f3d ordered selection scan")? {
        let Some(selection) = selection else {
            return Ok(None);
        };
        match selection {
            ResolvedProfileSelection::Loops(selected) if regions.is_empty() => {
                for loop_index in ctx.admit_iter(selected, "f3d ordered selected loop scan")? {
                    if !ctx.contains(
                        &loops,
                        loop_index,
                        "f3d ordered selected loop duplicate search",
                    )? {
                        loop_storage.with_storage(|| {
                            ctx.push_vec(&mut loops, *loop_index, "f3d ordered selected profile")
                        })?;
                    }
                }
            }
            ResolvedProfileSelection::Regions(selected) if loops.is_empty() => {
                for region in ctx.admit_iter(selected, "f3d ordered selected region scan")? {
                    if !ctx.any_by(
                        &regions,
                        |previous| {
                            ctx.equal(*previous, region, "f3d ordered selected region comparison")
                        },
                        "f3d ordered selected region duplicate search",
                    )? {
                        region_storage.with_storage(|| {
                            ctx.push_vec(&mut regions, region, "f3d ordered selected region")
                        })?;
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    if !loops.is_empty() {
        loop_storage.commit()?;
        Ok(Some(ResolvedProfileSelection::Loops(loops)))
    } else if !regions.is_empty() {
        let mut output = Vec::new();
        for region in ctx.admit_iter(&regions, "f3d ordered kept region scan")? {
            let region = copy_profile_region(region, ctx)?;
            ctx.push_vec(&mut output, region, "f3d ordered kept region")?;
        }
        Ok(Some(ResolvedProfileSelection::Regions(output)))
    } else {
        Ok(None)
    }
}

fn selection_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut projected_storage = ctx.reserve_scoped(0, "f3d historical projected point storage")?;
    let mut projected = Vec::new();
    let mut input_points = points.iter();
    while let Some(point) =
        ctx.next_charged(&mut input_points, "f3d historical projected point scan")?
    {
        let Some(point) = project_to_sketch(sketch, *point) else {
            return Ok(None);
        };
        projected_storage.with_storage(|| {
            ctx.push_vec(
                &mut projected,
                point,
                "f3d historical projected selection point",
            )
        })?;
    }
    let mut boundary_storage = ctx.reserve_scoped(0, "f3d historical boundary index storage")?;
    let mut boundaries = Vec::new();
    let mut profiles = sketch.profiles.iter().enumerate();
    while let Some((index, profile)) =
        ctx.next_charged(&mut profiles, "f3d historical boundary profile scan")?
    {
        let matches = ctx.all_by(
            &projected,
            |point| {
                ctx.any_by(
                    profile,
                    |use_| {
                        let entity = ctx.find_by(
                            entities,
                            |entity| {
                                ctx.equal(
                                    entity.id(),
                                    &use_.entity,
                                    "f3d historical boundary entity comparison",
                                )
                            },
                            "f3d historical boundary entity search",
                        )?;
                        match entity {
                            Some(entity) => point_on_sketch_entity(ctx, *point, entity, tolerance),
                            None => Ok(false),
                        }
                    },
                    "f3d historical boundary use search",
                )
            },
            "f3d historical boundary point search",
        )?;
        if matches {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            boundary_storage.with_storage(|| {
                ctx.push_vec(&mut boundaries, index, "f3d historical boundary profile")
            })?;
        }
    }
    if let [profile] = boundaries.as_slice() {
        let mut loops = Vec::new();
        ctx.push_vec(&mut loops, *profile, "f3d historical selected profile")?;
        return Ok(Some(ResolvedProfileSelection::Loops(loops)));
    }
    if let Some(region) = arrangement_region_containing_points(
        sketch,
        entities,
        &projected,
        tolerance,
        arrangement_budget,
        ctx,
    )? {
        let mut regions = Vec::new();
        ctx.push_vec(
            &mut regions,
            region,
            "f3d historical selected arrangement region",
        )?;
        return Ok(Some(ResolvedProfileSelection::Regions(regions)));
    }
    if !boundaries.is_empty() {
        return Ok(None);
    }
    let Some(region) = region_containing_points(sketch, entities, points, tolerance, ctx)? else {
        return Ok(None);
    };
    let mut regions = Vec::new();
    ctx.push_vec(
        &mut regions,
        region,
        "f3d historical selected geometric region",
    )?;
    Ok(Some(ResolvedProfileSelection::Regions(regions)))
}

/// Solved sketch records used to bind Loft and Revolve profile operands and
/// Loft guide selections.
pub(crate) struct SketchProfileResolution<'a> {
    pub(crate) entities: &'a [DesignEntityHeader],
    pub(crate) entity_selection_operands: &'a [DesignEntitySelectionOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
}

impl<'a> SketchProfileResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.entity_selection_operands,
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

/// Resolve one complete ordered entity-selection group to a neutral Sketch
/// path. The source identity pair is only meaningful with its owning Sketch
/// identity. Require one-to-one record ownership, a shared selection
/// namespace, a unique placement, and a unique neutral curve for every member
/// before exposing the path as typed geometry.
fn resolve_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PathRef>, CodecError> {
    use cadmpeg_ir::features::PathRef;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if group.members().is_empty() {
        return Ok(None);
    }
    let stream = or_none!(native_stream_charged(ctx, &group.id, "f3d profile native stream")?);
    let mut operand_storage = ctx.reserve_scoped(0, "f3d entity path operand keys")?;
    let mut operand_entries = Vec::new();
    let mut operand_source = resolution.operands.iter();
    while let Some(operand) = ctx.next_charged(&mut operand_source, "f3d entity path operand source")? {
        let stream = native_stream_charged(ctx, &operand.id, "f3d profile native stream")?;
        ctx.push_scoped_vec(
            &mut operand_storage,
            &mut operand_entries,
            ((stream, operand.scope_record_index, operand.group_record_index,
              operand.group_member_ordinal, operand.record_index()), operand),
            "f3d entity path operand key",
        )?;
    }
    let scoped_operands = ctx.unique_index(operand_entries, "f3d entity path operand index")?;
    drop(operand_storage);
    let operands = &scoped_operands.0;
    let mut source_storage = ctx.reserve_scoped(0, "f3d entity path source storage")?;
    let mut selected_identities = Vec::new();
    let mut member_records = HashSet::new();
    let mut primary_identity = None;
    let mut asset_id = None;
    let mut context_id = None;
    let mut members = group.members().iter().enumerate();
    while let Some((ordinal, member)) =
        ctx.next_charged(&mut members, "f3d entity path member scan")?
    {
        let ordinal = or_none!(u32::try_from(ordinal).ok());
        let record_index = member.value;
        if !source_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut member_records,
                record_index,
                "f3d entity path member record",
            )
        })? {
            return Ok(None);
        }
        let operand = or_none!(ctx
            .get_hash_map(
                operands,
                &(
                    Some(stream),
                    group.scope_record_index,
                    group.record_index,
                    ordinal,
                    record_index
                ),
                "f3d entity path operand lookup"
            )?
            .and_then(Option::as_ref));
        let secondary = or_none!(operand.secondary());
        if primary_identity.is_some_and(|expected| expected != operand.primary_identity)
            || match asset_id {
                Some(expected) => !ctx.equal(expected, operand.asset_id.as_str(), "f3d entity path asset comparison")?,
                None => false,
            }
            || match context_id {
                Some(expected) => !ctx.equal(expected, operand.context_id.as_str(), "f3d entity path context comparison")?,
                None => false,
            }
        {
            return Ok(None);
        }
        primary_identity.get_or_insert(operand.primary_identity);
        asset_id.get_or_insert(operand.asset_id.as_str());
        context_id.get_or_insert(operand.context_id.as_str());
        source_storage.with_storage(|| {
            ctx.push_vec(
                &mut selected_identities,
                secondary,
                "f3d entity path selected identity",
            )
        })?;
    }
    let primary_identity = or_none!(primary_identity);
    let placement = or_none!(unique_feature_match_for_decode(ctx, resolution.placements,
        |placement| Ok(ctx.equal(
            &native_stream_charged(ctx, &placement.id, "f3d profile native stream")?,
            &Some(stream),
            "f3d entity path placement stream"
        )? && placement.entity_id.suffix() == primary_identity),
        "f3d entity path placement search"
    )?);
    let mut curve_ids = Vec::new();
    let mut selected_curve_identities = HashSet::new();
    let owner_reference = or_none!(u32::try_from(primary_identity).ok());
    let mut identities = selected_identities.iter();
    while let Some(secondary) =
        ctx.next_charged(&mut identities, "f3d entity path selected identity scan")?
    {
        let curve = or_none!(unique_feature_match_for_decode(ctx, resolution.curve_identities,
            |curve| Ok(ctx.equal(
                &native_stream_charged(ctx, &curve.id, "f3d profile native stream")?,
                &Some(stream),
                "f3d entity path curve stream"
            )? && curve.owner_reference == Some(owner_reference)
                && curve.primary_id.get() == secondary.identity.value
                && secondary
                    .curve_identity
                    .is_none_or(|identity| curve.secondary_id == identity.value)),
            "f3d entity path curve search"
        )?);
        let identity = (curve.primary_id.get(), curve.secondary_id);
        if !source_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut selected_curve_identities,
                identity,
                "f3d entity path curve identity",
            )
        })? {
            return Ok(None);
        }
        source_storage.with_storage(|| {
            ctx.push_vec(&mut curve_ids, identity, "f3d entity path curve pair")
        })?;
    }

    let mut spatial_storage = ctx.reserve_scoped(0, "f3d profile spatial sketch id")?;
    let spatial_sketch = spatial_storage
        .with_storage(|| crate::design::identity::neutral_spatial_sketch_id(ctx, placement))?;
    if ctx.any_by(
        resolution.spatial_sketches,
        |sketch| {
            ctx.equal(
                &sketch.id,
                &spatial_sketch,
                "f3d entity path spatial sketch comparison",
            )
        },
        "f3d entity path spatial sketch search",
    )? {
        let mut curves = Vec::new();
        let mut pairs = curve_ids.iter();
        while let Some((primary, secondary)) =
            ctx.next_charged(&mut pairs, "f3d entity path spatial curve scan")?
        {
            let id = spatial_storage.with_storage(|| {
                crate::design::identity::neutral_spatial_sketch_curve_id(
                    ctx,
                    &spatial_sketch,
                    *primary,
                    *secondary,
                )
            })?;
            if !ctx.any_by(
                resolution.spatial_sketch_entities,
                |entity| {
                    Ok(ctx.equal(
                        &entity.sketch,
                        &spatial_sketch,
                        "f3d entity path spatial owner comparison",
                    )? && ctx.equal(
                        entity.id(),
                        &id,
                        "f3d entity path spatial entity comparison",
                    )?)
                },
                "f3d entity path spatial entity search",
            )? {
                return Ok(None);
            }
            spatial_storage.with_storage(|| {
                ctx.push_vec(&mut curves, id, "f3d entity path spatial output curve")
            })?;
        }
        let path = spatial_storage
            .with_storage(|| -> Result<_, CodecError> {
                Ok(PathRef::spatial_sketch_curves(spatial_sketch, curves, ctx)?)
            })?
            .ok();
        if path.is_some() {
            spatial_storage.commit()?;
        }
        return Ok(path);
    }
    drop((spatial_sketch, spatial_storage));
    let mut output_storage = ctx.reserve_scoped(0, "f3d entity path planar output storage")?;
    let sketch = output_storage
        .with_storage(|| crate::design::identity::neutral_sketch_id(ctx, placement))?;
    if !ctx.any_by(
        resolution.sketches,
        |candidate| {
            ctx.equal(
                &candidate.id,
                &sketch,
                "f3d entity path planar sketch comparison",
            )
        },
        "f3d entity path planar sketch search",
    )? {
        return Ok(None);
    }
    let mut curves = Vec::new();
    let mut pairs = curve_ids.iter();
    while let Some((primary, secondary)) =
        ctx.next_charged(&mut pairs, "f3d entity path planar curve scan")?
    {
        let id = output_storage.with_storage(|| {
            crate::design::identity::neutral_sketch_curve_id(ctx, &sketch, *primary, *secondary)
        })?;
        if !ctx.any_by(
            resolution.sketch_entities,
            |entity| {
                Ok(ctx.equal(
                    &entity.sketch,
                    &sketch,
                    "f3d entity path planar owner comparison",
                )? && ctx.equal(entity.id(), &id, "f3d entity path planar entity comparison")?
                    && !matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ))
            },
            "f3d entity path planar entity search",
        )? {
            return Ok(None);
        }
        output_storage.with_storage(|| {
            ctx.push_vec(&mut curves, id, "f3d entity path planar output curve")
        })?;
    }
    let path = output_storage
        .with_storage(|| -> Result<_, CodecError> {
            Ok(PathRef::sketch_curves(sketch, curves, ctx)?)
        })?
        .ok();
    if path.is_some() {
        output_storage.commit()?;
    }
    Ok(path)
}

/// Resolve one ordered Loft guide or centerline group whose members select
/// curves from a Sketch.
fn resolved_loft_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &SketchProfileResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PathRef>, CodecError> {
    if !matches!(
        group.role(),
        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
    ) {
        return Ok(None);
    }
    let path_resolution = resolution.path_resolution();
    resolve_entity_selection_path(group, &path_resolution, ctx)
}

fn spatial_profile_member_entity<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    curve_identities: &[SketchCurveIdentity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Result<Option<&'a cadmpeg_ir::sketches::SpatialSketchEntity>, CodecError> {
    let curve = or_none!(unique_feature_match_for_decode(ctx, curve_identities,
        |curve| Ok(ctx.equal(
            &native_stream_charged(ctx, &curve.id, "f3d profile native stream")?,
            &Some(stream),
            "f3d profile member curve stream"
        )? && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())),
        "f3d profile member curve search"
    )?);
    let mut temporary_storage =
        ctx.reserve_scoped(0, "f3d spatial profile member identity storage")?;
    let entity_id = temporary_storage.with_storage(|| {
        crate::design::identity::neutral_spatial_sketch_curve_id(
            ctx,
            &spatial_sketch.id,
            curve.primary_id.get(),
            curve.secondary_id,
        )
    })?;
    unique_feature_match_for_decode(ctx, spatial_entities,
        |entity| {
            Ok(ctx.equal(
                &entity.sketch,
                &spatial_sketch.id,
                "f3d spatial profile member sketch comparison",
            )? && ctx.equal(
                entity.id(),
                &entity_id,
                "f3d spatial profile member entity comparison",
            )?)
        },
        "f3d spatial profile member entity search",
    )
}

fn sketch_profile_member_entity<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<&'a cadmpeg_ir::sketches::SketchEntityId>, CodecError> {
    let curve = or_none!(unique_feature_match_for_decode(ctx, curve_identities,
        |curve| Ok(ctx.equal(
            &native_stream_charged(ctx, &curve.id, "f3d profile native stream")?,
            &Some(stream),
            "f3d profile member curve stream"
        )? && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())),
        "f3d profile member curve search"
    )?);
    let mut temporary_storage =
        ctx.reserve_scoped(0, "f3d planar profile member identity storage")?;
    let entity_id = temporary_storage.with_storage(|| {
        crate::design::identity::neutral_sketch_curve_id(
            ctx,
            &sketch.id,
            curve.primary_id.get(),
            curve.secondary_id,
        )
    })?;
    Ok(unique_feature_match_for_decode(ctx, sketch_entities,
        |entity| {
            Ok(ctx.equal(
                &entity.sketch,
                &sketch.id,
                "f3d planar profile member sketch comparison",
            )? && ctx.equal(
                entity.id(),
                &entity_id,
                "f3d planar profile member entity comparison",
            )?)
        },
        "f3d planar profile member entity search",
    )?
    .map(cadmpeg_ir::sketches::SketchEntity::id))
}

fn resolved_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let selection = or_none!(profile.region_selection.as_ref());
    let owner_reference = or_none!(u32::try_from(profile.entity_id.suffix()).ok());
    let mut storage = ctx.reserve_scoped(0, "f3d selected planar region storage")?;
    let mut resolved = Vec::new();
    let mut regions = selection.regions.iter();
    while let Some(region) = ctx.next_charged(&mut regions, "f3d planar region scan")? {
        let first_member = or_none!(region.members.first());
        let first = or_none!(sketch_profile_member_entity(
            ctx,
            stream,
            owner_reference,
            first_member,
            sketch,
            curve_identities,
            sketch_entities
        )?);
        let (profile_index, selected_profile) = or_none!(unique_feature_match_for_decode(ctx, sketch.profiles.iter().enumerate(),
            |(_, profile)| ctx.any_by(
                profile,
                |use_| ctx.equal(&use_.entity, first, "f3d planar region entity comparison"),
                "f3d planar region boundary search"
            ),
            "f3d planar region profile search"
        )?);
        let mut members = region.members[1..].iter();
        while let Some(member) = ctx.next_charged(&mut members, "f3d planar region member scan")? {
            let entity = or_none!(sketch_profile_member_entity(
                ctx,
                stream,
                owner_reference,
                member,
                sketch,
                curve_identities,
                sketch_entities
            )?);
            if !ctx.any_by(
                selected_profile,
                |use_| ctx.equal(&use_.entity, entity, "f3d planar region entity comparison"),
                "f3d planar selected boundary search",
            )? {
                return Ok(None);
            }
        }
        let profile_index = or_none!(u32::try_from(profile_index).ok());
        if ctx.contains(
            &resolved,
            &profile_index,
            "f3d selected planar region duplicate search",
        )? {
            return Ok(None);
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut resolved,
                profile_index,
                "f3d selected planar sketch profile region",
            )
        })?;
    }
    if resolved.is_empty() {
        return Ok(None);
    }
    storage.commit()?;
    Ok(Some(resolved))
}

fn coincident_spatial_profile_geometry(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Circle {
            center: first_center,
            normal: first_normal,
            radius: first_radius,
            ..
        },
        SpatialSketchGeometryDefinition::Circle {
            center: second_center,
            normal: second_normal,
            radius: second_radius,
            ..
        },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    if !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
        || !angular_tolerance.is_finite()
        || angular_tolerance < 0.0
    {
        return false;
    }
    let center_delta = Vector3::new(
        first_center.x - second_center.x,
        first_center.y - second_center.y,
        first_center.z - second_center.z,
    );
    let first_normal = *first_normal.to_unit_length_charted().as_raw();
    let second_normal = *second_normal.to_unit_length_charted().as_raw();
    let normal_angle = first_normal
        .cross(second_normal)
        .norm()
        .atan2(first_normal.dot(second_normal).abs());
    center_delta.norm() <= linear_tolerance
        && (first_radius.get() - second_radius.get()).abs() <= linear_tolerance
        && normal_angle <= angular_tolerance
}

fn resolved_spatial_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    resolution: &SketchProfileResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(selection) = profile.region_selection.as_ref() else {
        if spatial_sketch.profiles.is_empty() {
            return Ok(None);
        }
        let mut storage = ctx.reserve_scoped(0, "f3d all spatial region storage")?;
        let mut profiles = Vec::new();
        let mut indices = 0..spatial_sketch.profiles.len();
        while let Some(index) = ctx.next_charged(&mut indices, "f3d all spatial region scan")? {
            let index = or_none!(u32::try_from(index).ok());
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut profiles,
                    index,
                    "f3d all spatial sketch profile regions",
                )
            })?;
        }
        storage.commit()?;
        return Ok(Some(profiles));
    };
    let owner_reference = or_none!(u32::try_from(profile.entity_id.suffix()).ok());
    let mut storage = ctx.reserve_scoped(0, "f3d selected spatial region storage")?;
    let mut resolved = Vec::new();
    let mut regions = selection.regions.iter();
    while let Some(region) = ctx.next_charged(&mut regions, "f3d spatial region scan")? {
        let first_member = or_none!(region.members.first());
        let first = or_none!(spatial_profile_member_entity(
            ctx,
            stream,
            owner_reference,
            first_member,
            spatial_sketch,
            resolution.curve_identities,
            resolution.spatial_sketch_entities
        )?);
        let (profile_index, selected_profile) = or_none!(unique_feature_match_for_decode(ctx, spatial_sketch.profiles.iter().enumerate(),
            |(_, candidate)| ctx.any_by(
                candidate.boundary(),
                |use_| ctx.equal(
                    &use_.entity,
                    first.id(),
                    "f3d spatial region entity comparison"
                ),
                "f3d spatial region boundary search"
            ),
            "f3d spatial region profile search"
        )?);
        let mut members = region.members[1..].iter();
        while let Some(member) = ctx.next_charged(&mut members, "f3d spatial region member scan")? {
            let entity = or_none!(spatial_profile_member_entity(
                ctx,
                stream,
                owner_reference,
                member,
                spatial_sketch,
                resolution.curve_identities,
                resolution.spatial_sketch_entities
            )?);
            if ctx.any_by(
                selected_profile.boundary(),
                |use_| {
                    ctx.equal(
                        &use_.entity,
                        entity.id(),
                        "f3d spatial region entity comparison",
                    )
                },
                "f3d spatial selected boundary search",
            )? {
                continue;
            }
            let coincident = ctx.any_by(
                selected_profile.boundary(),
                |use_| {
                    let candidate = ctx.find_by(
                        resolution.spatial_sketch_entities,
                        |candidate| {
                            Ok(ctx.equal(
                                &candidate.sketch,
                                &spatial_sketch.id,
                                "f3d coincident profile sketch comparison",
                            )? && ctx.equal(
                                candidate.id(),
                                &use_.entity,
                                "f3d coincident profile entity comparison",
                            )?)
                        },
                        "f3d coincident profile entity search",
                    )?;
                    Ok(candidate.is_some_and(|candidate| {
                        coincident_spatial_profile_geometry(
                            &candidate.geometry,
                            &entity.geometry,
                            resolution.linear_tolerance,
                            resolution.angular_tolerance,
                        )
                    }))
                },
                "f3d coincident profile boundary search",
            )?;
            if !coincident {
                return Ok(None);
            }
        }
        let profile_index = or_none!(u32::try_from(profile_index).ok());
        if ctx.contains(
            &resolved,
            &profile_index,
            "f3d selected spatial region duplicate search",
        )? {
            return Ok(None);
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut resolved,
                profile_index,
                "f3d selected spatial sketch profile region",
            )
        })?;
    }
    if resolved.is_empty() {
        return Ok(None);
    }
    storage.commit()?;
    Ok(Some(resolved))
}

fn spatial_profile_containing_entity(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntityId,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    Ok(unique_feature_match_for_decode(ctx, sketch.profiles.iter().enumerate(),
        |(_, profile)| {
            ctx.any_by(
                profile.boundary(),
                |use_| {
                    ctx.equal(
                        &use_.entity,
                        entity,
                        "f3d spatial member profile entity comparison",
                    )
                },
                "f3d spatial member profile boundary search",
            )
        },
        "f3d spatial member profile search",
    )?
    .and_then(|(index, _)| u32::try_from(index).ok()))
}

pub(crate) fn bind_loft_and_revolve_sketch_selections(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    resolution: &SketchProfileResolution<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftSection, PathRef, PlanarProfileRef, ProfileRef,
    };

    let mut header_storage = ctx.reserve_scoped(0, "f3d loft header storage")?;
    let mut header_index = HashMap::new();
    for header in ctx.admit_iter(headers, "f3d loft header scan")? {
        let Some(stream) = native_stream_charged(ctx, &header.id, "f3d profile native stream")? else {
            continue;
        };
        header_storage
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut header_index,
                    (stream, header.record_index),
                    header,
                    "f3d loft header index",
                )
            })
            .map(|_| ())?;
    }
    let mut profile_index_storage = ctx.reserve_scoped(0, "f3d loft profile index storage")?;
    let mut resolved_profiles = HashMap::new();
    for group in ctx
        .admit_iter(groups, "f3d profile source group scan")?
        .filter(|group| {
            matches!(
                group.role(),
                DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
            ) && group.members().len() == 1
        })
    {
        let Some(stream) = native_stream_charged(ctx, &group.id, "f3d profile native stream")? else {
            continue;
        };
        let Some(entry) =
            scan.design_stream_entry_for_scope(ctx, ContainerRole::Bulkstream, stream)?
        else {
            continue;
        };
        let bytes = scan.entry_bytes(ctx, &entry.name)?;
        let Some(header) = ctx.get_hash_map(
            &header_index,
            &(stream, group.members()[0].value),
            "f3d loft header lookup",
        )?
        else {
            continue;
        };
        let mut temporary_profile_storage =
            ctx.reserve_scoped(0, "f3d loft parsed profile storage")?;
        let profile = temporary_profile_storage.with_storage(|| {
            parse_sketch_profile(
                ctx,
                bytes,
                stream,
                group.scope_reference_ordinal,
                header,
                resolution.entities,
            )
        })?;
        let Some(profile) = profile else {
            continue;
        };

        let Some(placement) = unique_feature_match_for_decode(ctx, resolution.placements.iter(),
            |placement| {
                Ok(ctx.equal(
                    &native_stream_charged(ctx, &placement.id, "f3d profile native stream")?,
                    &Some(stream),
                    "f3d profile candidate stream comparison",
                )? && placement.entity_id == profile.entity_id)
            },
            "f3d profile candidate source search",
        )?
        else {
            continue;
        };

        let mut temporary_identity_storage =
            ctx.reserve_scoped(0, "f3d loft sketch query storage")?;
        let spatial_sketch_id = temporary_identity_storage
            .with_storage(|| crate::design::identity::neutral_spatial_sketch_id(ctx, placement))?;
        let mut resolved_storage = ctx.reserve_scoped(0, "f3d loft resolved profile storage")?;
        let resolved =
            resolved_storage.with_storage(|| -> Result<Option<ProfileRef>, CodecError> {
                let resolved = if let Some(spatial_sketch) = ctx.find_by(
                    resolution.spatial_sketches,
                    |sketch| {
                        ctx.equal(
                            &sketch.id,
                            &spatial_sketch_id,
                            "f3d loft spatial sketch comparison",
                        )
                    },
                    "f3d loft spatial sketch search",
                )? {
                    let id = spatial_sketch_id
                        .try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
                    if let Some(profiles) = resolved_spatial_sketch_profile_regions(
                        stream,
                        &profile,
                        spatial_sketch,
                        resolution,
                        ctx,
                    )? {
                        match ProfileRef::spatial_sketch_profiles(id, profiles, ctx)? {
                            Ok(profile) => profile,
                            Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
                            )),
                        }
                    } else {
                        let group_id =
                            ctx.copy_retained_text(&group.id, "f3d loft spatial selection id")?;
                        match ProfileRef::spatial_sketch_selection(
                            id,
                            {
                                let mut selected = Vec::new();
                                ctx.push_vec(
                                    &mut selected,
                                    group_id,
                                    "f3d loft spatial selection entry",
                                )?;
                                selected
                            },
                            ctx,
                        )? {
                            Ok(profile) => profile,
                            Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
                            )),
                        }
                    }
                } else {
                    let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
                    if !ctx.any_by(
                        resolution.sketches,
                        |candidate| {
                            ctx.equal(&candidate.id, &sketch, "f3d loft planar sketch comparison")
                        },
                        "f3d loft planar sketch search",
                    )? {
                        return Ok(None);
                    }
                    ProfileRef::Planar(PlanarProfileRef::Sketch(sketch))
                };
                Ok(Some(resolved))
            })?;
        let Some(resolved) = resolved else {
            continue;
        };
        profile_index_storage
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut resolved_profiles,
                    group.id.as_str(),
                    (resolved, resolved_storage),
                    "f3d loft resolved profile index",
                )
            })
            .map(|_| ())?;
    }
    let mut path_index_storage = ctx.reserve_scoped(0, "f3d loft path index storage")?;
    let mut resolved_entity_paths = HashMap::new();
    for group in ctx
        .admit_iter(groups, "f3d profile source group scan")?
        .filter(|group| {
            matches!(
                group.role(),
                DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
            ) && !group.members().is_empty()
        })
    {
        let mut path_storage = ctx.reserve_scoped(0, "f3d loft resolved path storage")?;
        let path = path_storage
            .with_storage(|| resolved_loft_entity_selection_path(group, resolution, ctx))?;
        if let Some(path) = path {
            path_index_storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut resolved_entity_paths,
                        group.id.as_str(),
                        (path, path_storage),
                        "f3d loft resolved path index",
                    )
                })
                .map(|_| ())?;
        }
    }
    for group in ctx
        .admit_iter(groups, "f3d profile source group scan")?
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5 && group.members().len() == 1)
    {
        let Some(stream) = native_stream_charged(ctx, &group.id, "f3d profile native stream")? else {
            continue;
        };

        let Some(operand) = unique_feature_match_for_decode(ctx, resolution.entity_selection_operands.iter(),
            |operand| {
                Ok(ctx.equal(
                    &native_stream_charged(ctx, &operand.id, "f3d profile native stream")?,
                    &Some(stream),
                    "f3d profile candidate stream comparison",
                )? && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index() == group.members()[0].value)
            },
            "f3d profile candidate source search",
        )?
        else {
            continue;
        };

        let Some(placement) = unique_feature_match_for_decode(ctx, resolution.placements.iter(),
            |placement| {
                Ok(ctx.equal(
                    &native_stream_charged(ctx, &placement.id, "f3d profile native stream")?,
                    &Some(stream),
                    "f3d profile candidate stream comparison",
                )? && placement.entity_id.suffix() == operand.primary_identity)
            },
            "f3d profile candidate source search",
        )?
        else {
            continue;
        };

        let mut temporary_identity_storage =
            ctx.reserve_scoped(0, "f3d loft sketch query storage")?;
        let spatial_sketch_id = temporary_identity_storage
            .with_storage(|| crate::design::identity::neutral_spatial_sketch_id(ctx, placement))?;
        let Some(spatial_sketch) = ctx.find_by(
            resolution.spatial_sketches,
            |sketch| {
                ctx.equal(
                    &sketch.id,
                    &spatial_sketch_id,
                    "f3d loft spatial sketch comparison",
                )
            },
            "f3d loft spatial sketch search",
        )?
        else {
            continue;
        };
        let Ok(owner_reference) = u32::try_from(operand.primary_identity) else {
            continue;
        };

        let Some(curve) = unique_feature_match_for_decode(ctx, resolution.curve_identities.iter(),
            |curve| {
                Ok(ctx.equal(
                    &native_stream_charged(ctx, &curve.id, "f3d profile native stream")?,
                    &Some(stream),
                    "f3d profile candidate stream comparison",
                )? && curve.owner_reference == Some(owner_reference)
                    && entity_selection_matches_curve(operand, curve))
            },
            "f3d profile candidate source search",
        )?
        else {
            continue;
        };

        let mut temporary_entity_storage = ctx.reserve_scoped(0, "f3d loft curve query storage")?;
        let entity = temporary_entity_storage.with_storage(|| {
            crate::design::identity::neutral_spatial_sketch_curve_id(
                ctx,
                &spatial_sketch_id,
                curve.primary_id.get(),
                curve.secondary_id,
            )
        })?;
        let profile = spatial_profile_containing_entity(spatial_sketch, &entity, ctx)?;
        let mut resolved_storage = ctx.reserve_scoped(0, "f3d loft spatial profile storage")?;
        let resolved = resolved_storage.with_storage(|| -> Result<ProfileRef, CodecError> {
            let id =
                spatial_sketch_id.try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let resolved = if let Some(profile) = profile {
                ProfileRef::spatial_sketch_profiles(
                    id,
                    {
                        let mut selected = Vec::new();
                        ctx.push_vec(&mut selected, profile, "f3d loft spatial profile entry")?;
                        selected
                    },
                    ctx,
                )?
            } else {
                let operand_id =
                    ctx.copy_retained_text(&operand.id, "f3d loft entity selection operand id")?;
                ProfileRef::spatial_sketch_selection(
                    id,
                    {
                        let mut selected = Vec::new();
                        ctx.push_vec(
                            &mut selected,
                            operand_id,
                            "f3d loft operand selection entry",
                        )?;
                        selected
                    },
                    ctx,
                )?
            };
            let resolved = match resolved {
                Ok(profile) => profile,
                Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                    ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
                )),
            };
            Ok(resolved)
        })?;
        profile_index_storage
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut resolved_profiles,
                    group.id.as_str(),
                    (resolved, resolved_storage),
                    "f3d loft resolved profile index",
                )
            })
            .map(|_| ())?;
    }
    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Loft {
                    sections, guidance, ..
                }) = definition
                else {
                    return Ok(());
                };
                for section in ctx.admit_iter(&mut *sections, "f3d loft section scan")? {
                    let LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(native))) =
                        section
                    else {
                        continue;
                    };
                    if let Some((profile, _storage)) = ctx.get_hash_map(
                        &resolved_profiles,
                        native.as_str(),
                        "f3d loft resolved profile lookup",
                    )? {
                        *section = LoftSection::Profile(copy_bound_profile(profile, ctx)?);
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                        for guide in ctx.admit_iter(&mut *guides, "f3d loft guide scan")? {
                            let PathRef::Native(native) = guide else {
                                continue;
                            };
                            if let Some((path, _storage)) = ctx.get_hash_map(
                                &resolved_entity_paths,
                                native.as_str(),
                                "f3d loft resolved path lookup",
                            )? {
                                *guide = copy_bound_path(path, ctx)?;
                            }
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                        if let PathRef::Native(native) = centerline {
                            if let Some((path, _storage)) = ctx.get_hash_map(
                                &resolved_entity_paths,
                                native.as_str(),
                                "f3d loft resolved path lookup",
                            )? {
                                *centerline = copy_bound_path(path, ctx)?;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    for feature in ctx.admit_iter(&mut *features, "f3d sketch selection feature scan")? {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) = definition
                else {
                    return Ok(());
                };
                let Some(PlanarProfileRef::Native(native)) = construction.profile() else {
                    return Ok(());
                };
                let Some((ProfileRef::Planar(profile), _storage)) = ctx.get_hash_map(
                    &resolved_profiles,
                    native.as_str(),
                    "f3d loft resolved profile lookup",
                )?
                else {
                    return Ok(());
                };
                construction.set_profile(Some(copy_bound_planar_profile(profile, ctx)?));
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;
