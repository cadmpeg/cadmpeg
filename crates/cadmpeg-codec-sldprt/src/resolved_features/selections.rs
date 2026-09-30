//! Compact body, edge and surface selection decoding.

use super::component_paths::{
    component_path_input_features, component_path_terminal_feature, feature_precedes_consumer,
    surface_selection_producer_features,
};
use super::endpoints::{
    legacy_wide_profile_roster_curve, marker_profile_curve_role,
    wide_indexed_curve_endpoint_indices,
};
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
use super::{is_class_token, CLASS_MARKER, LEGACY_SKETCH_MARKER};
use crate::brep::feature_source::FeatureSourceId;
use crate::classification::{
    classify_type_token, native_object_class, FeatureClass, NativeClassKind,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::charged_clone::CloneCharged;
use crate::records::{
    FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputEdgeSelection,
    FeatureInputLane, FeatureInputOperandKind, FeatureInputSurfaceSelection, SketchInputKind,
};
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::{
    collections::HashSet,
    ops::Range,
};

use crate::layout::{
    component_face_compact_reference_prefix as compact_face,
    component_face_flagged_operation_prefix as flagged_face,
    component_face_nested_reference_prefix as nested_face,
    cosmetic_thread_component_edge_wrapper_prefix as component_edge,
    cosmetic_thread_repeated_edge_ref_prefix as repeated_edge_ref,
};
use crate::records::FeatureSource;
use crate::records::ObjectId;
use cadmpeg_core::decode::u64_from_index;

fn selection_objects<'history, 'lane>(
    ctx: &DecodeContext<'_>,
    histories: &'history [crate::records::FeatureHistory],
    lane: &'lane FeatureInputLane,
    operation: &'static str,
) -> Result<Vec<(&'lane crate::records::FeatureInputName, &'history crate::records::Feature, usize)>, CodecError> {
    let mut objects = Vec::new();
    for (input_index, feature) in histories.iter().flat_map(|history| &history.features).enumerate() {
        ctx.charge_work(1, operation)?;
        for name in &lane.names {
            let work = u64_from_index(name.value.len()).checked_add(u64_from_index(feature.name.len()))
                .and_then(|work| work.checked_add(2))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, operation)?;
        }
        let Some(name) = feature_object_name(feature, lane) else { continue; };
        ctx.reserve_collection_vec(&mut objects, 1, operation)?;
        objects.push((name, feature, input_index));
    }
    let levels = if objects.len() > 1 { objects.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(u64_from_index(objects.len()).checked_mul(u64::from(levels))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?, operation)?;
    objects.sort_unstable_by_key(|(name, _, input_index)| (name.offset, *input_index));
    Ok(objects)
}

fn copy_selection_text(
    ctx: &DecodeContext<'_>,
    text: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.charge_work(u64_from_index(text.len()), operation)?;
    let mut copy = String::new();
    ctx.reserve_retained_string(&mut copy, text.len(), operation)?;
    copy.push_str(text);
    Ok(copy)
}

pub(super) fn compact_body_selections(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputBodySelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact body selections";
    let objects = selection_objects(ctx, histories, lane, OPERATION)?;
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let state_token = compact_body_state_token(lane);
    let mut result = Vec::new();
    for (object_index, &(name, feature, _)) in objects.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let next = objects.get(object_index + 1);
        let end = next
            .and_then(|(next, _, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let next_token = next.and_then(|(next, next_feature, _)| {
            (native_object_class(next_feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::DeleteBody)
                .then(|| {
                    usize::try_from(next.offset)
                        .ok()
                        .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                })
                .flatten()
        });
        let selection = if kind == NativeClassKind::DeleteBody {
            match lane.native_payload.get(start..end) {
                Some(payload) => compact_body_selection_vector(ctx, payload, start, next_token)?,
                None => None,
            }
        } else if kind == NativeClassKind::Operation(FeatureClass::MoveBody) {
            ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
            let mut data_classes = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moMoveCopyBodyData_c"
                        && (u64_from_index(start)..u64_from_index(end)).contains(&class.offset)
                });
            match (data_classes.next(), data_classes.next()) {
                (Some(class), None) => super::direct_edits::move_body_translation_record(
                    ctx, &lane.native_payload,
                    start,
                    end,
                    class.offset,
                )?
                .map(|record| (record.selection_offset, record.local_body_ids)),
                _ => None,
            }
        } else {
            None
        };
        let Some((offset, local_body_ids)) = selection else {
            continue;
        };
        let ordinal = u32::try_from(result.len())
            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len())))?;
        ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
        let id = ctx.format_retained(format_args!("sldprt:feature-input:body-selection#{lane_key}:{offset}"), OPERATION)?;
        let parent = copy_selection_text(ctx, &lane.id, OPERATION)?;
        let object_name_ref = copy_selection_text(ctx, &name.id, OPERATION)?;
        let feature_ref = copy_selection_text(ctx, &feature.id, OPERATION)?;
        let (body_state_ids, mode) = match (kind, state_token) {
            (NativeClassKind::DeleteBody, Some(token)) => (
                compact_body_state_ids(ctx, &lane.native_payload, start, offset, token)?,
                compact_body_retention_mode(ctx, &lane.native_payload, start, offset, token)?,
            ),
            _ => (Vec::new(), None),
        };
        ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
        result.push(FeatureInputBodySelection {
            id,
            parent,
            ordinal,
            offset: u64_from_index(offset),
            object_name_ref,
            feature_ref,
            local_body_ids,
            body_state_ids,
            mode,
        });
    }
    Ok(result)
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
    View::u16_le_at(&lane.native_payload, offset + 8 + class.name.len())
}

pub(crate) fn compact_body_state_ids_for_selection(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Result<Vec<u32>, CodecError> {
    const OPERATION: &str = "decode SLDPRT body state identities";
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let Some(token) = compact_body_state_token(lane) else { return Ok(Vec::new()); };
    let mut start = None;
    for name in &lane.names {
        let work = name.id.len().checked_add(selection.object_name_ref.len()).and_then(|size| size.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(u64_from_index(work), OPERATION)?;
        if name.id == selection.object_name_ref {
            start = usize::try_from(name.offset).ok();
            break;
        }
    }
    let (Some(start), Ok(end)) = (start, usize::try_from(selection.offset)) else { return Ok(Vec::new()); };
    compact_body_state_ids(ctx, &lane.native_payload, start, end, token)
}

pub(crate) fn compact_body_retention_mode_for_selection(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Result<Option<cadmpeg_ir::features::BodyRetentionMode>, CodecError> {
    const OPERATION: &str = "decode SLDPRT body retention mode";
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let Some(token) = compact_body_state_token(lane) else { return Ok(None); };
    let mut start = None;
    for name in &lane.names {
        let work = name.id.len().checked_add(selection.object_name_ref.len()).and_then(|size| size.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(u64_from_index(work), OPERATION)?;
        if name.id == selection.object_name_ref {
            start = usize::try_from(name.offset).ok();
            break;
        }
    }
    let (Some(start), Ok(end)) = (start, usize::try_from(selection.offset)) else { return Ok(None); };
    compact_body_retention_mode(ctx, &lane.native_payload, start, end, token)
}

fn compact_body_retention_mode(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Result<Option<cadmpeg_ir::features::BodyRetentionMode>, CodecError> {
    const HEADER_LEN: usize = 83;
    const OPERATION: &str = "decode SLDPRT body retention mode";
    let Some(scan_end) = end.checked_sub(HEADER_LEN - 1) else { return Ok(None); };
    ctx.charge_work(u64_from_index(scan_end.checked_sub(start).unwrap_or(0)).checked_mul(u64_from_index(HEADER_LEN))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    Ok((|| {
    let token = token.to_le_bytes();
    let state_end = (start..scan_end)
        .filter(|offset| compact_body_state_id(payload, *offset, token).is_some())
        .map(|offset| offset + HEADER_LEN)
        .max()?;
    let field = payload.get(state_end..state_end + 10)?;
    if field[0..2] != [0x30, 0x80] || field[6..10] != [0; 4] {
        return None;
    }
    match View::u32_le_at(field, 2)? {
        0 => Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected),
        1 => Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected),
        _ => None,
    }
    })())
}

fn compact_body_state_id(payload: &[u8], offset: usize, token: [u8; 2]) -> Option<u32> {
    const HEADER_LEN: usize = 83;
    let header = payload.get(offset..offset + HEADER_LEN)?;
    let body_id = View::u32_le_at(header, 11)?;
    (header[0..2] == token
        && header[2..11] == [0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]
        && header[11..15] == header[15..19]
        && header[19..47].iter().all(|byte| *byte == 0)
        && header[47..63].iter().all(|byte| *byte == 0xff)
        && header[63..83].iter().all(|byte| *byte == 0))
    .then_some(body_id)
}

fn compact_body_state_ids(
    ctx: &DecodeContext<'_>, payload: &[u8], start: usize, end: usize, token: u16,
) -> Result<Vec<u32>, CodecError> {
    const HEADER_LEN: usize = 83;
    const OPERATION: &str = "decode SLDPRT body state identities";
    let token = token.to_le_bytes();
    let mut result = Vec::new();
    let Some(scan_end) = end.checked_sub(HEADER_LEN - 1) else { return Ok(result); };
    ctx.charge_work(u64_from_index(scan_end.checked_sub(start).unwrap_or(0)).checked_mul(u64_from_index(HEADER_LEN))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    for offset in start..scan_end {
        let Some(body_id) = compact_body_state_id(payload, offset, token) else { continue; };
        ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
        result.push(body_id);
    }
    Ok(result)
}

/// Decode an edge-selection reference list, including the count-framed
/// unpadded roster form used by variable-radius fillets.
pub(crate) fn compact_edge_reference_list_for_feature(
    payload: &[u8],
    offset: usize,
    feature_kind: &str,
) -> Option<Vec<Vec<FeatureInputComponentPathEntry>>> {
    compact_component_reference_list_at(payload, offset).or_else(|| {
        if !feature_kind.eq_ignore_ascii_case("VarFillet") {
            return None;
        }
        let count_start = offset.checked_sub(12)?;
        let count = usize::try_from(View::u32_le_at(payload, count_start)?).ok()?;
        compact_component_reference_list(payload, offset, false)
            .filter(|references| references.len() == count)
    })
}

pub(super) fn compact_edge_selections(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputEdgeSelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge selections";
    let history_features = history_features_with_object_sources(ctx, histories, lane)?;
    let objects = selection_objects(ctx, histories, lane, OPERATION)?;
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
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
    let compact_edge_token =
        class_name_end.and_then(|offset| View::u16_le_at(&lane.native_payload, offset));
    for (object_index, &(name, feature, _)) in objects.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
        if !matches!(kind, NativeClassKind::Fillet | NativeClassKind::Chamfer) {
            continue;
        }
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let object_end = objects
            .get(object_index + 1)
            .and_then(|(next, _, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let end = if kind == NativeClassKind::Fillet {
            fillet_edge_roster_end(ctx, lane, start, object_end)?.unwrap_or(object_end)
        } else {
            object_end
        };
        let direct_child = compact_edge_class
            .and_then(|class| usize::try_from(class.offset).ok())
            .filter(|offset| (start..end).contains(offset));
        let mut selections = Vec::new();
        if let Some(child_start) = direct_child {
            let selection = match lane.native_payload.get(child_start..end) {
                Some(payload) => compact_edge_selection_vector(ctx, payload, child_start)?,
                None => None,
            };
            if let Some(selection) = selection
            {
                ctx.reserve_collection_vec(&mut selections, 1, OPERATION)?;
                selections.push(selection);
            }
        }
        if let Some(token) = compact_edge_token {
            let repeated = repeated_edge_selections(
                ctx,
                &lane.native_payload,
                start,
                end,
                token,
            )?;
            ctx.reserve_collection_vec(&mut selections, repeated.len(), OPERATION)?;
            selections.extend(repeated);
        }
        let interval = edge_selection_vectors_in_interval(
                ctx,
            &lane.native_payload,
            start,
            end,
        )?;
        ctx.reserve_collection_vec(&mut selections, interval.len(), OPERATION)?;
        selections.extend(interval);
        let levels = if selections.len() > 1 { selections.len().ilog2() + 1 } else { 1 };
        ctx.charge_work(u64_from_index(selections.len()).checked_mul(u64::from(levels) + 1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        selections.sort_unstable_by_key(|selection| selection.0);
        selections.dedup_by_key(|selection| selection.0);
        let mut feature_selections = Vec::new();
        for (offset, local_edge_ids) in selections {
                ctx.charge_work(1, OPERATION)?;
                let references = compact_edge_reference_list_for_feature(
                    &lane.native_payload,
                    offset,
                    &feature.kind,
                ).unwrap_or_default();
                // Keep the established component projection separate from the
                // reference-list projection.  A vertex-bearing multi-hop
                // reference is excluded as a whole from `components`; its
                // lineage must not leak into the edge path merely because the
                // roster fallback retained the reference itself.
                let components = compact_edge_component_path_at(&lane.native_payload, offset)
                    .unwrap_or_default();
                let terminal_feature_ref = compact_edge_owner_feature_at(
                    ctx,
                    &lane.native_payload,
                    offset,
                    &components,
                    &history_features,
                    &feature.id,
                )?;
                let producer_feature_refs = compact_edge_producer_features_at(
                    ctx,
                    &lane.native_payload,
                    offset,
                    &components,
                    &history_features,
                    &feature.id,
                )?;
                ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
                let id = ctx.format_retained(format_args!("sldprt:feature-input:edge-selection#{lane_key}:{offset}"), OPERATION)?;
                let parent = copy_selection_text(ctx, &lane.id, OPERATION)?;
                let object_name_ref = copy_selection_text(ctx, &name.id, OPERATION)?;
                let feature_ref = copy_selection_text(ctx, &feature.id, OPERATION)?;
                ctx.reserve_collection_vec(&mut feature_selections, 1, OPERATION)?;
                feature_selections.push(FeatureInputEdgeSelection {
                    id,
                    parent,
                    ordinal: 0,
                    offset: u64_from_index(offset),
                    object_name_ref,
                    feature_ref,
                    local_edge_ids,
                    components,
                    references,
                    producer_feature_refs,
                    terminal_feature_ref,
                });
        }
        ctx.charge_work(u64_from_index(feature_selections.len()).checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        for mut selection in input_owned_edge_selections(feature_selections) {
            selection.ordinal = u32::try_from(result.len())
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len())))?;
            ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
            result.push(selection);
        }
    }
    Ok(result)
}

fn fillet_edge_roster_end(
    ctx: &DecodeContext<'_>, lane: &FeatureInputLane, start: usize, end: usize,
) -> Result<Option<usize>, CodecError> {
    let mut first = None;
    for class_name in ["moEdgeDim_c", "moVertDim_c"] {
        if let Some(offset) = first_class_object_in_interval(ctx, lane, start, end, class_name)? {
            first = Some(first.map_or(offset, |first: usize| first.min(offset)));
        }
    }
    Ok(first)
}

fn first_class_object_in_interval(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
    class_name: &str,
) -> Result<Option<usize>, CodecError> {
    const OPERATION: &str = "decode SLDPRT selection class intervals";
    let mut direct = None;
    let mut token = None;
    let mut ambiguous = false;
    for class in &lane.classes {
        let work = u64_from_index(class.name.len()).checked_add(u64_from_index(class_name.len()))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        if class.name != class_name { continue; }
        let Some(offset) = usize::try_from(class.offset).ok() else { continue; };
        if (start..end).contains(&offset) {
            let record = offset.checked_sub(4)
                .filter(|record| lane.native_payload.get(*record..*record + 2) == Some(&[0x20, 0x81]))
                .unwrap_or(offset);
            direct = Some(direct.map_or(record, |direct: usize| direct.min(record)));
        }
        let candidate = offset.checked_add(6 + class.name.len())
            .and_then(|body| View::u16_le_at(&lane.native_payload, body)).filter(|token| is_class_token(*token));
        if let Some(candidate) = candidate {
            if token.is_some_and(|token| token != candidate) { ambiguous = true; }
            else { token = Some(candidate); }
        }
    }
    let mut repeated = None;
    if let (Some(token), false, Some(scan_end)) = (token, ambiguous, end.checked_sub(7)) {
        let token = token.to_le_bytes();
        for offset in start..scan_end {
            ctx.charge_work(8, OPERATION)?;
            if lane.native_payload.get(offset..offset + 2) == Some(&[0x20, 0x81])
                && lane.native_payload.get(offset + 2..offset + 4) == Some(&[0x10, 0x00])
                && View::u16_le_at(&lane.native_payload, offset + 4).is_some_and(is_class_token)
                && lane.native_payload.get(offset + 6..offset + 8) == Some(token.as_slice()) {
                repeated = Some(offset);
                break;
            }
        }
    }
    Ok(direct.into_iter().chain(repeated).min())
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
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputSurfaceSelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact surface selections";
    let history_features = history_features_with_object_sources(ctx, histories, lane)?;
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
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let mut cylinder_reference_tokens = HashSet::new();
    for class in &lane.classes {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if class.name != "moCylinderRef_w" { continue; }
        let Some(body) = usize::try_from(class.offset).ok()
            .and_then(|offset| offset.checked_add(6 + class.name.len())) else { continue; };
        let Some(token) = View::u16_le_at(&lane.native_payload, body).filter(|token| is_class_token(*token)) else { continue; };
        if !cylinder_reference_tokens.contains(&token) {
            ctx.charge_collection_items(1, OPERATION)?;
            cylinder_reference_tokens.try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, 1))?;
            cylinder_reference_tokens.insert(token);
        }
    }
    let mirror_surface_prefix = mirror_surface_type_prefix(lane);
    let objects = selection_objects(ctx, histories, lane, OPERATION)?;
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    for (index, &(name, feature, _)) in objects.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let classified = native_object_class(feature.input_class.as_deref().unwrap_or_default());
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
            let mut next_object = None;
            for next in &objects[index + 1..] {
                let work = u64_from_index(next.1.id.len()).checked_add(u64_from_index(feature.id.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
                if next.1.id != feature.id { next_object = Some(next); break; }
            }
            next_object
        } else {
            objects.get(index + 1)
        };
        let end = next_object
            .and_then(|(next, _, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let candidates = match kind {
            NativeClassKind::Thicken => {
                let mut candidates = Vec::new();
                if let (Some(token), Some(scan_end)) = (surface_token, end.checked_sub(105)) {
                    for offset in start..scan_end {
                        ctx.charge_work(1, OPERATION)?;
                        if lane.native_payload.get(offset..offset + 2) != Some(token) { continue; }
                        let marker = offset + 103;
                        if let Some(ids) = compact_surface_selection_at(ctx, &lane.native_payload, marker)? {
                            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push((marker, ids));
                        }
                    }
                }
                candidates
            }
            NativeClassKind::Extrusion => {
                let mut candidates = Vec::new();
                if let Some(scan_end) = end.checked_sub(103) {
                    for offset in start..scan_end {
                        ctx.charge_work(1, OPERATION)?;
                        let marker = compact_extrusion_to_face_at(&lane.native_payload, offset, end)
                            .or_else(|| compact_extrusion_to_vertex_at(&lane.native_payload, offset, end).map(|(marker, _)| marker))
                            .or_else(|| compact_extrusion_offset_from_face_at(&lane.native_payload, offset, end));
                        if let Some((marker, ids)) = marker.and_then(|marker| {
                            compact_termination_reference_path_at(&lane.native_payload, marker).map(|ids| (marker, ids))
                        }) {
                            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push((marker, ids));
                        }
                    }
                }
                candidates
            }
            NativeClassKind::CosmeticThread => {
                let cylinder_references = cosmetic_thread_cylinder_references(
                    ctx,
                    feature,
                    lane,
                    start,
                    end,
                    &cylinder_reference_tokens,
                )?;
                let mut component_face_references = Vec::new();
                for class in &lane.classes {
                    ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                    let Some(offset) = usize::try_from(class.offset).ok() else { continue; };
                    if class.name != "moCompFace_c" || !(start..end).contains(&offset) { continue; }
                    let reference = match offset.checked_add(6 + class.name.len()) {
                        Some(body) => component_face_reference_at(ctx, &lane.native_payload, body)?,
                        None => None,
                    };
                    if let Some(reference) = reference {
                        ctx.reserve_collection_vec(&mut component_face_references, 1, OPERATION)?;
                        component_face_references.push(reference);
                    }
                }
                // The component edge is a fallback carrier. Some objects serialize
                // both forms for one support; admitting both would fail the
                // single-selection invariant even though a canonical carrier is
                // already authoritative.
                let component_references = if cylinder_references.is_empty() && component_face_references.is_empty() {
                    cosmetic_thread_component_references(ctx, lane, start, end)?
                } else { Vec::new() };
                let mut candidates = Vec::new();
                for candidate in cylinder_references.into_iter().chain(component_references).chain(component_face_references) {
                    ctx.charge_work(1, OPERATION)?;
                    ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push(candidate);
                }
                candidates
            }
            NativeClassKind::Fillet if feature.input_class.as_deref() == Some("Fillet_c") => {
                fillet_face_selection_candidates(ctx, lane, start, end)?
            }
            NativeClassKind::Fillet => continue,
            NativeClassKind::MirrorPattern => {
                const OPERATION: &str = "decode SLDPRT mirror surface selections";
                let mut candidates = Vec::new();
                if let (Some(scan_start), Some(scan_end)) = (
                    start.checked_add(12), end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
                ) {
                    for marker in scan_start..scan_end {
                        ctx.charge_work(16, OPERATION)?;
                        if lane.native_payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
                            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) { continue; }
                        if let Some(components) = counted_surface_component_path_at(&lane.native_payload, marker) {
                            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push((marker, components));
                        }
                    }
                }
                if let Some(prefix) = mirror_surface_prefix {
                    let inline = inline_mirror_surface_paths(ctx, &lane.native_payload, start, end, prefix)?;
                    ctx.reserve_collection_vec(&mut candidates, inline.len(), OPERATION)?;
                    candidates.extend(inline);
                }
                candidates
            }
            NativeClassKind::ReferencePlane => {
                face_reference_plane_selection_candidates(ctx, lane, start, end)?
            }
            NativeClassKind::PlanarSurface => {
                planar_surface_selection_candidates(ctx, &lane.native_payload, start, end)?
            }
            NativeClassKind::Operation(operation) => operation_surface_selection_candidates(
                ctx, operation,
                lane,
                start,
                end,
                name.object_id.and_then(ObjectId::value),
            )?,
            _ => continue,
        };
        let expected_count = match kind {
            NativeClassKind::Operation(FeatureClass::CutWithSurface)
            | NativeClassKind::PlanarSurface => 2,
            _ => 1,
        };
        if !matches!(
            kind,
            NativeClassKind::Fillet
                | NativeClassKind::MirrorPattern
                | NativeClassKind::Operation(FeatureClass::SplitFace)
        ) && candidates.len() != expected_count
        {
            continue;
        }
        for (offset, components) in candidates {
            let endpoint_selector = if kind == NativeClassKind::Extrusion {
                compact_extrusion_endpoint_selector_for_marker(
                    &lane.native_payload,
                    start,
                    end,
                    offset,
                )
            } else {
                None
            };
            let terminal_feature_ref = surface_selection_terminal_feature_at(
                ctx,
                &lane.native_payload,
                offset,
                &components,
                &history_features,
            )?;
            let producer_feature_refs = surface_selection_producer_features(
                    ctx,
                &components,
                terminal_feature_ref.as_deref(),
                &history_features,
            )?;
            ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
            let id = ctx.format_retained(format_args!("sldprt:feature-input:surface-selection#{lane_key}:{offset}"), OPERATION)?;
            let parent = copy_selection_text(ctx, &lane.id, OPERATION)?;
            let object_name_ref = copy_selection_text(ctx, &name.id, OPERATION)?;
            let feature_ref = copy_selection_text(ctx, &feature.id, OPERATION)?;
            let ordinal = u32::try_from(result.len())
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len())))?;
            ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
            result.push(FeatureInputSurfaceSelection {
                id,
                parent,
                ordinal,
                offset: u64_from_index(offset),
                selector: lane.native_payload[offset.checked_sub(8).unwrap_or(0)],
                kind: match endpoint_selector {
                    Some(endpoint_selector) => {
                        crate::records::FeatureInputSurfaceSelectionKind::ExtrusionEndpoint {
                            endpoint_selector,
                        }
                    }
                    None => crate::records::FeatureInputSurfaceSelectionKind::Component,
                },
                object_name_ref,
                feature_ref,
                producer_feature_refs,
                terminal_feature_ref,
                components,
            });
        }
    }
    Ok(result)
}

/// Return the opaque endpoint selector belonging to one extrusion selection
/// marker. The end-spec body can start after the feature-name offset, so the
/// lookup must scan the complete feature interval rather than probe `start`.
fn compact_extrusion_endpoint_selector_for_marker(
    payload: &[u8],
    start: usize,
    end: usize,
    marker: usize,
) -> Option<u32> {
    (start..end).find_map(|body| {
        let (candidate, kind) = compact_extrusion_to_vertex_at(payload, body, end)?;
        (candidate == marker)
            .then(|| kind.endpoint_selector())
            .flatten()
    })
}

fn fillet_face_selection_candidates(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT full round fillet surface candidates";
    // A full-round Fillet_c carries center, first-side, and second-side face
    // carriers in that order. Other role-03 counts are different fillet
    // constructions and remain outside this projection.
    let mut class_bodies = Vec::new();
    for class in &lane.classes {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if class.name != "moCompFace_c" { continue; }
        let Some(class_offset) = usize::try_from(class.offset).ok() else { continue; };
        if !(start..end).contains(&class_offset) { continue; }
        let Some(body) = class_offset.checked_add(6 + class.name.len()) else { continue; };
        let Some(token) = View::u16_le_at(&lane.native_payload, body).filter(|token| is_class_token(*token)) else { continue; };
        ctx.reserve_collection_vec(&mut class_bodies, 1, OPERATION)?;
        class_bodies.push((body, token));
    }
    let levels = if class_bodies.len() > 1 { class_bodies.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(u64_from_index(class_bodies.len()).checked_mul(u64::from(levels) + 1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    class_bodies.sort_unstable();
    class_bodies.dedup();
    let mut candidates = Vec::new();
    if let Some(scan_end) = end.checked_sub(6) {
        for (body, token) in class_bodies {
            let token = token.to_le_bytes();
            for offset in body..scan_end {
                ctx.charge_work(6, OPERATION)?;
                let header = lane.native_payload.get(offset..offset + 6);
                if offset != body && (header.and_then(|header| header.get(..2)) != Some(token.as_slice())
                    || header.and_then(|header| header.get(2..6)) != Some(&[2, 0, 0, 0])) { continue; }
                let Some((marker, components)) = component_face_reference_at_for_full_round_fillet(ctx, &lane.native_payload, offset)? else { continue; };
                let Some(selector) = marker.checked_sub(8).and_then(|start| lane.native_payload.get(start..marker - 4)) else { continue; };
                if !is_component_vector_selector_for_role(selector, 3) { continue; }
                ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                candidates.push((marker, components));
            }
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 3 { candidates } else { Vec::new() })
}

fn planar_surface_selection_candidates(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT planar surface candidates";
    let mut candidates = Vec::new();
    if let Some(scan_end) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) {
        for marker in start..scan_end {
            ctx.charge_work(4, OPERATION)?;
            let Some(selector) = marker.checked_sub(8).and_then(|start| payload.get(start..marker - 4)) else { continue; };
            if !is_component_vector_selector_for_role(selector, 2) { continue; }
            if let Some(components) = component_vector_path_at(payload, marker) {
                ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                candidates.push((marker, components));
            }
        }
    }
    Ok(candidates)
}

fn face_reference_plane_selection_candidates(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT reference plane surface candidates";
    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let mut data_classes = lane.classes.iter().filter(|class| class.name == "moFaceRefPlnData_c"
        && (u64_from_index(start)..u64_from_index(end)).contains(&class.offset));
    let mut candidates = Vec::new();
    if let (Some(data_class), None) = (data_classes.next(), data_classes.next()) {
        let Some(body) = usize::try_from(data_class.offset).ok()
            .and_then(|offset| offset.checked_add(6 + data_class.name.len())) else { return Ok(Vec::new()); };
        if let Some(scan_end) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) {
            for marker in body..scan_end {
                ctx.charge_work(16, OPERATION)?;
                if lane.native_payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len()) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) { continue; }
                if let Some(components) = counted_surface_component_path_at(&lane.native_payload, marker) {
                    ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push((marker, components));
                }
            }
        }
    }
    for class in &lane.classes {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if class.name != "moCompFace_c" { continue; }
        let Some(offset) = usize::try_from(class.offset).ok().filter(|offset| (start..end).contains(offset)) else { continue; };
        let reference = match offset.checked_add(6 + class.name.len()) {
            Some(body) => component_face_reference_at(ctx, &lane.native_payload, body)?,
            None => None,
        };
        if let Some(candidate) = reference {
            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
            candidates.push(candidate);
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 1 { candidates } else { Vec::new() })
}

fn order_surface_candidates(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<(usize, Vec<FeatureInputComponentPathEntry>)>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let mut indexed = Vec::new();
    ctx.reserve_collection_vec(&mut indexed, candidates.len(), operation)?;
    let levels = if candidates.len() > 1 { candidates.len().ilog2() + 1 } else { 1 };
    let work = u64_from_index(candidates.len()).checked_mul(u64::from(levels) + 2)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    for (index, (offset, components)) in candidates.drain(..).enumerate() {
        indexed.push((offset, components, index));
    }
    indexed.sort_unstable_by_key(|(offset, _, index)| (*offset, *index));
    for pair in indexed.windows(2) {
        let work = u64_from_index(pair[0].1.len()).checked_add(u64_from_index(pair[1].1.len()))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)?;
    }
    indexed.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    ctx.reserve_precharged_vec(candidates, indexed.len(), operation)?;
    for (offset, components, _) in indexed { candidates.push((offset, components)); }
    Ok(())
}

fn operation_surface_selection_candidates(
    ctx: &DecodeContext<'_>,
    operation: FeatureClass,
    lane: &FeatureInputLane,
    start: usize,
    end: usize,
    object_source: Option<u32>,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT operation surface candidates";
    if operation == FeatureClass::CutWithSurface {
        let mut candidates = Vec::new();
        if let Some(scan_end) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) {
            for marker in start..scan_end {
                ctx.charge_work(4, OPERATION)?;
                let Some(selector) = marker.checked_sub(8).and_then(|start| lane.native_payload.get(start..marker - 4)) else { continue; };
                if !is_component_vector_selector_for_role(selector, 2) { continue; }
                // The first role-02 vector is the target-body reference list;
                // the later vector belongs to the moCompSurfaceBody_c cutting
                // surface child. The selector's low byte is lane-local.
                let components = match compact_component_reference_list(&lane.native_payload, marker, false) {
                    Some(references) => {
                        let mut components = Vec::new();
                        for reference in references {
                            for component in reference {
                                ctx.charge_work(1, OPERATION)?;
                                ctx.reserve_collection_vec(&mut components, 1, OPERATION)?;
                                components.push(component);
                            }
                        }
                        Some(components)
                    }
                    None => compact_surface_selection_at(ctx, &lane.native_payload, marker)?,
                };
                if let Some(components) = components {
                    ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push((marker, components));
                }
            }
        }
        return Ok(candidates);
    }
    if operation == FeatureClass::SplitFace {
        ctx.charge_work(u64_from_index(lane.classes.len()).checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if !["moPLineProjIdRep_c", "moPLineSurfIdRep_c"]
            .into_iter()
            .all(|required| lane.classes.iter().any(|class| class.name == required))
        {
            return Ok(Vec::new());
        }
        let Some(object_source) = object_source else {
            return Ok(Vec::new());
        };
        const OPERATION: &str = "project SLDPRT split surface identity paths";
        let mut candidates = Vec::new();
        for identity in generated_surface_identities(ctx, lane)? {
            ctx.charge_work(1, OPERATION)?;
            let (Some(first), Some(last)) = (identity.components.first(), identity.components.last()) else { continue; };
            if component_source(first) != Some(object_source)
                || !component_source(last).is_some_and(|source| source != object_source)
                || last.local_id.is_none() { continue; }
            let Ok(offset) = usize::try_from(identity.offset) else { continue; };
            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
            candidates.push((offset, identity.components));
        }
        return Ok(candidates);
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
        return Ok(Vec::new());
    }

    ctx.charge_work(u64_from_index(lane.classes.len()), OPERATION)?;
    let mut surface_classes = lane.classes.iter().filter(|class| class.name == "moCompSurfaceBody_c"
        && usize::try_from(class.offset).ok().is_some_and(|offset| (start..end).contains(&offset)));
    let mut candidates = match (surface_classes.next(), surface_classes.next()) {
        (Some(class), None) => compact_surface_selection_candidates_for_class(ctx, &lane.native_payload, class, start, end)?,
        _ => Vec::new(),
    };
    let mut component_face_tokens = HashSet::new();
    for class in &lane.classes {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if class.name != "moCompFace_c" { continue; }
        let Some(offset) = usize::try_from(class.offset).ok() else { continue; };
        let Some(body) = offset.checked_add(6 + class.name.len()) else { continue; };
        if (start..end).contains(&offset) {
            if let Some(candidate) = component_face_reference_at_for_operation(ctx, &lane.native_payload, body)? {
                ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                candidates.push(candidate);
            }
        }
        let Some(token) = View::u16_le_at(&lane.native_payload, body).filter(|token| is_class_token(*token)) else { continue; };
        if !component_face_tokens.contains(&token) {
            ctx.charge_collection_items(1, OPERATION)?;
            component_face_tokens.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, 1))?;
            component_face_tokens.insert(token);
        }
    }
    for token in component_face_tokens {
        let repeated = component_face_reference_candidates(ctx, &lane.native_payload, token, start, end)?;
        ctx.reserve_collection_vec(&mut candidates, repeated.len(), OPERATION)?;
        candidates.extend(repeated);
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 1 {
        candidates
    } else {
        Vec::new()
    })
}

fn component_source(component: &FeatureInputComponentPathEntry) -> Option<u32> {
    View::u32_le_at(&component.type_signature, 4)
}

fn compact_surface_selection_candidates_for_class(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class: &crate::records::FeatureInputClass,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT class surface candidates";
    let Some(class_offset) = usize::try_from(class.offset).ok() else {
        return Ok(Vec::new());
    };
    if !(start..end).contains(&class_offset) {
        return Ok(Vec::new());
    }
    let Some(body) = class_offset.checked_add(6 + class.name.len()) else {
        return Ok(Vec::new());
    };
    let Some(bounded_payload) =
        super::DeclaredEnd::of(end, payload.len()).and_then(|end| payload.get(..end.get()))
    else {
        return Ok(Vec::new());
    };
    let Some(last_marker) = bounded_payload
        .len()
        .checked_sub(COMPACT_EDGE_VECTOR_MARKER.len())
    else {
        return Ok(Vec::new());
    };
    if body > last_marker {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for marker in body..=last_marker {
        ctx.charge_work(1, OPERATION)?;
        if let Some(components) = compact_surface_selection_at(ctx, bounded_payload, marker)? {
            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
            candidates.push((marker, components));
        }
    }
    Ok(candidates)
}

fn history_features_with_object_sources(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<crate::records::Feature>, CodecError> {
    const OPERATION: &str = "clone SLDPRT selection history features";
    let mut features = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        ctx.reserve_collection_vec(&mut features, 1, OPERATION)?;
        features.push(feature.clone_charged(ctx, OPERATION)?);
    }
    enrich_feature_object_sources(ctx, &mut features, std::slice::from_ref(lane))?;
    Ok(features)
}

/// Bind flat idless history records to identities from unique feature-input
/// object names without changing records that already carry source identity.
pub(crate) fn enrich_feature_object_sources(
    ctx: &DecodeContext<'_>,
    features: &mut [crate::records::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT selection feature object sources";
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        if feature.source_id.is_some() { continue; }
        let mut source = None;
        let mut ambiguous = false;
        for lane in lanes {
            ctx.charge_work(1, OPERATION)?;
            for name in &lane.names {
                let work = name.value.len().checked_add(feature.name.len())
                    .and_then(|size| size.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(u64_from_index(work), OPERATION)?;
            }
            let Some(candidate) = feature_object_name(feature, lane)
                .and_then(|name| name.object_id).and_then(ObjectId::value) else { continue; };
            match source {
                Some(previous) if previous != candidate => {
                    ambiguous = true;
                    break;
                }
                None => source = Some(candidate),
                _ => {}
            }
        }
        if !ambiguous {
            if let Some(source) = source {
                feature.source_id = FeatureSource::from_value(source);
            }
        }
    }
    Ok(())
}

fn cosmetic_thread_cylinder_references(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
    cylinder_reference_tokens: &HashSet<u16>,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT cosmetic cylinder references";
    let diameter_tail = cosmetic_thread_diameter_child_tail(ctx, feature, lane)?;
    let mut offsets = Vec::new();
    for offset in std::iter::once(object_start..object_end).chain(diameter_tail).flatten() {
        ctx.charge_work(2, OPERATION)?;
        if View::u16_le_at(&lane.native_payload, offset).is_some_and(|token| cylinder_reference_tokens.contains(&token)) {
            ctx.reserve_collection_vec(&mut offsets, 1, OPERATION)?;
            offsets.push(offset);
        }
    }
    let levels = if offsets.len() > 1 { offsets.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(u64_from_index(offsets.len()).checked_mul(u64::from(levels) + 1)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    offsets.sort_unstable();
    offsets.dedup();
    let mut references = Vec::new();
    for offset in offsets {
        ctx.charge_work(1, OPERATION)?;
        if let Some(reference) = cosmetic_thread_cylinder_reference_at(&lane.native_payload, offset) {
            ctx.reserve_collection_vec(&mut references, 1, OPERATION)?;
            references.push(reference);
            break;
        }
    }
    Ok(references)
}

/// Decode component-edge references owned by a cosmetic-thread object.
///
/// Some native lanes carry the selected cylindrical support as a `moCompEdge_c`
/// child instead of wrapping the same component path in `moCylinderRef_w`.
/// The component edge can be a declared class or a repeated class-token
/// instance. It can own the vector directly or through its immediate
/// `moEdgeRef_c` child. Restrict both scans to that wrapper and keep the normal
/// single-candidate check in `compact_surface_selections`; unrelated compact
/// vectors in the thread's other children must not become face selections.
fn cosmetic_thread_component_references(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT cosmetic component references";
    let mut classes = Vec::new();
    for class in &lane.classes {
        ctx.charge_work(1, OPERATION)?;
        let Some(offset) = usize::try_from(class.offset).ok().filter(|offset| (object_start..object_end).contains(offset)) else { continue; };
        ctx.reserve_collection_vec(&mut classes, 1, OPERATION)?;
        classes.push((offset, class));
    }
    let levels = if classes.len() > 1 { classes.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(u64_from_index(classes.len()).checked_mul(u64::from(levels))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    classes.sort_unstable_by_key(|(offset, _)| *offset);

    let mut class_ranges = Vec::<Range<usize>>::new();
    for (index, &(class_offset, class)) in classes.iter().enumerate() {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if class.name != "moCompEdge_c" {
            continue;
        }
        let Some(body) = class_offset.checked_add(6 + class.name.len()) else {
            continue;
        };
        let direct_end = classes
            .get(index + 1)
            .map_or(object_end, |(offset, _)| *offset);
        if body >= direct_end {
            continue;
        }
        ctx.reserve_collection_vec(&mut class_ranges, 1, OPERATION)?;
        class_ranges.push(body..direct_end);

        let Some((edge_ref_offset, edge_ref)) = classes.get(index + 1) else {
            continue;
        };
        if edge_ref.name != "moEdgeRef_c"
            || !cosmetic_thread_component_edge_wrapper_at(&lane.native_payload, body)
        {
            continue;
        }
        let Some(edge_ref_body) = edge_ref_offset.checked_add(6 + edge_ref.name.len()) else {
            continue;
        };
        let edge_ref_end = classes
            .get(index + 2)
            .map_or(object_end, |(offset, _)| *offset);
        if edge_ref_body < edge_ref_end {
            ctx.reserve_collection_vec(&mut class_ranges, 1, OPERATION)?;
            class_ranges.push(edge_ref_body..edge_ref_end);
        }
    }
    let repeated = cosmetic_thread_repeated_component_edge_ranges(ctx, &lane.native_payload, object_start, object_end)?;
    ctx.reserve_collection_vec(&mut class_ranges, repeated.len(), OPERATION)?;
    class_ranges.extend(repeated);
    let mut references = Vec::new();
    for marker in class_ranges.into_iter().flatten() {
        ctx.charge_work(16, OPERATION)?;
        if lane.native_payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len()) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) { continue; }
        if let Some(components) = compact_edge_component_path_at(&lane.native_payload, marker) {
            ctx.reserve_collection_vec(&mut references, 1, OPERATION)?;
            references.push((marker, components));
        }
    }
    order_surface_candidates(ctx, &mut references, OPERATION)?;
    ctx.charge_work(u64_from_index(references.len()), OPERATION)?;
    references.dedup_by_key(|(marker, _)| *marker);
    Ok(references)
}

fn cosmetic_thread_repeated_component_edge_ranges(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
) -> Result<Vec<Range<usize>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT cosmetic component ranges";
    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(Vec::new());
    };
    let Some(last_token) = end.checked_sub(2 + component_edge::LEN) else {
        return Ok(Vec::new());
    };
    if object_start > last_token {
        return Ok(Vec::new());
    }
    let mut ranges = Vec::new();
    for token_offset in object_start..=last_token {
        ctx.charge_work(u64_from_index(component_edge::LEN).checked_add(2)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if !View::u16_le_at(payload, token_offset).is_some_and(is_class_token)
            || !cosmetic_thread_component_edge_wrapper_at(payload, token_offset + 2)
        {
            continue;
        }
        let body = token_offset + 2;
        let child_start = body + component_edge::COMPONENT_COUNT;
        let Some(last_child) = end.checked_sub(2 + repeated_edge_ref::LEN) else {
            continue;
        };
        let mut child_token = None;
        if child_start <= last_child {
            for offset in child_start..=last_child {
                ctx.charge_work(u64_from_index(repeated_edge_ref::LEN).checked_add(2)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
                if View::u16_le_at(payload, offset).is_some_and(is_class_token)
                    && payload.get(offset + 2..offset + 2 + repeated_edge_ref::LEN) == Some(repeated_edge_ref::PREFIX_VALUE.as_slice()) {
                    child_token = Some(offset);
                    break;
                }
            }
        }
        ctx.reserve_collection_vec(&mut ranges, 1, OPERATION)?;
        if let Some(edge_ref_token) = child_token {
            ranges.push(edge_ref_token + 2..end);
        } else {
            ranges.push(body..end);
        }
    }
    Ok(ranges)
}

fn cosmetic_thread_component_edge_wrapper_at(payload: &[u8], body: usize) -> bool {
    let Some(flags_start) = body.checked_add(component_edge::WRAPPER_FLAGS) else {
        return false;
    };
    let Some(flags_end) = body.checked_add(component_edge::COMPONENT_COUNT) else {
        return false;
    };
    let Some(class_token) = View::u16_le_at(payload, body + component_edge::INNER_CLASS_TOKEN)
    else {
        return false;
    };
    let Some(count) = View::u32_le_at(payload, body + component_edge::COMPONENT_COUNT) else {
        return false;
    };
    is_class_token(class_token)
        && payload.get(flags_start..flags_end)
            == Some(component_edge::WRAPPER_FLAGS_VALUE.as_slice())
        && count != 0
        && View::u32_le_at(payload, body + component_edge::COMPONENT_COUNT_COPY) == Some(count)
}

pub(super) fn cosmetic_thread_cylinder_marker_reference(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
    cylinder_reference_tokens: &HashSet<u16>,
) -> Result<Vec<(usize, Option<Vec<FeatureInputComponentPathEntry>>)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT cosmetic thread cylinder markers";
    let diameter_tail = cosmetic_thread_diameter_child_tail(ctx, feature, lane)?;
    let mut markers = Vec::new();
    for body in std::iter::once(object_start..object_end).chain(diameter_tail).flatten() {
        ctx.charge_work(1, OPERATION)?;
        if !View::u16_le_at(&lane.native_payload, body)
            .is_some_and(|token| cylinder_reference_tokens.contains(&token))
        {
            continue;
        }
        let Some(marker) = cosmetic_thread_cylinder_reference_marker_layout_at(&lane.native_payload, body) else {
            continue;
        };
        ctx.reserve_collection_vec(&mut markers, 1, OPERATION)?;
        markers.push(marker);
    }
    let count = u64::try_from(markers.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let levels = if markers.len() > 1 { markers.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(count.checked_mul(u64::from(levels))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    markers.sort_unstable();
    markers.dedup();
    let mut references = Vec::new();
    ctx.reserve_collection_vec(&mut references, markers.len(), OPERATION)?;
    for marker in markers {
        let components = compact_sketch_surface_component_path_at(&lane.native_payload, marker)
            .or_else(|| compact_termination_reference_path_at(&lane.native_payload, marker))
            .or_else(|| compact_edge_component_path_at(&lane.native_payload, marker));
        references.push((marker, components));
    }
    Ok(references)
}

fn cosmetic_thread_diameter_child_tail(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
) -> Result<Option<std::ops::Range<usize>>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT cosmetic diameter interval";
    for scalar in &lane.scalars {
        ctx.charge_work(2, OPERATION)?;
        for name in &lane.names {
            let work = u64_from_index(name.id.len()).checked_add(u64_from_index(scalar.name.len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
        }
    }
    ctx.charge_work(u64_from_index(lane.names.len()), OPERATION)?;
    Ok((|| {
    let source_id = feature.source_value()?;
    let diameter_id = source_id.checked_sub(1)?;
    let mut diameters = lane.scalars.iter().filter(|scalar| {
        scalar.object_id == diameter_id
            && lane.names
                .iter()
                .rev()
                .find(|name| name.id == scalar.name)
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
                .filter(|name| name.object_id != Some(ObjectId::Absent))
                .map(|name| name.offset),
        )
        .filter(|offset| *offset >= u64_from_index(start))
        .min()
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(lane.native_payload.len());
    (start < end).then_some(start..end)
    })())
}

fn cosmetic_thread_cylinder_reference_at(
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
    let nested_token = View::u16_le_at(body, 2)?;
    if !is_class_token(nested_token)
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
            let count = View::u32_le_at(payload, marker.checked_sub(12)?)?;
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

fn component_face_reference_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    component_face_reference_at_impl(ctx, payload, body_offset, false, false)
}

fn component_face_reference_at_for_operation(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    component_face_reference_at_impl(ctx, payload, body_offset, false, true)
}

fn component_face_reference_at_for_full_round_fillet(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    component_face_reference_at_impl(ctx, payload, body_offset, true, false)
}

fn component_face_reference_at_impl(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_offset: usize,
    include_compact_frame: bool,
    allow_flagged_operation_frame: bool,
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT component face reference";
    ctx.charge_work(u64_from_index(nested_face::COMPONENT_MARKER).checked_add(32)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    const NESTED_FACE_CLASS: &[u8] = b"moFaceRef_c";
    let marker_offsets = (|| {
    let token = View::u16_le_at(payload, body_offset)?;
    let flags = payload.get(body_offset + 6..body_offset + 8)?;
    if !is_class_token(token)
        || payload.get(body_offset + 2..body_offset + 6)? != 2u32.to_le_bytes()
        || !matches!(flags, [0 | 0x40, 0])
    {
        return None;
    }
    let nested_face_class = payload
        .get(body_offset..body_offset + nested_face::COMPONENT_MARKER)
        .is_some_and(|body| {
            body.windows(CLASS_MARKER.len() + 2 + NESTED_FACE_CLASS.len())
                .any(|header| {
                    &header[..CLASS_MARKER.len()] == CLASS_MARKER
                        && header[CLASS_MARKER.len()..CLASS_MARKER.len() + 2]
                            == (NESTED_FACE_CLASS.len() as u16).to_le_bytes()
                        && &header[CLASS_MARKER.len() + 2..] == NESTED_FACE_CLASS
                })
        });
    let marker_offsets: &[usize] = if flags == [0x40, 0] && allow_flagged_operation_frame {
        &[100, flagged_face::COMPONENT_MARKER]
    } else if flags == [0x40, 0] {
        &[100]
    } else if nested_face_class {
        &[nested_face::COMPONENT_MARKER]
    } else if include_compact_frame {
        // The short compact face frame and the two established zero-flag
        // frames share this carrier header. The vector grammar selects the
        // complete frame at the chosen offset.
        &[compact_face::COMPONENT_MARKER, 68, 92]
    } else {
        &[68, 92]
    };
    Some(marker_offsets)
    })();
    let Some(marker_offsets) = marker_offsets else { return Ok(None); };
    let mut candidate = None;
    let mut ambiguous = false;
    for relative in marker_offsets {
        let Some(marker) = body_offset.checked_add(*relative) else { continue; };
        let Some(components) = compact_surface_reference_at(ctx, payload, marker)? else { continue; };
        if !include_compact_frame { return Ok(Some((marker, components))); }
        if candidate.is_some() { ambiguous = true; }
        else { candidate = Some((marker, components)); }
    }
    Ok(if ambiguous { None } else { candidate })
}

fn component_face_reference_candidates(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_token: u16,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT repeated component face candidates";
    let Some(bounded_end) = super::DeclaredEnd::of(end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(Vec::new());
    };
    let Some(bounded_payload) = payload.get(..bounded_end) else {
        return Ok(Vec::new());
    };
    let mut candidates = Vec::new();
    if let Some(scan_end) = bounded_end.checked_sub(8) {
        for offset in start..scan_end {
            ctx.charge_work(2, OPERATION)?;
            if View::u16_le_at(bounded_payload, offset) != Some(class_token) { continue; }
            if let Some(candidate) = component_face_reference_at_for_operation(ctx, bounded_payload, offset)? {
                ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                candidates.push(candidate);
            }
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(candidates)
}

pub(super) fn component_face_reference_in_record(
    ctx: &DecodeContext<'_>, payload: &[u8],
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const CLASS: &[u8] = b"moCompFace_c";
    const OPERATION: &str = "decode SLDPRT component face record";
    let header_length = CLASS_MARKER.len() + 2 + CLASS.len();
    let mut candidate: Option<(usize, Vec<FeatureInputComponentPathEntry>)> = None;
    let mut ambiguous = false;
    for (offset, header) in payload.windows(header_length).enumerate() {
        ctx.charge_work(u64_from_index(header_length), OPERATION)?;
        if &header[..CLASS_MARKER.len()] != CLASS_MARKER
            || header[CLASS_MARKER.len()..CLASS_MARKER.len() + 2] != (CLASS.len() as u16).to_le_bytes()
            || &header[CLASS_MARKER.len() + 2..] != CLASS { continue; }
        let Some(reference) = component_face_reference_at(ctx, payload, offset + header_length)? else { continue; };
        if let Some(candidate) = &candidate {
            let work = u64_from_index(candidate.1.len()).checked_add(u64_from_index(reference.1.len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            if candidate != &reference { ambiguous = true; }
        } else { candidate = Some(reference); }
    }
    Ok(if ambiguous { None } else { candidate })
}

fn compact_sketch_surface_component_path_at(
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
    let selector = View::u32_le_at(payload, marker - 4)?;
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

fn compact_surface_selection_at(
    ctx: &DecodeContext<'_>, payload: &[u8], marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact surface path";
    ctx.charge_work(32, OPERATION)?;
    let header = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        let entries_start = marker.checked_add(18)?;
        if payload.get(marker..marker.checked_add(16)?)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(count_start..count_start + 4)? != 6u32.to_le_bytes()
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0, 0]
            || !is_component_vector_selector_for_role(payload.get(kind_start..kind_start + 4)?, 2)
            || payload.get(marker + 16..entries_start)? != [0, 0] { return None; }
        let signature: [u8; 12] = payload.get(entries_start + 4..entries_start + 16)?.try_into().ok()?;
        Some((entries_start, signature))
    })();
    let Some((mut cursor, signature)) = header else { return Ok(None); };
    let mut components = Vec::new();
    loop {
        ctx.charge_work(64, OPERATION)?;
        if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice()) { break; }
        let entry = (|| Some(FeatureInputComponentPathEntry {
            instance: Some(View::u16_le_at(payload, cursor)?),
            type_signature: signature,
            local_id: Some(View::u32_le_at(payload, cursor + 16)?),
        }))();
        let Some(entry) = entry else { return Ok(None); };
        ctx.reserve_collection_vec(&mut components, 1, OPERATION)?;
        components.push(entry);
        let Some(next) = cursor.checked_add(20) else { return Ok(None); };
        cursor = next;
        if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice())
            && payload.get(cursor + 8..cursor + 20) == Some(signature.as_slice()) {
            let Some(next) = cursor.checked_add(4) else { return Ok(None); };
            cursor = next;
        }
    }
    Ok((!components.is_empty()).then_some(components))
}

fn flatten_surface_references(
    ctx: &DecodeContext<'_>, references: Option<Vec<Vec<FeatureInputComponentPathEntry>>>,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "flatten SLDPRT surface references";
    let Some(references) = references else { return Ok(None); };
    let mut components = Vec::new();
    for reference in references {
        ctx.charge_work(1, OPERATION)?;
        for component in reference {
            ctx.charge_work(1, OPERATION)?;
            ctx.reserve_collection_vec(&mut components, 1, OPERATION)?;
            components.push(component);
        }
    }
    Ok(Some(components))
}

fn compact_surface_reference_at(
    ctx: &DecodeContext<'_>, payload: &[u8], marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    if let Some(path) = compact_surface_selection_at(ctx, payload, marker)? { return Ok(Some(path)); }
    if let Some(path) = component_vector_path_at(payload, marker) { return Ok(Some(path)); }
    if let Some(path) = flatten_surface_references(ctx, compact_component_reference_list_at(payload, marker))? { return Ok(Some(path)); }
    if let Some(path) = flatten_surface_references(ctx, compact_component_reference_list(payload, marker, false))? { return Ok(Some(path)); }
    if let Some(path) = counted_surface_component_path_at(payload, marker) { return Ok(Some(path)); }
    if let Some(path) = compact_termination_reference_path_at(payload, marker) { return Ok(Some(path)); }
    if let Some(path) = compact_sketch_surface_component_path_at(payload, marker) { return Ok(Some(path)); }
    Ok(inline_surface_reference_at(payload, marker))
}

pub(crate) fn surface_reference_matches_at(
    ctx: &DecodeContext<'_>, payload: &[u8], marker: usize,
    expected: &[FeatureInputComponentPathEntry],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "compare SLDPRT surface reference candidates";
    let candidates = [
        compact_surface_selection_at(ctx, payload, marker)?,
        component_vector_path_at(payload, marker),
        flatten_surface_references(ctx, compact_component_reference_list_at(payload, marker))?,
        flatten_surface_references(ctx, compact_component_reference_list(payload, marker, false))?,
        counted_surface_component_path_at(payload, marker),
        compact_termination_reference_path_at(payload, marker),
        compact_sketch_surface_component_path_at(payload, marker),
        inline_surface_reference_at(payload, marker),
    ];
    for components in candidates.into_iter().flatten() {
        let work = u64_from_index(components.len()).checked_add(u64_from_index(expected.len()))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        if components == expected { return Ok(true); }
    }
    Ok(false)
}

fn repeated_edge_selections(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Result<Vec<(usize, Vec<u32>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT repeated edge selections";
    let token = token.to_le_bytes();
    let mut selections = Vec::new();
    if let Some(scan_end) = end.checked_sub(110) {
        for offset in start..scan_end {
            ctx.charge_work(3, OPERATION)?;
            if payload.get(offset..offset + 2) != Some(token.as_slice()) || payload.get(offset + 2) != Some(&2) { continue; }
            let marker = offset + 108;
            if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
                ctx.reserve_collection_vec(&mut selections, 1, OPERATION)?;
                selections.push((marker, ids));
            }
        }
    }
    Ok(selections)
}

fn edge_selection_vectors_in_interval(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<u32>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT interval edge selections";
    let mut selections = Vec::new();
    if let (Some(scan_start), Some(scan_end)) = (start.checked_add(12), end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len())) {
        for marker in scan_start..scan_end {
            ctx.charge_work(16, OPERATION)?;
            if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len()) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) { continue; }
            if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
                ctx.reserve_collection_vec(&mut selections, 1, OPERATION)?;
                selections.push((marker, ids));
            }
        }
    }
    Ok(selections)
}

pub(super) const COMPACT_EDGE_VECTOR_MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

const COMPACT_COMPONENT_PATH_GAPS: &[usize] = &[0, 2, 4, 6, 8, 10, 12];
const COMPACT_ROOT_COMPONENT_PATH_GAPS: &[usize] = &[0, 2, 4, 6, 8, 10, 12, 16];

fn component_path_gaps(root_separators: bool) -> &'static [usize] {
    if root_separators {
        COMPACT_ROOT_COMPONENT_PATH_GAPS
    } else {
        COMPACT_COMPONENT_PATH_GAPS
    }
}

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
    let prefix = marker.checked_sub(8)?;
    let marker_end = marker.checked_add(16)?;
    let trailer_end = marker_end.checked_add(2)?;
    if payload.get(marker..marker_end)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(prefix..marker)? != [0; 8]
        || payload.get(marker_end..trailer_end)? != [0, 0]
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
    let cell_count = usize::try_from(View::u32_le_at(payload, header)?)
        .ok()
        .filter(|count| (2..=65).contains(count))?;
    let candidate_results = [
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
    ];
    // An exact count is an explicit vector boundary. A following path-shaped
    // record does not extend it; continuation checks only disambiguate root
    // slot interpretations.
    let exact_count_candidates = candidate_results[2].clone().into_iter().collect::<Vec<_>>();
    if let [candidate] = exact_count_candidates.as_slice() {
        return Some(candidate.0.clone());
    }
    let candidates = candidate_results.into_iter().flatten().collect::<Vec<_>>();
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
    component_path_gaps(root_separators)
        .iter()
        .copied()
        .any(|gap| {
            let root_separator = root_separators
                && gap == 10
                && payload.get(end..end + 10) == Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            (compact_component_separator(payload, end, gap) || root_separator)
                && (compact_heterogeneous_component_path(payload, end + gap, 1).is_some()
                    || compact_mixed_component_path(payload, end + gap, 1, root_separators)
                        .is_some())
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
        let type_family = View::u16_le_at(&signature, 0)?;
        let type_variant = View::u16_le_at(&signature, 2)?;
        let source = View::u32_le_at(&signature, 4)?;
        let identity = View::u32_le_at(&signature, 8)?;
        (is_class_token(type_family) && type_variant != 0 && source != 0 && identity != 0)
            .then_some(signature)
    };
    let node_at =
        |offset: usize, remaining: usize| -> Option<(FeatureInputComponentPathEntry, usize)> {
            let tagged = payload
                .get(offset..offset + 4)
                .is_some_and(|bytes| {
                    View::u16_le_at(bytes, 0).is_some_and(is_class_token) && bytes[2..4] == [0, 0]
                })
                .then(|| {
                    let instance = View::u16_le_at(payload, offset)?;
                    let type_signature = signature_at(offset + 4)?;
                    let next_is_tagged = remaining > 1
                        && payload.get(offset + 16..offset + 20).is_some_and(|bytes| {
                            View::u16_le_at(bytes, 0).is_some_and(is_class_token)
                                && bytes[2..4] == [0, 0]
                                && signature_at(offset + 20).is_some()
                        });
                    let local_id = if next_is_tagged {
                        None
                    } else {
                        Some(View::u32_le_at(payload, offset + 16)?)
                    };
                    Some((
                        FeatureInputComponentPathEntry {
                            instance: Some(instance),
                            type_signature,
                            local_id,
                        },
                        if next_is_tagged { 16 } else { 20 },
                    ))
                })
                .flatten();
            tagged.or_else(|| {
                Some((
                    FeatureInputComponentPathEntry {
                        instance: None,
                        type_signature: signature_at(offset)?,
                        local_id: Some(View::u32_le_at(payload, offset + 12)?),
                    },
                    16,
                ))
            })
        };

    let mut components = Vec::with_capacity(count);
    for index in 0..count {
        let (component, len) = node_at(cursor, count - index)?;
        components.push(component);
        cursor += len;
        if index + 1 == count {
            continue;
        }
        let gap = component_path_gaps(root_separators)
            .iter()
            .copied()
            .find(|gap| {
                let root_separator = root_separators
                    && *gap == 10
                    && payload.get(cursor..cursor + 10) == Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                (compact_component_separator(payload, cursor, *gap) || root_separator)
                    && node_at(cursor + *gap, count - index - 1).is_some()
            })?;
        cursor += gap;
    }
    Some((components, cursor))
}

fn counted_surface_component_path_at(
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
    let count = usize::try_from(View::u32_le_at(payload, header)?)
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
    let family = View::u16_le_at(&prefix, 0)?;
    let variant = View::u16_le_at(&prefix, 2)?;
    (is_class_token(family) && variant != 0).then_some(prefix)
}

fn inline_mirror_surface_paths(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    prefix: [u8; 4],
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT inline mirror surface paths";
    let signature_at = |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let source = View::u32_le_at(&signature, 4)?;
        let identity = View::u32_le_at(&signature, 8)?;
        (signature[..4] == prefix && source != 0 && identity != 0).then_some(signature)
    };
    let instance_before = |offset: usize| -> Option<u16> {
        let bytes = payload.get(offset.checked_sub(4)?..offset)?;
        let instance = View::u16_le_at(bytes, 0)?;
        (is_class_token(instance) && bytes[2..] == [0, 0]).then_some(instance)
    };
    let mut result = Vec::<(usize, Vec<FeatureInputComponentPathEntry>)>::new();
    let Some(scan_end) = end.checked_sub(16) else { return Ok(result); };
    'terminals: for terminal in start..scan_end {
        ctx.charge_work(64, OPERATION)?;
        if signature_at(terminal).is_none() {
            continue;
        }
        let Some(instance) = View::u16_le_at(payload, terminal + 12) else {
            continue;
        };
        let Some(local_tail) = payload.get(terminal + 14..terminal + 16) else {
            continue;
        };
        let next_is_component = {
            is_class_token(instance)
                && local_tail == [0, 0]
                && signature_at(terminal + 16).is_some()
        };
        if next_is_component {
            continue;
        }
        let mut cursor = terminal;
        loop {
            ctx.charge_work(32, OPERATION)?;
            if instance_before(cursor).is_none() { break; }
            let Some(previous) = cursor.checked_sub(16) else {
                break;
            };
            if signature_at(previous).is_none() {
                break;
            }
            cursor = previous;
        }
        let offset = cursor;
        let Some(mut parsed_components) = inline_surface_components_at(payload, offset) else { continue; };
        let mut components = Vec::new();
        loop {
            ctx.charge_work(32, OPERATION)?;
            let Some(component) = parsed_components.next() else { break; };
            let Some(component) = component else { continue 'terminals; };
            ctx.reserve_collection_vec(&mut components, 1, OPERATION)?;
            components.push(component);
        }
        let mut duplicate = false;
        for (_, existing) in &result {
            ctx.charge_work(u64_from_index(components.len()).checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            if existing == &components {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
            result.push((offset, components));
        }
    }
    Ok(result)
}

fn inline_surface_components_at(
    payload: &[u8],
    offset: usize,
) -> Option<impl Iterator<Item = Option<FeatureInputComponentPathEntry>> + '_> {
    let prefix: [u8; 4] = payload.get(offset..offset + 4)?.try_into().ok()?;
    let family = View::u16_le_at(&prefix, 0)?;
    let variant = View::u16_le_at(&prefix, 2)?;
    if !is_class_token(family) || variant == 0 {
        return None;
    }
    let signature_at = move |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let source = View::u32_le_at(&signature, 4)?;
        let identity = View::u32_le_at(&signature, 8)?;
        (signature[..4] == prefix && source != 0 && identity != 0).then_some(signature)
    };
    let instance_before = move |offset: usize| -> Option<u16> {
        let bytes = payload.get(offset.checked_sub(4)?..offset)?;
        let instance = View::u16_le_at(bytes, 0)?;
        (is_class_token(instance) && bytes[2..] == [0, 0]).then_some(instance)
    };
    let mut cursor = offset;
    let mut finished = false;
    Some(std::iter::from_fn(move || {
        if finished { return None; }
        let node = (|| {
            let signature = signature_at(cursor)?;
            let tail: [u8; 4] = payload.get(cursor + 12..cursor + 16)?.try_into().ok()?;
            let instance = View::u16_le_at(&tail, 0)?;
            let continues = is_class_token(instance) && tail[2..] == [0, 0]
                && signature_at(cursor + 16).is_some();
            let component = FeatureInputComponentPathEntry {
                instance: instance_before(cursor),
                type_signature: signature,
                local_id: (!continues).then(|| View::u32_le_at(&tail, 0)).flatten(),
            };
            Some((component, continues))
        })();
        match node {
            Some((component, continues)) => {
                finished = !continues;
                if continues { cursor += 16; }
                Some(Some(component))
            }
            None => {
                finished = true;
                Some(None)
            }
        }
    }))
}

fn inline_surface_reference_at(
    payload: &[u8], offset: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    inline_surface_components_at(payload, offset)?.collect()
}

/// Decode persistent surface identities declared by `*SurfIdRep_c` classes.
/// Operation-specific consumers separately project the identities that also
/// carry input selections, including projected split-line target faces.
pub(crate) fn generated_surface_identities(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<Vec<crate::records::FeatureInputGeneratedSurfaceIdentity>, CodecError> {
    const OPERATION: &str = "decode SLDPRT generated surface identities";
    struct SurfaceIdentityFields {
        offset: u64,
        input_index: usize,
        type_prefix: [u8; 4],
        feature_source_id: FeatureSourceId,
        local_identity: u32,
        components: Vec<FeatureInputComponentPathEntry>,
    }

    let signature_prefix = |bytes: &[u8], prefix: [u8; 4]| -> Option<FeatureSourceId> {
        let signature = bytes.first_chunk::<12>()?;
        let source = FeatureSourceId::try_from(View::u32_le_at(signature, 4)?).ok()?;
        let identity = View::u32_le_at(signature, 8)?;
        (signature[..4] == prefix && identity != 0).then_some(source)
    };
    let instance_before = |offset: usize| -> Option<u16> {
        let bytes = lane.native_payload.get(offset.checked_sub(4)?..offset)?;
        let instance = View::u16_le_at(bytes, 0)?;
        (is_class_token(instance) && bytes[2..] == [0, 0]).then_some(instance)
    };
    let mut prefixes = HashSet::new();
    for class in &lane.classes {
        ctx.charge_work(u64_from_index(class.name.len()).checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if !class.name.ends_with("SurfIdRep_c") { continue; }
        let prefix = (|| {
            let body = usize::try_from(class.offset).ok()?
                .checked_add(6 + class.name.len())?;
            if lane.native_payload.get(body..body + 2)? != [0, 0] { return None; }
            let prefix: [u8; 4] = lane.native_payload.get(body + 2..body + 6)?.try_into().ok()?;
            let family = View::u16_le_at(&prefix, 0)?;
            let variant = View::u16_le_at(&prefix, 2)?;
            (is_class_token(family) && variant != 0).then_some(prefix)
        })();
        let Some(prefix) = prefix else { continue; };
        if !prefixes.contains(&prefix) {
            ctx.charge_collection_items(1, OPERATION)?;
            prefixes.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            prefixes.insert(prefix);
        }
    }

    let mut result = Vec::<SurfaceIdentityFields>::new();
    let Some(last) = lane.native_payload.len().checked_sub(16) else { return Ok(Vec::new()); };
    'terminals: for terminal in 0..=last {
        ctx.charge_work(64, OPERATION)?;
        let Some(window) = lane
            .native_payload
            .get(terminal..terminal + 16)
            .and_then(<[u8]>::first_chunk::<16>)
        else {
            continue;
        };
        let [p0, p1, p2, p3, .., t0, t1, t2, t3] = *window;
        let prefix = [p0, p1, p2, p3];
        if !prefixes.contains(&prefix) {
            continue;
        }
        let Some(feature_source_id) = signature_prefix(window, prefix) else {
            continue;
        };
        let tail = [t0, t1, t2, t3];
        let possible_instance = cadmpeg_core::bytes::assemble_u16_le([t0, t1]);
        if is_class_token(possible_instance)
            && tail[2..] == [0, 0]
            && lane
                .native_payload
                .get(terminal + 16..)
                .and_then(|bytes| signature_prefix(bytes, prefix))
                .is_some()
        {
            continue;
        }
        let mut offset = terminal;
        loop {
            ctx.charge_work(32, OPERATION)?;
            if instance_before(offset).is_none() { break; }
            let Some(previous) = offset.checked_sub(16) else { break; };
            if lane.native_payload.get(previous..).and_then(|bytes| signature_prefix(bytes, prefix)).is_none() {
                break;
            }
            offset = previous;
        }
        let Some(mut parsed_components) = inline_surface_components_at(&lane.native_payload, offset) else {
            continue;
        };
        let mut components = Vec::new();
        loop {
            ctx.charge_work(32, OPERATION)?;
            let Some(component) = parsed_components.next() else { break; };
            let Some(component) = component else { continue 'terminals; };
            ctx.reserve_collection_vec(&mut components, 1, OPERATION)?;
            components.push(component);
        }
        let mut duplicate = false;
        for identity in &result {
            ctx.charge_work(u64_from_index(components.len()).checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            if identity.type_prefix == prefix && identity.components == components {
                duplicate = true;
                break;
            }
        }
        if duplicate { continue; }
        let local_identity = cadmpeg_core::bytes::assemble_u32_le(tail);
        ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
        let input_index = result.len();
        result.push(SurfaceIdentityFields {
            offset: u64_from_index(offset),
            input_index,
            type_prefix: prefix,
            feature_source_id,
            local_identity,
            components,
        });
    }
    let levels = if result.len() > 1 { result.len().ilog2() + 1 } else { 1 };
    ctx.charge_work(u64_from_index(result.len()).checked_mul(u64::from(levels))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    result.sort_unstable_by_key(|identity| (identity.offset, identity.input_index));
    let lane_key = lane.id.rsplit_once('#').map_or(lane.id.as_str(), |(_, key)| key);
    let mut identities = Vec::new();
    ctx.reserve_collection_vec(&mut identities, result.len(), OPERATION)?;
    for (ordinal, fields) in result.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(ordinal)))?;
        ctx.charge_work(u64_from_index(lane_key.len()).checked_add(u64_from_index(lane.id.len()))
            .and_then(|size| size.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        let id = ctx.format_retained(format_args!("sldprt:feature-input:generated-surface#{lane_key}:{}", fields.offset), OPERATION)?;
        let parent = ctx.format_retained(format_args!("{}", lane.id), OPERATION)?;
        identities.push(crate::records::FeatureInputGeneratedSurfaceIdentity {
            id, parent, ordinal,
            offset: fields.offset,
            type_prefix: fields.type_prefix,
            feature_source_id: fields.feature_source_id,
            local_identity: fields.local_identity,
            components: fields.components,
        });
    }
    Ok(identities)
}

fn compact_edge_selection_vector(
    ctx: &DecodeContext<'_>, payload: &[u8], base: usize,
) -> Result<Option<(usize, Vec<u32>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge selection vector";
    let Some(last_marker) = payload.len().checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) else { return Ok(None); };
    for marker in 12..=last_marker {
        ctx.charge_work(16, OPERATION)?;
        if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) { continue; }
        if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
            let offset = base.checked_add(marker)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            return Ok(Some((offset, ids)));
        }
    }
    Ok(None)
}

pub(crate) fn compact_edge_selection_at(
    ctx: &DecodeContext<'_>, payload: &[u8], marker: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge identities";
    ctx.charge_work(32, OPERATION)?;
    let count = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
            || payload.get(marker + 16..marker + 18)? != [0, 0] { return None; }
        usize::try_from(View::u32_le_at(payload, count_start)?).ok().filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else { return Ok(None); };
    if let Some(references) = compact_component_reference_list_at(payload, marker) {
        let mut ids = Vec::new();
        for reference in references {
            ctx.charge_work(1, OPERATION)?;
            if let Some(id) = reference.last().and_then(|entry| entry.local_id) {
                ctx.reserve_collection_vec(&mut ids, 1, OPERATION)?;
                ids.push(id);
            }
        }
        return Ok(Some(ids));
    }
    let mut candidate: Option<Vec<u32>> = None;
    let mut ambiguous = false;
    let mut consider = |ids: Vec<u32>| {
        if let Some(candidate) = &candidate {
            let work = u64_from_index(candidate.len()).checked_add(u64_from_index(ids.len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            if candidate != &ids { ambiguous = true; }
        } else { candidate = Some(ids); }
        Ok::<_, CodecError>(())
    };
    if let Some(ids) = compact_homogeneous_edge_ids(ctx, payload, marker + 18, count)? { consider(ids)?; }
    for (components, _) in compact_edge_component_path_candidates(payload, marker, count) {
        let mut ids = Vec::new();
        for component in components {
            ctx.charge_work(1, OPERATION)?;
            if let Some(id) = component.local_id {
                ctx.reserve_collection_vec(&mut ids, 1, OPERATION)?;
                ids.push(id);
            }
        }
        if !ids.is_empty() { consider(ids)?; }
    }
    if let Some(ids) = compact_u16_edge_ids(ctx, payload, marker + 18, count)? { consider(ids)?; }
    Ok(if ambiguous { None } else { candidate })
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
    let count = usize::try_from(View::u32_le_at(payload, count_start)?)
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

fn compact_component_reference_list_at(
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
    let count = usize::try_from(View::u32_le_at(payload, count_start)?)
        .ok()
        .filter(|count| (1..=64).contains(count))?;
    let signature_prefix: [u8; 4] = payload.get(marker + 22..marker + 26)?.try_into().ok()?;
    let prefix_token = View::u16_le_at(&signature_prefix, 0)?;
    let prefix_variant = View::u16_le_at(&signature_prefix, 2)?;
    if !is_class_token(prefix_token) || prefix_variant == 0 {
        return None;
    }
    let hop_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = View::u16_le_at(payload, offset)?;
        if !is_class_token(instance) || payload.get(offset + 2..offset + 4)? != [0, 0] {
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
        last.local_id = Some(View::u32_le_at(payload, cursor)?);
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

pub(super) fn variable_fillet_control_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_end: usize,
) -> Result<Option<Vec<(String, Vec<Vec<FeatureInputComponentPathEntry>>)>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT variable fillet controls";
    if !feature.kind.eq_ignore_ascii_case("VarFillet") {
        return Ok(None);
    }
    ctx.charge_work(lane.names.len() as u64, OPERATION)?;
    ctx.charge_work(lane.names.len() as u64, OPERATION)?;
    let Some(object_start) = feature_object_name(feature, lane)
        .and_then(|name| usize::try_from(name.offset).ok()) else {
        return Ok(None);
    };
    let Some(control_start) = fillet_edge_roster_end(ctx, lane, object_start, object_end)? else {
        return Ok(None);
    };
    let (Some(marker_start), Some(marker_end)) = (
        control_start.checked_add(12),
        object_end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
    ) else {
        return Ok(None);
    };
    let mut controls = Vec::new();
    for marker in marker_start..marker_end {
        ctx.charge_work(1, OPERATION)?;
        if lane.native_payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) {
            continue;
        }
        let Some(references) = compact_component_reference_list(&lane.native_payload, marker, false) else {
            continue;
        };
        if references.len() == 3 {
            ctx.reserve_collection_vec(&mut controls, 1, OPERATION)?;
            controls.push((marker, references));
        }
    }
    let levels = if controls.len() > 1 { controls.len().ilog2() + 1 } else { 1 };
    let count = u64::try_from(controls.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let units = count.checked_mul(u64::from(levels))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(units, OPERATION)?;
    controls.sort_unstable_by_key(|(marker, _)| *marker);
    let mut result = Vec::new();
    let mut start = control_start;
    for (marker, references) in controls {
        let mut names = lane
            .names
            .iter()
            .filter(|name| {
                name.offset > u64_from_index(start) && name.offset < u64_from_index(marker)
            })
            .filter(|name| {
                variable_fillet_dimension_index_for_feature(feature, &name.value).is_some()
            });
        ctx.charge_work(lane.names.len() as u64, OPERATION)?;
        let Some(name) = names.next() else {
            return Ok(None);
        };
        if names.next().is_some() {
            return Ok(None);
        }
        let mut name_text = String::new();
        ctx.reserve_retained_string(&mut name_text, name.value.len(), OPERATION)?;
        name_text.push_str(&name.value);
        ctx.reserve_collection_vec(&mut result, 1, OPERATION)?;
        result.push((name_text, references));
        start = marker;
    }
    Ok((!result.is_empty()).then_some(result))
}

fn variable_fillet_dimension_index(name: &str) -> Option<usize> {
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

pub(crate) fn variable_fillet_dimension_index_for_feature(
    feature: &crate::records::Feature,
    name: &str,
) -> Option<usize> {
    if name == "D1" && !feature.parameters.contains_key("D01") {
        // SW2013-era lanes use D1 for the second variable-radius control.
        return Some(1);
    }
    variable_fillet_dimension_index(name)
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
    let count = usize::try_from(View::u32_le_at(payload, count_start)?)
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
    let source = View::u32_le_at(trailer, 16)?;
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT compact edge owner";
    let count = marker.checked_sub(12).and_then(|offset| View::u32_le_at(payload, offset)).and_then(|count| usize::try_from(count).ok());
    let Some(count) = count else { return Ok(None); };
    let owner_source = if compact_component_reference_list_at(payload, marker).is_some() { None }
    else {
        let Some((_, owner)) = compact_edge_component_path(payload, marker, count) else { return Ok(None); };
        owner
    };
    if let Some(source) = owner_source {
        for feature in features {
            ctx.charge_work(1, OPERATION)?;
            if feature.source_value() != Some(source) { continue; }
            if feature_precedes_consumer(ctx, feature, features, consumer_ref)? {
                return Ok(Some(copy_selection_text(ctx, &feature.id, OPERATION)?));
            }
            break;
        }
    }
    Ok(component_path_input_features(ctx, components, features, consumer_ref)?.pop())
}

pub(crate) fn compact_edge_producer_features_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT compact edge producers";
    let mut producers = component_path_input_features(ctx, components, features, consumer_ref)?;
    if let Some(owner) = compact_edge_owner_feature_at(ctx, payload, marker, components, features, consumer_ref)? {
        let mut duplicate = false;
        for producer in &producers {
            let work = u64_from_index(producer.len()).checked_add(u64_from_index(owner.len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            if producer == &owner { duplicate = true; break; }
        }
        if !duplicate {
            ctx.reserve_collection_vec(&mut producers, 1, OPERATION)?;
            producers.push(owner);
        }
    }
    Ok(producers)
}

pub(crate) fn surface_selection_terminal_feature_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT surface selection terminal";
    if let Some(source) = compact_single_face_reference_record_at(payload, marker).and_then(|(_, source)| source) {
        let mut found = None;
        for feature in features {
            ctx.charge_work(1, OPERATION)?;
            if feature.source_value() != Some(source) { continue; }
            if found.is_some() { found = None; break; }
            found = Some(feature);
        }
        if let Some(feature) = found {
            return Ok(Some(copy_selection_text(ctx, &feature.id, OPERATION)?));
        }
    }
    component_path_terminal_feature(ctx, components, features)
}

fn compact_homogeneous_edge_ids(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    count: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT homogeneous edge identities";
    let window = (|| {
        let signature = payload.get(cursor + 4..cursor + 16)?;
        bounded_len(u64_from_index(count), 20, payload.len().checked_sub(cursor)?)?;
        Some(signature)
    })();
    let Some(signature) = window else { return Ok(None); };
    let mut ids = Vec::new();
    ctx.reserve_collection_vec(&mut ids, count, OPERATION)?;
    ctx.charge_work(u64_from_index(count).checked_mul(40)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    Ok((|| {
    for index in 0..count {
        if payload.get(cursor + 4..cursor + 16)? != signature {
            return None;
        }
        ids.push(View::u32_le_at(payload, cursor + 16)?);
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
    })())
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
        let token = View::u16_le_at(instance, 0)?;
        (is_class_token(token)
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
            instance: Some(View::u16_le_at(payload, cursor)?),
            type_signature: payload.get(cursor + 4..cursor + 16)?.try_into().ok()?,
            local_id: Some(View::u32_le_at(payload, cursor + local_id_offset)?),
        });
        cursor += entry_length;
        if index + 1 == count {
            continue;
        }
        let gap = COMPACT_COMPONENT_PATH_GAPS.iter().copied().find(|gap| {
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
                || View::u16_le_at(bytes, 0).is_some_and(|token| {
                    (is_class_token(token) && bytes[2..4] == [1, 0])
                        || (token != 0 && bytes[0..2] != [0xff, 0xff] && bytes[2..4] == [0, 0])
                })
        }),
        6 => payload.get(cursor..cursor + 6).is_some_and(|bytes| {
            View::u16_le_at(bytes, 0).is_some_and(|token| token != u16::MAX) && bytes[2..] == [0; 4]
        }),
        8 => {
            let (Some(first), Some(second)) = (
                View::u32_le_at(payload, cursor),
                View::u32_le_at(payload, cursor + 4),
            ) else {
                return false;
            };
            (first == 0 && second == 0)
                || (first == u32::MAX && second <= 1)
                || (first == 0 && !matches!(second, 0 | u32::MAX))
                || (second == 0 && !matches!(first, 0 | u32::MAX))
        }
        10 => payload.get(cursor..cursor + 10) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]),
        12 => payload.get(cursor..cursor + 12) == Some(&[0; 12]),
        16 => {
            payload.get(cursor..cursor + 16)
                == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        }
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
        let token = View::u16_le_at(instance, 0)?;
        (is_class_token(token)
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
                Some(View::u32_le_at(payload, cursor + 16)?)
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
            for gap in COMPACT_COMPONENT_PATH_GAPS {
                if !compact_component_separator(payload, end, *gap) {
                    continue;
                }
                let Some(next) = end.checked_add(*gap) else {
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

fn compact_u16_edge_ids(
    ctx: &DecodeContext<'_>, payload: &[u8], cursor: usize, count: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT short edge identities";
    let end = count.checked_mul(2).and_then(|len| cursor.checked_add(len));
    let Some(end) = end else { return Ok(None); };
    let Some(bytes) = payload.get(cursor..end) else { return Ok(None); };
    let Some(suffix) = payload.get(end..) else { return Ok(None); };
    let mut ids = Vec::new();
    ctx.reserve_collection_vec(&mut ids, count, OPERATION)?;
    ctx.charge_work(u64_from_index(count).checked_mul(2).and_then(|work| work.checked_add(32))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    let mut view = View::over_retained(bytes);
    for _ in 0..count {
        let Some(id) = view.u16_le() else { return Ok(None); };
        ids.push(u32::from(id));
    }
    let sentinel_terminated = suffix.get(..19).is_some_and(|suffix| {
        suffix[..16].iter().all(|byte| *byte == 0) && suffix[16..19] == [0xff, 0xfe, 0xff]
    });
    let object_terminated = suffix.get(..10).is_some_and(|suffix| {
        suffix[..8].iter().all(|byte| *byte == 0)
            && View::u16_le_at(suffix, 8).is_some_and(is_class_token)
    });
    Ok((ids.iter().all(|id| *id != 0) && (sentinel_terminated || object_terminated)).then_some(ids))
}

fn compact_body_selection_vector(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    base: usize,
    next_object_token: Option<u16>,
) -> Result<Option<(usize, Vec<u32>)>, CodecError> {
    const SCHEMA: &[u8] = &11000u32.to_le_bytes();
    const OPERATION: &str = "decode SLDPRT compact body selection vector";
    let Some(last) = payload.len().checked_sub(16) else { return Ok(None); };
    for relative in (0..=last).rev() {
        ctx.charge_work(32, OPERATION)?;
        if payload.get(relative..relative + 4) != Some(SCHEMA)
            || payload.get(relative + 4..relative + 12) != Some(&[0; 8])
        {
            continue;
        }
        let Some(count) =
            View::u32_le_at(payload, relative + 12).and_then(|count| usize::try_from(count).ok())
        else {
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
        let Some(ids) = payload.get(relative + 16..ids_end) else { continue; };
        let local_body_ids = read_compact_body_ids(ctx, ids, OPERATION)?;
        let Some(offset) = base.checked_add(relative) else { return Ok(None); };
        return Ok(Some((offset, local_body_ids)));
    }
    Ok(None)
}

pub(crate) fn compact_body_selection_at(
    ctx: &DecodeContext<'_>, payload: &[u8], offset: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact body selection";
    ctx.charge_work(32, OPERATION)?;
    let Some(header_end) = offset.checked_add(12) else { return Ok(None); };
    let Some(schema_end) = offset.checked_add(4) else { return Ok(None); };
    if payload.get(offset..schema_end) != Some(11000u32.to_le_bytes().as_slice())
        || payload.get(schema_end..header_end) != Some(&[0; 8])
    {
        if payload.get(offset..schema_end).is_none() || payload.get(schema_end..header_end).is_none() {
            return Ok(None);
        }
        return super::direct_edits::move_body_selection_at(ctx, payload, offset);
    }
    let ids = (|| {
        let count = usize::try_from(View::u32_le_at(payload, offset.checked_add(12)?)?).ok()?;
        let ids_start = offset.checked_add(16)?;
        let ids_end = ids_start.checked_add(count.checked_mul(4)?)?;
        let sentinel_end = ids_end.checked_add(4)?;
        let zeros_end = sentinel_end.checked_add(12)?;
        if payload.get(ids_end..sentinel_end)? != u32::MAX.to_le_bytes()
            || payload.get(sentinel_end..zeros_end)? != [0; 12]
        { return None; }
        payload.get(ids_start..ids_end)
    })();
    match ids {
        Some(ids) => Ok(Some(read_compact_body_ids(ctx, ids, OPERATION)?)),
        None => Ok(None),
    }
}

pub(super) fn read_compact_body_ids(
    ctx: &DecodeContext<'_>, bytes: &[u8], operation: &'static str,
) -> Result<Vec<u32>, CodecError> {
    let mut result = Vec::new();
    ctx.reserve_collection_vec(&mut result, bytes.len() / 4, operation)?;
    ctx.charge_work(u64_from_index(bytes.len() / 4), operation)?;
    let mut view = View::over_retained(bytes);
    while let Some(id) = view.u32_le() {
        result.push(id);
    }
    Ok(result)
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
        let id = View::u32_le_at(payload, source)?;
        let stamp = View::u32_le_at(payload, source + 4)?;
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
    let prefix = marker.checked_sub(8)?;
    let prefix_end = marker.checked_sub(4)?;
    let marker_end = marker.checked_add(16)?;
    let trailer_end = marker_end.checked_add(2)?;
    if payload.get(marker..marker_end)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(prefix..prefix_end)? != [0x04, 0x02, 0, 0]
        || payload.get(marker_end..trailer_end)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(payload, marker.checked_sub(12)?)?)
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
                instance: Some(View::u16_le_at(payload, cursor)?),
                type_signature: signature,
                local_id: Some(View::u32_le_at(payload, cursor + 16)?),
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
            NativeOperandTag::TAG_80CC
            | NativeOperandTag::TAG_8152
            | NativeOperandTag::TAG_81B2
            | NativeOperandTag::TAG_8AB6
            | NativeOperandTag::TAG_8DCB
            | NativeOperandTag::TAG_929D
            | NativeOperandTag::TAG_BC7C
            | NativeOperandTag::TAG_BD69
            | NativeOperandTag::TAG_81DD,
        ) => {
            matches!(
                marker,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_837B) => matches!(
            marker,
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        ),
        FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_80AC | NativeOperandTag::TAG_80D5 | NativeOperandTag::TAG_8138,
        ) => matches!(
            marker,
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::Relation(_)
        ),
        FeatureInputOperandKind::E1
        | FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_8386
            | NativeOperandTag::TAG_83FE
            | NativeOperandTag::TAG_8DDA
            | NativeOperandTag::TAG_BC87
            | NativeOperandTag::TAG_81E7,
        ) => {
            // 81e7 also selects curve markers in coordinate-marker link cells;
            // scalar relation operands use the solver-line path separately.
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
            | FeatureInputOperandKind::Native(
                NativeOperandTag::TAG_80CC
                    | NativeOperandTag::TAG_81B2
                    | NativeOperandTag::TAG_81DD
                    | NativeOperandTag::TAG_83FE
                    | NativeOperandTag::TAG_8AB6
                    | NativeOperandTag::TAG_929D
                    | NativeOperandTag::TAG_BD69
            )
    )
}

pub(super) fn operand_allows_compatible_ordinal_fallback(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_837B
                | NativeOperandTag::TAG_8386
                | NativeOperandTag::TAG_8DCB
                | NativeOperandTag::TAG_8DDA
                | NativeOperandTag::TAG_BC7C
                | NativeOperandTag::TAG_BC87
        )
    )
}

pub(super) fn marker_local_links(payload: &[u8], offset: usize) -> Option<([u16; 2], u16)> {
    if legacy_wide_profile_roster_curve(payload, offset)
        || wide_indexed_curve_endpoint_indices(payload, offset).is_some()
    {
        return None;
    }
    if payload.get(offset + 70..offset + 72)? != [0, 0]
        || payload.get(offset + 72..offset + 80)? != (-1.0f64).to_le_bytes()
    {
        return None;
    }
    Some((
        [
            View::u16_le_at(payload, offset + 64)?,
            View::u16_le_at(payload, offset + 66)?,
        ],
        View::u16_le_at(payload, offset + 68)?,
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
        let tag = View::u16_le_at(cell, 0)?;
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
        links.push(View::u16_le_at(cell, 2)?);
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
mod tests;
