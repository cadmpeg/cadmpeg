// SPDX-License-Identifier: Apache-2.0
//! Author persistent native selections from their typed document records.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::BodyRetentionMode;

use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    Feature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputGeneratedSurfaceIdentity,
    FeatureInputLane, FeatureInputSurfaceSelectionKind,
};

/// A newly authored segment in the lane's declared typed content order.
pub(super) struct Event {
    pub(super) sequence: u32,
    pub(super) bytes: Vec<u8>,
    pub(super) records: Vec<(String, String)>,
}

/// A typed record whose required references or authored controls are unavailable.
pub(super) struct Omission {
    pub(super) arena: &'static str,
    pub(super) id: String,
    pub(super) reason: String,
}

/// Authored segments and individual records that could not be represented.
#[derive(Default)]
pub(super) struct Output {
    pub(super) events: Vec<Event>,
    pub(super) omitted: Vec<Omission>,
}

/// The caller reconciles neutral counterparts before passing this native lane.
/// No retained lane bytes participate in the reconstruction.
pub(super) fn events(
    lane: &FeatureInputLane,
    histories: &[FeatureHistory],
) -> Result<Output, CodecError> {
    let features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<BTreeMap<_, _>>();
    if features.len()
        != histories
            .iter()
            .map(|history| history.features.len())
            .sum::<usize>()
    {
        // (C): repeated record identities do not define a consistent native graph.
        return Err(CodecError::malformed(
            "SLDPRT selection writer received duplicate native feature identities",
        ));
    }
    let mut output = Output::default();
    let mut declared = BTreeSet::new();
    for record in &lane.body_selections {
        let mut bytes = Vec::new();
        let mut records = vec![("feature_input_body_selections".into(), record.id.clone())];
        let mut next_declared = declared.clone();
        let result = (|| {
            let feature = owner(
                lane,
                &features,
                &record.object_name_ref,
                &record.feature_ref,
            )?;
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::Operation(crate::classification::FeatureClass::MoveBody)
            {
                if !record.body_state_ids.is_empty() || record.mode.is_some() {
                    return Err(
                        "body-motion selection carries incompatible delete-body state".into(),
                    );
                }
                if feature.parameters.contains_key("Rotation")
                    || feature
                        .properties
                        .get("Copies")
                        .is_some_and(|value| value != "0")
                {
                    return Err("native body-motion rotation or copy mode has no decoded typed motion carrier".into());
                }
                let translation = feature
                    .properties
                    .get("Translation")
                    .and_then(|value| crate::history::literals::parse_vector3_mm(value))
                    .ok_or("body-motion owner has no typed translation")?
                    .get();
                declare(
                    lane,
                    &mut next_declared,
                    "moMoveCopyBodyData_c",
                    &mut bytes,
                    &mut records,
                )?;
                bytes.extend(count(record.local_body_ids.len())?.to_le_bytes());
                for id in &record.local_body_ids {
                    bytes.extend(id.to_le_bytes());
                }
                bytes.extend(u32::MAX.to_le_bytes());
                bytes.extend([0; 8]);
                let mut motion = [0; 208];
                for index in [0, 4, 8] {
                    motion[index * 8..index * 8 + 8].copy_from_slice(&1.0f64.to_le_bytes());
                }
                motion[72..80].copy_from_slice(&1u64.to_le_bytes());
                motion[104..112].copy_from_slice(&1.0f64.to_le_bytes());
                for (index, value) in [translation.x, translation.y, translation.z]
                    .into_iter()
                    .enumerate()
                {
                    motion[112 + index * 8..120 + index * 8]
                        .copy_from_slice(&(value / 1000.0).to_le_bytes());
                }
                motion[200..].copy_from_slice(&[1, 0, 0, 0, 0, 0, 1, 0]);
                bytes.extend(motion);
                return Ok(());
            }
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::DeleteBody
                && !feature.kind.eq_ignore_ascii_case("DeleteBody")
            {
                return Err("body selection requires an authored move/copy transform that is not held by this selection record".into());
            }
            if record.mode.is_some() && record.body_state_ids.is_empty() {
                return Err("delete-body retention mode has no body-state records".into());
            }
            if !record.body_state_ids.is_empty() {
                declare(
                    lane,
                    &mut next_declared,
                    "moDeleteBodyData_c",
                    &mut bytes,
                    &mut records,
                )?;
                bytes.extend([8, 0]);
                for id in &record.body_state_ids {
                    bytes.extend(0x8100u16.to_le_bytes());
                    bytes.extend([0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0]);
                    bytes.extend(id.to_le_bytes());
                    bytes.extend(id.to_le_bytes());
                    bytes.extend([0; 28]);
                    bytes.extend([0xff; 16]);
                    bytes.extend([0; 20]);
                }
                if let Some(mode) = record.mode {
                    bytes.extend([0x30, 0x80]);
                    bytes.extend(
                        match mode {
                            BodyRetentionMode::KeepSelected => 0u32,
                            BodyRetentionMode::DeleteSelected => 1u32,
                            BodyRetentionMode::Unresolved => {
                                return Err("delete-body retention mode is unresolved".into())
                            }
                        }
                        .to_le_bytes(),
                    );
                    bytes.extend([0; 4]);
                }
            }
            bytes.extend(11000u32.to_le_bytes());
            bytes.extend([0; 8]);
            bytes.extend(count(record.local_body_ids.len())?.to_le_bytes());
            for id in &record.local_body_ids {
                bytes.extend(id.to_le_bytes());
            }
            bytes.extend(u32::MAX.to_le_bytes());
            bytes.extend([0; 12]);
            Ok(())
        })();
        if result.is_ok() {
            declared = next_declared;
        }
        output.record(
            "feature_input_body_selections",
            &record.id,
            record.sequence,
            bytes,
            records,
            result,
        );
    }
    for record in &lane.edge_selections {
        let mut bytes = Vec::new();
        let mut records = vec![("feature_input_edge_selections".into(), record.id.clone())];
        let mut next_declared = declared.clone();
        let result = (|| {
            owner(
                lane,
                &features,
                &record.object_name_ref,
                &record.feature_ref,
            )?;
            resolve_features(
                &features,
                &record.producer_feature_refs,
                record.terminal_feature_ref.as_deref(),
            )?;
            declare(
                lane,
                &mut next_declared,
                "moCompEdge_c",
                &mut bytes,
                &mut records,
            )?;
            bytes.extend([0xcb, 0x80]);
            if !record.references.is_empty() {
                if let Some(terminal) = record.terminal_feature_ref.as_deref() {
                    let terminal_source = features
                        .get(terminal)
                        .and_then(|feature| feature.source_value());
                    let represented = record
                        .references
                        .iter()
                        .flatten()
                        .rev()
                        .filter_map(|entry| component_source(Some(entry)))
                        .find(|source| {
                            features
                                .values()
                                .any(|feature| feature.source_value() == Some(*source))
                        });
                    if terminal_source.is_none() || terminal_source != represented {
                        return Err(
                            "reference-list edge terminal feature disagrees with its typed paths"
                                .into(),
                        );
                    }
                }
                reference_list(&mut bytes, &record.references, &record.local_edge_ids)?;
            } else if !record.components.is_empty() {
                if record.terminal_feature_ref.as_deref().is_some_and(|id| {
                    features
                        .get(id)
                        .and_then(|feature| feature.source_value())
                        .is_none()
                }) {
                    return Err("terminal edge feature has no native source identity".into());
                }
                let terminal = record
                    .terminal_feature_ref
                    .as_deref()
                    .and_then(|id| features.get(id))
                    .and_then(|feature| feature.source_value());
                let implicit = component_source(record.components.last());
                let extra_terminal = terminal.filter(|source| Some(*source) != implicit);
                vector_header(
                    &mut bytes,
                    record.components.len() + usize::from(extra_terminal.is_some()),
                    0,
                    0,
                )?;
                component_path(&mut bytes, &record.components)?;
                if let Some(source) = extra_terminal {
                    bytes.extend([1, 0, 0, 0, 0, 0, 0, 0, 0x4a, 0x80, 0, 0, 1, 0, 0x37, 0]);
                    bytes.extend(source.to_le_bytes());
                    bytes.extend(1u32.to_le_bytes());
                    bytes.extend([0; 12]);
                }
                let local = record
                    .components
                    .iter()
                    .filter_map(|entry| entry.local_id)
                    .collect::<Vec<_>>();
                if !record.local_edge_ids.is_empty() && record.local_edge_ids != local {
                    return Err("edge roster disagrees with its typed component path".into());
                }
            } else {
                vector_header(&mut bytes, record.local_edge_ids.len(), 0, 0)?;
                for id in &record.local_edge_ids {
                    let id = u16::try_from(*id).map_err(|_| {
                        "edge ID has no typed component identity and exceeds the compact u16 form"
                    })?;
                    if id == 0 {
                        return Err("compact edge ID is zero".into());
                    }
                    bytes.extend(id.to_le_bytes());
                }
                bytes.extend([0; 16]);
                bytes.extend([0xff, 0xfe, 0xff]);
            }
            Ok(())
        })();
        if result.is_ok() {
            declared = next_declared;
        }
        output.record(
            "feature_input_edge_selections",
            &record.id,
            record.sequence,
            bytes,
            records,
            result,
        );
    }
    for record in &lane.surface_selections {
        let mut bytes = Vec::new();
        let mut records = vec![("feature_input_surface_selections".into(), record.id.clone())];
        let mut next_declared = declared.clone();
        let result = (|| {
            let feature = owner(
                lane,
                &features,
                &record.object_name_ref,
                &record.feature_ref,
            )?;
            resolve_features(
                &features,
                &record.producer_feature_refs,
                record.terminal_feature_ref.as_deref(),
            )?;
            let endpoint = match record.kind {
                FeatureInputSurfaceSelectionKind::Component => None,
                FeatureInputSurfaceSelectionKind::ExtrusionEndpoint { endpoint_selector } => {
                    Some(endpoint_selector)
                }
            };
            let is_extrusion = matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut")
                || feature.kind.eq_ignore_ascii_case("Extrude")
                || feature
                    .input_class
                    .as_deref()
                    .is_some_and(|name| name.contains("Extrud"));
            if is_extrusion {
                end_spec_header(&mut bytes, if endpoint.is_some() { 3 } else { 4 });
                if endpoint.is_some() {
                    append_class(&mut bytes, "moEndPointRef_w")?;
                    append_class(&mut bytes, "moCompEdge_c")?;
                    consume(lane, "moEndPointRef_w", &mut records);
                    consume(lane, "moCompEdge_c", &mut records);
                    next_declared.insert("moCompEdge_c");
                    bytes.extend([0xcb, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
                    bytes.extend([0; 12]);
                } else {
                    bytes.extend([1, 1, 0]);
                    append_class(&mut bytes, "moSingleFaceRef_w")?;
                    consume(lane, "moSingleFaceRef_w", &mut records);
                    bytes.extend([0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
                }
            } else if feature
                .input_class
                .as_deref()
                .is_some_and(|name| name.contains("Thicken"))
            {
                declare(
                    lane,
                    &mut next_declared,
                    "moCompSurfaceBody_c",
                    &mut bytes,
                    &mut records,
                )?;
                bytes.extend(0x8101u16.to_le_bytes());
                // The component-surface wrapper places its vector marker at 103.
                bytes.extend([0; 89]);
            } else if !matches!(
                native_object_class(feature.input_class.as_deref().unwrap_or_default()),
                NativeClassKind::MirrorPattern | NativeClassKind::PlanarSurface
            ) {
                append_class(&mut bytes, "moCompFace_c")?;
                consume(lane, "moCompFace_c", &mut records);
                bytes.extend(0x8102u16.to_le_bytes());
                bytes.extend(2u32.to_le_bytes());
                bytes.extend([0; 50]);
            }
            vector_header(
                &mut bytes,
                record.components.len(),
                record.selector,
                endpoint.unwrap_or(0),
            )?;
            component_path(&mut bytes, &record.components)?;
            if let Some(terminal) = record.terminal_feature_ref.as_deref() {
                let source = features
                    .get(terminal)
                    .and_then(|feature| feature.source_value());
                if source != component_source(record.components.last()) {
                    return Err(
                        "surface terminal feature has no matching typed terminal component".into(),
                    );
                }
            }
            Ok(())
        })();
        if result.is_ok() {
            declared = next_declared;
        }
        output.record(
            "feature_input_surface_selections",
            &record.id,
            record.sequence,
            bytes,
            records,
            result,
        );
    }
    for record in &lane.generated_surface_identities {
        let mut bytes = Vec::new();
        let mut records = vec![(
            "feature_input_generated_surface_identities".into(),
            record.id.clone(),
        )];
        let result = generated(lane, &features, record, &mut bytes, &mut records);
        output.record(
            "feature_input_generated_surface_identities",
            &record.id,
            record.sequence,
            bytes,
            records,
            result,
        );
    }
    Ok(output)
}

impl Output {
    fn record(
        &mut self,
        arena: &'static str,
        id: &str,
        sequence: u32,
        bytes: Vec<u8>,
        records: Vec<(String, String)>,
        result: Result<(), String>,
    ) {
        match result {
            Ok(()) => self.events.push(Event {
                sequence,
                bytes,
                records,
            }),
            Err(reason) => self.omitted.push(Omission {
                arena,
                id: id.into(),
                reason,
            }),
        }
    }
}

fn owner<'a>(
    lane: &FeatureInputLane,
    features: &BTreeMap<&str, &'a Feature>,
    name: &str,
    feature: &str,
) -> Result<&'a Feature, String> {
    if !lane.names.iter().any(|record| record.id == name) {
        return Err("owning object-name record no longer resolves".into());
    }
    features
        .get(feature)
        .copied()
        .ok_or_else(|| "owning native feature no longer resolves".into())
}

fn resolve_features(
    features: &BTreeMap<&str, &Feature>,
    producers: &[String],
    terminal: Option<&str>,
) -> Result<(), String> {
    if producers
        .iter()
        .map(String::as_str)
        .chain(terminal)
        .any(|id| !features.contains_key(id))
    {
        return Err("persistent selection references an absent native feature".into());
    }
    Ok(())
}

fn count(value: usize) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "native selection count exceeds its u32 field".into())
}

fn vector_header(
    bytes: &mut Vec<u8>,
    size: usize,
    selector: u8,
    endpoint: u32,
) -> Result<(), String> {
    if !(1..=64).contains(&size) {
        return Err("selection path is outside the decoded compact vector profile (1..=64)".into());
    }
    bytes.extend(count(size)?.to_le_bytes());
    bytes.extend([selector, 2, 0, 0]);
    bytes.extend(endpoint.to_le_bytes());
    bytes.extend(super::selections::COMPACT_EDGE_VECTOR_MARKER);
    bytes.extend([0, 0]);
    Ok(())
}

fn component_source(component: Option<&FeatureInputComponentPathEntry>) -> Option<u32> {
    component
        .and_then(|entry| View::u32_le_at(entry.type_signature.as_ref(), 4))
        .filter(|source| *source != 0 && *source != u32::MAX)
}

fn valid_signature(entry: &FeatureInputComponentPathEntry) -> bool {
    let signature = entry.type_signature.as_ref();
    View::u16_le_at(signature, 0).is_some_and(super::is_class_token)
        && View::u16_le_at(signature, 2).is_some_and(|value| value != 0)
        && View::u32_le_at(signature, 4).is_some_and(|value| value != 0)
        && View::u32_le_at(signature, 8).is_some_and(|value| value != 0)
}

fn component_path(
    bytes: &mut Vec<u8>,
    entries: &[FeatureInputComponentPathEntry],
) -> Result<(), String> {
    if entries.is_empty() {
        return Err("persistent selection has no typed component path".into());
    }
    for (index, entry) in entries.iter().enumerate() {
        if entry.type_signature[..2] == [0, 0] {
            return Err("component type identity is invalid".into());
        }
        if (entry.instance.is_none() || entry.local_id.is_none()) && !valid_signature(entry) {
            return Err("anonymous or nonterminal component has an invalid type identity".into());
        }
        if let Some(instance) = entry.instance {
            if !super::is_class_token(instance) {
                return Err("component instance is not a native class token".into());
            }
            bytes.extend(instance.to_le_bytes());
            bytes.extend([0, 0]);
        }
        bytes.extend(entry.type_signature.as_ref());
        match entry.local_id {
            Some(id) => bytes.extend(id.to_le_bytes()),
            None if index + 1 < entries.len() && entry.instance.is_some() => {}
            None => return Err("component path has no encodable terminal local ID".into()),
        }
    }
    Ok(())
}

fn reference_list(
    bytes: &mut Vec<u8>,
    references: &[Vec<FeatureInputComponentPathEntry>],
    local_ids: &[u32],
) -> Result<(), String> {
    vector_header(bytes, references.len(), 0, 0)?;
    let prefix = references
        .first()
        .and_then(|path| path.first())
        .map(|entry| &entry.type_signature[..4])
        .ok_or("edge reference list has an empty path")?;
    let mut actual_ids = Vec::new();
    for path in references {
        if path.is_empty() {
            return Err("edge reference list has an empty path".into());
        }
        for (index, entry) in path.iter().enumerate() {
            if !valid_signature(entry) || entry.type_signature[..4] != *prefix {
                return Err(
                    "edge reference list contains incompatible component type families".into(),
                );
            }
            let instance = entry
                .instance
                .filter(|token| super::is_class_token(*token))
                .ok_or("reference-list component has no class instance")?;
            if index + 1 < path.len() && entry.local_id.is_some() {
                return Err("nonterminal reference-list component carries a local ID".into());
            }
            bytes.extend(instance.to_le_bytes());
            bytes.extend([0, 0]);
            bytes.extend(entry.type_signature.as_ref());
        }
        let id = path
            .last()
            .and_then(|entry| entry.local_id)
            .ok_or("reference-list terminal has no local ID")?;
        actual_ids.push(id);
        bytes.extend(id.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes());
    }
    if !local_ids.is_empty() && local_ids != actual_ids {
        return Err("edge roster disagrees with the persistent reference list".into());
    }
    Ok(())
}

fn end_spec_header(bytes: &mut Vec<u8>, kind: u8) {
    let mut header = [0; 30];
    header[..2].copy_from_slice(&0x8e0cu16.to_le_bytes());
    header[4] = 1;
    header[18] = kind;
    bytes.extend(header);
}

fn generated(
    lane: &FeatureInputLane,
    features: &BTreeMap<&str, &Feature>,
    record: &FeatureInputGeneratedSurfaceIdentity,
    bytes: &mut Vec<u8>,
    records: &mut Vec<(String, String)>,
) -> Result<(), String> {
    if !features
        .values()
        .any(|feature| feature.source_value() == Some(record.feature_source_id.value()))
    {
        return Err("generated surface producer no longer resolves".into());
    }
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name.ends_with("SurfIdRep_c"));
    let class = classes
        .next()
        .ok_or("generated surface has no typed surface-identity class")?;
    if classes.any(|other| other.name != class.name) {
        return Err("generated surface type family has no unambiguous class relationship".into());
    }
    let prefix = record.type_prefix.as_ref();
    if record.components.is_empty() {
        return Err("generated surface has no typed components".into());
    }
    let first = &record.components[0];
    let reporting = record
        .components
        .iter()
        .enumerate()
        .find(|(index, entry)| {
            component_source(Some(entry)).is_some()
                && record
                    .components
                    .get(index + 1)
                    .is_none_or(|next| component_source(Some(next)).is_none())
        })
        .ok_or("generated surface has no non-absent producer component")?;
    if component_source(Some(reporting.1)) != Some(record.feature_source_id.value()) {
        return Err("generated surface producer disagrees with its typed path".into());
    }
    let identity_tail = if let Some(next) = record.components.get(reporting.0 + 1) {
        u32::from(
            next.instance
                .ok_or("generated surface continuation has no instance")?,
        )
    } else {
        reporting
            .1
            .local_id
            .ok_or("generated surface terminal has no local ID")?
    };
    if identity_tail != record.local_identity {
        return Err("generated surface local identity disagrees with its typed path".into());
    }
    append_class(bytes, &class.name)?;
    consume(lane, &class.name, records);
    bytes.extend([0, 0]);
    // Declare the family separately when the first path node has an instance.
    if first.instance.is_some() {
        bytes.extend(prefix);
        bytes.extend([0; 12]);
    }
    for (index, entry) in record.components.iter().enumerate() {
        if !valid_signature(entry) || entry.type_signature[..4] != *prefix {
            return Err("generated surface contains an incompatible component family".into());
        }
        if let Some(instance) = entry.instance {
            if !super::is_class_token(instance) {
                return Err("generated surface instance is not a class token".into());
            }
            bytes.extend(instance.to_le_bytes());
            bytes.extend([0, 0]);
        } else if index != 0 {
            return Err("generated surface continuation has no instance".into());
        }
        bytes.extend(entry.type_signature.as_ref());
        match (index + 1 == record.components.len(), entry.local_id) {
            (true, Some(id)) => bytes.extend(id.to_le_bytes()),
            (false, None) => {}
            _ => return Err("generated surface path has inconsistent terminal local IDs".into()),
        }
    }
    Ok(())
}

fn append_class(bytes: &mut Vec<u8>, name: &str) -> Result<(), String> {
    let length =
        u16::try_from(name.len()).map_err(|_| "native class name exceeds its u16 field")?;
    bytes.extend(super::CLASS_MARKER);
    bytes.extend(length.to_le_bytes());
    bytes.extend(name.as_bytes());
    Ok(())
}

fn consume(lane: &FeatureInputLane, name: &str, records: &mut Vec<(String, String)>) {
    records.extend(
        lane.classes
            .iter()
            .filter(|class| class.name == name)
            .map(|class| ("feature_input_classes".into(), class.id.clone())),
    );
}

fn declare(
    lane: &FeatureInputLane,
    declared: &mut BTreeSet<&'static str>,
    name: &'static str,
    bytes: &mut Vec<u8>,
    records: &mut Vec<(String, String)>,
) -> Result<(), String> {
    if declared.insert(name) {
        append_class(bytes, name)?;
    }
    consume(lane, name, records);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{FeatureInputBodySelection, FeatureInputClass, FeatureInputName};
    use crate::resolved_features::class_sources::ClassSources;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    fn component(
        instance: Option<u16>,
        source: u32,
        identity: u32,
        local: Option<u32>,
    ) -> FeatureInputComponentPathEntry {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&[0xc3, 0x80, 0xc5, 0]);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        FeatureInputComponentPathEntry {
            instance,
            type_signature: signature.into(),
            local_id: local,
        }
    }

    fn lane() -> FeatureInputLane {
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new().into(),
            classes: Vec::new(),
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
        }
    }

    fn feature() -> Feature {
        serde_json::from_value(serde_json::json!({
            "id":"feature", "parent":"history", "source_id":"89", "ordinal":0,
            "name":"Body-Delete/Keep 1", "kind":"DeleteBody", "input_class":"moDeleteBody_c"
        }))
        .expect("authored feature")
    }

    #[test]
    fn mixed_component_path_round_trips_all_identity_fields() {
        let entries = vec![
            component(Some(0x8103), 89, 1, None),
            component(Some(0x8104), 90, 2, Some(777)),
        ];
        let mut bytes = Vec::new();
        vector_header(&mut bytes, entries.len(), 0, 0).expect("vector header");
        component_path(&mut bytes, &entries).expect("typed path");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            super::super::selections::compact_edge_component_path_at(&ctx, &bytes, 12)
                .expect("parse path"),
            Some(entries)
        );
        assert_eq!(
            super::super::selections::compact_edge_selection_at(&ctx, &bytes, 12)
                .expect("parse IDs"),
            Some(vec![777])
        );
    }

    #[test]
    fn reference_lists_keep_each_path_and_terminal_id() {
        let paths = vec![
            vec![
                component(Some(0x8103), 89, 1, None),
                component(Some(0x8104), 90, 2, Some(123)),
            ],
            vec![component(Some(0x8105), 91, 3, Some(456))],
        ];
        let mut bytes = Vec::new();
        reference_list(&mut bytes, &paths, &[123, 456]).expect("typed references");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            super::super::selections::compact_edge_selection_at(&ctx, &bytes, 12)
                .expect("parse IDs"),
            Some(vec![123, 456])
        );
        assert_eq!(
            super::super::selections::compact_edge_component_path_at(&ctx, &bytes, 12)
                .expect("parse paths"),
            Some(paths.into_iter().flatten().collect())
        );
    }

    #[test]
    fn generated_identity_uses_typed_path_without_source_bytes() {
        let mut lane = lane();
        lane.classes.push(FeatureInputClass {
            feature_ref: None,
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,

            name: "moWzdHoleSurfIdRep_c".into(),
        });
        let record = FeatureInputGeneratedSurfaceIdentity {
            id: "identity".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,
            type_prefix: [0xc3, 0x80, 0xc5, 0].into(),
            feature_source_id: 89u32.try_into().expect("producer"),
            local_identity: 0x85b5,
            components: vec![
                component(None, 89, 1, None),
                component(Some(0x85b5), u32::MAX, 2, Some(3)),
            ],
        };
        let feature = feature();
        let features = BTreeMap::from([("feature", &feature)]);
        let mut bytes = Vec::new();
        let mut records = Vec::new();
        generated(&lane, &features, &record, &mut bytes, &mut records).expect("native identity");
        assert_eq!(
            records,
            vec![("feature_input_classes".into(), "class".into())]
        );
        lane.native_payload = bytes.into();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, source) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let classes = super::super::names::class_declarations(source.window(), &lane.id).unwrap();
        let class_sources = ClassSources::from_source(&ctx, source, &classes).unwrap();
        assert_eq!(class_sources.position(&classes[0]), Some(0));
        let output = super::super::selections::generated_surface_identities(
            &class_sources,
            &lane.id,
            &classes,
            &lane.native_payload,
        )
        .expect("re-read identity")
        .records;
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].components, record.components);
        assert_eq!(output[0].feature_source_id, record.feature_source_id);
        assert_eq!(output[0].local_identity, record.local_identity);
    }

    #[test]
    fn delete_body_states_and_mode_round_trip_without_retained_payload() {
        let mut lane = lane();
        lane.names.push(FeatureInputName {
            feature_ref: None,
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,
            offset: Some(0),
            object_id: None,
            value: "Body-Delete/Keep 1".into(),
        });
        lane.body_selections.push(FeatureInputBodySelection {
            id: "body-selection".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,
            object_name_ref: "name".into(),
            feature_ref: "feature".into(),
            local_body_ids: vec![287, 115],
            body_state_ids: vec![287],
            mode: Some(BodyRetentionMode::DeleteSelected),
        });
        let histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature()],
        }];
        let output = events(&lane, &histories).expect("native selections");
        assert!(output.omitted.is_empty());
        assert_eq!(output.events.len(), 1);
        lane.native_payload = output
            .events
            .into_iter()
            .next()
            .expect("event")
            .bytes
            .into();
        lane.classes.push(FeatureInputClass {
            feature_ref: None,
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,

            name: "moDeleteBodyData_c".into(),
        });
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("authored source");
        let mut body_sources =
            super::super::body_sources::BodySources::from_views([(lane.id.clone(), root)])
                .expect("lane source");
        let decoded = super::super::selections::compact_body_selections(
            &crate::test_support::native::class_sources(std::slice::from_ref(&lane)),
            &ctx,
            root,
            &mut body_sources,
            &histories,
            &lane,
        )
        .expect("re-read body selection");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].local_body_ids, vec![287, 115]);
        assert_eq!(decoded[0].body_state_ids, vec![287]);
        assert_eq!(decoded[0].mode, Some(BodyRetentionMode::DeleteSelected));
    }
    fn surface_lane(
        kind: FeatureInputSurfaceSelectionKind,
        class: &str,
        feature_kind: &str,
    ) -> (FeatureInputLane, Vec<FeatureHistory>) {
        let mut lane = lane();
        let mut feature = feature();
        feature.input_class = Some(class.into());
        feature.kind = feature_kind.into();
        lane.names.push(FeatureInputName {
            feature_ref: None,
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,
            offset: Some(0),
            object_id: None,
            value: feature.name.clone(),
        });
        lane.surface_selections
            .push(crate::records::FeatureInputSurfaceSelection {
                id: "surface".into(),
                parent: "lane".into(),
                ordinal: 0,
                sequence: 0,
                offset: Some(400),
                selector: 4,
                kind,
                object_name_ref: "name".into(),
                feature_ref: "feature".into(),
                producer_feature_refs: Vec::new(),
                terminal_feature_ref: None,
                components: vec![component(Some(0x8104), 89, 2, Some(7))],
            });
        let histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature],
        }];
        (lane, histories)
    }

    #[test]
    fn extrusion_endpoint_wrapper_round_trips_the_authored_selector() {
        let (lane, histories) = surface_lane(
            FeatureInputSurfaceSelectionKind::ExtrusionEndpoint {
                endpoint_selector: 0x0012_3456,
            },
            "moExtrusion_c",
            "Extrude",
        );
        let output = events(&lane, &histories).expect("selection events");
        assert!(output.omitted.is_empty());
        let bytes = &output.events[0].bytes;
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (marker, kind) =
            super::super::terminations::compact_extrusion_to_vertex_at(&ctx, bytes, 0, bytes.len())
                .expect("read wrapper")
                .expect("endpoint reference");
        assert_eq!(kind.endpoint_selector(), Some(0x0012_3456));
        let expected = lane.surface_selections[0].components.clone();
        assert_eq!(
            super::super::terminations::compact_termination_reference_path_at(&ctx, bytes, marker)
                .expect("read path"),
            Some(expected)
        );
    }

    #[test]
    fn component_face_wrapper_recovers_the_owned_surface_selection() {
        let (mut lane, histories) = surface_lane(
            FeatureInputSurfaceSelectionKind::Component,
            "moShell_c",
            "Shell",
        );
        let output = events(&lane, &histories).expect("selection events");
        assert!(output.omitted.is_empty());
        lane.native_payload = output
            .events
            .into_iter()
            .next()
            .expect("event")
            .bytes
            .into();
        lane.classes.push(FeatureInputClass {
            feature_ref: None,
            id: "face-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            sequence: 0,

            name: "moCompFace_c".into(),
        });
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let actual = super::super::selections::compact_surface_selections(
            &crate::test_support::native::class_sources(std::slice::from_ref(&lane)),
            &ctx,
            &histories,
            &lane,
        )
        .expect("read owned surface selection");
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].components, lane.surface_selections[0].components);
        assert_eq!(actual[0].selector, 4);
        assert_eq!(actual[0].feature_ref, "feature");
    }
}
