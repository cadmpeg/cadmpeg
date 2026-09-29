//! Draft-operation plane and face operands.

use super::is_class_token;
use super::scalars::feature_object_name;
use super::selections::{
    compact_mixed_component_path, component_vector_path_at, is_component_vector_selector,
    COMPACT_EDGE_VECTOR_MARKER,
};
use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureDirection3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;

use crate::layout::draft_aligned_direction_frame as aligned_dir;
use crate::layout::draft_compact_selection_prefix as compact_sel;
use crate::layout::draft_extended_direction_frame as extended_dir;
use crate::layout::draft_plane_reference_prefix as draft_plane;

const EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12: f64 = 1.0e-12;

const DIRECTION_FRAME_PREFIX_LEN: usize = 24;
const MAX_PATH_CELLS: usize = 65;

#[derive(Clone, Debug)]
pub(super) struct DraftOperands {
    pub(super) anchor: DraftAnchor,
    pub(super) faces: Vec<Vec<FeatureInputComponentPathEntry>>,
    pub(super) pull_direction: FeatureDirection3,
}

#[derive(Clone, Debug)]
pub(super) enum DraftAnchor {
    NeutralPlane(Vec<FeatureInputComponentPathEntry>),
    PartingTool(Vec<Vec<FeatureInputComponentPathEntry>>),
}

pub(super) fn same_draft_operands(left: &DraftOperands, right: &DraftOperands) -> bool {
    let left_direction = left.pull_direction.get();
    let right_direction = right.pull_direction.get();
    same_draft_anchor(&left.anchor, &right.anchor)
        && left.faces.len() == right.faces.len()
        && left
            .faces
            .iter()
            .zip(&right.faces)
            .all(|(left, right)| same_component_path_semantics(left, right))
        && (left_direction.x - right_direction.x).abs() <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
        && (left_direction.y - right_direction.y).abs() <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
        && (left_direction.z - right_direction.z).abs() <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
}

fn draft_operands(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<DraftOperands>, CodecError> {
    if classify(feature) != Some(FeatureClass::Draft) || object_start >= object_end {
        return Ok(None);
    }
    if let Some(operands) = declared_draft_operands(ctx, lane, object_start, object_end)? {
        return Ok(Some(operands));
    }
    compact_parting_line_draft_operands(ctx, lane, object_start, object_end)
}

fn declared_draft_operands(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<DraftOperands>, CodecError> {
    const OPERATION: &str = "collect SLDPRT declared draft references";
    let Some(token) = unique_declared_plane_reference_token(lane) else {
        return Ok(None);
    };
    let Some(end) = super::DeclaredEnd::of(object_end, lane.native_payload.len()) else {
        return Ok(None);
    };
    let end = end.get();
    let Some(final_record_start) = end.checked_sub(draft_plane::LEN) else {
        return Ok(None);
    };
    let mut records = Vec::new();
    for offset in object_start..=final_record_start {
        ctx.charge_work(1, "scan SLDPRT declared draft references")?;
        if lane.native_payload.get(offset..offset + 2) != Some(token.as_slice()) {
            continue;
        }
        if let Some(record) = draft_plane_reference_at(&lane.native_payload, offset, end) {
            ctx.reserve_collection_vec(&mut records, 1, OPERATION)?;
            records.push(record);
        }
    }
    let mut records = records.into_iter();
    let Some((_, neutral_plane, neutral_end)) = records.next() else {
        return Ok(None);
    };
    let Some(pull_direction) = unique_draft_direction(
        &lane.native_payload,
        neutral_end,
        records.as_slice().first().map_or(end, |record| record.0),
    ) else {
        return Ok(None);
    };
    let mut faces = Vec::<Vec<FeatureInputComponentPathEntry>>::new();
    for path in records.map(|(_, path, _)| path) {
        if !faces
            .iter()
            .any(|existing| same_component_path_semantics(existing, &path))
        {
            ctx.reserve_collection_vec(&mut faces, 1, "collect SLDPRT declared draft faces")?;
            faces.push(path);
        }
    }
    Ok((!faces.is_empty()).then_some(DraftOperands {
        anchor: DraftAnchor::NeutralPlane(neutral_plane),
        faces,
        pull_direction,
    }))
}

fn compact_parting_line_draft_operands(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<DraftOperands>, CodecError> {
    const OPERATION: &str = "collect SLDPRT compact draft records";
    let Some(end) = super::DeclaredEnd::of(object_end, lane.native_payload.len()) else {
        return Ok(None);
    };
    let end = end.get();
    let Some(final_marker) = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()) else {
        return Ok(None);
    };
    let Some(first_marker) = object_start.checked_add(12) else {
        return Ok(None);
    };
    let mut records = Vec::new();
    for marker in first_marker..=final_marker {
        ctx.charge_work(1, "scan SLDPRT compact draft records")?;
        if lane
            .native_payload
            .get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
            != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        {
            continue;
        }
        if let Some((role, paths, selection_end)) =
            compact_draft_selection_at(ctx, &lane.native_payload, marker)?
        {
            ctx.reserve_collection_vec(&mut records, 1, OPERATION)?;
            records.push((marker, role, paths, selection_end));
        }
    }
    let mut parting_records = records
        .iter()
        .enumerate()
        .filter(|(_, (_, role, _, _))| *role == CompactDraftSelectionRole::PartingTool);
    let Some((parting_index, parting_record)) = parting_records.next() else {
        return Ok(None);
    };
    if parting_records.next().is_some() {
        return Ok(None);
    }
    let Some(first_face) = records.iter().find(|(marker, role, _, _)| {
        *role == CompactDraftSelectionRole::DraftedFace && *marker > parting_record.0
    }) else {
        return Ok(None);
    };
    let Some(pull_direction) =
        unique_draft_direction(&lane.native_payload, parting_record.3, first_face.0)
    else {
        return Ok(None);
    };
    let parting_start = parting_record.0;
    let mut parting_paths = None;
    let mut faces = Vec::<Vec<FeatureInputComponentPathEntry>>::new();
    for (index, (marker, role, paths, _)) in records.into_iter().enumerate() {
        if index == parting_index {
            parting_paths = Some(paths);
            continue;
        }
        if role != CompactDraftSelectionRole::DraftedFace || marker <= parting_start {
            continue;
        }
        for path in paths {
            if !faces
                .iter()
                .any(|existing| same_component_path_semantics(existing, &path))
            {
                ctx.reserve_collection_vec(&mut faces, 1, "collect SLDPRT compact draft faces")?;
                faces.push(path);
            }
        }
    }
    let Some(parting_paths) = parting_paths else {
        return Ok(None);
    };
    Ok((!faces.is_empty()).then_some(DraftOperands {
        anchor: DraftAnchor::PartingTool(parting_paths),
        faces,
        pull_direction,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompactDraftSelectionRole {
    PartingTool,
    DraftedFace,
}

fn compact_draft_selection_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<(
    CompactDraftSelectionRole,
    Vec<Vec<FeatureInputComponentPathEntry>>,
    usize,
)>, CodecError> {
    let Some(header) = marker.checked_sub(compact_sel::COMPONENT_MARKER) else {
        return Ok(None);
    };
    let Some(cell_count) = View::u32_le_at(payload, header + compact_sel::CELL_FIELD) else {
        return Ok(None);
    };
    let Some(_cell_count) = usize::try_from(cell_count)
        .ok()
        .filter(|count| (1..=MAX_PATH_CELLS).contains(count))
    else {
        return Ok(None);
    };
    let Some(role_bytes) =
        payload.get(header + compact_sel::SELECTION_ROLE..header + compact_sel::SELECTOR)
    else {
        return Ok(None);
    };
    let role = match role_bytes {
        [_, 2, 0, 0] => CompactDraftSelectionRole::PartingTool,
        [_, 3, 0, 0] => CompactDraftSelectionRole::DraftedFace,
        _ => return Ok(None),
    };
    if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())
        != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || payload.get(marker + COMPACT_EDGE_VECTOR_MARKER.len()..header + compact_sel::LEN)
            != Some(&[0, 0])
    {
        return Ok(None);
    }
    let mut cursor = header + compact_sel::LEN;
    let mut paths = Vec::new();
    let candidate_trials = u64::try_from(MAX_PATH_CELLS).map_err(|_| {
        ctx.refuse_codec_limit("scan SLDPRT compact draft path lengths", u64::MAX - 1, u64::MAX)
    })?;
    loop {
        ctx.charge_work(candidate_trials, "scan SLDPRT compact draft path lengths")?;
        let candidate = (1..=MAX_PATH_CELLS)
            .filter_map(|length| compact_mixed_component_path(payload, cursor, length, false))
            .filter(|(_, path_end)| {
                payload.get(*path_end..path_end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
            })
            .min_by_key(|(_, path_end)| *path_end);
        let Some((path, path_end)) = candidate else {
            return Ok((!paths.is_empty()).then_some((role, paths, cursor)));
        };
        ctx.reserve_collection_vec(&mut paths, 1, "collect SLDPRT compact draft paths")?;
        paths.push(path);
        cursor = path_end + 8;
    }
}

fn same_draft_anchor(left: &DraftAnchor, right: &DraftAnchor) -> bool {
    match (left, right) {
        (DraftAnchor::NeutralPlane(left), DraftAnchor::NeutralPlane(right)) => {
            same_component_path_semantics(left, right)
        }
        (DraftAnchor::PartingTool(left), DraftAnchor::PartingTool(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_component_path_semantics(left, right))
        }
        _ => false,
    }
}

fn same_component_path_semantics(
    left: &[FeatureInputComponentPathEntry],
    right: &[FeatureInputComponentPathEntry],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.type_signature[4..8] == right.type_signature[4..8]
                && left.local_id == right.local_id
        })
}

fn unique_declared_plane_reference_token(lane: &FeatureInputLane) -> Option<[u8; 2]> {
    let mut tokens = lane.classes.iter().filter_map(|class| {
        if class.name != "moPlaneRef_w" {
            return None;
        }
        let body = usize::try_from(class.offset)
            .ok()?
            .checked_add(6 + class.name.len())?;
        let value = View::u16_le_at(&lane.native_payload, body)?;
        is_class_token(value).then_some(value.to_le_bytes())
    });
    let first = tokens.next()?;
    tokens.all(|token| token == first).then_some(first)
}

fn draft_plane_reference_at(
    payload: &[u8],
    offset: usize,
    object_end: usize,
) -> Option<(usize, Vec<FeatureInputComponentPathEntry>, usize)> {
    let header = payload.get(offset..offset.checked_add(draft_plane::COMPONENT_MARKER)?)?;
    if offset + draft_plane::COMPONENT_MARKER > object_end
        || !View::u16_le_at(header, draft_plane::CHILD_TOKEN).is_some_and(is_class_token)
        || header[draft_plane::FORM..draft_plane::WRAPPER_FLAGS] != 2u32.to_le_bytes()
        || !matches!(
            &header[draft_plane::WRAPPER_FLAGS..draft_plane::IDENTITY],
            [0 | 0x40, 0, 0]
        )
        || header[draft_plane::IDENTITY..draft_plane::IDENTITY_COPY] == [0; 4]
        || header[draft_plane::IDENTITY..draft_plane::IDENTITY_COPY]
            != header[draft_plane::IDENTITY_COPY..draft_plane::IDENTITY_COPY + 4]
        || header[19..47] != [0; 28]
        || header[draft_plane::SENTINEL..draft_plane::SENTINEL + 16] != [0xff; 16]
        || header[63..72] != [0; 9]
        || !View::u16_le_at(header, draft_plane::INSTANCE_TOKEN).is_some_and(is_class_token)
        || header[draft_plane::ZERO_AT_78..draft_plane::CELL_COUNT] != [0; 4]
        || !is_component_vector_selector(&header[draft_plane::PATH_KIND..draft_plane::SELECTOR])
    {
        return None;
    }
    usize::try_from(View::u32_le_at(header, draft_plane::CELL_COUNT)?)
        .ok()
        .filter(|count| (2..=MAX_PATH_CELLS).contains(count))?;
    let marker = offset + draft_plane::COMPONENT_MARKER;
    if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker + COMPACT_EDGE_VECTOR_MARKER.len()..offset + draft_plane::LEN)?
            != [0, 0]
    {
        return None;
    }
    let components = component_vector_path_at(payload, marker)?;
    let path_start = offset.checked_add(draft_plane::LEN)?;
    (path_start <= object_end).then_some((offset, components, path_start))
}

fn unique_draft_direction(payload: &[u8], start: usize, end: usize) -> Option<FeatureDirection3> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let final_frame_start = end
        .checked_sub(aligned_dir::LEN)
        .filter(|end| *end >= start)?;
    let mut candidates = (start..=final_frame_start)
        .filter(|offset| payload.get(*offset..*offset + HANDLES.len()) == Some(HANDLES.as_slice()))
        .filter_map(|offset| {
            let frame = payload.get(offset..end)?;
            if frame[aligned_dir::ZERO_AT_8..aligned_dir::ADDRESS] != [0; 4]
                || frame[aligned_dir::ADDRESS..aligned_dir::ADDRESS + 4] == [0; 4]
                || frame[16..DIRECTION_FRAME_PREFIX_LEN] != [0; 8]
            {
                return None;
            }
            let scalar = |relative: usize| {
                let value = View::f64_le_at(frame, relative)?;
                value.is_finite().then_some(value)
            };
            if !(DIRECTION_FRAME_PREFIX_LEN..aligned_dir::LEN)
                .step_by(8)
                .all(|relative| scalar(relative).is_some())
            {
                return None;
            }
            let direction_at = |relative: usize| {
                let direction = Vector3::new(
                    View::f64_le_at(frame, relative)?,
                    View::f64_le_at(frame, relative + 8)?,
                    View::f64_le_at(frame, relative + 16)?,
                );
                UnitVector3::new(direction)
                    .map(FeatureDirection3::from_unit_without_small_components)
            };
            direction_at(aligned_dir::PULL_DIRECTION).or_else(|| {
                (frame.len() >= extended_dir::LEN
                    && frame[aligned_dir::LEN..extended_dir::PULL_DIRECTION] == [0; 9])
                    .then(|| direction_at(extended_dir::PULL_DIRECTION))
                    .flatten()
            })
        });
    let direction = candidates.next()?;
    if candidates.any(|candidate| candidate != direction) {
        return None;
    }
    Some(direction)
}

pub(super) fn draft_operand_candidates(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<(String, DraftOperands)>, CodecError> {
    const OPERATION: &str = "collect SLDPRT draft operand candidates";
    let mut objects = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        if let Some(name) = feature_object_name(feature, lane) {
            ctx.reserve_collection_vec(&mut objects, 1, OPERATION)?;
            objects.push((name.offset, feature));
        }
    }
    let count = u64::try_from(objects.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let comparisons_per_item = u64::from(objects.len().checked_ilog2().unwrap_or(0)) + 1;
    let sort_work = count
        .checked_mul(comparisons_per_item)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(sort_work, OPERATION)?;
    objects.sort_unstable_by_key(|(offset, _)| *offset);
    let mut candidates = Vec::new();
    for (index, (start, feature)) in objects.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let Ok(start) = usize::try_from(*start) else {
            continue;
        };
        let end = objects
            .get(index + 1)
            .and_then(|(offset, _)| usize::try_from(*offset).ok())
            .unwrap_or(lane.native_payload.len());
        if let Some(operands) = draft_operands(ctx, feature, lane, start, end)? {
            let mut id = String::new();
            ctx.reserve_retained_string(&mut id, feature.id.len(), OPERATION)?;
            id.push_str(&feature.id);
            ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
            candidates.push((id, operands));
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::super::selections::COMPACT_EDGE_VECTOR_MARKER;
    use super::{
        compact_draft_selection_at, draft_operands, draft_plane_reference_at,
        unique_draft_direction as typed_unique_draft_direction, DraftAnchor,
    };
    use crate::layout::draft_extended_direction_frame as extended_dir;
    use crate::records::FeatureInputLane;
    use crate::records::FeatureSource;
    use crate::records::ObjectId;
    use crate::records::{Feature, FeatureHistory, FeatureInputClass, FeatureInputName};
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::Vector3;
    use std::collections::BTreeMap;

    fn unique_draft_direction(payload: &[u8], start: usize, end: usize) -> Option<Vector3> {
        typed_unique_draft_direction(payload, start, end)
            .map(cadmpeg_ir::features::FeatureDirection3::get)
    }

    fn component(instance: u16, source: u32, identity: u32, local_id: u32) -> Vec<u8> {
        let mut bytes = instance.to_le_bytes().to_vec();
        bytes.extend([0, 0]);
        bytes.extend([0x2a, 0x80, 0x35, 0]);
        bytes.extend(source.to_le_bytes());
        bytes.extend(identity.to_le_bytes());
        bytes.extend(local_id.to_le_bytes());
        bytes
    }

    fn plane_reference(token: u16, flags: [u8; 2], source: u32, local_id: u32) -> Vec<u8> {
        let mut bytes = token.to_le_bytes().to_vec();
        bytes.extend(0x802fu16.to_le_bytes());
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(flags);
        bytes.push(0);
        bytes.extend(source.to_le_bytes());
        bytes.extend(source.to_le_bytes());
        bytes.extend([0; 28]);
        bytes.extend([0xff; 16]);
        bytes.extend([0; 9]);
        bytes.extend(0x8099u16.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(2u32.to_le_bytes());
        bytes.extend([0, 2, 0, 0]);
        bytes.extend(17u32.to_le_bytes());
        bytes.extend(COMPACT_EDGE_VECTOR_MARKER);
        bytes.extend([0, 0]);
        bytes.extend(component(0x8194, source, 91, local_id));
        bytes
    }

    fn draft_feature() -> Feature {
        Feature {
            id: "draft".into(),
            parent: "history".into(),
            xml_tag: "Draft".into(),
            tree_parent: None,
            source_id: FeatureSource::from_value(7),
            ordinal: 0,
            name: "Draft1".into(),
            kind: "Draft".into(),
            input_class: Some("moDraft_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    #[test]
    fn draft_operand_candidates_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![draft_feature()],
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: vec![0],
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "Draft1".into(),
                object_id: ObjectId::from_value(7),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
            .expect("test context");
        let error = super::draft_operand_candidates(&ctx, &[history], &lane)
            .expect_err("draft object index exceeds collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT draft operand candidates"));
    }

    #[test]
    fn draft_operand_candidates_refuses_declared_reference_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let token = 0x8096;
        let mut payload = vec![0; 64];
        let object_start = payload.len();
        payload.extend(plane_reference(token, [0x40, 0], 101, 3));
        payload.extend(plane_reference(token, [0x40, 0], 102, 8));
        let class_offset = payload.len();
        let class_name = "moPlaneRef_w";
        payload.extend([0; 6]);
        payload.extend(class_name.as_bytes());
        payload.extend(token.to_le_bytes());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![FeatureInputClass {
                id: "plane-ref".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: class_offset as u64,
                name: class_name.into(),
            }],
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: object_start as u64,
                value: "Draft1".into(),
                object_id: ObjectId::from_value(7),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![draft_feature()],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
            .expect("test context");
        let error = super::draft_operand_candidates(&ctx, &[history], &lane)
            .expect_err("declared draft references exceed collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT declared draft references"));
    }

    fn compact_selection(role: u8, paths: &[&[(u16, u32, u32, u32)]]) -> Vec<u8> {
        let mut bytes = 6u32.to_le_bytes().to_vec();
        bytes.extend([0, role, 0, 0]);
        bytes.extend(17u32.to_le_bytes());
        bytes.extend(COMPACT_EDGE_VECTOR_MARKER);
        bytes.extend([0, 0]);
        for path in paths {
            for (instance, source, identity, local_id) in *path {
                bytes.extend(component(*instance, *source, *identity, *local_id));
            }
            bytes.extend([0xff; 4]);
            bytes.extend([0; 4]);
        }
        bytes
    }

    #[test]
    fn compact_draft_selection_refuses_path_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut payload = vec![0; 64];
        let marker = payload.len() + 12;
        payload.extend(compact_selection(2, &[&[(0x8083, 80, 900, 1)]]));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("test context");
        let error = compact_draft_selection_at(&ctx, &payload, marker)
            .expect_err("compact draft path exceeds collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT compact draft paths"));
    }

    #[test]
    fn compact_draft_selection_refuses_path_scan_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut payload = vec![0; 64];
        let marker = payload.len() + 12;
        payload.extend(compact_selection(2, &[&[(0x8083, 80, 900, 1)]]));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("test context");
        let error = compact_draft_selection_at(&ctx, &payload, marker)
            .expect_err("compact draft path scan exceeds work limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan SLDPRT compact draft path lengths"));
    }

    #[test]
    fn draft_operand_candidates_refuses_compact_record_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut payload = vec![0; 64];
        let object_start = payload.len();
        payload.extend(compact_selection(2, &[&[(0x8083, 80, 900, 1)]]));
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: object_start as u64,
                value: "Draft1".into(),
                object_id: ObjectId::from_value(7),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![draft_feature()],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
            .expect("test context");
        let error = super::draft_operand_candidates(&ctx, &[history], &lane)
            .expect_err("compact draft records exceed collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT compact draft records"));
    }

    fn aligned_direction(direction: [f64; 3]) -> Vec<u8> {
        let mut bytes = vec![0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(5000u32.to_le_bytes());
        bytes.extend([0; 8]);
        for value in [
            1.0f64,
            1.0,
            -1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            direction[0],
            direction[1],
            direction[2],
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn extended_draft_direction_uses_its_unaligned_discriminated_vector() {
        let mut payload = vec![0; 8];
        let frame = payload.len();
        payload.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        payload.extend(0u32.to_le_bytes());
        payload.extend(5000u32.to_le_bytes());
        payload.extend([0; 8]);
        for value in [
            -1.0f64, 1.0, 1.0, -1.0, 0.25, -0.5, 0.75, -1.0, 0.0, 0.0, 0.0, 0.0,
        ] {
            payload.extend(value.to_le_bytes());
        }
        payload.extend([0; 9]);
        for value in [0.0f64, -1.0, 0.0] {
            payload.extend(value.to_le_bytes());
        }
        let end = payload.len();
        assert_eq!(
            end - frame,
            extended_dir::LEN,
            "named fields define the fixed frame length"
        );
        assert_eq!(
            unique_draft_direction(&payload, frame, end),
            Some(Vector3::new(0.0, -1.0, 0.0))
        );
    }

    #[test]
    fn compact_draft_separates_parting_tool_faces_and_direction() {
        let parting_a = [(0x8083, 80, 900, 1)];
        let parting_b = [(0x8041, 80, 901, 1), (0x8041, 80, 902, 12)];
        let face_a = [(0x8036, 80, 903, 4), (0x8041, 80, 904, 1)];
        let face_b = [(0x8021, 80, 905, 3)];
        let mut payload = vec![0; 64];
        let object_start = payload.len();
        payload.extend(compact_selection(2, &[&parting_a, &parting_b]));
        let parting_selection_end = payload.len();
        payload.extend([0; 24]);
        payload.extend(aligned_direction([0.0, -1.0, 0.0]));
        payload.extend([0; 16]);
        let face_marker = payload.len() + 12;
        payload.extend(compact_selection(3, &[&face_a, &face_b]));
        payload.extend([0; 32]);
        let object_end = payload.len();
        let feature = draft_feature();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: object_start as u64,
                value: "Draft1".into(),
                object_id: ObjectId::from_value(7),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test decode context");
        let (_, parting_paths, parsed_parting_end) =
            compact_draft_selection_at(&ctx, &lane.native_payload, object_start + 12)
                .expect("compact selection parse")
                .expect("compact parting-tool selection");
        assert_eq!(parting_paths.len(), 2);
        assert_eq!(parsed_parting_end, parting_selection_end);
        assert_eq!(
            unique_draft_direction(&lane.native_payload, parsed_parting_end, face_marker),
            Some(Vector3::new(0.0, -1.0, 0.0))
        );
        assert_eq!(
            compact_draft_selection_at(&ctx, &lane.native_payload, face_marker)
                .expect("compact selection parse")
                .expect("compact drafted-face selection")
                .1
                .len(),
            2
        );

        let operands = draft_operands(&ctx, &feature, &lane, object_start, object_end)
            .expect("draft parse")
            .expect("compact parting-line draft operands");
        assert!(matches!(operands.anchor, DraftAnchor::PartingTool(ref paths) if paths.len() == 2));
        assert_eq!(operands.faces.len(), 2);
        assert_eq!(operands.pull_direction.get(), Vector3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn declared_draft_separates_neutral_plane_faces_and_direction() {
        let token = 0x8096;
        let mut payload = vec![0; 64];
        let object_start = payload.len();
        payload.extend(plane_reference(token, [0x40, 0], 101, 3));
        payload.extend([0; 8]);
        payload.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        payload.extend(0u32.to_le_bytes());
        payload.extend(5000u32.to_le_bytes());
        payload.extend([0; 8]);
        for value in [
            1.0f64, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ] {
            payload.extend(value.to_le_bytes());
        }
        payload.extend([0; 16]);
        let first_face = payload.len();
        payload.extend(plane_reference(token, [0x40, 0], 102, 8));
        let class_offset = payload.len();
        let class_name = "moPlaneRef_w";
        payload.extend([0; 6]);
        payload.extend(class_name.as_bytes());
        payload.extend(token.to_le_bytes());
        let feature = draft_feature();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![FeatureInputClass {
                id: "plane-ref".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: class_offset as u64,
                name: class_name.into(),
            }],
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: object_start as u64,
                value: "Draft1".into(),
                object_id: ObjectId::from_value(7),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let neutral = draft_plane_reference_at(&lane.native_payload, object_start, class_offset)
            .expect("neutral-plane record");
        assert_eq!(
            unique_draft_direction(&lane.native_payload, neutral.2, first_face),
            Some(Vector3::new(0.0, 0.0, 1.0))
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test decode context");
        let operands = draft_operands(&ctx, &feature, &lane, object_start, class_offset)
            .expect("draft parse")
            .expect("complete draft operands");
        assert!(matches!(
            operands.anchor,
            DraftAnchor::NeutralPlane(ref path) if path.last().unwrap().local_id == Some(3)
        ));
        assert_eq!(operands.faces.len(), 1);
        assert_eq!(operands.faces[0].last().unwrap().local_id, Some(8));
        assert_eq!(operands.pull_direction.get(), Vector3::new(0.0, 0.0, 1.0));

        let mut malformed = lane.clone();
        malformed.native_payload[object_start + 15..object_start + 19]
            .copy_from_slice(&103u32.to_le_bytes());
        assert!(draft_operands(&ctx, &feature, &malformed, object_start, class_offset)
            .expect("malformed draft parse")
            .is_none());

        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature],
        };
        let mut projected = vec![cadmpeg_ir::features::Feature {
            id: FeatureId::mint("synthetic:test:id#draft").expect("identity grammar"),
            ordinal: 0,
            name: Some("Draft1".into()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Draft".into()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Draft {
                    faces: FaceSelection::Unresolved,
                    anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                        plane: FaceSelection::Unresolved,
                        pull: None,
                    },
                    angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
                    outward: None,
                }),
            ),
            native_ref: Some("draft".into()),
        }];
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test decode context");
        super::super::projections::project_draft_operands(
            &ctx,
            &mut projected,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
        .expect("draft projection");
        assert!(matches!(
            projected[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: FaceSelection::Native(faces),
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    plane: FaceSelection::Native(neutral_plane),
                    pull: Some(cadmpeg_ir::features::DraftPull {
                        direction: geometry_1,
                        ..
                    }),
                },
                ..
            }) if ( faces.contains(":8") && neutral_plane.contains(":3")) && matches!(geometry_1.get(), Vector3 { x: 0.0, y: 0.0, z: 1.0 })
        ));

        projected[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Draft { faces, anchor, .. }) =
                definition
            else {
                panic!("typed draft");
            };
            *faces = FaceSelection::Native("explicit-faces".into());
            let (cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                pull: Some(pull), ..
            }
            | cadmpeg_ir::features::DraftAnchor::PartingLine { pull, .. }) = anchor
            else {
                panic!("draft pull fixture");
            };
            pull.direction =
                cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
        });
        super::super::projections::project_draft_operands(
            &ctx,
            &mut projected,
            &[history],
            std::slice::from_ref(&lane),
        )
        .expect("draft projection");
        assert!(matches!(
            projected[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: FaceSelection::Native(faces),
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    pull: Some(cadmpeg_ir::features::DraftPull {
                        direction: geometry_1,
                        ..
                    }),
                    ..
                },
                ..
            }) if ( faces == "explicit-faces") && matches!(geometry_1.get(), Vector3 { x: 0.0, y: 1.0, z: 0.0 })
        ));
    }
}
