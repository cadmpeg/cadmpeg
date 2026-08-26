//! Direct face and body edit inputs.

use super::axes::{
    canonical_unit_direction, compact_line_reference_directions, declared_line_reference_directions,
};
use super::scalars::feature_object_name;
use crate::classification::{classify, FeatureClass};
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Vector3;
use std::collections::BTreeMap;

const EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9: f64 = 1.0e-9;
const EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E12: f64 = 1.0e-12;
const EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct MoveBodyTranslationRecord {
    pub(super) selection_offset: usize,
    pub(super) local_body_ids: Vec<u32>,
    pub(super) translation_m: Vector3,
}

pub(super) fn move_body_translation_record(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    data_class_offset: u64,
) -> Option<MoveBodyTranslationRecord> {
    const TRAILER_OFFSET: usize = 200;
    const NON_COPY_TRAILER: [u8; 8] = [1, 0, 0, 0, 0, 0, 1, 0];
    let data_class_offset = usize::try_from(data_class_offset).ok()?;
    let end = object_end.min(payload.len());
    if data_class_offset < object_start || data_class_offset >= end {
        return None;
    }
    let scalar = |offset: usize| {
        let value = View::f64_le_at(payload, offset)?;
        value.is_finite().then_some(value)
    };
    let mut candidates = Vec::new();
    for selection_offset in data_class_offset..end.saturating_sub(TRAILER_OFFSET + 20) {
        let Some(bytes) = payload.get(selection_offset..selection_offset + 4) else {
            continue;
        };
        let count = View::u32_le_at(bytes, 0).expect("four-byte body count") as usize;
        if !(1..=4096).contains(&count) {
            continue;
        }
        let ids_start = selection_offset + 4;
        let Some(ids_end) = count
            .checked_mul(4)
            .and_then(|length| ids_start.checked_add(length))
        else {
            continue;
        };
        let Some(matrix_offset) = ids_end.checked_add(12) else {
            continue;
        };
        if matrix_offset
            .checked_add(TRAILER_OFFSET + 8)
            .is_none_or(|required| required > end)
            || payload.get(ids_end..ids_end + 4) != Some(u32::MAX.to_le_bytes().as_slice())
            || payload.get(ids_end + 4..matrix_offset) != Some([0; 8].as_slice())
            || payload.get(matrix_offset + TRAILER_OFFSET..matrix_offset + TRAILER_OFFSET + 8)
                != Some(NON_COPY_TRAILER.as_slice())
        {
            continue;
        }
        let Some(matrix) = (0..9)
            .map(|index| scalar(matrix_offset + index * 8))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if matrix
            .iter()
            .zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
            .any(|(actual, expected)| {
                (*actual - expected).abs() > EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9
            })
            || payload.get(matrix_offset + 72..matrix_offset + 80)
                != Some(1u64.to_le_bytes().as_slice())
            || (0..3).any(|index| scalar(matrix_offset + 80 + index * 8).is_none())
            || scalar(matrix_offset + 104).is_none_or(|value| {
                (value - 1.0).abs() > EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9
            })
        {
            continue;
        }
        let (Some(x), Some(y), Some(z)) = (
            scalar(matrix_offset + 112),
            scalar(matrix_offset + 120),
            scalar(matrix_offset + 128),
        ) else {
            continue;
        };
        let mut translation_m = Vector3::new(x, y, z);
        for component in [
            &mut translation_m.x,
            &mut translation_m.y,
            &mut translation_m.z,
        ] {
            if component.abs() <= EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E12 {
                *component = 0.0;
            }
        }
        let Some(ids) = payload.get(ids_start..ids_end) else {
            continue;
        };
        let mut view = View::over_retained(ids);
        let mut local_body_ids = Vec::new();
        while let Some(id) = view.u32_le() {
            local_body_ids.push(id);
        }
        if local_body_ids.contains(&0) {
            continue;
        }
        candidates.push(MoveBodyTranslationRecord {
            selection_offset,
            local_body_ids,
            translation_m,
        });
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn move_body_selection_at(payload: &[u8], offset: usize) -> Option<Vec<u32>> {
    move_body_translation_record(payload, offset, payload.len(), offset as u64)
        .filter(|record| record.selection_offset == offset)
        .map(|record| record.local_body_ids)
}

/// Add translation laws carried by Move Face direction-spec children.
pub(crate) fn enrich_history_move_face_translations(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut candidates = BTreeMap::<(usize, usize), Vec<Option<Vector3>>>::new();
    for lane in lanes {
        let mut starts =
            histories
                .iter()
                .enumerate()
                .flat_map(|(history_index, history)| {
                    history.features.iter().enumerate().filter_map(
                        move |(feature_index, feature)| {
                            Some((
                                feature_object_name(feature, lane)?.offset,
                                history_index,
                                feature_index,
                            ))
                        },
                    )
                })
                .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|entry| entry.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if classify(feature) != Some(FeatureClass::MoveFace)
                || feature.properties.contains_key("Mode")
                || feature.properties.contains_key("Direction")
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .and_then(|entry| usize::try_from(entry.0).ok())
                .unwrap_or(lane.native_payload.len())
                .min(lane.native_payload.len());
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            if start >= end {
                candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(None);
                continue;
            }
            let direction_specs = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moDirectionSpec_c"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| (start..end).contains(&offset))
                })
                .count();
            let line_refs = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moLineRef_w"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| (start..end).contains(&offset))
                })
                .collect::<Vec<_>>();
            if direction_specs != 1 || line_refs.len() != 1 {
                candidates
                    .entry((history_index, feature_index))
                    .or_default()
                    .push(None);
                continue;
            }
            let mut directions = line_refs
                .iter()
                .flat_map(|class| {
                    declared_line_reference_directions(&lane.native_payload, class.offset, end)
                })
                .collect::<Vec<_>>();
            let excluded_handles = line_refs
                .iter()
                .filter_map(|class| usize::try_from(class.offset).ok())
                .flat_map(|offset| [offset + 136, offset + 144])
                .collect::<Vec<_>>();
            directions.extend(compact_line_reference_directions(
                &lane.native_payload,
                start,
                end,
                &excluded_handles,
            ));
            let mut unique = Vec::new();
            for direction in directions.into_iter().map(canonical_unit_direction) {
                if !unique.contains(&direction) {
                    unique.push(direction);
                }
            }
            candidates
                .entry((history_index, feature_index))
                .or_default()
                .push(match unique.as_slice() {
                    [direction] => Some(*direction),
                    _ => None,
                });
        }
    }
    for ((history_index, feature_index), candidates) in candidates {
        let Some((&Some(first), rest)) = candidates.split_first() else {
            continue;
        };
        if rest.iter().any(|candidate| {
            candidate.is_none_or(|candidate| {
                (candidate.x - first.x).abs()
                    > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
                    || (candidate.y - first.y).abs()
                        > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
                    || (candidate.z - first.z).abs()
                        > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
            })
        }) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        feature.properties.insert("Mode".into(), "Translate".into());
        feature.properties.insert(
            "Direction".into(),
            format!("{},{},{}", first.x, first.y, first.z),
        );
    }
}

/// Add non-copy translations carried by Move/Copy Body data children.
pub(crate) fn enrich_history_move_body_translations(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut candidates = BTreeMap::<(usize, usize), Vec<Option<Vector3>>>::new();
    for lane in lanes {
        let mut starts =
            histories
                .iter()
                .enumerate()
                .flat_map(|(history_index, history)| {
                    history.features.iter().enumerate().filter_map(
                        move |(feature_index, feature)| {
                            Some((
                                feature_object_name(feature, lane)?.offset,
                                history_index,
                                feature_index,
                            ))
                        },
                    )
                })
                .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|entry| entry.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if classify(feature) != Some(FeatureClass::MoveBody)
                || feature.properties.contains_key("Translation")
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .and_then(|entry| usize::try_from(entry.0).ok())
                .unwrap_or(lane.native_payload.len())
                .min(lane.native_payload.len());
            let Some(start) = usize::try_from(start).ok().filter(|start| *start < end) else {
                continue;
            };
            let data_classes = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moMoveCopyBodyData_c"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| (start..end).contains(&offset))
                })
                .collect::<Vec<_>>();
            let candidate = match data_classes.as_slice() {
                [class] => {
                    move_body_translation_record(&lane.native_payload, start, end, class.offset)
                        .map(|record| record.translation_m)
                }
                _ => None,
            };
            candidates
                .entry((history_index, feature_index))
                .or_default()
                .push(candidate);
        }
    }
    for ((history_index, feature_index), candidates) in candidates {
        let Some((&Some(first), rest)) = candidates.split_first() else {
            continue;
        };
        if rest.iter().any(|candidate| *candidate != Some(first)) {
            continue;
        }
        histories[history_index].features[feature_index]
            .properties
            .insert(
                "Translation".into(),
                format!(
                    "{}mm,{}mm,{}mm",
                    first.x * 1000.0,
                    first.y * 1000.0,
                    first.z * 1000.0
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputName,
        FeatureInputScalar, FeatureInputScalarRole,
    };
    use cadmpeg_ir::features::{FaceMotion, FaceSelection, FeatureDefinition, Length};
    use std::collections::BTreeMap;

    fn move_face_history() -> FeatureHistory {
        FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
                id: "move-face".into(),
                parent: "history".into(),
                xml_tag: "Feature".into(),
                tree_parent: None,
                source_id: Some("7".into()),
                parent_source_id: None,
                ordinal: 0,
                name: "Move Face".into(),
                kind: "Move Face".into(),
                input_class: Some("moMoveFace_c".into()),
                suppressed: false,
                parameters: BTreeMap::from([("D1".into(), "0.2".into())]),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }
    }

    fn line_reference_lane(directions: &[Vector3], direction_specs: usize) -> FeatureInputLane {
        let mut native_payload = vec![0; 640];
        for (index, direction) in directions.iter().enumerate() {
            let handle = 160 + index * 160;
            native_payload[handle..handle + 8]
                .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
            native_payload[handle + 104..handle + 112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
            for (component, value) in [direction.x, direction.y, direction.z]
                .into_iter()
                .enumerate()
            {
                let offset = handle + 64 + component * 8;
                native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        let mut classes = (0..direction_specs)
            .map(|index| FeatureInputClass {
                id: format!("direction-spec-{index}"),
                parent: "lane".into(),
                ordinal: index as u32,
                offset: 32 + index as u64,
                name: "moDirectionSpec_c".into(),
                role: FeatureInputClassRole::Reference,
            })
            .collect::<Vec<_>>();
        classes.push(FeatureInputClass {
            id: "line-ref".into(),
            parent: "lane".into(),
            ordinal: direction_specs as u32,
            offset: 80,
            name: "moLineRef_w".into(),
            role: FeatureInputClassRole::Reference,
        });
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes,
            names: vec![
                FeatureInputName {
                    id: "name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 8,
                    value: "Move Face".into(),
                    object_id: Some(7),
                },
                FeatureInputName {
                    id: "d1-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 100,
                    value: "D1".into(),
                    object_id: None,
                },
            ],
            scalars: vec![FeatureInputScalar {
                id: "d1-scalar".into(),
                parent: "lane".into(),
                feature_ref: Some("move-face".into()),
                ordinal: 0,
                offset: 128,
                object_id: 8,
                name: "d1-name".into(),
                value: 0.005,
                role: FeatureInputScalarRole::Driving,
                entity_indices: Vec::new(),
                operands: Vec::new(),
            }],
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }

    #[test]
    fn move_face_translation_requires_one_direction_spec_and_one_direction() {
        let mut histories = vec![move_face_history()];
        let lane = line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 1);
        enrich_history_move_face_translations(&mut histories, std::slice::from_ref(&lane));
        crate::resolved_features::parameters::enrich_history_parameters(
            &mut histories,
            [&lane],
            true,
        );
        assert_eq!(histories[0].features[0].parameters["D1"], "5mm");
        let projected = crate::history::project_features(&histories);
        assert!(matches!(
            &projected[0].definition,
            FeatureDefinition::MoveFace {
                faces: FaceSelection::Unresolved,
                motion: FaceMotion::Translate { direction, distance },
            } if *direction == Vector3::new(0.0, -1.0, 0.0) && *distance == Length(5.0)
        ));

        for lane in [
            line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 0),
            line_reference_lane(
                &[Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)],
                1,
            ),
        ] {
            let mut histories = vec![move_face_history()];
            enrich_history_move_face_translations(&mut histories, &[lane]);
            assert!(matches!(
                crate::history::project_features(&histories)[0].definition,
                FeatureDefinition::Native { .. }
            ));
        }

        let mut histories = vec![move_face_history()];
        enrich_history_move_face_translations(
            &mut histories,
            &[
                line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 1),
                line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 0),
            ],
        );
        assert!(matches!(
            crate::history::project_features(&histories)[0].definition,
            FeatureDefinition::Native { .. }
        ));
    }

    #[test]
    fn move_body_translation_requires_fixed_identity_and_non_copy_trailer() {
        let selection_offset = 64;
        let mut payload = vec![0; 384];
        payload[selection_offset..selection_offset + 4].copy_from_slice(&2u32.to_le_bytes());
        payload[selection_offset + 4..selection_offset + 8].copy_from_slice(&17u32.to_le_bytes());
        payload[selection_offset + 8..selection_offset + 12].copy_from_slice(&23u32.to_le_bytes());
        payload[selection_offset + 12..selection_offset + 16]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        let matrix_offset = selection_offset + 24;
        for (index, value) in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .enumerate()
        {
            payload[matrix_offset + index * 8..matrix_offset + index * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 72..matrix_offset + 80].copy_from_slice(&1u64.to_le_bytes());
        payload[matrix_offset + 104..matrix_offset + 112].copy_from_slice(&1.0f64.to_le_bytes());
        for (index, value) in [0.01f64, -0.02, 0.03].into_iter().enumerate() {
            payload[matrix_offset + 112 + index * 8..matrix_offset + 120 + index * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 200..matrix_offset + 208]
            .copy_from_slice(&[1, 0, 0, 0, 0, 0, 1, 0]);

        assert_eq!(
            move_body_translation_record(&payload, 0, payload.len(), 0),
            Some(MoveBodyTranslationRecord {
                selection_offset,
                local_body_ids: vec![17, 23],
                translation_m: Vector3::new(0.01, -0.02, 0.03),
            })
        );
        let mut rotated = payload.clone();
        rotated[matrix_offset..matrix_offset + 8].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(
            move_body_translation_record(&rotated, 0, rotated.len(), 0),
            None
        );
        payload[matrix_offset + 200] = 0;
        assert_eq!(
            move_body_translation_record(&payload, 0, payload.len(), 0),
            None
        );
    }
}
