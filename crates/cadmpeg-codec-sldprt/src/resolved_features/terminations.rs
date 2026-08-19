//! Extrusion terminations, combine selections and sweep paths.

use super::component_paths::{
    component_path_features, component_path_terminal_feature, is_profile_feature_object,
};
use super::is_class_token;
use super::parameters::value_only_scalar_offset;
use super::scalars::feature_object_name;
use super::selections::{
    compact_general_curve_ref_at, compact_heterogeneous_component_path,
    compact_mixed_component_path, compact_profile_general_curve_ref_at,
    component_profile_source_at, component_reference_curve_path_at,
    declared_general_curve_profile_prefix, is_component_vector_selector,
    is_component_vector_selector_for_role, COMPACT_EDGE_VECTOR_MARKER,
};
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::FeatureDefinition;
use std::collections::HashMap;
use std::fmt::Write as _;

/// Add semantic termination forms carried by compact extrusion end-spec children.
#[derive(Clone)]
pub(super) struct TerminationVote {
    pub(super) condition: String,
    pub(super) reference: Option<String>,
    pub(super) second_condition: Option<String>,
    pub(super) reference_identity: Option<String>,
    pub(super) canonical_reference: Option<String>,
    pub(super) depth_m: Option<f64>,
}

pub(crate) fn enrich_history_extrusion_terminations(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut terminations = HashMap::<String, Vec<Option<TerminationVote>>>::new();
    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name))
            .collect::<HashMap<_, _>>();
        let blind_offsets = (0..lane.native_payload.len().saturating_sub(103))
            .filter(|offset| compact_extrusion_blind_at(&lane.native_payload, *offset))
            .collect::<Vec<_>>();
        let mut grouped_blind = HashMap::<String, Vec<TerminationVote>>::new();
        for &offset in &blind_offsets {
            let Some(scalar) = lane
                .scalars
                .iter()
                .filter(|scalar| u64::try_from(offset).is_ok_and(|offset| scalar.offset > offset))
                .min_by_key(|scalar| scalar.offset)
            else {
                continue;
            };
            let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                continue;
            };
            let owners = histories
                .iter()
                .flat_map(|history| &history.features)
                .filter(|feature| is_extrusion_end_spec_owner(feature))
                .filter(|feature| feature.parameters.len() == 1)
                .filter(|feature| {
                    feature
                        .parameters
                        .get(name.value.as_str())
                        .is_some_and(|value| {
                            crate::history::parse_dimension_length_mm(value).is_some_and(|value| {
                                (value - scalar.value * 1000.0).abs() <= 1.0e-9
                            })
                        })
                })
                .collect::<Vec<_>>();
            let [owner] = owners.as_slice() else {
                continue;
            };
            grouped_blind
                .entry(owner.id.clone())
                .or_default()
                .push(TerminationVote {
                    condition: "Blind".to_string(),
                    reference: None,
                    second_condition: None,
                    reference_identity: None,
                    canonical_reference: None,
                    depth_m: None,
                });
        }
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if !is_extrusion_end_spec_owner(feature) {
                continue;
            }
            let is_cosmetic_thread = |candidate: &crate::records::Feature| {
                native_object_class(candidate.input_class.as_deref().unwrap_or_default()).kind
                    == NativeClassKind::CosmeticThread
            };
            let has_depth =
                feature.parameters.contains_key("Depth") || feature.parameters.contains_key("D1");
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            // Cosmetic-thread children may be serialized between an extrusion
            // object and its end spec. Other following objects still delimit
            // the scan so a later feature cannot supply the termination.
            let end_spec_end = objects[index + 1..]
                .iter()
                .find(|(_, next_id)| {
                    if next_id == feature_id {
                        return false;
                    }
                    !histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .find(|candidate| candidate.id == *next_id)
                        .is_some_and(is_cosmetic_thread)
                })
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let mut end_index = index + 1;
            while let Some((_, next_id)) = objects.get(end_index) {
                if next_id == feature_id {
                    end_index += 1;
                    continue;
                }
                let skip = histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|feature| feature.id == *next_id)
                    .is_some_and(|feature| {
                        let class = feature.input_class.as_deref().unwrap_or_default();
                        is_profile_feature_object(feature)
                            || native_object_class(class).kind == NativeClassKind::CosmeticThread
                    });
                if !skip {
                    break;
                }
                end_index += 1;
            }
            let end = objects
                .get(end_index)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let candidates = (start..end_spec_end.saturating_sub(103))
                .filter_map(|offset| {
                    if compact_extrusion_blind_at(&lane.native_payload, offset) {
                        let depth_m = lane
                            .scalars
                            .iter()
                            .filter(|scalar| {
                                usize::try_from(scalar.offset)
                                    .is_ok_and(|scalar| scalar > offset && scalar < end)
                            })
                            .filter_map(|scalar| {
                                let name = names_by_id.get(scalar.name.as_str())?;
                                matches!(name.value.as_str(), "D1" | "Depth")
                                    .then_some((scalar, *name))
                            })
                            .filter(|(scalar, name)| {
                                value_only_scalar_offset(&lane.native_payload, name)
                                    == usize::try_from(scalar.offset).ok()
                            })
                            .min_by_key(|(scalar, _)| scalar.offset)
                            .map(|(scalar, _)| scalar.value);
                        return Some(TerminationVote {
                            condition: "Blind".to_string(),
                            reference: None,
                            second_condition: None,
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m,
                        });
                    }
                    if compact_extrusion_mid_plane_at(&lane.native_payload, offset) {
                        return Some(TerminationVote {
                            condition: "Symmetric".to_string(),
                            reference: None,
                            second_condition: None,
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m: None,
                        });
                    }
                    if let Some(reference) = compact_extrusion_offset_from_face_at(
                        &lane.native_payload,
                        offset,
                        end_spec_end,
                    ) {
                        return Some(compact_termination_face_vote(
                            "OffsetFromFace",
                            lane,
                            feature_id,
                            lane_key,
                            reference,
                        ));
                    }
                    if compact_extrusion_through_all_both_at(&lane.native_payload, offset) {
                        return Some(TerminationVote {
                            condition: "ThroughAllBoth".to_string(),
                            reference: None,
                            second_condition: None,
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m: None,
                        });
                    }
                    if has_depth
                        && compact_extrusion_blind_through_all_second_at(
                            &lane.native_payload,
                            offset,
                        )
                    {
                        return Some(TerminationVote {
                            condition: "Blind".to_string(),
                            reference: None,
                            second_condition: Some("ThroughAll".to_string()),
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m: None,
                        });
                    }
                    // One-sided through-all/through-next forms may retain a
                    // display dimension even when semantic D1/Depth already
                    // exists. Recognize their complete end spec before the
                    // depth guard, while malformed reference forms remain
                    // unresolved.
                    if compact_extrusion_through_all_at(&lane.native_payload, offset) {
                        Some(TerminationVote {
                            condition: "ThroughAll".to_string(),
                            reference: None,
                            second_condition: None,
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m: None,
                        })
                    } else if compact_extrusion_through_next_at(&lane.native_payload, offset) {
                        Some(TerminationVote {
                            condition: "ThroughNext".to_string(),
                            reference: None,
                            second_condition: None,
                            reference_identity: None,
                            canonical_reference: None,
                            depth_m: None,
                        })
                    } else if has_depth {
                        None
                    } else if let Some((reference, kind)) =
                        compact_extrusion_to_vertex_at(&lane.native_payload, offset, end_spec_end)
                    {
                        let prefix = match kind {
                            CompactPointReferenceKind::Point => "point-ref",
                            CompactPointReferenceKind::EdgeEndpoint => "edge-endpoint-ref",
                        };
                        let reference =
                            format!("sldprt:feature-input:{prefix}:{lane_key}:{reference}");
                        Some(TerminationVote {
                            condition: "ToVertex".to_string(),
                            reference_identity: Some(reference.clone()),
                            canonical_reference: None,
                            depth_m: None,
                            reference: Some(reference),
                            second_condition: None,
                        })
                    } else {
                        compact_extrusion_to_face_at(&lane.native_payload, offset, end_spec_end)
                            .map(|reference| {
                                compact_termination_face_vote(
                                    "ToFace", lane, feature_id, lane_key, reference,
                                )
                            })
                    }
                })
                .collect::<Vec<_>>();
            let grouped = grouped_blind.get(feature_id).and_then(|candidates| {
                let [candidate] = candidates.as_slice() else {
                    return None;
                };
                Some(candidate.clone())
            });
            terminations.entry(feature_id.clone()).or_default().push(
                candidates
                    .as_slice()
                    .first()
                    .cloned()
                    .filter(|_| candidates.len() == 1)
                    .or(grouped),
            );
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if feature.properties.contains_key("EndCondition") {
            continue;
        }
        let Some(votes) = terminations.get(&feature.id) else {
            continue;
        };
        let Some(vote) = consensus_termination_vote(votes) else {
            continue;
        };
        feature
            .properties
            .insert("EndCondition".into(), vote.condition.clone());
        if let Some(reference) = vote.reference {
            let key = if vote.condition == "ToVertex" {
                "Vertex"
            } else {
                "Face"
            };
            feature.properties.entry(key.into()).or_insert(reference);
        }
        if let Some(second) = &vote.second_condition {
            feature
                .properties
                .insert("EndCondition2".into(), second.clone());
        }
        if let Some(depth_m) = vote.depth_m {
            if !feature.parameters.contains_key("D1") && !feature.parameters.contains_key("Depth") {
                feature.parameters.insert(
                    "D1".into(),
                    crate::history::format_length_mm(depth_m * 1000.0),
                );
            }
        }
    }
}

pub(super) fn consensus_termination_vote(
    votes: &[Option<TerminationVote>],
) -> Option<TerminationVote> {
    let first = votes.first()?.as_ref()?;
    if !votes.iter().all(|vote| {
        vote.as_ref().is_some_and(|vote| {
            vote.condition == first.condition
                && vote.second_condition == first.second_condition
                && vote.reference_identity == first.reference_identity
                && vote.depth_m.map(f64::to_bits) == first.depth_m.map(f64::to_bits)
        })
    }) {
        return None;
    }
    let mut consensus = first.clone();
    if !votes
        .iter()
        .filter_map(Option::as_ref)
        .all(|vote| vote.reference == first.reference)
    {
        consensus.reference.clone_from(&first.canonical_reference);
    }
    Some(consensus)
}

fn compact_termination_face_vote(
    condition: &str,
    lane: &FeatureInputLane,
    feature_ref: &str,
    lane_key: &str,
    offset: usize,
) -> TerminationVote {
    let reference = format!("sldprt:feature-input:single-face-ref:{lane_key}:{offset}");
    let selection = lane.surface_selections.iter().find(|selection| {
        selection.feature_ref == feature_ref
            && usize::try_from(selection.offset).ok() == Some(offset)
    });
    let canonical_reference =
        selection.map(|selection| compact_surface_selection_value(&selection.components));
    let reference_identity = selection.map(|selection| {
        format!(
            "{}|{}|{}",
            canonical_reference.as_deref().unwrap_or_default(),
            selection.producer_feature_refs.join(","),
            selection
                .terminal_feature_ref
                .as_deref()
                .unwrap_or_default()
        )
    });
    let reference_identity = reference_identity.or_else(|| Some(reference.clone()));
    TerminationVote {
        condition: condition.to_string(),
        reference: Some(reference),
        second_condition: None,
        reference_identity,
        canonical_reference,
        depth_m: None,
    }
}

pub(super) fn is_extrusion_end_spec_owner(feature: &crate::records::Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
        == NativeClassKind::Extrusion
        || matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut")
}

/// Add target and tool body paths carried by compact combine objects.
pub(crate) fn enrich_history_combine_selections(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut selections = HashMap::<String, Vec<Option<(String, String, Option<String>)>>>::new();
    for lane in lanes {
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::Combine
            {
                continue;
            }
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            let end = objects
                .get(index + 1)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let paths = (start.saturating_add(12)
                ..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
                .filter_map(|marker| {
                    // Combine operands use the same type-3 vector framing as
                    // body selections, but a path may contain identifier-less
                    // lineage hops.  The component-path parser retains those
                    // hops; the local-id-only helper would reject the whole
                    // operand before projection.
                    compact_body_component_path_at(&lane.native_payload, marker).map(|_| marker)
                })
                .collect::<Vec<_>>();
            // A Combine object can carry auxiliary type-3 vectors between its two
            // operand paths; the outermost recognized paths are the operands.
            let selection = match (paths.first(), paths.last()) {
                (Some(&target), Some(&tools)) if target != tools => {
                    let operation = compact_combine_operation_at(&lane.native_payload, start);
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    Some((
                        format!("sldprt:feature-input:body-path:{lane_key}:{target}"),
                        format!("sldprt:feature-input:body-path:{lane_key}:{tools}"),
                        operation.map(str::to_string),
                    ))
                }
                _ => None,
            };
            selections
                .entry(feature_id.clone())
                .or_default()
                .push(selection);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(votes) = selections.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if !votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            continue;
        }
        feature
            .properties
            .entry("Target".into())
            .or_insert_with(|| first.0.clone());
        feature
            .properties
            .entry("Tools".into())
            .or_insert_with(|| first.1.clone());
        if let Some(operation) = &first.2 {
            feature
                .properties
                .entry("Operation".into())
                .or_insert_with(|| operation.clone());
        }
    }
}

pub(super) fn compact_combine_operation_at(
    payload: &[u8],
    name_offset: usize,
) -> Option<&'static str> {
    let name_prefix = payload.get(name_offset..name_offset.checked_add(5)?)?;
    let name_token = View::u16_le_at(name_prefix, 0)?;
    if !is_class_token(name_token) || name_prefix[2..] != [0xff, 0xfe, 0xff] {
        return None;
    }
    let name_units = usize::from(*payload.get(name_offset.checked_add(5)?)?);
    let operation = name_offset.checked_add(117usize.checked_add(name_units.checked_mul(2)?)?)?;
    let operation_end = operation.checked_add(4)?;
    let standard_tail = payload
        .get(operation_end..operation_end.checked_add(10)?)
        .is_some_and(|tail| tail == [0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    let alternate_tail = payload
        .get(operation_end..operation_end.checked_add(6)?)
        .is_some_and(|tail| tail == [0, 0, 0xff, 0xff, 0xff, 0xff]);
    if payload
        .get(operation - 12..operation)?
        .iter()
        .any(|byte| *byte != 0)
        || !(standard_tail || alternate_tail)
    {
        return None;
    }
    match View::u32_le_at(payload, operation)? {
        0 => Some("Join"),
        1 => Some("Cut"),
        2 => Some("Intersect"),
        _ => None,
    }
}

/// Add compact general-curve reference identities carried by solid sweeps.
pub(crate) fn enrich_history_sweep_paths(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut paths = HashMap::<String, Vec<Option<String>>>::new();
    for lane in lanes {
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, &(start, ref feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if !matches!(
                native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind,
                NativeClassKind::Sweep | NativeClassKind::SweepReferenceSurface
            ) || feature.properties.contains_key("Path")
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|object| usize::try_from(object.0).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let declared = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moGeneralCurveRef_w"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| offset >= start && offset < end)
                })
                .filter_map(|class| usize::try_from(class.offset).ok())
                .collect::<Vec<_>>();
            let compact = (start..end.saturating_sub(16))
                .filter(|offset| compact_general_curve_ref_at(&lane.native_payload, *offset))
                .collect::<Vec<_>>();
            let compact_profiles = (start..end.saturating_sub(16))
                .filter(|offset| {
                    compact_profile_general_curve_ref_at(&lane.native_payload, *offset)
                })
                .collect::<Vec<_>>();
            let mut source_candidates = declared
                .iter()
                .filter_map(|offset| {
                    declared_general_curve_profile_prefix(&lane.native_payload, *offset)
                })
                .chain(compact_profiles.iter().map(|offset| offset + 6))
                .filter_map(|prefix| component_profile_source_at(&lane.native_payload, prefix))
                .collect::<Vec<_>>();
            source_candidates.sort_unstable();
            source_candidates.dedup();
            let path = if let [source] = source_candidates.as_slice() {
                Some(source.to_string())
            } else {
                let mut candidates = declared;
                candidates.extend(compact);
                candidates.extend(compact_profiles);
                candidates.sort_unstable();
                candidates.dedup();
                if let [offset] = candidates.as_slice() {
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    Some(format!(
                        "sldprt:feature-input:general-curve-ref:{lane_key}:{offset}"
                    ))
                } else {
                    None
                }
            };
            paths.entry(feature_id.clone()).or_default().push(path);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if feature.properties.contains_key("Path") {
            continue;
        }
        let Some(votes) = paths.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            feature.properties.insert("Path".into(), first.clone());
        }
    }
}

/// Bind reference-curve cross sections consumed by surface sweeps.
pub(crate) fn project_surface_sweep_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    use cadmpeg_ir::features::{GeneratedCurveRef, ProfileRef};

    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let mut projections = HashMap::new();
    for lane in lanes {
        let Some(reference_class) = lane
            .classes
            .iter()
            .find(|class| class.name == "moCompReferenceCurve_c")
        else {
            continue;
        };
        let Some(class_offset) = usize::try_from(reference_class.offset).ok() else {
            continue;
        };
        let Some(wrapper_token) = class_offset
            .checked_sub(2)
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
        else {
            continue;
        };
        let wrapper_token = [wrapper_token[0], wrapper_token[1]];
        let declared_prefix = class_offset.checked_add(6 + reference_class.name.len());
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let mut objects = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, &(start, feature)) in objects.iter().enumerate() {
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::SweepReferenceSurface
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|(offset, _)| usize::try_from(*offset).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let direct = declared_prefix
                .filter(|prefix| (start..end).contains(prefix))
                .and_then(|prefix| component_profile_source_at(&lane.native_payload, prefix))
                .and_then(|source| {
                    let native = history_features.iter().find(|candidate| {
                        candidate
                            .source_id
                            .as_deref()
                            .and_then(|value| value.parse::<u32>().ok())
                            == Some(source)
                    })?;
                    feature_ids_by_native
                        .get(native.id.as_str())
                        .cloned()
                        .map(ProfileRef::Feature)
                });
            let generated = (start..end.saturating_sub(6))
                .filter(|offset| {
                    lane.native_payload.get(*offset..*offset + 2) == Some(&wrapper_token)
                        && lane.native_payload.get(*offset + 4..*offset + 9)
                            == Some(&[0x2b, 0x80, 0x02, 0, 0])
                        && offset.checked_sub(2).is_none_or(|prefix| {
                            lane.native_payload.get(prefix..*offset) != Some(&[1, 0])
                        })
                })
                .filter_map(|wrapper| {
                    let candidates = (wrapper + 4..end.saturating_sub(16))
                        .filter(|marker| {
                            lane.native_payload.get(*marker..*marker + 16)
                                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                        })
                        .filter_map(|marker| {
                            component_reference_curve_path_at(&lane.native_payload, marker)
                                .map(|components| (marker, components))
                        })
                        .collect::<Vec<_>>();
                    let [(_, components)] = candidates.as_slice() else {
                        return None;
                    };
                    let owner = component_path_terminal_feature(components, &history_features)?;
                    let feature_id = feature_ids_by_native.get(owner.as_str())?.clone();
                    let local_id = components
                        .iter()
                        .map(|component| {
                            component
                                .local_id
                                .map_or_else(|| "_".into(), |id| id.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let native = format!(
                        "sldprt:feature-input:component-reference-curve:{lane_key}:{wrapper}"
                    );
                    Some((
                        ProfileRef::Generated {
                            curves: vec![GeneratedCurveRef {
                                feature: feature_id,
                                local_id,
                            }],
                            native,
                        },
                        components.clone(),
                    ))
                })
                .collect::<Vec<_>>();
            let profile = match (direct, generated.as_slice()) {
                (Some(profile), []) => profile,
                (None, [(profile, _)]) => profile.clone(),
                _ => continue,
            };
            let mut dependencies = match generated.as_slice() {
                [(_, components)] => component_path_features(components, &history_features)
                    .into_iter()
                    .filter_map(|native| feature_ids_by_native.get(native.as_str()).cloned())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            match &profile {
                ProfileRef::Feature(feature) => dependencies.push(feature.clone()),
                ProfileRef::Generated { curves, .. } => {
                    dependencies.extend(curves.iter().map(|curve| curve.feature.clone()));
                }
                _ => {}
            }
            projections.insert(feature.id.clone(), (profile, dependencies));
        }
    }
    for feature in features {
        let Some((profile, dependencies)) = feature
            .native_ref
            .as_ref()
            .and_then(|native| projections.remove(native))
        else {
            continue;
        };
        let FeatureDefinition::Sweep { section, .. } = &mut feature.definition else {
            continue;
        };
        if !matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_)) {
            continue;
        }
        *section = cadmpeg_ir::features::SweepSection::Profile(profile);
        for dependency in dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn compact_body_path_at(payload: &[u8], marker: usize) -> Option<Vec<u32>> {
    if marker < 12
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || !payload
            .get(marker - 8..marker - 4)
            .is_some_and(|selector| is_component_vector_selector_for_role(selector, 3))
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(payload, marker - 12)?).ok()?;
    if count == 0 {
        return None;
    }
    compact_body_component_entries_at(payload, marker + 18, count).and_then(|components| {
        components
            .into_iter()
            .map(|component| component.local_id)
            .collect()
    })
}

pub(super) fn compact_body_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if marker < 12
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || !payload
            .get(marker - 8..marker - 4)
            .is_some_and(|selector| is_component_vector_selector_for_role(selector, 3))
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(payload, marker - 12)?)
        .ok()
        .filter(|count| *count != 0)?;
    compact_body_component_entries_at(payload, marker + 18, count)
}

fn compact_body_component_entries_at(
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if count == 0 {
        return None;
    }
    let mut candidates = Vec::new();
    let mixed = |count| {
        let (components, end) = compact_mixed_component_path(payload, cursor, count, true)?;
        components
            .iter()
            .any(|component| component.instance.is_none() || component.local_id.is_none())
            .then_some((components, end))
    };
    let parse = |count| {
        compact_heterogeneous_component_path(payload, cursor, count).or_else(|| mixed(count))
    };
    if let Some((components, _)) = parse(count) {
        candidates.push(components);
    } else if count > 1 {
        if let Some((components, end)) = parse(count - 1) {
            if compact_body_null_slot_at(payload, end) {
                candidates.push(components);
            }
        }
    }
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn compact_body_null_slot_at(payload: &[u8], end: usize) -> bool {
    payload.get(end..end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
        || payload.get(end..end + 10) == Some(&[0; 10])
}

pub(crate) fn project_compact_combine_paths(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    struct Projection {
        target: cadmpeg_ir::features::BodySelection,
        tools: cadmpeg_ir::features::BodySelection,
        dependencies: Vec<cadmpeg_ir::features::FeatureId>,
    }

    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let mut projections = HashMap::<String, Projection>::new();
    for history_feature in &history_features {
        let (Some(target), Some(tools)) = (
            history_feature.properties.get("Target"),
            history_feature.properties.get("Tools"),
        ) else {
            continue;
        };
        let project = |native: &str| {
            let (prefix, offset) = native.rsplit_once(':')?;
            let offset = offset.parse::<usize>().ok()?;
            let lane_key = prefix.rsplit_once(':')?.1;
            let lane = lanes.iter().find(|lane| {
                lane.id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key)
                    == lane_key
            })?;
            let components = compact_body_component_path_at(&lane.native_payload, offset)?;
            let producer = component_path_terminal_feature(&components, &history_features)?;
            let feature = feature_ids_by_native.get(&producer)?.clone();
            let local_id = components
                .iter()
                .map(|component| {
                    component
                        .local_id
                        .map_or_else(|| "_".into(), |id| id.to_string())
                })
                .collect::<Vec<_>>()
                .join(",");
            Some((
                cadmpeg_ir::features::BodySelection::Generated {
                    bodies: vec![cadmpeg_ir::features::GeneratedBodyRef {
                        feature: feature.clone(),
                        local_id,
                    }],
                    native: native.to_owned(),
                },
                components,
                feature,
            ))
        };
        let (
            Some((target, target_components, target_owner)),
            Some((tools, tool_components, tool_owner)),
        ) = (project(target), project(tools))
        else {
            continue;
        };
        let mut dependencies = target_components
            .iter()
            .chain(&tool_components)
            .filter_map(|component| {
                let native = component_path_terminal_feature(
                    std::slice::from_ref(component),
                    &history_features,
                )?;
                feature_ids_by_native.get(&native).cloned()
            })
            .collect::<Vec<_>>();
        dependencies.push(target_owner);
        dependencies.push(tool_owner);
        dependencies.sort_by_key(|dependency| {
            features
                .iter()
                .find(|feature| feature.id == *dependency)
                .map_or(u64::MAX, |feature| feature.ordinal)
        });
        dependencies.dedup();
        projections.insert(
            history_feature.id.clone(),
            Projection {
                target,
                tools,
                dependencies,
            },
        );
    }
    for feature in features {
        let Some(projection) = feature
            .native_ref
            .as_ref()
            .and_then(|native| projections.remove(native))
        else {
            continue;
        };
        let FeatureDefinition::Combine { target, tools, .. } = &mut feature.definition else {
            continue;
        };
        *target = projection.target;
        *tools = projection.tools;
        for dependency in projection.dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

pub(super) fn compact_extrusion_through_all_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 1)
        && (compact_extrusion_traversal_tail_at(payload, offset)
            || compact_extrusion_dimensioned_traversal_at(payload, offset)
            || (payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
                && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()))
}

fn compact_extrusion_dimensioned_traversal_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 22..offset + 30) == Some(&[0; 8])
        && payload.get(offset + 30..offset + 34) == Some(&[1, 0, 0, 1])
        && payload
            .get(offset + 34..offset + 44)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(offset + 44..offset + 48) == Some(&1u32.to_le_bytes())
        && payload
            .get(offset + 48..offset + 68)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && compact_extrusion_dimension_child_at(payload, offset + 68).is_some()
}

pub(super) fn compact_extrusion_blind_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 0)
        && ((payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
            && compact_extrusion_dimension_child_at(payload, offset + 26).is_some())
            || compact_extrusion_dimension_child_at(payload, offset + 22).is_some())
}

pub(super) fn compact_extrusion_through_next_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 2)
        && compact_extrusion_traversal_tail_at(payload, offset)
}

/// Through-all in both directions. Two carriers exist: a first-direction
/// traversal code `1` with second-direction code `1` and the shared traversal
/// tail, and the dedicated code `9` whose second-direction word is `1` and
/// whose retained blind dimension child follows immediately.
pub(super) fn compact_extrusion_through_all_both_at(payload: &[u8], offset: usize) -> bool {
    (compact_extrusion_two_direction_header(payload, offset, 1)
        && payload.get(offset + 26..offset + 30) == Some(&[0, 0, 0, 0])
        && compact_extrusion_traversal_body_at(payload, offset))
        || (compact_extrusion_two_direction_header(payload, offset, 9)
            && compact_extrusion_dimension_child_at(payload, offset + 26).is_some())
}

/// Blind first direction with a through-all second direction: a code `0`
/// header whose second-direction word is `1`, owning the blind dimension
/// child.
pub(super) fn compact_extrusion_blind_through_all_second_at(payload: &[u8], offset: usize) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 22) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Two-direction end-spec header: the words at `+4` and `+8` carry `0` or
/// `1`, the first-direction code sits at `+18`, and the second-direction
/// code `1` sits at `+22`.
fn compact_extrusion_two_direction_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 4) == Some(&[0, 0])
        && View::u32_le_at(payload, offset + 4).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 8).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
}

fn compact_extrusion_traversal_tail_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 22..offset + 30) == Some(&[0, 0, 0, 0, 0, 0, 0, 0])
        && compact_extrusion_traversal_body_from(payload, offset + 30)
}

/// Shared traversal run from `+30`: the `[1, 0, 0, 1]` marker and the fixed
/// zero fill through the `+90` word.
fn compact_extrusion_traversal_body_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_traversal_body_from(payload, offset + 30)
}

fn compact_extrusion_traversal_body_from(payload: &[u8], start: usize) -> bool {
    payload.get(start..start + 4) == Some(&[1, 0, 0, 1])
        && payload
            .get(start + 4..start + 60)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload
            .get(start + 60..start + 64)
            .is_some_and(|word| word == [0, 0, 1, 0] || word == [1, 0, 0, 0])
        && payload
            .get(start + 64..start + 70)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && compact_extrusion_traversal_follow_on_at(payload, start + 70)
}

fn compact_extrusion_traversal_follow_on_at(payload: &[u8], offset: usize) -> bool {
    let Some(bytes) = payload.get(offset..offset + 4) else {
        return false;
    };
    if bytes == [0, 0, 0, 0]
        || (bytes[1] & 0x80 != 0 && bytes[2..4] == [0, 0])
        || bytes == [0xff, 0xff, 1, 0]
    {
        return true;
    }
    bytes[1] & 0x80 != 0
        && payload.get(offset + 2..offset + 6) == Some(&5u32.to_le_bytes())
        && compact_extrusion_dimension_child_at(payload, offset + 6).is_some()
}

pub(super) fn compact_extrusion_mid_plane_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 6)
        && payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Validate the owned dimension child at `child` and return the offset just
/// past its fixed tail.
fn compact_extrusion_dimension_child_at(payload: &[u8], child: usize) -> Option<usize> {
    let declaration = b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c";
    let block = if payload.get(child..child + declaration.len()) == Some(declaration) {
        child + declaration.len()
    } else if payload
        .get(child + 1)
        .is_some_and(|byte| byte & 0x80 != 0 && *byte != 0xff)
    {
        child + 2
    } else {
        return None;
    };
    (payload.get(block..block + 16).is_some_and(|bytes| {
        bytes.iter().enumerate().all(|(index, byte)| match index {
            8 => matches!(*byte, 0 | 0x40),
            9 => byte.trailing_zeros() >= 3,
            _ => *byte == 0,
        })
    }) && payload.get(block + 16..block + 20) == Some(&[0xff, 0xff, 0, 0])
        && payload
            .get(block + 20)
            .is_some_and(|byte| *byte == 1 || *byte == 3)
        && payload.get(block + 21..block + 25) == Some(&[0xff, 0xff, 0xff, 0xff])
        && payload.get(block + 25..block + 31) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(block + 31..block + 33) == Some(&[0x80, 0xbf]))
    .then_some(block + 33)
}

/// Form of the point reference owned by an up-to-vertex end spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactPointReferenceKind {
    /// Direct vertex reference; the final path entry's component id is the
    /// feature-local vertex id.
    Point,
    /// Edge endpoint reference; the path selects an edge and the endpoint
    /// selector stays native.
    EdgeEndpoint,
}

pub(super) fn compact_extrusion_to_vertex_at(
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Option<(usize, CompactPointReferenceKind)> {
    let end = end.min(payload.len());
    let payload = payload.get(..end)?;
    if !compact_extrusion_end_spec_header(payload, offset, 3)
        || payload.get(offset + 22..offset + 30) != Some(&[0, 0, 0, 0, 0, 0, 0, 0])
    {
        return None;
    }
    let child = offset + 30;
    let point_declaration = b"\xff\xff\x01\x00\x0c\x00moPointRef_w";
    let endpoint_declaration = b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w";
    let point_body_at = |body: usize| {
        payload.get(body + 1).is_some_and(|byte| byte & 0x80 != 0)
            && payload
                .get(body + 2..body + 4)
                .is_some_and(|bytes| bytes == [0xa9, 0x80] || bytes == [0x2b, 0x80])
            && payload.get(body + 4..body + 9) == Some(&[2, 0, 0, 0, 0])
    };
    let kind = if (payload.get(child..child + point_declaration.len()) == Some(point_declaration)
        && point_body_at(child + point_declaration.len()))
        || point_body_at(child)
    {
        CompactPointReferenceKind::Point
    } else if payload.get(child..child + endpoint_declaration.len()) == Some(endpoint_declaration) {
        let edge_declaration = b"\xff\xff\x01\x00\x0c\x00moCompEdge_c";
        let inner = child + endpoint_declaration.len();
        let body = inner + edge_declaration.len();
        if payload.get(inner..inner + edge_declaration.len()) != Some(edge_declaration)
            || payload.get(body + 1).is_none_or(|byte| byte & 0x80 == 0)
            || payload.get(body + 2..body + 7) != Some(&[2, 0, 0, 0, 0x40])
        {
            return None;
        }
        CompactPointReferenceKind::EdgeEndpoint
    } else {
        return None;
    };
    let candidates = compact_termination_reference_candidates(payload, child, end, true);
    let [marker] = candidates.as_slice() else {
        return None;
    };
    Some((*marker, kind))
}

pub(super) fn compact_extrusion_offset_from_face_at(
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Option<usize> {
    let end = end.min(payload.len());
    let payload = payload.get(..end)?;
    if !compact_extrusion_end_spec_header(payload, offset, 5)
        || payload.get(offset + 22..offset + 26) != Some(&[0, 0, 0, 0])
    {
        return None;
    }
    let resume = compact_extrusion_dimension_child_at(payload, offset + 26)?;
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut candidates = Vec::new();
    for anchor in resume..end.saturating_sub(2) {
        if payload.get(anchor..anchor + 3) != Some(&[1, 1, 0]) {
            continue;
        }
        let child = anchor + 3;
        let body = if payload.get(child..child + declaration.len()) == Some(declaration) {
            child + declaration.len()
        } else {
            child
        };
        // The reference body opens with lane tokens followed by the selector.
        let open_start = body.saturating_add(2);
        let open_end = end.min(body.saturating_add(9));
        for open in (open_start..open_end).filter(|cursor| {
            (*cursor).saturating_add(7) <= end
                && payload.get(cursor - 1).is_some_and(|byte| byte & 0x80 != 0)
                && payload.get(*cursor..cursor + 7) == Some(&[2, 0, 0, 0, 0x40, 0, 0])
        }) {
            candidates.extend(compact_termination_reference_candidates(
                payload, open, end, true,
            ));
        }
    }
    let candidates = distinct_offsets(candidates);
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn compact_extrusion_to_face_at(
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Option<usize> {
    let end = end.min(payload.len());
    let payload = payload.get(..end)?;
    // Older end-spec streams encode the `moEndSpec_c` class as the fixed
    // two-byte token `03 00`; their remaining header and child grammar is
    // identical. Keep that token scoped to the to-face form whose required
    // single-face child independently validates the interpretation.
    let legacy_header = payload.get(offset..offset + 2) == Some(&[3, 0])
        && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(&[4, 0, 0, 0]);
    if !(compact_extrusion_end_spec_header(payload, offset, 4) || legacy_header)
        || View::u32_le_at(payload, offset + 22).is_none_or(|flag| flag > 1)
        || payload.get(offset + 26..offset + 30) != Some(&[0, 0, 0, 0])
        || payload.get(offset + 30..offset + 33) != Some(&[1, 1, 0])
    {
        return None;
    }
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let child = offset + 33;
    let declared = payload.get(child..child + declaration.len()) == Some(declaration);
    let body_offset = if declared {
        child + declaration.len()
    } else if compact_single_face_child_body_at(payload, child + 2) {
        child + 2
    } else {
        return None;
    };
    if !declared && !compact_single_face_child_body_at(payload, body_offset) {
        return None;
    }
    // A declared child begins with a fixed body header. Starting the marker
    // search at that header lets the legacy path decoder reinterpret the
    // header as a second, spurious compact reference. The modern marker is
    // always after the header; an undeclared legacy child still needs the
    // body offset as its fallback anchor.
    let search_start = if declared {
        body_offset.saturating_add(11)
    } else {
        body_offset
    };
    let compact_candidates = distinct_offsets(compact_termination_reference_candidates(
        payload,
        search_start,
        end,
        declared,
    ));
    match compact_candidates.as_slice() {
        [candidate] => Some(*candidate),
        [] if declared && compact_tokenized_single_face_child_at(payload, body_offset) => {
            // A declared single-face child is still a complete native
            // selection when its compact body header validates but its
            // component path uses an unknown layout. Preserve that child as
            // the reference instead of discarding the independently decoded
            // to-face termination.
            Some(body_offset)
        }
        [] if declared && legacy_single_face_reference_path_at(payload, body_offset).is_some() => {
            // Some declared children retain the older counted path directly
            // in the body and do not carry a modern marker. Preserve that
            // complete legacy selection when no modern marker is present.
            Some(body_offset)
        }
        [] if !declared && legacy_single_face_reference_path_at(payload, body_offset).is_some() => {
            // Legacy streams place the path directly in the child body and
            // have no compact marker to identify. Keep that form as a
            // fallback only after the modern marker search is empty.
            Some(body_offset)
        }
        _ => None,
    }
}

fn compact_termination_reference_candidates(
    payload: &[u8],
    start: usize,
    end: usize,
    require_path: bool,
) -> Vec<usize> {
    (start..end.min(payload.len()))
        .filter(|marker| {
            if require_path {
                compact_termination_reference_at(payload, *marker)
            } else {
                compact_termination_reference_frame_at(payload, *marker).is_some()
            }
        })
        .collect()
}

fn distinct_offsets(mut offsets: Vec<usize>) -> Vec<usize> {
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

fn compact_tokenized_single_face_child_at(payload: &[u8], offset: usize) -> bool {
    if compact_tokenized_face_body_at(payload, offset, 2) {
        return true;
    }
    let declaration = b"\xff\xff\x01\x00\x0c\x00moCompFace_c";
    payload.get(offset..offset + declaration.len()) == Some(declaration)
        && compact_tokenized_face_body_at(payload, offset + declaration.len(), 1)
}

fn compact_tokenized_face_body_at(
    payload: &[u8],
    offset: usize,
    leading_class_tokens: usize,
) -> bool {
    let word_count = leading_class_tokens + 7;
    let Some(body) = payload.get(offset..offset + word_count * 2) else {
        return false;
    };
    let token_at = |index: usize| View::u16_le_at(body, index * 2);
    (1..=2).contains(&leading_class_tokens)
        && (0..leading_class_tokens).all(|index| token_at(index).is_some_and(is_class_token))
        && token_at(leading_class_tokens) == Some(2)
        && token_at(leading_class_tokens + 1).is_some_and(is_class_token)
        && token_at(leading_class_tokens + 2) == Some(0)
        && token_at(leading_class_tokens + 3).is_some_and(is_class_token)
        && token_at(leading_class_tokens + 4) == Some(1)
        && token_at(leading_class_tokens + 5) == Some(0)
        && token_at(leading_class_tokens + 6).is_some_and(is_class_token)
}

fn compact_single_face_child_body_at(payload: &[u8], offset: usize) -> bool {
    let Some(body) = payload.get(offset..offset + 11) else {
        return false;
    };
    View::u16_le_at(body, 0).is_some_and(is_class_token)
        && View::u16_le_at(body, 2).is_some_and(is_class_token)
        && body[4..8] == 2u32.to_le_bytes()
        && matches!(body[8], 0 | 0x40)
        && body[9..11] == [0, 0]
}

/// End-spec children carry their class at the anchor: either a lane-scoped
/// class token or a direct `moEndSpec_c` declaration ending at the anchor.
/// Header-shaped runs without this identity belong to fillet edge-set records.
fn compact_end_spec_identity_at(payload: &[u8], offset: usize) -> bool {
    payload
        .get(offset..offset + 2)
        .and_then(|bytes| View::u16_le_at(bytes, 0))
        .is_some_and(is_class_token)
        || offset
            .checked_sub(15)
            .and_then(|start| payload.get(start..offset + 2))
            == Some(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c".as_slice())
}

fn compact_extrusion_end_spec_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 4) == Some(&[0, 0])
        && payload.get(offset + 4..offset + 8) == Some(&1u32.to_le_bytes())
        && View::u32_le_at(payload, offset + 8).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
}

pub(super) fn compact_single_face_reference_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    compact_single_face_reference_record_at(payload, marker)
        .map(|record| record.0)
        .or_else(|| legacy_single_face_reference_path_at(payload, marker))
}

pub(super) fn legacy_single_face_reference_path_at(
    payload: &[u8],
    body: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let header = payload.get(body..body + 19)?;
    let class_token = View::u16_le_at(header, 0)?;
    let component_token = View::u16_le_at(header, 2)?;
    let owner = View::u32_le_at(header, 11)?;
    if !is_class_token(class_token)
        || !is_class_token(component_token)
        || header[4..8] != 2u32.to_le_bytes()
        || !matches!(header[8], 0 | 0x40)
        || header[9..11] != [0, 0]
        || owner == 0
        || header[15..19] != owner.to_le_bytes()
    {
        return None;
    }
    let entry_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = payload.get(offset..offset + 4)?;
        let signature: [u8; 12] = payload.get(offset + 4..offset + 16)?.try_into().ok()?;
        (View::u16_le_at(instance, 0).is_some_and(is_class_token)
            && instance[2..4] == [0, 0]
            && signature[0..2] != [0, 0])
        .then(|| FeatureInputComponentPathEntry {
            instance: View::u16_le_at(instance, 0),
            type_signature: signature,
            local_id: None,
        })
    };
    let terminal = |offset: usize| {
        payload.get(offset..offset + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
            || [20usize, 24].into_iter().any(|zero_count| {
                payload
                    .get(offset..offset + zero_count)
                    .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                    && View::u32_le_at(payload, offset + zero_count)
                        .is_some_and(|source| source != 0)
            })
    };
    #[allow(clippy::items_after_statements, clippy::too_many_arguments)]
    fn parse_entries(
        payload: &[u8],
        entry_at: &impl Fn(usize) -> Option<FeatureInputComponentPathEntry>,
        terminal: &impl Fn(usize) -> bool,
        cursor: usize,
        remaining: usize,
        has_path_slots: bool,
        entries: &mut Vec<FeatureInputComponentPathEntry>,
        complete: &mut Vec<Vec<FeatureInputComponentPathEntry>>,
    ) {
        if complete.len() > 1 {
            return;
        }
        if remaining == 0 {
            if terminal(cursor) && !complete.contains(entries) {
                complete.push(entries.clone());
            }
            return;
        }
        let Some(entry) = entry_at(cursor) else {
            return;
        };
        let next_offsets = |end: usize| {
            let mut offsets = Vec::new();
            for slot_bytes in [0usize, 4] {
                if slot_bytes == 4 && !has_path_slots {
                    continue;
                }
                if slot_bytes == 4
                    && !View::u32_le_at(payload, end)
                        .is_some_and(|slot| (1..=u32::from(u16::MAX)).contains(&slot))
                {
                    continue;
                }
                for gap in [0usize, 2, 4, 6, 8] {
                    let next = end + slot_bytes;
                    if payload
                        .get(next..next + gap)
                        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                    {
                        offsets.push(next + gap);
                    }
                }
            }
            offsets
        };
        for with_local_id in [true, false] {
            let mut entry = entry.clone();
            let end = if with_local_id {
                let Some(bytes) = payload.get(cursor + 16..cursor + 20) else {
                    continue;
                };
                entry.local_id = View::u32_le_at(bytes, 0);
                cursor + 20
            } else {
                cursor + 16
            };
            entries.push(entry);
            for next in next_offsets(end) {
                parse_entries(
                    payload,
                    entry_at,
                    terminal,
                    next,
                    remaining - 1,
                    has_path_slots,
                    entries,
                    complete,
                );
            }
            entries.pop();
        }
    }
    let mut candidates = Vec::new();
    for control in [body + 44, body + 48, body + 84, body + 88] {
        let Some(prefix) = payload.get(control..control + 40) else {
            continue;
        };
        let Some(filler) = payload.get(body + 19..control) else {
            continue;
        };
        let padded = filler.len() >= 16
            && filler.windows(16).enumerate().any(|(start, window)| {
                window.iter().all(|byte| *byte == 0xff)
                    && filler[..start].iter().all(|byte| *byte == 0)
                    && filler[start + 16..].iter().all(|byte| *byte == 0)
            });
        if !filler.iter().all(|byte| *byte == 0) && !padded {
            continue;
        }
        let token = View::u16_le_at(prefix, 0)?;
        let count = usize::try_from(View::u32_le_at(prefix, 10)?).ok()?;
        if !is_class_token(token)
            || prefix[2..6] != 1u32.to_le_bytes()
            || prefix[6..10] != [0; 4]
            || !(1..=64).contains(&count)
            || !is_component_vector_selector(&prefix[14..18])
            || prefix[22..30] != prefix[30..38]
            || prefix[38..40] != [0, 0]
        {
            continue;
        }
        for serialized_roots in [0usize, 2] {
            let Some(entry_count) = count
                .checked_sub(serialized_roots)
                .filter(|count| *count > 0)
            else {
                continue;
            };
            parse_entries(
                payload,
                &entry_at,
                &terminal,
                control + 40,
                entry_count,
                prefix[15] == 3,
                &mut Vec::new(),
                &mut candidates,
            );
        }
    }
    candidates.sort_by_key(|entries| {
        entries
            .iter()
            .map(|entry| (entry.instance, entry.type_signature, entry.local_id))
            .collect::<Vec<_>>()
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn compact_single_face_reference_record_at(
    payload: &[u8],
    marker: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, Option<u32>)> {
    let count = marker
        .checked_sub(12)
        .and_then(|offset| View::u32_le_at(payload, offset))?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| (1..=64).contains(count))?;
    if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || !payload
            .get(marker - 8..marker - 4)
            .is_some_and(is_component_vector_selector)
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    compact_heterogeneous_component_path(payload, marker + 18, count)
        .map(|(components, _)| (components, None))
        .or_else(|| {
            [1usize, 2].into_iter().find_map(|serialized_roots| {
                let entry_count = count.checked_sub(serialized_roots)?;
                let (components, end) =
                    compact_heterogeneous_component_path(payload, marker + 18, entry_count)?;
                [0usize, 4, 8].into_iter().find_map(|gap| {
                    let filler = match gap {
                        0 => true,
                        4 => payload.get(end..end + 4) == Some(&[0; 4]),
                        8 => matches!(
                            payload.get(end..end + 8),
                            Some(
                                [0, 0, 0, 0, 0, 0, 0, 0]
                                    | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                                    | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                            )
                        ),
                        _ => false,
                    };
                    if !filler {
                        return None;
                    }
                    let terminal = end + gap;
                    if payload.get(terminal..terminal + 8)
                        == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
                    {
                        return Some((components.clone(), None));
                    }
                    let source = View::u32_le_at(payload, terminal + 20)?;
                    (payload.get(terminal..terminal + 20)? == [0; 20] && source != 0)
                        .then(|| (components.clone(), Some(source)))
                })
            })
        })
}

fn compact_termination_reference_at(payload: &[u8], marker: usize) -> bool {
    compact_termination_reference_path_at(payload, marker).is_some()
}

/// Decode the component path of an up-to-vertex or offset-from-face
/// termination reference. These vectors share the single-face-reference
/// grammar and may additionally carry a leading identifier-less component
/// cell, `a0 86 01 00` filler words, or an `01 00 00 00` slot word between
/// counted entries.
pub(super) fn compact_termination_reference_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if let Some(components) = compact_single_face_reference_path_at(payload, marker) {
        return Some(components);
    }
    let count = compact_termination_reference_frame_at(payload, marker)?;
    let count = usize::try_from(count).ok()?;
    let entry_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = payload.get(offset..offset + 4)?;
        if instance[0..2] == [0, 0]
            || instance[0..2] == [0xff, 0xff]
            || instance[2..4] != [0, 0]
            || payload.get(offset + 4..offset + 6)? == [0, 0]
        {
            return None;
        }
        Some(FeatureInputComponentPathEntry {
            instance: Some(View::u16_le_at(instance, 0)?),
            type_signature: payload.get(offset + 4..offset + 16)?.try_into().ok()?,
            local_id: Some(View::u32_le_at(payload, offset + 16)?),
        })
    };
    let mut cursor = marker + 18;
    // A leading identifier-less cell repeats the first counted entry's
    // signature immediately after its own.
    if entry_at(cursor).is_some()
        && entry_at(cursor + 16).is_some()
        && payload.get(cursor + 20..cursor + 32) == payload.get(cursor + 4..cursor + 16)
    {
        cursor += 16;
    }
    let mut entries = Vec::new();
    while entries.len() < count {
        let ordinal_gap = payload
            .get(cursor..cursor + 4)
            .and_then(|bytes| {
                let ordinal = View::u16_le_at(bytes, 0)?;
                (ordinal != 0 && ordinal & 0x8000 == 0 && bytes[2..4] == [0, 0]).then_some(ordinal)
            })
            .is_some()
            && entry_at(cursor + 4).is_some();
        if ordinal_gap {
            cursor += 4;
            continue;
        }
        if let Some(entry) = entry_at(cursor) {
            entries.push(entry);
            cursor += 20;
            continue;
        }
        let gap = [4usize, 8].into_iter().find(|gap| {
            let filler_ok = match gap {
                4 => matches!(
                    payload.get(cursor..cursor + 4),
                    Some([0, 0, 0, 0] | [0xa0, 0x86, 0x01, 0x00])
                ),
                8 => matches!(
                    payload.get(cursor..cursor + 8),
                    Some(
                        [0, 0, 0, 0, 0, 0, 0, 0]
                            | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                            | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                            | [0x01, 0x00, 0x00, 0x00, 0, 0, 0, 0]
                    )
                ),
                _ => false,
            };
            filler_ok && entry_at(cursor + gap).is_some()
        });
        match gap {
            Some(gap) => cursor += gap,
            None => break,
        }
    }
    (!entries.is_empty()).then_some(entries)
}

fn compact_termination_reference_frame_at(payload: &[u8], marker: usize) -> Option<u32> {
    let count = marker
        .checked_sub(12)
        .and_then(|offset| View::u32_le_at(payload, offset))?;
    if !(1..=64).contains(&count)
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || !payload
            .get(marker - 8..marker - 4)
            .is_some_and(is_component_vector_selector)
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    Some(count)
}

pub(crate) fn compact_surface_selection_value(
    components: &[FeatureInputComponentPathEntry],
) -> String {
    let mut value = String::from("sldprt:feature-input:surface-component-ids:");
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        match component.local_id {
            Some(local_id) => {
                write!(&mut value, "{local_id}").expect("writing to String cannot fail");
            }
            None => value.push('_'),
        }
    }
    value
}

#[cfg(test)]
mod terminations_tests;
