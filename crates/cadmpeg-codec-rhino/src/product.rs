// SPDX-License-Identifier: Apache-2.0
//! Persistent Rhino definition, occurrence, and external-reference graph.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::container::Scan;
use crate::instances::{DefinitionKind, FileReference};
use crate::wire::Uuid;

#[derive(Debug, Serialize)]
struct DefinitionRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    archive_index: Option<i32>,
    name: String,
    description: String,
    url: String,
    url_tag: String,
    kind: &'static str,
    member_object_ids: Vec<String>,
    unit_system: i32,
    meters_per_unit: f64,
    custom_unit_name: String,
    linked_depth: i32,
    linked_component_appearance: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_reference: Option<String>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct OccurrenceRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    definition_uuid: String,
    transform: [[f64; 4]; 4],
    transform_units: &'static str,
    parent_definition_uuids: Vec<String>,
    name: String,
    visible: bool,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExternalReferenceRecord {
    id: String,
    definition_uuid: String,
    full_path: String,
    relative_path: String,
    relative_path_preferred: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path_status: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedded_file_uuid: Option<String>,
    links: Vec<String>,
}

fn definition_id(id: Uuid) -> String {
    format!("rhino:product:definition#{id}")
}

fn external_id(id: Uuid) -> String {
    format!("rhino:product:external#{id}")
}

fn kind(value: DefinitionKind) -> &'static str {
    match value {
        DefinitionKind::Static => "static",
        DefinitionKind::LinkedAndEmbedded => "linked_and_embedded",
        DefinitionKind::Linked => "linked",
        DefinitionKind::Unset => "unset",
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut value, byte| {
            write!(value, "{byte:02x}").expect("writing to String cannot fail");
            value
        })
}

fn external_record(
    definition_uuid: Uuid,
    legacy_path: &str,
    legacy_relative: bool,
    value: Option<&FileReference>,
) -> Option<ExternalReferenceRecord> {
    if value.is_none() && legacy_path.is_empty() {
        return None;
    }
    let definition = definition_id(definition_uuid);
    Some(match value {
        Some(value) => ExternalReferenceRecord {
            id: external_id(definition_uuid),
            definition_uuid: definition_uuid.to_string(),
            full_path: value.full_path.clone(),
            relative_path: value.relative_path.clone(),
            relative_path_preferred: false,
            byte_count: Some(value.content_hash.byte_count),
            hash_time: Some(value.content_hash.hash_time),
            content_time: Some(value.content_hash.content_time),
            name_sha1: Some(hex(&value.content_hash.name_sha1)),
            content_sha1: Some(hex(&value.content_hash.content_sha1)),
            path_status: Some(value.path_status),
            embedded_file_uuid: value.embedded_file_id.map(|id| id.to_string()),
            links: vec![definition],
        },
        None => ExternalReferenceRecord {
            id: external_id(definition_uuid),
            definition_uuid: definition_uuid.to_string(),
            full_path: legacy_path.to_string(),
            relative_path: String::new(),
            relative_path_preferred: legacy_relative,
            byte_count: None,
            hash_time: None,
            content_time: None,
            name_sha1: None,
            content_sha1: None,
            path_status: None,
            embedded_file_uuid: None,
            links: vec![definition],
        },
    })
}

/// Installs the source product graph without requiring occurrence expansion.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let mut object_records = BTreeMap::<Uuid, Vec<(usize, String)>>::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(identity) = &object.identity {
            object_records.entry(identity.object_id).or_default().push((
                source_order,
                format!("rhino:object:record#{source_order:06}"),
            ));
        }
    }

    let mut definitions = Vec::new();
    let mut external = Vec::new();
    for definition in &scan.definitions.definitions {
        let external_reference = external_record(
            definition.id,
            &definition.legacy_linked_path,
            definition.legacy_relative_path,
            definition.file_reference.as_ref(),
        );
        let external_id = external_reference.as_ref().map(|value| value.id.clone());
        if let Some(value) = external_reference {
            external.push(value);
        }
        let mut links = definition
            .members
            .iter()
            .filter_map(|id| object_records.get(id))
            .filter(|matches| matches.len() == 1)
            .map(|matches| matches[0].1.clone())
            .collect::<Vec<_>>();
        links.extend(external_id.iter().cloned());
        links.sort();
        links.dedup();
        definitions.push(DefinitionRecord {
            id: definition_id(definition.id),
            source_offset: definition.source_range.start as u64,
            source_uuid: definition.id.to_string(),
            archive_index: definition.index,
            name: definition.name.clone(),
            description: definition.description.clone(),
            url: definition.url.clone(),
            url_tag: definition.url_tag.clone(),
            kind: kind(definition.kind),
            member_object_ids: definition.members.iter().map(ToString::to_string).collect(),
            unit_system: definition.units.unit,
            meters_per_unit: definition.units.meters_per_unit,
            custom_unit_name: definition.units.custom_name.clone(),
            linked_depth: definition.linked_depth,
            linked_component_appearance: definition.linked_appearance,
            external_reference: external_id,
            links,
        });
    }

    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|units| units.millimeters_per_unit);
    let mut member_definitions = HashMap::<Uuid, Vec<String>>::new();
    let mut definition_ids = std::collections::HashSet::new();
    for definition in &scan.definitions.definitions {
        definition_ids.insert(definition.id);
        for member in &definition.members {
            member_definitions
                .entry(*member)
                .or_default()
                .push(definition.id.to_string());
        }
    }
    for parents in member_definitions.values_mut() {
        parents.sort();
        parents.dedup();
    }
    let mut occurrences = Vec::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if !crate::instances::is_reference_class(object.class_uuid) || object.framing_degraded {
            continue;
        }
        let Ok(reference) =
            crate::instances::parse_reference(scan.data, object.class_data_range.clone())
        else {
            continue;
        };
        let Some(identity) = &object.identity else {
            continue;
        };
        let (transform, transform_units) = scale
            .and_then(|scale| crate::instances::scale_translation(reference.transform, scale))
            .map_or((reference.transform.rows, "source_length_unit"), |value| {
                (value.rows, "millimeter")
            });
        let definition = definition_id(reference.definition_id);
        let object_record = format!("rhino:object:record#{source_order:06}");
        let parents = member_definitions
            .get(&identity.object_id)
            .cloned()
            .unwrap_or_default();
        let key = if identity.object_id.is_nil()
            || object_records
                .get(&identity.object_id)
                .is_some_and(|matches| matches.len() != 1)
        {
            format!("record-{source_order:06}")
        } else {
            identity.object_id.to_string()
        };
        let mut links = vec![object_record];
        if definition_ids.contains(&reference.definition_id) {
            links.push(definition);
        }
        links.sort();
        occurrences.push(OccurrenceRecord {
            id: format!("rhino:product:occurrence#{key}"),
            source_offset: object.range.start as u64,
            source_uuid: identity.object_id.to_string(),
            definition_uuid: reference.definition_id.to_string(),
            transform,
            transform_units,
            parent_definition_uuids: parents,
            name: identity.name.clone(),
            visible: identity.effective_visible,
            links,
        });
    }

    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("product_definitions", &definitions)
        .expect("Rhino definitions serialize");
    namespace
        .set_arena("product_occurrences", &occurrences)
        .expect("Rhino occurrences serialize");
    namespace
        .set_arena("external_references", &external)
        .expect("Rhino external references serialize");
}
