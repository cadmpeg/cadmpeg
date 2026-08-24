//! Draft-operation plane and face operands.

use super::axes::canonical_unit_direction;
use super::is_class_token;
use super::scalars::feature_object_name;
use super::selections::{
    compact_mixed_component_path, component_vector_path_at, is_component_vector_selector,
    COMPACT_EDGE_VECTOR_MARKER,
};
use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Vector3;

use crate::layout::draft_aligned_direction_frame as aligned_dir;
use crate::layout::draft_compact_selection_prefix as compact_sel;
use crate::layout::draft_extended_direction_frame as extended_dir;
use crate::layout::draft_plane_reference_prefix as draft_plane;

const EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12: f64 = 1e-12;
const EPS_DRAFTS_UNIQUE_DRAFT_DIRECTION_E9: f64 = 1e-9;

const DIRECTION_FRAME_PREFIX_LEN: usize = 24;
const MAX_PATH_CELLS: usize = 65;

#[derive(Clone, Debug)]
pub(super) struct DraftOperands {
    pub(super) anchor: DraftAnchor,
    pub(super) faces: Vec<Vec<FeatureInputComponentPathEntry>>,
    pub(super) pull_direction: Vector3,
}

#[derive(Clone, Debug)]
pub(super) enum DraftAnchor {
    NeutralPlane(Vec<FeatureInputComponentPathEntry>),
    PartingTool(Vec<Vec<FeatureInputComponentPathEntry>>),
}

pub(super) fn same_draft_operands(left: &DraftOperands, right: &DraftOperands) -> bool {
    same_draft_anchor(&left.anchor, &right.anchor)
        && left.faces.len() == right.faces.len()
        && left
            .faces
            .iter()
            .zip(&right.faces)
            .all(|(left, right)| same_component_path_semantics(left, right))
        && (left.pull_direction.x - right.pull_direction.x).abs()
            <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
        && (left.pull_direction.y - right.pull_direction.y).abs()
            <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
        && (left.pull_direction.z - right.pull_direction.z).abs()
            <= EPS_DRAFTS_SAME_DRAFT_OPERANDS_E12
}

pub(super) fn draft_operands(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Option<DraftOperands> {
    if classify(feature) != Some(FeatureClass::Draft) || object_start >= object_end {
        return None;
    }
    if let Some(operands) = declared_draft_operands(lane, object_start, object_end) {
        return Some(operands);
    }
    compact_parting_line_draft_operands(lane, object_start, object_end)
}

fn declared_draft_operands(
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Option<DraftOperands> {
    let token = unique_declared_plane_reference_token(lane)?;
    let end = object_end.min(lane.native_payload.len());
    let final_record_start = end.checked_sub(draft_plane::LEN)?;
    let records = (object_start..=final_record_start)
        .filter(|offset| lane.native_payload.get(*offset..*offset + 2) == Some(token.as_slice()))
        .filter_map(|offset| draft_plane_reference_at(&lane.native_payload, offset, end))
        .collect::<Vec<_>>();
    let (_, neutral_plane, neutral_end) = records.first()?.clone();
    let pull_direction = unique_draft_direction(
        &lane.native_payload,
        neutral_end,
        records.get(1).map_or(end, |record| record.0),
    )?;
    let mut faces = Vec::<Vec<FeatureInputComponentPathEntry>>::new();
    for path in records.into_iter().skip(1).map(|(_, path, _)| path) {
        if !faces
            .iter()
            .any(|existing| same_component_path_semantics(existing, &path))
        {
            faces.push(path);
        }
    }
    (!faces.is_empty()).then_some(DraftOperands {
        anchor: DraftAnchor::NeutralPlane(neutral_plane),
        faces,
        pull_direction,
    })
}

fn compact_parting_line_draft_operands(
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Option<DraftOperands> {
    let end = object_end.min(lane.native_payload.len());
    let final_marker = end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len())?;
    let records = (object_start.saturating_add(12)..=final_marker)
        .filter(|marker| {
            lane.native_payload
                .get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        })
        .filter_map(|marker| {
            compact_draft_selection_at(&lane.native_payload, marker)
                .map(|(role, paths, selection_end)| (marker, role, paths, selection_end))
        })
        .collect::<Vec<_>>();
    let parting_records = records
        .iter()
        .filter(|(_, role, _, _)| *role == 2)
        .collect::<Vec<_>>();
    let [parting_record] = parting_records.as_slice() else {
        return None;
    };
    let first_face = records
        .iter()
        .find(|(marker, role, _, _)| *role == 3 && *marker > parting_record.0)?;
    let pull_direction =
        unique_draft_direction(&lane.native_payload, parting_record.3, first_face.0)?;
    let faces = records
        .iter()
        .filter(|(marker, role, _, _)| *role == 3 && *marker > parting_record.0)
        .flat_map(|(_, _, paths, _)| paths.iter().cloned())
        .fold(
            Vec::<Vec<FeatureInputComponentPathEntry>>::new(),
            |mut paths, path| {
                if !paths
                    .iter()
                    .any(|existing| same_component_path_semantics(existing, &path))
                {
                    paths.push(path);
                }
                paths
            },
        );
    (!faces.is_empty()).then_some(DraftOperands {
        anchor: DraftAnchor::PartingTool(parting_record.2.clone()),
        faces,
        pull_direction,
    })
}

fn compact_draft_selection_at(
    payload: &[u8],
    marker: usize,
) -> Option<(u8, Vec<Vec<FeatureInputComponentPathEntry>>, usize)> {
    let header = marker.checked_sub(compact_sel::COMPONENT_MARKER)?;
    usize::try_from(View::u32_le_at(payload, header + compact_sel::CELL_FIELD)?)
        .ok()
        .filter(|count| (1..=MAX_PATH_CELLS).contains(count))?;
    let role_bytes =
        payload.get(header + compact_sel::SELECTION_ROLE..header + compact_sel::SELECTOR)?;
    let role = is_component_vector_selector(role_bytes).then(|| match role_bytes[1] {
        2 => 2,
        3 => 3,
        _ => unreachable!("component-vector selector helper validated role"),
    })?;
    if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker + COMPACT_EDGE_VECTOR_MARKER.len()..header + compact_sel::LEN)?
            != [0, 0]
    {
        return None;
    }
    let mut cursor = header + compact_sel::LEN;
    let mut paths = Vec::new();
    loop {
        let candidates = (1..=MAX_PATH_CELLS)
            .filter_map(|length| compact_mixed_component_path(payload, cursor, length, false))
            .filter(|(_, path_end)| {
                payload.get(*path_end..path_end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
            })
            .collect::<Vec<_>>();
        let Some((path, path_end)) = candidates.iter().min_by_key(|(_, path_end)| *path_end) else {
            return (!paths.is_empty()).then_some((role, paths, cursor));
        };
        paths.push(path.clone());
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

fn unique_draft_direction(payload: &[u8], start: usize, end: usize) -> Option<Vector3> {
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
                    scalar(relative)?,
                    scalar(relative + 8)?,
                    scalar(relative + 16)?,
                );
                let norm = direction.norm();
                ((norm - 1.0).abs() <= EPS_DRAFTS_UNIQUE_DRAFT_DIRECTION_E9)
                    .then_some(canonical_unit_direction(direction))
            };
            direction_at(aligned_dir::PULL_DIRECTION).or_else(|| {
                (frame.len() >= extended_dir::LEN
                    && frame[aligned_dir::LEN..extended_dir::PULL_DIRECTION] == [0; 9])
                    .then(|| direction_at(extended_dir::PULL_DIRECTION))
                    .flatten()
            })
        })
        .collect::<Vec<_>>();
    candidates.dedup();
    let [direction] = candidates.as_slice() else {
        return None;
    };
    Some(*direction)
}

pub(super) fn draft_operand_candidates(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<(String, DraftOperands)> {
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
        .collect::<Vec<_>>();
    objects.sort_unstable_by_key(|(offset, _)| *offset);
    objects
        .iter()
        .enumerate()
        .filter_map(|(index, (start, feature))| {
            let start = usize::try_from(*start).ok()?;
            let end = objects
                .get(index + 1)
                .and_then(|(offset, _)| usize::try_from(*offset).ok())
                .unwrap_or(lane.native_payload.len());
            draft_operands(feature, lane, start, end).map(|operands| (feature.id.clone(), operands))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputName,
    };
    use cadmpeg_ir::features::{Angle, FaceSelection, FeatureDefinition, FeatureId};
    use std::collections::BTreeMap;

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
            source_id: Some("7".into()),
            parent_source_id: None,
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
                object_id: Some(7),
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

        let (_, parting_paths, parsed_parting_end) =
            compact_draft_selection_at(&lane.native_payload, object_start + 12)
                .expect("compact parting-tool selection");
        assert_eq!(parting_paths.len(), 2);
        assert_eq!(parsed_parting_end, parting_selection_end);
        assert_eq!(
            unique_draft_direction(&lane.native_payload, parsed_parting_end, face_marker),
            Some(Vector3::new(0.0, -1.0, 0.0))
        );
        assert_eq!(
            compact_draft_selection_at(&lane.native_payload, face_marker)
                .expect("compact drafted-face selection")
                .1
                .len(),
            2
        );

        let operands = draft_operands(&feature, &lane, object_start, object_end)
            .expect("compact parting-line draft operands");
        assert!(matches!(operands.anchor, DraftAnchor::PartingTool(ref paths) if paths.len() == 2));
        assert_eq!(operands.faces.len(), 2);
        assert_eq!(operands.pull_direction, Vector3::new(0.0, -1.0, 0.0));
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
                role: FeatureInputClassRole::Reference,
            }],
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: object_start as u64,
                value: "Draft1".into(),
                object_id: Some(7),
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
        let operands = draft_operands(&feature, &lane, object_start, class_offset)
            .expect("complete draft operands");
        assert!(matches!(
            operands.anchor,
            DraftAnchor::NeutralPlane(ref path) if path.last().unwrap().local_id == Some(3)
        ));
        assert_eq!(operands.faces.len(), 1);
        assert_eq!(operands.faces[0].last().unwrap().local_id, Some(8));
        assert_eq!(operands.pull_direction, Vector3::new(0.0, 0.0, 1.0));

        let mut malformed = lane.clone();
        malformed.native_payload[object_start + 15..object_start + 19]
            .copy_from_slice(&103u32.to_le_bytes());
        assert!(draft_operands(&feature, &malformed, object_start, class_offset).is_none());

        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature],
        };
        let mut projected = vec![cadmpeg_ir::features::Feature {
            id: FeatureId("draft".into()),
            ordinal: 0,
            name: Some("Draft1".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Draft".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Draft {
                faces: FaceSelection::Unresolved,
                neutral_plane: FaceSelection::Unresolved,
                parting_tool: None,
                pull_direction: None,
                pull_plane: None,
                angle: Some(Angle(0.1)),
                outward: None,
            },
            native_ref: Some("draft".into()),
        }];
        super::super::projections::project_draft_operands(
            &mut projected,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        );
        assert!(matches!(
            &projected[0].definition,
            FeatureDefinition::Draft {
                faces: FaceSelection::Native(faces),
                neutral_plane: FaceSelection::Native(neutral_plane),
                pull_direction: Some(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
                ..
            } if faces.contains(":8") && neutral_plane.contains(":3")
        ));

        let FeatureDefinition::Draft {
            faces,
            pull_direction,
            ..
        } = &mut projected[0].definition
        else {
            panic!("typed draft");
        };
        *faces = FaceSelection::Native("explicit-faces".into());
        *pull_direction = Some(Vector3::new(0.0, 1.0, 0.0));
        super::super::projections::project_draft_operands(&mut projected, &[history], &[lane]);
        assert!(matches!(
            &projected[0].definition,
            FeatureDefinition::Draft {
                faces: FaceSelection::Native(faces),
                pull_direction: Some(Vector3 { x: 0.0, y: 1.0, z: 0.0 }),
                ..
            } if faces == "explicit-faces"
        ));
    }
}
