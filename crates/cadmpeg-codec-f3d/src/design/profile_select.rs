// SPDX-License-Identifier: Apache-2.0
//! Resolve extrude profile selections against sketch regions.

use crate::container::{role, ContainerScan};
use crate::design::decode::operands::{entity_selection_matches_curve, parse_sketch_profile};
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::geometry::{
    arrangement_region_containing_points, historical_member_points_in_state, point_in_polygon,
    point_on_sketch_entity, point_segment_distance, profile_loops_are_independent,
    project_to_sketch, region_containing_points,
};
use crate::ids::{
    self, native_stream, neutral_sketch_curve_id, neutral_sketch_id, neutral_sketch_point_id,
    neutral_sketch_record_id, neutral_spatial_sketch_curve_id, neutral_spatial_sketch_id,
};
use crate::records::{
    DesignConstructionOperandGroup, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignParameterScope,
    DesignRecordHeader, DesignSketchPlacement, DesignSketchProfileOperand,
    DesignSketchProfileRegionMember, SketchCurveIdentity, SketchRelationOperand,
};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

const EPS_PROFILE_SELECT_TRANSITION_PROFILE_SELECTION_E7: f64 = 1e-7;
const EPS_PROFILE_SELECT_TRANSITION_SPATIAL_PROFILE_SELECTION_E7: f64 = 1e-7;
const EPS_PROFILE_SELECT_HISTORICAL_SELECTION_REGIONS_E7: f64 = 1e-7;

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub histories: &'a [crate::history_records::AsmHistory],
    pub scope_histories: &'a HashMap<String, String>,
    pub linear_tolerance: f64,
    pub angular_tolerance: f64,
    pub arrangement_budget: &'a WorkBudget<'a>,
}

#[derive(Clone, Copy)]
pub(crate) struct ScopedExtrudeProfileResolution<'a> {
    entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &'a [crate::history_records::AsmHistory],
    linear_tolerance: f64,
    angular_tolerance: f64,
    arrangement_budget: &'a WorkBudget<'a>,
}

impl<'a> ExtrudeProfileResolution<'a> {
    pub(crate) fn scoped(
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
        }
    }
}

fn histories_for_scope<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [crate::history_records::AsmHistory],
) -> &'a [crate::history_records::AsmHistory] {
    if scope_histories.contains_key(scope_id) {
        crate::history::bound_scope_history(scope_id, scope_histories, histories)
            .map_or(&[], std::slice::from_ref)
    } else {
        histories
    }
}

/// Native and neutral arenas required to resolve curve selections in sketches.
pub(crate) struct SketchCurveSelectionResolution<'a> {
    /// Decoded Design feature scopes.
    pub scopes: &'a [DesignParameterScope],
    /// Counted construction-operand groups.
    pub groups: &'a [DesignConstructionOperandGroup],
    /// Nested entity-selection operands.
    pub operands: &'a [DesignEntitySelectionOperand],
    /// Projected Sketch placement carriers.
    pub placements: &'a [DesignSketchPlacement],
    /// Persistent Sketch curve identities.
    pub curve_identities: &'a [SketchCurveIdentity],
    /// Neutral planar Sketches.
    pub sketches: &'a [cadmpeg_ir::sketches::Sketch],
    /// Neutral planar Sketch entities.
    pub sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    /// Neutral model-space Sketches for non-planar sketch carriers.
    pub spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    /// Neutral model-space entities for non-planar sketch carriers.
    pub spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
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
) {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};
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
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let FeatureDefinition::Sweep {
            section,
            path,
            guide_rail,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if let (Some(ProfileRef::Native(group_id)), Some(profile_operand)) =
            (section.referenced_profile(), scope.sweep_profile.as_ref())
        {
            let group_id = group_id.clone();
            let group_matches = {
                let mut matching_groups = groups.iter().filter(|group| {
                    group.id == group_id
                        && group.scope_record_index == scope.record_index
                        && group.role == 0x0000_0041_0000_0000
                        && group.members.as_slice() == [profile_operand.record_index]
                        && native_stream(&group.id) == Some(stream)
                });
                matches!(
                    (matching_groups.next(), matching_groups.next()),
                    (Some(_), None)
                )
            };
            if group_matches {
                let mut candidates = placements.iter().filter(|placement| {
                    native_stream(&placement.id) == Some(stream)
                        && placement.entity_suffix == profile_operand.entity_suffix
                });
                if let (Some(placement), None) = (candidates.next(), candidates.next()) {
                    let sketch = neutral_sketch_id(placement);
                    if sketches.iter().any(|candidate| candidate.id == sketch) {
                        *section =
                            cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(sketch));
                    }
                }
            }
        }
        if let Some(ProfileRef::Native(group_id)) = section.referenced_profile() {
            let group_id = group_id.clone();
            let resolved = (|| {
                let mut matching_groups = groups.iter().filter(|group| {
                    group.id == group_id
                        && group.scope_record_index == scope.record_index
                        && group.role == 0x0000_0041_0000_0000
                        && group.members.len() == 1
                        && native_stream(&group.id) == Some(stream)
                });
                let group = matching_groups.next()?;
                if matching_groups.next().is_some() {
                    return None;
                }
                let mut matching_operands = operands.iter().filter(|operand| {
                    operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == 0
                        && operand.record_index == group.members[0]
                        && native_stream(&operand.id) == Some(stream)
                });
                let operand = matching_operands.next()?;
                if matching_operands.next().is_some() {
                    return None;
                }
                let mut matching_placements = placements.iter().filter(|placement| {
                    native_stream(&placement.id) == Some(stream)
                        && placement.entity_suffix == operand.primary_identity
                });
                let placement = matching_placements.next()?;
                if matching_placements.next().is_some() {
                    return None;
                }
                let sketch = neutral_sketch_id(placement);
                if !sketches.iter().any(|candidate| candidate.id == sketch) {
                    return None;
                }
                let owner_reference = u32::try_from(operand.primary_identity).ok()?;
                let mut matching_curves = curve_identities.iter().filter(|curve| {
                    native_stream(&curve.id) == Some(stream)
                        && curve.owner_reference == Some(owner_reference)
                        && entity_selection_matches_curve(operand, curve)
                });
                let curve = matching_curves.next()?;
                if matching_curves.next().is_some() {
                    return None;
                }
                let selected =
                    neutral_sketch_curve_id(&sketch, curve.primary_id, curve.secondary_id);
                sketch_entities
                    .iter()
                    .any(|entity| entity.sketch == sketch && entity.id == selected)
                    .then_some((sketch, selected))
            })();
            if let Some((sketch, selected)) = resolved {
                *section =
                    cadmpeg_ir::features::SweepSection::Profile(ProfileRef::SketchEntities {
                        sketch,
                        entities: vec![selected],
                    });
            }
        }
        let resolve_path = |path: &mut PathRef| -> Option<()> {
            let PathRef::Native(group_id) = path else {
                return None;
            };
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && group.role == 0x0000_0005_0000_0000
                    && native_stream(&group.id) == Some(stream)
            });
            let group = matching_groups.next()?;
            if matching_groups.next().is_some() || group.members.len() != 1 {
                return None;
            }
            *path = resolve_entity_selection_path(group, &path_resolution)?;
            Some(())
        };
        if let Some(path) = path {
            let _ = resolve_path(path);
        }
        if let Some(guide_rail) = guide_rail {
            let _ = resolve_path(&mut guide_rail.path);
        }
    }
}

/// Resolve `SplitFace` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_split_face_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, SplitFaceTool};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        let FeatureDefinition::SplitFace { tool, .. } = &mut feature.definition else {
            continue;
        };
        let SplitFaceTool::Path(PathRef::Native(group_id)) = tool else {
            continue;
        };
        let mut matching_groups = resolution.groups.iter().filter(|group| {
            group.id == *group_id
                && group.role == 0x0000_0021_0000_0000
                && !group.members.is_empty()
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        if let Some(path) = resolve_entity_selection_path(group, &path_resolution) {
            *tool = SplitFaceTool::Path(path);
        }
    }
}

/// Resolve `SurfaceTrim` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_surface_trim_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        let FeatureDefinition::TrimSurface { tool, .. } = &mut feature.definition else {
            continue;
        };
        let PathRef::Native(group_id) = tool else {
            continue;
        };
        let mut matching_groups = resolution.groups.iter().filter(|group| {
            group.id == *group_id
                && group.role == 0x0000_0021_0000_0000
                && !group.members.is_empty()
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        if let Some(path) = resolve_entity_selection_path(group, &path_resolution) {
            *tool = path;
        }
    }
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    for feature in features {
        let Some(scope) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|candidate| candidate.id == scope) else {
            continue;
        };
        let scoped_histories =
            histories_for_scope(&scope.id, resolution.scope_histories, resolution.histories);
        let scoped_resolution = resolution.scoped(scoped_histories);
        let effective_previous_history_state_id =
            crate::history::effective_scope_previous_history_state_id(scope, scoped_histories);
        let mut matching_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        matching_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let FeatureDefinition::Extrude { profile, .. } = &mut feature.definition else {
            continue;
        };
        if let ProfileRef::Native(native) = profile {
            let mut entity_groups = curve_resolution.groups.iter().filter(|group| {
                group.id == *native
                    && native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            });
            if let (Some(group), None) = (entity_groups.next(), entity_groups.next()) {
                if let Some(selection) =
                    resolve_entity_selection_profile(group, &curve_resolution.path_resolution())
                {
                    *profile = selection;
                    continue;
                }
            }
            if let Some(selection) = historical_face_profile_selection(
                &matching_groups,
                members,
                effective_previous_history_state_id,
                &feature.id,
                scoped_histories,
            ) {
                *profile = selection;
            }
            continue;
        }
        let ProfileRef::Sketch(sketch_id) = profile else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            if matching_groups.is_empty() {
                continue;
            }
            let spatial_id = cadmpeg_ir::sketches::SpatialSketchId(sketch_id.0.replacen(
                "f3d:model:sketch#",
                "f3d:model:spatial-sketch#",
                1,
            ));
            if let Some(spatial_sketch) = resolution
                .spatial_sketches
                .iter()
                .find(|candidate| candidate.id == spatial_id)
            {
                let selections = matching_groups
                    .iter()
                    .map(|group| {
                        resolved_spatial_extrude_profile_selection(
                            group,
                            members,
                            spatial_sketch,
                            scoped_resolution.spatial_entities,
                            scoped_resolution,
                            scope.history_state_id,
                            effective_previous_history_state_id,
                        )
                    })
                    .collect::<Vec<_>>();
                let mut indices = Vec::new();
                if selections.iter().all(|selection| {
                    if let Some(index) = selection {
                        if !indices.contains(index) {
                            indices.push(*index);
                        }
                        true
                    } else {
                        false
                    }
                }) {
                    *profile = ProfileRef::SpatialSketchProfiles {
                        sketch: spatial_id,
                        profiles: indices,
                    };
                } else {
                    *profile = ProfileRef::SpatialSketchSelection {
                        sketch: spatial_id,
                        selections: matching_groups
                            .iter()
                            .map(|group| group.id.clone())
                            .collect(),
                    };
                }
                continue;
            }
            *profile = ProfileRef::Native(match matching_groups.as_slice() {
                [group] => group.id.clone(),
                _ => scope.id.clone(),
            });
            continue;
        };
        if let (Some(profile_operand), Some(stream)) =
            (scope.extrude_profile.as_ref(), native_stream(&scope.id))
        {
            if let Some(profiles) = resolved_sketch_profile_regions(
                stream,
                profile_operand,
                sketch,
                curve_resolution.curve_identities,
                curve_resolution.sketch_entities,
            ) {
                *profile = ProfileRef::SketchProfiles {
                    sketch: sketch_id.clone(),
                    profiles,
                };
                continue;
            }
        }
        if matching_groups.is_empty() {
            continue;
        }
        let selections = matching_groups
            .iter()
            .map(|group| {
                resolved_extrude_profile_selection(
                    sketch_id,
                    group,
                    members,
                    sketch,
                    scoped_resolution,
                    scope.history_state_id,
                    effective_previous_history_state_id,
                )
            })
            .collect::<Vec<_>>();
        *profile = merge_resolved_profile_selections(sketch_id, &selections).unwrap_or_else(|| {
            ProfileRef::SketchSelection {
                sketch: sketch_id.clone(),
                selections: matching_groups
                    .iter()
                    .map(|group| group.id.clone())
                    .collect(),
            }
        });
    }
}

fn resolve_entity_selection_profile(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::{PathRef, ProfileRef};

    if group.role != 0x41_0000_0000 {
        return None;
    }
    match resolve_entity_selection_path(group, resolution)? {
        PathRef::SketchCurves { sketch, curves } => {
            let source = resolution
                .sketches
                .iter()
                .find(|source| source.id == sketch)?;
            let mut selected_profiles = Vec::new();
            let mut has_unprofiled_entity = false;
            for curve in &curves {
                let mut matches = source
                    .profiles
                    .iter()
                    .enumerate()
                    .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == *curve));
                let Some((profile_index, _)) = matches.next() else {
                    has_unprofiled_entity = true;
                    continue;
                };
                if matches.next().is_some() {
                    return None;
                }
                let profile_index = u32::try_from(profile_index).ok()?;
                if !selected_profiles.contains(&profile_index) {
                    selected_profiles.push(profile_index);
                }
            }
            if has_unprofiled_entity {
                Some(ProfileRef::SketchEntities {
                    sketch,
                    entities: curves,
                })
            } else {
                Some(ProfileRef::SketchProfiles {
                    sketch,
                    profiles: selected_profiles,
                })
            }
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let source = resolution
                .spatial_sketches
                .iter()
                .find(|source| source.id == sketch)?;
            let profiles = selected_profile_indices(
                curves.iter(),
                source.profiles.iter().map(|profile| {
                    profile
                        .boundary
                        .iter()
                        .map(|use_| &use_.entity)
                        .collect::<HashSet<_>>()
                }),
            )?;
            Some(ProfileRef::SpatialSketchProfiles { sketch, profiles })
        }
        _ => None,
    }
}

fn selected_profile_indices<'a, Id: Eq + std::hash::Hash + 'a>(
    selected: impl IntoIterator<Item = &'a Id>,
    profiles: impl IntoIterator<Item = HashSet<&'a Id>>,
) -> Option<Vec<u32>> {
    let profiles = profiles.into_iter().collect::<Vec<_>>();
    let mut selected_profiles = Vec::new();
    for entity in selected {
        let mut matches = profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| profile.contains(entity));
        let (index, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let index = u32::try_from(index).ok()?;
        if !selected_profiles.contains(&index) {
            selected_profiles.push(index);
        }
    }
    (!selected_profiles.is_empty()).then_some(selected_profiles)
}

fn historical_face_profile_selection(
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scoped_histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let previous_state_id = previous_state_id?;
    let mut states = scoped_histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let topology = states.next()?.topology.as_ref()?;
    if states.next().is_some() {
        return None;
    }
    let stream = groups.first().and_then(|group| native_stream(&group.id))?;
    let mut selected_faces = Vec::new();
    for group in groups {
        if native_stream(&group.id) != Some(stream) {
            return None;
        }
        let mut group_members = members
            .iter()
            .filter(|member| {
                native_stream(&member.id) == Some(stream)
                    && member.group_record_index == group.record_index
            })
            .collect::<Vec<_>>();
        group_members.sort_by_key(|member| member.group_member_ordinal);
        if group_members.len() != group.members.len()
            || group_members
                .iter()
                .zip(&group.members)
                .any(|(member, record_index)| member.record_index != *record_index)
        {
            return None;
        }
        let mut candidates = None::<HashSet<i64>>;
        for member in group_members {
            if !member.historical_state_ids.is_empty()
                && !member.historical_state_ids.contains(&previous_state_id)
            {
                return None;
            }
            let entity_ref = member
                .historical_entity_ref
                .or_else(|| i64::try_from(member.local_id).ok())?;
            let member_faces = historical_profile_face_candidates(
                member.historical_entity_kind,
                entity_ref,
                topology,
            );
            if member_faces.is_empty() {
                return None;
            }
            candidates = Some(match candidates {
                None => member_faces,
                Some(mut candidates) => {
                    candidates.retain(|face| member_faces.contains(face));
                    candidates
                }
            });
        }
        let candidates = candidates?;
        let mut candidates = candidates.into_iter();
        let face = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        if !selected_faces.contains(&face) {
            selected_faces.push(face);
        }
    }
    if selected_faces.is_empty() {
        return None;
    }
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    Some(ProfileRef::HistoricalFaces {
        state: feature_input_topology_id(feature_id, previous_state_id),
        faces: selected_faces
            .into_iter()
            .map(|face| {
                ids::history_input_face_id(
                    &ids::history_input_prefix(feature_key, previous_state_id),
                    face,
                )
            })
            .collect(),
        native: groups.iter().map(|group| group.id.clone()).collect(),
    })
}

pub(crate) fn historical_profile_face_candidates(
    kind: Option<crate::records::AsmHistoricalEntityKind>,
    entity_ref: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> HashSet<i64> {
    use crate::records::AsmHistoricalEntityKind;

    let kinds = match kind {
        Some(kind) => vec![kind],
        None => vec![
            AsmHistoricalEntityKind::Face,
            AsmHistoricalEntityKind::Loop,
            AsmHistoricalEntityKind::Coedge,
            AsmHistoricalEntityKind::Edge,
            AsmHistoricalEntityKind::Pcurve,
            AsmHistoricalEntityKind::Curve,
            AsmHistoricalEntityKind::Vertex,
            AsmHistoricalEntityKind::Point,
            AsmHistoricalEntityKind::Surface,
        ],
    };
    let loop_faces = |loop_ref| {
        topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&loop_ref))
            .map(|relation| relation.owner_ref)
            .collect::<HashSet<_>>()
    };
    let coedge_faces = |coedge_ref| {
        topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == coedge_ref)
            .flat_map(|coedge| loop_faces(coedge.owner_loop))
            .collect::<HashSet<_>>()
    };
    let edge_faces = |edge_ref| {
        topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.edge == edge_ref)
            .flat_map(|coedge| loop_faces(coedge.owner_loop))
            .collect::<HashSet<_>>()
    };
    let mut faces = HashSet::new();
    for kind in kinds {
        match kind {
            AsmHistoricalEntityKind::Face => {
                if topology.faces.contains(&entity_ref) {
                    faces.insert(entity_ref);
                }
            }
            AsmHistoricalEntityKind::Loop => faces.extend(loop_faces(entity_ref)),
            AsmHistoricalEntityKind::Coedge => faces.extend(coedge_faces(entity_ref)),
            AsmHistoricalEntityKind::Edge => faces.extend(edge_faces(entity_ref)),
            AsmHistoricalEntityKind::Pcurve => faces.extend(
                topology
                    .coedge_pcurves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                    .flat_map(|binding| coedge_faces(binding.entity)),
            ),
            AsmHistoricalEntityKind::Curve => faces.extend(
                topology
                    .edge_curves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                    .flat_map(|binding| edge_faces(binding.entity)),
            ),
            AsmHistoricalEntityKind::Vertex => faces.extend(
                topology
                    .edge_vertices
                    .iter()
                    .filter(|edge| edge.start_vertex == entity_ref || edge.end_vertex == entity_ref)
                    .flat_map(|edge| edge_faces(edge.edge)),
            ),
            AsmHistoricalEntityKind::Point => {
                let vertices = topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity)
                    .collect::<HashSet<_>>();
                faces.extend(
                    topology
                        .edge_vertices
                        .iter()
                        .filter(|edge| {
                            vertices.contains(&edge.start_vertex)
                                || vertices.contains(&edge.end_vertex)
                        })
                        .flat_map(|edge| edge_faces(edge.edge)),
                );
            }
            AsmHistoricalEntityKind::Surface => faces.extend(
                topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity),
            ),
            AsmHistoricalEntityKind::Body
            | AsmHistoricalEntityKind::Region
            | AsmHistoricalEntityKind::Shell => {}
        }
    }
    faces
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ResolvedProfileSelection {
    Loops(Vec<u32>),
    Regions(Vec<cadmpeg_ir::features::SketchProfileRegion>),
}

pub(crate) fn merge_resolved_profile_selections(
    sketch: &cadmpeg_ir::sketches::SketchId,
    selections: &[cadmpeg_ir::features::ProfileRef],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    match ordered_unique_profile_selections(selections.iter().map(|selection| match selection {
        ProfileRef::SketchProfiles {
            sketch: selected,
            profiles,
        } if selected == sketch => Some(ResolvedProfileSelection::Loops(profiles.clone())),
        ProfileRef::SketchRegions {
            sketch: selected,
            regions,
        } if selected == sketch => Some(ResolvedProfileSelection::Regions(regions.clone())),
        _ => None,
    }))? {
        ResolvedProfileSelection::Loops(profiles) => Some(ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles,
        }),
        ResolvedProfileSelection::Regions(regions) => Some(ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions,
        }),
    }
}

pub(crate) fn resolved_extrude_profile_selection(
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> cadmpeg_ir::features::ProfileRef {
    use cadmpeg_ir::features::ProfileRef;

    let mut selection_members = members
        .iter()
        .filter(|member| {
            native_stream(&member.id) == native_stream(&group.id)
                && member.group_record_index == group.record_index
        })
        .collect::<Vec<_>>();
    selection_members.sort_by_key(|member| member.group_member_ordinal);
    let exact_member_run = selection_members.len() == group.members.len()
        && selection_members
            .iter()
            .zip(&group.members)
            .all(|(member, record_index)| member.record_index == *record_index);
    let resolved_profiles = exact_member_run.then(|| {
        let mut selected = Vec::new();
        for member in &selection_members {
            let SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            } = member.resolved_geometry.as_ref()?
            else {
                return None;
            };
            let entity = neutral_sketch_curve_id(sketch_id, *primary_id, *secondary_id);
            let matches = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity))
                .map(|(index, _)| u32::try_from(index).ok())
                .collect::<Option<Vec<_>>>()?;
            let [profile_index] = matches.as_slice() else {
                return None;
            };
            if !selected.contains(profile_index) {
                selected.push(*profile_index);
            }
        }
        (!selected.is_empty()).then_some(ResolvedProfileSelection::Loops(selected))
    });
    let resolved_profiles = resolved_profiles
        .flatten()
        .or_else(|| {
            exact_member_run.then(|| {
                historical_selection_regions(
                    &selection_members,
                    sketch,
                    resolution.entities,
                    resolution.histories,
                    resolution.linear_tolerance,
                    resolution.arrangement_budget,
                )
            })?
        })
        .or_else(|| {
            transition_profile_selection(
                sketch,
                resolution,
                history_state_id?,
                previous_history_state_id?,
            )
        })
        .or_else(|| {
            (sketch.profiles.len() == 1).then_some(ResolvedProfileSelection::Loops(vec![0]))
        });
    match resolved_profiles {
        Some(ResolvedProfileSelection::Loops(profiles)) => ProfileRef::SketchProfiles {
            sketch: sketch_id.clone(),
            profiles,
        },
        Some(ResolvedProfileSelection::Regions(regions)) => ProfileRef::SketchRegions {
            sketch: sketch_id.clone(),
            regions,
        },
        None => ProfileRef::SketchSelection {
            sketch: sketch_id.clone(),
            selections: vec![group.id.clone()],
        },
    }
}

fn transition_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
) -> Option<ResolvedProfileSelection> {
    let entities = resolution.entities;
    let arrangement_budget = resolution.arrangement_budget;
    let mut states = resolution
        .histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
    {
        return None;
    }
    let topology = state.topology.as_ref()?;
    let inserted_faces = &state.transition.as_ref()?.topology.faces.inserted;
    let tolerance = resolution
        .linear_tolerance
        .max(EPS_PROFILE_SELECT_TRANSITION_PROFILE_SELECTION_E7);
    let inserted = transition_inserted_profile_selection(
        sketch,
        entities,
        tolerance,
        inserted_faces.iter().map(|face| {
            let points = historical_face_points(*face, topology)?;
            selection_containing_points(sketch, entities, &points, tolerance, arrangement_budget)
        }),
    );
    if inserted.is_some() {
        return inserted;
    }
    if let Some(selection) = unique_resolved_selection(inserted_faces.iter().map(|face| {
        inserted_cylindrical_profile_selection(
            sketch,
            entities,
            topology,
            *face,
            tolerance,
            resolution.angular_tolerance,
        )
    })) {
        return Some(selection);
    }
    let mut previous_states = resolution
        .histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some() {
        return None;
    }
    let previous_topology = previous.topology.as_ref()?;
    let deleted = &state.transition.as_ref()?.topology.faces.deleted;
    let faces = unique_multi_face_deleted_carrier_family(deleted, previous_topology)?;
    ordered_unique_profile_selections(faces.into_iter().map(|face| {
        let points = historical_face_points(face, previous_topology)?;
        selection_containing_points(sketch, entities, &points, tolerance, arrangement_budget)
    }))
}

pub(crate) fn inserted_cylindrical_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Option<ResolvedProfileSelection> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut carriers = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let carrier = carriers.next()?.carrier;
    if carriers.next().is_some() {
        return None;
    }
    let mut cylinders = topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| cylinder.surface == carrier);
    let cylinder = cylinders.next()?;
    if cylinders.next().is_some() || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
        return None;
    }
    let (sketch_origin, sketch_normal, _) = sketch.resolved_placement()?;
    let alignment = cylinder.axis.x * sketch_normal.x
        + cylinder.axis.y * sketch_normal.y
        + cylinder.axis.z * sketch_normal.z;
    if alignment.abs() < angular_tolerance.cos() {
        return None;
    }
    let offset = (sketch_origin.x - cylinder.origin.x) * sketch_normal.x
        + (sketch_origin.y - cylinder.origin.y) * sketch_normal.y
        + (sketch_origin.z - cylinder.origin.z) * sketch_normal.z;
    let parameter = offset / alignment;
    let center = project_to_sketch(
        sketch,
        Point3::new(
            cylinder.origin.x + parameter * cylinder.axis.x,
            cylinder.origin.y + parameter * cylinder.axis.y,
            cylinder.origin.z + parameter * cylinder.axis.z,
        ),
    )?;
    let points = historical_face_points(face, topology)?;
    let projected = points
        .iter()
        .map(|point| project_to_sketch(sketch, *point))
        .collect::<Option<Vec<_>>>()?;
    let mut matches = sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            let [use_] = profile.as_slice() else {
                return None;
            };
            let entity = entities
                .iter()
                .find(|entity| entity.sketch == sketch.id && entity.id == use_.entity)?;
            let SketchGeometry::Circle {
                center: candidate_center,
                radius: candidate_radius,
            } = entity.geometry
            else {
                return None;
            };
            ((candidate_center.u - center.u).hypot(candidate_center.v - center.v)
                <= linear_tolerance
                && (candidate_radius.0 - cylinder.radius).abs() <= linear_tolerance
                && projected
                    .iter()
                    .all(|point| point_on_sketch_entity(*point, entity, linear_tolerance)))
            .then(|| u32::try_from(index).ok())?
        });
    let profile = matches.next()?;
    matches
        .next()
        .is_none()
        .then(|| ResolvedProfileSelection::Loops(vec![profile]))
}

fn resolved_spatial_extrude_profile_selection(
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Option<u32> {
    enum ExactSelection {
        Resolved(u32),
        Unavailable,
        Contradictory,
    }

    let mut group_members = members
        .iter()
        .filter(|member| {
            native_stream(&member.id) == native_stream(&group.id)
                && member.group_record_index == group.record_index
        })
        .collect::<Vec<_>>();
    group_members.sort_by_key(|member| member.group_member_ordinal);
    let exact_member_run = group_members.len() == group.members.len()
        && group_members
            .iter()
            .zip(&group.members)
            .all(|(member, record_index)| member.record_index == *record_index);
    let exact_selection = (|| {
        if !exact_member_run {
            return ExactSelection::Unavailable;
        }
        let mut selected = None;
        for member in group_members {
            let Some(SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            }) = member.resolved_geometry.as_ref()
            else {
                return ExactSelection::Unavailable;
            };
            let entity =
                crate::ids::neutral_spatial_sketch_curve_id(&sketch.id, *primary_id, *secondary_id);
            let matches = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.boundary.iter().any(|use_| use_.entity == entity))
                .map(|(index, _)| u32::try_from(index).ok())
                .collect::<Option<Vec<_>>>();
            let Some(matches) = matches else {
                return ExactSelection::Unavailable;
            };
            let [profile] = matches.as_slice() else {
                return ExactSelection::Unavailable;
            };
            if selected
                .replace(*profile)
                .is_some_and(|selected| selected != *profile)
            {
                return ExactSelection::Contradictory;
            }
        }
        selected.map_or(ExactSelection::Unavailable, ExactSelection::Resolved)
    })();
    match exact_selection {
        ExactSelection::Resolved(selection) => Some(selection),
        ExactSelection::Contradictory => None,
        ExactSelection::Unavailable => history_state_id
            .zip(previous_history_state_id)
            .and_then(|(state_id, previous_state_id)| {
                transition_spatial_profile_selection(
                    sketch,
                    entities,
                    resolution.histories,
                    state_id,
                    previous_state_id,
                    resolution.linear_tolerance,
                )
            })
            .or_else(|| (sketch.profiles.len() == 1).then_some(0)),
    }
}

fn transition_spatial_profile_selection(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
) -> Option<u32> {
    let mut states = histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
    {
        return None;
    }
    let topology = state.topology.as_ref()?;
    let tolerance =
        linear_tolerance.max(EPS_PROFILE_SELECT_TRANSITION_SPATIAL_PROFILE_SELECTION_E7);
    let unique = |faces: &[i64], topology: &crate::history_records::AsmHistoricalTopology| {
        let mut indices = faces
            .iter()
            .filter_map(|face| {
                let points = historical_face_points(*face, topology)?;
                spatial_polyline_profile_containing_points(sketch, entities, &points, tolerance)
            })
            .collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        (indices.len() == 1).then(|| indices[0])
    };
    if let Some(index) = unique(
        &state.transition.as_ref()?.topology.faces.inserted,
        topology,
    ) {
        return Some(index);
    }
    let mut previous_states = histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some() {
        return None;
    }
    unique(
        &state.transition.as_ref()?.topology.faces.deleted,
        previous.topology.as_ref()?,
    )
}

fn spatial_polyline_profile_containing_points(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<u32> {
    let mut matches = Vec::new();
    for (index, profile) in sketch.profiles.iter().enumerate() {
        let offsets = points
            .iter()
            .map(|point| point.vector_from(profile.origin).dot(profile.normal))
            .collect::<Vec<_>>();
        if !offsets.first().is_some_and(|first| {
            offsets
                .iter()
                .all(|offset| (offset - first).abs() <= tolerance)
        }) {
            continue;
        }
        let v_axis = profile.normal.cross(profile.u_axis);
        let project = |point: Point3| {
            let offset = point.vector_from(profile.origin);
            Point2::new(offset.dot(profile.u_axis), offset.dot(v_axis))
        };
        let polygon = profile
            .boundary
            .iter()
            .map(|use_| {
                let entity = entities
                    .iter()
                    .find(|entity| entity.sketch == sketch.id && entity.id == use_.entity)?;
                let cadmpeg_ir::sketches::SpatialSketchGeometry::Line { start, end } =
                    &entity.geometry
                else {
                    return None;
                };
                Some(project(if use_.reversed { *end } else { *start }))
            })
            .collect::<Option<Vec<_>>>()?;
        if polygon.len() >= 3
            && points.iter().all(|point| {
                let point = project(*point);
                point_in_polygon(point, &polygon)
                    || polygon.iter().enumerate().any(|(index, start)| {
                        let end = polygon[(index + 1) % polygon.len()];
                        point_segment_distance(point, (*start, end)) <= tolerance
                    })
            })
        {
            matches.push(u32::try_from(index).ok()?);
        }
    }
    let [selected] = matches.as_slice() else {
        return None;
    };
    Some(*selected)
}

pub(crate) fn unique_multi_face_deleted_carrier_family(
    deleted_faces: &[i64],
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let mut seen = HashSet::new();
    let mut families = HashMap::<i64, Vec<i64>>::new();
    for face in deleted_faces.iter().copied() {
        if !seen.insert(face) {
            return None;
        }
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let carrier = bindings.next()?.carrier;
        if bindings.next().is_some() {
            return None;
        }
        families.entry(carrier).or_default().push(face);
    }
    let mut candidates = families.into_values().filter(|faces| faces.len() > 1);
    let mut faces = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    faces.sort_unstable();
    Some(faces)
}

pub(crate) fn unique_resolved_selection<T: PartialEq>(
    selections: impl IntoIterator<Item = Option<T>>,
) -> Option<T> {
    let mut selections = selections.into_iter().flatten();
    let first = selections.next()?;
    selections
        .all(|selection| selection == first)
        .then_some(first)
}

pub(crate) fn transition_inserted_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    selections: impl IntoIterator<Item = Option<ResolvedProfileSelection>>,
) -> Option<ResolvedProfileSelection> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let selections = selections.into_iter().flatten().collect::<Vec<_>>();
    if let Some(selection) = unique_resolved_selection(selections.iter().cloned().map(Some)) {
        return Some(selection);
    }
    let loop_selections = selections
        .iter()
        .filter_map(|selection| match selection {
            ResolvedProfileSelection::Loops(loops) if !loops.is_empty() => Some(loops.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if let Some(first) = loop_selections.first() {
        if loop_selections.iter().all(|candidate| candidate == first) {
            return Some(ResolvedProfileSelection::Loops(first.to_vec()));
        }
    }
    if loop_selections.len() == selections.len() {
        let loops = loop_selections
            .iter()
            .flat_map(|selection| selection.iter().copied())
            .fold(Vec::new(), |mut loops, profile| {
                if !loops.contains(&profile) {
                    loops.push(profile);
                }
                loops
            });
        if !loops.is_empty() && profile_loops_are_independent(sketch, entities, &loops, tolerance) {
            return Some(ResolvedProfileSelection::Loops(loops));
        }
    }
    let mut regions = selections.iter().filter_map(|selection| match selection {
        ResolvedProfileSelection::Regions(regions) => match regions.as_slice() {
            [SketchProfileRegion::Loops { outer, holes }] if !holes.is_empty() => {
                Some((*outer, holes.as_slice()))
            }
            _ => None,
        },
        ResolvedProfileSelection::Loops(_) => None,
    });
    let (outer, holes) = regions.next()?;
    if regions.any(|candidate| candidate != (outer, holes)) {
        return None;
    }
    let mut has_boundary_support = false;
    for selection in &selections {
        match selection {
            ResolvedProfileSelection::Regions(regions)
                if matches!(
                    regions.as_slice(),
                    [SketchProfileRegion::Loops {
                        outer: candidate_outer,
                        holes: candidate_holes,
                    }] if *candidate_outer == outer && candidate_holes.as_slice() == holes
                ) => {}
            ResolvedProfileSelection::Loops(loops)
                if !loops.is_empty()
                    && loops
                        .iter()
                        .all(|profile| *profile == outer || holes.contains(profile)) =>
            {
                has_boundary_support = true;
            }
            _ => return None,
        }
    }
    has_boundary_support.then(|| {
        ResolvedProfileSelection::Regions(vec![SketchProfileRegion::Loops {
            outer,
            holes: holes.to_vec(),
        }])
    })
}

pub(crate) fn historical_face_points(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    let loops = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)?;
    let mut positions = Vec::new();
    for loop_ref in &loops.member_refs {
        let coedges = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)?;
        for coedge_ref in &coedges.member_refs {
            let coedge = topology
                .coedge_topology
                .iter()
                .find(|coedge| coedge.coedge == *coedge_ref)?;
            let edge = topology
                .edge_vertices
                .iter()
                .find(|edge| edge.edge == coedge.edge)?;
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                let point_ref = topology
                    .vertex_points
                    .iter()
                    .find(|binding| binding.entity == vertex_ref)?
                    .carrier;
                let position = topology
                    .point_positions
                    .iter()
                    .find(|point| point.point == point_ref)?
                    .position;
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
    }
    (positions.len() >= 3).then_some(positions)
}

fn historical_selection_regions(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    linear_tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
) -> Option<ResolvedProfileSelection> {
    let tolerance = linear_tolerance.max(EPS_PROFILE_SELECT_HISTORICAL_SELECTION_REGIONS_E7);
    let mut states = HashMap::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    let mut state_ids = members
        .first()?
        .historical_state_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    for member in &members[1..] {
        state_ids.retain(|state_id| member.historical_state_ids.contains(state_id));
    }
    let mut state_ids = state_ids.into_iter().collect::<Vec<_>>();
    state_ids.sort_unstable();
    let mut previous_member_points = None;
    let mut previous_selection = None;
    let state_selections = state_ids
        .into_iter()
        .filter_map(|state_id| {
            let topology = states.get(&state_id)?.as_ref()?.topology.as_ref()?;
            let member_points = members
                .iter()
                .map(|member| {
                    historical_member_points_in_state(member, topology)
                        .or_else(|| resolved_selection_member_points(member, sketch, entities))
                })
                .collect::<Option<Vec<_>>>()?;
            let key = member_points
                .iter()
                .map(|points| {
                    points
                        .iter()
                        .map(|point| (point.x.to_bits(), point.y.to_bits(), point.z.to_bits()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            if previous_member_points.as_ref() == Some(&key) {
                return previous_selection.clone();
            }
            let selection = selection_for_member_points(
                members,
                sketch,
                entities,
                &member_points,
                tolerance,
                arrangement_budget,
            );
            previous_member_points = Some(key);
            previous_selection.clone_from(&selection);
            selection
        })
        .collect::<Vec<_>>();
    if !state_selections.is_empty() {
        return unique_resolved_selection(state_selections.into_iter().map(Some));
    }
    {
        if let Some(selection) = members
            .iter()
            .map(|member| resolved_selection_member_points(member, sketch, entities))
            .collect::<Option<Vec<_>>>()
            .and_then(|member_points| {
                selection_for_member_points(
                    members,
                    sketch,
                    entities,
                    &member_points,
                    tolerance,
                    arrangement_budget,
                )
            })
        {
            return Some(selection);
        }
        let selections = members
            .iter()
            .map(|member| {
                if let Some(points) = resolved_selection_member_points(member, sketch, entities) {
                    selection_containing_points(
                        sketch,
                        entities,
                        &points,
                        tolerance,
                        arrangement_budget,
                    )
                } else {
                    resolved_selection_member_profiles(member, sketch)
                        .map(ResolvedProfileSelection::Loops)
                }
            })
            .collect::<Vec<_>>();
        ordered_unique_profile_selections(selections.iter().cloned())
            .or_else(|| region_with_boundary_selection_members(members, sketch, &selections))
    }
}

fn selection_for_member_points(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    member_points: &[Vec<Point3>],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
) -> Option<ResolvedProfileSelection> {
    let all_points = member_points.iter().flatten().copied().collect::<Vec<_>>();
    if let Some(selection) =
        selection_containing_points(sketch, entities, &all_points, tolerance, arrangement_budget)
    {
        return Some(selection);
    }
    let selections = member_points
        .iter()
        .map(|points| {
            selection_containing_points(sketch, entities, points, tolerance, arrangement_budget)
        })
        .collect::<Vec<_>>();
    ordered_unique_profile_selections(selections.iter().cloned())
        .or_else(|| region_with_boundary_selection_members(members, sketch, &selections))
}

fn region_with_boundary_selection_members(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    selections: &[Option<ResolvedProfileSelection>],
) -> Option<ResolvedProfileSelection> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let regions = selections
        .iter()
        .filter_map(|selection| match selection {
            Some(ResolvedProfileSelection::Regions(regions)) => Some(regions.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [region] = regions.first()? else {
        return None;
    };
    let SketchProfileRegion::Loops { outer, holes } = region else {
        return None;
    };
    if regions
        .iter()
        .any(|candidate| *candidate != std::slice::from_ref(region))
    {
        return None;
    }
    let boundary = std::iter::once(*outer)
        .chain(holes.iter().copied())
        .collect::<HashSet<_>>();
    let member_matches = |member: &DesignExtrudeSelectionMember,
                          selection: &Option<ResolvedProfileSelection>| {
        match selection {
            Some(ResolvedProfileSelection::Regions(candidate)) => {
                candidate == std::slice::from_ref(region)
            }
            Some(ResolvedProfileSelection::Loops(loops)) => {
                !loops.is_empty() && loops.iter().all(|profile| boundary.contains(profile))
            }
            None => resolved_selection_member_profiles(member, sketch).is_some_and(|profiles| {
                !profiles.is_empty() && profiles.iter().all(|profile| boundary.contains(profile))
            }),
        }
    };
    members
        .iter()
        .zip(selections)
        .all(|(member, selection)| member_matches(member, selection))
        .then(|| {
            ResolvedProfileSelection::Regions(vec![SketchProfileRegion::Loops {
                outer: *outer,
                holes: holes.clone(),
            }])
        })
}

fn resolved_selection_member_profiles(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
) -> Option<Vec<u32>> {
    let SketchRelationOperand::Curve {
        primary_id,
        secondary_id,
        ..
    } = member.resolved_geometry.as_ref()?
    else {
        return None;
    };
    let entity = neutral_sketch_curve_id(&sketch.id, *primary_id, *secondary_id);
    sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity))
        .map(|(index, _)| u32::try_from(index).ok())
        .collect::<Option<Vec<_>>>()
}

fn resolved_selection_member_points(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<Vec<Point3>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchRelationOperand::Point {
        record_index,
        persistent_id,
    } = member.resolved_geometry.as_ref()?
    else {
        return None;
    };
    let entity_id = persistent_id.map_or_else(
        || neutral_sketch_record_id(&sketch.id, *record_index),
        |persistent_id| neutral_sketch_point_id(&sketch.id, persistent_id),
    );
    let SketchGeometry::Point { position } = &entities
        .iter()
        .find(|entity| entity.id == entity_id && entity.sketch == sketch.id)?
        .geometry
    else {
        return None;
    };
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let v_axis = normal.cross(u_axis);
    Some(vec![origin
        .translated(u_axis, position.u)
        .translated(v_axis, position.v)])
}

pub(crate) fn ordered_unique_profile_selections(
    matches: impl IntoIterator<Item = Option<ResolvedProfileSelection>>,
) -> Option<ResolvedProfileSelection> {
    let mut loops = Vec::new();
    let mut regions = Vec::new();
    for selection in matches {
        match selection? {
            ResolvedProfileSelection::Loops(selected) if regions.is_empty() => {
                for loop_index in selected {
                    if !loops.contains(&loop_index) {
                        loops.push(loop_index);
                    }
                }
            }
            ResolvedProfileSelection::Regions(selected) if loops.is_empty() => {
                for region in selected {
                    if !regions.contains(&region) {
                        regions.push(region);
                    }
                }
            }
            _ => return None,
        }
    }
    if !loops.is_empty() {
        Some(ResolvedProfileSelection::Loops(loops))
    } else if !regions.is_empty() {
        Some(ResolvedProfileSelection::Regions(regions))
    } else {
        None
    }
}

pub(crate) fn selection_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
) -> Option<ResolvedProfileSelection> {
    let projected = points
        .iter()
        .map(|point| project_to_sketch(sketch, *point))
        .collect::<Option<Vec<_>>>()?;
    let boundaries = sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| {
            projected.iter().all(|point| {
                profile.iter().any(|use_| {
                    entities
                        .iter()
                        .find(|entity| entity.id == use_.entity)
                        .is_some_and(|entity| point_on_sketch_entity(*point, entity, tolerance))
                })
            })
        })
        .map(|(index, _)| u32::try_from(index).ok())
        .collect::<Option<Vec<_>>>()?;
    if let [profile] = boundaries.as_slice() {
        return Some(ResolvedProfileSelection::Loops(vec![*profile]));
    }
    if let Some(region) = arrangement_region_containing_points(
        sketch,
        entities,
        &projected,
        tolerance,
        arrangement_budget,
    ) {
        return Some(ResolvedProfileSelection::Regions(vec![region]));
    }
    if !boundaries.is_empty() {
        return None;
    }
    region_containing_points(sketch, entities, points, tolerance)
        .map(|region| ResolvedProfileSelection::Regions(vec![region]))
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
) -> Option<cadmpeg_ir::features::PathRef> {
    use cadmpeg_ir::features::PathRef;
    use cadmpeg_ir::sketches::SketchGeometry;

    if group.members.is_empty() {
        return None;
    }
    let stream = native_stream(&group.id)?;
    let mut selected_operands = Vec::with_capacity(group.members.len());
    let mut member_records = HashSet::with_capacity(group.members.len());
    let mut primary_identity = None;
    let mut asset_id = None;
    let mut context_id = None;
    for (ordinal, record_index) in group.members.iter().copied().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        if !member_records.insert(record_index) {
            return None;
        }
        let mut matches = resolution.operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index == record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() || operand.secondary_identity.is_none() {
            return None;
        }
        if let Some(expected) = primary_identity {
            if expected != operand.primary_identity {
                return None;
            }
        } else {
            primary_identity = Some(operand.primary_identity);
        }
        if let Some(expected) = asset_id {
            if expected != operand.asset_id.as_str() {
                return None;
            }
        } else {
            asset_id = Some(operand.asset_id.as_str());
        }
        if let Some(expected) = context_id {
            if expected != operand.context_id.as_str() {
                return None;
            }
        } else {
            context_id = Some(operand.context_id.as_str());
        }
        selected_operands.push(operand);
    }
    let primary_identity = primary_identity?;
    let mut matching_placements = resolution.placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream) && placement.entity_suffix == primary_identity
    });
    let placement = matching_placements.next()?;
    if matching_placements.next().is_some() {
        return None;
    }

    let mut curve_ids = Vec::with_capacity(selected_operands.len());
    let mut selected_curve_identities = HashSet::with_capacity(selected_operands.len());
    for operand in &selected_operands {
        let owner_reference = u32::try_from(operand.primary_identity).ok()?;
        let secondary_identity = operand.secondary_identity?;
        let mut curves = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && curve.primary_id == secondary_identity
                && operand
                    .curve_secondary_identity
                    .is_none_or(|secondary| curve.secondary_id == secondary)
        });
        let curve = curves.next()?;
        if curves.next().is_some()
            || !selected_curve_identities.insert((curve.primary_id, curve.secondary_id))
        {
            return None;
        }
        curve_ids.push((curve.primary_id, curve.secondary_id));
    }

    let spatial_sketch = neutral_spatial_sketch_id(placement);
    if resolution
        .spatial_sketches
        .iter()
        .any(|sketch| sketch.id == spatial_sketch)
    {
        let selections = curve_ids
            .iter()
            .map(|(primary, secondary)| {
                neutral_spatial_sketch_curve_id(&spatial_sketch, *primary, *secondary)
            })
            .collect::<HashSet<_>>();
        if selections.len() != curve_ids.len()
            || selections.iter().any(|curve| {
                !resolution
                    .spatial_sketch_entities
                    .iter()
                    .any(|entity| entity.sketch == spatial_sketch && entity.id == *curve)
            })
        {
            return None;
        }
        return Some(PathRef::SpatialSketchCurves {
            sketch: spatial_sketch.clone(),
            curves: curve_ids
                .into_iter()
                .map(|(primary, secondary)| {
                    neutral_spatial_sketch_curve_id(&spatial_sketch, primary, secondary)
                })
                .collect(),
        });
    }

    let sketch = neutral_sketch_id(placement);
    if !resolution
        .sketches
        .iter()
        .any(|candidate| candidate.id == sketch)
    {
        return None;
    }
    let curves = curve_ids
        .into_iter()
        .map(|(primary, secondary)| neutral_sketch_curve_id(&sketch, primary, secondary))
        .collect::<Vec<_>>();
    if curves.iter().any(|curve| {
        !resolution.sketch_entities.iter().any(|entity| {
            entity.sketch == sketch
                && entity.id == *curve
                && !matches!(entity.geometry, SketchGeometry::Point { .. })
        })
    }) {
        return None;
    }
    Some(PathRef::SketchCurves { sketch, curves })
}

/// Resolve one ordered Loft guide or centerline group whose members select
/// curves from a Sketch.
fn resolved_loft_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &SketchProfileResolution<'_>,
) -> Option<cadmpeg_ir::features::PathRef> {
    if !matches!(group.role, 0x5_0000_0000 | 0x7_0000_0000) {
        return None;
    }
    let path_resolution = resolution.path_resolution();
    resolve_entity_selection_path(group, &path_resolution)
}

fn spatial_profile_member_entity<'a>(
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    curve_identities: &[SketchCurveIdentity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Option<&'a cadmpeg_ir::sketches::SpatialSketchEntity> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id == member.curve_primary_id
    });
    let curve = curves.next()?;
    if curves.next().is_some() {
        return None;
    }
    let entity_id =
        neutral_spatial_sketch_curve_id(&spatial_sketch.id, curve.primary_id, curve.secondary_id);
    let mut entities = spatial_entities
        .iter()
        .filter(|entity| entity.sketch == spatial_sketch.id && entity.id == entity_id);
    let entity = entities.next()?;
    entities.next().is_none().then_some(entity)
}

fn sketch_profile_member_entity(
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchEntityId> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id == member.curve_primary_id
    });
    let curve = curves.next()?;
    if curves.next().is_some() {
        return None;
    }
    let entity_id = neutral_sketch_curve_id(&sketch.id, curve.primary_id, curve.secondary_id);
    let mut entities = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && entity.id == entity_id);
    let entity = entities.next()?;
    if entities.next().is_some() {
        return None;
    }
    Some(entity.id.clone())
}

fn resolved_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<Vec<u32>> {
    let selection = profile.region_selection.as_ref()?;
    let owner_reference = u32::try_from(profile.entity_suffix).ok()?;
    let mut resolved = Vec::with_capacity(selection.regions.len());
    for region in &selection.regions {
        let first = sketch_profile_member_entity(
            stream,
            owner_reference,
            region.members.first()?,
            sketch,
            curve_identities,
            sketch_entities,
        )?;
        let mut matching_profiles = sketch
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == first));
        let (profile_index, selected_profile) = matching_profiles.next()?;
        if matching_profiles.next().is_some() {
            return None;
        }
        for member in &region.members[1..] {
            let entity = sketch_profile_member_entity(
                stream,
                owner_reference,
                member,
                sketch,
                curve_identities,
                sketch_entities,
            )?;
            if !selected_profile.iter().any(|use_| use_.entity == entity) {
                return None;
            }
        }
        let profile_index = u32::try_from(profile_index).ok()?;
        if resolved.contains(&profile_index) {
            return None;
        }
        resolved.push(profile_index);
    }
    (!resolved.is_empty()).then_some(resolved)
}

fn coincident_spatial_profile_geometry(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;

    let (
        SpatialSketchGeometry::Circle {
            center: first_center,
            normal: first_normal,
            radius: first_radius,
            ..
        },
        SpatialSketchGeometry::Circle {
            center: second_center,
            normal: second_normal,
            radius: second_radius,
            ..
        },
    ) = (first, second)
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
    let Some((first_normal, second_normal)) = first_normal.unit().zip(second_normal.unit()) else {
        return false;
    };
    let normal_angle = first_normal.dot(second_normal).abs().clamp(0.0, 1.0).acos();
    center_delta.x * center_delta.x
        + center_delta.y * center_delta.y
        + center_delta.z * center_delta.z
        <= linear_tolerance * linear_tolerance
        && (first_radius.0 - second_radius.0).abs() <= linear_tolerance
        && normal_angle <= angular_tolerance
}

fn resolved_spatial_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    resolution: &SketchProfileResolution<'_>,
) -> Option<Vec<u32>> {
    let Some(selection) = profile.region_selection.as_ref() else {
        if spatial_sketch.profiles.is_empty() {
            return None;
        }
        return (0..spatial_sketch.profiles.len())
            .map(u32::try_from)
            .collect::<Result<Vec<_>, _>>()
            .ok();
    };
    let owner_reference = u32::try_from(profile.entity_suffix).ok()?;
    let mut resolved = Vec::with_capacity(selection.regions.len());
    for region in &selection.regions {
        let first = spatial_profile_member_entity(
            stream,
            owner_reference,
            region.members.first()?,
            spatial_sketch,
            resolution.curve_identities,
            resolution.spatial_sketch_entities,
        )?;
        let mut matching_profiles =
            spatial_sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate
                        .boundary
                        .iter()
                        .any(|use_| use_.entity == first.id)
                });
        let (profile_index, selected_profile) = matching_profiles.next()?;
        if matching_profiles.next().is_some() {
            return None;
        }
        for member in &region.members[1..] {
            let entity = spatial_profile_member_entity(
                stream,
                owner_reference,
                member,
                spatial_sketch,
                resolution.curve_identities,
                resolution.spatial_sketch_entities,
            )?;
            if selected_profile
                .boundary
                .iter()
                .any(|use_| use_.entity == entity.id)
            {
                continue;
            }
            let coincident = selected_profile.boundary.iter().any(|use_| {
                resolution
                    .spatial_sketch_entities
                    .iter()
                    .find(|candidate| {
                        candidate.sketch == spatial_sketch.id && candidate.id == use_.entity
                    })
                    .is_some_and(|candidate| {
                        coincident_spatial_profile_geometry(
                            &candidate.geometry,
                            &entity.geometry,
                            resolution.linear_tolerance,
                            resolution.angular_tolerance,
                        )
                    })
            });
            if !coincident {
                return None;
            }
        }
        let profile_index = u32::try_from(profile_index).ok()?;
        if resolved.contains(&profile_index) {
            return None;
        }
        resolved.push(profile_index);
    }
    (!resolved.is_empty()).then_some(resolved)
}

fn spatial_profile_containing_entity(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntityId,
) -> Option<u32> {
    let mut profiles = sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.boundary.iter().any(|use_| use_.entity == *entity));
    let (index, _) = profiles.next()?;
    if profiles.next().is_some() {
        return None;
    }
    u32::try_from(index).ok()
}

pub(crate) fn bind_loft_and_revolve_sketch_selections(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    resolution: &SketchProfileResolution<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, LoftSection, PathRef, ProfileRef};

    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut resolved_profiles = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(group.role, 0x41_0000_0000 | 0x43_0000_0000) && group.members.len() == 1
    }) {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(header) = headers.get(&(stream, group.members[0])) else {
            continue;
        };
        let Some(profile) = parse_sketch_profile(
            bytes,
            stream,
            group.scope_reference_ordinal,
            header,
            resolution.entities,
        ) else {
            continue;
        };
        let matches = resolution
            .placements
            .iter()
            .filter(|placement| {
                native_stream(&placement.id) == Some(stream)
                    && placement.entity_id == profile.entity_id
            })
            .collect::<Vec<_>>();
        let [placement] = matches.as_slice() else {
            continue;
        };
        let spatial_sketch_id = neutral_spatial_sketch_id(placement);
        let resolved = if let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        {
            resolved_spatial_sketch_profile_regions(stream, &profile, spatial_sketch, resolution)
                .map_or_else(
                    || ProfileRef::SpatialSketchSelection {
                        sketch: spatial_sketch_id.clone(),
                        selections: vec![group.id.clone()],
                    },
                    |profiles| ProfileRef::SpatialSketchProfiles {
                        sketch: spatial_sketch_id.clone(),
                        profiles,
                    },
                )
        } else {
            let sketch = neutral_sketch_id(placement);
            if !resolution
                .sketches
                .iter()
                .any(|candidate| candidate.id == sketch)
            {
                continue;
            }
            ProfileRef::Sketch(sketch)
        };
        resolved_profiles.insert(group.id.clone(), resolved);
    }
    let mut resolved_entity_paths = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(group.role, 0x5_0000_0000 | 0x7_0000_0000) && !group.members.is_empty()
    }) {
        if let Some(path) = resolved_loft_entity_selection_path(group, resolution) {
            resolved_entity_paths.insert(group.id.clone(), path);
        }
    }
    for group in groups
        .iter()
        .filter(|group| group.role == 0x5_0000_0000 && group.members.len() == 1)
    {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut operands = resolution
            .entity_selection_operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index == group.members[0]
            });
        let Some(operand) = operands.next() else {
            continue;
        };
        if operands.next().is_some() {
            continue;
        }
        let mut matching_placements = resolution.placements.iter().filter(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_suffix == operand.primary_identity
        });
        let Some(placement) = matching_placements.next() else {
            continue;
        };
        if matching_placements.next().is_some() {
            continue;
        }
        let spatial_sketch_id = neutral_spatial_sketch_id(placement);
        let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        else {
            continue;
        };
        let Ok(owner_reference) = u32::try_from(operand.primary_identity) else {
            continue;
        };
        let mut geometry_matches = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && entity_selection_matches_curve(operand, curve)
        });
        let Some(curve) = geometry_matches.next() else {
            continue;
        };
        if geometry_matches.next().is_some() {
            continue;
        }
        let entity = neutral_spatial_sketch_curve_id(
            &spatial_sketch_id,
            curve.primary_id,
            curve.secondary_id,
        );
        let profile = spatial_profile_containing_entity(spatial_sketch, &entity);
        resolved_profiles.insert(
            group.id.clone(),
            profile.map_or_else(
                || ProfileRef::SpatialSketchSelection {
                    sketch: spatial_sketch_id.clone(),
                    selections: vec![operand.id.clone()],
                },
                |profile| ProfileRef::SpatialSketchProfiles {
                    sketch: spatial_sketch_id.clone(),
                    profiles: vec![profile],
                },
            ),
        );
    }
    for feature in features.iter_mut() {
        let FeatureDefinition::Loft {
            sections,
            guides,
            centerline,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        for section in sections.iter_mut() {
            let LoftSection::Profile(ProfileRef::Native(native)) = section else {
                continue;
            };
            if let Some(profile) = resolved_profiles.get(native) {
                *section = LoftSection::Profile(profile.clone());
            }
        }
        for guide in guides.iter_mut() {
            let PathRef::Native(native) = guide else {
                continue;
            };
            if let Some(path) = resolved_entity_paths.get(native) {
                *guide = path.clone();
            }
        }
        if let Some(PathRef::Native(native)) = centerline.as_ref() {
            if let Some(path) = resolved_entity_paths.get(native) {
                *centerline = Some(path.clone());
            }
        }
    }
    for feature in features.iter_mut() {
        let FeatureDefinition::Revolve { construction, .. } = &mut feature.definition else {
            continue;
        };
        let Some(ProfileRef::Native(native)) = construction.profile.as_ref() else {
            continue;
        };
        let Some(profile) = resolved_profiles.get(native) else {
            continue;
        };
        construction.profile = Some(profile.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
