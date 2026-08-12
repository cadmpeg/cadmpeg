//! Compact body, edge and surface selection decoding.

use super::component_paths::{
    component_path_input_features, component_path_terminal_feature, feature_precedes_consumer,
    surface_selection_producer_features,
};
use super::endpoints::{marker_profile_curve_role, wide_indexed_curve_endpoint_indices};
use super::markers::{
    linked_profile_point, marker_coordinates, marker_is_geometry_locus, marker_native_code,
};
use super::operations::repeated_class_token;
use super::scalars::{feature_object_name, operand_kind};
use super::terminations::{
    compact_extrusion_offset_from_face_at, compact_extrusion_to_face_at,
    compact_extrusion_to_vertex_at, compact_single_face_reference_record_at,
    compact_termination_reference_path_at,
};
use super::{CLASS_MARKER, LEGACY_SKETCH_MARKER};
use crate::classification::{
    classify_type_token, native_object_class, FeatureClass, NativeClassKind,
};
use crate::records::{
    FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputEdgeSelection,
    FeatureInputLane, FeatureInputOperandKind, FeatureInputSurfaceSelection, SketchInputKind,
};
use cadmpeg_core::cursor::bounded_len;
use std::collections::{HashMap, HashSet};

pub(super) fn compact_body_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputBodySelection> {
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let state_token = compact_body_state_token(lane);
    let mut result = Vec::new();
    for (object_index, &(name, feature)) in objects.iter().enumerate() {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let next = objects.get(object_index + 1);
        let end = next
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let next_token = next.and_then(|(next, next_feature)| {
            (native_object_class(next_feature.input_class.as_deref().unwrap_or_default()).kind
                == NativeClassKind::DeleteBody)
                .then(|| {
                    usize::try_from(next.offset)
                        .ok()
                        .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                })
                .flatten()
        });
        let selection = if kind == NativeClassKind::DeleteBody {
            lane.native_payload
                .get(start..end)
                .and_then(|payload| compact_body_selection_vector(payload, start, next_token))
        } else if kind == NativeClassKind::Operation(FeatureClass::MoveBody) {
            let data_classes = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moMoveCopyBodyData_c"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| (start..end).contains(&offset))
                })
                .collect::<Vec<_>>();
            match data_classes.as_slice() {
                [class] => super::direct_edits::move_body_translation_record(
                    &lane.native_payload,
                    start,
                    end,
                    class.offset,
                )
                .map(|record| (record.selection_offset, record.local_body_ids)),
                _ => None,
            }
        } else {
            None
        };
        let Some((offset, local_body_ids)) = selection else {
            continue;
        };
        result.push(FeatureInputBodySelection {
            id: format!("sldprt:feature-input:body-selection#{lane_key}:{offset}"),
            parent: lane.id.clone(),
            ordinal: result.len() as u32,
            offset: offset as u64,
            object_name_ref: name.id.clone(),
            feature_ref: feature.id.clone(),
            local_body_ids,
            body_state_ids: if kind == NativeClassKind::DeleteBody {
                state_token.map_or_else(Vec::new, |token| {
                    compact_body_state_ids(&lane.native_payload, start, offset, token)
                })
            } else {
                Vec::new()
            },
            mode: if kind == NativeClassKind::DeleteBody {
                state_token.and_then(|token| {
                    compact_body_retention_mode(&lane.native_payload, start, offset, token)
                })
            } else {
                None
            },
        });
    }
    result
}

fn compact_body_state_token(lane: &FeatureInputLane) -> Option<u16> {
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moDeleteBodyData_c");
    let class = classes.next()?;
    if classes.next().is_some() {
        return None;
    }
    let offset = usize::try_from(class.offset).ok()?;
    Some(u16::from_le_bytes(
        lane.native_payload
            .get(offset + 8 + class.name.len()..offset + 10 + class.name.len())?
            .try_into()
            .ok()?,
    ))
}

pub(crate) fn compact_body_state_ids_for_selection(
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Vec<u32> {
    let Some(token) = compact_body_state_token(lane) else {
        return Vec::new();
    };
    let Some(start) = lane
        .names
        .iter()
        .find(|name| name.id == selection.object_name_ref)
        .and_then(|name| usize::try_from(name.offset).ok())
    else {
        return Vec::new();
    };
    let Some(end) = usize::try_from(selection.offset).ok() else {
        return Vec::new();
    };
    compact_body_state_ids(&lane.native_payload, start, end, token)
}

pub(crate) fn compact_body_retention_mode_for_selection(
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Option<cadmpeg_ir::features::BodyRetentionMode> {
    let token = compact_body_state_token(lane)?;
    let start = lane
        .names
        .iter()
        .find(|name| name.id == selection.object_name_ref)
        .and_then(|name| usize::try_from(name.offset).ok())?;
    let end = usize::try_from(selection.offset).ok()?;
    compact_body_retention_mode(&lane.native_payload, start, end, token)
}

pub(super) fn compact_body_retention_mode(
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Option<cadmpeg_ir::features::BodyRetentionMode> {
    const HEADER_LEN: usize = 83;
    let token = token.to_le_bytes();
    let state_end = (start..end.saturating_sub(HEADER_LEN - 1))
        .filter(|offset| compact_body_state_header(payload, *offset, token).is_some())
        .map(|offset| offset + HEADER_LEN)
        .max()?;
    let field = payload.get(state_end..state_end + 10)?;
    if field[0..2] != [0x30, 0x80] || field[6..10] != [0; 4] {
        return None;
    }
    match u32::from_le_bytes(field[2..6].try_into().ok()?) {
        0 => Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected),
        1 => Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected),
        _ => None,
    }
}

fn compact_body_state_header(payload: &[u8], offset: usize, token: [u8; 2]) -> Option<&[u8]> {
    const HEADER_LEN: usize = 83;
    let header = payload.get(offset..offset + HEADER_LEN)?;
    (header[0..2] == token
        && header[2..11] == [0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]
        && header[11..15] == header[15..19]
        && header[19..47].iter().all(|byte| *byte == 0)
        && header[47..63].iter().all(|byte| *byte == 0xff)
        && header[63..83].iter().all(|byte| *byte == 0))
    .then_some(header)
}

pub(super) fn compact_body_state_ids(
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Vec<u32> {
    const HEADER_LEN: usize = 83;
    let token = token.to_le_bytes();
    let mut result = Vec::new();
    for offset in start..end.saturating_sub(HEADER_LEN - 1) {
        let Some(header) = compact_body_state_header(payload, offset, token) else {
            continue;
        };
        result.push(u32::from_le_bytes(
            header[11..15].try_into().expect("four-byte body id"),
        ));
    }
    result
}

pub(super) fn compact_edge_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputEdgeSelection> {
    let history_features = history_features_with_object_sources(histories, lane);
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    let mut compact_edge_classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moCompEdge_c");
    let compact_edge_class = compact_edge_classes
        .next()
        .filter(|_| compact_edge_classes.next().is_none());
    let class_name_end = compact_edge_class.and_then(|class| {
        usize::try_from(class.offset)
            .ok()?
            .checked_add(6 + class.name.len())
    });
    let compact_edge_token = class_name_end.and_then(|offset| {
        Some(u16::from_le_bytes(
            lane.native_payload
                .get(offset..offset + 2)?
                .try_into()
                .ok()?,
        ))
    });
    for (object_index, &(name, feature)) in objects.iter().enumerate() {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
        if !matches!(kind, NativeClassKind::Fillet | NativeClassKind::Chamfer) {
            continue;
        }
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let object_end = objects
            .get(object_index + 1)
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let end = if kind == NativeClassKind::Fillet {
            fillet_edge_roster_end(lane, start, object_end).unwrap_or(object_end)
        } else {
            object_end
        };
        let direct_child = compact_edge_class
            .and_then(|class| usize::try_from(class.offset).ok())
            .filter(|offset| (start..end).contains(offset));
        let mut selections = Vec::new();
        if let Some(child_start) = direct_child {
            if let Some(selection) = lane
                .native_payload
                .get(child_start..end)
                .and_then(|payload| compact_edge_selection_vector(payload, child_start))
            {
                selections.push(selection);
            }
        }
        if let Some(token) = compact_edge_token {
            selections.extend(repeated_edge_selections(
                &lane.native_payload,
                start,
                end,
                token,
            ));
        }
        selections.extend(edge_selection_vectors_in_interval(
            &lane.native_payload,
            start,
            end,
        ));
        selections.sort_unstable_by_key(|selection| selection.0);
        selections.dedup_by_key(|selection| selection.0);
        let feature_selections = selections
            .into_iter()
            .map(|(offset, local_edge_ids)| {
                let references = compact_component_reference_list_at(&lane.native_payload, offset)
                    .unwrap_or_default();
                let components = compact_edge_component_path_at(&lane.native_payload, offset)
                    .unwrap_or_default();
                let terminal_feature_ref = compact_edge_owner_feature_at(
                    &lane.native_payload,
                    offset,
                    &components,
                    &history_features,
                    &feature.id,
                );
                let producer_feature_refs = compact_edge_producer_features_at(
                    &lane.native_payload,
                    offset,
                    &components,
                    &history_features,
                    &feature.id,
                );
                FeatureInputEdgeSelection {
                    id: format!("sldprt:feature-input:edge-selection#{lane_key}:{offset}"),
                    parent: lane.id.clone(),
                    ordinal: 0,
                    offset: offset as u64,
                    object_name_ref: name.id.clone(),
                    feature_ref: feature.id.clone(),
                    local_edge_ids,
                    components,
                    references,
                    producer_feature_refs,
                    terminal_feature_ref,
                }
            })
            .collect::<Vec<_>>();
        for mut selection in input_owned_edge_selections(feature_selections) {
            selection.ordinal = result.len() as u32;
            result.push(selection);
        }
    }
    result
}

fn fillet_edge_roster_end(lane: &FeatureInputLane, start: usize, end: usize) -> Option<usize> {
    ["moEdgeDim_c", "moVertDim_c"]
        .into_iter()
        .filter_map(|class_name| first_class_object_in_interval(lane, start, end, class_name))
        .min()
}

fn first_class_object_in_interval(
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
    class_name: &str,
) -> Option<usize> {
    let direct = lane
        .classes
        .iter()
        .filter(|class| class.name == class_name)
        .filter_map(|class| usize::try_from(class.offset).ok())
        .filter(|offset| (start..end).contains(offset))
        .map(|offset| {
            offset
                .checked_sub(4)
                .filter(|record_start| {
                    lane.native_payload.get(*record_start..*record_start + 2) == Some(&[0x20, 0x81])
                })
                .unwrap_or(offset)
        })
        .min();

    let mut tokens = lane
        .classes
        .iter()
        .filter(|class| class.name == class_name)
        .filter_map(|class| {
            usize::try_from(class.offset)
                .ok()?
                .checked_add(6 + class.name.len())
        })
        .filter_map(|offset| {
            Some(u16::from_le_bytes(
                lane.native_payload
                    .get(offset..offset + 2)?
                    .try_into()
                    .ok()?,
            ))
        })
        .filter(|token| token & 0x8000 != 0 && *token != u16::MAX)
        .collect::<Vec<_>>();
    tokens.sort_unstable();
    tokens.dedup();
    let repeated = match tokens.as_slice() {
        [token] => (start..end.saturating_sub(7)).find(|offset| {
            lane.native_payload.get(*offset..*offset + 2) == Some(&[0x20, 0x81])
                && lane.native_payload.get(*offset + 2..*offset + 4) == Some(&[0x10, 0x00])
                && lane
                    .native_payload
                    .get(*offset + 4..*offset + 6)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u16::from_le_bytes)
                    .is_some_and(|instance| instance & 0x8000 != 0 && instance != u16::MAX)
                && lane.native_payload.get(*offset + 6..*offset + 8)
                    == Some(token.to_le_bytes().as_slice())
        }),
        _ => None,
    };
    direct.into_iter().chain(repeated).min()
}

pub(super) fn input_owned_edge_selections(
    mut selections: Vec<FeatureInputEdgeSelection>,
) -> Vec<FeatureInputEdgeSelection> {
    if selections
        .iter()
        .any(|selection| !selection.producer_feature_refs.is_empty())
    {
        selections.retain(|selection| !selection.producer_feature_refs.is_empty());
    }
    selections
}

pub(super) fn compact_surface_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputSurfaceSelection> {
    let history_features = history_features_with_object_sources(histories, lane);
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moCompSurfaceBody_c");
    let surface_class = classes.next().filter(|_| classes.next().is_none());
    let surface_token = surface_class.and_then(|class| {
        usize::try_from(class.offset)
            .ok()
            .and_then(|offset| offset.checked_add(6 + class.name.len()))
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
    });
    let cylinder_reference_tokens = lane
        .classes
        .iter()
        .filter(|class| class.name == "moCylinderRef_w")
        .filter_map(|class| {
            let body = usize::try_from(class.offset)
                .ok()?
                .checked_add(6 + class.name.len())?;
            let token =
                u16::from_le_bytes(lane.native_payload.get(body..body + 2)?.try_into().ok()?);
            (token & 0x8000 != 0 && token != 0xffff).then_some(token)
        })
        .collect::<HashSet<_>>();
    let mirror_surface_prefix = mirror_surface_type_prefix(lane);
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    for (index, &(name, feature)) in objects.iter().enumerate() {
        let classified =
            native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
        let kind = match classified {
            NativeClassKind::Unknown if matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut") => {
                NativeClassKind::Extrusion
            }
            NativeClassKind::Unknown => {
                classify_type_token(&feature.kind).map_or(NativeClassKind::Unknown, |class| {
                    match class {
                        FeatureClass::Extrude => NativeClassKind::Extrusion,
                        class => NativeClassKind::Operation(class),
                    }
                })
            }
            classified => classified,
        };
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let next_object = if kind == NativeClassKind::Extrusion {
            objects[index + 1..]
                .iter()
                .find(|(_, next)| next.id != feature.id)
        } else {
            objects.get(index + 1)
        };
        let end = next_object
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let candidates = match kind {
            NativeClassKind::Thicken => surface_token.map_or_else(Vec::new, |token| {
                (start..end.saturating_sub(105))
                    .filter(|offset| lane.native_payload.get(*offset..*offset + 2) == Some(token))
                    .filter_map(|offset| {
                        let marker = offset + 103;
                        compact_surface_selection_at(&lane.native_payload, marker)
                            .map(|ids| (marker, ids))
                    })
                    .collect()
            }),
            NativeClassKind::Extrusion => (start..end.saturating_sub(103))
                .filter_map(|offset| {
                    compact_extrusion_to_face_at(&lane.native_payload, offset, end)
                        .or_else(|| {
                            compact_extrusion_to_vertex_at(&lane.native_payload, offset, end)
                                .map(|(marker, _)| marker)
                        })
                        .or_else(|| {
                            compact_extrusion_offset_from_face_at(&lane.native_payload, offset, end)
                        })
                })
                .filter_map(|marker| {
                    compact_termination_reference_path_at(&lane.native_payload, marker)
                        .map(|ids| (marker, ids))
                })
                .collect(),
            NativeClassKind::CosmeticThread => cosmetic_thread_cylinder_references(
                feature,
                lane,
                start,
                end,
                &cylinder_reference_tokens,
            )
            .into_iter()
            .chain(lane.classes.iter().filter_map(|class| {
                let offset = usize::try_from(class.offset).ok()?;
                (class.name == "moCompFace_c" && (start..end).contains(&offset))
                    .then(|| offset.checked_add(6 + class.name.len()))
                    .flatten()
                    .and_then(|body| component_face_reference_at(&lane.native_payload, body))
            }))
            .collect(),
            NativeClassKind::MirrorPattern => (start.saturating_add(12)
                ..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
                .filter(|marker| {
                    lane.native_payload
                        .get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                        == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                })
                .filter_map(|marker| {
                    counted_surface_component_path_at(&lane.native_payload, marker)
                        .map(|components| (marker, components))
                })
                .chain(mirror_surface_prefix.into_iter().flat_map(|prefix| {
                    inline_mirror_surface_paths(&lane.native_payload, start, end, prefix)
                }))
                .collect(),
            NativeClassKind::ReferencePlane => {
                face_reference_plane_selection_candidates(lane, start, end)
            }
            NativeClassKind::PlanarSurface => {
                planar_surface_selection_candidates(&lane.native_payload, start, end)
            }
            NativeClassKind::Operation(operation) => {
                operation_surface_selection_candidates(operation, lane, start, end, name.object_id)
            }
            _ => continue,
        };
        let expected_count = match kind {
            NativeClassKind::Operation(FeatureClass::CutWithSurface)
            | NativeClassKind::PlanarSurface => 2,
            _ => 1,
        };
        if !matches!(
            kind,
            NativeClassKind::MirrorPattern | NativeClassKind::Operation(FeatureClass::SplitFace)
        ) && candidates.len() != expected_count
        {
            continue;
        }
        for (offset, components) in candidates {
            let terminal_feature_ref = surface_selection_terminal_feature_at(
                &lane.native_payload,
                offset,
                &components,
                &history_features,
            );
            let producer_feature_refs = surface_selection_producer_features(
                &components,
                terminal_feature_ref.as_deref(),
                &history_features,
            );
            result.push(FeatureInputSurfaceSelection {
                id: format!("sldprt:feature-input:surface-selection#{lane_key}:{offset}"),
                parent: lane.id.clone(),
                ordinal: result.len() as u32,
                offset: offset as u64,
                selector: lane.native_payload[offset.saturating_sub(8)],
                object_name_ref: name.id.clone(),
                feature_ref: feature.id.clone(),
                producer_feature_refs,
                terminal_feature_ref,
                components,
            });
        }
    }
    result
}

fn planar_surface_selection_candidates(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    (start..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
        .filter_map(|marker| {
            let selector = payload.get(marker.checked_sub(8)?..marker - 4)?;
            (selector[1..] == [2, 0, 0] && matches!(selector[0], 4 | 6))
                .then(|| component_vector_path_at(payload, marker))
                .flatten()
                .map(|components| (marker, components))
        })
        .collect()
}

fn face_reference_plane_selection_candidates(
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let data_classes = lane
        .classes
        .iter()
        .filter(|class| {
            class.name == "moFaceRefPlnData_c"
                && usize::try_from(class.offset).is_ok_and(|offset| (start..end).contains(&offset))
        })
        .collect::<Vec<_>>();
    let [data_class] = data_classes.as_slice() else {
        return Vec::new();
    };
    let Some(body) = usize::try_from(data_class.offset)
        .ok()
        .and_then(|offset| offset.checked_add(6 + data_class.name.len()))
    else {
        return Vec::new();
    };
    let mut candidates = (body..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
        .filter(|marker| {
            lane.native_payload
                .get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        })
        .filter_map(|marker| {
            counted_surface_component_path_at(&lane.native_payload, marker)
                .map(|components| (marker, components))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(offset, _)| *offset);
    candidates.dedup();
    if candidates.len() == 1 {
        candidates
    } else {
        Vec::new()
    }
}

fn operation_surface_selection_candidates(
    operation: FeatureClass,
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
    object_source: Option<u32>,
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    if operation == FeatureClass::CutWithSurface {
        return (start..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
            .filter_map(|marker| {
                let selector = lane
                    .native_payload
                    .get(marker.checked_sub(8)?..marker - 4)?;
                let components = if selector[1..] == [2, 0, 0] && selector[0] == 0 {
                    compact_component_reference_list(&lane.native_payload, marker, false)?
                        .into_iter()
                        .flatten()
                        .collect()
                } else if selector[1..] == [2, 0, 0] && matches!(selector[0], 4 | 6) {
                    compact_surface_selection_at(&lane.native_payload, marker)?
                } else {
                    return None;
                };
                Some((marker, components))
            })
            .collect();
    }
    if operation == FeatureClass::SplitFace {
        if !["moPLineProjIdRep_c", "moPLineSurfIdRep_c"]
            .into_iter()
            .all(|required| lane.classes.iter().any(|class| class.name == required))
        {
            return Vec::new();
        }
        let Some(object_source) = object_source else {
            return Vec::new();
        };
        return generated_surface_identities(lane)
            .into_iter()
            .filter(|identity| {
                let Some(first) = identity.components.first() else {
                    return false;
                };
                let Some(last) = identity.components.last() else {
                    return false;
                };
                component_source(first) == Some(object_source)
                    && component_source(last).is_some_and(|source| source != object_source)
                    && last.local_id.is_some()
            })
            .map(|identity| (identity.offset as usize, identity.components))
            .collect();
    }
    if !matches!(
        operation,
        FeatureClass::Dome
            | FeatureClass::Shell
            | FeatureClass::OffsetSurface
            | FeatureClass::KnitSurface
            | FeatureClass::FilledSurface
            | FeatureClass::TrimSurface
            | FeatureClass::ExtendSurface
            | FeatureClass::Draft
            | FeatureClass::DeleteFace
            | FeatureClass::MoveFace
    ) {
        return Vec::new();
    }

    let surface_classes = lane
        .classes
        .iter()
        .filter(|class| {
            class.name == "moCompSurfaceBody_c"
                && usize::try_from(class.offset)
                    .ok()
                    .is_some_and(|offset| (start..end).contains(&offset))
        })
        .collect::<Vec<_>>();
    let mut candidates = if let [surface_class] = surface_classes.as_slice() {
        compact_surface_selection_candidates_for_class(
            &lane.native_payload,
            surface_class,
            start,
            end,
        )
    } else {
        Vec::new()
    };

    candidates.extend(
        lane.classes
            .iter()
            .filter(|class| {
                class.name == "moCompFace_c"
                    && usize::try_from(class.offset)
                        .ok()
                        .is_some_and(|offset| (start..end).contains(&offset))
            })
            .filter_map(|class| {
                let class_offset = usize::try_from(class.offset).ok()?;
                let body = class_offset.checked_add(6 + class.name.len())?;
                component_face_reference_at(&lane.native_payload, body)
            }),
    );
    candidates.sort_by_key(|(offset, _)| *offset);
    candidates.dedup();
    if candidates.len() == 1 {
        candidates
    } else {
        Vec::new()
    }
}

fn component_source(component: &FeatureInputComponentPathEntry) -> Option<u32> {
    let source: [u8; 4] = component.type_signature[4..8].try_into().ok()?;
    Some(u32::from_le_bytes(source))
}

fn compact_surface_selection_candidates_for_class(
    payload: &[u8],
    class: &crate::records::FeatureInputClass,
    start: usize,
    end: usize,
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let Some(class_offset) = usize::try_from(class.offset).ok() else {
        return Vec::new();
    };
    if !(start..end).contains(&class_offset) {
        return Vec::new();
    }
    let Some(body) = class_offset.checked_add(6 + class.name.len()) else {
        return Vec::new();
    };
    let Some(bounded_payload) = payload.get(..end.min(payload.len())) else {
        return Vec::new();
    };
    let Some(last_marker) = bounded_payload
        .len()
        .checked_sub(COMPACT_EDGE_VECTOR_MARKER.len())
    else {
        return Vec::new();
    };
    if body > last_marker {
        return Vec::new();
    }
    (body..=last_marker)
        .filter_map(|marker| {
            compact_surface_selection_at(bounded_payload, marker)
                .map(|components| (marker, components))
        })
        .collect()
}

pub(super) fn history_features_with_object_sources(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<crate::records::Feature> {
    let mut features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    enrich_feature_object_sources(&mut features, std::slice::from_ref(lane));
    features
}

/// Bind flat idless history records to identities from unique feature-input
/// object names without changing records that already carry source identity.
pub(crate) fn enrich_feature_object_sources(
    features: &mut [crate::records::Feature],
    lanes: &[FeatureInputLane],
) {
    for feature in features
        .iter_mut()
        .filter(|feature| feature.source_id.is_none())
    {
        let sources = lanes
            .iter()
            .filter_map(|lane| feature_object_name(feature, lane)?.object_id)
            .collect::<HashSet<_>>();
        if sources.len() == 1 {
            let source = sources
                .iter()
                .next()
                .expect("singleton source set has one member");
            feature.source_id = Some(source.to_string());
        }
    }
}

pub(super) fn cosmetic_thread_cylinder_references(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
    cylinder_reference_tokens: &HashSet<u16>,
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let mut ranges = Vec::with_capacity(2);
    ranges.push(object_start..object_end);
    if let Some(range) = cosmetic_thread_diameter_child_tail(feature, lane) {
        ranges.push(range);
    }
    let mut offsets = ranges
        .into_iter()
        .flatten()
        .filter(|offset| {
            lane.native_payload
                .get(*offset..*offset + 2)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u16::from_le_bytes)
                .is_some_and(|token| cylinder_reference_tokens.contains(&token))
        })
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
        .into_iter()
        .find_map(|offset| cosmetic_thread_cylinder_reference_at(&lane.native_payload, offset))
        .into_iter()
        .collect()
}

pub(super) fn cosmetic_thread_cylinder_marker_reference(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
    cylinder_reference_tokens: &HashSet<u16>,
) -> Vec<(usize, Option<Vec<FeatureInputComponentPathEntry>>)> {
    let diameter_tail = cosmetic_thread_diameter_child_tail(feature, lane);
    let mut markers = std::iter::once(object_start..object_end)
        .chain(diameter_tail)
        .flatten()
        .filter(|offset| {
            lane.native_payload
                .get(*offset..*offset + 2)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u16::from_le_bytes)
                .is_some_and(|token| cylinder_reference_tokens.contains(&token))
        })
        .filter_map(|body| {
            cosmetic_thread_cylinder_reference_marker_layout_at(&lane.native_payload, body)
        })
        .collect::<Vec<_>>();
    markers.sort_unstable();
    markers.dedup();
    markers
        .into_iter()
        .map(|marker| {
            let components = compact_sketch_surface_component_path_at(&lane.native_payload, marker)
                .or_else(|| compact_termination_reference_path_at(&lane.native_payload, marker))
                .or_else(|| compact_edge_component_path_at(&lane.native_payload, marker));
            (marker, components)
        })
        .collect()
}

pub(super) fn cosmetic_thread_diameter_child_tail(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
) -> Option<std::ops::Range<usize>> {
    let source_id = feature.source_id.as_deref()?.parse::<u32>().ok()?;
    let diameter_id = source_id.checked_sub(1)?;
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<HashMap<_, _>>();
    let mut diameters = lane.scalars.iter().filter(|scalar| {
        scalar.object_id == diameter_id
            && names
                .get(scalar.name.as_str())
                .is_some_and(|name| name.value == "D2")
    });
    let diameter = diameters.next()?;
    if diameters.next().is_some() {
        return None;
    }
    let start = usize::try_from(diameter.offset).ok()?.checked_add(8)?;
    let end = lane
        .scalars
        .iter()
        .map(|scalar| scalar.offset)
        .chain(
            lane.names
                .iter()
                .filter(|name| name.object_id != Some(u32::MAX))
                .map(|name| name.offset),
        )
        .filter(|offset| *offset >= start as u64)
        .min()
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(lane.native_payload.len());
    (start < end).then_some(start..end)
}

pub(super) fn cosmetic_thread_cylinder_reference_at(
    payload: &[u8],
    body_offset: usize,
) -> Option<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let marker = cosmetic_thread_cylinder_reference_marker_layout_at(payload, body_offset)?;
    compact_sketch_surface_component_path_at(payload, marker)
        .or_else(|| compact_termination_reference_path_at(payload, marker))
        .or_else(|| compact_edge_component_path_at(payload, marker))
        .map(|components| (marker, components))
}

fn cosmetic_thread_cylinder_reference_marker_layout_at(
    payload: &[u8],
    body_offset: usize,
) -> Option<usize> {
    let body = payload.get(body_offset..body_offset + 11)?;
    let nested_token = u16::from_le_bytes(body[2..4].try_into().ok()?);
    if nested_token & 0x8000 == 0
        || nested_token == 0xffff
        || body[4..8] != 2u32.to_le_bytes()
        || !matches!(body[8], 0 | 0x40)
        || body[9..11] != [0, 0]
    {
        return None;
    }
    [46, 62, 66, 70, 90, 94, 102, 106, 110]
        .into_iter()
        .find_map(|relative| {
            let marker = body_offset.checked_add(relative)?;
            let count = u32::from_le_bytes(
                payload
                    .get(marker.checked_sub(12)?..marker - 8)?
                    .try_into()
                    .ok()?,
            );
            ((1..=64).contains(&count)
                && payload
                    .get(marker - 8..marker - 4)
                    .is_some_and(is_component_vector_selector)
                && payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
                    == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                && payload.get(marker + COMPACT_EDGE_VECTOR_MARKER.len()..marker + 18)
                    == Some(&[0, 0]))
            .then_some(marker)
        })
}

pub(super) fn component_face_reference_at(
    payload: &[u8],
    body_offset: usize,
) -> Option<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let token = u16::from_le_bytes(payload.get(body_offset..body_offset + 2)?.try_into().ok()?);
    let flags = payload.get(body_offset + 6..body_offset + 8)?;
    if token & 0x8000 == 0
        || token == 0xffff
        || payload.get(body_offset + 2..body_offset + 6)? != 2u32.to_le_bytes()
        || !matches!(flags, [0 | 0x40, 0])
    {
        return None;
    }
    let marker_offsets: &[usize] = if flags == [0x40, 0] {
        &[100]
    } else {
        &[68, 92]
    };
    marker_offsets.iter().find_map(|relative| {
        let marker = body_offset.checked_add(*relative)?;
        compact_surface_reference_at(payload, marker).map(|components| (marker, components))
    })
}

pub(super) fn component_face_reference_in_record(
    payload: &[u8],
) -> Option<(usize, Vec<FeatureInputComponentPathEntry>)> {
    const CLASS: &[u8] = b"moCompFace_c";
    let header_length = CLASS_MARKER.len() + 2 + CLASS.len();
    let mut references = payload
        .windows(header_length)
        .enumerate()
        .filter_map(|(offset, header)| {
            (&header[..CLASS_MARKER.len()] == CLASS_MARKER
                && header[CLASS_MARKER.len()..CLASS_MARKER.len() + 2]
                    == (CLASS.len() as u16).to_le_bytes()
                && &header[CLASS_MARKER.len() + 2..] == CLASS)
                .then_some(offset + header_length)
        })
        .filter_map(|body| component_face_reference_at(payload, body))
        .collect::<Vec<_>>();
    references.sort_by_key(|(offset, _)| *offset);
    references.dedup();
    let [reference] = references.as_slice() else {
        return None;
    };
    Some(reference.clone())
}

pub(super) fn compact_sketch_surface_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if payload.get(marker.checked_sub(12)?..marker - 8)? != 5u32.to_le_bytes()
        || payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let kind = payload.get(marker - 8..marker - 4)?;
    let selector = u32::from_le_bytes(payload.get(marker - 4..marker)?.try_into().ok()?);
    let (components, end) = compact_heterogeneous_component_path(payload, marker + 18, 3)?;
    match kind {
        [_, 3, 0, 0] => Some(components),
        [_, 2, 0, 0] if selector != 0 => {
            let extended = payload.get(end..end + 44).is_some_and(|trailer| {
                trailer[..20] == [0; 20]
                    && trailer[20..24] == 1u32.to_le_bytes()
                    && trailer[24..28] == [0; 4]
                    && trailer[28..32] != [0; 4]
                    && trailer[32..] == [0; 12]
            });
            let compact = payload.get(end..end + 36).is_some_and(|trailer| {
                trailer[..4] != [0; 4]
                    && trailer[4..12] == [0; 8]
                    && trailer[12..16] == 1u32.to_le_bytes()
                    && trailer[16..20] == [0; 4]
                    && trailer[20..24] != [0; 4]
                    && trailer[24..] == [0; 12]
            });
            let short = payload.get(end..end + 32).is_some_and(|trailer| {
                trailer[..8] == [0; 8]
                    && trailer[8..12] == 1u32.to_le_bytes()
                    && trailer[12..16] == [0; 4]
                    && trailer[16..20] != [0; 4]
                    && trailer[20..] == [0; 12]
            });
            (extended || compact || short).then_some(components)
        }
        _ => None,
    }
}

pub(super) fn compact_surface_selection_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(count_start..count_start + 4)? != 6u32.to_le_bytes()
        || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0, 0]
        || !matches!(payload.get(kind_start)?.to_owned(), 4 | 6)
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let mut cursor = marker + 18;
    let signature = payload.get(cursor + 4..cursor + 16)?.to_vec();
    let mut components = Vec::new();
    while payload.get(cursor + 4..cursor + 16) == Some(signature.as_slice()) {
        components.push(FeatureInputComponentPathEntry {
            instance: Some(u16::from_le_bytes(
                payload.get(cursor..cursor + 2)?.try_into().ok()?,
            )),
            type_signature: signature.as_slice().try_into().ok()?,
            local_id: Some(u32::from_le_bytes(
                payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
            )),
        });
        cursor += 20;
        if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice())
            && payload.get(cursor + 8..cursor + 20) == Some(signature.as_slice())
        {
            cursor += 4;
        }
    }
    (!components.is_empty()).then_some(components)
}

pub(crate) fn compact_surface_reference_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    compact_surface_selection_at(payload, marker)
        .or_else(|| component_vector_path_at(payload, marker))
        .or_else(|| {
            compact_component_reference_list_at(payload, marker)
                .map(|references| references.into_iter().flatten().collect())
        })
        .or_else(|| {
            compact_component_reference_list(payload, marker, false)
                .map(|references| references.into_iter().flatten().collect())
        })
        .or_else(|| counted_surface_component_path_at(payload, marker))
        .or_else(|| compact_termination_reference_path_at(payload, marker))
        .or_else(|| compact_sketch_surface_component_path_at(payload, marker))
        .or_else(|| inline_surface_reference_at(payload, marker))
}

pub(crate) fn surface_reference_matches_at(
    payload: &[u8],
    marker: usize,
    expected: &[FeatureInputComponentPathEntry],
) -> bool {
    [
        compact_surface_selection_at(payload, marker),
        component_vector_path_at(payload, marker),
        compact_component_reference_list_at(payload, marker)
            .map(|references| references.into_iter().flatten().collect()),
        compact_component_reference_list(payload, marker, false)
            .map(|references| references.into_iter().flatten().collect()),
        counted_surface_component_path_at(payload, marker),
        compact_termination_reference_path_at(payload, marker),
        compact_sketch_surface_component_path_at(payload, marker),
        inline_surface_reference_at(payload, marker),
    ]
    .into_iter()
    .flatten()
    .any(|components| components == expected)
}

fn repeated_edge_selections(
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Vec<(usize, Vec<u32>)> {
    let token = token.to_le_bytes();
    let mut selections = Vec::new();
    for offset in start..end.saturating_sub(110) {
        if payload.get(offset..offset + 2) != Some(token.as_slice())
            || payload.get(offset + 2) != Some(&2)
        {
            continue;
        }
        let marker = offset + 108;
        if let Some(ids) = compact_edge_selection_at(payload, marker) {
            selections.push((marker, ids));
        }
    }
    selections
}

fn edge_selection_vectors_in_interval(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<(usize, Vec<u32>)> {
    (start.saturating_add(12)..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
        .filter(|marker| {
            payload.get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        })
        .filter_map(|marker| compact_edge_selection_at(payload, marker).map(|ids| (marker, ids)))
        .collect()
}

pub(super) const COMPACT_EDGE_VECTOR_MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

/// Component-vector selectors carry a lane-specific low subtype byte. The
/// high role byte identifies the path family; the low byte is not a fixed
/// discriminator and therefore must not be required to be zero.
pub(super) fn is_component_vector_selector(selector: &[u8]) -> bool {
    matches!(selector, [_, 2 | 3, 0, 0])
}

pub(super) fn is_component_vector_selector_for_role(selector: &[u8], role: u8) -> bool {
    matches!(role, 2 | 3) && selector.get(1) == Some(&role) && selector.get(2..4) == Some(&[0, 0])
}

pub(super) fn mirror_pattern_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker - 8..marker)? != [0; 8]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    component_vector_path_at(payload, marker)
}

pub(super) fn component_vector_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let header = marker.checked_sub(12)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let cell_count = usize::try_from(u32::from_le_bytes(
        payload.get(header..header + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (2..=65).contains(count))?;
    let candidates = [
        compact_heterogeneous_component_path(payload, marker + 18, cell_count - 1),
        (cell_count > 2)
            .then(|| compact_heterogeneous_component_path(payload, marker + 18, cell_count - 2))
            .flatten(),
        compact_mixed_component_path(payload, marker + 18, cell_count, true),
        compact_mixed_component_path(payload, marker + 18, cell_count - 1, true),
        (cell_count > 2)
            .then(|| compact_mixed_component_path(payload, marker + 18, cell_count - 2, true))
            .flatten(),
        (cell_count % 2 == 1)
            .then(|| {
                compact_mixed_component_path(payload, marker + 18, cell_count.div_ceil(2), true)
            })
            .flatten(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let candidates = distinct_candidates(
        // A shorter root-slot interpretation is incomplete when another valid
        // entry follows its end; the remaining entry is part of this path.
        candidates
            .into_iter()
            .filter(|(_, end)| !component_path_continues(payload, *end, true)),
    );
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.0.clone())
}

fn component_path_continues(payload: &[u8], end: usize, root_separators: bool) -> bool {
    [0usize, 2, 4, 6, 8, 10, 12].into_iter().any(|gap| {
        let root_separator = root_separators
            && gap == 10
            && payload.get(end..end + 10) == Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        (compact_component_separator(payload, end, gap) || root_separator)
            && (compact_heterogeneous_component_path(payload, end + gap, 1).is_some()
                || compact_mixed_component_path(payload, end + gap, 1, root_separators).is_some())
    })
}

pub(super) fn compact_mixed_component_path(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    root_separators: bool,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    let signature_at = |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let type_family = u16::from_le_bytes(signature[0..2].try_into().ok()?);
        let type_variant = u16::from_le_bytes(signature[2..4].try_into().ok()?);
        let source = u32::from_le_bytes(signature[4..8].try_into().ok()?);
        let identity = u32::from_le_bytes(signature[8..12].try_into().ok()?);
        (type_family & 0x8000 != 0
            && type_family != u16::MAX
            && type_variant != 0
            && source != 0
            && identity != 0)
            .then_some(signature)
    };
    let node_at = |offset: usize| -> Option<(FeatureInputComponentPathEntry, usize)> {
        let tagged = payload
            .get(offset..offset + 4)
            .is_some_and(|bytes| {
                let instance = u16::from_le_bytes([bytes[0], bytes[1]]);
                instance & 0x8000 != 0 && instance != u16::MAX && bytes[2..4] == [0, 0]
            })
            .then(|| {
                Some((
                    FeatureInputComponentPathEntry {
                        instance: Some(u16::from_le_bytes(
                            payload.get(offset..offset + 2)?.try_into().ok()?,
                        )),
                        type_signature: signature_at(offset + 4)?,
                        local_id: Some(u32::from_le_bytes(
                            payload.get(offset + 16..offset + 20)?.try_into().ok()?,
                        )),
                    },
                    20,
                ))
            })
            .flatten();
        tagged.or_else(|| {
            Some((
                FeatureInputComponentPathEntry {
                    instance: None,
                    type_signature: signature_at(offset)?,
                    local_id: Some(u32::from_le_bytes(
                        payload.get(offset + 12..offset + 16)?.try_into().ok()?,
                    )),
                },
                16,
            ))
        })
    };

    let mut components = Vec::with_capacity(count);
    for index in 0..count {
        let (component, len) = node_at(cursor)?;
        components.push(component);
        cursor += len;
        if index + 1 == count {
            continue;
        }
        let gap = [0usize, 2, 4, 6, 8, 10, 12].into_iter().find(|gap| {
            let root_separator = root_separators
                && *gap == 10
                && payload.get(cursor..cursor + 10) == Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            (compact_component_separator(payload, cursor, *gap) || root_separator)
                && node_at(cursor + *gap).is_some()
        })?;
        cursor += gap;
    }
    Some((components, cursor))
}

pub(super) fn counted_surface_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let header = marker.checked_sub(12)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker - 7..marker - 4)? != [2, 0, 0]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(header..header + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    let candidates = [
        compact_mixed_component_path(payload, marker + 18, count, false),
        (count > 1)
            .then(|| compact_mixed_component_path(payload, marker + 18, count - 1, false))
            .flatten(),
    ]
    .into_iter()
    .flatten()
    .filter(|(_, end)| !component_path_continues(payload, *end, false));
    let candidates = distinct_candidates(candidates);
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.0.clone())
}

fn mirror_surface_type_prefix(lane: &FeatureInputLane) -> Option<[u8; 4]> {
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moMirPatternSurfIdRep_c");
    let class = classes.next().filter(|_| classes.next().is_none())?;
    let offset = usize::try_from(class.offset).ok()?;
    let signature = offset.checked_add(8 + class.name.len())?;
    let prefix: [u8; 4] = lane
        .native_payload
        .get(signature..signature + 4)?
        .try_into()
        .ok()?;
    let family = u16::from_le_bytes(prefix[..2].try_into().ok()?);
    let variant = u16::from_le_bytes(prefix[2..].try_into().ok()?);
    (family & 0x8000 != 0 && family != u16::MAX && variant != 0).then_some(prefix)
}

fn inline_mirror_surface_paths(
    payload: &[u8],
    start: usize,
    end: usize,
    prefix: [u8; 4],
) -> Vec<(usize, Vec<FeatureInputComponentPathEntry>)> {
    let signature_at = |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let source = u32::from_le_bytes(signature[4..8].try_into().ok()?);
        let identity = u32::from_le_bytes(signature[8..12].try_into().ok()?);
        (signature[..4] == prefix && source != 0 && identity != 0).then_some(signature)
    };
    let instance_before = |offset: usize| -> Option<u16> {
        let bytes = payload.get(offset.checked_sub(4)?..offset)?;
        let instance = u16::from_le_bytes(bytes[..2].try_into().ok()?);
        (instance & 0x8000 != 0 && instance != u16::MAX && bytes[2..] == [0, 0]).then_some(instance)
    };
    let mut result = Vec::new();
    for terminal in start..end.saturating_sub(16) {
        if signature_at(terminal).is_none() {
            continue;
        }
        let local_bytes = &payload[terminal + 12..terminal + 16];
        let next_is_component = {
            let instance = u16::from_le_bytes(
                local_bytes[..2]
                    .try_into()
                    .expect("two-byte instance slice"),
            );
            instance & 0x8000 != 0
                && instance != u16::MAX
                && local_bytes[2..] == [0, 0]
                && signature_at(terminal + 16).is_some()
        };
        if next_is_component {
            continue;
        }
        let mut cursor = terminal;
        while instance_before(cursor).is_some() {
            let Some(previous) = cursor.checked_sub(16) else {
                break;
            };
            if signature_at(previous).is_none() {
                break;
            }
            cursor = previous;
        }
        let offset = cursor;
        let Some(components) = inline_surface_reference_at(payload, offset) else {
            continue;
        };
        if !result.iter().any(|(_, existing)| existing == &components) {
            result.push((offset, components));
        }
    }
    result
}

pub(super) fn inline_surface_reference_at(
    payload: &[u8],
    offset: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let prefix: [u8; 4] = payload.get(offset..offset + 4)?.try_into().ok()?;
    let family = u16::from_le_bytes(prefix[..2].try_into().ok()?);
    let variant = u16::from_le_bytes(prefix[2..].try_into().ok()?);
    if family & 0x8000 == 0 || family == u16::MAX || variant == 0 {
        return None;
    }
    let signature_at = |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let source = u32::from_le_bytes(signature[4..8].try_into().ok()?);
        let identity = u32::from_le_bytes(signature[8..12].try_into().ok()?);
        (signature[..4] == prefix && source != 0 && identity != 0).then_some(signature)
    };
    let instance_before = |offset: usize| -> Option<u16> {
        let bytes = payload.get(offset.checked_sub(4)?..offset)?;
        let instance = u16::from_le_bytes(bytes[..2].try_into().ok()?);
        (instance & 0x8000 != 0 && instance != u16::MAX && bytes[2..] == [0, 0]).then_some(instance)
    };
    let mut cursor = offset;
    let mut components = Vec::new();
    loop {
        let signature = signature_at(cursor)?;
        let tail: [u8; 4] = payload.get(cursor + 12..cursor + 16)?.try_into().ok()?;
        let instance = u16::from_le_bytes(tail[..2].try_into().ok()?);
        let continues = instance & 0x8000 != 0
            && instance != u16::MAX
            && tail[2..] == [0, 0]
            && signature_at(cursor + 16).is_some();
        components.push(FeatureInputComponentPathEntry {
            instance: instance_before(cursor),
            type_signature: signature,
            local_id: (!continues).then(|| u32::from_le_bytes(tail)),
        });
        if !continues {
            return Some(components);
        }
        cursor += 16;
    }
}

/// Decode persistent surface identities declared by `*SurfIdRep_c` classes.
/// Operation-specific consumers separately project the identities that also
/// carry input selections, including projected split-line target faces.
pub(crate) fn generated_surface_identities(
    lane: &FeatureInputLane,
) -> Vec<crate::records::FeatureInputGeneratedSurfaceIdentity> {
    let signature_prefix_at = |offset: usize, prefix: [u8; 4]| -> Option<[u8; 12]> {
        let signature: [u8; 12] = lane
            .native_payload
            .get(offset..offset + 12)?
            .try_into()
            .ok()?;
        let source = u32::from_le_bytes(signature[4..8].try_into().ok()?);
        let identity = u32::from_le_bytes(signature[8..12].try_into().ok()?);
        (signature[..4] == prefix && source != 0 && identity != 0).then_some(signature)
    };
    let instance_before = |offset: usize| -> Option<u16> {
        let bytes = lane.native_payload.get(offset.checked_sub(4)?..offset)?;
        let instance = u16::from_le_bytes(bytes[..2].try_into().ok()?);
        (instance & 0x8000 != 0 && instance != u16::MAX && bytes[2..] == [0, 0]).then_some(instance)
    };
    let prefixes = lane
        .classes
        .iter()
        .filter(|class| class.name.ends_with("SurfIdRep_c"))
        .filter_map(|class| {
            let body = usize::try_from(class.offset)
                .ok()?
                .checked_add(6 + class.name.len())?;
            if lane.native_payload.get(body..body + 2)? != [0, 0] {
                return None;
            }
            let prefix: [u8; 4] = lane
                .native_payload
                .get(body + 2..body + 6)?
                .try_into()
                .ok()?;
            let family = u16::from_le_bytes(prefix[..2].try_into().ok()?);
            let variant = u16::from_le_bytes(prefix[2..].try_into().ok()?);
            if family & 0x8000 == 0 || family == u16::MAX || variant == 0 {
                return None;
            }
            Some(prefix)
        })
        .collect::<HashSet<_>>();

    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for terminal in 0..=lane.native_payload.len().saturating_sub(16) {
        let Some(prefix) = lane
            .native_payload
            .get(terminal..terminal + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .filter(|prefix| prefixes.contains(prefix))
        else {
            continue;
        };
        let Some(signature) = signature_prefix_at(terminal, prefix) else {
            continue;
        };
        let tail: [u8; 4] = lane.native_payload[terminal + 12..terminal + 16]
            .try_into()
            .expect("bounded surface identity tail");
        let possible_instance =
            u16::from_le_bytes(tail[..2].try_into().expect("two-byte instance slice"));
        if possible_instance & 0x8000 != 0
            && possible_instance != u16::MAX
            && tail[2..] == [0, 0]
            && signature_prefix_at(terminal + 16, prefix).is_some()
        {
            continue;
        }
        let mut offset = terminal;
        while instance_before(offset).is_some()
            && offset
                .checked_sub(16)
                .is_some_and(|previous| signature_prefix_at(previous, prefix).is_some())
        {
            offset -= 16;
        }
        let Some(components) = inline_surface_reference_at(&lane.native_payload, offset) else {
            continue;
        };
        let feature_source_id = u32::from_le_bytes(
            signature[4..8]
                .try_into()
                .expect("four-byte feature source ID slice"),
        );
        let local_identity = u32::from_le_bytes(tail);
        let key = (
            prefix,
            components
                .iter()
                .map(|component| {
                    (
                        component.instance,
                        component.type_signature,
                        component.local_id,
                    )
                })
                .collect::<Vec<_>>(),
        );
        if !seen.insert(key) {
            continue;
        }
        result.push(crate::records::FeatureInputGeneratedSurfaceIdentity {
            id: String::new(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: offset as u64,
            type_prefix: prefix,
            feature_source_id,
            local_identity,
            components,
        });
    }
    result.sort_by_key(|identity| identity.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    for (ordinal, identity) in result.iter_mut().enumerate() {
        identity.ordinal = ordinal as u32;
        identity.id = format!(
            "sldprt:feature-input:generated-surface#{lane_key}:{}",
            identity.offset
        );
    }
    result
}

fn compact_edge_selection_vector(payload: &[u8], base: usize) -> Option<(usize, Vec<u32>)> {
    for marker in 12..=payload
        .len()
        .saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len())
    {
        if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) {
            continue;
        }
        if let Some(ids) = compact_edge_selection_at(payload, marker) {
            return Some((base + marker, ids));
        }
    }
    None
}

pub(crate) fn compact_edge_selection_at(payload: &[u8], marker: usize) -> Option<Vec<u32>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    if let Some(references) = compact_component_reference_list_at(payload, marker) {
        return Some(
            references
                .iter()
                .filter_map(|reference| reference.last()?.local_id)
                .collect(),
        );
    }
    let mut candidates = Vec::new();
    if let Some(ids) = compact_homogeneous_edge_ids(payload, marker + 18, count) {
        candidates.push(ids);
    }
    for (components, _) in compact_edge_component_path_candidates(payload, marker, count) {
        let ids = components
            .into_iter()
            .filter_map(|component| component.local_id)
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            candidates.push(ids);
        }
    }
    if let Some(ids) = compact_u16_edge_ids(payload, marker + 18, count) {
        candidates.push(ids);
    }
    let candidates = distinct_candidates(candidates);
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(crate) fn compact_edge_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    compact_component_reference_list_at(payload, marker)
        .map(|references| {
            references
                .into_iter()
                .filter(|reference| {
                    !reference
                        .iter()
                        .any(|component| component.instance == Some(0x8083))
                })
                .flatten()
                .collect()
        })
        .or_else(|| {
            compact_edge_component_path(payload, marker, count).map(|(components, _)| components)
        })
}

pub(crate) fn compact_component_reference_list_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<Vec<FeatureInputComponentPathEntry>>> {
    compact_component_reference_list(payload, marker, true)
}

fn compact_component_reference_list(
    payload: &[u8],
    marker: usize,
    require_distinct_framing: bool,
) -> Option<Vec<Vec<FeatureInputComponentPathEntry>>> {
    let count_start = marker.checked_sub(12)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker - 7..marker - 4)? != [0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    let signature_prefix: [u8; 4] = payload.get(marker + 22..marker + 26)?.try_into().ok()?;
    let prefix_token = u16::from_le_bytes(signature_prefix[..2].try_into().ok()?);
    let prefix_variant = u16::from_le_bytes(signature_prefix[2..].try_into().ok()?);
    if prefix_token & 0x8000 == 0 || prefix_token == u16::MAX || prefix_variant == 0 {
        return None;
    }
    let hop_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = u16::from_le_bytes(payload.get(offset..offset + 2)?.try_into().ok()?);
        if instance & 0x8000 == 0
            || instance == u16::MAX
            || payload.get(offset + 2..offset + 4)? != [0, 0]
        {
            return None;
        }
        let type_signature: [u8; 12] = payload.get(offset + 4..offset + 16)?.try_into().ok()?;
        (type_signature[..4] == signature_prefix
            && type_signature[4..8] != [0; 4]
            && type_signature[8..12] != [0; 4])
            .then_some(FeatureInputComponentPathEntry {
                instance: Some(instance),
                type_signature,
                local_id: None,
            })
    };
    let terminal_null_at = |offset: usize| {
        (16..=18).any(|zero_count| {
            payload
                .get(offset..offset + zero_count)
                .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                && payload.get(offset + zero_count..offset + zero_count + 3)
                    == Some(&[0xff, 0xfe, 0xff])
        })
    };

    let mut cursor = marker + 18;
    let mut references = Vec::with_capacity(count);
    let mut has_reference_framing = false;
    for index in 0..count {
        let mut reference = Vec::new();
        while let Some(hop) = hop_at(cursor) {
            reference.push(hop);
            cursor += 16;
        }
        if reference.is_empty() && index + 1 == count && terminal_null_at(cursor) {
            return (!references.is_empty()).then_some(references);
        }
        if reference.is_empty() {
            return (!require_distinct_framing && !references.is_empty()).then_some(references);
        }
        has_reference_framing |= reference.len() > 1;
        let last = reference.last_mut()?;
        last.local_id = Some(u32::from_le_bytes(
            payload.get(cursor..cursor + 4)?.try_into().ok()?,
        ));
        cursor += 4;
        if payload.get(cursor..cursor + 4) == Some(&[0xff; 4]) {
            cursor += 4;
            has_reference_framing = true;
        }
        references.push(reference);
        if index + 2 == count && terminal_null_at(cursor) {
            return Some(references);
        }
        if index + 1 == count {
            continue;
        }
        let Some(gap) = (0..=10).find(|gap| {
            payload
                .get(cursor..cursor + *gap)
                .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
                && hop_at(cursor + *gap).is_some()
        }) else {
            return (!require_distinct_framing && !references.is_empty()).then_some(references);
        };
        cursor += gap;
    }
    (!require_distinct_framing || has_reference_framing).then_some(references)
}

pub(crate) fn variable_fillet_control_vertices(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_end: usize,
) -> Option<Vec<(String, [u8; 12])>> {
    if !feature.kind.eq_ignore_ascii_case("VarFillet") {
        return None;
    }
    let object_start = feature_object_name(feature, lane)?.offset.try_into().ok()?;
    let control_start = fillet_edge_roster_end(lane, object_start, object_end)?;
    let mut controls = (control_start.saturating_add(12)
        ..object_end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
        .filter(|marker| {
            lane.native_payload
                .get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        })
        .filter_map(|marker| {
            let references = compact_component_reference_list(&lane.native_payload, marker, false)?;
            (references.len() == 3).then_some(())?;
            let vertices = references
                .iter()
                .flat_map(|reference| reference.iter())
                .filter(|component| component.instance == Some(0x8083))
                .collect::<Vec<_>>();
            let [vertex] = vertices.as_slice() else {
                return None;
            };
            Some((marker, vertex.type_signature))
        })
        .collect::<Vec<_>>();
    controls.sort_unstable_by_key(|(marker, _)| *marker);
    let mut result = Vec::with_capacity(controls.len());
    for (index, &(marker, signature)) in controls.iter().enumerate() {
        let start = index
            .checked_sub(1)
            .and_then(|previous| controls.get(previous))
            .map_or(control_start, |(previous, _)| *previous);
        let names = lane
            .names
            .iter()
            .filter(|name| {
                usize::try_from(name.offset).is_ok_and(|offset| start < offset && offset < marker)
            })
            .filter(|name| variable_fillet_dimension_index(&name.value).is_some())
            .collect::<Vec<_>>();
        let [name] = names.as_slice() else {
            return None;
        };
        result.push((name.value.clone(), signature));
    }
    (!result.is_empty()).then_some(result)
}

pub(crate) fn variable_fillet_dimension_index(name: &str) -> Option<usize> {
    let suffix = name.strip_prefix("D0")?;
    let index = if suffix.is_empty() {
        0
    } else {
        suffix.parse().ok()?
    };
    let canonical = if index == 0 {
        "D0".to_string()
    } else {
        format!("D0{index}")
    };
    (name == canonical).then_some(index)
}

pub(super) fn compact_component_path_end_at(payload: &[u8], marker: usize) -> Option<usize> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    let candidates = distinct_candidates(
        [
            compact_wide_component_path(payload, marker + 18, count),
            compact_heterogeneous_component_path(payload, marker + 18, count),
            compact_sparse_component_path(payload, marker + 18, count),
        ]
        .into_iter()
        .flatten(),
    );
    let [(_, end)] = candidates.as_slice() else {
        return None;
    };
    Some(*end)
}

fn compact_edge_component_path(
    payload: &[u8],
    marker: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, Option<u32>)> {
    let candidates = compact_edge_component_path_candidates(payload, marker, count);
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn compact_edge_component_path_candidates(
    payload: &[u8],
    marker: usize,
    count: usize,
) -> Vec<(Vec<FeatureInputComponentPathEntry>, Option<u32>)> {
    let component_paths = |entry_count| {
        let candidates = [
            compact_wide_component_path(payload, marker + 18, entry_count),
            compact_heterogeneous_component_path(payload, marker + 18, entry_count),
            compact_sparse_component_path(payload, marker + 18, entry_count),
        ];
        distinct_candidates(candidates.into_iter().flatten())
    };
    let terminal_paths = if count > 1 {
        component_paths(count - 1)
            .into_iter()
            .filter_map(|(components, end)| {
                Some((components, end, edge_terminal_source_at(payload, end)?))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut candidates = terminal_paths
        .iter()
        .map(|(components, _, source)| (components.clone(), Some(*source)))
        .collect::<Vec<_>>();
    candidates.extend(
        component_paths(count)
            .into_iter()
            .filter(|(components, end)| {
                !terminal_paths.iter().any(|(prefix, prefix_end, _)| {
                    *prefix_end < *end && components.starts_with(prefix)
                })
            })
            .map(|(components, _)| (components, None)),
    );
    distinct_candidates(candidates)
}

fn edge_terminal_source_at(payload: &[u8], end: usize) -> Option<u32> {
    let trailer = payload.get(end..end + 36)?;
    if trailer[..8] != [1, 0, 0, 0, 0, 0, 0, 0]
        || trailer[8..12] != [0x4a, 0x80, 0, 0]
        || trailer[12..14] == [0, 0]
        || trailer[14..16] != [0x37, 0]
        || trailer[20..24].iter().all(|byte| *byte == 0)
        || trailer[24..].iter().any(|byte| *byte != 0)
    {
        return None;
    }
    let source = u32::from_le_bytes(trailer[16..20].try_into().ok()?);
    (source != 0).then_some(source)
}

fn distinct_candidates<T: PartialEq>(candidates: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut distinct = Vec::new();
    for candidate in candidates {
        if !distinct.contains(&candidate) {
            distinct.push(candidate);
        }
    }
    distinct
}

pub(crate) fn compact_edge_owner_feature_at(
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Option<String> {
    let count = usize::try_from(u32::from_le_bytes(
        payload
            .get(marker.checked_sub(12)?..marker - 8)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    let owner_source = if compact_component_reference_list_at(payload, marker).is_some() {
        None
    } else {
        compact_edge_component_path(payload, marker, count)?.1
    };
    owner_source
        .and_then(|source| {
            features.iter().find(|feature| {
                feature.source_id.as_deref().and_then(|id| id.parse().ok()) == Some(source)
            })
        })
        .filter(|feature| feature_precedes_consumer(feature, features, consumer_ref))
        .map(|feature| feature.id.clone())
        .or_else(|| component_path_input_features(components, features, consumer_ref).pop())
}

pub(crate) fn compact_edge_producer_features_at(
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Vec<String> {
    let mut producers = component_path_input_features(components, features, consumer_ref);
    if let Some(owner) =
        compact_edge_owner_feature_at(payload, marker, components, features, consumer_ref)
    {
        if !producers.contains(&owner) {
            producers.push(owner);
        }
    }
    producers
}

pub(crate) fn surface_selection_terminal_feature_at(
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Option<String> {
    compact_single_face_reference_record_at(payload, marker)
        .and_then(|(_, source)| source)
        .and_then(|source| {
            let mut matches = features.iter().filter(|candidate| {
                candidate
                    .source_id
                    .as_deref()
                    .and_then(|value| value.parse::<u32>().ok())
                    == Some(source)
            });
            let feature = matches.next()?;
            matches.next().is_none().then(|| feature.id.clone())
        })
        .or_else(|| component_path_terminal_feature(components, features))
}

fn compact_homogeneous_edge_ids(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
) -> Option<Vec<u32>> {
    let signature = payload.get(cursor + 4..cursor + 16)?.to_vec();
    // Each edge id consumes at least a 20-byte record from `cursor` onward.
    bounded_len(count as u64, 20, payload.len().saturating_sub(cursor))?;
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        if payload.get(cursor + 4..cursor + 16)? != signature {
            return None;
        }
        ids.push(u32::from_le_bytes(
            payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
        ));
        cursor += 20;
        if index + 1 < count && payload.get(cursor + 4..cursor + 16)? != signature {
            if payload.get(cursor..cursor + 4)? == [0; 4]
                && payload.get(cursor + 8..cursor + 20)? == signature
            {
                cursor += 4;
            } else {
                match payload.get(cursor..cursor + 8)? {
                    [0, 0, 0, 0, 0, 0, 0, 0] | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0] => {
                        cursor += 8;
                    }
                    _ => return None,
                }
            }
        }
    }
    Some(ids)
}

pub(super) fn compact_heterogeneous_component_path(
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    compact_component_path_with_layout(payload, cursor, count, false)
}

fn compact_wide_component_path(
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    compact_component_path_with_layout(payload, cursor, count, true)
}

fn compact_component_path_with_layout(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    wide: bool,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    let entry_length = if wide { 24 } else { 20 };
    let local_id_offset = if wide { 20 } else { 16 };
    let entry_at = |offset: usize| {
        let instance = payload.get(offset..offset + 4)?;
        let token = u16::from_le_bytes(instance[0..2].try_into().ok()?);
        (token & 0x8000 != 0
            && token != u16::MAX
            && instance[2..4] == [0, 0]
            && payload.get(offset + 4..offset + 6)? != [0, 0]
            && (!wide || payload.get(offset + 16..offset + 20)? == [0; 4])
            && payload
                .get(offset + local_id_offset..offset + entry_length)
                .is_some())
        .then_some(())
    };
    bounded_len(
        count as u64,
        entry_length,
        payload.len().saturating_sub(cursor),
    )?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        entry_at(cursor)?;
        entries.push(FeatureInputComponentPathEntry {
            instance: Some(u16::from_le_bytes(
                payload.get(cursor..cursor + 2)?.try_into().ok()?,
            )),
            type_signature: payload.get(cursor + 4..cursor + 16)?.try_into().ok()?,
            local_id: Some(u32::from_le_bytes(
                payload
                    .get(cursor + local_id_offset..cursor + entry_length)?
                    .try_into()
                    .ok()?,
            )),
        });
        cursor += entry_length;
        if index + 1 == count {
            continue;
        }
        let gap = [0usize, 2, 4, 6, 8, 10, 12].into_iter().find(|gap| {
            compact_component_separator(payload, cursor, *gap) && entry_at(cursor + *gap).is_some()
        })?;
        cursor += gap;
    }
    Some((entries, cursor))
}

fn compact_component_separator(payload: &[u8], cursor: usize, gap: usize) -> bool {
    match gap {
        0 => true,
        2 => payload.get(cursor..cursor + 2) == Some(&[0; 2]),
        4 => payload.get(cursor..cursor + 4).is_some_and(|bytes| {
            bytes == [0; 4]
                || bytes == [0xff; 4]
                || (u16::from_le_bytes([bytes[0], bytes[1]]) & 0x8000 != 0
                    && bytes[0..2] != [0xff, 0xff]
                    && bytes[2..4] == [1, 0])
                || (u16::from_le_bytes([bytes[0], bytes[1]]) != 0
                    && bytes[0..2] != [0xff, 0xff]
                    && bytes[2..4] == [0, 0])
        }),
        6 => payload.get(cursor..cursor + 6).is_some_and(|bytes| {
            u16::from_le_bytes([bytes[0], bytes[1]]) != u16::MAX && bytes[2..] == [0; 4]
        }),
        8 => payload.get(cursor..cursor + 8).is_some_and(|bytes| {
            let first = u32::from_le_bytes(bytes[..4].try_into().expect("four-byte state"));
            let second = u32::from_le_bytes(bytes[4..].try_into().expect("four-byte state"));
            (first == 0 && second == 0)
                || (first == u32::MAX && second <= 1)
                || (first == 0 && !matches!(second, 0 | u32::MAX))
                || (second == 0 && !matches!(first, 0 | u32::MAX))
        }),
        10 => payload.get(cursor..cursor + 10) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]),
        12 => payload.get(cursor..cursor + 12) == Some(&[0; 12]),
        _ => false,
    }
}

fn compact_sparse_component_path(
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    fn entry_prefix(payload: &[u8], offset: usize) -> Option<(u16, [u8; 12])> {
        let instance = payload.get(offset..offset + 4)?;
        let token = u16::from_le_bytes(instance[..2].try_into().ok()?);
        (token & 0x8000 != 0
            && token != u16::MAX
            && instance[2..] == [0; 2]
            && payload.get(offset + 4..offset + 6)? != [0; 2])
            .then(|| {
                Some((
                    token,
                    payload.get(offset + 4..offset + 16)?.try_into().ok()?,
                ))
            })
            .flatten()
    }

    fn parse(
        payload: &[u8],
        cursor: usize,
        remaining: usize,
        failed: &mut HashSet<(usize, usize)>,
    ) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
        if !failed.insert((cursor, remaining)) {
            return None;
        }
        let (instance, type_signature) = entry_prefix(payload, cursor)?;
        for entry_length in [20usize, 16] {
            let local_id = if entry_length == 20 {
                Some(u32::from_le_bytes(
                    payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
                ))
            } else {
                None
            };
            let entry = FeatureInputComponentPathEntry {
                instance: Some(instance),
                type_signature,
                local_id,
            };
            let end = cursor.checked_add(entry_length)?;
            if remaining == 1 {
                return Some((vec![entry], end));
            }
            for gap in [0usize, 2, 4, 6, 8, 10, 12] {
                if !compact_component_separator(payload, end, gap) {
                    continue;
                }
                let Some(next) = end.checked_add(gap) else {
                    continue;
                };
                let Some((mut tail, path_end)) = parse(payload, next, remaining - 1, failed) else {
                    continue;
                };
                let mut entries = Vec::with_capacity(remaining);
                entries.push(entry);
                entries.append(&mut tail);
                return Some((entries, path_end));
            }
        }
        None
    }

    bounded_len(count as u64, 16, payload.len().saturating_sub(cursor))?;
    parse(payload, cursor, count, &mut HashSet::new())
}

fn compact_u16_edge_ids(payload: &[u8], cursor: usize, count: usize) -> Option<Vec<u32>> {
    let end = cursor.checked_add(count.checked_mul(2)?)?;
    let ids = payload
        .get(cursor..end)?
        .chunks_exact(2)
        .map(|bytes| u32::from(u16::from_le_bytes([bytes[0], bytes[1]])))
        .collect::<Vec<_>>();
    let suffix = payload.get(end..)?;
    let sentinel_terminated = suffix.get(..19).is_some_and(|suffix| {
        suffix[..16].iter().all(|byte| *byte == 0) && suffix[16..19] == [0xff, 0xfe, 0xff]
    });
    let object_terminated = suffix.get(..10).is_some_and(|suffix| {
        suffix[..8].iter().all(|byte| *byte == 0) && {
            let token = u16::from_le_bytes([suffix[8], suffix[9]]);
            token & 0x8000 != 0 && token != u16::MAX
        }
    });
    (ids.iter().all(|id| *id != 0) && (sentinel_terminated || object_terminated)).then_some(ids)
}

pub(super) fn compact_body_selection_vector(
    payload: &[u8],
    base: usize,
    next_object_token: Option<u16>,
) -> Option<(usize, Vec<u32>)> {
    const SCHEMA: &[u8] = &11000u32.to_le_bytes();
    for relative in (0..=payload.len().checked_sub(16)?).rev() {
        if payload.get(relative..relative + 4)? != SCHEMA
            || payload.get(relative + 4..relative + 12)? != [0; 8]
        {
            continue;
        }
        let Some(count_bytes) = payload.get(relative + 12..relative + 16) else {
            continue;
        };
        let Ok(count) = usize::try_from(u32::from_le_bytes(
            count_bytes.try_into().expect("four-byte count"),
        )) else {
            continue;
        };
        let Some(ids_end) = count
            .checked_mul(4)
            .and_then(|byte_len| relative.checked_add(16)?.checked_add(byte_len))
        else {
            continue;
        };
        let Some(sentinel_end) = ids_end.checked_add(4) else {
            continue;
        };
        let Some(zeros_end) = sentinel_end.checked_add(12) else {
            continue;
        };
        let Some(suffix) = payload.get(zeros_end..) else {
            continue;
        };
        let valid_suffix = matches!(suffix, [] | [0, 0, 0, 0])
            || next_object_token.is_some_and(|token| suffix == token.to_le_bytes());
        if payload.get(ids_end..sentinel_end) != Some(u32::MAX.to_le_bytes().as_slice())
            || payload.get(sentinel_end..zeros_end) != Some([0; 12].as_slice())
            || !valid_suffix
        {
            continue;
        }
        let Some(ids) = payload.get(relative + 16..ids_end) else {
            continue;
        };
        let local_body_ids = ids
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
            .collect();
        return Some((base + relative, local_body_ids));
    }
    None
}

pub(crate) fn compact_body_selection_at(payload: &[u8], offset: usize) -> Option<Vec<u32>> {
    if payload.get(offset..offset + 4)? != 11000u32.to_le_bytes()
        || payload.get(offset + 4..offset + 12)? != [0; 8]
    {
        return super::direct_edits::move_body_selection_at(payload, offset);
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(offset + 12..offset + 16)?.try_into().ok()?,
    ))
    .ok()?;
    let ids_end = offset.checked_add(16 + count.checked_mul(4)?)?;
    let sentinel_end = ids_end.checked_add(4)?;
    let zeros_end = sentinel_end.checked_add(12)?;
    if payload.get(ids_end..sentinel_end)? != u32::MAX.to_le_bytes()
        || payload.get(sentinel_end..zeros_end)? != [0; 12]
    {
        return None;
    }
    Some(
        payload
            .get(offset + 16..ids_end)?
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
            .collect(),
    )
}

pub(super) fn compact_general_curve_ref_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 2..offset + 4) == Some(&[0; 2])
        && payload.get(offset + 6..offset + 16) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0])
}

pub(super) fn compact_profile_general_curve_ref_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + 6) == Some(&[1, 0, 0xdd, 0x94, 0xdf, 0x94])
        && payload.get(offset + 6..offset + 16) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0])
}

pub(super) fn declared_general_curve_profile_prefix(
    payload: &[u8],
    offset: usize,
) -> Option<usize> {
    const COMPONENT_PROFILE: &[u8] = b"moCompProfile_c";
    let interval = payload.get(offset..offset.checked_add(96)?.min(payload.len()))?;
    let name = interval
        .windows(COMPONENT_PROFILE.len())
        .position(|bytes| bytes == COMPONENT_PROFILE)?;
    let prefix = offset.checked_add(name + COMPONENT_PROFILE.len())?;
    (payload.get(prefix..prefix + 10) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]))
        .then_some(prefix)
}

pub(super) fn component_profile_source_at(payload: &[u8], prefix: usize) -> Option<u32> {
    const PREFIX: &[u8] = &[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0];
    const HANDLE: &[u8] = &[0xc7, 0xcf, 0xff, 0xff];
    const RECORD_END: &[u8] = &[0xf8, 0x2a, 0, 0];
    if payload.get(prefix..prefix + PREFIX.len()) != Some(PREFIX)
        || payload.get(prefix + 45..prefix + 61) != Some(&[0xff; 16])
    {
        return None;
    }
    let mut sources = [prefix + 69, prefix + 81].into_iter().filter_map(|source| {
        let id = u32::from_le_bytes(payload.get(source..source + 4)?.try_into().ok()?);
        let stamp = u32::from_le_bytes(payload.get(source + 4..source + 8)?.try_into().ok()?);
        if id == 0 || stamp == 0 {
            return None;
        }
        let older = payload.get(source + 12..source + 16) == Some(&[0; 4])
            && payload.get(source + 20..source + 32) == Some(&[0; 12])
            && payload.get(source + 32..source + 36) == Some(HANDLE)
            && payload.get(source + 36..source + 40) == Some(HANDLE)
            && payload.get(source + 40..source + 44) == Some(&[0; 4])
            && payload.get(source + 44..source + 48) == Some(RECORD_END);
        let newer = payload.get(source + 8..source + 16) == Some(&[0; 8])
            && payload.get(source + 16..source + 20) == Some(&0x65u32.to_le_bytes())
            && payload.get(source + 20..source + 24) == Some(&[0; 4])
            && payload.get(source + 24..source + 28) == Some(&[0xff; 4])
            && payload.get(source + 28..source + 32) == Some(&[0; 4])
            && payload.get(source + 32..source + 36) == Some(HANDLE)
            && payload.get(source + 36..source + 40) == Some(HANDLE)
            && payload.get(source + 40..source + 44) == Some(HANDLE)
            && payload.get(source + 44..source + 48) == Some(&[0; 4])
            && payload.get(source + 48..source + 52) == Some(RECORD_END);
        (older || newer).then_some(id)
    });
    let source = sources.next()?;
    sources.next().is_none().then_some(source)
}

pub(super) fn component_reference_curve_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker - 8..marker - 4)? != [0x04, 0x02, 0, 0]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(marker - 12..marker - 8)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    let parse = |count: usize| {
        let mut cursor = marker + 18;
        let signature: [u8; 12] = payload.get(cursor + 4..cursor + 16)?.try_into().ok()?;
        let mut components = Vec::with_capacity(count);
        for index in 0..count {
            if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice()) {
                return None;
            }
            components.push(FeatureInputComponentPathEntry {
                instance: Some(u16::from_le_bytes(
                    payload.get(cursor..cursor + 2)?.try_into().ok()?,
                )),
                type_signature: signature,
                local_id: Some(u32::from_le_bytes(
                    payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
                )),
            });
            cursor += 20;
            if index + 1 != count {
                let gaps = [0usize, 6]
                    .into_iter()
                    .filter(|gap| {
                        payload.get(cursor + gap + 4..cursor + gap + 16)
                            == Some(signature.as_slice())
                            && match *gap {
                                0 => true,
                                6 => {
                                    payload.get(cursor..cursor + 2) != Some(&[0, 0])
                                        && payload.get(cursor + 2..cursor + 6) == Some(&[0; 4])
                                }
                                _ => false,
                            }
                    })
                    .collect::<Vec<_>>();
                let [gap] = gaps.as_slice() else {
                    return None;
                };
                cursor += gap;
            }
        }
        Some((components, cursor))
    };
    parse(count).map(|(components, _)| components).or_else(|| {
        let (components, end) = (count > 1).then(|| parse(count - 1)).flatten()?;
        (payload.get(end..end + 12) == Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0xf8, 0x2a, 0, 0]))
            .then_some(components)
    })
}

pub(super) fn unique_marker_candidate(candidates: &[(String, bool)]) -> Option<&str> {
    let mut coordinate = candidates
        .iter()
        .filter(|(_, coordinate)| *coordinate)
        .map(|(id, _)| id.as_str());
    if let Some(first) = coordinate.next() {
        return coordinate.next().is_none().then_some(first);
    }
    let [(id, _)] = candidates else {
        return None;
    };
    Some(id)
}

pub(super) fn operand_accepts_marker(
    kind: FeatureInputOperandKind,
    marker: SketchInputKind,
) -> bool {
    match kind {
        FeatureInputOperandKind::D6
        | FeatureInputOperandKind::Native(
            0x80cc | 0x8152 | 0x8ab6 | 0x8dcb | 0x929d | 0xbc7c | 0xbd69,
        ) => {
            matches!(
                marker,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }
        FeatureInputOperandKind::Native(0x837b) => matches!(
            marker,
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        ),
        FeatureInputOperandKind::E1
        | FeatureInputOperandKind::Native(0x8386 | 0x83fe | 0x8dda | 0xbc87) => {
            matches!(marker, SketchInputKind::LineOrCircle | SketchInputKind::Arc)
        }
        FeatureInputOperandKind::Native(_) => true,
    }
}

pub(super) fn operand_uses_compatible_ordinal(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::D6
            | FeatureInputOperandKind::E1
            | FeatureInputOperandKind::Native(0x80cc | 0x83fe | 0x8ab6 | 0x929d | 0xbd69)
    )
}

pub(super) fn operand_allows_compatible_ordinal_fallback(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::Native(0x837b | 0x8386 | 0x8dcb | 0x8dda | 0xbc7c | 0xbc87)
    )
}

pub(super) fn marker_local_links(payload: &[u8], offset: usize) -> Option<([u16; 2], u16)> {
    if wide_indexed_curve_endpoint_indices(payload, offset).is_some() {
        return None;
    }
    if payload.get(offset + 70..offset + 72)? != [0, 0]
        || payload.get(offset + 72..offset + 80)? != (-1.0f64).to_le_bytes()
    {
        return None;
    }
    Some((
        [
            u16::from_le_bytes(payload.get(offset + 64..offset + 66)?.try_into().ok()?),
            u16::from_le_bytes(payload.get(offset + 66..offset + 68)?.try_into().ok()?),
        ],
        u16::from_le_bytes(payload.get(offset + 68..offset + 70)?.try_into().ok()?),
    ))
}

pub(super) fn coordinate_marker_local_links(
    payload: &[u8],
    offset: usize,
) -> Option<(Vec<u16>, u16)> {
    let legacy_geometry_linked_point = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(1)
        && marker_is_geometry_locus(payload, offset);
    if legacy_geometry_linked_point {
        let (_, links) = linked_profile_point(payload, offset)?;
        let selector = links.first()?.0;
        return Some((
            links.into_iter().map(|(_, local_id)| local_id).collect(),
            selector,
        ));
    }
    if marker_coordinates(payload, offset).is_none()
        && !counted_legacy_profile_line_layout(payload, offset)
    {
        return None;
    }
    let mut links = Vec::with_capacity(2);
    let mut selector = None;
    for index in 0..=2 {
        let start = offset.checked_add(86 + index * 12)?;
        if payload.get(start..start + 6)? == [0, 0, 0xfe, 0xff, 0xff, 0xff] {
            return (!links.is_empty()).then_some((links, selector?));
        }
        if index == 2 {
            return None;
        }
        let cell = payload.get(start..start + 12)?;
        let tag = u16::from_le_bytes([cell[0], cell[1]]);
        let kind = operand_kind([cell[0], cell[1]])?;
        if !operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
            || !operand_accepts_marker(kind, SketchInputKind::Arc)
            || selector.is_some_and(|selector| selector != tag)
            || cell[4..8] != [0xff; 4]
            || cell[8..12] != [0; 4]
        {
            return None;
        }
        selector = Some(tag);
        links.push(u16::from_le_bytes([cell[2], cell[3]]));
    }
    None
}

fn counted_legacy_profile_line_layout(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(0)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 84..offset + 86) == Some(&2u16.to_le_bytes())
}

#[cfg(test)]
pub(super) fn selection_vector_tail(payload: &mut Vec<u8>, entries: &[u32]) -> usize {
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    payload.extend_from_slice(&[0, 2, 0, 0]);
    payload.extend_from_slice(&[0, 0, 0, 0]);
    let marker = payload.len();
    payload.extend_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload.extend_from_slice(&[0, 0]);
    for local_id in entries {
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }
    marker
}

#[cfg(test)]
mod selections_tests;
