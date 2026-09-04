//! History feature class binding.

use super::is_class_token;
use super::operations::repeated_class_token;
use super::scalars::feature_object_name;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{FeatureInputClassRole, FeatureInputLane};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use super::component_paths::{
    is_profile_feature_object, profile_owns_intervening_sketch_blocks,
    project_adjacent_extrusion_profiles,
};
#[cfg(test)]
use super::reference_geometry::enrich_history_reference_planes;
#[cfg(test)]
use super::terminations::is_extrusion_end_spec_owner;
#[cfg(test)]
use crate::records::FeatureInputClass;
#[cfg(test)]
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, Length, LinearTermination};
#[cfg(test)]
use std::collections::BTreeMap;

/// Recognize the source-less legacy plane/origin/sketch/extrusion prefix.
fn idless_legacy_startup_shape(records: &[crate::records::Feature]) -> bool {
    let [front, top, right, origin, sketch, extrusion] = records else {
        return false;
    };
    let plain = |record: &crate::records::Feature| {
        record.input_class.is_none()
            && record.source_id.is_none()
            && record.tree_parent.is_none()
            && record.parent_source_id.is_none()
            && record.xml_tag.eq_ignore_ascii_case("Feature")
            && record.properties.is_empty()
    };
    [front, top, right, origin, sketch, extrusion]
        .into_iter()
        .all(plain)
        && [front, top, right, origin]
            .into_iter()
            .all(|record| record.parameters.is_empty())
        && !extrusion.parameters.is_empty()
        && !front.kind.is_empty()
        && front.kind == top.kind
        && front.kind == right.kind
        && origin.kind != front.kind
        && sketch.kind != origin.kind
        && extrusion.kind != sketch.kind
        && [front, top, right, origin, sketch, extrusion]
            .windows(2)
            .all(|pair| pair[1].ordinal == pair[0].ordinal + 1)
}

/// Bind Keywords history records to their serialized feature-input object classes.
pub(crate) fn bind_history_classes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        feature.input_class = None;
    }
    let mut classes_by_object = HashMap::<u32, Vec<&str>>::new();
    for lane in lanes {
        let names_by_offset = lane
            .names
            .iter()
            .map(|name| (name.offset, name))
            .collect::<HashMap<_, _>>();
        for class in &lane.classes {
            let name_offset = class.offset + 6 + class.name.len() as u64;
            let Some(name) = names_by_offset.get(&name_offset) else {
                continue;
            };
            if let Some(object_id) = name.object_id {
                classes_by_object
                    .entry(object_id)
                    .or_default()
                    .push(&class.name);
            }
        }
    }

    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let classes = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(|object_id| classes_by_object.get(&object_id));
        let Some(classes) = classes else {
            continue;
        };
        let Some((&first, rest)) = classes.split_first() else {
            continue;
        };
        if rest.iter().all(|class| *class == first) {
            feature.input_class = Some(first.to_string());
        }
    }

    let mut direct_classes_by_name = HashMap::<&str, Vec<&str>>::new();
    for lane in lanes {
        let names_by_offset = lane
            .names
            .iter()
            .map(|name| (name.offset, name.value.as_str()))
            .collect::<HashMap<_, _>>();
        for class in &lane.classes {
            let name_offset = class.offset + 6 + class.name.len() as u64;
            let Some(name) = names_by_offset.get(&name_offset) else {
                continue;
            };
            if native_object_class(&class.name).role != FeatureInputClassRole::Native {
                direct_classes_by_name
                    .entry(name)
                    .or_default()
                    .push(&class.name);
            }
        }
    }
    for classes in direct_classes_by_name.values_mut() {
        classes.sort_unstable();
        classes.dedup();
    }
    let mut history_name_counts = HashMap::<String, usize>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if !feature.name.is_empty() {
            *history_name_counts.entry(feature.name.clone()).or_default() += 1;
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none() && feature.source_id.is_none())
    {
        if history_name_counts.get(&feature.name) == Some(&1) {
            let Some([class]) = direct_classes_by_name
                .get(feature.name.as_str())
                .map(Vec::as_slice)
            else {
                continue;
            };
            feature.input_class = Some((*class).to_string());
        }
    }

    let mut cosmetic_thread_classes = HashMap::<String, Vec<String>>::new();
    for lane in lanes {
        let mut declared = lane
            .classes
            .iter()
            .filter(|class| {
                native_object_class(&class.name).kind == NativeClassKind::CosmeticThread
            })
            .map(|class| class.name.clone())
            .collect::<Vec<_>>();
        declared.sort();
        declared.dedup();
        let [class] = declared.as_slice() else {
            continue;
        };
        let direct_name_offsets = lane
            .classes
            .iter()
            .map(|class| class.offset + 6 + class.name.len() as u64)
            .collect::<HashSet<_>>();
        let mut groups = HashMap::<u16, Vec<&crate::records::Feature>>::new();
        for feature in histories
            .iter()
            .flat_map(|history| &history.features)
            .filter(|feature| feature.input_class.is_none())
        {
            let Some(name) = feature_object_name(feature, lane).filter(|name| {
                !direct_name_offsets.contains(&name.offset)
                    && name.object_id.is_some()
                    && name.value == feature.name
            }) else {
                continue;
            };
            let Some(token) = usize::try_from(name.offset)
                .ok()
                .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                .filter(|token| is_class_token(*token))
            else {
                continue;
            };
            groups.entry(token).or_default().push(feature);
        }
        for features in groups.values() {
            if features
                .iter()
                .all(|feature| cosmetic_thread_parameter_shape(feature))
            {
                for feature in features {
                    cosmetic_thread_classes
                        .entry(feature.id.clone())
                        .or_default()
                        .push(class.clone());
                }
            }
        }
    }
    for classes in cosmetic_thread_classes.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some([class]) = cosmetic_thread_classes.get(&feature.id).map(Vec::as_slice) {
            feature.input_class = Some(class.clone());
        }
    }

    let mut native_startups = Vec::<[&str; 6]>::new();
    for lane in lanes {
        let resolved = lane
            .classes
            .iter()
            .filter_map(|class| {
                matches!(
                    native_object_class(&class.name).kind,
                    NativeClassKind::ReferencePlane
                        | NativeClassKind::OriginProfileFeature
                        | NativeClassKind::ProfileFeature
                        | NativeClassKind::Extrusion
                )
                .then_some(class.name.as_str())
            })
            .collect::<Vec<_>>();
        for classes in resolved.windows(4) {
            let [plane, origin, sketch, extrusion] = classes else {
                continue;
            };
            if native_object_class(plane).kind == NativeClassKind::ReferencePlane
                && native_object_class(origin).kind == NativeClassKind::OriginProfileFeature
                && native_object_class(sketch).kind == NativeClassKind::ProfileFeature
                && native_object_class(extrusion).kind == NativeClassKind::Extrusion
            {
                native_startups.push([plane, plane, plane, origin, sketch, extrusion]);
            }
        }
    }
    native_startups.sort_unstable();
    native_startups.dedup();
    if let [classes] = native_startups.as_slice() {
        for history in histories.iter_mut() {
            let candidates = history
                .features
                .windows(6)
                .enumerate()
                .filter(|(_, records)| idless_legacy_startup_shape(records))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if let [index] = candidates.as_slice() {
                for (feature, class) in history.features[*index..*index + 6].iter_mut().zip(classes)
                {
                    feature.input_class = Some((*class).to_string());
                }
            }
        }
    }

    let mut classes_by_type = HashMap::<String, Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if let Some(class) = &feature.input_class {
            classes_by_type
                .entry(feature.kind.clone())
                .or_default()
                .push(class.clone());
        }
    }
    for classes in classes_by_type.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some([class]) = classes_by_type.get(&feature.kind).map(Vec::as_slice) {
            feature.input_class = Some(class.clone());
        }
    }

    let direct_name_offsets = lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .map(|class| (lane.id.as_str(), class.offset + 6 + class.name.len() as u64))
        })
        .collect::<HashSet<_>>();
    let mut classes_by_token = HashMap::<(&str, u16), Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let Some(class) = &feature.input_class else {
            continue;
        };
        let object_id = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok());
        let unique_idless_name = object_id.is_none()
            && history_name_counts.get(&feature.name) == Some(&1)
            && lanes
                .iter()
                .flat_map(|lane| &lane.names)
                .filter(|name| name.object_id.is_some() && name.value == feature.name)
                .count()
                == 1;
        if object_id.is_none() && !unique_idless_name {
            continue;
        }
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                object_id.map_or(name.value == feature.name, |object_id| {
                    name.object_id == Some(object_id)
                }) && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                if let Some(token) = repeated_class_token(&lane.native_payload, offset) {
                    classes_by_token
                        .entry((lane.id.as_str(), token))
                        .or_default()
                        .push(class.clone());
                }
            }
        }
    }
    for classes in classes_by_token.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        let object_id = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok());
        let unique_idless_name = object_id.is_none()
            && history_name_counts.get(&feature.name) == Some(&1)
            && lanes
                .iter()
                .flat_map(|lane| &lane.names)
                .filter(|name| name.object_id.is_some() && name.value == feature.name)
                .count()
                == 1;
        if object_id.is_none() && !unique_idless_name {
            continue;
        }
        let mut candidates = Vec::new();
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                object_id.map_or(name.value == feature.name, |object_id| {
                    name.object_id == Some(object_id)
                }) && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                let Some(token) = repeated_class_token(&lane.native_payload, offset) else {
                    continue;
                };
                if let Some([class]) = classes_by_token
                    .get(&(lane.id.as_str(), token))
                    .map(Vec::as_slice)
                {
                    candidates.push(class.clone());
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        if let [class] = candidates.as_slice() {
            feature.input_class = Some(class.clone());
        }
    }

    let legacy_hole_bindings = legacy_repeated_hole_wizard_classes(histories, lanes);
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some(class) = legacy_hole_bindings.get(&feature.id) {
            feature.input_class = Some(class.clone());
        }
    }

    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some(class) = classless_dimension_schema_class(feature) {
            feature.input_class = Some(class.into());
        }
    }
}

fn legacy_repeated_hole_wizard_classes(
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> HashMap<String, String> {
    let mut by_name = HashMap::<&str, Option<&crate::records::Feature>>::new();
    let mut hole_shapes = HashSet::<&str>::new();
    for history in histories {
        for feature in &history.features {
            if !feature.name.is_empty() {
                by_name
                    .entry(&feature.name)
                    .and_modify(|candidate| *candidate = None)
                    .or_insert(Some(feature));
            }
        }
        for records in history.features.windows(3) {
            let [operation, first_sketch, second_sketch] = records else {
                continue;
            };
            if operation.input_class.is_none()
                && operation.source_id.is_none()
                && operation.xml_tag.eq_ignore_ascii_case("Feature")
                && first_sketch.ordinal == operation.ordinal + 1
                && second_sketch.ordinal == operation.ordinal + 2
                && [first_sketch, second_sketch].into_iter().all(|sketch| {
                    sketch.input_class.as_deref().is_some_and(|class| {
                        native_object_class(class).kind == NativeClassKind::ProfileFeature
                    })
                })
            {
                hole_shapes.insert(operation.id.as_str());
            }
        }
    }

    let mut bindings = HashMap::new();
    for lane in lanes {
        let mut declared = lane
            .classes
            .iter()
            .filter(|class| native_object_class(&class.name).kind == NativeClassKind::HoleWizard)
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>();
        declared.sort_unstable();
        declared.dedup();
        let [class] = declared.as_slice() else {
            continue;
        };
        let direct_name_offsets = lane
            .classes
            .iter()
            .map(|class| class.offset + 6 + class.name.len() as u64)
            .collect::<HashSet<_>>();
        let mut groups = HashMap::<u16, Vec<&crate::records::Feature>>::new();
        for name in &lane.names {
            if direct_name_offsets.contains(&name.offset) {
                continue;
            }
            let Some(feature) = by_name.get(name.value.as_str()).copied().flatten() else {
                continue;
            };
            let Some(token) = usize::try_from(name.offset)
                .ok()
                .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                .filter(|token| is_class_token(*token))
            else {
                continue;
            };
            groups.entry(token).or_default().push(feature);
        }
        for features in groups.values() {
            if features
                .iter()
                .all(|feature| hole_shapes.contains(feature.id.as_str()))
            {
                for feature in features {
                    bindings.insert(feature.id.clone(), (*class).to_string());
                }
            }
        }
    }
    bindings
}

fn classless_dimension_schema_class(feature: &crate::records::Feature) -> Option<&'static str> {
    if !feature.xml_tag.eq_ignore_ascii_case("Feature")
        || feature.parameters.is_empty()
        || feature.content.len() != feature.parameters.len()
    {
        return None;
    }
    let dimensions = feature
        .content
        .iter()
        .map(|content| match content {
            crate::records::FeatureContent::Dimension(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Option<HashSet<_>>>()?;
    if dimensions.len() != feature.parameters.len()
        || !feature
            .parameters
            .keys()
            .all(|name| dimensions.contains(name.as_str()))
    {
        return None;
    }
    if cosmetic_thread_parameter_shape(feature) {
        return Some("moCosmeticThread_c");
    }
    if feature.parameters.len() == 2
        && feature
            .parameters
            .keys()
            .all(|name| matches!(name.as_str(), "D1" | "D2"))
        && feature
            .parameters
            .get("D1")
            .and_then(|value| crate::history::parse_positive_dimension_length_mm(value))
            .is_some()
        && feature
            .parameters
            .get("D2")
            .filter(|value| {
                let value = value.trim();
                value.ends_with("deg") || value.ends_with('°') || value.ends_with("rad")
            })
            .and_then(|value| crate::history::parse_angle_rad(value))
            .is_some_and(|angle| angle > 0.0 && angle < std::f64::consts::PI)
    {
        return Some("Chamfer_c");
    }
    None
}

fn cosmetic_thread_parameter_shape(feature: &crate::records::Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature
            .parameters
            .keys()
            .all(|name| matches!(name.as_str(), "D1" | "D2"))
        && feature.parameters.get("D2").is_some_and(|expression| {
            let expression = expression.trim();
            expression.starts_with("<MOD-DIAM>") || expression.starts_with("&lt;MOD-DIAM&gt;")
        })
}

#[cfg(test)]
mod idless_history_binding_tests {
    use super::*;
    use crate::records::{Feature, FeatureContent, FeatureHistory, FeatureInputName};

    fn feature(ordinal: u32, kind: &str) -> Feature {
        Feature {
            id: format!("feature-{ordinal}"),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            parent_source_id: None,
            ordinal,
            name: format!("name-{ordinal}"),
            kind: kind.into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    #[test]
    fn face_plane_record_suppresses_embedded_plane_source_candidate() {
        let mut offset = feature(0, "offset plane");
        offset.source_id = Some("10".into());
        offset.input_class = Some("moRefPlane_c".into());
        offset.parameters.insert("D1".into(), "0mm".into());
        let mut principal = feature(1, "principal plane");
        principal.source_id = Some("3".into());
        principal.input_class = Some("moRefPlane_c".into());

        let mut payload = Vec::new();
        payload.extend(3u32.to_le_bytes());
        payload.extend([0x43, 0xf6, 0x8a, 0x4d]);
        payload.extend([0; 2]);
        payload.extend(3u32.to_le_bytes());
        payload.extend(1u32.to_le_bytes());
        payload.extend([0; 4]);
        payload.extend(247u32.to_le_bytes());
        payload.extend([0; 12]);
        payload.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        let face_offset = payload.len();
        payload.resize(face_offset + 115, 0);
        payload[face_offset..face_offset + 2].copy_from_slice(&0x802d_u16.to_le_bytes());
        payload[face_offset + 2..face_offset + 6].copy_from_slice(&2u32.to_le_bytes());
        payload[face_offset + 45..face_offset + 61].fill(0xff);
        payload[face_offset + 69..face_offset + 73].copy_from_slice(&2u32.to_le_bytes());
        payload[face_offset + 73..face_offset + 77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
        payload[face_offset + 77..face_offset + 83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
        payload[face_offset + 83..face_offset + 87].copy_from_slice(&1u32.to_le_bytes());
        payload[face_offset + 91..face_offset + 95].copy_from_slice(&175u32.to_le_bytes());
        payload[face_offset + 99..face_offset + 103].copy_from_slice(&3u32.to_le_bytes());
        payload[face_offset + 107..face_offset + 115]
            .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        let end = payload.len() as u64;
        let mut histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![offset, principal],
        }];
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![
                FeatureInputName {
                    id: "offset-name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 0,
                    object_id: Some(10),
                    value: "name-0".into(),
                },
                FeatureInputName {
                    id: "principal-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: end,
                    object_id: Some(3),
                    value: "name-1".into(),
                },
            ],
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

        enrich_history_reference_planes(&mut histories, &[lane]);

        let properties = &histories[0].features[0].properties;
        assert!(!properties.contains_key("Reference"));
        assert!(properties.contains_key("ReferenceFaceNative"));
    }

    #[test]
    fn classless_sketch_objects_are_profile_features_only_with_source_identity() {
        let mut sketch = feature(1, "localized sketch");
        sketch.xml_tag = "Sketch".into();
        sketch.source_id = Some("77".into());
        assert!(is_profile_feature_object(&sketch));

        sketch.source_id = Some("0".into());
        assert!(!is_profile_feature_object(&sketch));
        sketch.source_id = Some("77".into());
        sketch.input_class = Some("moRefPlane_c".into());
        assert!(!is_profile_feature_object(&sketch));
    }

    #[test]
    fn history_metadata_does_not_interrupt_an_extrusion_profile_pair() {
        let mut profile = feature(1, "sketch");
        profile.id = "profile-native".into();
        profile.xml_tag = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        profile.source_id = Some("41".into());
        let mut metadata = feature(2, "attribute");
        metadata.id = "metadata-native".into();
        metadata.source_id = Some("42".into());
        metadata.input_class = Some("moAttribute_c".into());
        let mut extrusion = feature(2, "extrusion");
        extrusion.id = "extrusion-native".into();
        extrusion.xml_tag = "Extrusion".into();
        extrusion.source_id = Some("43".into());
        extrusion
            .properties
            .insert("Dissectable".into(), "true".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![profile, metadata, extrusion],
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: vec![
                FeatureInputName {
                    id: "profile-name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 100,
                    object_id: Some(41),
                    value: "name-1".into(),
                },
                FeatureInputName {
                    id: "metadata-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 150,
                    object_id: Some(42),
                    value: "name-2".into(),
                },
                FeatureInputName {
                    id: "extrusion-name".into(),
                    parent: "lane".into(),
                    ordinal: 2,
                    offset: 200,
                    object_id: Some(43),
                    value: "name-2".into(),
                },
            ],
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
        let profile_id = cadmpeg_ir::features::FeatureId("profile".into());
        let mut features = vec![
            cadmpeg_ir::features::Feature {
                id: profile_id.clone(),
                ordinal: 0,
                name: None,
                suppressed: Some(false),
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: FeatureDefinition::Sketch { sketch: None },
                native_ref: Some("profile-native".into()),
            },
            cadmpeg_ir::features::Feature {
                id: cadmpeg_ir::features::FeatureId("extrusion".into()),
                ordinal: 1,
                name: None,
                suppressed: Some(false),
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: FeatureDefinition::Extrude {
                    profile: cadmpeg_ir::features::ProfileRef::Unresolved(
                        "extrusion-native".into(),
                    ),
                    direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                    start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                    extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                        side: cadmpeg_ir::features::ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: Length(1.0),
                            },
                            draft: None,
                        },
                    },
                    op: BooleanOp::Join,
                    solid: Some(true),
                    face_maker: None,
                    inner_wire_taper: None,
                    length_along_profile_normal: None,
                    allow_multi_profile_faces: None,
                },
                native_ref: Some("extrusion-native".into()),
            },
        ];

        project_adjacent_extrusion_profiles(
            &mut features,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        );

        assert!(matches!(
            &features[1].definition,
            FeatureDefinition::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Feature(actual),
                ..
            } if actual == &profile_id
        ));
        assert_eq!(features[1].dependencies, [profile_id]);
    }

    #[test]
    fn exact_dimension_schemas_bind_classless_thread_and_chamfer_features() {
        let mut thread = feature(1, "localized thread");
        thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
        thread.content.push(FeatureContent::Dimension("D2".into()));
        assert_eq!(
            classless_dimension_schema_class(&thread),
            Some("moCosmeticThread_c")
        );

        let mut chamfer = feature(2, "localized chamfer");
        chamfer.parameters.insert("D1".into(), "0.57".into());
        chamfer.parameters.insert("D2".into(), "45°".into());
        chamfer.content.extend([
            FeatureContent::Dimension("D1".into()),
            FeatureContent::Dimension("D2".into()),
        ]);
        assert_eq!(
            classless_dimension_schema_class(&chamfer),
            Some("Chamfer_c")
        );

        chamfer.parameters.insert("D2".into(), "1.25".into());
        assert_eq!(classless_dimension_schema_class(&chamfer), None);
        chamfer.content.pop();
        assert_eq!(classless_dimension_schema_class(&chamfer), None);
    }

    #[test]
    fn native_extrusion_class_establishes_an_end_spec_owner() {
        let mut cut = feature(1, "localized cut");
        cut.xml_tag = "Feature".into();
        cut.input_class = Some("moCut_c".into());
        assert!(is_extrusion_end_spec_owner(&cut));

        cut.input_class = Some("moRefPlane_c".into());
        assert!(!is_extrusion_end_spec_owner(&cut));
        cut.xml_tag = "Cut".into();
        assert!(is_extrusion_end_spec_owner(&cut));
    }

    #[test]
    fn repeated_legacy_holes_own_two_consecutive_profile_children() {
        let mut first = feature(10, "localized hole A");
        first.name = "hole A".into();
        let mut first_position = feature(11, "sketch");
        first_position.input_class = Some("moProfileFeature_c".into());
        let mut first_profile = feature(12, "sketch");
        first_profile.input_class = Some("moProfileFeature_c".into());
        let mut second = feature(20, "localized hole B");
        second.name = "hole B".into();
        let mut second_position = feature(21, "sketch");
        second_position.input_class = Some("moProfileFeature_c".into());
        let mut second_profile = feature(22, "sketch");
        second_profile.input_class = Some("moProfileFeature_c".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![
                first,
                first_position,
                first_profile,
                second,
                second_position,
                second_profile,
            ],
        };
        let mut payload = vec![0; 240];
        payload[98..100].copy_from_slice(&0x82a4u16.to_le_bytes());
        payload[198..200].copy_from_slice(&0x82a4u16.to_le_bytes());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                name: "moHoleWzd_c".into(),
                role: FeatureInputClassRole::Feature,
            }],
            names: vec![
                FeatureInputName {
                    id: "first".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 100,
                    object_id: Some(41),
                    value: "hole A".into(),
                },
                FeatureInputName {
                    id: "second".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 200,
                    object_id: Some(42),
                    value: "hole B".into(),
                },
            ],
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

        let bindings = legacy_repeated_hole_wizard_classes(&[history], &[lane]);
        assert_eq!(
            bindings.get("feature-10").map(String::as_str),
            Some("moHoleWzd_c")
        );
        assert_eq!(
            bindings.get("feature-20").map(String::as_str),
            Some("moHoleWzd_c")
        );
    }

    #[test]
    fn dissectable_profile_owns_its_block_object_sequence() {
        let mut profile = feature(0, "sketch");
        profile.input_class = Some("moProfileFeature_c".into());
        profile
            .properties
            .insert("DissectableChildren".into(), "23,27".into());
        let mut definition_a = feature(1, "block");
        definition_a.input_class = Some("moSketchBlockDef_c".into());
        definition_a.source_id = Some("23".into());
        let mut instance = feature(2, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = Some("25".into());
        let mut definition_b = feature(3, "block");
        definition_b.input_class = Some("moSketchBlockDef_c".into());
        definition_b.source_id = Some("27".into());
        let objects = [&definition_a, &instance, &definition_b];
        assert!(profile_owns_intervening_sketch_blocks(
            &profile,
            objects.iter().copied()
        ));

        definition_b.input_class = Some("moRefPlane_c".into());
        let objects = [&definition_a, &instance, &definition_b];
        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            objects.iter().copied()
        ));
        definition_b.input_class = Some("moSketchBlockDef_c".into());
        let objects = [&definition_a, &instance, &definition_b];
        profile
            .properties
            .insert("DissectableChildren".into(), "23,23".into());
        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            objects.iter().copied()
        ));
    }

    #[test]
    fn closed_block_graph_binds_a_profile_without_an_explicit_child_list() {
        let profile = feature(0, "sketch");
        let mut instance = feature(1, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = Some("25".into());
        instance
            .properties
            .insert("BlockDefinition".into(), "23".into());
        let mut definition = feature(2, "block");
        definition.input_class = Some("moSketchBlockDef_c".into());
        definition.source_id = Some("23".into());

        assert!(profile_owns_intervening_sketch_blocks(
            &profile,
            [&instance, &definition]
        ));

        instance
            .properties
            .insert("BlockDefinition".into(), "24".into());
        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            [&instance, &definition]
        ));
    }

    #[test]
    fn incomplete_block_graph_does_not_bind_a_profile() {
        let profile = feature(0, "sketch");
        let mut instance = feature(1, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = Some("25".into());
        instance
            .properties
            .insert("BlockDefinition".into(), "23".into());
        let mut referenced = feature(2, "block");
        referenced.input_class = Some("moSketchBlockDef_c".into());
        referenced.source_id = Some("23".into());
        let mut unused = feature(3, "block");
        unused.input_class = Some("moSketchBlockDef_c".into());
        unused.source_id = Some("24".into());

        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            [&instance, &referenced, &unused]
        ));
        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            [&referenced]
        ));

        let mut second_instance = feature(4, "block instance");
        second_instance.input_class = Some("moSketchBlockInst_c".into());
        second_instance.source_id = Some("26".into());
        second_instance
            .properties
            .insert("BlockDefinition".into(), "23".into());
        assert!(profile_owns_intervening_sketch_blocks(
            &profile,
            [&instance, &second_instance, &referenced]
        ));
        second_instance
            .properties
            .insert("BlockDefinition".into(), "24".into());
        assert!(!profile_owns_intervening_sketch_blocks(
            &profile,
            [&instance, &second_instance, &referenced]
        ));
    }

    #[test]
    fn exact_idless_startup_binds_from_the_native_class_roster() {
        let mut features = vec![
            feature(10, "localized plane"),
            feature(11, "localized plane"),
            feature(12, "localized plane"),
            feature(13, "localized origin"),
            feature(14, "localized sketch"),
            feature(15, "localized extrusion"),
        ];
        features[4].parameters.insert("D1".into(), "88".into());
        features[5].parameters.insert("D1".into(), "20".into());
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features,
        }];
        let classes = [
            "moRefPlane_c",
            "moOriginProfileFeature_c",
            "moProfileFeature_c",
            "moExtrusion_c",
        ]
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| FeatureInputClass {
            id: format!("class-{ordinal}"),
            parent: "lane".into(),
            ordinal: ordinal as u32,
            offset: ordinal as u64 * 100,
            name: name.into(),
            role: native_object_class(name).role,
        })
        .collect();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names: Vec::new(),
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

        bind_history_classes(&mut histories, &[lane]);

        assert_eq!(
            histories[0]
                .features
                .iter()
                .map(|feature| feature.input_class.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("moRefPlane_c"),
                Some("moRefPlane_c"),
                Some("moRefPlane_c"),
                Some("moOriginProfileFeature_c"),
                Some("moProfileFeature_c"),
                Some("moExtrusion_c"),
            ]
        );
    }

    #[test]
    fn unique_idless_name_binds_to_its_direct_class_declaration() {
        let mut unique = feature(0, "localized thread");
        unique.name = "unique generated name".into();
        let mut duplicate_a = feature(1, "localized duplicate");
        duplicate_a.name = "duplicate name".into();
        let mut duplicate_b = feature(2, "localized duplicate");
        duplicate_b.name = duplicate_a.name.clone();
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![unique, duplicate_a, duplicate_b],
        }];
        let class = |ordinal: u32, offset: u64, name: &str| FeatureInputClass {
            id: format!("class-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset,
            name: name.into(),
            role: native_object_class(name).role,
        };
        let classes = vec![
            class(0, 100, "moCosmeticThread_c"),
            class(1, 200, "moRefAxis_c"),
        ];
        let name = |ordinal: u32, class: &FeatureInputClass, value: &str| FeatureInputName {
            id: format!("name-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset: class.offset + 6 + class.name.len() as u64,
            object_id: None,
            value: value.into(),
        };
        let names = vec![
            name(0, &classes[0], "unique generated name"),
            name(1, &classes[1], "duplicate name"),
        ];
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names,
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

        bind_history_classes(&mut histories, &[lane]);

        assert_eq!(
            histories[0].features[0].input_class.as_deref(),
            Some("moCosmeticThread_c")
        );
        assert_eq!(histories[0].features[1].input_class, None);
        assert_eq!(histories[0].features[2].input_class, None);
    }

    #[test]
    fn unique_idless_name_inherits_a_proven_repeated_class_token() {
        let mut direct = feature(0, "localized hole");
        direct.name = "direct hole".into();
        let mut repeated = feature(1, "localized hole");
        repeated.name = "repeated hole".into();
        let mut target = feature(2, "another localized hole");
        target.name = "target hole".into();
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![direct, repeated, target],
        }];
        let class = FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            name: "moHoleWzd_c".into(),
            role: native_object_class("moHoleWzd_c").role,
        };
        let direct_offset = class.offset + 6 + class.name.len() as u64;
        let names = vec![
            FeatureInputName {
                id: "direct-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: direct_offset,
                object_id: Some(1),
                value: "direct hole".into(),
            },
            FeatureInputName {
                id: "repeated-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: Some(2),
                value: "repeated hole".into(),
            },
            FeatureInputName {
                id: "target-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 400,
                object_id: Some(3),
                value: "target hole".into(),
            },
        ];
        let mut payload = vec![0; 500];
        payload[298..300].copy_from_slice(&0x82a4_u16.to_le_bytes());
        payload[398..400].copy_from_slice(&0x82a4_u16.to_le_bytes());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![class],
            names,
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

        bind_history_classes(&mut histories, &[lane]);

        assert!(histories[0]
            .features
            .iter()
            .all(|feature| feature.input_class.as_deref() == Some("moHoleWzd_c")));
    }

    #[test]
    fn diameter_parameter_schema_binds_a_repeated_cosmetic_thread_group() {
        let mut first = feature(0, "localized external thread");
        first.source_id = Some("11".into());
        first.parameters.insert("D1".into(), "12".into());
        first.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
        let mut second = feature(1, "localized hole thread");
        second.source_id = Some("12".into());
        second
            .parameters
            .insert("D2".into(), "&lt;MOD-DIAM&gt;6".into());
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second],
        }];
        let class = FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            name: "moCosmeticThread_c".into(),
            role: native_object_class("moCosmeticThread_c").role,
        };
        let names = histories[0]
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| FeatureInputName {
                id: format!("name-{index}"),
                parent: "lane".into(),
                ordinal: index as u32,
                offset: 300 + index as u64 * 100,
                object_id: feature.source_id.as_deref().and_then(|id| id.parse().ok()),
                value: feature.name.clone(),
            })
            .collect::<Vec<_>>();
        let mut payload = vec![0; 500];
        for name in &names {
            let offset = usize::try_from(name.offset).expect("required invariant");
            payload[offset - 2..offset].copy_from_slice(&0x82a4_u16.to_le_bytes());
        }
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![class],
            names,
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

        bind_history_classes(&mut histories, &[lane]);

        assert!(histories[0]
            .features
            .iter()
            .all(|feature| { feature.input_class.as_deref() == Some("moCosmeticThread_c") }));
    }
}
