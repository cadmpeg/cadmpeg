//! Hole construction, bore topology and hole axis projection.

use super::compact_reference_planes::{
    compact_profile_component_plane_frame, compact_profile_reference_plane_source,
    CompactReferencePlaneIndex,
};
use super::curves::{lane_sketch_plane_frames, SketchPlaneFrame, SketchPlaneUAxisSource};
use super::helix::fit_helix_polyline;
use super::reference_geometry::{explicit_reference_plane_frame, reference_plane_frame_key};
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::transforms::{quantize, sketch_frame_marker_transform};
use super::{is_class_token, CLASS_MARKER};
use crate::classification::{classify, FeatureClass};
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputScalarRole,
    SketchInputKind,
};
use cadmpeg_core::decode::{alloc_filled, View};
use cadmpeg_ir::features::{
    Angle, FeatureDefinition, HoleBottom, HoleKind, HolePlacement, Length, Termination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketch,
    SpatialSketchEntity, SpatialSketchGeometry,
};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};
use std::collections::{HashMap, HashSet};

const EPS_HOLE_POSITION: f64 = 1.0e-8;
const EPS_HOLE_GEOMETRY: f64 = 1.0e-9;
const EPS_HOLE_DEGENERATE_NORMAL: f64 = 1.0e-10;
const EPS_HOLE_EXACT_GEOMETRY: f64 = 1.0e-12;

#[cfg(test)]
use super::parameters::enrich_history_parameters;

/// Resolve helix placement from the counted curve mesh stored in its feature
/// object. Promotion requires one mesh stream and a circular-helix fit whose
/// residual is small relative to its radius.
pub(crate) fn project_helix_axes(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let records = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for model_feature in model_features {
        let FeatureDefinition::HelixNativeAxis {
            axial_rise,
            revolutions,
            start_angle,
            clockwise,
            ..
        } = &model_feature.definition
        else {
            continue;
        };
        let Some(native_ref) = model_feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = records.get(native_ref).copied() else {
            continue;
        };
        let mut meshes = Vec::new();
        for lane in lanes {
            let Some(name) = feature_object_name(record, lane) else {
                continue;
            };
            let start = usize::try_from(name.offset).ok();
            let end = histories
                .iter()
                .flat_map(|history| &history.features)
                .filter_map(|feature| feature_object_name(feature, lane))
                .filter(|candidate| candidate.offset > name.offset)
                .map(|candidate| candidate.offset)
                .min()
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = start.and_then(|start| lane.native_payload.get(start..end)) else {
                continue;
            };
            meshes.extend(
                crate::parasolid::extract_streams_with_offsets(object)
                    .into_iter()
                    .filter_map(|stream| {
                        crate::parasolid::mesh_polyline_from_header(&stream.payload, &stream.header)
                    }),
            );
        }
        let [points] = meshes.as_slice() else {
            continue;
        };
        let Some((axis_origin, mut axis_direction, radius, fitted_rise)) =
            fit_helix_polyline(points, *revolutions, *clockwise)
        else {
            continue;
        };
        if fitted_rise * axial_rise.0 < 0.0 {
            axis_direction = Vector3::new(-axis_direction.x, -axis_direction.y, -axis_direction.z);
        }
        let Some(last_point) = points.last() else {
            continue;
        };
        let signed_rise = Vector3::new(
            last_point.x - points[0].x,
            last_point.y - points[0].y,
            last_point.z - points[0].z,
        )
        .dot(axis_direction);
        let Some(pitch) = cadmpeg_ir::features::HelixPitch::new(Length(signed_rise / *revolutions))
        else {
            continue;
        };
        model_feature.definition = FeatureDefinition::Helix {
            axis_origin,
            axis_direction,
            radius: Length(radius),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical { pitch },
            revolutions: *revolutions,
            start_angle: *start_angle,
            clockwise: *clockwise,
            segment_turns: None,
            construction_style: None,
        };
    }
}

fn hole_position_sketch_source(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
) -> Option<u32> {
    if classify(feature) != Some(FeatureClass::Hole) {
        return None;
    }
    let name = feature_object_name(feature, lane)?;
    // Legacy keyword records may omit the XML source id while the serialized
    // object name still carries the stable object id used by the input lane.
    let source = feature
        .source_id
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
        .or(name.object_id)?;
    let offset = usize::try_from(name.offset)
        .ok()?
        .checked_add(6 + name.value.encode_utf16().count().checked_mul(2)?)?;
    if lane.native_payload.get(offset..offset + 8)
        != Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40])
        || lane.native_payload.get(offset + 8..offset + 12) != Some(&source.to_le_bytes())
        || lane.native_payload.get(offset + 12..offset + 16) != Some(&[0x00; 4])
    {
        return None;
    }
    let body_start = offset + 16;
    let body_end = offset + 144;
    let body = lane.native_payload.get(body_start..body_end)?;
    let mut sources = body
        .windows(12)
        .filter_map(|bytes| {
            (bytes[..2] == [0x00, 0xc0]
                && (bytes[6..12] == [0; 6] || bytes[6..12] == [0, 0, 0, 0, 0xff, 0xfe]))
            .then(|| View::u32_le_at(bytes, 2))
            .flatten()
            .filter(|source| *source != 0 && *source != u32::MAX)
        })
        .collect::<HashSet<_>>();

    sources.extend(lane.names.iter().filter_map(|child| {
        let child_offset = usize::try_from(child.offset).ok()?;
        if !(body_start..body_end).contains(&child_offset) {
            return None;
        }
        let child_source = child.object_id?;
        let trailer =
            child_offset.checked_add(6 + child.value.encode_utf16().count().checked_mul(2)?)?;
        if trailer.checked_add(12)? > body_end {
            return None;
        }
        (lane.native_payload.get(trailer..trailer + 8)
            == Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40])
            && lane.native_payload.get(trailer + 8..trailer + 12)
                == Some(&child_source.to_le_bytes())
            && child_source != 0
            && child_source != u32::MAX)
            .then_some(child_source)
    }));
    let mut sources = sources.into_iter();
    let source = sources.next()?;
    if sources.next().is_some() {
        return None;
    }
    Some(source)
}

pub(crate) fn enrich_history_hole_constructions(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for history in histories {
        let additions = history
            .features
            .iter()
            .enumerate()
            .filter(|(_, feature)| {
                classify(feature) == Some(FeatureClass::Hole)
                    && !feature.properties.contains_key("DissectableChildren")
            })
            .filter_map(|(feature_index, feature)| {
                let profile_from_position_source = || {
                    let mut position_sources = lanes
                        .iter()
                        .filter_map(|lane| hole_position_sketch_source(feature, lane))
                        .collect::<Vec<_>>();
                    position_sources.sort_unstable();
                    position_sources.dedup();
                    let [position_source] = position_sources.as_slice() else {
                        return None;
                    };
                    let unique_position = || {
                        let mut positions = history.features.iter().filter(|candidate| {
                            (candidate
                                .source_id
                                .as_deref()
                                .and_then(|source| source.parse::<u32>().ok())
                                == Some(*position_source)
                                || candidate.ordinal == *position_source)
                                && classify(candidate) == Some(FeatureClass::Sketch)
                        });
                        let position = positions.next()?;
                        positions.next().is_none().then_some(position)
                    };
                    // Legacy holes serialize the generated axial profile
                    // immediately after their inline position-sketch object.
                    let serialized_successor_profile = || {
                        let position = unique_position()?;
                        let mut profiles = Vec::new();
                        for lane in lanes.iter().filter(|lane| {
                            hole_position_sketch_source(feature, lane) == Some(*position_source)
                        }) {
                            let position_offset = feature_object_name(position, lane)?.offset;
                            let minimum_offset = history
                                .features
                                .iter()
                                .filter_map(|candidate| {
                                    let offset = feature_object_name(candidate, lane)?.offset;
                                    (offset > position_offset).then_some(offset)
                                })
                                .min()?;
                            let mut successors = history.features.iter().filter(|candidate| {
                                feature_object_name(candidate, lane)
                                    .is_some_and(|name| name.offset == minimum_offset)
                            });
                            let successor = successors.next()?;
                            if successors.next().is_some()
                                || classify(successor) != Some(FeatureClass::Sketch)
                                || !crate::history::is_hole_profile_construction(successor)
                            {
                                return None;
                            }
                            profiles.push(successor);
                        }
                        profiles.sort_by_key(|profile| profile.id.as_str());
                        profiles.dedup_by_key(|profile| profile.id.as_str());
                        let [profile] = profiles.as_slice() else {
                            return None;
                        };
                        Some((*profile, 4_u8))
                    };
                    if let Some(profile) = serialized_successor_profile() {
                        return Some(profile);
                    }
                    let adjacent_sources = [
                        position_source.checked_sub(1),
                        position_source.checked_add(1),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<HashSet<_>>();
                    let source_profile = || {
                        let mut profiles = history.features.iter().filter(|candidate| {
                            candidate
                                .source_id
                                .as_deref()
                                .and_then(|source| source.parse::<u32>().ok())
                                .is_some_and(|source| adjacent_sources.contains(&source))
                                && classify(candidate) == Some(FeatureClass::Sketch)
                                && crate::history::is_hole_profile_construction(candidate)
                        });
                        let profile = profiles.next()?;
                        profiles.next().is_none().then_some(profile)
                    };
                    if let Some(profile) = source_profile() {
                        return Some((profile, 3_u8));
                    }
                    let hole_source = feature
                        .source_id
                        .as_deref()
                        .and_then(|source| source.parse::<u32>().ok())?;
                    let (lower, upper) = if hole_source < *position_source {
                        (hole_source, *position_source)
                    } else {
                        (*position_source, hole_source)
                    };
                    let mut bounded_profiles = history.features.iter().filter(|candidate| {
                        candidate
                            .source_id
                            .as_deref()
                            .and_then(|source| source.parse::<u32>().ok())
                            .is_some_and(|source| lower < source && source < upper)
                            && classify(candidate) == Some(FeatureClass::Sketch)
                            && crate::history::is_hole_profile_construction(candidate)
                    });
                    let bounded_profile = bounded_profiles.next();
                    if bounded_profiles.next().is_some() {
                        return None;
                    }
                    if let Some(profile) = bounded_profile {
                        return Some((profile, 2_u8));
                    }
                    let position = unique_position()?;
                    let adjacent_ordinals = [
                        position.ordinal.checked_sub(1),
                        position.ordinal.checked_add(1),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<HashSet<_>>();
                    let mut profiles = history.features.iter().filter(|candidate| {
                        adjacent_ordinals.contains(&candidate.ordinal)
                            && candidate.id != position.id
                            && classify(candidate) == Some(FeatureClass::Sketch)
                            && crate::history::is_hole_profile_construction(candidate)
                    });
                    let profile = profiles.next()?;
                    profiles.next().is_none().then_some((profile, 1_u8))
                };
                let profile_from_child_order = || {
                    let child_ordinals = [
                        feature.ordinal.checked_add(1)?,
                        feature.ordinal.checked_add(2)?,
                    ];
                    let children = child_ordinals
                        .iter()
                        .filter_map(|ordinal| {
                            let mut children = history
                                .features
                                .iter()
                                .filter(|child| child.ordinal == *ordinal);
                            let child = children.next()?;
                            children.next().is_none().then_some(child)
                        })
                        .collect::<Vec<_>>();
                    if children.len() != child_ordinals.len()
                        || children
                            .iter()
                            .any(|child| classify(child) != Some(FeatureClass::Sketch))
                    {
                        return None;
                    }
                    let mut profiles = children
                        .into_iter()
                        .filter(|child| crate::history::is_hole_profile_construction(child));
                    let profile = profiles.next()?;
                    profiles.next().is_none().then_some((profile, 1_u8))
                };
                profile_from_position_source()
                    .or_else(profile_from_child_order)
                    .map(|(profile, rank)| {
                        (
                            feature_index,
                            profile
                                .source_id
                                .clone()
                                .unwrap_or_else(|| profile.id.clone()),
                            rank,
                        )
                    })
            })
            .collect::<Vec<_>>();
        let claimed_profiles = history
            .features
            .iter()
            .filter_map(|feature| feature.properties.get("DissectableChildren"))
            .flat_map(|children| children.split(',').map(str::trim))
            .filter(|child| !child.is_empty())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let profile_claim_ranks = additions.iter().fold(
            HashMap::<String, (u8, usize)>::new(),
            |mut ranks, (_, profile, rank)| {
                let entry = ranks.entry(profile.clone()).or_default();
                match rank.cmp(&entry.0) {
                    std::cmp::Ordering::Greater => *entry = (*rank, 1),
                    std::cmp::Ordering::Equal => entry.1 += 1,
                    std::cmp::Ordering::Less => {}
                }
                ranks
            },
        );
        for (feature_index, profile_source, rank) in additions {
            if claimed_profiles.contains(profile_source.as_str())
                || profile_claim_ranks.get(profile_source.as_str()) != Some(&(rank, 1))
            {
                continue;
            }
            history.features[feature_index]
                .properties
                .insert("DissectableChildren".into(), profile_source);
        }
        let claimed_profiles = history
            .features
            .iter()
            .filter_map(|feature| feature.properties.get("DissectableChildren"))
            .flat_map(|children| children.split(',').map(str::trim))
            .filter(|child| !child.is_empty())
            .collect::<HashSet<_>>();
        let interval_additions = history
            .features
            .iter()
            .enumerate()
            .filter(|(_, feature)| {
                classify(feature) == Some(FeatureClass::Hole)
                    && !feature.properties.contains_key("DissectableChildren")
            })
            .filter_map(|(feature_index, feature)| {
                let source = feature
                    .source_id
                    .as_deref()
                    .and_then(|source| source.parse::<u32>().ok())?;
                let upper = history
                    .features
                    .iter()
                    .filter(|candidate| classify(candidate) == Some(FeatureClass::Hole))
                    .filter_map(|candidate| {
                        candidate
                            .source_id
                            .as_deref()
                            .and_then(|source| source.parse::<u32>().ok())
                    })
                    .filter(|candidate| *candidate > source)
                    .min()?;
                let mut profiles = history.features.iter().filter(|candidate| {
                    let identity = candidate.source_id.as_deref().unwrap_or(&candidate.id);
                    !claimed_profiles.contains(identity)
                        && candidate
                            .source_id
                            .as_deref()
                            .and_then(|source| source.parse::<u32>().ok())
                            .is_some_and(|candidate| source < candidate && candidate < upper)
                        && classify(candidate) == Some(FeatureClass::Sketch)
                        && crate::history::is_hole_profile_construction(candidate)
                });
                let profile = profiles.next()?;
                profiles.next().is_none().then(|| {
                    (
                        feature_index,
                        profile
                            .source_id
                            .clone()
                            .unwrap_or_else(|| profile.id.clone()),
                    )
                })
            })
            .collect::<Vec<_>>();
        let interval_claim_counts = interval_additions.iter().fold(
            HashMap::<String, usize>::new(),
            |mut counts, (_, profile)| {
                *counts.entry(profile.clone()).or_default() += 1;
                counts
            },
        );
        for (feature_index, profile_source) in interval_additions {
            if interval_claim_counts.get(profile_source.as_str()) != Some(&1) {
                continue;
            }
            history.features[feature_index]
                .properties
                .insert("DissectableChildren".into(), profile_source);
        }
    }
}

pub(crate) fn enrich_history_cosmetic_thread_diameters(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for history in histories {
        let features_by_id = history
            .features
            .iter()
            .map(|feature| (feature.id.as_str(), feature))
            .collect::<HashMap<_, _>>();
        let features_by_source = history
            .features
            .iter()
            .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
            .collect::<HashMap<_, _>>();
        let mut candidates = HashMap::<String, Vec<f64>>::new();
        for lane in lanes {
            for selection in &lane.surface_selections {
                let Some(thread) = features_by_id.get(selection.feature_ref.as_str()) else {
                    continue;
                };
                if classify(thread) != Some(FeatureClass::CosmeticThread) {
                    continue;
                }
                let mut producer_diameters = selection
                    .producer_feature_refs
                    .iter()
                    .chain(selection.terminal_feature_ref.iter())
                    .filter_map(|producer| features_by_id.get(producer.as_str()).copied())
                    .filter_map(|producer| {
                        crate::history::threaded_hole_major_diameter(
                            producer,
                            &features_by_source,
                            &history.features,
                        )
                    })
                    .collect::<Vec<_>>();
                producer_diameters.sort_by(f64::total_cmp);
                producer_diameters.dedup_by(|left, right| left.to_bits() == right.to_bits());
                let [diameter] = producer_diameters.as_slice() else {
                    continue;
                };
                candidates
                    .entry(thread.id.clone())
                    .or_default()
                    .push(*diameter);
            }
        }
        for feature in &mut history.features {
            if feature.parameters.contains_key("D2") {
                continue;
            }
            let Some(values) = candidates.get(&feature.id) else {
                continue;
            };
            let Some((&diameter, rest)) = values.split_first() else {
                continue;
            };
            if rest
                .iter()
                .any(|candidate| candidate.to_bits() != diameter.to_bits())
            {
                continue;
            }
            feature.parameters.insert(
                "D2".into(),
                format!("<MOD-DIAM>{}", crate::history::format_length_mm(diameter)),
            );
        }
    }
}

pub(crate) fn enrich_history_cosmetic_thread_diameters_without_hole_constructions(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut projection = histories.to_vec();
    enrich_history_hole_constructions(&mut projection, lanes);
    enrich_history_cosmetic_thread_diameters(&mut projection, lanes);
    let fallback_parameters = projection
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature.id.clone(), feature.parameters.get("D2")?.clone())))
        .collect::<HashMap<_, _>>();
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if feature.parameters.contains_key("D2") {
            continue;
        }
        let Some(diameter) = fallback_parameters.get(&feature.id) else {
            continue;
        };
        feature.parameters.insert("D2".into(), diameter.clone());
    }
}

#[derive(Clone)]
struct ProfiledHoleConstruction {
    diameter: Length,
    extent: Termination,
    kind: HoleKind,
    bottom: Option<HoleBottom>,
    taper_angle: Option<Angle>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfileEvidence {
    Dimensions,
    AxialTopology,
}

const DISPLAY_DIMENSION_TOLERANCE_MM: f64 = 1.0e-5;
const GENERATED_PROFILE_TERMINAL_OVERRUN_MM: [f64; 4] = [0.0, 0.000_025, 0.000_05, 0.001];

fn profiled_hole_construction(
    profile: &crate::records::Feature,
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Option<ProfiledHoleConstruction> {
    profiled_hole_construction_with_evidence(profile, sketch, entities, ProfileEvidence::Dimensions)
}

fn profiled_hole_construction_with_evidence(
    profile: &crate::records::Feature,
    sketch: &SketchId,
    entities: &[SketchEntity],
    evidence: ProfileEvidence,
) -> Option<ProfiledHoleConstruction> {
    let source_dimensions = profile
        .content
        .iter()
        .filter_map(|content| match content {
            crate::records::FeatureContent::Dimension(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expressions = if source_dimensions.is_empty() {
        profile.parameters.values().map(String::as_str).collect()
    } else {
        source_dimensions
            .into_iter()
            .filter_map(|name| profile.parameters.get(name).map(String::as_str))
            .collect::<Vec<_>>()
    };
    let mut diameters = expressions
        .iter()
        .copied()
        .filter_map(|value| crate::history::strip_diameter_modifier(value))
        .filter_map(crate::history::parse_dimension_length_mm)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect::<Vec<_>>();
    let mut angles = expressions
        .iter()
        .copied()
        .filter_map(crate::history::parse_bounded_angle_rad)
        .collect::<Vec<_>>();
    let flat_bottom = expressions.iter().copied().any(|value| {
        crate::history::parse_angle_rad(value)
            .is_some_and(|angle| (angle - std::f64::consts::PI).abs() <= EPS_HOLE_EXACT_GEOMETRY)
    });
    let mut lengths = expressions
        .iter()
        .copied()
        .filter(|value| {
            crate::history::strip_diameter_modifier(value).is_none()
                && crate::history::parse_bounded_angle_rad(value).is_none()
        })
        .filter_map(crate::history::parse_dimension_length_mm)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect::<Vec<_>>();
    diameters.sort_by(f64::total_cmp);
    diameters.dedup_by(|left, right| (*left - *right).abs() <= EPS_HOLE_GEOMETRY);
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|left, right| (*left - *right).abs() <= EPS_HOLE_EXACT_GEOMETRY);
    lengths.sort_by(f64::total_cmp);
    lengths.dedup_by(|left, right| (*left - *right).abs() <= EPS_HOLE_GEOMETRY);
    let dimension_only = if crate::history::is_hole_profile_construction(profile) {
        match (diameters.as_slice(), lengths.as_slice(), angles.as_slice()) {
            ([diameter], [depth], []) => Some(ProfiledHoleConstruction {
                diameter: Length(*diameter),
                extent: Termination::Blind {
                    length: Length(*depth),
                },
                kind: HoleKind::Simple,
                bottom: Some(HoleBottom::Flat),
                taper_angle: None,
            }),
            ([diameter], [depth], [drill_point_angle]) => Some(ProfiledHoleConstruction {
                diameter: Length(*diameter),
                extent: Termination::Blind {
                    length: Length(*depth),
                },
                kind: HoleKind::SimpleDrilled {
                    drill_point_angle: Angle(*drill_point_angle),
                },
                bottom: Some(HoleBottom::Angled {
                    included_angle: Angle(*drill_point_angle),
                    depth_to_tip: false,
                }),
                taper_angle: None,
            }),
            _ => None,
        }
    } else {
        None
    };
    if evidence == ProfileEvidence::Dimensions {
        if let Some(construction) = dimension_only.clone() {
            return Some(construction);
        }
    }
    let lines = entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && !entity.construction)
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Line { start, end } => Some((start, end)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let points = entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && !entity.construction)
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Point { position } => Some(position),
            _ => None,
        })
        .collect::<Vec<_>>();
    let same_point = |left: Point2, right: Point2| {
        (left.u - right.u).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
            && (left.v - right.v).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
    };
    let has_line = |first: Point2, second: Point2| {
        lines.iter().any(|(start, end)| {
            (same_point(*start, first) && same_point(*end, second))
                || (same_point(*start, second) && same_point(*end, first))
        })
    };
    let has_point_pair = |first: Point2, second: Point2| {
        points.iter().any(|point| same_point(*point, first))
            && points.iter().any(|point| same_point(*point, second))
    };
    let profile_translation = |edges: &[(Point2, Point2)], minimum_lines: usize| {
        let expected_points = edges
            .iter()
            .flat_map(|(first, second)| [*first, *second])
            .collect::<Vec<_>>();
        let actual_points = lines
            .iter()
            .flat_map(|(first, second)| [*first, *second])
            .chain(points.iter().copied())
            .collect::<Vec<_>>();
        actual_points.iter().find_map(|actual| {
            expected_points.iter().find_map(|expected| {
                let translation = Point2::new(actual.u - expected.u, actual.v - expected.v);
                let translated = edges
                    .iter()
                    .map(|(first, second)| {
                        (
                            Point2::new(first.u + translation.u, first.v + translation.v),
                            Point2::new(second.u + translation.u, second.v + translation.v),
                        )
                    })
                    .collect::<Vec<_>>();
                (translated
                    .iter()
                    .filter(|(first, second)| has_line(*first, *second))
                    .count()
                    >= minimum_lines
                    && translated.iter().all(|(first, second)| {
                        has_line(*first, *second) || has_point_pair(*first, *second)
                    }))
                .then_some(translation)
            })
        })
    };
    if let Some(construction) = dimension_only {
        let Termination::Blind { length } = construction.extent else {
            unreachable!("dimension-only hole profiles are blind");
        };
        let radius = construction.diameter.0 / 2.0;
        for swap in [false, true] {
            for axial_sign in [-1.0, 1.0] {
                for radial_sign in [-1.0, 1.0] {
                    let point = |axial: f64, radial: f64| {
                        let axial = axial * axial_sign;
                        let radial = radial * radial_sign;
                        if swap {
                            Point2::new(radial, axial)
                        } else {
                            Point2::new(axial, radial)
                        }
                    };
                    let axis_entry = point(0.0, 0.0);
                    let wall_entry = point(0.0, radius);
                    let wall_end = point(-length.0, radius);
                    let axis_end = point(-length.0, 0.0);
                    let edges = [
                        (axis_entry, wall_entry),
                        (wall_entry, wall_end),
                        (wall_end, axis_end),
                        (axis_end, axis_entry),
                    ];
                    if profile_translation(&edges, 2).is_some() {
                        return Some(construction);
                    }
                }
            }
        }
        return None;
    }
    if let ([diameter, recess_diameter, entry_diameter], [recess_depth, depth], [entry_angle]) =
        (diameters.as_slice(), lengths.as_slice(), angles.as_slice())
    {
        let bore_radius = diameter / 2.0;
        let recess_radius = recess_diameter / 2.0;
        let entry_radius = entry_diameter / 2.0;
        let setback = (entry_radius - recess_radius) / (entry_angle / 2.0).tan();
        if !setback.is_finite()
            || setback <= 0.0
            || recess_depth <= &setback
            || depth <= recess_depth
        {
            return None;
        }
        for swap in [false, true] {
            for axial_sign in [-1.0, 1.0] {
                for radial_sign in [-1.0, 1.0] {
                    let point = |axial: f64, radial: f64| {
                        let axial = axial * axial_sign;
                        let radial = radial * radial_sign;
                        if swap {
                            Point2::new(radial, axial)
                        } else {
                            Point2::new(axial, radial)
                        }
                    };
                    let entry = point(0.0, entry_radius);
                    let recess_start = point(-setback, recess_radius);
                    let recess_end = point(-recess_depth, recess_radius);
                    let bore_start = point(-recess_depth, bore_radius);
                    for terminal_overrun in GENERATED_PROFILE_TERMINAL_OVERRUN_MM {
                        let bore_end = point(-depth - terminal_overrun, bore_radius);
                        let edges = [
                            (entry, recess_start),
                            (recess_start, recess_end),
                            (recess_end, bore_start),
                            (bore_start, bore_end),
                        ];
                        if profile_translation(&edges, 2).is_some() {
                            return Some(ProfiledHoleConstruction {
                                diameter: Length(*diameter),
                                extent: Termination::ThroughAll,
                                kind: HoleKind::Counterdrill {
                                    diameter: Length(*recess_diameter),
                                    entry_diameter: Some(Length(*entry_diameter)),
                                    depth: Length(*recess_depth),
                                    angle: Angle(*entry_angle),
                                },
                                bottom: None,
                                taper_angle: None,
                            });
                        }
                    }
                }
            }
        }
        return None;
    }
    let [diameter, entry_diameter] = diameters.as_slice() else {
        return None;
    };
    if diameter >= entry_diameter {
        return None;
    }
    let bore_radius = diameter / 2.0;
    let entry_radius = entry_diameter / 2.0;
    for swap in [false, true] {
        for axial_sign in [-1.0, 1.0] {
            for radial_sign in [-1.0, 1.0] {
                let point = |axial: f64, radial: f64| {
                    let axial = axial * axial_sign;
                    let radial = radial * radial_sign;
                    if swap {
                        Point2::new(radial, axial)
                    } else {
                        Point2::new(axial, radial)
                    }
                };
                if let ([depth], []) = (lengths.as_slice(), angles.as_slice()) {
                    for (entry_radius, terminal_radius) in
                        [(bore_radius, entry_radius), (entry_radius, bore_radius)]
                    {
                        let axis_entry = point(0.0, 0.0);
                        let wall_entry = point(0.0, entry_radius);
                        let wall_end = point(-depth, terminal_radius);
                        let axis_end = point(-depth, 0.0);
                        let edges = [
                            (axis_entry, wall_entry),
                            (wall_entry, wall_end),
                            (wall_end, axis_end),
                            (axis_end, axis_entry),
                        ];
                        let materialized_edges = edges
                            .iter()
                            .filter(|(first, second)| has_line(*first, *second))
                            .count();
                        if materialized_edges < 2
                            || edges.iter().any(|(first, second)| {
                                !has_line(*first, *second) && !has_point_pair(*first, *second)
                            })
                        {
                            continue;
                        }
                        let half_angle = ((terminal_radius - entry_radius).abs() / depth).atan();
                        if !half_angle.is_finite() || half_angle <= 0.0 {
                            continue;
                        }
                        return Some(ProfiledHoleConstruction {
                            diameter: Length(entry_radius * 2.0),
                            extent: Termination::Blind {
                                length: Length(*depth),
                            },
                            kind: HoleKind::Simple,
                            bottom: Some(HoleBottom::Flat),
                            taper_angle: Some(Angle(half_angle * 2.0)),
                        });
                    }
                }
                if let [entry_depth, depth] = lengths.as_slice() {
                    let entry = point(0.0, entry_radius);
                    let entry_corner = point(-entry_depth, entry_radius);
                    let bore_corner = point(-entry_depth, bore_radius);
                    let terminal_overruns = if angles.is_empty() && !flat_bottom {
                        &GENERATED_PROFILE_TERMINAL_OVERRUN_MM[..]
                    } else {
                        &GENERATED_PROFILE_TERMINAL_OVERRUN_MM[..1]
                    };
                    for terminal_overrun in terminal_overruns {
                        let bore_end = point(-depth - terminal_overrun, bore_radius);
                        let edges = [
                            (entry, entry_corner),
                            (entry_corner, bore_corner),
                            (bore_corner, bore_end),
                        ];
                        let Some(translation) = profile_translation(&edges, 2) else {
                            continue;
                        };
                        let (kind, extent) = match angles.as_slice() {
                            [] => {
                                let extent = if flat_bottom {
                                    Termination::Blind {
                                        length: Length(*depth),
                                    }
                                } else {
                                    Termination::ThroughAll
                                };
                                (
                                    HoleKind::Counterbore {
                                        diameter: Length(*entry_diameter),
                                        depth: Length(*entry_depth),
                                    },
                                    extent,
                                )
                            }
                            [drill_point_angle] => {
                                let drill_length = bore_radius / (drill_point_angle / 2.0).tan();
                                let translated = |point: Point2| {
                                    Point2::new(point.u + translation.u, point.v + translation.v)
                                };
                                if !drill_length.is_finite()
                                    || !has_line(
                                        translated(bore_end),
                                        translated(point(-depth - drill_length, 0.0)),
                                    )
                                {
                                    continue;
                                }
                                (
                                    HoleKind::CounterboreDrilled {
                                        diameter: Length(*entry_diameter),
                                        depth: Length(*entry_depth),
                                        drill_point_angle: Angle(*drill_point_angle),
                                    },
                                    Termination::Blind {
                                        length: Length(*depth),
                                    },
                                )
                            }
                            _ => continue,
                        };
                        return Some(ProfiledHoleConstruction {
                            diameter: Length(*diameter),
                            extent,
                            kind,
                            bottom: (angles.is_empty() && flat_bottom).then_some(HoleBottom::Flat),
                            taper_angle: None,
                        });
                    }
                }
                let [depth] = lengths.as_slice() else {
                    continue;
                };
                if let [sink_angle] = angles.as_slice() {
                    let setback = (entry_radius - bore_radius) / (sink_angle / 2.0).tan();
                    if !setback.is_finite() {
                        continue;
                    }
                    let entry = point(0.0, entry_radius);
                    let bore_start = point(-setback, bore_radius);
                    let mirrored_bore_start = point(-setback, -bore_radius);
                    let profile_matches =
                        GENERATED_PROFILE_TERMINAL_OVERRUN_MM.iter().any(|overrun| {
                            [
                                (bore_start, bore_radius),
                                (mirrored_bore_start, -bore_radius),
                            ]
                            .into_iter()
                            .any(|(wall_start, wall_radius)| {
                                let edges = [
                                    (entry, bore_start),
                                    (wall_start, point(-depth - overrun, wall_radius)),
                                ];
                                profile_translation(&edges, 2).is_some()
                            })
                        });
                    if profile_matches {
                        return Some(ProfiledHoleConstruction {
                            diameter: Length(*diameter),
                            extent: Termination::ThroughAll,
                            kind: HoleKind::Countersink {
                                diameter: Length(*entry_diameter),
                                angle: Angle(*sink_angle),
                            },
                            bottom: None,
                            taper_angle: None,
                        });
                    }
                    continue;
                }
                let [first_angle, second_angle] = angles.as_slice() else {
                    continue;
                };
                for (sink_angle, drill_point_angle) in
                    [(*first_angle, *second_angle), (*second_angle, *first_angle)]
                {
                    let setback = (entry_radius - bore_radius) / (sink_angle / 2.0).tan();
                    let drill_length = bore_radius / (drill_point_angle / 2.0).tan();
                    if !setback.is_finite() || !drill_length.is_finite() {
                        continue;
                    }
                    let entry = point(0.0, entry_radius);
                    let bore_start = point(-setback, bore_radius);
                    let bore_end = point(-depth, bore_radius);
                    let tip = point(-depth - drill_length, 0.0);
                    let edges = [(entry, bore_start), (bore_start, bore_end), (bore_end, tip)];
                    if profile_translation(&edges, 2).is_some() {
                        return Some(ProfiledHoleConstruction {
                            diameter: Length(*diameter),
                            extent: Termination::Blind {
                                length: Length(*depth),
                            },
                            kind: HoleKind::Countersink {
                                diameter: Length(*entry_diameter),
                                angle: Angle(sink_angle),
                            },
                            bottom: Some(HoleBottom::Angled {
                                included_angle: Angle(drill_point_angle),
                                depth_to_tip: false,
                            }),
                            taper_angle: None,
                        });
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn project_profiled_hole_constructions(
    features: &mut [cadmpeg_ir::features::Feature],
    entities: &[SketchEntity],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut enriched_histories = histories.to_vec();
    crate::history::enrich_history_parameters_semantic(&mut enriched_histories, lanes);
    let mut ownership_histories = enriched_histories.clone();
    enrich_history_hole_constructions(&mut ownership_histories, lanes);
    let histories = enriched_histories.as_slice();
    let incomplete = |diameter: &Option<Length>, extent: &Option<Termination>, kind: &HoleKind| {
        diameter.is_none()
            || extent
                .as_ref()
                .is_none_or(|extent| matches!(extent, Termination::Unresolved))
            || kind.is_unresolved()
    };
    let complete_native_holes = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Hole {
                diameter,
                extent,
                kind,
                ..
            } = &feature.definition
            else {
                return None;
            };
            if incomplete(diameter, extent, kind) {
                return None;
            }
            feature.native_ref.clone()
        })
        .collect::<HashSet<_>>();
    let model_sketches = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.clone()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let native_histories = histories
        .iter()
        .enumerate()
        .flat_map(|(history_index, history)| {
            history
                .features
                .iter()
                .map(move |feature| (feature.id.as_str(), history_index))
        })
        .collect::<HashMap<_, _>>();
    let Ok(mut unowned_incomplete_holes) = alloc_filled(
        histories.len(),
        Vec::<(String, u32)>::new(),
        "SLDPRT unowned incomplete-hole histories",
    ) else {
        return;
    };
    for feature in features.iter() {
        let FeatureDefinition::Hole {
            diameter,
            extent,
            kind,
            ..
        } = &feature.definition
        else {
            continue;
        };
        if !incomplete(diameter, extent, kind) {
            continue;
        }
        let Some(history_index) = feature
            .native_ref
            .as_deref()
            .and_then(|native| native_histories.get(native))
        else {
            continue;
        };
        let unowned = feature
            .native_ref
            .as_deref()
            .and_then(|native| {
                histories[*history_index]
                    .features
                    .iter()
                    .find(|candidate| candidate.id == native)
            })
            .is_some_and(|native| !native.properties.contains_key("DissectableChildren"));
        if unowned {
            let native = feature
                .native_ref
                .as_deref()
                .expect("unowned holes carry a native reference");
            let ordinal = histories[*history_index]
                .features
                .iter()
                .find(|candidate| candidate.id == native)
                .expect("native hole was resolved above")
                .ordinal;
            unowned_incomplete_holes[*history_index].push((native.into(), ordinal));
        }
    }
    let profiled_constructions = histories
        .iter()
        .zip(&ownership_histories)
        .map(|(history, ownership_history)| {
            let claimed_profiles = history
                .features
                .iter()
                .filter_map(|feature| feature.properties.get("DissectableChildren"))
                .chain(
                    ownership_history
                        .features
                        .iter()
                        .filter(|feature| complete_native_holes.contains(&feature.id))
                        .filter_map(|feature| feature.properties.get("DissectableChildren")),
                )
                .flat_map(|children| children.split(',').map(str::trim))
                .filter(|child| !child.is_empty())
                .filter_map(|child| {
                    let mut profiles = history.features.iter().filter(|candidate| {
                        candidate.source_id.as_deref() == Some(child) || candidate.id == child
                    });
                    let profile = profiles.next()?;
                    profiles.next().is_none().then_some(&profile.id)
                })
                .collect::<HashSet<_>>();
            history
                .features
                .iter()
                .filter(|profile| !claimed_profiles.contains(&profile.id))
                .filter_map(|profile| {
                    let sketch = model_sketches.get(&profile.id)?;
                    Some((
                        profile.ordinal,
                        profiled_hole_construction_with_evidence(
                            profile,
                            sketch,
                            entities,
                            ProfileEvidence::AxialTopology,
                        )?,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let fallback_constructions = unowned_incomplete_holes
        .iter_mut()
        .zip(profiled_constructions.iter())
        .flat_map(|(holes, profiles)| {
            holes.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
            let mut profiles = profiles.clone();
            profiles.sort_by_key(|(ordinal, _)| *ordinal);
            (holes.len() == profiles.len())
                .then_some(
                    holes
                        .iter()
                        .zip(profiles)
                        .map(|((hole, _), (_, construction))| (hole.clone(), construction)),
                )
                .into_iter()
                .flatten()
        })
        .collect::<HashMap<_, _>>();
    for feature in features.iter_mut() {
        let FeatureDefinition::Hole {
            diameter,
            extent,
            kind,
            bottom,
            taper_angle,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if !incomplete(diameter, extent, kind) {
            continue;
        }
        let Some((history_index, native)) = feature.native_ref.as_deref().and_then(|native| {
            let history_index = *native_histories.get(native)?;
            let native = histories[history_index]
                .features
                .iter()
                .find(|candidate| candidate.id == native)?;
            Some((history_index, native))
        }) else {
            continue;
        };
        let history = &histories[history_index];
        let position = hole_position_feature(native, histories, lanes).map(|feature| &feature.id);
        let direct = native
            .properties
            .get("DissectableChildren")
            .and_then(|children| {
                let mut constructions = children.split(',').filter_map(|source| {
                    let mut profiles = history.features.iter().filter(|candidate| {
                        candidate.source_id.as_deref() == Some(source.trim())
                            || candidate.id == source.trim()
                    });
                    let profile = profiles.next()?;
                    profiles.next().is_none().then_some(())?;
                    if position == Some(&profile.id) {
                        return None;
                    }
                    let sketch = model_sketches.get(&profile.id)?;
                    profiled_hole_construction(profile, sketch, entities)
                });
                let construction = constructions.next()?;
                constructions.next().is_none().then_some(construction)
            });
        let construction = direct.or_else(|| {
            if native.properties.contains_key("DissectableChildren") {
                return None;
            }
            fallback_constructions.get(&native.id).cloned()
        });
        let Some(construction) = construction else {
            continue;
        };
        *diameter = Some(construction.diameter);
        *extent = Some(construction.extent);
        *kind = construction.kind;
        *bottom = construction.bottom;
        *taper_angle = construction.taper_angle;
    }
}

pub(crate) fn project_hole_position_sketches(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[Sketch],
    sketch_entities: &[SketchEntity],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let model_sketch_features = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
            } = &feature.definition
            else {
                return None;
            };
            Some((
                feature.native_ref.clone()?,
                (feature.id.clone(), sketch.clone()),
            ))
        })
        .collect::<HashMap<_, _>>();
    let model_sketches = model_sketch_features
        .iter()
        .map(|(native, (_, sketch))| (native.clone(), sketch.clone()))
        .collect::<HashMap<_, _>>();
    for feature in features.iter_mut() {
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Hole { placements, .. } = &mut feature.definition else {
            continue;
        };
        if !placements.is_empty() {
            continue;
        }
        let Some(native) = feature
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native).copied())
        else {
            continue;
        };
        let Some(position_feature) =
            hole_position_feature(native, histories, lanes).or_else(|| {
                direct_hole_position_feature(native, histories, &model_sketches, sketch_entities)
            })
        else {
            continue;
        };
        let Some((position_dependency, sketch_id)) =
            model_sketch_features.get(position_feature.id.as_str())
        else {
            continue;
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
            continue;
        };
        let matching_lanes = lanes
            .iter()
            .filter(|lane| lane.configuration == sketch.configuration)
            .collect::<Vec<_>>();
        let mut authored_markers = matching_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .filter(|marker| {
                marker.feature_ref.as_deref() == Some(position_feature.id.as_str())
                    && marker.object_index.is_some()
                    && marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            })
            .collect::<Vec<_>>();
        let mut unindexed_marker_ids = HashSet::new();
        let paired_marker_ids = if authored_markers.is_empty() {
            // Direct projection requires a complete alternate object roster.
            // An isolated pair among other coordinates can describe a
            // construction curve or dimension handle instead of a hole locus.
            let mut paired_marker_ids = HashSet::new();
            let mut complete_alternate_encoding = true;
            let mut unindexed_locus: Option<&crate::records::SketchInputEntity> = None;
            let mut complete_unindexed_encoding = true;
            for lane in matching_lanes {
                let position_markers = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| {
                        marker.feature_ref.as_deref() == Some(position_feature.id.as_str())
                            && marker.coordinates_m.is_some()
                    })
                    .collect::<Vec<_>>();
                let indexed_markers = position_markers
                    .iter()
                    .filter(|marker| marker.object_index.is_some())
                    .count();
                let paired = paired_object_locus_markers(lane, position_feature.id.as_str());
                complete_alternate_encoding &= paired.len() == indexed_markers;
                paired_marker_ids.extend(paired.iter().map(|marker| marker.id.as_str()));
                authored_markers.extend(paired);
                if indexed_markers == 0
                    && position_markers.iter().all(|marker| {
                        matches!(
                            marker.kind,
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                    })
                {
                    let loci = position_markers.into_iter().filter(|marker| {
                        marker
                            .coordinates_m
                            .is_some_and(|[u, v]| u != 0.0 || v != 0.0)
                    });
                    let mut loci = loci.collect::<Vec<_>>();
                    if let [locus] = loci.as_mut_slice() {
                        if unindexed_locus
                            .is_some_and(|previous| previous.coordinates_m != locus.coordinates_m)
                        {
                            complete_unindexed_encoding = false;
                        } else {
                            unindexed_locus = Some(*locus);
                        }
                    } else {
                        complete_unindexed_encoding = false;
                    }
                } else {
                    complete_unindexed_encoding = false;
                }
            }
            // Legacy position sketches omit object indexes from their point
            // records. Accept one locus only when every matching lane has a
            // point-only coordinate roster with exactly one non-origin point;
            // zero points are relation anchors and do not identify a hole.
            if complete_unindexed_encoding {
                if let Some(marker) = unindexed_locus {
                    unindexed_marker_ids.insert(marker.id.as_str());
                    authored_markers.push(marker);
                    HashSet::new()
                } else if complete_alternate_encoding {
                    paired_marker_ids
                } else {
                    authored_markers.clear();
                    HashSet::new()
                }
            } else if complete_alternate_encoding {
                paired_marker_ids
            } else {
                authored_markers.clear();
                HashSet::new()
            }
        } else {
            HashSet::new()
        };
        if authored_markers.is_empty() {
            continue;
        }
        let marker_transform = sketch_frame_marker_transform(sketch, QUANTUM);
        let v_axis = normal.cross(u_axis);
        let mut resolved = Vec::with_capacity(authored_markers.len());
        for marker in &authored_markers {
            let mut entities = sketch_entities.iter().filter(|entity| {
                entity.sketch == *sketch_id
                    && entity.native_ref.as_deref() == Some(marker.id.as_str())
                    && matches!(entity.geometry, SketchGeometry::Point { .. })
            });
            let entity = entities.next();
            if entities.next().is_some() {
                resolved.clear();
                break;
            }
            let position = match entity {
                Some(entity) => {
                    let SketchGeometry::Point { position } = entity.geometry else {
                        unreachable!("point geometry was filtered above");
                    };
                    position
                }
                None if paired_marker_ids.contains(marker.id.as_str())
                    || unindexed_marker_ids.contains(marker.id.as_str()) =>
                {
                    let Some(transform) = marker_transform else {
                        resolved.clear();
                        break;
                    };
                    let [u, v] = marker
                        .coordinates_m
                        .expect("coordinates were filtered above");
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let Some((u, v)) = transform.apply(native) else {
                        resolved.clear();
                        break;
                    };
                    Point2::new(u as f64 * QUANTUM, v as f64 * QUANTUM)
                }
                None => {
                    resolved.clear();
                    break;
                }
            };
            resolved.push(HolePlacement::Axis {
                origin: Point3::new(
                    origin.x + position.u * u_axis.x + position.v * v_axis.x,
                    origin.y + position.u * u_axis.y + position.v * v_axis.y,
                    origin.z + position.u * u_axis.z + position.v * v_axis.z,
                ),
                axis: normal,
            });
        }
        if resolved.len() == authored_markers.len() {
            *placements = resolved;
            if !feature.dependencies.contains(position_dependency) {
                feature.dependencies.push(position_dependency.clone());
            }
        }
    }
}

fn paired_object_locus_markers<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<&'a crate::records::SketchInputEntity> {
    // Object-locus layouts emit an indexed coordinate handle followed by an
    // unindexed zero point. The adjacent anchor distinguishes object loci from
    // the dimension and display handles in the same feature object.
    lane.sketch_entities
        .windows(2)
        .filter_map(|pair| {
            let [object, anchor] = pair else {
                unreachable!("two-record window");
            };
            (object.feature_ref.as_deref() == Some(feature)
                && anchor.feature_ref.as_deref() == Some(feature)
                && object.object_index.is_some()
                && object.coordinates_m.is_some()
                && anchor.object_index.is_none()
                && anchor.kind == SketchInputKind::Point
                && anchor.coordinates_m == Some([0.0, 0.0]))
            .then_some(object)
        })
        .collect()
}

fn hole_position_feature<'a>(
    hole: &crate::records::Feature,
    histories: &'a [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Option<&'a crate::records::Feature> {
    let mut sources = lanes
        .iter()
        .filter_map(|lane| hole_position_sketch_source(hole, lane))
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    let [source] = sources.as_slice() else {
        return None;
    };
    let mut position_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|candidate| {
            classify(candidate) == Some(FeatureClass::Sketch)
                && lanes.iter().any(|lane| {
                    candidate
                        .source_id
                        .as_deref()
                        .and_then(|value| value.parse::<u32>().ok())
                        .or_else(|| {
                            feature_object_name(candidate, lane).and_then(|name| name.object_id)
                        })
                        == Some(*source)
                })
        });
    let position = position_features.next()?;
    position_features.next().is_none().then_some(position)
}

/// Whether a hole has a configuration-local position source in the supplied
/// lanes. A lane without this carrier inherits the document hole placements;
/// a lane with one must retain unresolved placement state when projection
/// cannot establish its authored loci.
pub(crate) fn hole_position_carrier_present(
    feature: &cadmpeg_ir::features::Feature,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> bool {
    let Some(native_ref) = feature.native_ref.as_deref() else {
        return false;
    };
    let Some(native) = histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|candidate| candidate.id == native_ref)
    else {
        return false;
    };
    lanes
        .iter()
        .any(|lane| hole_position_sketch_source(native, lane).is_some())
}

pub(crate) fn project_spatial_hole_position_sketches(
    features: &mut [cadmpeg_ir::features::Feature],
    spatial_sketches: &[SpatialSketch],
    spatial_entities: &[SpatialSketchEntity],
    surfaces: &[Surface],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let model_sketches = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::SpatialSketch {
                sketch: Some(sketch),
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.clone()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    for feature in features.iter_mut() {
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Hole {
            placements,
            diameter: Some(Length(diameter)),
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if !placements.is_empty() || !diameter.is_finite() || *diameter <= 0.0 {
            continue;
        }
        let Some(native) = feature
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native).copied())
        else {
            continue;
        };
        let Some(position_feature) = hole_position_feature(native, histories, lanes) else {
            continue;
        };
        let Some(sketch_id) = model_sketches.get(position_feature.id.as_str()) else {
            continue;
        };
        let Some(sketch) = spatial_sketches
            .iter()
            .find(|sketch| sketch.id == *sketch_id)
        else {
            continue;
        };
        let authored_markers = lanes
            .iter()
            .filter(|lane| lane.configuration == sketch.configuration)
            .flat_map(|lane| &lane.sketch_entities)
            .filter(|marker| {
                marker.feature_ref.as_deref() == Some(position_feature.id.as_str())
                    && marker.object_index.is_some()
            })
            .collect::<Vec<_>>();
        let radius = *diameter * 0.5;
        let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
        let axis_tolerance_squared = EPS_HOLE_EXACT_GEOMETRY;
        let mut resolved = Vec::with_capacity(authored_markers.len());
        let mut ambiguous = false;
        for marker in &authored_markers {
            let mut points = spatial_entities.iter().filter_map(|entity| {
                (entity.sketch == *sketch_id
                    && entity.native_ref.as_deref() == Some(marker.id.as_str()))
                .then_some(&entity.geometry)
                .and_then(|geometry| match geometry {
                    SpatialSketchGeometry::Point { position } => Some(*position),
                    _ => None,
                })
            });
            let Some(point) = points.next() else {
                continue;
            };
            if points.next().is_some() {
                ambiguous = true;
                break;
            }
            let mut axes = surfaces
                .iter()
                .filter_map(|surface| match &surface.geometry {
                    SurfaceGeometry::Cylinder {
                        origin,
                        axis,
                        radius: candidate,
                        ..
                    } if (*candidate - radius).abs() <= radius_tolerance
                        && point_axis_distance_squared(point, *origin, *axis)
                            <= axis_tolerance_squared =>
                    {
                        Some((*origin, *axis))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if axes.is_empty() {
                let mut support_axes = surfaces
                    .iter()
                    .filter_map(|surface| cylindrical_support_normal(surface, point))
                    .map(canonical_axis)
                    .collect::<Vec<_>>();
                support_axes
                    .sort_by_key(|axis| [axis.x.to_bits(), axis.y.to_bits(), axis.z.to_bits()]);
                support_axes.dedup_by(|left, right| left.dot(*right) >= 1.0 - EPS_HOLE_GEOMETRY);
                if let [axis] = support_axes.as_slice() {
                    axes.push((point, *axis));
                }
            }
            let Some(axes) = carrier_placements(axes) else {
                continue;
            };
            let [placement] = axes.as_slice() else {
                ambiguous = true;
                break;
            };
            resolved.push(placement.clone());
        }
        if resolved.is_empty() && !ambiguous {
            let points = spatial_entities
                .iter()
                .filter(|entity| entity.sketch == *sketch_id)
                .filter_map(|entity| match entity.geometry {
                    SpatialSketchGeometry::Point { position } => Some(position),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let Some(inferred) = coplanar_spatial_position_placements(&points) {
                resolved = inferred;
            }
        }
        resolved.sort_by_key(|placement| match placement {
            HolePlacement::Axis { origin, axis } => [
                origin.x.to_bits(),
                origin.y.to_bits(),
                origin.z.to_bits(),
                axis.x.to_bits(),
                axis.y.to_bits(),
                axis.z.to_bits(),
            ],
            HolePlacement::Directed { .. } => [0; 6],
        });
        resolved.dedup();
        if !ambiguous && !resolved.is_empty() {
            *placements = resolved;
        }
    }
}

fn coplanar_spatial_position_placements(points: &[Point3]) -> Option<Vec<HolePlacement>> {
    let mut points = points.to_vec();
    points.sort_by_key(|point| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]);
    points.dedup();
    if points.len() < 3
        || points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return None;
    }
    let displacement = |point: Point3| {
        Vector3::new(
            point.x - points[0].x,
            point.y - points[0].y,
            point.z - points[0].z,
        )
    };
    let extent = points
        .iter()
        .skip(1)
        .map(|point| displacement(*point).norm())
        .fold(1.0_f64, f64::max);
    let first = points
        .iter()
        .skip(1)
        .map(|point| displacement(*point))
        .max_by(|left, right| left.norm().total_cmp(&right.norm()))?;
    let candidate = points
        .iter()
        .skip(1)
        .map(|point| first.cross(displacement(*point)))
        .max_by(|left, right| left.norm().total_cmp(&right.norm()))?;
    let norm = candidate.norm();
    if norm <= extent * extent * EPS_HOLE_DEGENERATE_NORMAL {
        return None;
    }
    let normal = Vector3::new(candidate.x / norm, candidate.y / norm, candidate.z / norm);
    if points.iter().any(|point| {
        Vector3::new(
            point.x - points[0].x,
            point.y - points[0].y,
            point.z - points[0].z,
        )
        .dot(normal)
        .abs()
            > extent * EPS_HOLE_POSITION
    }) {
        return None;
    }
    let axis = canonical_axis(normal);
    let axis = Vector3::new(
        if axis.x.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.x
        },
        if axis.y.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.y
        },
        if axis.z.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.z
        },
    );
    Some(
        points
            .into_iter()
            .map(|origin| HolePlacement::Axis { origin, axis })
            .collect(),
    )
}

/// Resolve hole axes from persistent identities of faces generated by the
/// operation. Each configuration lane must identify the same cylindrical
/// axes; local identities that name planar or secondary-diameter faces do not
/// participate.
pub(crate) fn project_generated_hole_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    face_identities: &[(String, u32, u32)],
    faces: &[Face],
    surfaces: &[Surface],
) {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| (value / AXIS_QUANTUM).round() as i64;
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let faces_by_id = faces
        .iter()
        .map(|face| (face.id.0.as_str(), face))
        .collect::<HashMap<_, _>>();
    let surfaces_by_id = surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), surface))
        .collect::<HashMap<_, _>>();

    for feature in features {
        let FeatureDefinition::Hole {
            placements,
            diameter: Some(Length(diameter)),
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if !placements.is_empty() || !diameter.is_finite() || *diameter <= 0.0 {
            continue;
        }
        let Some(source) = feature
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native))
            .and_then(|native| native.source_id.as_deref())
            .and_then(|source| source.parse::<u32>().ok())
        else {
            continue;
        };
        let radius = *diameter * 0.5;
        let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
        let mut lane_solutions = Vec::new();
        for lane in lanes {
            let local_identities = lane
                .generated_surface_identities
                .iter()
                .filter(|identity| identity.feature_source_id == source)
                .map(|identity| identity.local_identity)
                .collect::<HashSet<_>>();
            if local_identities.is_empty() {
                continue;
            }
            let mut axes = HashMap::<[i64; 6], HolePlacement>::new();
            for (face, face_source, local_identity) in face_identities {
                if *face_source != source || !local_identities.contains(local_identity) {
                    continue;
                }
                let Some(surface) = faces_by_id
                    .get(face.as_str())
                    .and_then(|face| surfaces_by_id.get(face.surface.0.as_str()))
                else {
                    continue;
                };
                let SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    radius: candidate_radius,
                    ..
                } = surface.geometry
                else {
                    continue;
                };
                if (candidate_radius - radius).abs() > radius_tolerance {
                    continue;
                }
                let axis = canonical_axis(axis);
                let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
                let closest = Point3::new(
                    origin.x - station * axis.x,
                    origin.y - station * axis.y,
                    origin.z - station * axis.z,
                );
                axes.entry([
                    quantize(closest.x),
                    quantize(closest.y),
                    quantize(closest.z),
                    quantize(axis.x),
                    quantize(axis.y),
                    quantize(axis.z),
                ])
                .or_insert(HolePlacement::Axis {
                    origin: closest,
                    axis,
                });
            }
            if axes.is_empty() {
                continue;
            }
            let mut solution = axes.into_iter().collect::<Vec<_>>();
            solution.sort_by_key(|(key, _)| *key);
            lane_solutions.push(
                solution
                    .into_iter()
                    .map(|(_, placement)| placement)
                    .collect::<Vec<_>>(),
            );
        }
        lane_solutions.sort_by_key(|solution| {
            solution
                .iter()
                .map(|placement| match placement {
                    HolePlacement::Axis { origin, axis } => [
                        quantize(origin.x),
                        quantize(origin.y),
                        quantize(origin.z),
                        quantize(axis.x),
                        quantize(axis.y),
                        quantize(axis.z),
                    ],
                    HolePlacement::Directed { .. } => [0; 6],
                })
                .collect::<Vec<_>>()
        });
        lane_solutions.dedup();
        if let [solution] = lane_solutions.as_slice() {
            placements.clone_from(solution);
        }
    }
}

/// Resolve placements from exact dimensional topology matches.
/// Counterbores require identical primary and counterbore axis sets. Flat
/// blind holes require a finite cylinder span equal to the declared depth.
/// Drilled holes additionally require a coaxial cone with the declared angle.
/// Ownership must be unique, or exact seed placements must partition the
/// remaining carrier set without a shared or unowned direction.
pub(crate) fn project_hole_topology_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    topology: &HoleTopology<'_>,
) {
    let diameter_counts = features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter_map(|feature| match feature.definition {
            FeatureDefinition::Hole {
                diameter: Some(Length(diameter)),
                ..
            } if diameter.is_finite() && diameter > 0.0 => Some(diameter.to_bits()),
            _ => None,
        })
        .fold(HashMap::<u64, usize>::new(), |mut counts, diameter| {
            *counts.entry(diameter).or_default() += 1;
            counts
        });
    let unresolved = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match &feature.definition {
            FeatureDefinition::Hole {
                placements,
                diameter: Some(Length(diameter)),
                ..
            } if placements.is_empty() && diameter.is_finite() && *diameter > 0.0 => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();

    for unresolved_index in unresolved {
        let FeatureDefinition::Hole {
            diameter: Some(Length(diameter)),
            ..
        } = features[unresolved_index].definition
        else {
            unreachable!("unresolved hole selection requires a finite diameter");
        };
        let Some(candidates) =
            counterbore_topology_candidates(&features[unresolved_index].definition, topology)
        else {
            continue;
        };
        if diameter_counts.get(&diameter.to_bits()) == Some(&1) {
            let FeatureDefinition::Hole { placements, .. } =
                &mut features[unresolved_index].definition
            else {
                unreachable!("unresolved hole selection requires a hole feature");
            };
            *placements = candidates;
            continue;
        }

        let siblings = features
            .iter()
            .enumerate()
            .filter(|(_, feature)| feature.suppressed != Some(true))
            .filter(|(_, feature)| {
                same_hole_construction(&features[unresolved_index].definition, &feature.definition)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if siblings.len() < 2
            || siblings
                .iter()
                .filter(|&&index| {
                    let FeatureDefinition::Hole { placements, .. } = &features[index].definition
                    else {
                        unreachable!("hole construction matching returned a non-hole feature");
                    };
                    placements.is_empty()
                })
                .count()
                != 1
        {
            continue;
        }

        let candidate_keys = candidates
            .iter()
            .filter_map(hole_axis_key)
            .collect::<HashSet<_>>();
        if candidate_keys.len() != candidates.len() {
            continue;
        }

        let mut claimed = HashSet::new();
        let mut complete = true;
        for sibling_index in siblings
            .iter()
            .copied()
            .filter(|&index| index != unresolved_index)
        {
            let FeatureDefinition::Hole { placements, .. } = &features[sibling_index].definition
            else {
                unreachable!("hole construction matching returned a non-hole feature");
            };
            if placements.is_empty() {
                complete = false;
                break;
            }
            for placement in placements {
                let Some(key) = hole_axis_key(placement) else {
                    complete = false;
                    break;
                };
                if !candidate_keys.contains(&key) || !claimed.insert(key) {
                    complete = false;
                    break;
                }
            }
            if !complete {
                break;
            }
        }
        if !complete || claimed.is_empty() {
            continue;
        }

        let residual = candidates
            .into_iter()
            .filter(|placement| hole_axis_key(placement).is_some_and(|key| !claimed.contains(&key)))
            .collect::<Vec<_>>();
        if residual.is_empty() {
            continue;
        }
        let FeatureDefinition::Hole { placements, .. } = &mut features[unresolved_index].definition
        else {
            unreachable!("unresolved hole selection requires a hole feature");
        };
        *placements = residual;
    }

    let cylinders = cylindrical_bore_face_spans(topology);
    project_flat_blind_topology_axes(features, &cylinders);
    project_drilled_hole_topology_axes(features, &cylinders, topology);
}

fn project_flat_blind_topology_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[(Point3, Vector3, f64, f64, bool)],
) {
    let unresolved = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match feature.definition {
            FeatureDefinition::Hole {
                placements: ref hole_placements,
                kind: HoleKind::Simple,
                diameter: Some(Length(diameter)),
                extent:
                    Some(Termination::Blind {
                        length: Length(length),
                    }),
                bottom: Some(HoleBottom::Flat),
                ..
            } if hole_placements.is_empty()
                && diameter.is_finite()
                && diameter > 0.0
                && length.is_finite()
                && length > 0.0 =>
            {
                Some((index, diameter, length))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for (index, diameter, length) in unresolved {
        if !hole_construction_is_unique(features, index) {
            continue;
        }
        let radius = diameter * 0.5;
        let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
        let length_tolerance = (length.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_POSITION);
        let Some(placements) = carrier_placements(cylinders.iter().filter_map(
            |(origin, axis, candidate_radius, candidate_span, _)| {
                ((candidate_radius - radius).abs() <= radius_tolerance
                    && (candidate_span - length).abs() <= length_tolerance)
                    .then_some((*origin, *axis))
            },
        )) else {
            continue;
        };
        let FeatureDefinition::Hole {
            placements: hole_placements,
            ..
        } = &mut features[index].definition
        else {
            unreachable!("flat blind topology selection requires a hole feature");
        };
        *hole_placements = placements;
    }
}

fn project_drilled_hole_topology_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[(Point3, Vector3, f64, f64, bool)],
    topology: &HoleTopology<'_>,
) {
    expand_seeded_drilled_hole_topology_axes(features, cylinders, topology);
    let unresolved = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match feature.definition {
            FeatureDefinition::Hole {
                placements: ref hole_placements,
                kind:
                    HoleKind::SimpleDrilled {
                        drill_point_angle: Angle(drill_point_angle),
                    },
                diameter: Some(Length(diameter)),
                extent:
                    Some(Termination::Blind {
                        length: Length(length),
                    }),
                bottom:
                    Some(HoleBottom::Angled {
                        included_angle: Angle(bottom_angle),
                        depth_to_tip: false,
                    }),
                ..
            } if hole_placements.is_empty()
                && diameter.is_finite()
                && diameter > 0.0
                && length.is_finite()
                && length > 0.0
                && drill_point_angle.is_finite()
                && drill_point_angle > 0.0
                && (bottom_angle - drill_point_angle).abs() <= EPS_HOLE_GEOMETRY =>
            {
                Some((index, diameter, length, drill_point_angle))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for (index, diameter, length, drill_point_angle) in unresolved {
        if !hole_construction_is_unique(features, index) {
            continue;
        }
        let Some(placements) = drilled_hole_topology_candidates(
            diameter,
            length,
            drill_point_angle,
            cylinders,
            topology.surfaces,
        ) else {
            continue;
        };
        let FeatureDefinition::Hole {
            placements: hole_placements,
            ..
        } = &mut features[index].definition
        else {
            unreachable!("drilled topology selection requires a hole feature");
        };
        *hole_placements = placements;
    }
}

fn drilled_hole_topology_candidates(
    diameter: f64,
    length: f64,
    drill_point_angle: f64,
    cylinders: &[(Point3, Vector3, f64, f64, bool)],
    surfaces: &[Surface],
) -> Option<Vec<HolePlacement>> {
    let radius = diameter * 0.5;
    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let length_tolerance = (length.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_POSITION);
    let cone_keys = surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cone {
                origin,
                axis,
                radius: candidate_radius,
                ratio,
                half_angle,
                ..
            } if (candidate_radius - radius).abs() <= radius_tolerance
                && (ratio - 1.0).abs() <= EPS_HOLE_GEOMETRY
                && (half_angle - drill_point_angle * 0.5).abs() <= EPS_HOLE_GEOMETRY =>
            {
                hole_axis_key(&HolePlacement::Axis { origin, axis })
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    if cone_keys.is_empty() {
        return None;
    }
    let placements = carrier_placements(cylinders.iter().filter_map(
        |(origin, axis, candidate_radius, candidate_span, _)| {
            ((candidate_radius - radius).abs() <= radius_tolerance
                && (candidate_span - length).abs() <= length_tolerance)
                .then_some((*origin, *axis))
        },
    ))?;
    let placements = placements
        .into_iter()
        .filter(|placement| hole_axis_key(placement).is_some_and(|key| cone_keys.contains(&key)))
        .collect::<Vec<_>>();
    (!placements.is_empty()).then_some(placements)
}

fn expand_seeded_drilled_hole_topology_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[(Point3, Vector3, f64, f64, bool)],
    topology: &HoleTopology<'_>,
) {
    let mut visited = HashSet::new();
    for index in 0..features.len() {
        if visited.contains(&index) || features[index].suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Hole {
            placements,
            kind:
                HoleKind::SimpleDrilled {
                    drill_point_angle: Angle(drill_point_angle),
                },
            diameter: Some(Length(diameter)),
            extent: Some(Termination::Blind {
                length: Length(length),
            }),
            bottom:
                Some(HoleBottom::Angled {
                    included_angle: Angle(bottom_angle),
                    depth_to_tip: false,
                }),
            ..
        } = &features[index].definition
        else {
            continue;
        };
        if placements.is_empty()
            || !diameter.is_finite()
            || *diameter <= 0.0
            || !length.is_finite()
            || *length <= 0.0
            || !drill_point_angle.is_finite()
            || *drill_point_angle <= 0.0
            || (bottom_angle - drill_point_angle).abs() > EPS_HOLE_GEOMETRY
        {
            continue;
        }
        let siblings = features
            .iter()
            .enumerate()
            .filter(|(_, feature)| feature.suppressed != Some(true))
            .filter(|(_, feature)| {
                same_hole_construction(&features[index].definition, &feature.definition)
            })
            .map(|(sibling, _)| sibling)
            .collect::<Vec<_>>();
        visited.extend(siblings.iter().copied());
        if siblings.len() < 2
            || siblings.iter().any(|&sibling| {
                matches!(
                    &features[sibling].definition,
                    FeatureDefinition::Hole { placements, .. } if placements.is_empty()
                )
            })
        {
            continue;
        }
        let candidates = drilled_hole_topology_candidates(
            *diameter,
            *length,
            *drill_point_angle,
            cylinders,
            topology.surfaces,
        )
        .and_then(|candidates| {
            unclaimed_seeded_hole_candidates(features, &siblings, *diameter, candidates)
        })
        .or_else(|| seeded_drilled_bore_candidates(features, &siblings, *diameter, topology));
        let Some(candidates) = candidates else {
            continue;
        };
        partition_seeded_hole_axes(features, &siblings, &candidates);
    }
}

fn seeded_drilled_bore_candidates(
    features: &[cadmpeg_ir::features::Feature],
    siblings: &[usize],
    diameter: f64,
    topology: &HoleTopology<'_>,
) -> Option<Vec<HolePlacement>> {
    let candidates = bore_carrier_placements(diameter * 0.5, topology)?;
    unclaimed_seeded_hole_candidates(features, siblings, diameter, candidates)
}

fn unclaimed_seeded_hole_candidates(
    features: &[cadmpeg_ir::features::Feature],
    siblings: &[usize],
    diameter: f64,
    candidates: Vec<HolePlacement>,
) -> Option<Vec<HolePlacement>> {
    let sibling_set = siblings.iter().copied().collect::<HashSet<_>>();
    let same_diameter = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match &feature.definition {
            FeatureDefinition::Hole {
                diameter: Some(Length(candidate)),
                placements,
                ..
            } if candidate.to_bits() == diameter.to_bits() => Some((index, placements)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if same_diameter
        .iter()
        .any(|(index, placements)| !sibling_set.contains(index) && placements.is_empty())
    {
        return None;
    }
    let claimed = same_diameter
        .iter()
        .filter(|(index, _)| !sibling_set.contains(index))
        .flat_map(|(_, placements)| placements.iter())
        .map(hole_axis_key)
        .collect::<Option<HashSet<_>>>()?;
    let candidates = candidates
        .into_iter()
        .filter(|placement| hole_axis_key(placement).is_some_and(|key| !claimed.contains(&key)))
        .collect::<Vec<_>>();
    (!candidates.is_empty()).then_some(candidates)
}

fn partition_seeded_hole_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    siblings: &[usize],
    candidates: &[HolePlacement],
) {
    let candidate_keys = candidates
        .iter()
        .filter_map(hole_axis_key)
        .collect::<HashSet<_>>();
    if candidate_keys.len() != candidates.len() {
        return;
    }
    let mut seed_directions: Vec<Vector3> = Vec::with_capacity(siblings.len());
    for &sibling in siblings {
        let FeatureDefinition::Hole { placements, .. } = &features[sibling].definition else {
            return;
        };
        let mut axes = placements.iter().filter_map(|placement| match placement {
            HolePlacement::Axis { axis, .. } => Some(canonical_axis(*axis)),
            HolePlacement::Directed { .. } => None,
        });
        let Some(direction) = axes.next() else {
            return;
        };
        if axes.any(|axis| axis.dot(direction) < 1.0 - EPS_HOLE_GEOMETRY)
            || placements.iter().any(|placement| {
                hole_axis_key(placement).is_none_or(|key| !candidate_keys.contains(&key))
            })
            || seed_directions
                .iter()
                .any(|candidate| candidate.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY)
        {
            return;
        }
        seed_directions.push(direction);
    }

    let Ok(mut partitions) = alloc_filled(
        siblings.len(),
        Vec::<HolePlacement>::new(),
        "SLDPRT seeded hole-axis partitions",
    ) else {
        return;
    };
    for placement in candidates {
        let HolePlacement::Axis { axis, .. } = placement else {
            return;
        };
        let direction = canonical_axis(*axis);
        let matches = seed_directions
            .iter()
            .enumerate()
            .filter(|(_, seed)| seed.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [partition] = matches.as_slice() else {
            return;
        };
        partitions[*partition].push(placement.clone());
    }
    if partitions.iter().any(Vec::is_empty) {
        return;
    }
    for (&sibling, partition) in siblings.iter().zip(partitions) {
        let FeatureDefinition::Hole { placements, .. } = &mut features[sibling].definition else {
            unreachable!("seed partition requires hole features");
        };
        *placements = partition;
    }
}

fn hole_construction_is_unique(features: &[cadmpeg_ir::features::Feature], index: usize) -> bool {
    features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter(|feature| same_hole_construction(&features[index].definition, &feature.definition))
        .count()
        == 1
}

fn counterbore_topology_candidates(
    definition: &FeatureDefinition,
    topology: &HoleTopology<'_>,
) -> Option<Vec<HolePlacement>> {
    let FeatureDefinition::Hole {
        diameter: Some(Length(diameter)),
        kind:
            HoleKind::Counterbore {
                diameter: Length(counterbore_diameter),
                ..
            }
            | HoleKind::CounterboreDrilled {
                diameter: Length(counterbore_diameter),
                ..
            },
        ..
    } = definition
    else {
        return None;
    };
    if !diameter.is_finite()
        || *diameter <= 0.0
        || !counterbore_diameter.is_finite()
        || *counterbore_diameter <= *diameter
    {
        return None;
    }
    let primary = cylindrical_surface_placements(*diameter * 0.5, topology.surfaces)?;
    let counterbores =
        cylindrical_surface_placements(*counterbore_diameter * 0.5, topology.surfaces)?;
    let primary_keys = primary
        .iter()
        .filter_map(hole_axis_key)
        .collect::<HashSet<_>>();
    let counterbore_keys = counterbores
        .iter()
        .filter_map(hole_axis_key)
        .collect::<HashSet<_>>();
    (primary_keys.len() == primary.len()
        && counterbore_keys.len() == counterbores.len()
        && primary_keys == counterbore_keys)
        .then_some(primary)
}

fn same_hole_construction(left: &FeatureDefinition, right: &FeatureDefinition) -> bool {
    let FeatureDefinition::Hole {
        kind: left_kind,
        exit_kind: left_exit_kind,
        diameter: left_diameter,
        extent: left_extent,
        bottom: left_bottom,
        taper_angle: left_taper_angle,
        specification: left_specification,
        allow_multi_profile_faces: left_allow_multi_profile_faces,
        ..
    } = left
    else {
        return false;
    };
    let FeatureDefinition::Hole {
        kind: right_kind,
        exit_kind: right_exit_kind,
        diameter: right_diameter,
        extent: right_extent,
        bottom: right_bottom,
        taper_angle: right_taper_angle,
        specification: right_specification,
        allow_multi_profile_faces: right_allow_multi_profile_faces,
        ..
    } = right
    else {
        return false;
    };
    left_kind == right_kind
        && left_exit_kind == right_exit_kind
        && left_diameter == right_diameter
        && left_extent == right_extent
        && left_bottom == right_bottom
        && left_taper_angle == right_taper_angle
        && left_specification == right_specification
        && left_allow_multi_profile_faces == right_allow_multi_profile_faces
}

fn hole_axis_key(placement: &HolePlacement) -> Option<[i64; 6]> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| (value / AXIS_QUANTUM).round() as i64;
    let HolePlacement::Axis { origin, axis } = placement else {
        return None;
    };
    let axis = canonical_axis(*axis);
    let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
    let closest = Point3::new(
        origin.x - station * axis.x,
        origin.y - station * axis.y,
        origin.z - station * axis.z,
    );
    Some([
        quantize(closest.x),
        quantize(closest.y),
        quantize(closest.z),
        quantize(axis.x),
        quantize(axis.y),
        quantize(axis.z),
    ])
}

fn cylindrical_support_normal(surface: &Surface, point: Point3) -> Option<Vector3> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = surface.geometry
    else {
        return None;
    };
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    let along = delta.dot(axis);
    let radial = Vector3::new(
        delta.x - along * axis.x,
        delta.y - along * axis.y,
        delta.z - along * axis.z,
    );
    let radial_length = radial.norm();
    let tolerance = (radius * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    ((radial_length - radius).abs() <= tolerance).then(|| {
        Vector3::new(
            radial.x / radial_length,
            radial.y / radial_length,
            radial.z / radial_length,
        )
    })
}

fn point_axis_distance_squared(point: Point3, origin: Point3, axis: Vector3) -> f64 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    let along = delta.x * axis.x + delta.y * axis.y + delta.z * axis.z;
    let across = Vector3::new(
        delta.x - along * axis.x,
        delta.y - along * axis.y,
        delta.z - along * axis.z,
    );
    across.x * across.x + across.y * across.y + across.z * across.z
}

pub(crate) struct HoleTopology<'a> {
    pub(crate) surfaces: &'a [Surface],
    pub(crate) faces: &'a [Face],
    pub(crate) loops: &'a [Loop],
    pub(crate) coedges: &'a [Coedge],
    pub(crate) edges: &'a [Edge],
    pub(crate) vertices: &'a [Vertex],
    pub(crate) points: &'a [Point],
}

fn direct_hole_position_feature<'a>(
    hole: &crate::records::Feature,
    histories: &'a [crate::records::FeatureHistory],
    model_sketches: &HashMap<String, cadmpeg_ir::sketches::SketchId>,
    sketch_entities: &[SketchEntity],
) -> Option<&'a crate::records::Feature> {
    let children = hole.properties.get("DissectableChildren");
    let history = histories
        .iter()
        .find(|history| history.features.iter().any(|feature| feature.id == hole.id))?;
    let mut direct_sketches = children
        .into_iter()
        .flat_map(|children| children.split(','))
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .filter_map(|source| {
            let mut matches = history.features.iter().filter(|candidate| {
                candidate.source_id.as_deref() == Some(source) || candidate.id == source
            });
            let child = matches.next()?;
            matches.next().is_none().then_some(child)
        })
        .filter(|child| classify(child) == Some(FeatureClass::Sketch))
        .collect::<Vec<_>>();
    direct_sketches.sort_by_key(|child| child.id.as_str());
    direct_sketches.dedup_by_key(|child| child.id.as_str());
    let is_axial_profile = |child: &crate::records::Feature| {
        model_sketches.get(&child.id).is_some_and(|sketch| {
            profiled_hole_construction_with_evidence(
                child,
                sketch,
                sketch_entities,
                ProfileEvidence::AxialTopology,
            )
            .is_some()
        })
    };
    let adjacent_position = || {
        let position_ordinal = hole.ordinal.checked_add(1)?;
        let profile_ordinal = hole.ordinal.checked_add(2)?;
        let unique_at = |ordinal| {
            let mut candidates = history.features.iter().filter(|candidate| {
                candidate.ordinal == ordinal
                    && classify(candidate) == Some(FeatureClass::Sketch)
                    && model_sketches.contains_key(&candidate.id)
            });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        };
        let position = unique_at(position_ordinal)?;
        let profile = unique_at(profile_ordinal)?;
        (!is_axial_profile(position) && is_axial_profile(profile)).then_some((position, profile))
    };
    match direct_sketches.as_slice() {
        [first, second] => match (is_axial_profile(first), is_axial_profile(second)) {
            (true, false) => Some(second),
            (false, true) => Some(first),
            _ => None,
        },
        [profile] => adjacent_position()
            .filter(|(_, adjacent_profile)| adjacent_profile.id == profile.id)
            .map(|(position, _)| position),
        [] => adjacent_position().map(|(position, _)| position),
        _ => None,
    }
}

pub(crate) fn project_hole_axes(
    model_features: &mut [cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    topology: &HoleTopology<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let surfaces = topology.surfaces;
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let model_sketches = model_features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.clone()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let hole_positions = native_features
        .values()
        .filter(|feature| classify(feature) == Some(FeatureClass::Hole))
        .filter_map(|hole| {
            Some((
                hole.id.as_str(),
                hole_position_feature(hole, histories, lanes).or_else(|| {
                    direct_hole_position_feature(hole, histories, &model_sketches, sketch_entities)
                })?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let position_features = hole_positions
        .values()
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let feature_ranges = lanes
        .iter()
        .map(|lane| {
            (
                lane.id.as_str(),
                feature_object_byte_ranges(histories, lane),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut feature_frames = HashMap::new();
    for lane in lanes {
        let Some(ranges) = feature_ranges.get(lane.id.as_str()) else {
            continue;
        };
        let plane_frames = lane_sketch_plane_frames(model_features, histories, lane);
        let plane_index = CompactReferencePlaneIndex::new(&lane.native_payload);
        for feature in native_features
            .values()
            .filter(|feature| position_features.contains(feature.id.as_str()))
        {
            let Some(&range) = ranges.get(feature.id.as_str()) else {
                continue;
            };
            let (context_start, start, end) = range;
            let Some(frame) = feature_input_sketch_frame(
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            ) else {
                continue;
            };
            feature_frames.insert((lane.id.as_str(), feature.id.as_str()), frame);
        }
    }
    let hole_diameter_counts = model_features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter_map(|feature| match feature.definition {
            FeatureDefinition::Hole {
                diameter: Some(Length(diameter)),
                ..
            } if diameter.is_finite() && diameter > 0.0 => Some(diameter.to_bits()),
            _ => None,
        })
        .fold(HashMap::<u64, usize>::new(), |mut counts, diameter| {
            *counts.entry(diameter).or_default() += 1;
            counts
        });

    for feature in model_features {
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Hole {
            placements,
            diameter: Some(Length(diameter)),
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if !placements.is_empty() || !diameter.is_finite() || *diameter <= 0.0 {
            continue;
        }
        let radius = *diameter / 2.0;
        let Some(native_feature) = feature
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native).copied())
        else {
            continue;
        };
        let Some(position_feature) = hole_positions.get(native_feature.id.as_str()).copied() else {
            continue;
        };
        let mut frames = lanes.iter().filter_map(|lane| {
            feature_frames
                .get(&(lane.id.as_str(), position_feature.id.as_str()))
                .copied()
        });
        if hole_diameter_counts.get(&diameter.to_bits()) == Some(&1) {
            if let Some(frame) = frames.next() {
                let same_frame = |candidate: (Point3, Vector3, Vector3)| {
                    frame.1.dot(candidate.1).abs() >= 1.0 - EPS_HOLE_GEOMETRY
                        && Vector3::new(
                            candidate.0.x - frame.0.x,
                            candidate.0.y - frame.0.y,
                            candidate.0.z - frame.0.z,
                        )
                        .dot(frame.1)
                        .abs()
                            <= EPS_HOLE_POSITION
                };
                if frames.all(same_frame) {
                    if let Some(bore_placements) =
                        plane_owned_bore_placements(frame.0, frame.1, radius, topology)
                    {
                        *placements = bore_placements;
                        continue;
                    }
                }
            }
            if let Some(bore_placements) = bore_carrier_placements(radius, topology) {
                *placements = bore_placements;
                continue;
            }
        }
        let mut solutions = Vec::new();
        for lane in lanes {
            let Some(&frame) =
                feature_frames.get(&(lane.id.as_str(), position_feature.id.as_str()))
            else {
                continue;
            };
            let relations = compact_position_relations(lane, position_feature.id.as_str());
            if relations.is_empty() {
                continue;
            }
            if let Some(solution) = constrained_bore_axes(frame, radius, surfaces, &relations) {
                solutions.push(solution);
            }
        }
        if solutions.is_empty() {
            for lane in lanes {
                let temporary_axis = feature_ranges
                    .get(lane.id.as_str())
                    .and_then(|ranges| ranges.get(position_feature.id.as_str()))
                    .and_then(|(_, start, end)| {
                        hole_temporary_axis(&lane.native_payload, *start, *end)
                    })
                    .map(|(_, direction)| direction);
                if let Some(solution) = marker_pattern_bore_axes(
                    lane,
                    position_feature.id.as_str(),
                    radius,
                    surfaces,
                    temporary_axis,
                ) {
                    solutions.push(solution);
                }
            }
        }
        solutions.sort_by_key(|placements| {
            placements
                .iter()
                .map(|placement| match placement {
                    HolePlacement::Axis { origin, axis } => [
                        origin.x.to_bits(),
                        origin.y.to_bits(),
                        origin.z.to_bits(),
                        axis.x.to_bits(),
                        axis.y.to_bits(),
                        axis.z.to_bits(),
                    ],
                    HolePlacement::Directed { .. } => [0; 6],
                })
                .collect::<Vec<_>>()
        });
        solutions.dedup();
        if let [solution] = solutions.as_slice() {
            placements.clone_from(solution);
        }
    }
}

fn cylindrical_bore_axes(radius: f64, topology: &HoleTopology<'_>) -> Vec<(Point3, Vector3)> {
    let surfaces = topology
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect::<HashMap<_, _>>();
    let tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let mut axes = topology
        .faces
        .iter()
        .filter(|face| face.sense == Sense::Reversed)
        .filter_map(|face| {
            let SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius: candidate,
                ..
            } = surfaces.get(&face.surface)?.geometry
            else {
                return None;
            };
            ((candidate - radius).abs() <= tolerance).then_some((origin, axis))
        })
        .collect::<Vec<_>>();
    axes.sort_by_key(|(origin, axis)| {
        [
            origin.x.to_bits(),
            origin.y.to_bits(),
            origin.z.to_bits(),
            axis.x.to_bits(),
            axis.y.to_bits(),
            axis.z.to_bits(),
        ]
    });
    axes.dedup();
    axes
}

fn plane_owned_bore_placements(
    plane_origin: Point3,
    plane_normal: Vector3,
    radius: f64,
    topology: &HoleTopology<'_>,
) -> Option<Vec<HolePlacement>> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| (value / AXIS_QUANTUM).round() as i64;
    let mut placements = cylindrical_bore_axes(radius, topology)
        .into_iter()
        .filter(|(_, axis)| axis.dot(plane_normal).abs() >= 1.0 - EPS_HOLE_GEOMETRY)
        .map(|(origin, axis)| {
            let station = Vector3::new(
                plane_origin.x - origin.x,
                plane_origin.y - origin.y,
                plane_origin.z - origin.z,
            )
            .dot(axis);
            HolePlacement::Axis {
                origin: Point3::new(
                    origin.x + station * axis.x,
                    origin.y + station * axis.y,
                    origin.z + station * axis.z,
                ),
                axis: plane_normal,
            }
        })
        .fold(
            HashMap::<[i64; 3], HolePlacement>::new(),
            |mut placements, placement| {
                let HolePlacement::Axis { origin, .. } = placement else {
                    unreachable!("bore carriers always produce axis placements");
                };
                placements
                    .entry([quantize(origin.x), quantize(origin.y), quantize(origin.z)])
                    .or_insert(placement);
                placements
            },
        )
        .into_iter()
        .collect::<Vec<_>>();
    placements.sort_by_key(|(key, _)| *key);
    let placements = placements
        .into_iter()
        .map(|(_, placement)| placement)
        .collect::<Vec<_>>();
    (!placements.is_empty()).then_some(placements)
}

fn bore_carrier_placements(radius: f64, topology: &HoleTopology<'_>) -> Option<Vec<HolePlacement>> {
    carrier_placements(cylindrical_bore_axes(radius, topology))
}

fn cylindrical_surface_placements(radius: f64, surfaces: &[Surface]) -> Option<Vec<HolePlacement>> {
    let tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    carrier_placements(surfaces.iter().filter_map(|surface| {
        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius: candidate,
            ..
        } = surface.geometry
        else {
            return None;
        };
        ((candidate - radius).abs() <= tolerance).then_some((origin, axis))
    }))
}

fn carrier_placements(
    axes: impl IntoIterator<Item = (Point3, Vector3)>,
) -> Option<Vec<HolePlacement>> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| (value / AXIS_QUANTUM).round() as i64;
    let mut carriers = axes
        .into_iter()
        .map(|(origin, axis)| {
            let axis = canonical_axis(axis);
            let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
            let closest = Point3::new(
                origin.x - station * axis.x,
                origin.y - station * axis.y,
                origin.z - station * axis.z,
            );
            (
                [
                    quantize(closest.x),
                    quantize(closest.y),
                    quantize(closest.z),
                    quantize(axis.x),
                    quantize(axis.y),
                    quantize(axis.z),
                ],
                HolePlacement::Axis {
                    origin: closest,
                    axis,
                },
            )
        })
        .collect::<HashMap<_, _>>()
        .into_iter()
        .collect::<Vec<_>>();
    carriers.sort_by_key(|(key, _)| *key);
    let placements = carriers
        .into_iter()
        .map(|(_, placement)| placement)
        .collect::<Vec<_>>();
    (!placements.is_empty()).then_some(placements)
}

fn cylindrical_bore_face_spans(
    topology: &HoleTopology<'_>,
) -> Vec<(Point3, Vector3, f64, f64, bool)> {
    let surfaces = topology
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect::<HashMap<_, _>>();
    let loops = topology
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = topology
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = topology
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = topology
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let points = topology
        .points
        .iter()
        .map(|point| (&point.id, point))
        .collect::<HashMap<_, _>>();
    topology
        .faces
        .iter()
        .filter_map(|face| {
            let surface = surfaces.get(&face.surface)?;
            let SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            } = surface.geometry
            else {
                return None;
            };
            let mut stations = face
                .loops
                .iter()
                .filter_map(|loop_id| loops.get(loop_id))
                .flat_map(|loop_| {
                    loop_
                        .coedges()
                        .iter()
                        .filter_map(|coedge_id| coedges.get(coedge_id))
                        .filter_map(|coedge| edges.get(&coedge.edge))
                        .flat_map(|edge| [&edge.start, &edge.end])
                        .chain(loop_.vertices())
                })
                .filter_map(|vertex_id| vertices.get(vertex_id))
                .filter_map(|vertex| points.get(&vertex.point))
                .map(|point| {
                    Vector3::new(
                        point.position.x - origin.x,
                        point.position.y - origin.y,
                        point.position.z - origin.z,
                    )
                    .dot(axis)
                });
            let first = stations.next()?;
            let (minimum, maximum) = stations
                .fold((first, first), |(minimum, maximum), station| {
                    (minimum.min(station), maximum.max(station))
                });
            let span = maximum - minimum;
            (radius.is_finite() && radius > 0.0 && span.is_finite() && span > 0.0).then_some((
                origin,
                axis,
                radius,
                span,
                face.sense == Sense::Reversed,
            ))
        })
        .collect()
}

pub(crate) fn project_topological_hole_constructions(
    features: &mut [cadmpeg_ir::features::Feature],
    topology: &HoleTopology<'_>,
) {
    let bore_faces = cylindrical_bore_face_spans(topology);
    for feature in features {
        let FeatureDefinition::Hole {
            placements,
            diameter,
            extent,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if placements.is_empty()
            || (diameter.is_some()
                && extent
                    .as_ref()
                    .is_some_and(|extent| !matches!(extent, Termination::Unresolved)))
        {
            continue;
        }
        let mut common = None::<Vec<(f64, f64)>>;
        for placement in placements.iter() {
            let HolePlacement::Axis {
                origin: placement_origin,
                axis: placement_axis,
            } = placement
            else {
                common = Some(Vec::new());
                break;
            };
            let mut candidates = bore_faces
                .iter()
                .filter_map(|(origin, axis, radius, span, reversed)| {
                    if !reversed {
                        return None;
                    }
                    let parallel = axis.dot(*placement_axis).abs() >= 1.0 - EPS_HOLE_GEOMETRY;
                    let distance = point_axis_distance_squared(*placement_origin, *origin, *axis);
                    (parallel && distance <= EPS_HOLE_EXACT_GEOMETRY).then_some((*radius, *span))
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                left.0
                    .total_cmp(&right.0)
                    .then_with(|| left.1.total_cmp(&right.1))
            });
            candidates.dedup_by(|left, right| {
                (left.0 - right.0).abs() <= EPS_HOLE_GEOMETRY
                    && (left.1 - right.1).abs() <= EPS_HOLE_GEOMETRY
            });
            common = Some(match common {
                None => candidates,
                Some(previous) => previous
                    .into_iter()
                    .filter(|candidate| {
                        candidates.iter().any(|other| {
                            (candidate.0 - other.0).abs() <= EPS_HOLE_GEOMETRY
                                && (candidate.1 - other.1).abs() <= EPS_HOLE_GEOMETRY
                        })
                    })
                    .collect(),
            });
        }
        let Some([(radius, depth)]) = common.as_deref() else {
            continue;
        };
        if diameter.is_none() {
            *diameter = Some(Length(radius * 2.0));
        }
        if extent
            .as_ref()
            .is_none_or(|extent| matches!(extent, Termination::Unresolved))
        {
            *extent = Some(Termination::Blind {
                length: Length(*depth),
            });
        }
    }
}

pub(crate) fn project_bore_backed_position_sketches(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    surfaces: &[Surface],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    struct Projection {
        feature: cadmpeg_ir::features::FeatureId,
        sketch: Sketch,
        entities: Vec<SketchEntity>,
    }

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let model_features = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let mut projections = Vec::new();
    for hole in features.iter() {
        let FeatureDefinition::Hole { placements, .. } = &hole.definition else {
            continue;
        };
        let axes = placements
            .iter()
            .filter_map(|placement| match placement {
                HolePlacement::Axis { origin, axis } => Some((*origin, *axis)),
                HolePlacement::Directed { .. } => None,
            })
            .collect::<Vec<_>>();
        if axes.len() != placements.len() || axes.is_empty() {
            continue;
        }
        let Some(native_hole) = hole
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native).copied())
        else {
            continue;
        };
        let Some(position) = hole_position_feature(native_hole, histories, lanes) else {
            continue;
        };
        let Some(position_feature) = model_features.get(position.id.as_str()) else {
            continue;
        };
        let Some(model_position) = features
            .iter()
            .find(|feature| feature.id == *position_feature)
        else {
            continue;
        };
        if !matches!(
            model_position.definition,
            FeatureDefinition::Sketch { sketch: None }
        ) {
            continue;
        }
        let canonical = canonical_axis(axes[0].1);
        if !axes
            .iter()
            .all(|(_, axis)| canonical_axis(*axis).dot(canonical) >= 1.0 - EPS_HOLE_GEOMETRY)
        {
            continue;
        }
        let mut frames = surfaces
            .iter()
            .filter_map(|surface| match surface.geometry {
                SurfaceGeometry::Plane {
                    origin,
                    normal,
                    u_axis,
                } if normal.dot(canonical).abs() >= 1.0 - EPS_HOLE_GEOMETRY
                    && axes.iter().all(|(point, _)| {
                        Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
                            .dot(normal)
                            .abs()
                            <= EPS_HOLE_POSITION
                    }) =>
                {
                    Some((origin, normal, u_axis))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        frames.sort_by_key(reference_plane_frame_key);
        frames.dedup_by_key(|frame| reference_plane_frame_key(frame));
        let [(origin, normal, u_axis)] = frames.as_slice() else {
            continue;
        };
        let mut owning_lanes = lanes
            .iter()
            .filter(|lane| {
                hole_position_sketch_source(native_hole, lane)
                    == position
                        .source_id
                        .as_deref()
                        .and_then(|source| source.parse::<u32>().ok())
            })
            .collect::<Vec<_>>();
        owning_lanes.sort_by_key(|lane| lane.id.as_str());
        owning_lanes.dedup_by_key(|lane| lane.id.as_str());
        let [lane] = owning_lanes.as_slice() else {
            continue;
        };
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let sketch_id = SketchId(format!(
            "sldprt:model:sketch#bore:{lane_key}:{}",
            position.ordinal
        ));
        let v_axis = normal.cross(*u_axis);
        let projected_entities = axes
            .iter()
            .enumerate()
            .map(|(ordinal, (point, _))| {
                let delta =
                    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
                SketchEntity::new(
                    SketchEntityId(format!("{}:entity:{ordinal}", sketch_id.0)),
                    sketch_id.clone(),
                    SketchGeometry::Point {
                        position: Point2::new(delta.dot(*u_axis), delta.dot(v_axis)),
                    },
                )
            })
            .collect();
        projections.push(Projection {
            feature: position_feature.clone(),
            sketch: Sketch {
                id: sketch_id,
                name: model_position.name.clone(),
                configuration: lane.configuration.clone(),
                visible: None,
                placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                },
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            },
            entities: projected_entities,
        });
    }
    for projection in projections {
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.id == projection.feature)
        else {
            continue;
        };
        let FeatureDefinition::Sketch { sketch, .. } = &mut feature.definition else {
            continue;
        };
        if sketch.is_some() {
            continue;
        }
        *sketch = Some(projection.sketch.id.clone());
        entities.extend(projection.entities);
        sketches.push(projection.sketch);
    }
}

fn marker_pattern_bore_axes(
    lane: &FeatureInputLane,
    feature: &str,
    radius: f64,
    surfaces: &[Surface],
    direction: Option<Vector3>,
) -> Option<Vec<HolePlacement>> {
    let paired_markers = paired_object_locus_markers(lane, feature);
    let paired_marker_ids = paired_markers
        .iter()
        .map(|marker| marker.id.as_str())
        .collect::<HashSet<_>>();
    let reduced_marker_ids = paired_markers
        .into_iter()
        .filter(|paired| {
            if paired.kind != SketchInputKind::Point {
                return true;
            }
            let Some([paired_u, paired_v]) = paired.coordinates_m else {
                return false;
            };
            !lane.sketch_entities.iter().any(|candidate| {
                candidate.id != paired.id
                    && candidate.feature_ref.as_deref() == Some(feature)
                    && candidate.object_index.is_some()
                    && candidate.coordinates_m.is_some_and(|[u, v]| {
                        same_dimension_length(paired_u * 1000.0, u * 1000.0)
                            && same_dimension_length(paired_v * 1000.0, v * 1000.0)
                    })
            })
        })
        .map(|marker| marker.id.as_str())
        .collect::<HashSet<_>>();
    let marker_loci = |paired: &HashSet<&str>| {
        let mut loci = lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
            .filter(|marker| marker.object_index.is_some())
            .filter(|marker| {
                matches!(
                    marker.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                ) || paired.contains(marker.id.as_str())
            })
            .filter_map(|marker| {
                let [u, v] = marker.coordinates_m?;
                Some(Point2::new(u * 1000.0, v * 1000.0))
            })
            .collect::<Vec<_>>();
        loci.sort_by(|left, right| {
            left.u
                .total_cmp(&right.u)
                .then_with(|| left.v.total_cmp(&right.v))
        });
        loci.dedup_by(|left, right| {
            same_dimension_length(left.u, right.u) && same_dimension_length(left.v, right.v)
        });
        loci
    };
    let curve_loci = marker_loci(&HashSet::new());
    let complete_loci = marker_loci(&paired_marker_ids);
    let reduced_loci = marker_loci(&reduced_marker_ids);
    match_marker_loci_to_bore_axes(&curve_loci, radius, surfaces, direction)
        .or_else(|| match_marker_loci_to_bore_axes(&complete_loci, radius, surfaces, direction))
        .or_else(|| {
            if reduced_loci == complete_loci {
                None
            } else {
                match_marker_loci_to_bore_axes(&reduced_loci, radius, surfaces, direction)
            }
        })
}

fn match_marker_loci_to_bore_axes(
    marker_loci: &[Point2],
    radius: f64,
    surfaces: &[Surface],
    direction: Option<Vector3>,
) -> Option<Vec<HolePlacement>> {
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    if marker_loci.is_empty() {
        return None;
    }

    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let quantize_scalar = |value: f64| (value / QUANTUM).round() as i64;
    let mut grouped = HashMap::<[i64; 3], HashMap<[i64; 3], Vec<(Point3, Vector3)>>>::new();
    for surface in surfaces {
        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius: candidate,
            ..
        } = surface.geometry
        else {
            continue;
        };
        if (candidate - radius).abs() > radius_tolerance {
            continue;
        }
        let canonical = canonical_axis(axis);
        let closest_distance = Vector3::new(origin.x, origin.y, origin.z).dot(canonical);
        let closest = Point3::new(
            origin.x - closest_distance * canonical.x,
            origin.y - closest_distance * canonical.y,
            origin.z - closest_distance * canonical.z,
        );
        grouped
            .entry([
                quantize_scalar(canonical.x),
                quantize_scalar(canonical.y),
                quantize_scalar(canonical.z),
            ])
            .or_default()
            .entry([
                quantize_scalar(closest.x),
                quantize_scalar(closest.y),
                quantize_scalar(closest.z),
            ])
            .or_default()
            .push((origin, axis));
    }

    let mut solutions = HashMap::<Vec<[i64; 6]>, Vec<HolePlacement>>::new();
    for lines in grouped.into_values() {
        let mut candidates = lines
            .into_iter()
            .filter_map(|(point, surfaces)| {
                let compare_origins = |left: &&(Point3, Vector3), right: &&(Point3, Vector3)| {
                    left.0
                        .x
                        .total_cmp(&right.0.x)
                        .then_with(|| left.0.y.total_cmp(&right.0.y))
                        .then_with(|| left.0.z.total_cmp(&right.0.z))
                };
                let (origin, axis) = match direction {
                    Some(expected) => surfaces
                        .iter()
                        .filter(|(_, axis)| expected.dot(*axis) >= 1.0 - EPS_HOLE_GEOMETRY)
                        .min_by(compare_origins)?,
                    None => surfaces.iter().min_by(compare_origins)?,
                };
                let axis = direction.map_or_else(|| canonical_axis(*axis), |_| *axis);
                Some((point, *origin, axis))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|(point, _, _)| *point);
        let candidate_loci = candidates
            .iter()
            .map(|([x, y, z], ..)| {
                Point3::new(
                    *x as f64 * QUANTUM,
                    *y as f64 * QUANTUM,
                    *z as f64 * QUANTUM,
                )
            })
            .collect::<Vec<_>>();
        if candidates.len() < marker_loci.len() {
            // A position sketch can retain construction curves that have no
            // current B-rep carrier. Accept the topology carrier set only
            // when it consumes one unique congruent subset of those curves.
            if !has_unique_marker_loci_subset(marker_loci, &candidate_loci) {
                continue;
            }
            let placements = candidates
                .iter()
                .map(|(_, origin, axis)| HolePlacement::Axis {
                    origin: *origin,
                    axis: *axis,
                })
                .collect::<Vec<_>>();
            let key = placements
                .iter()
                .map(|placement| match placement {
                    HolePlacement::Axis { origin, axis } => [
                        quantize_scalar(origin.x),
                        quantize_scalar(origin.y),
                        quantize_scalar(origin.z),
                        quantize_scalar(axis.x),
                        quantize_scalar(axis.y),
                        quantize_scalar(axis.z),
                    ],
                    HolePlacement::Directed { .. } => [0; 6],
                })
                .collect::<Vec<_>>();
            solutions.insert(key, placements);
            if solutions.len() > 1 {
                return None;
            }
            continue;
        }
        let mut subsets = HashSet::new();
        if congruent_bore_axis_subsets(
            0,
            marker_loci,
            &candidate_loci,
            &mut Vec::new(),
            &mut HashSet::new(),
            &mut subsets,
        ) {
            return None;
        }
        for subset in subsets {
            let placements = subset
                .iter()
                .map(|index| HolePlacement::Axis {
                    origin: candidates[*index].1,
                    axis: candidates[*index].2,
                })
                .collect::<Vec<_>>();
            let key = placements
                .iter()
                .map(|placement| match placement {
                    HolePlacement::Axis { origin, axis } => [
                        quantize_scalar(origin.x),
                        quantize_scalar(origin.y),
                        quantize_scalar(origin.z),
                        quantize_scalar(axis.x),
                        quantize_scalar(axis.y),
                        quantize_scalar(axis.z),
                    ],
                    HolePlacement::Directed { .. } => [0; 6],
                })
                .collect::<Vec<_>>();
            solutions.insert(key, placements);
            if solutions.len() > 1 {
                return None;
            }
        }
    }
    let solutions = solutions.into_values().collect::<Vec<_>>();
    let [solution] = solutions.as_slice() else {
        return None;
    };
    Some(solution.clone())
}

fn canonical_axis(axis: Vector3) -> Vector3 {
    let sign = [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > EPS_HOLE_EXACT_GEOMETRY)
        .map_or(1.0, f64::signum);
    Vector3::new(axis.x * sign, axis.y * sign, axis.z * sign)
}

fn congruent_bore_axis_subsets(
    marker_index: usize,
    marker_loci: &[Point2],
    candidate_loci: &[Point3],
    assigned: &mut Vec<usize>,
    used: &mut HashSet<usize>,
    subsets: &mut HashSet<Vec<usize>>,
) -> bool {
    if marker_index == marker_loci.len() {
        let mut subset = assigned.clone();
        subset.sort_unstable();
        subsets.insert(subset);
        return subsets.len() > 1;
    }
    for candidate_index in 0..candidate_loci.len() {
        if !used.insert(candidate_index) {
            continue;
        }
        let valid =
            assigned
                .iter()
                .copied()
                .enumerate()
                .all(|(previous_marker, previous_candidate)| {
                    let marker_distance = (marker_loci[marker_index].u
                        - marker_loci[previous_marker].u)
                        .hypot(marker_loci[marker_index].v - marker_loci[previous_marker].v);
                    let delta = Vector3::new(
                        candidate_loci[candidate_index].x - candidate_loci[previous_candidate].x,
                        candidate_loci[candidate_index].y - candidate_loci[previous_candidate].y,
                        candidate_loci[candidate_index].z - candidate_loci[previous_candidate].z,
                    );
                    same_dimension_length(marker_distance, delta.norm())
                });
        if valid {
            assigned.push(candidate_index);
            let ambiguous = congruent_bore_axis_subsets(
                marker_index + 1,
                marker_loci,
                candidate_loci,
                assigned,
                used,
                subsets,
            );
            assigned.pop();
            used.remove(&candidate_index);
            if ambiguous {
                return true;
            }
            continue;
        }
        used.remove(&candidate_index);
    }
    false
}

fn has_unique_marker_loci_subset(marker_loci: &[Point2], candidate_loci: &[Point3]) -> bool {
    fn collect_subsets(
        candidate_index: usize,
        marker_loci: &[Point2],
        candidate_loci: &[Point3],
        assigned: &mut Vec<usize>,
        used: &mut HashSet<usize>,
        subsets: &mut HashSet<Vec<usize>>,
    ) -> bool {
        if candidate_index == candidate_loci.len() {
            let mut subset = assigned.clone();
            subset.sort_unstable();
            subsets.insert(subset);
            return subsets.len() > 1;
        }
        for marker_index in 0..marker_loci.len() {
            if !used.insert(marker_index) {
                continue;
            }
            let valid = assigned.iter().copied().enumerate().all(
                |(previous_candidate, previous_marker)| {
                    let marker_delta = Vector3::new(
                        marker_loci[marker_index].u - marker_loci[previous_marker].u,
                        marker_loci[marker_index].v - marker_loci[previous_marker].v,
                        0.0,
                    );
                    let candidate_delta = Vector3::new(
                        candidate_loci[candidate_index].x - candidate_loci[previous_candidate].x,
                        candidate_loci[candidate_index].y - candidate_loci[previous_candidate].y,
                        candidate_loci[candidate_index].z - candidate_loci[previous_candidate].z,
                    );
                    same_dimension_length(marker_delta.norm(), candidate_delta.norm())
                },
            );
            if valid {
                assigned.push(marker_index);
                let ambiguous = collect_subsets(
                    candidate_index + 1,
                    marker_loci,
                    candidate_loci,
                    assigned,
                    used,
                    subsets,
                );
                assigned.pop();
                used.remove(&marker_index);
                if ambiguous {
                    return true;
                }
                continue;
            }
            used.remove(&marker_index);
        }
        false
    }

    if candidate_loci.is_empty() || candidate_loci.len() > marker_loci.len() {
        return false;
    }
    let mut subsets = HashSet::new();
    collect_subsets(
        0,
        marker_loci,
        candidate_loci,
        &mut Vec::new(),
        &mut HashSet::new(),
        &mut subsets,
    );
    subsets.len() == 1
}

pub(super) fn feature_object_byte_ranges<'a>(
    histories: &'a [crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> HashMap<&'a str, (usize, usize, usize)> {
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(offset, _)| *offset);
    objects
        .iter()
        .enumerate()
        .filter_map(|(index, (offset, feature))| {
            let start = usize::try_from(*offset).ok()?;
            let context_start = index
                .checked_sub(1)
                .and_then(|index| objects.get(index))
                .and_then(|(offset, _)| usize::try_from(*offset).ok())
                .unwrap_or(0);
            let end = objects
                .get(index + 1)
                .and_then(|(offset, _)| usize::try_from(*offset).ok())
                .unwrap_or(lane.native_payload.len());
            Some((feature.id.as_str(), (context_start, start, end)))
        })
        .collect()
}

fn hole_temporary_axis(payload: &[u8], start: usize, end: usize) -> Option<(Point3, Vector3)> {
    const DECLARATION: &[u8] = b"\xff\xff\x01\x00\x0f\x00moTempAxisRef_w";
    const HANDLE_PAIR: &[u8] = b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff";
    const NATIVE_TO_IR: f64 = 1000.0;

    let last_declaration = end.checked_sub(364)?;
    let mut axes = (start..=last_declaration).filter_map(|declaration| {
        if payload.get(declaration..declaration + DECLARATION.len()) != Some(DECLARATION)
            || payload.get(declaration + 267..declaration + 275) != Some(HANDLE_PAIR)
            || payload.get(declaration + 275..declaration + 279) != Some(&[0; 4])
            || View::u32_le_at(payload, declaration + 279).is_none_or(|address| address == 0)
            || payload.get(declaration + 283..declaration + 299) != Some(&[0; 16])
        {
            return None;
        }
        let scalar = |index: usize| {
            let offset = declaration + 299 + index * 8;
            let value = View::f64_le_at(payload, offset)?;
            value.is_finite().then_some(value)
        };
        let depth = scalar(0)?;
        let origin = Point3::new(
            scalar(1)? * NATIVE_TO_IR,
            scalar(2)? * NATIVE_TO_IR,
            scalar(3)? * NATIVE_TO_IR,
        );
        let direction = Vector3::new(scalar(4)?, scalar(5)?, scalar(6)?);
        let norm = direction.norm();
        let record_end = declaration + 355;
        let next_record = (record_end..=record_end + 24).find(|offset| {
            payload.get(record_end..*offset).is_some_and(|padding| {
                padding.iter().all(|byte| *byte == 0)
                    && (payload.get(*offset..*offset + 4) == Some(CLASS_MARKER)
                        || View::u16_le_at(payload, *offset).is_some_and(is_class_token))
            })
        })?;
        (depth > 0.0 && (norm - 1.0).abs() <= EPS_HOLE_GEOMETRY && next_record < end).then_some((
            origin,
            Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
        ))
    });
    let axis = axes.next()?;
    axes.all(|candidate| candidate == axis).then_some(axis)
}

pub(super) fn feature_input_sketch_frame(
    payload: &[u8],
    plane_frames: &HashMap<u32, SketchPlaneFrame>,
    plane_index: &CompactReferencePlaneIndex,
    context_start: usize,
    start: usize,
    end: usize,
) -> Option<(Point3, Vector3, Vector3)> {
    let reference = compact_profile_reference_plane_source(plane_index, context_start, start, end)
        .and_then(|source| plane_frames.get(&source).copied());
    let component = compact_profile_component_plane_frame(payload, context_start, start, end);
    let explicit = || {
        let (origin, normal, u_axis) = payload
            .get(start..end)
            .and_then(|object| explicit_reference_plane_frame(object).ok().flatten())?;
        let finite_zero = |value: f64| {
            if value.abs() <= EPS_HOLE_EXACT_GEOMETRY {
                0.0
            } else {
                value
            }
        };
        Some((
            Point3::new(
                finite_zero(origin.x),
                finite_zero(origin.y),
                finite_zero(origin.z),
            ),
            Vector3::new(
                finite_zero(normal.x),
                finite_zero(normal.y),
                finite_zero(normal.z),
            ),
            Vector3::new(
                finite_zero(u_axis.x),
                finite_zero(u_axis.y),
                finite_zero(u_axis.z),
            ),
        ))
    };
    match reference {
        Some(reference) => {
            let component = component
                .filter(|component| coplanar_plane_frames(reference.as_tuple(), *component));
            if reference.u_axis_source == SketchPlaneUAxisSource::ConstructedMidPlane {
                component.or_else(|| {
                    explicit().filter(|frame| coplanar_plane_frames(reference.as_tuple(), *frame))
                })
            } else {
                component
                    .or_else(|| Some(reference.as_tuple()))
                    .or_else(explicit)
            }
        }
        None => component.or_else(explicit),
    }
}

fn coplanar_plane_frames(
    reference: (Point3, Vector3, Vector3),
    candidate: (Point3, Vector3, Vector3),
) -> bool {
    let reference_normal_length = reference.1.norm();
    let candidate_normal_length = candidate.1.norm();
    if !reference_normal_length.is_finite()
        || !candidate_normal_length.is_finite()
        || reference_normal_length <= f64::EPSILON
        || candidate_normal_length <= f64::EPSILON
    {
        return false;
    }
    let normal_alignment = (reference.1.x * candidate.1.x
        + reference.1.y * candidate.1.y
        + reference.1.z * candidate.1.z)
        / (reference_normal_length * candidate_normal_length);
    if (normal_alignment.abs() - 1.0).abs() > EPS_HOLE_POSITION {
        return false;
    }
    let displacement = Vector3::new(
        candidate.0.x - reference.0.x,
        candidate.0.y - reference.0.y,
        candidate.0.z - reference.0.z,
    );
    let normal_distance = (displacement.x * reference.1.x
        + displacement.y * reference.1.y
        + displacement.z * reference.1.z)
        / reference_normal_length;
    let scale = reference
        .0
        .x
        .abs()
        .max(reference.0.y.abs())
        .max(reference.0.z.abs())
        .max(candidate.0.x.abs())
        .max(candidate.0.y.abs())
        .max(candidate.0.z.abs())
        .max(1.0);
    normal_distance.abs() <= EPS_HOLE_POSITION * scale
}

pub(super) fn sketch_feature_frames(
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> HashMap<String, (Point3, Vector3, Vector3)> {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag == "Sketch")
        .collect::<Vec<_>>();
    let mut candidates = HashMap::<String, Vec<(Point3, Vector3, Vector3)>>::new();
    for lane in lanes {
        let ranges = feature_object_byte_ranges(histories, lane);
        let plane_frames = lane_sketch_plane_frames(features, histories, lane);
        let plane_index = CompactReferencePlaneIndex::new(&lane.native_payload);
        for feature in &native_features {
            let Some(&(context_start, start, end)) = ranges.get(feature.id.as_str()) else {
                continue;
            };
            let Some(frame) = feature_input_sketch_frame(
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            ) else {
                continue;
            };
            candidates
                .entry(feature.id.clone())
                .or_default()
                .push(frame);
        }
    }
    candidates
        .into_iter()
        .filter_map(|(feature, mut frames)| {
            frames.sort_by_key(reference_plane_frame_key);
            frames.dedup();
            let [frame] = frames.as_slice() else {
                return None;
            };
            Some((feature, *frame))
        })
        .collect()
}

fn compact_position_relations(
    lane: &FeatureInputLane,
    feature: &str,
) -> Vec<(FeatureInputRelationFamily, u16, u16, f64)> {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    lane.relation_instances
        .iter()
        .filter(|relation| relation.feature_ref == feature)
        .filter_map(|relation| {
            let [first, second] = relation.operands.as_slice() else {
                return None;
            };
            if first.kind != FeatureInputOperandKind::Native(0x8152)
                || second.kind != FeatureInputOperandKind::Native(0x8152)
            {
                return None;
            }
            let scalar = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|id| scalars.get(id))?;
            (scalar.role == FeatureInputScalarRole::Driving
                && scalar.value.is_finite()
                && scalar.value >= 0.0)
                .then_some((
                    relation.family,
                    first.entity_index,
                    second.entity_index,
                    scalar.value * 1000.0,
                ))
        })
        .collect()
}

fn constrained_bore_axes(
    (origin, normal, u_axis): (Point3, Vector3, Vector3),
    radius: f64,
    surfaces: &[Surface],
    relations: &[(FeatureInputRelationFamily, u16, u16, f64)],
) -> Option<Vec<HolePlacement>> {
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    let v_axis = normal.cross(u_axis);
    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let mut axes = surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                origin: candidate,
                axis,
                radius: candidate_radius,
                ..
            } if (candidate_radius - radius).abs() <= radius_tolerance
                && axis.dot(normal).abs() >= 1.0 - EPS_HOLE_GEOMETRY =>
            {
                let delta = Vector3::new(
                    candidate.x - origin.x,
                    candidate.y - origin.y,
                    candidate.z - origin.z,
                );
                Some(quantize(
                    Point2::new(delta.dot(u_axis), delta.dot(v_axis)),
                    QUANTUM,
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    axes.sort_unstable();
    axes.dedup();
    if axes.is_empty() {
        return None;
    }
    let mut loci = Vec::with_capacity(axes.len() + 1);
    loci.push(Point2::new(0.0, 0.0));
    let mut bore_loci = HashSet::new();
    for (u, v) in axes {
        let point = Point2::new(u as f64 * QUANTUM, v as f64 * QUANTUM);
        let index = loci
            .iter()
            .position(|candidate| *candidate == point)
            .unwrap_or_else(|| {
                loci.push(point);
                loci.len() - 1
            });
        bore_loci.insert(index);
    }
    let indices = compact_position_loci(&loci, &bore_loci, relations)?;
    Some(
        indices
            .into_iter()
            .map(|index| {
                let point = loci[index];
                HolePlacement::Axis {
                    origin: Point3::new(
                        origin.x + point.u * u_axis.x + point.v * v_axis.x,
                        origin.y + point.u * u_axis.y + point.v * v_axis.y,
                        origin.z + point.u * u_axis.z + point.v * v_axis.z,
                    ),
                    axis: normal,
                }
            })
            .collect(),
    )
}

fn compact_position_loci(
    loci: &[Point2],
    placement_loci: &HashSet<usize>,
    relations: &[(FeatureInputRelationFamily, u16, u16, f64)],
) -> Option<Vec<usize>> {
    let mut nodes = relations
        .iter()
        .flat_map(|(_, first, second, _)| [*first, *second])
        .collect::<Vec<_>>();
    nodes.sort_unstable();
    nodes.dedup();
    if nodes.is_empty() || nodes.len() > loci.len() {
        return None;
    }
    let mut solution_sets = HashSet::<Vec<usize>>::new();
    for swap_axes in [false, true] {
        compact_position_assignments(
            0,
            &nodes,
            loci,
            relations,
            placement_loci,
            swap_axes,
            &mut HashMap::new(),
            &mut HashSet::new(),
            &mut solution_sets,
        );
    }
    let solution_sets = solution_sets.into_iter().collect::<Vec<_>>();
    let [solution] = solution_sets.as_slice() else {
        return None;
    };
    Some(solution.clone())
}

#[allow(clippy::too_many_arguments)]
fn compact_position_assignments(
    node_index: usize,
    nodes: &[u16],
    loci: &[Point2],
    relations: &[(FeatureInputRelationFamily, u16, u16, f64)],
    placement_loci: &HashSet<usize>,
    swap_axes: bool,
    assigned: &mut HashMap<u16, usize>,
    used: &mut HashSet<usize>,
    solutions: &mut HashSet<Vec<usize>>,
) {
    if solutions.len() > 1 {
        return;
    }
    if node_index == nodes.len() {
        let mut solution = used
            .iter()
            .copied()
            .filter(|index| placement_loci.contains(index))
            .collect::<Vec<_>>();
        if solution.is_empty() {
            return;
        }
        solution.sort_unstable();
        solutions.insert(solution);
        return;
    }
    let node = nodes[node_index];
    for locus_index in 0..loci.len() {
        if !used.insert(locus_index) {
            continue;
        }
        assigned.insert(node, locus_index);
        let valid = relations.iter().all(|(family, first, second, distance)| {
            let (Some(&first), Some(&second)) = (assigned.get(first), assigned.get(second)) else {
                return true;
            };
            let first = loci[first];
            let second = loci[second];
            let measured = match family {
                FeatureInputRelationFamily::PointPointDistance => {
                    (second.u - first.u).hypot(second.v - first.v)
                }
                FeatureInputRelationFamily::PointPointHorizontalDistance => {
                    if swap_axes {
                        (second.v - first.v).abs()
                    } else {
                        (second.u - first.u).abs()
                    }
                }
                FeatureInputRelationFamily::PointPointVerticalDistance => {
                    if swap_axes {
                        (second.u - first.u).abs()
                    } else {
                        (second.v - first.v).abs()
                    }
                }
                _ => return false,
            };
            same_dimension_length(measured, *distance)
        });
        if valid {
            compact_position_assignments(
                node_index + 1,
                nodes,
                loci,
                relations,
                placement_loci,
                swap_axes,
                assigned,
                used,
                solutions,
            );
        }
        assigned.remove(&node);
        used.remove(&locus_index);
    }
}

#[cfg(test)]
mod tests;
