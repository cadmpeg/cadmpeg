//! Direct face and body edit inputs.

use super::axes::{
    canonical_unit_direction, compact_line_reference_directions, declared_line_reference_directions,
};
use super::scalars::feature_object_name;
use crate::classification::{classify, FeatureClass};
use crate::records::FeatureInputLane;
use cadmpeg_ir::math::Vector3;
use std::collections::BTreeMap;

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
                (candidate.x - first.x).abs() > 1.0e-12
                    || (candidate.y - first.y).abs() > 1.0e-12
                    || (candidate.z - first.z).abs() > 1.0e-12
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
}
