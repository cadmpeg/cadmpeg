//! Compact body, edge and surface selection decoding.

pub(super) mod diameter_index;
use diameter_index::CosmeticDiameterIndex;

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
use super::scalars::{operand_kind, ObjectNames};
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
use crate::records::charged_clone::CloneCharged;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputEdgeSelection,
    FeatureInputLane, FeatureInputOperandKind, FeatureInputSurfaceSelection, SketchInputKind,
};
use cadmpeg_core::decode::{bounded_len, DecodeContext, ScopedReservation, View};
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
) -> Result<
    Vec<(
        &'lane crate::records::FeatureInputName,
        &'history crate::records::Feature,
        usize,
    )>,
    CodecError,
> {
    let names = ObjectNames::new(ctx, lane)?;
    let mut objects = Vec::new();
    let mut input_index = 0_usize;
    for history in ctx.admit_iter(histories, operation)? {
        for feature in ctx.admit_iter(&history.features, operation)? {
            let index = input_index;
            input_index += 1;
            let Some(name) = names.of(ctx, feature)? else {
                continue;
            };
            ctx.push_vec(&mut objects, (name, feature, index), operation)?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut objects,
        |value| {
            let (left_name, _, left_index) = value;
            (left_name.offset, *left_index)
        },
        Ord::cmp,
        operation,
    )?;
    Ok(objects)
}

pub(super) fn compact_body_selections(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputBodySelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact body selections";
    let (objects, _objects_storage) = ctx.with_scoped_storage(OPERATION, || selection_objects(ctx, histories, lane, OPERATION))?;
    let lane_key = ctx
        .rsplit_once(&lane.id, "#", "split SLDPRT feature-input lane key")?
        .map_or(lane.id.as_str(), |(_, key)| key);
    let state_token = compact_body_state_token(ctx, lane)?;
    let (move_data_offsets, _move_data_storage) =
        super::profiles::class_offsets(ctx, lane, "moMoveCopyBodyData_c", OPERATION)?;
    let mut result = Vec::new();
    for (object_index, &(name, feature, _)) in ctx.admit_iter(&objects, OPERATION)?.enumerate() {
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
            let lower = ctx.partition_point(
                &move_data_offsets,
                |offset| Ok(*offset < u64_from_index(start)),
                OPERATION,
            )?;
            let upper = ctx.partition_point(
                &move_data_offsets,
                |offset| Ok(*offset < u64_from_index(end)),
                OPERATION,
            )?;
            match move_data_offsets.get(lower..upper.max(lower)) {
                Some(&[class_offset]) => super::direct_edits::move_body_translation_record(
                    ctx,
                    &lane.native_payload,
                    start,
                    end,
                    class_offset,
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
        let ordinal = u32::try_from(result.len()).map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len()))
        })?;
        let id = ctx.format_retained(
            format_args!("sldprt:feature-input:body-selection#{lane_key}:{offset}"),
            OPERATION,
        )?;
        let parent = ctx.copy_retained_text(&lane.id, OPERATION)?;
        let object_name_ref = ctx.copy_retained_text(&name.id, OPERATION)?;
        let feature_ref = ctx.copy_retained_text(&feature.id, OPERATION)?;
        let (body_state_ids, mode) = match (kind, state_token) {
            (NativeClassKind::DeleteBody, Some(token)) => (
                compact_body_state_ids(ctx, &lane.native_payload, start, offset, token)?,
                compact_body_retention_mode(ctx, &lane.native_payload, start, offset, token)?,
            ),
            _ => (Vec::new(), None),
        };
        ctx.reserve_vec(&mut result, 1, OPERATION)?;
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

fn compact_body_state_token(ctx: &DecodeContext<'_>, lane: &FeatureInputLane) -> Result<Option<u16>, CodecError> {
    const OPERATION: &str = "find SLDPRT unique body state class";
    let mut selected = None;
    let ambiguous = ctx.any_by(&lane.classes, |class| {
        if class.name != "moDeleteBodyData_c" { return Ok(false); }
        if selected.is_some() { return Ok(true); }
        selected = Some(class);
        Ok(false)
    }, OPERATION)?;
    if ambiguous { return Ok(None); }
    Ok(selected.and_then(|class| {
        let offset = usize::try_from(class.offset).ok()?;
        View::u16_le_at(&lane.native_payload, offset + 8 + class.name.len())
    }))
}

pub(crate) fn compact_body_state_ids_for_selection(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Result<Vec<u32>, CodecError> {
    const OPERATION: &str = "decode SLDPRT body state identities";
    let Some(token) = compact_body_state_token(ctx, lane)? else {
        return Ok(Vec::new());
    };
    let start = ctx
        .find_by(
            &lane.names,
            |name| {
                ctx.equal(
                    name.id.as_str(),
                    selection.object_name_ref.as_str(),
                    OPERATION,
                )
            },
            OPERATION,
        )?
        .and_then(|name| usize::try_from(name.offset).ok());
    let (Some(start), Ok(end)) = (start, usize::try_from(selection.offset)) else {
        return Ok(Vec::new());
    };
    compact_body_state_ids(ctx, &lane.native_payload, start, end, token)
}

pub(crate) fn compact_body_retention_mode_for_selection(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Result<Option<cadmpeg_ir::features::BodyRetentionMode>, CodecError> {
    const OPERATION: &str = "decode SLDPRT body retention mode";
    let Some(token) = compact_body_state_token(ctx, lane)? else {
        return Ok(None);
    };
    let start = ctx
        .find_by(
            &lane.names,
            |name| {
                ctx.equal(
                    name.id.as_str(),
                    selection.object_name_ref.as_str(),
                    OPERATION,
                )
            },
            OPERATION,
        )?
        .and_then(|name| usize::try_from(name.offset).ok());
    let (Some(start), Ok(end)) = (start, usize::try_from(selection.offset)) else {
        return Ok(None);
    };
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
    let Some(scan_end) = end.checked_sub(HEADER_LEN - 1) else {
        return Ok(None);
    };
    let token = token.to_le_bytes();
    let Some(offset) = ctx.find_by((start..scan_end).rev(), |offset| Ok(compact_body_state_id(payload, *offset, token).is_some()), OPERATION)? else { return Ok(None); };
    Ok((|| {
        let state_end = offset + HEADER_LEN;
        let field = payload.get(state_end..state_end + 10)?;
        if field[0..2] != [0x30, 0x80] || field[6..10] != [0; 4] { return None; }
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Result<Vec<u32>, CodecError> {
    const HEADER_LEN: usize = 83;
    const OPERATION: &str = "decode SLDPRT body state identities";
    let token = token.to_le_bytes();
    let mut result = Vec::new();
    let Some(scan_end) = end.checked_sub(HEADER_LEN - 1) else {
        return Ok(result);
    };
    for offset in ctx.admit_iter(start..scan_end, OPERATION)? {
        let Some(body_id) = compact_body_state_id(payload, offset, token) else {
            continue;
        };
        ctx.reserve_vec(&mut result, 1, OPERATION)?;
        result.push(body_id);
    }
    Ok(result)
}

/// Decode an edge-selection reference list, including the count-framed
/// unpadded roster form used by variable-radius fillets.
pub(crate) fn compact_edge_reference_list_for_feature(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    feature_kind: &str,
) -> Result<Option<Vec<Vec<FeatureInputComponentPathEntry>>>, CodecError> {
    if let Some(references) = compact_component_reference_list_at(ctx, payload, offset)? {
        return Ok(Some(references));
    }
    if !feature_kind.eq_ignore_ascii_case("VarFillet") {
        return Ok(None);
    }
    let count = offset
        .checked_sub(12)
        .and_then(|start| View::u32_le_at(payload, start))
        .and_then(|count| usize::try_from(count).ok());
    let Some(count) = count else {
        return Ok(None);
    };
    let (references, storage) = ctx.with_scoped_storage(
        "decode SLDPRT variable fillet references",
        || compact_component_reference_list(ctx, payload, offset, false),
    )?;
    let references = references.filter(|references| references.len() == count);
    if references.is_some() { storage.commit()?; }
    Ok(references)
}

pub(super) fn compact_edge_selections(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    history_features: &[crate::records::Feature],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputEdgeSelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge selections";
    let (objects, _objects_storage) = ctx.with_scoped_storage(OPERATION, || selection_objects(ctx, histories, lane, OPERATION))?;
    let lane_key = ctx
        .rsplit_once(&lane.id, "#", "split SLDPRT feature-input lane key")?
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    let dimension_classes = fillet_dimension_classes(ctx, lane)?;
    let mut compact_edge_class = None;
    if ctx.any_by(&lane.classes, |class| {
        if class.name != "moCompEdge_c" { return Ok(false); }
        Ok(compact_edge_class.replace(class).is_some())
    }, OPERATION)? { compact_edge_class = None; }
    let class_name_end = compact_edge_class.and_then(|class| {
        usize::try_from(class.offset)
            .ok()?
            .checked_add(6 + class.name.len())
    });
    let compact_edge_token =
        class_name_end.and_then(|offset| View::u16_le_at(&lane.native_payload, offset));
    for (object_index, &(name, feature, _)) in ctx.admit_iter(&objects, OPERATION)?.enumerate() {
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
            fillet_edge_roster_end(ctx, lane, &dimension_classes, start, object_end)?
                .unwrap_or(object_end)
        } else {
            object_end
        };
        let direct_child = compact_edge_class
            .and_then(|class| usize::try_from(class.offset).ok())
            .filter(|offset| (start..end).contains(offset));
        let (selections, _selections_storage) = ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
        let mut selections = Vec::new();
        if let Some(child_start) = direct_child {
            let selection = match lane.native_payload.get(child_start..end) {
                Some(payload) => compact_edge_selection_vector(ctx, payload, child_start)?,
                None => None,
            };
            if let Some(selection) = selection {
                ctx.reserve_vec(&mut selections, 1, OPERATION)?;
                selections.push(selection);
            }
        }
        if let Some(token) = compact_edge_token {
            let repeated = repeated_edge_selections(ctx, &lane.native_payload, start, end, token)?;
            ctx.extend_vec(&mut selections, repeated, OPERATION)?;
        }
        let interval = edge_selection_vectors_in_interval(ctx, &lane.native_payload, start, end)?;
        ctx.extend_vec(&mut selections, interval, OPERATION)?;
        ctx.sort_unstable_by(
            &mut selections,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT compact edge selections",
        )?;
        ctx.dedup_by_key(
            &mut selections,
            |selection| Ok(selection.0),
            "deduplicate SLDPRT compact edge selection offsets",
        )?;
        Ok(selections)
        })?;
        let mut feature_selections_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut feature_selections = Vec::new();
        for (offset, local_edge_ids) in ctx.admit_iter(selections, OPERATION)? {
            let (selection, storage) = ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
            let references = compact_edge_reference_list_for_feature(
                ctx,
                &lane.native_payload,
                offset,
                &feature.kind,
            )?
            .unwrap_or_default();
            // Keep the established component projection separate from the
            // reference-list projection.  A vertex-bearing multi-hop
            // reference is excluded as a whole from `components`; its
            // lineage must not leak into the edge path merely because the
            // roster fallback retained the reference itself.
            let components = compact_edge_component_path_at(ctx, &lane.native_payload, offset)?
                .unwrap_or_default();
            let terminal_feature_ref = compact_edge_owner_feature_at(
                ctx,
                &lane.native_payload,
                offset,
                &components,
                history_features,
                &feature.id,
            )?;
            let producer_feature_refs = compact_edge_producer_features_at(
                ctx,
                &lane.native_payload,
                offset,
                &components,
                history_features,
                &feature.id,
            )?;
            let id = ctx.format_retained(
                format_args!("sldprt:feature-input:edge-selection#{lane_key}:{offset}"),
                OPERATION,
            )?;
            let parent = ctx.copy_retained_text(&lane.id, OPERATION)?;
            let object_name_ref = ctx.copy_retained_text(&name.id, OPERATION)?;
            let feature_ref = ctx.copy_retained_text(&feature.id, OPERATION)?;
            let local_edge_ids = ctx.collect_vec(local_edge_ids.iter().copied(), OPERATION)?;
            Ok(FeatureInputEdgeSelection {
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
            })
            })?;
            feature_selections_storage.with_storage(|| ctx.push_vec(&mut feature_selections, (selection, storage), OPERATION))?;
        }
        for (mut selection, storage) in ctx.admit_iter(
            input_owned_edge_selections(ctx, feature_selections)?,
            OPERATION,
        )? {
            selection.ordinal = u32::try_from(result.len()).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len()))
            })?;
            ctx.reserve_vec(&mut result, 1, OPERATION)?;
            result.push(selection);
            storage.commit()?;
        }
    }
    Ok(result)
}

/// The declarations of one class in a lane, in offset order, with the class token their
/// repeated instances carry when every declaration names the same one.
pub(super) struct ClassObjects<'ctx> {
    offsets: Vec<usize>,
    token: Option<u16>,
    tokens: Vec<u16>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> ClassObjects<'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &FeatureInputLane,
        class_name: &'static str,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "decode SLDPRT selection class intervals";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut offsets = Vec::new();
        let mut tokens = Vec::new();
        for class in ctx.admit_iter(&lane.classes, OPERATION)? {
            if class.name != class_name {
                continue;
            }
            let Some(offset) = usize::try_from(class.offset).ok() else {
                continue;
            };
            storage.with_storage(|| ctx.push_vec(&mut offsets, offset, OPERATION))?;
            let candidate = offset
                .checked_add(6 + class.name.len())
                .and_then(|body| View::u16_le_at(&lane.native_payload, body))
                .filter(|token| is_class_token(*token));
            if let Some(candidate) = candidate {
                storage.with_storage(|| ctx.push_vec(&mut tokens, candidate, OPERATION))?;
            }
        }
        ctx.sort_unstable_by_key(&mut offsets, |offset| *offset, Ord::cmp, OPERATION)?;
        ctx.sort_unstable_by_key(&mut tokens, |token| *token, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut tokens, OPERATION)?;
        let token = match tokens.as_slice() {
            [token] => Some(*token),
            _ => None,
        };
        Ok(Self { offsets, token, tokens, _storage: storage })
    }
}

impl ClassObjects<'_> {
    /// The declaration offsets in `start..end`.
    fn in_interval(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
    ) -> Result<&[usize], CodecError> {
        const OPERATION: &str = "decode SLDPRT selection class intervals";
        let lower = ctx.partition_point(&self.offsets, |offset| Ok(*offset < start), OPERATION)?;
        let upper = ctx.partition_point(&self.offsets, |offset| Ok(*offset < end), OPERATION)?;
        Ok(&self.offsets[lower..upper.max(lower)])
    }
}

/// The edge- and vertex-dimension declarations that end a fillet's edge roster.
pub(super) fn fillet_dimension_classes<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<[ClassObjects<'ctx>; 2], CodecError> {
    Ok([
        ClassObjects::new(ctx, lane, "moEdgeDim_c")?,
        ClassObjects::new(ctx, lane, "moVertDim_c")?,
    ])
}

fn fillet_edge_roster_end(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    classes: &[ClassObjects<'_>; 2],
    start: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    let mut first = None;
    for class in classes {
        if let Some(offset) = first_class_object_in_interval(ctx, lane, class, start, end)? {
            first = Some(first.map_or(offset, |first: usize| first.min(offset)));
        }
    }
    Ok(first)
}

fn first_class_object_in_interval(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    class: &ClassObjects<'_>,
    start: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    const OPERATION: &str = "decode SLDPRT selection class intervals";
    let mut direct = None;
    for &offset in ctx.admit_iter(class.in_interval(ctx, start, end)?, OPERATION)? {
        let record = offset
            .checked_sub(4)
            .filter(|record| lane.native_payload.get(*record..*record + 2) == Some(&[0x20, 0x81]))
            .unwrap_or(offset);
        direct = Some(direct.map_or(record, |direct: usize| direct.min(record)));
    }
    let mut repeated = None;
    if let (Some(token), Some(scan_end)) = (class.token, end.checked_sub(7)) {
        let token = token.to_le_bytes();
        repeated = ctx
            .position_by(
                start..scan_end.max(start),
                |offset| {
                    Ok(
                        lane.native_payload.get(offset..offset + 2) == Some(&[0x20, 0x81])
                            && lane.native_payload.get(offset + 2..offset + 4)
                                == Some(&[0x10, 0x00])
                            && View::u16_le_at(&lane.native_payload, offset + 4)
                                .is_some_and(is_class_token)
                            && lane.native_payload.get(offset + 6..offset + 8)
                                == Some(token.as_slice()),
                    )
                },
                OPERATION,
            )?
            .map(|position| start + position);
    }
    Ok(direct.into_iter().chain(repeated).min())
}

/// The selections a producer path ties to an input feature, or every selection when none is.
pub(super) fn input_owned_edge_selections<'ctx>(
    ctx: &DecodeContext<'_>,
    mut selections: Vec<(FeatureInputEdgeSelection, ScopedReservation<'ctx>)>,
) -> Result<Vec<(FeatureInputEdgeSelection, ScopedReservation<'ctx>)>, CodecError> {
    const OPERATION: &str = "retain SLDPRT input-owned edge selections";
    if ctx.any_by(
        &selections,
        |(selection, _)| Ok(!selection.producer_feature_refs.is_empty()),
        OPERATION,
    )? {
        ctx.retain_vec(
            &mut selections,
            |(selection, _)| Ok(!selection.producer_feature_refs.is_empty()),
            OPERATION,
        )?;
    }
    Ok(selections)
}

pub(super) fn compact_surface_selections(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    history_features: &[crate::records::Feature],
    lane: &FeatureInputLane,
    identities: &[crate::records::FeatureInputGeneratedSurfaceIdentity],
) -> Result<Vec<FeatureInputSurfaceSelection>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact surface selections";
    let mut surface_class = None;
    if ctx.any_by(&lane.classes, |class| {
        if class.name != "moCompSurfaceBody_c" { return Ok(false); }
        Ok(surface_class.replace(class).is_some())
    }, OPERATION)? { surface_class = None; }
    let surface_token = surface_class.and_then(|class| {
        usize::try_from(class.offset)
            .ok()
            .and_then(|offset| offset.checked_add(6 + class.name.len()))
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
    });
    let mut cylinder_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut cylinder_reference_tokens = HashSet::new();
    for class in ctx.admit_iter(&lane.classes, OPERATION)? {
        if class.name != "moCylinderRef_w" {
            continue;
        }
        let Some(body) = usize::try_from(class.offset)
            .ok()
            .and_then(|offset| offset.checked_add(6 + class.name.len()))
        else {
            continue;
        };
        let Some(token) =
            View::u16_le_at(&lane.native_payload, body).filter(|token| is_class_token(*token))
        else {
            continue;
        };
        cylinder_storage.with_storage(|| ctx.insert_hash_set(&mut cylinder_reference_tokens, token, OPERATION))?;
    }
    let mirror_surface_prefix = mirror_surface_type_prefix(ctx, lane)?;
    let operation_classes = OperationSurfaceClasses::new(ctx, lane, identities)?;
    let diameter_index = CosmeticDiameterIndex::new(ctx, lane)?;
    let face_classes = &operation_classes.faces;
    let reference_plane_classes = ClassObjects::new(ctx, lane, "moFaceRefPlnData_c")?;
    let (objects, _objects_storage) = ctx.with_scoped_storage(OPERATION, || selection_objects(ctx, histories, lane, OPERATION))?;
    let lane_key = ctx
        .rsplit_once(&lane.id, "#", "split SLDPRT feature-input lane key")?
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    for (index, &(name, feature, _)) in ctx.admit_iter(&objects, OPERATION)?.enumerate() {
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
            ctx.find_by(&objects[index + 1..], |next| Ok(!ctx.equal(&next.1.id, &feature.id, OPERATION)?), OPERATION)?
        } else {
            objects.get(index + 1)
        };
        let end = next_object
            .and_then(|(next, _, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let (candidates, _candidate_storage) = ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
        let candidates = match kind {
            NativeClassKind::Thicken => {
                let mut candidates = Vec::new();
                if let (Some(token), Some(scan_end)) = (surface_token, end.checked_sub(105)) {
                    for offset in ctx.admit_iter(start..scan_end, OPERATION)? {
                        if lane.native_payload.get(offset..offset + 2) != Some(token) {
                            continue;
                        }
                        let marker = offset + 103;
                        if let Some(ids) =
                            compact_surface_selection_at(ctx, &lane.native_payload, marker)?
                        {
                            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push((marker, ids));
                        }
                    }
                }
                candidates
            }
            NativeClassKind::Extrusion => {
                let mut candidates = Vec::new();
                if let Some(scan_end) = end.checked_sub(103) {
                    for offset in ctx.admit_iter(start..scan_end, OPERATION)? {
                        let marker = match compact_extrusion_to_face_at(
                            ctx,
                            &lane.native_payload,
                            offset,
                            end,
                        )? {
                            Some(marker) => Some(marker),
                            None => match compact_extrusion_to_vertex_at(
                                ctx,
                                &lane.native_payload,
                                offset,
                                end,
                            )? {
                                Some((marker, _)) => Some(marker),
                                None => compact_extrusion_offset_from_face_at(
                                    ctx,
                                    &lane.native_payload,
                                    offset,
                                    end,
                                )?,
                            },
                        };
                        let reference = match marker {
                            Some(marker) => compact_termination_reference_path_at(
                                ctx,
                                &lane.native_payload,
                                marker,
                            )?
                            .map(|ids| (marker, ids)),
                            None => None,
                        };
                        if let Some((marker, ids)) = reference {
                            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
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
                 &diameter_index)?;
                let mut component_face_references = Vec::new();
                for &offset in
                    ctx.admit_iter(face_classes.in_interval(ctx, start, end)?, OPERATION)?
                {
                    let reference = match offset.checked_add(6 + "moCompFace_c".len()) {
                        Some(body) => component_face_reference_at(ctx, &lane.native_payload, body)?,
                        None => None,
                    };
                    if let Some(reference) = reference {
                        ctx.reserve_vec(&mut component_face_references, 1, OPERATION)?;
                        component_face_references.push(reference);
                    }
                }
                // The component edge is a fallback carrier. Some objects serialize
                // both forms for one support; admitting both would fail the
                // single-selection invariant even though a canonical carrier is
                // already authoritative.
                let component_references =
                    if cylinder_references.is_empty() && component_face_references.is_empty() {
                        cosmetic_thread_component_references(ctx, lane, start, end)?
                    } else {
                        Vec::new()
                    };
                let mut candidates = Vec::new();
                for candidate in ctx.admit_iter(cylinder_references, OPERATION)?
                    .chain(ctx.admit_iter(component_references, OPERATION)?)
                    .chain(ctx.admit_iter(component_face_references, OPERATION)?)
                {
                    ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push(candidate);
                }
                candidates
            }
            NativeClassKind::Fillet if feature.input_class.as_deref() == Some("Fillet_c") => {
                fillet_face_selection_candidates(ctx, lane, &face_classes, start, end)?
            }
            NativeClassKind::Fillet => return Ok(Vec::new()),
            NativeClassKind::MirrorPattern => {
                const OPERATION: &str = "decode SLDPRT mirror surface selections";
                let mut candidates = Vec::new();
                if let (Some(scan_start), Some(scan_end)) = (
                    start.checked_add(12),
                    end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
                ) {
                    for marker in ctx.admit_iter(scan_start..scan_end, OPERATION)? {
                        if lane
                            .native_payload
                            .get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
                            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                        {
                            continue;
                        }
                        if let Some(components) =
                            counted_surface_component_path_at(ctx, &lane.native_payload, marker)?
                        {
                            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                            candidates.push((marker, components));
                        }
                    }
                }
                if let Some(prefix) = mirror_surface_prefix {
                    let inline =
                        inline_mirror_surface_paths(ctx, &lane.native_payload, start, end, prefix)?;
                    ctx.extend_vec(&mut candidates, inline, OPERATION)?;
                }
                candidates
            }
            NativeClassKind::ReferencePlane => face_reference_plane_selection_candidates(
                ctx,
                lane,
                &face_classes,
                &reference_plane_classes,
                start,
                end,
            )?,
            NativeClassKind::PlanarSurface => {
                planar_surface_selection_candidates(ctx, &lane.native_payload, start, end)?
            }
            NativeClassKind::Operation(operation) => operation_surface_selection_candidates(
                ctx,
                operation,
                lane,
                &operation_classes,
                start,
                end,
                name.object_id.and_then(ObjectId::value),
            )?,
            _ => return Ok(Vec::new()),
        };
        Ok(candidates)
        })?;
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
        for (offset, components) in ctx.admit_iter(candidates, OPERATION)? {
            let components = ctx.collect_vec(components.iter().cloned(), OPERATION)?;
            let endpoint_selector = if kind == NativeClassKind::Extrusion {
                compact_extrusion_endpoint_selector_for_marker(
                    ctx,
                    &lane.native_payload,
                    start,
                    end,
                    offset,
                )?
            } else {
                None
            };
            let terminal_feature_ref = surface_selection_terminal_feature_at(
                ctx,
                &lane.native_payload,
                offset,
                &components,
                history_features,
            )?;
            let producer_feature_refs = surface_selection_producer_features(
                ctx,
                &components,
                terminal_feature_ref.as_deref(),
                history_features,
            )?;
            let id = ctx.format_retained(
                format_args!("sldprt:feature-input:surface-selection#{lane_key}:{offset}"),
                OPERATION,
            )?;
            let parent = ctx.copy_retained_text(&lane.id, OPERATION)?;
            let object_name_ref = ctx.copy_retained_text(&name.id, OPERATION)?;
            let feature_ref = ctx.copy_retained_text(&feature.id, OPERATION)?;
            let ordinal = u32::try_from(result.len()).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(result.len()))
            })?;
            ctx.reserve_vec(&mut result, 1, OPERATION)?;
            result.push(FeatureInputSurfaceSelection {
                id,
                parent,
                ordinal,
                offset: u64_from_index(offset),
                selector: lane.native_payload
                    [offset.checked_sub(8).map_or(0, std::convert::identity)],
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    marker: usize,
) -> Result<Option<u32>, CodecError> {
    ctx.find_map(start..end, |body| {
        let Some((candidate, kind)) = compact_extrusion_to_vertex_at(ctx, payload, body, end)? else { return Ok(None); };
        Ok((candidate == marker).then(|| kind.endpoint_selector()).flatten())
    }, "scan SLDPRT extrusion endpoint selectors")
}

fn fillet_face_selection_candidates(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    face_classes: &ClassObjects<'_>,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT full round fillet surface candidates";
    // A full-round Fillet_c carries center, first-side, and second-side face
    // carriers in that order. Other role-03 counts are different fillet
    // constructions and remain outside this projection.
    let mut class_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut class_bodies = Vec::new();
    for &class_offset in ctx.admit_iter(face_classes.in_interval(ctx, start, end)?, OPERATION)? {
        let Some(body) = class_offset.checked_add(6 + "moCompFace_c".len()) else {
            continue;
        };
        let Some(token) =
            View::u16_le_at(&lane.native_payload, body).filter(|token| is_class_token(*token))
        else {
            continue;
        };
        class_storage.with_storage(|| ctx.reserve_vec(&mut class_bodies, 1, OPERATION))?;
        class_bodies.push((body, token));
    }
    ctx.sort_unstable_by(
        &mut class_bodies,
        |value| value,
        Ord::cmp,
        "sort SLDPRT full round fillet class bodies",
    )?;
    ctx.dedup_vec(
        &mut class_bodies,
        "deduplicate SLDPRT full round fillet class bodies",
    )?;
    let mut candidates = Vec::new();
    if let Some(scan_end) = end.checked_sub(6) {
        for (body, token) in ctx.admit_iter(class_bodies, OPERATION)? {
            let token = token.to_le_bytes();
            for offset in ctx.admit_iter(body..scan_end, OPERATION)? {
                let header = lane.native_payload.get(offset..offset + 6);
                if offset != body
                    && (header.and_then(|header| header.get(..2)) != Some(token.as_slice())
                        || header.and_then(|header| header.get(2..6)) != Some(&[2, 0, 0, 0]))
                {
                    continue;
                }
                let Some((marker, components)) = component_face_reference_at_for_full_round_fillet(
                    ctx,
                    &lane.native_payload,
                    offset,
                )?
                else {
                    continue;
                };
                let Some(selector) = marker
                    .checked_sub(8)
                    .and_then(|start| lane.native_payload.get(start..marker - 4))
                else {
                    continue;
                };
                if !is_component_vector_selector_for_role(selector, 3) {
                    continue;
                }
                ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                candidates.push((marker, components));
            }
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 3 {
        candidates
    } else {
        Vec::new()
    })
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
        for marker in ctx.admit_iter(start..scan_end, OPERATION)? {
            let Some(selector) = marker
                .checked_sub(8)
                .and_then(|start| payload.get(start..marker - 4))
            else {
                continue;
            };
            if !is_component_vector_selector_for_role(selector, 2) {
                continue;
            }
            if let Some(components) = component_vector_path_at(
                ctx,
                payload,
                marker,
                "decode SLDPRT component vector path",
            )? {
                ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                candidates.push((marker, components));
            }
        }
    }
    Ok(candidates)
}

fn face_reference_plane_selection_candidates(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    face_classes: &ClassObjects<'_>,
    reference_plane_classes: &ClassObjects<'_>,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT reference plane surface candidates";
    let mut candidates = Vec::new();
    if let &[data_offset] = reference_plane_classes.in_interval(ctx, start, end)? {
        let Some(body) = data_offset.checked_add(6 + "moFaceRefPlnData_c".len()) else {
            return Ok(Vec::new());
        };
        if let Some(scan_end) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) {
            for marker in ctx.admit_iter(body..scan_end, OPERATION)? {
                if lane
                    .native_payload
                    .get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
                    != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                {
                    continue;
                }
                if let Some(components) =
                    counted_surface_component_path_at(ctx, &lane.native_payload, marker)?
                {
                    ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push((marker, components));
                }
            }
        }
    }
    for &offset in ctx.admit_iter(face_classes.in_interval(ctx, start, end)?, OPERATION)? {
        let reference = match offset.checked_add(6 + "moCompFace_c".len()) {
            Some(body) => component_face_reference_at(ctx, &lane.native_payload, body)?,
            None => None,
        };
        if let Some(candidate) = reference {
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push(candidate);
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 1 {
        candidates
    } else {
        Vec::new()
    })
}

/// Orders candidates by marker offset and drops repeated (offset, path) pairs.
fn order_surface_candidates(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<(usize, Vec<FeatureInputComponentPathEntry>)>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.stable_sort_by_key(candidates, |value| value.0, Ord::cmp, operation)?;
    ctx.dedup_by(
        candidates,
        |left, right| Ok(left.0 == right.0 && ctx.equal(&left.1, &right.1, operation)?),
        operation,
    )
}

struct OperationSurfaceClasses<'identities, 'ctx> {
identities: &'identities [crate::records::FeatureInputGeneratedSurfaceIdentity],
    surfaces: ClassObjects<'ctx>,
    faces: ClassObjects<'ctx>,
    split_classes: bool,
}

impl<'identities, 'ctx> OperationSurfaceClasses<'identities, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>, lane: &FeatureInputLane, identities: &'identities [crate::records::FeatureInputGeneratedSurfaceIdentity]) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT operation surface classes";
        let mut split_classes = true;
        for required in ["moPLineProjIdRep_c", "moPLineSurfIdRep_c"] {
            if !ctx.any_by(&lane.classes, |class| Ok(class.name == required), OPERATION)? {
                split_classes = false;
                break;
            }
        }
        Ok(Self {
            surfaces: ClassObjects::new(ctx, lane, "moCompSurfaceBody_c")?,
            faces: ClassObjects::new(ctx, lane, "moCompFace_c")?,
            split_classes, identities,
        })
    }
}

fn operation_surface_selection_candidates(
    ctx: &DecodeContext<'_>,
    operation: FeatureClass,
    lane: &FeatureInputLane,
    classes: &OperationSurfaceClasses<'_, '_>,
    start: usize,
    end: usize,
    object_source: Option<u32>,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT operation surface candidates";
    if operation == FeatureClass::CutWithSurface {
        let mut candidates = Vec::new();
        if let Some(scan_end) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) {
            for marker in ctx.admit_iter(start..scan_end, OPERATION)? {
                let Some(selector) = marker
                    .checked_sub(8)
                    .and_then(|start| lane.native_payload.get(start..marker - 4))
                else {
                    continue;
                };
                if !is_component_vector_selector_for_role(selector, 2) {
                    continue;
                }
                // The first role-02 vector is the target-body reference list;
                // the later vector belongs to the moCompSurfaceBody_c cutting
                // surface child. The selector's low byte is lane-local.
                let components = match compact_component_reference_list(
                    ctx,
                    &lane.native_payload,
                    marker,
                    false,
                )? {
                    Some(references) => {
                        let mut components = Vec::new();
                        for reference in ctx.admit_iter(references, OPERATION)? {
                            ctx.extend_vec(&mut components, reference, OPERATION)?;
                        }
                        Some(components)
                    }
                    None => compact_surface_selection_at(ctx, &lane.native_payload, marker)?,
                };
                if let Some(components) = components {
                    ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push((marker, components));
                }
            }
        }
        return Ok(candidates);
    }
    if operation == FeatureClass::SplitFace {
        const OPERATION: &str = "project SLDPRT split surface identity paths";

        if !classes.split_classes {
            return Ok(Vec::new());
        }
        let Some(object_source) = object_source else {
            return Ok(Vec::new());
        };
        let mut candidates = Vec::new();
        for identity in ctx.admit_iter(classes.identities, OPERATION)? {
            let (Some(first), Some(last)) =
                (identity.components.first(), identity.components.last())
            else {
                continue;
            };
            if component_source(first) != Some(object_source)
                || component_source(last).is_none_or(|source| source == object_source)
                || last.local_id.is_none()
            {
                continue;
            }
            let Ok(offset) = usize::try_from(identity.offset) else {
                continue;
            };
            let components = ctx.collect_vec(identity.components.iter().cloned(), OPERATION)?;
            ctx.push_vec(&mut candidates, (offset, components), OPERATION)?;
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

    let mut candidates = match classes.surfaces.in_interval(ctx, start, end)? {
        [offset] => compact_surface_selection_candidates_for_class(
            ctx, &lane.native_payload, *offset, start, end,
        )?,
        _ => Vec::new(),
    };
    for &offset in ctx.admit_iter(classes.faces.in_interval(ctx, start, end)?, OPERATION)? {
        let Some(body) = offset.checked_add(6 + "moCompFace_c".len()) else {
            continue;
        };
        if let Some(candidate) = component_face_reference_at_for_operation(ctx, &lane.native_payload, body)? {
            ctx.push_vec(&mut candidates, candidate, OPERATION)?;
        }
    }
    for &token in ctx.admit_iter(&classes.faces.tokens, OPERATION)? {
        let repeated = component_face_reference_candidates(ctx, &lane.native_payload, token, start, end)?;
        ctx.extend_vec(&mut candidates, repeated, OPERATION)?;
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
    class_offset: usize,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT class surface candidates";
    if !(start..end).contains(&class_offset) {
        return Ok(Vec::new());
    }
    let Some(body) = class_offset.checked_add(6 + "moCompSurfaceBody_c".len()) else {
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
    for marker in ctx.admit_iter(body..=last_marker, OPERATION)? {
        if let Some(components) = compact_surface_selection_at(ctx, bounded_payload, marker)? {
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push((marker, components));
        }
    }
    Ok(candidates)
}

/// A pass's history copy; lane-local source identities are cleared before the next lane.
pub(crate) struct SelectionHistory<'history, 'ctx> {
    histories: &'history [crate::records::FeatureHistory],
    features: Option<Vec<crate::records::Feature>>,
    missing_sources: Vec<usize>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'history, 'ctx> SelectionHistory<'history, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        histories: &'history [crate::records::FeatureHistory],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            histories,
            features: None,
            missing_sources: Vec::new(),
            _storage: ctx.reserve_scoped(0, "clone SLDPRT selection history features")?,
        })
    }

    pub(crate) fn for_lane(
        &mut self,
        ctx: &DecodeContext<'_>,
        lane: &FeatureInputLane,
    ) -> Result<&[crate::records::Feature], CodecError> {
        const OPERATION: &str = "clone SLDPRT selection history features";
        if self.features.is_none() {
            let mut features = Vec::new();
            for history in ctx.admit_iter(self.histories, OPERATION)? {
                for feature in ctx.admit_iter(&history.features, OPERATION)? {
                    self._storage.with_storage(|| {
                        if feature.source_id.is_none() {
                            ctx.push_vec(&mut self.missing_sources, features.len(), OPERATION)?;
                        }
                        let feature = feature.clone_charged(ctx, OPERATION)?;
                        ctx.push_vec(&mut features, feature, OPERATION)
                    })?;
                }
            }
            self.features = Some(features);
        }
        let features = self.features.get_or_insert_with(Vec::new);
        for &index in ctx.admit_iter(&self.missing_sources, OPERATION)? {
            features[index].source_id = None;
        }
        enrich_feature_object_sources(ctx, features, std::slice::from_ref(lane))?;
        Ok(features)
    }
}

/// Bind flat idless history records to identities from unique feature-input
/// object names without changing records that already carry source identity.
pub(crate) fn enrich_feature_object_sources(
    ctx: &DecodeContext<'_>,
    features: &mut [crate::records::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT selection feature object sources";
    if !ctx.any_by(
        &*features,
        |feature| Ok(feature.source_id.is_none()),
        OPERATION,
    )? {
        return Ok(());
    }
    let mut lane_names_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut lane_names = Vec::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        lane_names_storage.with_storage(|| ctx.push_vec(&mut lane_names, ObjectNames::new(ctx, lane)?, OPERATION))?;
    }
    for feature in ctx.admit_iter(features, OPERATION)? {
        if feature.source_id.is_some() {
            continue;
        }
        let mut source = None;
        let mut ambiguous = false;
        let mut names_iter = lane_names.iter();
        while let Some(names) = ctx.next_charged(&mut names_iter, OPERATION)? {
            let Some(candidate) = names
                .of(ctx, feature)?
                .and_then(|name| name.object_id)
                .and_then(ObjectId::value)
            else {
                continue;
            };
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
    diameter_index: &CosmeticDiameterIndex<'_, '_>,
) -> Result<Vec<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT cosmetic cylinder references";
    let diameter_tail = diameter_index.tail(ctx, feature)?;
    let ranges = match diameter_tail {
        Some(tail) if object_start < object_end && tail.start <= object_end && object_start <= tail.end =>
            [Some(object_start.min(tail.start)..object_end.max(tail.end)), None],
        tail => [Some(object_start..object_end), tail],
    };
    let mut scan_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut offsets = Vec::new();
    for range in ranges.into_iter().flatten() {
    for offset in ctx.admit_iter(range, OPERATION)? {
        if let Some(token) = View::u16_le_at(&lane.native_payload, offset) {
        if ctx.contains_hash_set(cylinder_reference_tokens, &token, OPERATION)? {
            scan_storage.with_storage(|| ctx.push_vec(&mut offsets, offset, OPERATION))?;
        }
        }
    }
    }
    ctx.sort_unstable_by(
        &mut offsets,
        |value| value,
        Ord::cmp,
        "sort SLDPRT cosmetic thread cylinder offsets",
    )?;
    ctx.dedup_vec(
        &mut offsets,
        "deduplicate SLDPRT cosmetic thread cylinder offsets",
    )?;
    let mut references = Vec::new();
    if let Some(reference) = ctx.find_map(offsets, |offset| cosmetic_thread_cylinder_reference_at(ctx, &lane.native_payload, offset), OPERATION)? {
        ctx.push_vec(&mut references, reference, OPERATION)?;
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut classes = Vec::new();
    for class in ctx.admit_iter(&lane.classes, OPERATION)? {
        let Some(offset) = usize::try_from(class.offset)
            .ok()
            .filter(|offset| (object_start..object_end).contains(offset))
        else {
            continue;
        };
        storage.with_storage(|| ctx.push_vec(&mut classes, (offset, class), OPERATION))?;
    }
    ctx.sort_unstable_by(
        &mut classes,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT cosmetic component classes",
    )?;

    let mut class_ranges = Vec::<Range<usize>>::new();
    for (index, &(class_offset, class)) in ctx.admit_iter(&classes, OPERATION)?.enumerate() {
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
        storage.with_storage(|| ctx.push_vec(&mut class_ranges, body..direct_end, OPERATION))?;

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
            storage.with_storage(|| ctx.push_vec(&mut class_ranges, edge_ref_body..edge_ref_end, OPERATION))?;
        }
    }
    storage.with_storage(|| {
        let repeated = cosmetic_thread_repeated_component_edge_ranges(ctx, &lane.native_payload, object_start, object_end)?;
        ctx.extend_vec(&mut class_ranges, repeated, OPERATION)
    })?;
    ctx.sort_unstable_by_key(&mut class_ranges, |range| (range.start, range.end), Ord::cmp, OPERATION)?;
    let mut ranges = Vec::<Range<usize>>::new();
    for range in ctx.admit_iter(class_ranges, OPERATION)? {
        if let Some(previous) = ranges.last_mut().filter(|previous| range.start <= previous.end) {
            previous.end = previous.end.max(range.end);
        } else {
            storage.with_storage(|| ctx.push_vec(&mut ranges, range, OPERATION))?;
        }
    }
    let mut references = Vec::new();
    for range in ctx.admit_iter(ranges, OPERATION)? {
        for marker in ctx.admit_iter(range, OPERATION)? {
        if lane
            .native_payload
            .get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        {
            continue;
        }
        if let Some(components) = compact_edge_component_path_at(ctx, &lane.native_payload, marker)?
        {
            ctx.reserve_vec(&mut references, 1, OPERATION)?;
            references.push((marker, components));
        }
    }
    }
    order_surface_candidates(ctx, &mut references, OPERATION)?;
    ctx.dedup_by_key(&mut references, |(marker, _)| Ok(*marker), OPERATION)?;
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut child_offsets = Vec::new();
    if let Some(last_child) = end.checked_sub(2 + repeated_edge_ref::LEN) {
        for offset in ctx.admit_iter(object_start..=last_child, OPERATION)? {
            if View::u16_le_at(payload, offset).is_some_and(is_class_token)
                && payload.get(offset + 2..offset + 2 + repeated_edge_ref::LEN)
                    == Some(repeated_edge_ref::PREFIX_VALUE.as_slice()) {
                storage.with_storage(|| ctx.push_vec(&mut child_offsets, offset, OPERATION))?;
            }
        }
    }
    let mut ranges = Vec::new();
    for token_offset in ctx.admit_iter(object_start..=last_token, OPERATION)? {
        if !View::u16_le_at(payload, token_offset).is_some_and(is_class_token)
            || !cosmetic_thread_component_edge_wrapper_at(payload, token_offset + 2) {
            continue;
        }
        let body = token_offset + 2;
        let child_start = body + component_edge::COMPONENT_COUNT;
        let child = ctx.partition_point(&child_offsets, |offset| Ok(*offset < child_start), OPERATION)?;
        let start = child_offsets.get(child).map_or(body, |offset| offset + 2);
        ctx.push_vec(&mut ranges, start..end, OPERATION)?;
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

#[derive(Debug, PartialEq)]
pub(super) struct CylinderMarkerReference(
    pub usize,
    pub Option<Vec<FeatureInputComponentPathEntry>>,
);

impl cadmpeg_core::decode::cost::DecodeCost for CylinderMarkerReference {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let mut bytes = cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(self.0, self.1.is_some()),
            ctx,
            operation,
        )?;
        if let Some(components) = &self.1 {
            for component in ctx.admit_iter(components, operation)? {
                let child = cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                    &(
                        &component.instance,
                        &component.type_signature,
                        &component.local_id,
                    ),
                    ctx,
                    operation,
                )?;
                bytes = bytes
                    .checked_add(child)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            }
        }
        Ok(bytes)
    }
}

pub(super) fn cosmetic_thread_cylinder_marker_reference(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
    cylinder_reference_tokens: &HashSet<u16>,
    diameter_index: &CosmeticDiameterIndex<'_, '_>,
) -> Result<Vec<CylinderMarkerReference>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT cosmetic thread cylinder markers";
    let diameter_tail = diameter_index.tail(ctx, feature)?;
    let ranges = match diameter_tail {
        Some(tail) if object_start < object_end && tail.start <= object_end && object_start <= tail.end =>
            [Some(object_start.min(tail.start)..object_end.max(tail.end)), None],
        tail => [Some(object_start..object_end), tail],
    };
    let mut scan_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut markers = Vec::new();
    for range in ranges.into_iter().flatten() {
    for body in ctx.admit_iter(range, OPERATION)? {
        let Some(token) = View::u16_le_at(&lane.native_payload, body) else { continue; };
        if !ctx.contains_hash_set(cylinder_reference_tokens, &token, OPERATION)? { continue; }
        let Some(marker) =
            cosmetic_thread_cylinder_reference_marker_layout_at(&lane.native_payload, body)
        else {
            continue;
        };
        scan_storage.with_storage(|| ctx.push_vec(&mut markers, marker, OPERATION))?;
    }
    }
    ctx.sort_unstable_by(
        &mut markers,
        |value| value,
        Ord::cmp,
        "sort SLDPRT cosmetic thread cylinder markers",
    )?;
    ctx.dedup_vec(
        &mut markers,
        "deduplicate SLDPRT cosmetic thread cylinder markers",
    )?;
    let mut references = Vec::new();
    ctx.reserve_vec(&mut references, markers.len(), OPERATION)?;
    for marker in ctx.admit_iter(markers, OPERATION)? {
        let path =
            match compact_sketch_surface_component_path_at(ctx, &lane.native_payload, marker)? {
                Some(path) => Some(path),
                None => compact_termination_reference_path_at(ctx, &lane.native_payload, marker)?,
            };
        let components = match path {
            Some(components) => Some(components),
            None => compact_edge_component_path_at(ctx, &lane.native_payload, marker)?,
        };
        references.push(CylinderMarkerReference(marker, components));
    }
    Ok(references)
}



fn cosmetic_thread_cylinder_reference_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    let Some(marker) = cosmetic_thread_cylinder_reference_marker_layout_at(payload, body_offset)
    else {
        return Ok(None);
    };
    let path = match compact_sketch_surface_component_path_at(ctx, payload, marker)? {
        Some(path) => Some(path),
        None => compact_termination_reference_path_at(ctx, payload, marker)?,
    };
    let components = match path {
        Some(components) => Some(components),
        None => compact_edge_component_path_at(ctx, payload, marker)?,
    };
    Ok(components.map(|components| (marker, components)))
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
        let nested_face_class_length = u16::try_from(NESTED_FACE_CLASS.len()).ok()?;
        let nested_face_class = payload
            .get(body_offset..body_offset + nested_face::COMPONENT_MARKER)
            .is_some_and(|body| {
                body.windows(CLASS_MARKER.len() + 2 + NESTED_FACE_CLASS.len())
                    .any(|header| {
                        &header[..CLASS_MARKER.len()] == CLASS_MARKER
                            && header[CLASS_MARKER.len()..CLASS_MARKER.len() + 2]
                                == nested_face_class_length.to_le_bytes()
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
    let Some(marker_offsets) = marker_offsets else {
        return Ok(None);
    };
    let mut candidate = None;
    for relative in marker_offsets {
        let Some(marker) = body_offset.checked_add(*relative) else { continue; };
        let (components, storage) = ctx.with_scoped_storage("decode SLDPRT component face path", || compact_surface_reference_at(ctx, payload, marker))?;
        let Some(components) = components else { continue; };
        if !include_compact_frame { storage.commit()?; return Ok(Some((marker, components))); }
        if candidate.is_some() { return Ok(None); }
        candidate = Some((marker, components, storage));
    }
    match candidate {
        Some((marker, components, storage)) => { storage.commit()?; Ok(Some((marker, components))) }
        None => Ok(None),
    }
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
        for offset in ctx.admit_iter(start..scan_end, OPERATION)? {
            if View::u16_le_at(bounded_payload, offset) != Some(class_token) {
                continue;
            }
            if let Some(candidate) =
                component_face_reference_at_for_operation(ctx, bounded_payload, offset)?
            {
                ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                candidates.push(candidate);
            }
        }
    }
    order_surface_candidates(ctx, &mut candidates, OPERATION)?;
    Ok(candidates)
}

pub(super) fn component_face_reference_in_record(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(usize, Vec<FeatureInputComponentPathEntry>)>, CodecError> {
    const CLASS: &[u8] = b"moCompFace_c";
    const OPERATION: &str = "decode SLDPRT component face record";
    let header_length = CLASS_MARKER.len() + 2 + CLASS.len();
    let Ok(class_length) = u16::try_from(CLASS.len()) else {
        return Ok(None);
    };
    let mut candidate = None;
    let ambiguous = ctx.any_by(payload.windows(header_length).enumerate(), |(offset, header)| {
        if &header[..CLASS_MARKER.len()] != CLASS_MARKER
            || header[CLASS_MARKER.len()..CLASS_MARKER.len() + 2] != class_length.to_le_bytes()
            || &header[CLASS_MARKER.len() + 2..] != CLASS { return Ok(false); }
        let (reference, storage) = ctx.with_scoped_storage(OPERATION, || component_face_reference_at(ctx, payload, offset + header_length))?;
        let Some(reference) = reference else { return Ok(false); };
        if let Some((first, _)) = &candidate { Ok(!ctx.equal(first, &reference, OPERATION)?) }
        else { candidate = Some((reference, storage)); Ok(false) }
    }, OPERATION)?;
    if ambiguous { return Ok(None); }
    match candidate {
        Some((reference, storage)) => { storage.commit()?; Ok(Some(reference)) }
        None => Ok(None),
    }
}

fn compact_sketch_surface_component_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT sketch surface path", || -> Result<_, CodecError> {
    let header = (|| {
        if payload.get(marker.checked_sub(12)?..marker - 8)? != 5u32.to_le_bytes()
            || payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(marker + 16..marker + 18)? != [0, 0]
        {
            return None;
        }
        let kind = payload.get(marker - 8..marker - 4)?;
        let selector = View::u32_le_at(payload, marker - 4)?;
        Some((kind, selector))
    })();
    let Some((kind, selector)) = header else {
        return Ok(None);
    };
    let Some((components, end)) = compact_heterogeneous_component_path(
        ctx,
        payload,
        marker + 18,
        3,
        "decode SLDPRT component path layout",
    )?
    else {
        return Ok(None);
    };
    Ok(match kind {
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
    })
    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

fn compact_surface_selection_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT compact surface path", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact surface path";
    let header = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        let entries_start = marker.checked_add(18)?;
        if payload.get(marker..marker.checked_add(16)?)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(count_start..count_start + 4)? != 6u32.to_le_bytes()
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0, 0]
            || !is_component_vector_selector_for_role(payload.get(kind_start..kind_start + 4)?, 2)
            || payload.get(marker + 16..entries_start)? != [0, 0]
        {
            return None;
        }
        let signature: [u8; 12] = payload
            .get(entries_start + 4..entries_start + 16)?
            .try_into()
            .ok()?;
        Some((entries_start, signature))
    })();
    let Some((mut cursor, signature)) = header else {
        return Ok(None);
    };
    let mut components = Vec::new();
    let mut offsets = std::iter::successors(Some(cursor), |offset| {
        let next = offset.checked_add(20)?;
        Some(if payload.get(next + 4..next + 16) != Some(signature.as_slice())
            && payload.get(next + 8..next + 20) == Some(signature.as_slice()) { next.checked_add(4)? } else { next })
    });
    while let Some(offset) = ctx.next_charged(&mut offsets, OPERATION)? {
        cursor = offset;
        if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice()) { break; }
        let Some(entry) = (|| { Some(FeatureInputComponentPathEntry {
            instance: Some(View::u16_le_at(payload, cursor)?), type_signature: signature,
            local_id: Some(View::u32_le_at(payload, cursor + 16)?),
        }) })() else { return Ok(None); };
        ctx.push_vec(&mut components, entry, OPERATION)?;
    }
    Ok((!components.is_empty()).then_some(components))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

fn flatten_surface_references(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    counted: bool,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "flatten SLDPRT surface references";
    let (references, _references_storage) = ctx.with_scoped_storage(OPERATION, || {
        if counted { compact_component_reference_list_at(ctx, payload, marker) }
        else { compact_component_reference_list(ctx, payload, marker, false) }
    })?;
    let Some(references) = references else {
        return Ok(None);
    };
    let mut components = Vec::new();
    for reference in ctx.admit_iter(references, OPERATION)? {
        ctx.extend_vec(&mut components, reference, OPERATION)?;
    }
    Ok(Some(components))
}

fn compact_surface_reference_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    if let Some(path) = compact_surface_selection_at(ctx, payload, marker)? {
        return Ok(Some(path));
    }
    if let Some(path) =
        component_vector_path_at(ctx, payload, marker, "decode SLDPRT component vector path")?
    {
        return Ok(Some(path));
    }
    if let Some(path) = flatten_surface_references(ctx, payload, marker, true)? {
        return Ok(Some(path));
    }
    if let Some(path) = flatten_surface_references(ctx, payload, marker, false)? {
        return Ok(Some(path));
    }
    if let Some(path) = counted_surface_component_path_at(ctx, payload, marker)? {
        return Ok(Some(path));
    }
    if let Some(path) = compact_termination_reference_path_at(ctx, payload, marker)? {
        return Ok(Some(path));
    }
    if let Some(path) = compact_sketch_surface_component_path_at(ctx, payload, marker)? {
        return Ok(Some(path));
    }
    inline_surface_reference_at(ctx, payload, marker)
}

pub(crate) fn surface_reference_matches_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    expected: &[FeatureInputComponentPathEntry],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "compare SLDPRT surface reference candidates";
    for kind in 0..8 {
        let (components, _storage) = ctx.with_scoped_storage(OPERATION, || match kind {
            0 => compact_surface_selection_at(ctx, payload, marker),
            1 => component_vector_path_at(ctx, payload, marker, "decode SLDPRT component vector path"),
            2 => flatten_surface_references(ctx, payload, marker, true),
            3 => flatten_surface_references(ctx, payload, marker, false),
            4 => counted_surface_component_path_at(ctx, payload, marker),
            5 => compact_termination_reference_path_at(ctx, payload, marker),
            6 => compact_sketch_surface_component_path_at(ctx, payload, marker),
            _ => inline_surface_reference_at(ctx, payload, marker),
        })?;
        if let Some(components) = components {
            if ctx.equal(components.as_slice(), expected, OPERATION)? { return Ok(true); }
        }
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
        for offset in ctx.admit_iter(start..scan_end, OPERATION)? {
            if payload.get(offset..offset + 2) != Some(token.as_slice())
                || payload.get(offset + 2) != Some(&2)
            {
                continue;
            }
            let marker = offset + 108;
            if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
                ctx.reserve_vec(&mut selections, 1, OPERATION)?;
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
    if let (Some(scan_start), Some(scan_end)) = (
        start.checked_add(12),
        end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
    ) {
        for marker in ctx.admit_iter(scan_start..scan_end, OPERATION)? {
            if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
                != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
            {
                continue;
            }
            if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
                ctx.reserve_vec(&mut selections, 1, OPERATION)?;
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    let admitted = (|| {
        let prefix = marker.checked_sub(8)?;
        let marker_end = marker.checked_add(16)?;
        let trailer_end = marker_end.checked_add(2)?;
        if payload.get(marker..marker_end)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(prefix..marker)? != [0; 8]
            || payload.get(marker_end..trailer_end)? != [0, 0]
        {
            return None;
        }
        Some(())
    })();
    if admitted.is_none() {
        return Ok(None);
    }
    component_vector_path_at(ctx, payload, marker, "decode SLDPRT component vector path")
}

pub(super) fn component_vector_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    reserve_operation: &'static str,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT component vector path";
    let count = (|| {
        let header = marker.checked_sub(12)?;
        if payload.get(marker..marker.checked_add(16)?)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(marker + 16..marker.checked_add(18)?)? != [0, 0]
        {
            return None;
        }
        usize::try_from(View::u32_le_at(payload, header)?)
            .ok()
            .filter(|count| (2..=65).contains(count))
    })();
    let Some(cell_count) = count else {
        return Ok(None);
    };
    let (exact, exact_storage) = ctx.with_scoped_storage(OPERATION, || compact_mixed_component_path(ctx, payload, marker + 18, cell_count, true, reserve_operation))?;
    if let Some((components, _)) = exact {
        exact_storage.commit()?;
        return Ok(Some(components));
    }
    drop(exact_storage);
    // An exact count states the boundary; shorter root-slot paths need continuation checks.
    let layouts = [
        (true, Some(cell_count - 1)),
        (true, (cell_count > 2).then(|| cell_count - 2)),
        (false, Some(cell_count - 1)),
        (false, (cell_count > 2).then(|| cell_count - 2)),
        (false, (cell_count % 2 == 1).then(|| cell_count.div_ceil(2))),
    ];
    let mut unique = None;
    for (heterogeneous, count) in layouts {
        let Some(count) = count else { continue; };
        let (candidate, storage) = ctx.with_scoped_storage(OPERATION, || {
            if heterogeneous { compact_heterogeneous_component_path(ctx, payload, marker + 18, count, reserve_operation) }
            else { compact_mixed_component_path(ctx, payload, marker + 18, count, true, reserve_operation) }
        })?;
        let Some(candidate) = candidate else { continue; };
        if component_path_continues(payload, candidate.1, true) { continue; }
        if let Some((first, _)) = &unique {
            if !ctx.equal(first, &candidate, OPERATION)? { return Ok(None); }
        } else { unique = Some((candidate, storage)); }
    }
    match unique {
        Some(((components, _), storage)) => { storage.commit()?; Ok(Some(components)) }
        None => Ok(None),
    }
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
                && (compact_component_entry_at(payload, end + gap, false).is_some()
                    || compact_mixed_component_at(payload, end + gap, 1).is_some())
        })
}

fn compact_mixed_component_at(
    payload: &[u8],
    offset: usize,
    remaining: usize,
) -> Option<(FeatureInputComponentPathEntry, usize)> {
    let signature_at = |offset: usize| -> Option<[u8; 12]> {
        let signature: [u8; 12] = payload.get(offset..offset + 12)?.try_into().ok()?;
        let type_family = View::u16_le_at(&signature, 0)?;
        let type_variant = View::u16_le_at(&signature, 2)?;
        let source = View::u32_le_at(&signature, 4)?;
        let identity = View::u32_le_at(&signature, 8)?;
        (is_class_token(type_family) && type_variant != 0 && source != 0 && identity != 0)
            .then_some(signature)
    };
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
}

pub(super) fn compact_mixed_component_path(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    root_separators: bool,
    reserve_operation: &'static str,
) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT mixed component path", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT mixed component path";
    let mut components = Vec::new();
    ctx.reserve_capacity(&mut components, count, reserve_operation)?;
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
        let Some((component, len)) = compact_mixed_component_at(payload, cursor, count - index)
        else {
            return Ok(None);
        };
        ctx.push_vec(&mut components, component, reserve_operation)?;
        cursor += len;
        if index + 1 == count {
            continue;
        }
        let Some(gap) = component_path_gaps(root_separators)
            .iter()
            .copied()
            .find(|gap| {
                let root_separator = root_separators
                    && *gap == 10
                    && payload.get(cursor..cursor + 10) == Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                (compact_component_separator(payload, cursor, *gap) || root_separator)
                    && compact_mixed_component_at(payload, cursor + *gap, count - index - 1)
                        .is_some()
            })
        else {
            return Ok(None);
        };
        cursor += gap;
    }
    Ok(Some((components, cursor)))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

fn counted_surface_component_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT counted surface path";
    let count = (|| {
        let header = marker.checked_sub(12)?;
        if payload.get(marker..marker.checked_add(16)?)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(marker - 7..marker - 4)? != [2, 0, 0]
            || payload.get(marker + 16..marker.checked_add(18)?)? != [0, 0]
        {
            return None;
        }
        usize::try_from(View::u32_le_at(payload, header)?)
            .ok()
            .filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    let mut unique = None;
    for count in [Some(count), (count > 1).then(|| count - 1)] {
        let Some(count) = count else { continue; };
        let (candidate, storage) = ctx.with_scoped_storage(OPERATION, || compact_mixed_component_path(ctx, payload, marker + 18, count, false, "decode SLDPRT mixed component path"))?;
        let Some(candidate) = candidate else { continue; };
        if component_path_continues(payload, candidate.1, false) { continue; }
        if let Some((first, _)) = &unique {
            if !ctx.equal(first, &candidate, OPERATION)? { return Ok(None); }
        } else { unique = Some((candidate, storage)); }
    }
    match unique {
        Some(((components, _), storage)) => { storage.commit()?; Ok(Some(components)) }
        None => Ok(None),
    }
}

fn mirror_surface_type_prefix(ctx: &DecodeContext<'_>, lane: &FeatureInputLane) -> Result<Option<[u8; 4]>, CodecError> {
    const OPERATION: &str = "find SLDPRT unique mirror surface class";
    let mut selected = None;
    let ambiguous = ctx.any_by(&lane.classes, |class| {
        if class.name != "moMirPatternSurfIdRep_c" { return Ok(false); }
        if selected.is_some() { return Ok(true); }
        selected = Some(class);
        Ok(false)
    }, OPERATION)?;
    if ambiguous { return Ok(None); }
    Ok(selected.and_then(|class| {
        let offset = usize::try_from(class.offset).ok()?;
        let signature = offset.checked_add(8 + class.name.len())?;
        let prefix: [u8; 4] = lane.native_payload.get(signature..signature + 4)?.try_into().ok()?;
        let family = View::u16_le_at(&prefix, 0)?;
        let variant = View::u16_le_at(&prefix, 2)?;
        (is_class_token(family) && variant != 0).then_some(prefix)
    }))
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
    let mut index_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::<(usize, Vec<FeatureInputComponentPathEntry>)>::new();
    let Some(scan_end) = end.checked_sub(16) else {
        return Ok(result);
    };
    'terminals: for terminal in ctx.admit_iter(start..scan_end, OPERATION)? {
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
        let mut predecessors = std::iter::successors(Some(terminal), |offset| offset.checked_sub(16));
        while let Some(current) = ctx.next_charged(&mut predecessors, OPERATION)? {
            if instance_before(current).is_none() {
                break;
            }
            let Some(previous) = current.checked_sub(16) else {
                break;
            };
            if signature_at(previous).is_none() {
                break;
            }
            cursor = previous;
        }
        let offset = cursor;
        let Some(mut parsed_components) = inline_surface_components_at(payload, offset) else {
            continue;
        };
        let mut component_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut components = Vec::new();
        while let Some(component) = ctx.next_charged(&mut parsed_components, OPERATION)? {
            let Some(component) = component else {
                continue 'terminals;
            };
            component_storage.with_storage(|| ctx.push_vec(&mut components, component, OPERATION))?;
        }
        let (key, key_storage) = ctx.with_scoped_storage(OPERATION, || ctx.collect_vec(
            components.iter().map(|component| (component.instance, component.type_signature, component.local_id)), OPERATION))?;
        if index_storage.with_storage(|| ctx.insert_hash_set(&mut seen, key, OPERATION))? {
            index_storage.with_storage(|| key_storage.commit())?;
            component_storage.commit()?;
            ctx.push_vec(&mut result, (offset, components), OPERATION)?;
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
        if finished {
            return None;
        }
        let node = (|| {
            let signature = signature_at(cursor)?;
            let tail: [u8; 4] = payload.get(cursor + 12..cursor + 16)?.try_into().ok()?;
            let instance = View::u16_le_at(&tail, 0)?;
            let continues = is_class_token(instance)
                && tail[2..] == [0, 0]
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
                if continues {
                    cursor += 16;
                }
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT inline surface reference", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT inline surface reference";
    let Some(mut parsed_components) = inline_surface_components_at(payload, offset) else {
        return Ok(None);
    };
    let mut components = Vec::new();
    while let Some(component) = ctx.next_charged(&mut parsed_components, OPERATION)? {
        let Some(component) = component else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut components, 1, OPERATION)?;
        components.push(component);
    }
    Ok(Some(components))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
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
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut prefixes = HashSet::new();
    let mut seen = HashSet::new();
    for class in ctx.admit_iter(&lane.classes, OPERATION)? {
        if !class.name.ends_with("SurfIdRep_c") {
            continue;
        }
        let prefix = (|| {
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
            let family = View::u16_le_at(&prefix, 0)?;
            let variant = View::u16_le_at(&prefix, 2)?;
            (is_class_token(family) && variant != 0).then_some(prefix)
        })();
        let Some(prefix) = prefix else {
            continue;
        };
        scratch.with_storage(|| ctx.insert_hash_set(&mut prefixes, prefix, OPERATION))?;
    }

    let mut result = Vec::<SurfaceIdentityFields>::new();
    let Some(last) = lane.native_payload.len().checked_sub(16) else {
        return Ok(Vec::new());
    };
    'terminals: for terminal in ctx.admit_iter(0..=last, OPERATION)? {
        let Some(window) = lane
            .native_payload
            .get(terminal..terminal + 16)
            .and_then(<[u8]>::first_chunk::<16>)
        else {
            continue;
        };
        let [p0, p1, p2, p3, .., t0, t1, t2, t3] = *window;
        let prefix = [p0, p1, p2, p3];
        if !ctx.contains_hash_set(&prefixes, &prefix, OPERATION)? {
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
        let mut predecessors = std::iter::successors(Some(terminal), |offset| offset.checked_sub(16));
        while let Some(current) = ctx.next_charged(&mut predecessors, OPERATION)? {
            if instance_before(current).is_none() {
                break;
            }
            let Some(previous) = current.checked_sub(16) else {
                break;
            };
            if lane
                .native_payload
                .get(previous..)
                .and_then(|bytes| signature_prefix(bytes, prefix))
                .is_none()
            {
                break;
            }
            offset = previous;
        }
        let Some(mut parsed_components) =
            inline_surface_components_at(&lane.native_payload, offset)
        else {
            continue;
        };
        let mut component_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut components = Vec::new();
        while let Some(component) = ctx.next_charged(&mut parsed_components, OPERATION)? {
            let Some(component) = component else {
                continue 'terminals;
            };
            component_storage.with_storage(|| ctx.push_vec(&mut components, component, OPERATION))?;
        }
        let (key, key_storage) = ctx.with_scoped_storage(OPERATION, || ctx.collect_vec(
            components.iter().map(|component| (component.instance, component.type_signature, component.local_id)), OPERATION))?;
        if !scratch.with_storage(|| ctx.insert_hash_set(&mut seen, (prefix, key), OPERATION))? { continue; }
        scratch.with_storage(|| key_storage.commit())?;
        component_storage.commit()?;
        let local_identity = cadmpeg_core::bytes::assemble_u32_le(tail);
        scratch.with_storage(|| ctx.reserve_vec(&mut result, 1, OPERATION))?;
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
    ctx.sort_unstable_by_key(
        &mut result,
        |value| (value.offset, value.input_index),
        Ord::cmp,
        "sort SLDPRT generated surface identities",
    )?;
    let lane_key = ctx
        .rsplit_once(&lane.id, "#", "split SLDPRT feature-input lane key")?
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut identities = Vec::new();
    ctx.reserve_vec(&mut identities, result.len(), OPERATION)?;
    for (ordinal, fields) in ctx.admit_iter(result, OPERATION)?.enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::from(u32::MAX), u64_from_index(ordinal))
        })?;
        let id = ctx.format_retained(
            format_args!(
                "sldprt:feature-input:generated-surface#{lane_key}:{}",
                fields.offset
            ),
            OPERATION,
        )?;
        let parent = ctx.format_retained(format_args!("{}", lane.id), OPERATION)?;
        identities.push(crate::records::FeatureInputGeneratedSurfaceIdentity {
            id,
            parent,
            ordinal,
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    base: usize,
) -> Result<Option<(usize, Vec<u32>)>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge selection vector";
    let Some(last_marker) = payload.len().checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) else {
        return Ok(None);
    };
    let mut offsets = 12..=last_marker;
    while let Some(marker) = ctx.next_charged(&mut offsets, OPERATION)? {
        if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) {
            continue;
        }
        if let Some(ids) = compact_edge_selection_at(ctx, payload, marker)? {
            let offset = base
                .checked_add(marker)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            return Ok(Some((offset, ids)));
        }
    }
    Ok(None)
}

pub(crate) fn compact_edge_selection_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact edge identities";
    let count = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
            || payload.get(marker + 16..marker + 18)? != [0, 0]
        {
            return None;
        }
        usize::try_from(View::u32_le_at(payload, count_start)?)
            .ok()
            .filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    let (references, _references_storage) = ctx.with_scoped_storage(OPERATION, || compact_component_reference_list_at(ctx, payload, marker))?;
    if let Some(references) = references {
        let mut ids = Vec::new();
        for reference in ctx.admit_iter(references, OPERATION)? {
            if let Some(id) = reference.last().and_then(|entry| entry.local_id) {
                ctx.push_vec(&mut ids, id, OPERATION)?;
            }
        }
        return Ok(Some(ids));
    }
    let mut candidate = None;
    let mut consider = |ids: Vec<u32>, storage| {
        if let Some((candidate, _)) = &candidate {
            ctx.equal(candidate, &ids, OPERATION)
        } else { candidate = Some((ids, storage)); Ok(true) }
    };
    let (homogeneous, homogeneous_storage) = ctx.with_scoped_storage(OPERATION, || compact_homogeneous_edge_ids(ctx, payload, marker + 18, count))?;
    if let Some(ids) = homogeneous {
        if !consider(ids, homogeneous_storage)? { return Ok(None); }
    }
    let (paths, _paths_storage) = compact_edge_component_path_candidates(ctx, payload, marker, count)?;
    for (ComponentPathReference(components, _), _components_storage) in ctx.admit_iter(paths, OPERATION)? {
        let (ids, ids_storage) = ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
            let mut ids = Vec::new();
            for component in ctx.admit_iter(components, OPERATION)? {
                if let Some(id) = component.local_id { ctx.push_vec(&mut ids, id, OPERATION)?; }
            }
            Ok(ids)
        })?;
        if !ids.is_empty() && !consider(ids, ids_storage)? { return Ok(None); }
    }
    let (short, short_storage) = ctx.with_scoped_storage(OPERATION, || compact_u16_edge_ids(ctx, payload, marker + 18, count))?;
    if let Some(ids) = short {
        if !consider(ids, short_storage)? { return Ok(None); }
    }
    match candidate {
        Some((ids, storage)) => { storage.commit()?; Ok(Some(ids)) }
        None => Ok(None),
    }
}

pub(crate) fn compact_edge_component_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT edge component projection";
    let count = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
            || payload.get(marker + 16..marker + 18)? != [0, 0]
        {
            return None;
        }
        usize::try_from(View::u32_le_at(payload, count_start)?)
            .ok()
            .filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    let (references, _references_storage) = ctx.with_scoped_storage(OPERATION, || compact_component_reference_list_at(ctx, payload, marker))?;
    if let Some(references) = references {
        let mut components = Vec::new();
        for reference in ctx.admit_iter(references, OPERATION)? {
            if ctx.any_by(&reference, |component| Ok(component.instance == Some(0x8083)), OPERATION)? {
                continue;
            }
            ctx.extend_vec(&mut components, reference, OPERATION)?;
        }
        return Ok(Some(components));
    }
    Ok(compact_edge_component_path(ctx, payload, marker, count)?
        .map(|ComponentPathReference(components, _)| components))
}

fn compact_component_reference_list_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<Vec<FeatureInputComponentPathEntry>>>, CodecError> {
    compact_component_reference_list(ctx, payload, marker, true)
}

fn compact_component_reference_list(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    require_distinct_framing: bool,
) -> Result<Option<Vec<Vec<FeatureInputComponentPathEntry>>>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT component reference list", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT component reference list";
    let header = (|| {
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
        Some((count, signature_prefix))
    })();
    let Some((count, signature_prefix)) = header else {
        return Ok(None);
    };
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
    let mut references = Vec::new();
    ctx.reserve_capacity(&mut references, count, OPERATION)?;
    let mut has_reference_framing = false;
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
        let mut reference = Vec::new();
        let mut offsets = (cursor..payload.len()).step_by(16);
        while let Some(offset) = ctx.next_charged(&mut offsets, OPERATION)? {
            let Some(hop) = hop_at(offset) else { break; };
            ctx.push_vec(&mut reference, hop, OPERATION)?;
            cursor = offset + 16;
        }
        if reference.is_empty() && index + 1 == count && terminal_null_at(cursor) {
            return Ok((!references.is_empty()).then_some(references));
        }
        if reference.is_empty() {
            return Ok((!require_distinct_framing && !references.is_empty()).then_some(references));
        }
        has_reference_framing |= reference.len() > 1;
        let Some(last) = reference.last_mut() else {
            return Ok(None);
        };
        let Some(local_id) = View::u32_le_at(payload, cursor) else {
            return Ok(None);
        };
        last.local_id = Some(local_id);
        cursor += 4;
        if payload.get(cursor..cursor + 4) == Some(&[0xff; 4]) {
            cursor += 4;
            has_reference_framing = true;
        }
        ctx.push_vec(&mut references, reference, OPERATION)?;
        if index + 2 == count && terminal_null_at(cursor) {
            return Ok(Some(references));
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
            return Ok((!require_distinct_framing && !references.is_empty()).then_some(references));
        };
        cursor += gap;
    }
    Ok((!require_distinct_framing || has_reference_framing).then_some(references))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

#[derive(Debug)]
pub(super) struct VariableFilletControl(pub String, pub Vec<Vec<FeatureInputComponentPathEntry>>);

pub(super) fn variable_fillet_control_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<Vec<VariableFilletControl>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT variable fillet controls";
    if !feature.kind.eq_ignore_ascii_case("VarFillet") {
        return Ok(None);
    }
    let classes = fillet_dimension_classes(ctx, lane)?;
    let Some(control_start) =
        fillet_edge_roster_end(ctx, lane, &classes, object_start, object_end)?
    else {
        return Ok(None);
    };
    let (Some(marker_start), Some(marker_end)) = (
        control_start.checked_add(12),
        object_end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
    ) else {
        return Ok(None);
    };
    let mut controls_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut controls = Vec::new();
    for marker in ctx.admit_iter(marker_start..marker_end, OPERATION)? {
        if lane
            .native_payload
            .get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        {
            continue;
        }
        let (references, references_storage) = ctx.with_scoped_storage(OPERATION, || compact_component_reference_list(ctx, &lane.native_payload, marker, false))?;
        let Some(references) = references else {
            continue;
        };
        if references.len() == 3 {
            controls_storage.with_storage(|| ctx.push_vec(&mut controls, (marker, references, references_storage), OPERATION))?;
        }
    }
    ctx.sort_unstable_by(
        &mut controls,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT variable fillet controls",
    )?;
    let mut names_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut names = names_storage.with_storage(|| ctx.collect_vec(&lane.names, OPERATION))?;
    ctx.sort_unstable_by_key(&mut names, |name| name.offset, Ord::cmp, OPERATION)?;
    let mut result_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut result = Vec::new();
    let mut start = control_start;
    for (marker, references, references_storage) in ctx.admit_iter(controls, OPERATION)? {
        let first = ctx.partition_point(&names, |name| Ok(name.offset <= u64_from_index(start)), OPERATION)?;
        let end = ctx.partition_point(&names, |name| Ok(name.offset < u64_from_index(marker)), OPERATION)?;
        let mut name = None;
        let ambiguous = ctx.any_by(&names[first..end], |candidate| {
            if variable_fillet_dimension_index_for_feature(ctx, feature, &candidate.value)?.is_none() { return Ok(false); }
            Ok(name.replace(*candidate).is_some())
        }, OPERATION)?;
        let Some(name) = name.filter(|_| !ambiguous) else { return Ok(None); };
        result_storage.with_storage(|| -> Result<_, CodecError> {
            let name_text = ctx.copy_retained_text(&name.value, OPERATION)?;
            ctx.push_vec(&mut result, VariableFilletControl(name_text, references), OPERATION)?;
            references_storage.commit()?;
            Ok(())
        })?;
        start = marker;
    }

    if result.is_empty() { return Ok(None); }
    result_storage.commit()?;
    Ok(Some(result))
}

pub(crate) fn variable_fillet_dimension_index_for_feature(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    name: &str,
) -> Result<Option<usize>, CodecError> {
    if name == "D1" && !ctx.contains_key_btree_map(&feature.parameters, "D01", "resolve SLDPRT variable fillet dimension name")? {
        // SW2013-era lanes use D1 for the second variable-radius control.
        return Ok(Some(1));
    }
    let Some(suffix) = ctx.strip_prefix(name, "D0", "resolve SLDPRT variable fillet dimension name")? else {
        return Ok(None);
    };
    if suffix.is_empty() {
        return Ok(Some(0));
    }
    if suffix.starts_with('0') || !ctx.all_by(suffix.bytes(), |byte| Ok(byte.is_ascii_digit()), "parse SLDPRT variable fillet dimension index")? {
        return Ok(None);
    }
    Ok(ctx
        .parse_text(suffix, "parse SLDPRT variable fillet dimension index")?
        .ok())
}

pub(super) fn compact_component_path_end_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<usize>, CodecError> {
    const OPERATION: &str = "decode SLDPRT component path end";
    let count = (|| {
        let count_start = marker.checked_sub(12)?;
        let kind_start = marker.checked_sub(8)?;
        if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
            || payload.get(kind_start + 1..kind_start + 4)? != [0x02, 0x00, 0x00]
            || payload.get(marker + 16..marker + 18)? != [0, 0]
        {
            return None;
        }
        usize::try_from(View::u32_le_at(payload, count_start)?)
            .ok()
            .filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    let (result, _storage) = ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
        let mut candidate = None;
        for kind in 0..3 {
            let path = match kind {
                0 => compact_wide_component_path(ctx, payload, marker + 18, count, "decode SLDPRT component path layout")?,
                1 => compact_heterogeneous_component_path(ctx, payload, marker + 18, count, "decode SLDPRT component path layout")?,
                _ => compact_sparse_component_path(ctx, payload, marker + 18, count)?,
            };
            let Some(path) = path else { continue; };
            if let Some(first) = &candidate {
                if !ctx.equal(first, &path, OPERATION)? { return Ok(None); }
            } else { candidate = Some(path); }
        }
        Ok(candidate.map(|(_, end)| end))
    })?;
    Ok(result)

}

#[derive(Debug, PartialEq)]
pub(super) struct ComponentPathReference(pub Vec<FeatureInputComponentPathEntry>, pub Option<u32>);

fn compact_edge_component_path(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    count: usize,
) -> Result<Option<ComponentPathReference>, CodecError> {
    let (candidates, _container_storage) = compact_edge_component_path_candidates(ctx, payload, marker, count)?;
    if candidates.len() != 1 { return Ok(None); }
    let Some((candidate, storage)) = candidates.into_iter().next() else { return Ok(None); };
    storage.commit()?;
    Ok(Some(candidate))
}

fn compact_edge_component_path_candidates<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    count: usize,
) -> Result<(Vec<(ComponentPathReference, ScopedReservation<'ctx>)>, ScopedReservation<'ctx>), CodecError> {
    const OPERATION: &str = "decode SLDPRT edge path candidates";
    let component_paths = |entry_count| -> Result<_, CodecError> {
        let mut distinct = Vec::new();
        let mut container_storage = ctx.reserve_scoped(0, OPERATION)?;
        for kind in 0..3 {
            let (candidate, storage) = ctx.with_scoped_storage(OPERATION, || match kind {
                0 => compact_wide_component_path(ctx, payload, marker + 18, entry_count, "decode SLDPRT component path layout"),
                1 => compact_heterogeneous_component_path(ctx, payload, marker + 18, entry_count, "decode SLDPRT component path layout"),
                _ => compact_sparse_component_path(ctx, payload, marker + 18, entry_count),
            })?;
            let Some((components, end)) = candidate else { continue; };
            if !ctx.any_by(&distinct, |(previous, previous_end, _)| ctx.equal(&(previous, *previous_end), &(&components, end), OPERATION), OPERATION)? {
                container_storage.with_storage(|| ctx.push_vec(&mut distinct, (components, end, storage), OPERATION))?;
            }
        }
        Ok((distinct, container_storage))
    };
    let mut intermediate_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut terminal_paths = Vec::new();
    if count > 1 {
        let (paths, _paths_storage) = component_paths(count - 1)?;
        for (components, end, storage) in ctx.admit_iter(paths, OPERATION)? {
            if let Some(source) = edge_terminal_source_at(payload, end) {
                intermediate_storage.with_storage(|| ctx.push_vec(&mut terminal_paths, (components, end, source, storage), OPERATION))?;
            }
        }
    }
    let (mut full_paths, _full_storage) = component_paths(count)?;
    ctx.retain_vec(&mut full_paths, |(components, end, _)| {
        Ok(!ctx.any_by(&terminal_paths, |(prefix, prefix_end, _, _)| {
            Ok(*prefix_end < *end && components.len() >= prefix.len()
                && ctx.equal(&components[..prefix.len()], prefix.as_slice(), OPERATION)?)
        }, OPERATION)?)
    }, OPERATION)?;
    let mut candidates = Vec::new();
    let mut candidates_storage = ctx.reserve_scoped(0, OPERATION)?;
    for (candidate, storage) in ctx.admit_iter(terminal_paths, OPERATION)?
        .map(|(components, _, source, storage)| (ComponentPathReference(components, Some(source)), storage))
        .chain(ctx.admit_iter(full_paths, OPERATION)?.map(|(components, _, storage)| (ComponentPathReference(components, None), storage)))
    {
        if !ctx.any_by(&candidates, |(existing, _): &(ComponentPathReference, ScopedReservation<'_>)| {
            ctx.equal(&(&existing.0, existing.1), &(&candidate.0, candidate.1), OPERATION)
        }, OPERATION)? {
            candidates_storage.with_storage(|| ctx.push_vec(&mut candidates, (candidate, storage), OPERATION))?;
        }
    }
    Ok((candidates, candidates_storage))
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

pub(crate) fn compact_edge_owner_feature_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT compact edge owner";
    let count = marker
        .checked_sub(12)
        .and_then(|offset| View::u32_le_at(payload, offset))
        .and_then(|count| usize::try_from(count).ok());
    let Some(count) = count else {
        return Ok(None);
    };
    let (reference_list, _reference_storage) = ctx.with_scoped_storage(OPERATION, || compact_component_reference_list_at(ctx, payload, marker))?;
    let owner_source = if reference_list.is_some() {
        None
    } else {
        let (path, _path_storage) = ctx.with_scoped_storage(OPERATION, || compact_edge_component_path(ctx, payload, marker, count))?;
        let Some(ComponentPathReference(_, owner)) = path else { return Ok(None); };
        owner
    };
    if let Some(source) = owner_source {
        if let Some(feature) = ctx.find_by(features, |feature| Ok(feature.source_value() == Some(source)), OPERATION)? {
            if feature_precedes_consumer(ctx, feature, features, consumer_ref)? {
                return Ok(Some(ctx.copy_retained_text(&feature.id, OPERATION)?));
            }
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
    if let Some(owner) =
        compact_edge_owner_feature_at(ctx, payload, marker, components, features, consumer_ref)?
    {
        if !ctx.contains(&producers, &owner, OPERATION)? { ctx.push_vec(&mut producers, owner, OPERATION)?; }

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
    if let Some(source) = compact_single_face_reference_record_at(ctx, payload, marker)?
        .and_then(|ComponentPathReference(_, source)| source)
    {
        let mut found = None;
        if ctx.any_by(features, |feature| {
            if feature.source_value() != Some(source) { return Ok(false); }
            Ok(found.replace(feature).is_some())
        }, OPERATION)? { found = None; }
        if let Some(feature) = found {
            return Ok(Some(ctx.copy_retained_text(&feature.id, OPERATION)?));
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
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT homogeneous edge identities", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT homogeneous edge identities";
    let window = (|| {
        let signature = payload.get(cursor + 4..cursor + 16)?;
        bounded_len(
            u64_from_index(count),
            20,
            payload.len().checked_sub(cursor)?,
        )?;
        Some(signature)
    })();
    let Some(signature) = window else {
        return Ok(None);
    };
    let mut ids = Vec::new();
    ctx.reserve_capacity(&mut ids, count, OPERATION)?;

    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
        let Some(entry_signature) = payload.get(cursor + 4..cursor + 16) else {
            return Ok(None);
        };
        if entry_signature != signature {
            return Ok(None);
        }
        let Some(id) = View::u32_le_at(payload, cursor + 16) else {
            return Ok(None);
        };
        ctx.push_vec(&mut ids, id, OPERATION)?;
        cursor += 20;
        if index + 1 < count
            && match payload.get(cursor + 4..cursor + 16) {
                Some(bytes) => bytes,
                None => return Ok(None),
            } != signature
        {
            if match payload.get(cursor..cursor + 4) {
                Some(bytes) => bytes,
                None => return Ok(None),
            } == [0; 4]
                && match payload.get(cursor + 8..cursor + 20) {
                    Some(bytes) => bytes,
                    None => return Ok(None),
                } == signature
            {
                cursor += 4;
            } else {
                let Some(separator) = payload.get(cursor..cursor + 8) else {
                    return Ok(None);
                };
                match separator {
                    [0, 0, 0, 0, 0, 0, 0, 0] | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0] => {
                        cursor += 8;
                    }
                    _ => return Ok(None),
                }
            }
        }
    }
    Ok(Some(ids))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

pub(super) fn compact_heterogeneous_component_path(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    count: usize,
    reserve_operation: &'static str,
) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
    compact_component_path_with_layout(ctx, payload, cursor, count, false, reserve_operation)
}

fn compact_wide_component_path(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    count: usize,
    reserve_operation: &'static str,
) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
    compact_component_path_with_layout(ctx, payload, cursor, count, true, reserve_operation)
}

fn compact_component_entry_at(payload: &[u8], offset: usize, wide: bool) -> Option<()> {
    let entry_length = if wide { 24 } else { 20 };
    let local_id_offset = if wide { 20 } else { 16 };

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
}

fn compact_component_path_with_layout(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    wide: bool,
    reserve_operation: &'static str,
) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT component path layout", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT component path layout";
    let entry_length = if wide { 24 } else { 20 };
    let local_id_offset = if wide { 20 } else { 16 };

    if bounded_len(
        u64_from_index(count),
        entry_length,
        if cursor <= payload.len() {
            payload.len() - cursor
        } else {
            0
        },
    )
    .is_none()
    {
        return Ok(None);
    }

    let mut entries = Vec::new();
    ctx.reserve_capacity(&mut entries, count, reserve_operation)?;
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
        if compact_component_entry_at(payload, cursor, wide).is_none() {
            return Ok(None);
        }
        let Some(entry) = (|| {
            Some(FeatureInputComponentPathEntry {
                instance: Some(View::u16_le_at(payload, cursor)?),
                type_signature: payload.get(cursor + 4..cursor + 16)?.try_into().ok()?,
                local_id: Some(View::u32_le_at(payload, cursor + local_id_offset)?),
            })
        })() else {
            return Ok(None);
        };
        ctx.push_vec(&mut entries, entry, reserve_operation)?;
        cursor += entry_length;
        if index + 1 == count {
            continue;
        }
        let Some(gap) = COMPACT_COMPONENT_PATH_GAPS.iter().copied().find(|gap| {
            compact_component_separator(payload, cursor, *gap)
                && compact_component_entry_at(payload, cursor + *gap, wide).is_some()
        }) else {
            return Ok(None);
        };
        cursor += gap;
    }
    Ok(Some((entries, cursor)))

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT sparse component path", || -> Result<_, CodecError> {
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
        ctx: &DecodeContext<'_>,
        payload: &[u8],
        cursor: usize,
        remaining: usize,
        failed: &mut HashSet<(usize, usize)>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
        const OPERATION: &str = "decode SLDPRT sparse component path";
        let _depth = ctx.enter_nested(OPERATION)?;
        if ctx.contains_hash_set(failed, &(cursor, remaining), OPERATION)? {
            return Ok(None);
        }
        storage.with_storage(|| ctx.insert_hash_set(failed, (cursor, remaining), OPERATION))?;
        let Some((instance, type_signature)) = entry_prefix(payload, cursor) else {
            return Ok(None);
        };
        for entry_length in [20usize, 16] {
            let local_id = if entry_length == 20 {
                let Some(local_id) = View::u32_le_at(payload, cursor + 16) else {
                    return Ok(None);
                };
                Some(local_id)
            } else {
                None
            };
            let entry = FeatureInputComponentPathEntry {
                instance: Some(instance),
                type_signature,
                local_id,
            };
            let Some(end) = cursor.checked_add(entry_length) else {
                return Ok(None);
            };
            if remaining == 1 {
                let mut entries = Vec::new();
                ctx.reserve_vec(&mut entries, 1, OPERATION)?;
                entries.push(entry);
                return Ok(Some((entries, end)));
            }
            for gap in COMPACT_COMPONENT_PATH_GAPS {
                if !compact_component_separator(payload, end, *gap) {
                    continue;
                }
                let Some(next) = end.checked_add(*gap) else {
                    continue;
                };
                let Some((mut tail, path_end)) = parse(ctx, payload, next, remaining - 1, failed, storage)?
                else {
                    continue;
                };
                ctx.insert_vec(&mut tail, 0, entry, OPERATION)?;
                return Ok(Some((tail, path_end)));
            }
        }
        Ok(None)
    }

    if bounded_len(
        u64_from_index(count),
        16,
        if cursor <= payload.len() {
            payload.len() - cursor
        } else {
            0
        },
    )
    .is_none()
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "decode SLDPRT sparse component path")?;
    let mut failed = HashSet::new();
    // The recursive path survives; only the memo table is scratch storage.
    parse(ctx, payload, cursor, count, &mut failed, &mut storage)

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

fn compact_u16_edge_ids(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    let (result, storage) = ctx.with_scoped_storage("decode SLDPRT short edge identities", || -> Result<_, CodecError> {
    const OPERATION: &str = "decode SLDPRT short edge identities";
    let end = count.checked_mul(2).and_then(|len| cursor.checked_add(len));
    let Some(end) = end else {
        return Ok(None);
    };
    let Some(bytes) = payload.get(cursor..end) else {
        return Ok(None);
    };
    let Some(suffix) = payload.get(end..) else {
        return Ok(None);
    };
    let mut ids = Vec::new();
    ctx.reserve_vec(&mut ids, count, OPERATION)?;

    let mut view = View::over_retained(bytes);
    for _ in ctx.admit_iter(0..count, OPERATION)? {
        let Some(id) = view.u16_le() else {
            return Ok(None);
        };
        ctx.push_vec(&mut ids, u32::from(id), OPERATION)?;
    }
    let sentinel_terminated = suffix.get(..19).is_some_and(|suffix| {
        suffix[..16].iter().all(|byte| *byte == 0) && suffix[16..19] == [0xff, 0xfe, 0xff]
    });
    let object_terminated = suffix.get(..10).is_some_and(|suffix| {
        suffix[..8].iter().all(|byte| *byte == 0)
            && View::u16_le_at(suffix, 8).is_some_and(is_class_token)
    });
    Ok(
        (ctx.all_by(&ids, |id| Ok(*id != 0), OPERATION)? && (sentinel_terminated || object_terminated))
            .then_some(ids),
    )

    })?;
    if result.is_some() { storage.commit()?; }
    Ok(result)
}

fn compact_body_selection_vector(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    base: usize,
    next_object_token: Option<u16>,
) -> Result<Option<(usize, Vec<u32>)>, CodecError> {
    const SCHEMA: &[u8] = &11000u32.to_le_bytes();
    const OPERATION: &str = "decode SLDPRT compact body selection vector";
    let Some(last) = payload.len().checked_sub(16) else {
        return Ok(None);
    };
    let mut offsets = (0..=last).rev();
    while let Some(relative) = ctx.next_charged(&mut offsets, OPERATION)? {
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
        let Some(ids) = payload.get(relative + 16..ids_end) else {
            continue;
        };
        let local_body_ids = read_compact_body_ids(ctx, ids, OPERATION)?;
        let Some(offset) = base.checked_add(relative) else {
            return Ok(None);
        };
        return Ok(Some((offset, local_body_ids)));
    }
    Ok(None)
}

pub(crate) fn compact_body_selection_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT compact body selection";
    let Some(header_end) = offset.checked_add(12) else {
        return Ok(None);
    };
    let Some(schema_end) = offset.checked_add(4) else {
        return Ok(None);
    };
    if payload.get(offset..schema_end) != Some(11000u32.to_le_bytes().as_slice())
        || payload.get(schema_end..header_end) != Some(&[0; 8])
    {
        if payload.get(offset..schema_end).is_none()
            || payload.get(schema_end..header_end).is_none()
        {
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
        {
            return None;
        }
        payload.get(ids_start..ids_end)
    })();
    match ids {
        Some(ids) => Ok(Some(read_compact_body_ids(ctx, ids, OPERATION)?)),
        None => Ok(None),
    }
}

pub(super) fn read_compact_body_ids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u32>, CodecError> {
    let mut result = Vec::new();
    ctx.reserve_capacity(&mut result, bytes.len() / 4, operation)?;
    let mut view = View::over_retained(bytes);
    for _ in ctx.admit_iter(0..bytes.len() / 4, operation)? {
        let Some(id) = view.u32_le() else { break; };
        ctx.push_vec(&mut result, id, operation)?;
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, CodecError> {
    const OPERATION: &str = "decode SLDPRT component reference curve path";
    let count = (|| {
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
        usize::try_from(View::u32_le_at(payload, marker.checked_sub(12)?)?)
            .ok()
            .filter(|count| (1..=64).contains(count))
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    let parse =
        |count: usize| -> Result<Option<(Vec<FeatureInputComponentPathEntry>, usize)>, CodecError> {
            let mut cursor = marker + 18;
            let signature = payload
                .get(cursor + 4..cursor + 16)
                .and_then(|bytes| <[u8; 12]>::try_from(bytes).ok());
            let Some(signature) = signature else {
                return Ok(None);
            };
            let mut components = Vec::new();
            ctx.reserve_vec(&mut components, count, OPERATION)?;
            let mut indices = 0..count;
            while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
                if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice()) {
                    return Ok(None);
                }
                let entry = (|| {
                    Some(FeatureInputComponentPathEntry {
                        instance: Some(View::u16_le_at(payload, cursor)?),
                        type_signature: signature,
                        local_id: Some(View::u32_le_at(payload, cursor + 16)?),
                    })
                })();
                let Some(entry) = entry else {
                    return Ok(None);
                };
                ctx.push_vec(&mut components, entry, OPERATION)?;
                cursor += 20;
                if index + 1 != count {
                    let gap_valid = |gap: usize| {
                        payload.get(cursor + gap + 4..cursor + gap + 16)
                            == Some(signature.as_slice())
                            && (gap == 0
                                || (payload.get(cursor..cursor + 2) != Some(&[0, 0])
                                    && payload.get(cursor + 2..cursor + 6) == Some(&[0; 4])))
                    };
                    let gap = match (gap_valid(0), gap_valid(6)) {
                        (true, false) => 0,
                        (false, true) => 6,
                        _ => return Ok(None),
                    };
                    cursor += gap;
                }
            }
            Ok(Some((components, cursor)))
        };
    let (exact, exact_storage) = ctx.with_scoped_storage(OPERATION, || parse(count))?;
    if let Some((components, _)) = exact {
        exact_storage.commit()?;
        return Ok(Some(components));
    }
    drop(exact_storage);
    if count <= 1 {
        return Ok(None);
    }
    let (shorter, shorter_storage) = ctx.with_scoped_storage(OPERATION, || parse(count - 1))?;
    let Some((components, end)) = shorter else { return Ok(None); };
    if payload.get(end..end + 12) != Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0xf8, 0x2a, 0, 0]) { return Ok(None); }
    shorter_storage.commit()?;
    Ok(Some(components))
}

pub(super) fn unique_marker_candidate<'a>(ctx: &DecodeContext<'_>, candidates: &'a [(String, bool)]) -> Result<Option<&'a str>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT scalar marker candidate";
    let mut coordinate = None;
    if ctx.any_by(candidates, |(id, located)| {
        if !located { return Ok(false); }
        Ok(coordinate.replace(id.as_str()).is_some())
    }, OPERATION)? { return Ok(None); }
    Ok(coordinate.or_else(|| match candidates { [(id, _)] => Some(id.as_str()), _ => None }))
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<(Vec<u16>, u16)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "decode SLDPRT coordinate marker links";
    let legacy_geometry_linked_point = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(1)
        && marker_is_geometry_locus(payload, offset);
    let parsed = if legacy_geometry_linked_point {
        let Some((_, links)) = linked_profile_point(payload, offset) else {
            return Ok(None);
        };
        let Some(selector) = links.first().map(|link| link.0) else {
            return Ok(None);
        };
        Some(([links[0].1, links[1].1], 2, selector))
    } else {
        if marker_coordinates(payload, offset).is_none()
            && !counted_legacy_profile_line_layout(payload, offset)
        {
            return Ok(None);
        }
        (|| {
            let mut links = [0; 2];
            let mut selector = None;
            for index in 0..=2 {
                let start = offset.checked_add(86 + index * 12)?;
                if payload.get(start..start + 6)? == [0, 0, 0xfe, 0xff, 0xff, 0xff] {
                    return (index != 0).then_some((links, index, selector?));
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
                links[index] = View::u16_le_at(cell, 2)?;
            }
            None
        })()
    };
    let Some((local_ids, count, selector)) = parsed else {
        return Ok(None);
    };
    let mut links = Vec::new();
    ctx.extend_from_slice(&mut links, &local_ids[..count], OPERATION)?;
    Ok(Some((links, selector)))
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
    let count = u32::try_from(entries.len()).expect("selection vector entry count fits u32");
    payload.extend_from_slice(&count.to_le_bytes());
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
