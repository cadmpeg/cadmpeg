//! Draft-operation plane and face operands.

use super::axes::canonical_unit_direction;
use super::scalars::feature_object_name;
use super::selections::{component_vector_path_at, COMPACT_EDGE_VECTOR_MARKER};
use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};
use cadmpeg_ir::math::Vector3;

const PLANE_REFERENCE_HEADER_LEN: usize = 94;
const DIRECTION_FRAME_PREFIX_LEN: usize = 24;
const DIRECTION_FRAME_LEN: usize = 120;
const DIRECTION_OFFSET: usize = 96;
const MAX_PATH_CELLS: usize = 65;

#[derive(Clone, Debug)]
pub(super) struct DraftOperands {
    pub(super) neutral_plane: Vec<FeatureInputComponentPathEntry>,
    pub(super) faces: Vec<Vec<FeatureInputComponentPathEntry>>,
    pub(super) pull_direction: Vector3,
}

pub(super) fn same_draft_operands(left: &DraftOperands, right: &DraftOperands) -> bool {
    same_component_path_semantics(&left.neutral_plane, &right.neutral_plane)
        && left.faces.len() == right.faces.len()
        && left
            .faces
            .iter()
            .zip(&right.faces)
            .all(|(left, right)| same_component_path_semantics(left, right))
        && (left.pull_direction.x - right.pull_direction.x).abs() <= 1.0e-12
        && (left.pull_direction.y - right.pull_direction.y).abs() <= 1.0e-12
        && (left.pull_direction.z - right.pull_direction.z).abs() <= 1.0e-12
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
    let token = unique_declared_plane_reference_token(lane)?;
    let end = object_end.min(lane.native_payload.len());
    let final_record_start = end.checked_sub(PLANE_REFERENCE_HEADER_LEN + 18)?;
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
        neutral_plane,
        faces,
        pull_direction,
    })
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
        let token: [u8; 2] = lane.native_payload.get(body..body + 2)?.try_into().ok()?;
        let value = u16::from_le_bytes(token);
        (value & 0x8000 != 0 && value != u16::MAX).then_some(token)
    });
    let first = tokens.next()?;
    tokens.all(|token| token == first).then_some(first)
}

fn draft_plane_reference_at(
    payload: &[u8],
    offset: usize,
    object_end: usize,
) -> Option<(usize, Vec<FeatureInputComponentPathEntry>, usize)> {
    let header = payload.get(offset..offset.checked_add(PLANE_REFERENCE_HEADER_LEN)?)?;
    if offset + PLANE_REFERENCE_HEADER_LEN > object_end
        || !header[2..4]
            .try_into()
            .ok()
            .map(u16::from_le_bytes)
            .is_some_and(|token| token & 0x8000 != 0 && token != u16::MAX)
        || header[4..8] != 2u32.to_le_bytes()
        || !matches!(&header[8..11], [0 | 0x40, 0, 0])
        || header[11..15] == [0; 4]
        || header[11..15] != header[15..19]
        || header[19..47] != [0; 28]
        || header[47..63] != [0xff; 16]
        || header[63..72] != [0; 9]
        || !header[72..74]
            .try_into()
            .ok()
            .map(u16::from_le_bytes)
            .is_some_and(|token| token & 0x8000 != 0 && token != u16::MAX)
        || header[78..82] != [0; 4]
        || !matches!(&header[86..90], [0, 2 | 3, 0, 0])
    {
        return None;
    }
    usize::try_from(u32::from_le_bytes(header[82..86].try_into().ok()?))
        .ok()
        .filter(|count| (2..=MAX_PATH_CELLS).contains(count))?;
    let marker = offset + PLANE_REFERENCE_HEADER_LEN;
    if payload.get(marker..marker + COMPACT_EDGE_VECTOR_MARKER.len())? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker + COMPACT_EDGE_VECTOR_MARKER.len()..marker + 18)? != [0, 0]
    {
        return None;
    }
    let components = component_vector_path_at(payload, marker)?;
    let path_start = marker.checked_add(18)?;
    (path_start <= object_end).then_some((offset, components, path_start))
}

fn unique_draft_direction(payload: &[u8], start: usize, end: usize) -> Option<Vector3> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let final_frame_start = end
        .checked_sub(DIRECTION_FRAME_LEN)
        .filter(|end| *end >= start)?;
    let mut candidates = (start..=final_frame_start)
        .filter(|offset| payload.get(*offset..*offset + HANDLES.len()) == Some(HANDLES.as_slice()))
        .filter_map(|offset| {
            let frame = payload.get(offset..offset + DIRECTION_FRAME_LEN)?;
            if frame[8..12] != [0; 4]
                || frame[12..16] == [0; 4]
                || frame[16..DIRECTION_FRAME_PREFIX_LEN] != [0; 8]
            {
                return None;
            }
            let scalar = |relative: usize| {
                let value = f64::from_le_bytes(frame.get(relative..relative + 8)?.try_into().ok()?);
                value.is_finite().then_some(value)
            };
            if !(DIRECTION_FRAME_PREFIX_LEN..DIRECTION_FRAME_LEN)
                .step_by(8)
                .all(|relative| scalar(relative).is_some())
            {
                return None;
            }
            let direction = Vector3::new(
                scalar(DIRECTION_OFFSET)?,
                scalar(DIRECTION_OFFSET + 8)?,
                scalar(DIRECTION_OFFSET + 16)?,
            );
            let norm = direction.norm();
            ((norm - 1.0).abs() <= 1.0e-9).then_some(canonical_unit_direction(direction))
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
        assert_eq!(operands.neutral_plane.last().unwrap().local_id, Some(3));
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
