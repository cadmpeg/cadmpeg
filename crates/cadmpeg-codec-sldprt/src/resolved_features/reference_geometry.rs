//! Reference plane and reference axis frames.

use super::compact_reference_planes::principal_sketch_frame;
use super::curves::sketch_plane_frames;
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::selections::component_face_reference_in_record;
use super::{CLASS_MARKER, NAME_MARKER};
use crate::classification::{native_object_class, principal_plane_with_siblings, NativeClassKind};
use crate::records::{FeatureInputLane, FeatureInputName};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Prefer an explicit basis when it agrees with the class-anchored plane constraint.
pub(super) fn reconcile_reference_plane_frame(
    explicit: Option<(Point3, Vector3, Vector3)>,
    constraint: Option<(Point3, Vector3, Vector3)>,
) -> Option<(Point3, Vector3, Vector3)> {
    let (Some(explicit), Some(constraint)) = (explicit, constraint) else {
        return explicit.or(constraint);
    };
    let explicit_distance =
        explicit.1.x * explicit.0.x + explicit.1.y * explicit.0.y + explicit.1.z * explicit.0.z;
    let alignment = explicit.1.x * constraint.1.x
        + explicit.1.y * constraint.1.y
        + explicit.1.z * constraint.1.z;
    let constraint_distance = constraint.1.x * constraint.0.x
        + constraint.1.y * constraint.0.y
        + constraint.1.z * constraint.0.z;
    if (alignment.abs() - 1.0).abs() <= 1.0e-9
        && (explicit_distance - alignment.signum() * constraint_distance).abs() <= 1.0e-9
    {
        Some(explicit)
    } else {
        Some(constraint)
    }
}

/// Add validated reference-plane frames to a projection copy of history.
pub(crate) fn enrich_history_reference_planes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut candidates = BTreeMap::<(usize, usize), Vec<(Point3, Vector3, Vector3)>>::new();
    let mut reference_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let mut reference_frame_candidates =
        BTreeMap::<(usize, usize), Vec<(Point3, Vector3, Vector3)>>::new();
    let mut explicit_reference_indices = HashSet::new();
    let mut face_feature_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let mut face_native_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let known_sources = histories
        .iter()
        .map(|history| {
            history
                .features
                .iter()
                .filter_map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    let features_by_source = histories
        .iter()
        .map(|history| {
            history
                .features
                .iter()
                .filter_map(|feature| {
                    Some((
                        feature.source_id.as_deref()?.parse::<u32>().ok()?,
                        feature.id.clone(),
                    ))
                })
                .collect::<HashMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let known_reference_plane_sources = histories
        .iter()
        .map(|history| {
            history
                .features
                .iter()
                .filter(|feature| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                        == NativeClassKind::ReferencePlane
                })
                .filter_map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    for lane in lanes {
        let mut starts =
            histories
                .iter()
                .enumerate()
                .flat_map(|(history_index, history)| {
                    history.features.iter().enumerate().filter_map(
                        move |(feature_index, feature)| {
                            feature_object_name(feature, lane)
                                .map(|name| (name.offset, history_index, feature_index))
                        },
                    )
                })
                .collect::<Vec<_>>();
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::ReferencePlane
                || principal_plane_with_siblings(feature, &histories[history_index].features)
                    .is_some()
                || feature.properties.contains_key("Origin")
                || feature.properties.contains_key("Normal")
                || feature.properties.contains_key("UAxis")
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .map_or(lane.native_payload.len(), |next| next.0 as usize);
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(bytes) = lane.native_payload.get(start..end) else {
                continue;
            };
            let self_source = feature
                .source_id
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok());
            if let Some(source) = offset_plane_reference_source(
                bytes,
                &known_sources[history_index],
                &known_reference_plane_sources[history_index],
                self_source,
            ) {
                explicit_reference_indices.insert((history_index, feature_index));
                reference_candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(source.to_string());
            }
            if let Some((relative_offset, owner)) = legacy_offset_plane_face_alias(bytes) {
                face_native_candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(format!(
                        "sldprt:feature-input:legacy-face-alias#{}:{}:{}",
                        lane.id,
                        start + relative_offset,
                        owner
                    ));
                if let Some(target) = features_by_source[history_index].get(&owner) {
                    face_feature_candidates
                        .entry((history_index, feature_index))
                        .or_default()
                        .push(target.clone());
                }
            }
            if let Some((relative_offset, components)) = component_face_reference_in_record(bytes) {
                face_native_candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(format!(
                        "sldprt:feature-input:surface-component-ids#{}:{}:{}",
                        lane.id,
                        start + relative_offset,
                        components
                            .iter()
                            .filter_map(|component| component.local_id)
                            .map(|local_id| local_id.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
            }
            let offset_frames = feature
                .parameters
                .get("D1")
                .and_then(|value| crate::history::parse_dimension_length_mm(value))
                .and_then(|distance| offset_reference_plane_frame_pair(bytes, distance));
            if let Some((offset, reference)) = offset_frames {
                reference_frame_candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(reference);
                candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(offset);
            }
            let constraint = constraint_midplane_frame(bytes);
            let mut anchored_frames = lane
                .classes
                .iter()
                .filter_map(|class| {
                    let offset = usize::try_from(class.offset).ok()?;
                    (start..end).contains(&offset).then(|| {
                        constraint_reference_plane_frame(&lane.native_payload, offset, &class.name)
                    })?
                })
                .collect::<Vec<_>>();
            anchored_frames.sort_by_key(reference_plane_frame_key);
            anchored_frames.dedup_by_key(|frame| reference_plane_frame_key(frame));
            let explicit = if offset_frames.is_some() {
                None
            } else if anchored_frames.is_empty() {
                match explicit_reference_plane_frame(bytes) {
                    Ok(frame) => frame,
                    Err(()) if constraint.is_some() => None,
                    Err(()) => continue,
                }
            } else {
                let [frame] = anchored_frames.as_slice() else {
                    continue;
                };
                Some(*frame)
            };
            let Some(frame) = reconcile_reference_plane_frame(explicit, constraint) else {
                continue;
            };
            let (origin, normal, u_axis) = frame;
            candidates
                .entry((history_index, feature_index))
                .or_default()
                .push((origin, normal, u_axis));
        }
    }
    let face_reference_indices = face_native_candidates
        .keys()
        .chain(face_feature_candidates.keys())
        .copied()
        .collect::<HashSet<_>>();
    for index in &face_reference_indices {
        reference_candidates.remove(index);
        explicit_reference_indices.remove(index);
    }
    for ((history_index, feature_index), mut native) in face_native_candidates {
        native.sort_unstable();
        native.dedup();
        if let [native] = native.as_slice() {
            histories[history_index].features[feature_index]
                .properties
                .insert("ReferenceFaceNative".into(), native.clone());
        }
    }
    for ((history_index, feature_index), mut targets) in face_feature_candidates {
        targets.sort_unstable();
        targets.dedup();
        if let [target] = targets.as_slice() {
            histories[history_index].features[feature_index]
                .properties
                .insert("ReferenceFaceFeature".into(), target.clone());
        }
    }
    let unique_frames = candidates
        .iter()
        .filter_map(|(index, frames)| {
            let mut frames = frames.clone();
            frames.sort_by_key(reference_plane_frame_key);
            frames.dedup();
            let [frame] = frames.as_slice() else {
                return None;
            };
            Some((*index, *frame))
        })
        .collect::<HashMap<_, _>>();
    let unique_reference_frames = reference_frame_candidates
        .iter()
        .filter_map(|(index, frames)| {
            let mut frames = frames.clone();
            frames.sort_by_key(reference_plane_frame_key);
            frames.dedup();
            let [frame] = frames.as_slice() else {
                return None;
            };
            Some((*index, *frame))
        })
        .collect::<HashMap<_, _>>();
    let mut frames_by_reference = histories
        .iter()
        .enumerate()
        .flat_map(|(history_index, history)| {
            history
                .features
                .iter()
                .enumerate()
                .filter_map(move |(feature_index, feature)| {
                    let reference = feature
                        .source_id
                        .clone()
                        .unwrap_or_else(|| feature.id.clone());
                    Some((
                        reference,
                        (history_index, feature_index),
                        principal_sketch_frame(principal_plane_with_siblings(
                            feature,
                            &history.features,
                        )?),
                    ))
                })
        })
        .collect::<Vec<_>>();
    frames_by_reference.extend(unique_frames.iter().map(|(index, frame)| {
        let feature = &histories[index.0].features[index.1];
        (
            feature
                .source_id
                .clone()
                .unwrap_or_else(|| feature.id.clone()),
            *index,
            *frame,
        )
    }));
    for (&index, frames) in &reference_frame_candidates {
        if reference_candidates.contains_key(&index) || face_reference_indices.contains(&index) {
            continue;
        }
        let mut sources = Vec::new();
        for &reference in frames {
            let matching = frames_by_reference
                .iter()
                .filter(|(_, candidate_index, candidate)| {
                    candidate_index.0 == index.0
                        && (candidate_index.1 < index.1
                            || principal_plane_with_siblings(
                                &histories[candidate_index.0].features[candidate_index.1],
                                &histories[candidate_index.0].features,
                            )
                            .is_some())
                        && offset_plane_reference_frame_matches(*candidate, reference, 0.0)
                })
                .collect::<Vec<_>>();
            let selected = select_reference_plane_frame_source(matching.iter().map(
                |(source, candidate_index, _)| {
                    (
                        source.as_str(),
                        candidate_index.1,
                        principal_plane_with_siblings(
                            &histories[candidate_index.0].features[candidate_index.1],
                            &histories[candidate_index.0].features,
                        )
                        .is_some(),
                    )
                },
            ));
            if let Some(source) = selected {
                sources.push(source);
            }
        }
        sources.sort_unstable();
        sources.dedup();
        if let [source] = sources.as_slice() {
            reference_candidates
                .entry(index)
                .or_default()
                .push(source.clone());
        }
    }
    for (&index, &frame) in &unique_frames {
        let feature = &histories[index.0].features[index.1];
        if !feature.parameters.contains_key("D1")
            || reference_candidates.contains_key(&index)
            || face_reference_indices.contains(&index)
        {
            continue;
        }
        let Some(distance) = feature
            .parameters
            .get("D1")
            .and_then(|value| crate::history::parse_dimension_length_mm(value))
        else {
            continue;
        };
        let matching = frames_by_reference
            .iter()
            .filter(|(_, candidate_index, candidate)| {
                candidate_index.0 == index.0
                    && offset_plane_reference_frame_matches(*candidate, frame, distance)
            })
            .collect::<Vec<_>>();
        if let Some(source) = select_reference_plane_frame_source(matching.iter().map(
            |(source, candidate_index, _)| {
                (
                    source.as_str(),
                    candidate_index.1,
                    principal_plane_with_siblings(
                        &histories[candidate_index.0].features[candidate_index.1],
                        &histories[candidate_index.0].features,
                    )
                    .is_some(),
                )
            },
        )) {
            reference_candidates.entry(index).or_default().push(source);
        }
    }
    for ((history_index, feature_index), mut sources) in reference_candidates {
        let index = (history_index, feature_index);
        if !explicit_reference_indices.contains(&index) {
            if let Some(offset) = unique_frames.get(&index) {
                if let Some(distance) = histories[history_index].features[feature_index]
                    .parameters
                    .get("D1")
                    .and_then(|value| crate::history::parse_dimension_length_mm(value))
                {
                    let compatible = sources
                        .iter()
                        .filter(|source| {
                            frames_by_reference.iter().any(
                                |(candidate_source, candidate_index, candidate)| {
                                    candidate_source == *source
                                        && candidate_index.0 == history_index
                                        && offset_plane_reference_frame_matches(
                                            *candidate, *offset, distance,
                                        )
                                },
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !compatible.is_empty() {
                        sources = compatible;
                    }
                }
            }
        }
        sources.sort_unstable();
        sources.dedup();
        let [source] = sources.as_slice() else {
            continue;
        };
        histories[history_index].features[feature_index]
            .properties
            .insert("Reference".into(), source.clone());
    }
    for ((history_index, feature_index), (origin, normal, u_axis)) in unique_reference_frames {
        let feature = &mut histories[history_index].features[feature_index];
        feature.properties.insert(
            "ReferenceFaceOrigin".into(),
            format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        );
        feature.properties.insert(
            "ReferenceFaceNormal".into(),
            format!("{},{},{}", normal.x, normal.y, normal.z),
        );
        feature.properties.insert(
            "ReferenceFaceUAxis".into(),
            format!("{},{},{}", u_axis.x, u_axis.y, u_axis.z),
        );
    }
    for ((history_index, feature_index), mut frames) in candidates {
        frames.sort_by_key(reference_plane_frame_key);
        frames.dedup();
        let [(origin, normal, u_axis)] = frames.as_slice() else {
            continue;
        };
        let feature = &mut histories[history_index].features[feature_index];
        feature.properties.insert(
            "Origin".into(),
            format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        );
        feature.properties.insert(
            "Normal".into(),
            format!("{},{},{}", normal.x, normal.y, normal.z),
        );
        feature.properties.insert(
            "UAxis".into(),
            format!("{},{},{}", u_axis.x, u_axis.y, u_axis.z),
        );
    }
}

pub(super) fn select_reference_plane_frame_source<'a>(
    candidates: impl Iterator<Item = (&'a str, usize, bool)>,
) -> Option<String> {
    let candidates = candidates.collect::<Vec<_>>();
    let mut principals = candidates
        .iter()
        .filter(|(_, _, principal)| *principal)
        .map(|(source, _, _)| *source)
        .collect::<Vec<_>>();
    principals.sort_unstable();
    principals.dedup();
    match principals.as_slice() {
        [source] => return Some((*source).to_string()),
        [] => {}
        _ => return None,
    }
    let latest_index = candidates.iter().map(|(_, index, _)| index).max()?;
    let mut latest = candidates
        .iter()
        .filter(|(_, index, _)| index == latest_index)
        .map(|(source, _, _)| *source)
        .collect::<Vec<_>>();
    latest.sort_unstable();
    latest.dedup();
    let [source] = latest.as_slice() else {
        return None;
    };
    Some((*source).to_string())
}

pub(super) fn offset_plane_reference_frame_matches(
    reference: (Point3, Vector3, Vector3),
    offset: (Point3, Vector3, Vector3),
    distance: f64,
) -> bool {
    let (reference_origin, reference_normal, _) = reference;
    let (offset_origin, offset_normal, _) = offset;
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let delta = Vector3::new(
        offset_origin.x - reference_origin.x,
        offset_origin.y - reference_origin.y,
        offset_origin.z - reference_origin.z,
    );
    let axial = dot(delta, reference_normal);
    let tangential = Vector3::new(
        delta.x - axial * reference_normal.x,
        delta.y - axial * reference_normal.y,
        delta.z - axial * reference_normal.z,
    );
    let tangential_length = dot(tangential, tangential).sqrt();
    (dot(reference_normal, offset_normal).abs() - 1.0).abs() <= 1.0e-9
        && tangential_length <= 1.0e-8
        && (axial.abs() - distance.abs()).abs() <= 1.0e-8
}

/// Resolve sketch-block definition ownership and placement from typed object records.
pub(crate) fn enrich_history_sketch_block_references(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for history in histories {
        let mut by_source = HashMap::<u32, Option<(usize, NativeClassKind)>>::new();
        for (feature_index, feature) in history.features.iter().enumerate() {
            let Some(source) = feature
                .source_id
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok())
            else {
                continue;
            };
            let identity = (
                feature_index,
                native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind,
            );
            by_source
                .entry(source)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(identity));
        }
        let mut candidates = HashMap::<usize, Vec<u32>>::new();
        let mut placement_candidates = HashMap::<usize, Vec<Point3>>::new();
        for lane in lanes {
            let mut names = lane.names.iter().collect::<Vec<_>>();
            names.sort_by_key(|name| name.offset);
            let instance_names = names
                .iter()
                .filter_map(|name| {
                    let source = name.object_id?;
                    let (feature_index, kind) = by_source.get(&source).and_then(|entry| *entry)?;
                    (kind == NativeClassKind::SketchBlockInstance).then_some((*name, feature_index))
                })
                .collect::<Vec<_>>();
            let mut definitions_by_local_id = HashMap::<u16, Option<u32>>::new();
            for (position, (name, _)) in instance_names.iter().enumerate() {
                let Some(next) = names.iter().find(|next| next.offset > name.offset) else {
                    continue;
                };
                let Some(definition_source) = next.object_id.filter(|source| {
                    by_source
                        .get(source)
                        .and_then(|entry| *entry)
                        .is_some_and(|(_, kind)| kind == NativeClassKind::SketchBlockDefinition)
                }) else {
                    continue;
                };
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(start) = usize::try_from(name.offset).ok() else {
                    continue;
                };
                let Some(local_id) = sketch_block_record_local_id(&lane.native_payload, start, end)
                else {
                    continue;
                };
                definitions_by_local_id
                    .entry(local_id)
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some(definition_source));
            }
            for (position, (name, instance_index)) in instance_names.iter().enumerate() {
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(start) = usize::try_from(name.offset).ok() else {
                    continue;
                };
                if let Some(origin) = sketch_block_record_origin(&lane.native_payload, start, end)
                    .or_else(|| {
                        sketch_block_identity_normalization_origin(&lane.native_payload, start, end)
                    })
                {
                    placement_candidates
                        .entry(*instance_index)
                        .or_default()
                        .push(origin);
                }
            }
            for pair in names.windows(2) {
                let (Some(instance_source), Some(definition_source)) =
                    (pair[0].object_id, pair[1].object_id)
                else {
                    continue;
                };
                let Some((instance_index, NativeClassKind::SketchBlockInstance)) =
                    by_source.get(&instance_source).and_then(|entry| *entry)
                else {
                    continue;
                };
                let Some((_, NativeClassKind::SketchBlockDefinition)) =
                    by_source.get(&definition_source).and_then(|entry| *entry)
                else {
                    continue;
                };
                candidates
                    .entry(instance_index)
                    .or_default()
                    .push(definition_source);
            }
            for (position, (name, instance_index)) in instance_names.iter().enumerate() {
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(local_id) = sketch_block_compact_local_id(&lane.native_payload, name, end)
                else {
                    continue;
                };
                let Some(definition_source) = definitions_by_local_id
                    .get(&local_id)
                    .and_then(|entry| *entry)
                else {
                    continue;
                };
                candidates
                    .entry(*instance_index)
                    .or_default()
                    .push(definition_source);
            }
        }
        for (feature_index, mut sources) in candidates {
            sources.sort_unstable();
            sources.dedup();
            let [source] = sources.as_slice() else {
                continue;
            };
            history.features[feature_index]
                .properties
                .insert("BlockDefinition".into(), source.to_string());
        }
        for (feature_index, mut origins) in placement_candidates {
            origins
                .sort_by_key(|origin| [origin.x.to_bits(), origin.y.to_bits(), origin.z.to_bits()]);
            origins.dedup();
            let [origin] = origins.as_slice() else {
                continue;
            };
            history.features[feature_index].properties.insert(
                "BlockOrigin".into(),
                format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
            );
        }
    }
}

fn sketch_block_record_identity(payload: &[u8], start: usize, end: usize) -> Option<(u16, usize)> {
    let bytes = payload.get(start..end)?;
    bytes
        .windows(44)
        .enumerate()
        .rev()
        .find_map(|(relative, window)| {
            let local_id = u16::from_le_bytes([window[18], window[19]]);
            (window.get(..4) == Some(&[0xff; 4])
                && window.get(12..18) == Some(&[0x02, 0, 0, 0, 0, 0])
                && local_id != 0
                && window.get(40..44) == Some(&[0, 0, 1, 0]))
            .then_some((local_id, start + relative))
        })
}

fn sketch_block_record_local_id(payload: &[u8], start: usize, end: usize) -> Option<u16> {
    sketch_block_record_identity(payload, start, end).map(|(local_id, _)| local_id)
}

pub(super) fn sketch_block_record_origin(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<Point3> {
    const ABSOLUTE_POINT_CLASS: &[u8] = b"moAbsolutePoint_c";
    const NATIVE_TO_IR: f64 = 1000.0;

    let (_, identity) = sketch_block_record_identity(payload, start, end)?;
    let body = identity.checked_add(44)?;
    if payload.get(body..body + CLASS_MARKER.len()) == Some(CLASS_MARKER)
        && payload.get(body + 4..body + 6)
            == Some(&(ABSOLUTE_POINT_CLASS.len() as u16).to_le_bytes())
        && payload.get(body + 6..body + 6 + ABSOLUTE_POINT_CLASS.len())
            == Some(ABSOLUTE_POINT_CLASS)
    {
        return Some(Point3::new(0.0, 0.0, 0.0));
    }
    let point_token = u16::from_le_bytes(payload.get(body..body + 2)?.try_into().ok()?);
    if point_token & 0x8000 == 0 || point_token == 0xffff {
        return None;
    }
    let scalar = |relative: usize| {
        let value = f64::from_le_bytes(
            payload
                .get(body + 2 + relative..body + 10 + relative)?
                .try_into()
                .ok()?,
        ) * NATIVE_TO_IR;
        (value.is_finite() && value.abs() <= 1.0e9).then_some(value)
    };
    Some(Point3::new(scalar(0)?, scalar(8)?, scalar(16)?))
}

pub(super) fn sketch_block_identity_normalization_origin(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<Point3> {
    const CLASS: &[u8] = b"sgBlock";
    const NATIVE_TO_IR: f64 = 1000.0;

    let bytes = payload.get(start..end)?;
    let record_len = CLASS_MARKER.len() + 2 + CLASS.len();
    let mut records = bytes
        .windows(record_len)
        .enumerate()
        .filter_map(|(relative, record)| {
            (record.get(..CLASS_MARKER.len()) == Some(CLASS_MARKER)
                && record.get(CLASS_MARKER.len()..CLASS_MARKER.len() + 2)
                    == Some(&(CLASS.len() as u16).to_le_bytes())
                && record.get(CLASS_MARKER.len() + 2..) == Some(CLASS))
            .then_some(start + relative + record_len)
        });
    let body = records.next()?;
    if records.next().is_some() {
        return None;
    }
    let scalar = |relative: usize| {
        let value = f64::from_le_bytes(
            payload
                .get(body + relative..body + relative + 8)?
                .try_into()
                .ok()?,
        );
        value.is_finite().then_some(value)
    };
    let basis = [
        scalar(72)?,
        scalar(80)?,
        scalar(88)?,
        scalar(96)?,
        scalar(104)?,
        scalar(112)?,
        scalar(120)?,
        scalar(128)?,
        scalar(136)?,
    ];
    if basis != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        || payload.get(body + 144..body + 152) != Some(&1_u64.to_le_bytes())
        || scalar(176)? != 1.0
    {
        return None;
    }
    let translation = [scalar(152)?, scalar(160)?, scalar(168)?];
    (translation.iter().all(|value| value.abs() <= 1.0e6)).then(|| {
        Point3::new(
            -translation[0] * NATIVE_TO_IR,
            -translation[1] * NATIVE_TO_IR,
            -translation[2] * NATIVE_TO_IR,
        )
    })
}

fn sketch_block_compact_local_id(
    payload: &[u8],
    name: &FeatureInputName,
    record_end: usize,
) -> Option<u16> {
    let name_start = usize::try_from(name.offset).ok()?;
    let name_end = name_start
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.value.encode_utf16().count().checked_mul(2)?)?;
    let header_start = name_end.checked_add(28)?;
    let header_end = header_start.checked_add(2)?;
    let header = u16::from_le_bytes(payload.get(header_start..header_end)?.try_into().ok()?);
    (sketch_block_record_local_id(payload, name_start, record_end)? == header).then_some(header)
}

/// Add the two serialized construction-plane operands to plane-intersection axes.
pub(crate) fn enrich_history_reference_axes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let known_sources = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
        .collect::<HashSet<_>>();
    for lane in lanes {
        let mut starts =
            histories
                .iter()
                .enumerate()
                .flat_map(|(history_index, history)| {
                    history.features.iter().enumerate().filter_map(
                        move |(feature_index, feature)| {
                            feature_object_name(feature, lane)
                                .map(|name| (name.offset, history_index, feature_index))
                        },
                    )
                })
                .collect::<Vec<_>>();
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::ReferenceAxis
                || feature.properties.contains_key("Planes")
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .map_or(lane.native_payload.len(), |next| next.0 as usize);
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(bytes) = lane.native_payload.get(start..end) else {
                continue;
            };
            let axis_data_classes = lane
                .classes
                .iter()
                .filter(|class| {
                    matches!(
                        class.name.as_str(),
                        "moPlaneInterAxisData_c" | "moSurfaceAxisData_c"
                    ) && usize::try_from(class.offset)
                        .is_ok_and(|offset| (start..end).contains(&offset))
                })
                .collect::<Vec<_>>();
            let mut anchored_frames = axis_data_classes
                .iter()
                .filter_map(|class| {
                    let body = usize::try_from(class.offset)
                        .ok()?
                        .checked_add(6 + class.name.len())?;
                    explicit_reference_axis_frame(lane.native_payload.get(body..body + 88)?)
                })
                .collect::<Vec<_>>();
            anchored_frames.sort_by_key(reference_axis_frame_key);
            anchored_frames.dedup_by_key(|frame| reference_axis_frame_key(frame));
            let explicit_frame = if axis_data_classes.is_empty() {
                explicit_reference_axis_frame(bytes)
            } else {
                let [frame] = anchored_frames.as_slice() else {
                    continue;
                };
                Some(*frame)
            };
            if let Some((origin, direction)) = explicit_frame {
                let feature = &mut histories[history_index].features[feature_index];
                feature.properties.insert(
                    "Origin".into(),
                    format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
                );
                feature.properties.insert(
                    "Direction".into(),
                    format!("{},{},{}", direction.x, direction.y, direction.z),
                );
                continue;
            }
            let Some([first, second]) = plane_intersection_axis_sources(bytes, &known_sources)
            else {
                continue;
            };
            histories[history_index].features[feature_index]
                .properties
                .insert("Planes".into(), format!("{first},{second}"));
        }
    }

    for history in histories.iter_mut() {
        for (axes, pairs) in legacy_reference_axis_triads(&history.features) {
            for (axis_index, planes) in axes.into_iter().zip(pairs) {
                let axis = &mut history.features[axis_index];
                axis.properties
                    .entry("Planes".into())
                    .or_insert_with(|| format!("{},{}", planes[0], planes[1]));
            }
        }
    }

    let projected = crate::history::project_features(histories);
    let plane_frames = sketch_plane_frames(&projected, histories);
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
            != NativeClassKind::ReferenceAxis
            || feature.properties.contains_key("Origin")
            || feature.properties.contains_key("Direction")
        {
            continue;
        }
        let Some([first, second]) = feature.properties.get("Planes").and_then(|planes| {
            let mut sources = planes.split(',').map(str::parse::<u32>);
            let pair = [sources.next()?.ok()?, sources.next()?.ok()?];
            sources.next().is_none().then_some(pair)
        }) else {
            continue;
        };
        let Some(frame) = plane_frames
            .get(&first)
            .zip(plane_frames.get(&second))
            .and_then(|(first, second)| plane_intersection_axis_frame(*first, *second))
        else {
            continue;
        };
        feature.properties.insert(
            "Origin".into(),
            format!("{}mm,{}mm,{}mm", frame.0.x, frame.0.y, frame.0.z),
        );
        feature.properties.insert(
            "Direction".into(),
            format!("{},{},{}", frame.1.x, frame.1.y, frame.1.z),
        );
    }
}

pub(super) fn explicit_reference_axis_frame(payload: &[u8]) -> Option<(Point3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const UNIT_TOLERANCE: f64 = 1.0e-9;
    const ORIGIN_ZERO_TOLERANCE_MM: f64 = 1.0e-9;
    const DIRECTION_ZERO_TOLERANCE: f64 = 1.0e-12;

    let scalar = |bytes: &[u8], offset: usize| {
        let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let mut candidates = payload
        .windows(88)
        .filter_map(|bytes| {
            let first = Vector3::new(scalar(bytes, 0)?, scalar(bytes, 8)?, scalar(bytes, 16)?);
            let second = Vector3::new(scalar(bytes, 24)?, scalar(bytes, 32)?, scalar(bytes, 40)?);
            let first_extent = scalar(bytes, 48)?;
            let second_extent = scalar(bytes, 56)?;
            let stored_direction =
                Vector3::new(scalar(bytes, 64)?, scalar(bytes, 72)?, scalar(bytes, 80)?);
            let delta = Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z);
            let delta_length = (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt();
            let direction_length = (stored_direction.x * stored_direction.x
                + stored_direction.y * stored_direction.y
                + stored_direction.z * stored_direction.z)
                .sqrt();
            if delta_length <= 1.0e-12
                || (direction_length - 1.0).abs() > UNIT_TOLERANCE
                || [first.x, first.y, first.z, second.x, second.y, second.z]
                    .into_iter()
                    .any(|value| value.abs() > 1.0e6)
            {
                return None;
            }
            let direction = Vector3::new(
                stored_direction.x / direction_length,
                stored_direction.y / direction_length,
                stored_direction.z / direction_length,
            );
            let aligned = (delta.x * direction.x + delta.y * direction.y + delta.z * direction.z)
                / delta_length;
            if aligned < 1.0 - UNIT_TOLERANCE {
                return None;
            }
            let projection = first.x * direction.x + first.y * direction.y + first.z * direction.z;
            let origin = Point3::new(
                (first.x - projection * direction.x) * NATIVE_TO_IR,
                (first.y - projection * direction.y) * NATIVE_TO_IR,
                (first.z - projection * direction.z) * NATIVE_TO_IR,
            );
            let canonical_zero =
                |value: f64, tolerance: f64| if value.abs() <= tolerance { 0.0 } else { value };
            let layout_rank = usize::from(
                first_extent <= 0.0
                    || second_extent <= 0.0
                    || !same_dimension_length(first_extent, second_extent),
            );
            Some((
                layout_rank,
                (
                    Point3::new(
                        canonical_zero(origin.x, ORIGIN_ZERO_TOLERANCE_MM),
                        canonical_zero(origin.y, ORIGIN_ZERO_TOLERANCE_MM),
                        canonical_zero(origin.z, ORIGIN_ZERO_TOLERANCE_MM),
                    ),
                    Vector3::new(
                        canonical_zero(direction.x, DIRECTION_ZERO_TOLERANCE),
                        canonical_zero(direction.y, DIRECTION_ZERO_TOLERANCE),
                        canonical_zero(direction.z, DIRECTION_ZERO_TOLERANCE),
                    ),
                ),
            ))
        })
        .collect::<Vec<_>>();
    let best_rank = candidates.iter().map(|(rank, _)| *rank).min()?;
    candidates.retain(|(rank, _)| *rank == best_rank);
    let mut candidates = candidates
        .into_iter()
        .map(|(_, frame)| frame)
        .collect::<Vec<_>>();
    candidates.sort_by_key(reference_axis_frame_key);
    candidates.dedup_by_key(|frame| reference_axis_frame_key(frame));
    let [frame] = candidates.as_slice() else {
        return None;
    };
    Some(*frame)
}

fn reference_axis_frame_key((origin, direction): &(Point3, Vector3)) -> [u64; 6] {
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        origin.z.to_bits(),
        direction.x.to_bits(),
        direction.y.to_bits(),
        direction.z.to_bits(),
    ]
}

pub(super) fn legacy_reference_axis_triads(
    features: &[crate::records::Feature],
) -> Vec<([usize; 3], [[u32; 2]; 3])> {
    let mut by_source = HashMap::<u32, Option<usize>>::new();
    for (index, feature) in features.iter().enumerate() {
        let Some(source) = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        by_source
            .entry(source)
            .and_modify(|index| *index = None)
            .or_insert(Some(index));
    }
    features
        .iter()
        .filter_map(|first| {
            let source = first.source_id.as_deref()?.parse::<u32>().ok()?;
            let indices = (0..6)
                .map(|offset| {
                    by_source
                        .get(&source.checked_add(offset)?)
                        .copied()
                        .flatten()
                })
                .collect::<Option<Vec<_>>>()?;
            let records = indices
                .iter()
                .map(|index| &features[*index])
                .collect::<Vec<_>>();
            let classes = records
                .iter()
                .map(|feature| {
                    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                })
                .collect::<Vec<_>>();
            if classes[..3]
                .iter()
                .any(|class| *class != NativeClassKind::ReferencePlane)
                || classes[3..]
                    .iter()
                    .any(|class| *class != NativeClassKind::ReferenceAxis)
            {
                return None;
            }
            let sources = records
                .iter()
                .map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
                .collect::<Option<Vec<_>>>()?;
            Some((
                [indices[3], indices[4], indices[5]],
                [
                    [sources[0], sources[1]],
                    [sources[0], sources[2]],
                    [sources[2], sources[1]],
                ],
            ))
        })
        .collect()
}

pub(super) fn plane_intersection_axis_frame(
    first: (Point3, Vector3, Vector3),
    second: (Point3, Vector3, Vector3),
) -> Option<(Point3, Vector3)> {
    let (first_origin, first_normal, _) = first;
    let (second_origin, second_normal, _) = second;
    let cross = |left: Vector3, right: Vector3| {
        Vector3::new(
            left.y * right.z - left.z * right.y,
            left.z * right.x - left.x * right.z,
            left.x * right.y - left.y * right.x,
        )
    };
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let direction = cross(first_normal, second_normal);
    let squared_length = dot(direction, direction);
    if !squared_length.is_finite() || squared_length <= 1.0e-18 {
        return None;
    }
    let first_distance = first_normal.x * first_origin.x
        + first_normal.y * first_origin.y
        + first_normal.z * first_origin.z;
    let second_distance = second_normal.x * second_origin.x
        + second_normal.y * second_origin.y
        + second_normal.z * second_origin.z;
    let first_term = cross(second_normal, direction);
    let second_term = cross(direction, first_normal);
    let origin = Point3::new(
        (first_distance * first_term.x + second_distance * second_term.x) / squared_length,
        (first_distance * first_term.y + second_distance * second_term.y) / squared_length,
        (first_distance * first_term.z + second_distance * second_term.z) / squared_length,
    );
    let length = squared_length.sqrt();
    let direction = Vector3::new(
        direction.x / length,
        direction.y / length,
        direction.z / length,
    );
    [
        origin.x,
        origin.y,
        origin.z,
        direction.x,
        direction.y,
        direction.z,
    ]
    .into_iter()
    .all(f64::is_finite)
    .then_some((origin, direction))
}

pub(super) fn plane_intersection_axis_sources(
    payload: &[u8],
    known_sources: &HashSet<u32>,
) -> Option<[u32; 2]> {
    const RECORD_LEN: usize = 46;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let mut sources = Vec::new();
    for source in payload.windows(RECORD_LEN).filter_map(|bytes| {
        let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
        (known_sources.contains(&source)
            && bytes.get(8..14)?.iter().all(|byte| *byte == 0)
            && bytes.get(14..16) == Some(&[1, 0])
            && bytes.get(16..22)?.iter().all(|byte| *byte == 0)
            && bytes.get(22).is_some_and(|object| *object != 0xff)
            && bytes.get(23..30)?.iter().all(|byte| *byte == 0)
            && matches!(bytes.get(30), Some(0 | 3))
            && bytes.get(31..38)?.iter().all(|byte| *byte == 0)
            && bytes.get(38..46) == Some(TERMINATOR))
        .then_some(source)
    }) {
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    let [first, second] = sources.as_slice() else {
        return None;
    };
    (first != second).then_some([*first, *second])
}

pub(super) fn compact_offset_plane_source(payload: &[u8]) -> Option<u32> {
    const TRAILER: &[u8] = &[
        0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x2d, 0x80, 0x2b, 0x80,
    ];
    let matches = payload.windows(4 + TRAILER.len()).filter_map(|bytes| {
        let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
        (source != 0 && bytes.get(4..) == Some(TRAILER)).then_some(source)
    });
    let matches = matches.collect::<HashSet<_>>();
    let mut matches = matches.into_iter();
    let source = matches.next()?;
    matches.next().is_none().then_some(source)
}

pub(super) fn structured_offset_plane_sources(payload: &[u8]) -> Vec<u32> {
    const RECORD_LEN: usize = 140;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    payload
        .windows(RECORD_LEN)
        .filter_map(|bytes| {
            let header = bytes.get(4..8)?;
            let identity = bytes.get(8..20)?;
            let link = bytes.get(28..32)?;
            let source = u32::from_le_bytes(bytes.get(44..48)?.try_into().ok()?);
            let address = bytes.get(116..120)?;
            (bytes.get(..4) == Some(&4u32.to_le_bytes())
                && header != [0; 4]
                && bytes.get(48..52) == Some(header)
                && identity != [0; 12]
                && bytes.get(32..44) == Some(identity)
                && bytes.get(52..64) == Some(identity)
                && bytes.get(76..88) == Some(identity)
                && bytes.get(20..28) == Some(&[0; 8])
                && link != [0; 4]
                && bytes.get(72..76) == Some(link)
                && bytes.get(64..68) == Some(&1u32.to_le_bytes())
                && bytes.get(68..72) == Some(&[0; 4])
                && bytes.get(88..92) == Some(&1u32.to_le_bytes())
                && bytes.get(92..108) == Some(&[0; 16])
                && bytes.get(108..112) == Some(&1u32.to_le_bytes())
                && bytes.get(112..116) == Some(&[0; 4])
                && address != [0; 4]
                && bytes.get(120..132) == Some(&[0; 12])
                && bytes.get(132..140) == Some(TERMINATOR))
            .then_some(source)
        })
        .collect()
}

pub(super) fn classed_offset_plane_sources(payload: &[u8]) -> Vec<u32> {
    const TRAILER: &[u8] = b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00";
    payload
        .windows(4 + TRAILER.len())
        .filter_map(|bytes| {
            let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
            (bytes.get(4..) == Some(TRAILER)).then_some(source)
        })
        .collect()
}

pub(super) fn offset_plane_reference_source(
    payload: &[u8],
    known_sources: &HashSet<u32>,
    known_reference_plane_sources: &HashSet<u32>,
    self_source: Option<u32>,
) -> Option<u32> {
    const RECORD_LEN: usize = 46;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let typed_sources = payload
        .windows(RECORD_LEN)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
            let signature = bytes.get(4..8)?;
            let selector = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?);
            (known_reference_plane_sources.contains(&source)
                && Some(source) != self_source
                && signature != [0; 4]
                && bytes.get(8..10) == Some(&[0, 0])
                && selector <= 3
                && bytes.get(14..18) == Some(&1u32.to_le_bytes())
                && bytes.get(18..22) == Some(&[0; 4])
                && bytes.get(26..38) == Some(&[0; 12])
                && bytes.get(38..46) == Some(TERMINATOR))
            .then_some((offset, source))
        })
        .collect::<Vec<_>>();
    let latest_offset = typed_sources.iter().map(|(offset, _)| *offset).max();
    let mut sources = typed_sources
        .into_iter()
        .filter_map(|(offset, source)| (Some(offset) == latest_offset).then_some(source))
        .collect::<Vec<_>>();
    sources.extend(
        compact_offset_plane_source(payload)
            .filter(|source| Some(*source) != self_source && known_sources.contains(source)),
    );
    sources.extend(
        structured_offset_plane_sources(payload)
            .into_iter()
            .filter(|source| Some(*source) != self_source && known_sources.contains(source)),
    );
    sources.extend(
        classed_offset_plane_sources(payload)
            .into_iter()
            .filter(|source| Some(*source) != self_source && known_sources.contains(source)),
    );
    sources.sort_unstable();
    sources.dedup();
    let [source] = sources.as_slice() else {
        return None;
    };
    Some(*source)
}

pub(super) fn legacy_offset_plane_face_alias(payload: &[u8]) -> Option<(usize, u32)> {
    const TERMINATOR: &[u8] = b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff";
    let mut aliases = payload
        .windows(115)
        .enumerate()
        .filter_map(|(offset, body)| {
            let token = u16::from_le_bytes(body[..2].try_into().ok()?);
            if token & 0x8000 == 0
                || token == u16::MAX
                || body[2..6] != 2u32.to_le_bytes()
                || body[6..42] != [0; 36]
                || body[42..45] != [0; 3]
                || body[45..61] != [0xff; 16]
                || body[61..69] != [0; 8]
                || body[69..73] != 2u32.to_le_bytes()
                || body[73..77] == [0; 4]
                || body[77..83] != [0, 0, 3, 0, 0, 0]
                || body[83..91] != [1, 0, 0, 0, 0, 0, 0, 0]
                || body[95..99] != [0; 4]
                || body[99..103] != 3u32.to_le_bytes()
                || body[103..107] != [0; 4]
                || &body[107..115] != TERMINATOR
            {
                return None;
            }
            let owner = u32::from_le_bytes(body[91..95].try_into().ok()?);
            (owner != 0 && owner != u32::MAX).then_some((offset, owner))
        })
        .collect::<Vec<_>>();
    aliases.sort_unstable();
    aliases.dedup();
    let [alias] = aliases.as_slice() else {
        return None;
    };
    Some(*alias)
}

pub(super) const FIXED_REFERENCE_PLANE_FRAME_LEN: usize = 97;
const MATRIX_REFERENCE_PLANE_FRAME_LEN: usize = 121;
pub(super) const MINIMAL_REFERENCE_PLANE_FRAME_LEN: usize = 81;
const COMPACT_REFERENCE_PLANE_FRAME_LEN: usize = 82;

pub(super) fn explicit_reference_plane_frame(
    payload: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3)>, ()> {
    // The complete matrix stores all three basis columns and a redundant normal.
    // Shorter layouts can occur as incidental aligned scalar runs later in the
    // same feature record, so they only participate when no matrix is present.
    if let Some(frame) = matrix_reference_plane_frame(payload) {
        return Ok(Some(frame));
    }
    let mut frames = payload
        .windows(FIXED_REFERENCE_PLANE_FRAME_LEN)
        .filter_map(fixed_reference_plane_frame)
        .collect::<Vec<_>>();
    if frames.is_empty() {
        frames.extend(angled_reference_plane_frame(payload));
        frames.extend(minimal_reference_plane_frame(payload));
        frames.extend(compact_reference_plane_frame(payload));
    }
    frames.sort_by_key(reference_plane_frame_key);
    frames.dedup_by(|left, right| left == right);
    match frames.as_slice() {
        [frame] => Ok(Some(*frame)),
        [] => Ok(None),
        _ => Err(()),
    }
}

pub(super) fn constraint_reference_plane_frame(
    payload: &[u8],
    class_offset: usize,
    class_name: &str,
) -> Option<(Point3, Vector3, Vector3)> {
    let body = class_offset.checked_add(6 + class_name.len())?;
    match class_name {
        "moConstraintPerpPlnTanOneCylinderRefplaneData_c" => {
            fixed_reference_plane_frame(payload.get(body..body + FIXED_REFERENCE_PLANE_FRAME_LEN)?)
        }
        "moFaceRefPlnData_c" => {
            fixed_reference_plane_frame(payload.get(body..body + FIXED_REFERENCE_PLANE_FRAME_LEN)?)
                .or_else(|| {
                    minimal_reference_plane_frame(
                        payload.get(body..body + MINIMAL_REFERENCE_PLANE_FRAME_LEN)?,
                    )
                })
        }
        "moConstraintPrllPlnTanOneCylinderRefplaneData_c" => minimal_reference_plane_frame(
            payload.get(body..body + MINIMAL_REFERENCE_PLANE_FRAME_LEN)?,
        ),
        _ => None,
    }
}

pub(super) fn reference_plane_frame_key(
    (origin, normal, u_axis): &(Point3, Vector3, Vector3),
) -> [u64; 9] {
    let canonical_bits = |value: f64| if value == 0.0 { 0 } else { value.to_bits() };
    [
        canonical_bits(origin.x),
        canonical_bits(origin.y),
        canonical_bits(origin.z),
        canonical_bits(normal.x),
        canonical_bits(normal.y),
        canonical_bits(normal.z),
        canonical_bits(u_axis.x),
        canonical_bits(u_axis.y),
        canonical_bits(u_axis.z),
    ]
}

pub(super) fn fixed_reference_plane_frame(bytes: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    if bytes.len() != FIXED_REFERENCE_PLANE_FRAME_LEN || bytes.get(48) != Some(&1) {
        return None;
    }
    let scalar = |offset| {
        let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let native_origin = [scalar(0)?, scalar(8)?, scalar(16)?];
    let origin = Point3::new(
        native_origin[0] * NATIVE_TO_IR,
        native_origin[1] * NATIVE_TO_IR,
        native_origin[2] * NATIVE_TO_IR,
    );
    let normal = Vector3::new(scalar(24)?, scalar(32)?, scalar(40)?);
    let u_axis = Vector3::new(scalar(49)?, scalar(57)?, scalar(65)?);
    let v_axis = Vector3::new(scalar(73)?, scalar(81)?, scalar(89)?);
    let norm =
        |vector: Vector3| (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    ([normal, u_axis, v_axis]
        .into_iter()
        .all(|vector| (norm(vector) - 1.0).abs() <= 1.0e-9)
        && dot(normal, u_axis).abs() <= 1.0e-9
        && dot(normal, v_axis).abs() <= 1.0e-9
        && dot(u_axis, v_axis).abs() <= 1.0e-9)
        .then_some((origin, normal, u_axis))
}

type ReferencePlaneFrame = (Point3, Vector3, Vector3);

pub(super) fn offset_reference_plane_frame_pair(
    payload: &[u8],
    distance: f64,
) -> Option<(ReferencePlaneFrame, ReferencePlaneFrame)> {
    let valid_pair = |result: ReferencePlaneFrame, reference: ReferencePlaneFrame| {
        (distance.is_finite() && offset_plane_reference_frame_matches(reference, result, distance))
            .then_some((result, reference))
    };
    let fixed = payload
        .windows(FIXED_REFERENCE_PLANE_FRAME_LEN)
        .filter_map(fixed_reference_plane_frame)
        .collect::<Vec<_>>();
    if let [result, reference] = fixed.as_slice() {
        return valid_pair(*result, *reference);
    }
    let matrix = matrix_reference_plane_frames(payload);
    if let [result, reference] = matrix.as_slice() {
        return valid_pair(*result, *reference);
    }
    let mut frames = Vec::new();
    for offset in 0..payload.len() {
        let candidates = [
            payload
                .get(offset..offset + FIXED_REFERENCE_PLANE_FRAME_LEN)
                .and_then(fixed_reference_plane_frame),
            payload
                .get(offset..offset + MATRIX_REFERENCE_PLANE_FRAME_LEN)
                .and_then(matrix_reference_plane_frame),
            payload
                .get(offset..offset + MINIMAL_REFERENCE_PLANE_FRAME_LEN)
                .and_then(minimal_reference_plane_frame),
            payload
                .get(offset..offset + COMPACT_REFERENCE_PLANE_FRAME_LEN)
                .and_then(compact_reference_plane_frame),
        ];
        for frame in candidates.into_iter().flatten() {
            if !frames.contains(&(offset, frame)) {
                frames.push((offset, frame));
            }
        }
    }
    let mut pairs = frames
        .iter()
        .enumerate()
        .flat_map(|(result_index, (result_offset, result))| {
            frames
                .iter()
                .skip(result_index + 1)
                .filter_map(move |(reference_offset, reference)| {
                    (result_offset < reference_offset)
                        .then(|| valid_pair(*result, *reference))
                        .flatten()
                })
        })
        .collect::<Vec<_>>();
    pairs.sort_by_key(|(result, reference)| {
        [
            reference_plane_frame_key(result),
            reference_plane_frame_key(reference),
        ]
    });
    pairs.dedup();
    let [pair] = pairs.as_slice() else {
        return None;
    };
    Some(*pair)
}

pub(super) fn constraint_midplane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const CLASS: &[u8] = b"moConstraintMidPlaneRefplaneData_c";
    const NATIVE_TO_IR: f64 = 1000.0;
    let record_len = CLASS_MARKER.len() + 2 + CLASS.len();
    let mut frames = payload
        .windows(record_len)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes.get(..CLASS_MARKER.len()) == Some(CLASS_MARKER)
                && bytes.get(CLASS_MARKER.len()..CLASS_MARKER.len() + 2)
                    == Some(&(CLASS.len() as u16).to_le_bytes())
                && bytes.get(CLASS_MARKER.len() + 2..) == Some(CLASS))
            .then_some(offset + record_len)
        })
        .filter_map(|body| {
            if payload.get(body..body + 8)?.iter().any(|byte| *byte != 0) {
                return None;
            }
            let scalar = |relative| {
                let value = f64::from_le_bytes(
                    payload
                        .get(body + relative..body + relative + 8)?
                        .try_into()
                        .ok()?,
                );
                value.is_finite().then_some(value)
            };
            let tolerance = scalar(8)?;
            if tolerance.abs() > 1.0e-9 {
                return None;
            }
            let distance = scalar(16)?;
            let normal = Vector3::new(scalar(24)?, scalar(32)?, scalar(40)?);
            let squared_norm = normal.x * normal.x + normal.y * normal.y + normal.z * normal.z;
            if (squared_norm - 1.0).abs() > 1.0e-9 {
                return None;
            }
            let reference = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs()
            {
                Vector3::new(1.0, 0.0, 0.0)
            } else if normal.y.abs() <= normal.z.abs() {
                Vector3::new(0.0, 1.0, 0.0)
            } else {
                Vector3::new(0.0, 0.0, 1.0)
            };
            let projection =
                reference.x * normal.x + reference.y * normal.y + reference.z * normal.z;
            let u_axis = Vector3::new(
                reference.x - projection * normal.x,
                reference.y - projection * normal.y,
                reference.z - projection * normal.z,
            );
            let u_length = (u_axis.x * u_axis.x + u_axis.y * u_axis.y + u_axis.z * u_axis.z).sqrt();
            let u_axis = Vector3::new(
                u_axis.x / u_length,
                u_axis.y / u_length,
                u_axis.z / u_length,
            );
            Some((
                Point3::new(
                    normal.x * distance * NATIVE_TO_IR,
                    normal.y * distance * NATIVE_TO_IR,
                    normal.z * distance * NATIVE_TO_IR,
                ),
                normal,
                u_axis,
            ))
        })
        .collect::<Vec<_>>();
    frames.sort_by_key(|(origin, normal, u_axis)| {
        [
            origin.x.to_bits(),
            origin.y.to_bits(),
            origin.z.to_bits(),
            normal.x.to_bits(),
            normal.y.to_bits(),
            normal.z.to_bits(),
            u_axis.x.to_bits(),
            u_axis.y.to_bits(),
            u_axis.z.to_bits(),
        ]
    });
    frames.dedup();
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

pub(super) fn angled_reference_plane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const RECORD_LEN: usize = 121;
    let scalar = |bytes: &[u8], relative| {
        let value = f64::from_le_bytes(bytes.get(relative..relative + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let norm =
        |vector: Vector3| (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let fixed_ranges = payload
        .windows(FIXED_REFERENCE_PLANE_FRAME_LEN)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            fixed_reference_plane_frame(bytes)
                .is_some()
                .then_some(offset..offset + FIXED_REFERENCE_PLANE_FRAME_LEN)
        })
        .collect::<Vec<_>>();
    let mut frames = payload
        .windows(RECORD_LEN)
        .enumerate()
        .filter(|(offset, _)| {
            let range = *offset..*offset + RECORD_LEN;
            fixed_ranges
                .iter()
                .all(|fixed| range.end <= fixed.start || range.start >= fixed.end)
        })
        .filter_map(|(_, bytes)| {
            if bytes.get(16) != Some(&1)
                || bytes.get(89..113)?.iter().any(|byte| *byte != 0)
                || scalar(bytes, 113)? != 1.0
            {
                return None;
            }
            let u_axis = Vector3::new(scalar(bytes, 17)?, scalar(bytes, 25)?, scalar(bytes, 33)?);
            let normal = Vector3::new(scalar(bytes, 41)?, scalar(bytes, 49)?, scalar(bytes, 57)?);
            let v_axis = Vector3::new(scalar(bytes, 65)?, scalar(bytes, 73)?, scalar(bytes, 81)?);
            if normal.x != 0.0
                || scalar(bytes, 0)?.to_bits() != normal.z.to_bits()
                || scalar(bytes, 8)?.to_bits() != normal.y.to_bits()
                || [u_axis, normal, v_axis]
                    .into_iter()
                    .any(|vector| (norm(vector) - 1.0).abs() > 1.0e-9)
                || dot(u_axis, normal).abs() > 1.0e-9
                || dot(u_axis, v_axis).abs() > 1.0e-9
                || dot(normal, v_axis).abs() > 1.0e-9
            {
                return None;
            }
            Some((Point3::new(0.0, 0.0, 0.0), normal, u_axis))
        })
        .collect::<Vec<_>>();
    frames.sort_by_key(|(_, normal, u_axis)| {
        [
            normal.x.to_bits(),
            normal.y.to_bits(),
            normal.z.to_bits(),
            u_axis.x.to_bits(),
            u_axis.y.to_bits(),
            u_axis.z.to_bits(),
        ]
    });
    frames.dedup();
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

pub(super) fn matrix_reference_plane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    let frames = matrix_reference_plane_frames(payload);
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

fn matrix_reference_plane_frames(payload: &[u8]) -> Vec<ReferencePlaneFrame> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = f64::from_le_bytes(bytes.get(relative..relative + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let norm =
        |vector: Vector3| (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let cross = |left: Vector3, right: Vector3| {
        Vector3::new(
            left.y * right.z - left.z * right.y,
            left.z * right.x - left.x * right.z,
            left.x * right.y - left.y * right.x,
        )
    };
    let frames = payload
        .windows(MATRIX_REFERENCE_PLANE_FRAME_LEN)
        .filter_map(|bytes| {
            if bytes[48] != 1 {
                return None;
            }
            let origin = Point3::new(
                scalar(bytes, 0)? * NATIVE_TO_IR,
                scalar(bytes, 8)? * NATIVE_TO_IR,
                scalar(bytes, 16)? * NATIVE_TO_IR,
            );
            let normal = Vector3::new(scalar(bytes, 24)?, scalar(bytes, 32)?, scalar(bytes, 40)?);
            let rows = [
                Vector3::new(scalar(bytes, 49)?, scalar(bytes, 57)?, scalar(bytes, 65)?),
                Vector3::new(scalar(bytes, 73)?, scalar(bytes, 81)?, scalar(bytes, 89)?),
                Vector3::new(scalar(bytes, 97)?, scalar(bytes, 105)?, scalar(bytes, 113)?),
            ];
            let u_axis = Vector3::new(rows[0].x, rows[1].x, rows[2].x);
            let v_axis = Vector3::new(rows[0].y, rows[1].y, rows[2].y);
            let matrix_normal = Vector3::new(rows[0].z, rows[1].z, rows[2].z);
            if [normal, u_axis, v_axis, matrix_normal]
                .into_iter()
                .any(|vector| (norm(vector) - 1.0).abs() > 1.0e-9)
                || dot(u_axis, v_axis).abs() > 1.0e-9
                || dot(u_axis, matrix_normal).abs() > 1.0e-9
                || dot(v_axis, matrix_normal).abs() > 1.0e-9
                || dot(normal, matrix_normal) < 1.0 - 1.0e-9
                || dot(cross(u_axis, v_axis), matrix_normal) < 1.0 - 1.0e-9
            {
                return None;
            }
            Some((origin, normal, u_axis))
        })
        .collect::<Vec<_>>();
    frames.into_iter().fold(Vec::new(), |mut unique, frame| {
        if !unique.contains(&frame) {
            unique.push(frame);
        }
        unique
    })
}

pub(super) fn minimal_reference_plane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = f64::from_le_bytes(bytes.get(relative..relative + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let mut frames = payload
        .windows(MINIMAL_REFERENCE_PLANE_FRAME_LEN)
        .filter_map(|bytes| {
            let origin = Point3::new(scalar(bytes, 0)?, scalar(bytes, 8)?, scalar(bytes, 16)?);
            let normal = Vector3::new(scalar(bytes, 24)?, scalar(bytes, 32)?, scalar(bytes, 40)?);
            let tail = [scalar(bytes, 57)?, scalar(bytes, 65)?, scalar(bytes, 73)?];
            if normal != Vector3::new(0.0, 0.0, 1.0)
                || bytes[48..56].iter().any(|byte| *byte != 0)
                || bytes[56] != 0x80
                || tail[0].to_bits() != (-0.0_f64).to_bits()
                || tail[1].to_bits() != (-origin.z).to_bits()
                || tail[2] != 1.0
            {
                return None;
            }
            Some((
                Point3::new(
                    origin.x * NATIVE_TO_IR,
                    origin.y * NATIVE_TO_IR,
                    origin.z * NATIVE_TO_IR,
                ),
                normal,
                Vector3::new(1.0, 0.0, 0.0),
            ))
        })
        .collect::<Vec<_>>();
    frames
        .sort_by_key(|(origin, _, _)| [origin.x.to_bits(), origin.y.to_bits(), origin.z.to_bits()]);
    frames.dedup();
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

pub(super) fn compact_reference_plane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = f64::from_le_bytes(bytes.get(relative..relative + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let cross = |left: Vector3, right: Vector3| {
        Vector3::new(
            left.y * right.z - left.z * right.y,
            left.z * right.x - left.x * right.z,
            left.x * right.y - left.y * right.x,
        )
    };
    let mut frames = payload
        .windows(COMPACT_REFERENCE_PLANE_FRAME_LEN)
        .filter(|bytes| bytes[64] == 0 && bytes[81] == 0)
        .flat_map(|bytes| {
            let Some(origin) = (|| {
                Some(Point3::new(
                    scalar(bytes, 0)? * NATIVE_TO_IR,
                    scalar(bytes, 8)? * NATIVE_TO_IR,
                    scalar(bytes, 16)? * NATIVE_TO_IR,
                ))
            })() else {
                return Vec::new();
            };
            let Some(normal_xy) = scalar(bytes, 24).zip(scalar(bytes, 32)) else {
                return Vec::new();
            };
            let Some(u_axis) = (|| {
                Some(Vector3::new(
                    scalar(bytes, 40)?,
                    scalar(bytes, 48)?,
                    scalar(bytes, 56)?,
                ))
            })() else {
                return Vec::new();
            };
            let Some(v_xy) = scalar(bytes, 65).zip(scalar(bytes, 73)) else {
                return Vec::new();
            };
            if (dot(u_axis, u_axis) - 1.0).abs() > 1.0e-9 {
                return Vec::new();
            }
            let remaining = 1.0 - v_xy.0 * v_xy.0 - v_xy.1 * v_xy.1;
            if remaining < -1.0e-9 {
                return Vec::new();
            }
            let omitted = remaining.max(0.0).sqrt();
            [omitted, -omitted]
                .into_iter()
                .filter_map(|v_z| {
                    let v_axis = Vector3::new(v_xy.0, v_xy.1, v_z);
                    let normal = cross(u_axis, v_axis);
                    (dot(u_axis, v_axis).abs() <= 1.0e-9
                        && (dot(normal, normal) - 1.0).abs() <= 1.0e-9
                        && (normal.x - normal_xy.0).abs() <= 1.0e-9
                        && (normal.y - normal_xy.1).abs() <= 1.0e-9)
                        .then_some((origin, normal, u_axis))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    frames.sort_by_key(|(origin, normal, u_axis)| {
        [
            origin.x.to_bits(),
            origin.y.to_bits(),
            origin.z.to_bits(),
            normal.x.to_bits(),
            normal.y.to_bits(),
            normal.z.to_bits(),
            u_axis.x.to_bits(),
            u_axis.y.to_bits(),
            u_axis.z.to_bits(),
        ]
    });
    frames.dedup();
    let [frame] = frames.as_slice() else {
        return None;
    };
    Some(*frame)
}

#[cfg(test)]
mod reference_geometry_tests;
