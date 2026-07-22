// SPDX-License-Identifier: Apache-2.0
//! Resolve extrude profile selections against sketch regions.

use crate::container::{role, ContainerScan};
use crate::design::decode::operands::parse_sketch_profile;
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::feature_project::spatial_sketch_entity_endpoints;
use crate::design::geometry::{
    arrangement_region_containing_points, historical_member_points_in_state, point_in_polygon,
    point_on_sketch_entity, point_segment_distance, project_to_sketch, region_containing_points,
};
use crate::ids::{
    self, native_stream, neutral_sketch_curve_id, neutral_sketch_id, neutral_sketch_point_id,
    neutral_spatial_sketch_id,
};
use crate::records::{
    DesignConstructionOperandGroup, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignParameterScope,
    DesignRecordHeader, DesignSketchPlacement, SketchCurveIdentity, SketchRelationOperand,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub histories: &'a [crate::history_records::AsmHistory],
    pub linear_tolerance: f64,
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
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
        let mut matching_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        matching_groups.sort_by_key(|group| group.scope_reference_ordinal);
        if matching_groups.is_empty() {
            continue;
        }
        let FeatureDefinition::Extrude { profile, .. } = &mut feature.definition else {
            continue;
        };
        if matches!(profile, ProfileRef::Native(_)) {
            if let Some(selection) = historical_face_profile_selection(
                &matching_groups,
                members,
                resolution.histories,
                scope.previous_history_state_id,
                &feature.id,
            ) {
                *profile = selection;
            }
            continue;
        }
        let ProfileRef::Sketch(sketch_id) = profile else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
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
                            spatial_sketch,
                            resolution.spatial_entities,
                            resolution,
                            scope.history_state_id,
                            scope.previous_history_state_id,
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
        let selections = matching_groups
            .iter()
            .map(|group| {
                resolved_extrude_profile_selection(
                    sketch_id,
                    group,
                    members,
                    sketch,
                    resolution,
                    scope.history_state_id,
                    scope.previous_history_state_id,
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

fn historical_face_profile_selection(
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    histories: &[crate::history_records::AsmHistory],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let previous_state_id = previous_state_id?;
    let mut states = histories
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
    resolution: ExtrudeProfileResolution<'_>,
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
                )
            })?
        })
        .or_else(|| {
            transition_profile_selection(
                sketch,
                resolution.entities,
                resolution.histories,
                history_state_id?,
                previous_history_state_id?,
                resolution.linear_tolerance,
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
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
) -> Option<ResolvedProfileSelection> {
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
    let inserted_faces = &state.transition.as_ref()?.topology.faces.inserted;
    let tolerance = linear_tolerance.max(1.0e-7);
    let inserted = transition_inserted_profile_selection(inserted_faces.iter().map(|face| {
        let points = historical_face_points(*face, topology)?;
        selection_containing_points(sketch, entities, &points, tolerance)
    }));
    if inserted.is_some() {
        return inserted;
    }
    let mut previous_states = histories
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
        selection_containing_points(sketch, entities, &points, tolerance)
    }))
}

fn resolved_spatial_extrude_profile_selection(
    _group: &DesignExtrudeSelectionGroup,
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    resolution: ExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Option<u32> {
    transition_spatial_profile_selection(
        sketch,
        entities,
        resolution.histories,
        history_state_id?,
        previous_history_state_id?,
        resolution.linear_tolerance,
    )
    .or_else(|| (sketch.profiles.len() == 1).then_some(0))
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
    let tolerance = linear_tolerance.max(1.0e-7);
    let unique = |faces: &[i64], topology: &crate::history_records::AsmHistoricalTopology| {
        let mut indices = faces
            .iter()
            .filter_map(|face| {
                if let Some(profile) =
                    spatial_polyline_profile_for_face(*face, topology, sketch, entities, tolerance)
                {
                    return Some(profile);
                }
                let points = historical_face_points(*face, topology)?;
                spatial_profile_containing_points(sketch, entities, &points, tolerance)
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

fn spatial_polyline_profile_for_face(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    tolerance: f64,
) -> Option<u32> {
    let face_loops = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)?;
    let mut loop_signatures = Vec::new();
    for loop_ref in &face_loops.member_refs {
        let coedges = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)?;
        let mut lengths = coedges
            .member_refs
            .iter()
            .map(|coedge_ref| {
                let coedge = topology
                    .coedge_topology
                    .iter()
                    .find(|coedge| coedge.coedge == *coedge_ref)?;
                let edge = topology
                    .edge_vertices
                    .iter()
                    .find(|edge| edge.edge == coedge.edge)?;
                let position = |vertex| {
                    let point = topology
                        .vertex_points
                        .iter()
                        .find(|binding| binding.entity == vertex)?
                        .carrier;
                    topology
                        .point_positions
                        .iter()
                        .find(|candidate| candidate.point == point)
                        .map(|point| point.position)
                };
                let start = position(edge.start_vertex)?;
                let end = position(edge.end_vertex)?;
                Some(
                    (end.x - start.x)
                        .hypot(end.y - start.y)
                        .hypot(end.z - start.z),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        lengths.sort_by(f64::total_cmp);
        loop_signatures.push(lengths);
    }
    let mut matches = sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            let mut lengths = profile
                .boundary
                .iter()
                .map(|use_| {
                    let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
                    let cadmpeg_ir::sketches::SpatialSketchGeometry::Line { start, end } =
                        entity.geometry
                    else {
                        return None;
                    };
                    Some(
                        (end.x - start.x)
                            .hypot(end.y - start.y)
                            .hypot(end.z - start.z),
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            lengths.sort_by(f64::total_cmp);
            loop_signatures
                .iter()
                .any(|signature| {
                    signature.len() == lengths.len()
                        && signature.iter().zip(&lengths).all(|(historical, profile)| {
                            (historical - profile).abs()
                                <= tolerance * (1.0 + historical.abs().max(profile.abs()))
                        })
                })
                .then(|| u32::try_from(index).ok())?
        });
    let selected = matches.next()?;
    matches.next().is_none().then_some(selected)
}

fn spatial_profile_containing_points(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<u32> {
    let mut matches = sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            let offsets = points
                .iter()
                .map(|point| {
                    (point.x - profile.origin.x) * profile.normal.x
                        + (point.y - profile.origin.y) * profile.normal.y
                        + (point.z - profile.origin.z) * profile.normal.z
                })
                .collect::<Vec<_>>();
            if !offsets.first().is_some_and(|first| {
                offsets
                    .iter()
                    .all(|offset| (offset - first).abs() <= tolerance)
            }) {
                return None;
            }
            let v_axis = Vector3::new(
                profile.normal.y * profile.u_axis.z - profile.normal.z * profile.u_axis.y,
                profile.normal.z * profile.u_axis.x - profile.normal.x * profile.u_axis.z,
                profile.normal.x * profile.u_axis.y - profile.normal.y * profile.u_axis.x,
            );
            let project = |point: Point3| {
                let offset = Vector3::new(
                    point.x - profile.origin.x,
                    point.y - profile.origin.y,
                    point.z - profile.origin.z,
                );
                Point2::new(
                    offset.x * profile.u_axis.x
                        + offset.y * profile.u_axis.y
                        + offset.z * profile.u_axis.z,
                    offset.x * v_axis.x + offset.y * v_axis.y + offset.z * v_axis.z,
                )
            };
            let polygon = profile
                .boundary
                .iter()
                .map(|use_| {
                    let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
                    let endpoints = spatial_sketch_entity_endpoints(entity)?;
                    Some(project(endpoints[usize::from(use_.reversed)]))
                })
                .collect::<Option<Vec<_>>>()?;
            (polygon.len() >= 3
                && points.iter().all(|point| {
                    let point = project(*point);
                    point_in_polygon(point, &polygon)
                        || polygon.iter().enumerate().any(|(index, start)| {
                            let end = polygon[(index + 1) % polygon.len()];
                            point_segment_distance(point, (*start, end)) <= tolerance
                        })
                }))
            .then(|| u32::try_from(index).ok())?
        });
    let selected = matches.next()?;
    matches.next().is_none().then_some(selected)
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
) -> Option<ResolvedProfileSelection> {
    let tolerance = linear_tolerance.max(1.0e-7);
    let mut states = HashMap::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    let mut state_ids = members
        .iter()
        .flat_map(|member| member.historical_state_ids.iter().copied())
        .collect::<Vec<_>>();
    state_ids.sort_unstable();
    state_ids.dedup();
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
            selection_for_member_points(members, sketch, entities, &member_points, tolerance)
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
                selection_for_member_points(members, sketch, entities, &member_points, tolerance)
            })
        {
            return Some(selection);
        }
        let selections = members
            .iter()
            .map(|member| {
                if let Some(points) = resolved_selection_member_points(member, sketch, entities) {
                    selection_containing_points(sketch, entities, &points, tolerance)
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
) -> Option<ResolvedProfileSelection> {
    let all_points = member_points.iter().flatten().copied().collect::<Vec<_>>();
    if let Some(selection) = selection_containing_points(sketch, entities, &all_points, tolerance) {
        return Some(selection);
    }
    let selections = member_points
        .iter()
        .map(|points| selection_containing_points(sketch, entities, points, tolerance))
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

    let SketchRelationOperand::Point { persistent_id, .. } = member.resolved_geometry.as_ref()?
    else {
        return None;
    };
    let entity_id = neutral_sketch_point_id(&sketch.id, *persistent_id);
    let SketchGeometry::Point { position } = &entities
        .iter()
        .find(|entity| entity.id == entity_id && entity.sketch == sketch.id)?
        .geometry
    else {
        return None;
    };
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let v_axis = Vector3::new(
        normal.y * u_axis.z - normal.z * u_axis.y,
        normal.z * u_axis.x - normal.x * u_axis.z,
        normal.x * u_axis.y - normal.y * u_axis.x,
    );
    Some(vec![Point3::new(
        origin.x + position.u * u_axis.x + position.v * v_axis.x,
        origin.y + position.u * u_axis.y + position.v * v_axis.y,
        origin.z + position.u * u_axis.z + position.v * v_axis.z,
    )])
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
    if let Some(region) =
        arrangement_region_containing_points(sketch, entities, &projected, tolerance)
    {
        return Some(ResolvedProfileSelection::Regions(vec![region]));
    }
    if !boundaries.is_empty() {
        return None;
    }
    region_containing_points(sketch, entities, points, tolerance)
        .map(|region| ResolvedProfileSelection::Regions(vec![region]))
}

/// Solved sketch records used to bind Loft section and guide selections.
pub(crate) struct LoftSketchResolution<'a> {
    pub(crate) entities: &'a [DesignEntityHeader],
    pub(crate) entity_selection_operands: &'a [DesignEntitySelectionOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
}

pub(crate) fn bind_loft_sketch_selections(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    resolution: &LoftSketchResolution<'_>,
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
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
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
                    && !resolution
                        .spatial_sketches
                        .iter()
                        .any(|sketch| sketch.id == neutral_spatial_sketch_id(placement))
            })
            .collect::<Vec<_>>();
        let [placement] = matches.as_slice() else {
            continue;
        };
        resolved_profiles.insert(
            group.id.clone(),
            ProfileRef::Sketch(neutral_sketch_id(placement)),
        );
    }
    let mut resolved_spatial_paths = HashMap::new();
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
        let spatial_sketch = neutral_spatial_sketch_id(placement);
        if !resolution
            .spatial_sketches
            .iter()
            .any(|sketch| sketch.id == spatial_sketch)
        {
            continue;
        }
        let Ok(owner_reference) = u32::try_from(operand.primary_identity) else {
            continue;
        };
        let geometry_matches = resolution
            .curve_identities
            .iter()
            .filter(|curve| {
                native_stream(&curve.id) == Some(stream)
                    && curve.owner_reference == Some(owner_reference)
                    && curve.primary_id == operand.secondary_identity
            })
            .count();
        if geometry_matches != 1 {
            continue;
        }
        let selections = vec![operand.id.clone()];
        resolved_profiles.insert(
            group.id.clone(),
            ProfileRef::SpatialSketchSelection {
                sketch: spatial_sketch.clone(),
                selections: selections.clone(),
            },
        );
        resolved_spatial_paths.insert(
            group.id.clone(),
            PathRef::SpatialSketchSelection {
                sketch: spatial_sketch,
                selections,
            },
        );
    }
    for feature in features {
        let FeatureDefinition::Loft {
            sections, guides, ..
        } = &mut feature.definition
        else {
            continue;
        };
        for section in sections {
            let LoftSection::Profile(ProfileRef::Native(native)) = section else {
                continue;
            };
            if let Some(profile) = resolved_profiles.get(native) {
                *section = LoftSection::Profile(profile.clone());
            }
        }
        for guide in guides {
            let PathRef::Native(native) = guide else {
                continue;
            };
            if let Some(path) = resolved_spatial_paths.get(native) {
                *guide = path.clone();
            }
        }
    }
    Ok(())
}
