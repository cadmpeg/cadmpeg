// SPDX-License-Identifier: Apache-2.0
//! Persistent Rhino definition, occurrence, and external-reference graph.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;

use crate::container::Scan;
use crate::instances::{DefinitionKind, LinkSource};
use crate::loss::RhinoLossCode;
use crate::settings::UnitBinding;
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
    kind: DefinitionKind,
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
    #[serde(flatten)]
    transform: OccurrenceTransform,
    parent_definition_uuids: Vec<String>,
    name: String,
    visible: bool,
    links: Vec<String>,
}

/// A native placement retains source units when physical conversion is unavailable.
#[derive(Debug, Serialize)]
#[serde(tag = "transform_units", content = "transform")]
enum OccurrenceTransform {
    #[serde(rename = "millimeter")]
    Millimeters(#[serde(serialize_with = "transform_rows")] Transform),
    #[serde(rename = "source_length_unit")]
    Source(#[serde(serialize_with = "transform_rows")] Transform),
}

fn transform_rows<S: serde::Serializer>(
    transform: &Transform,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    transform.rows().serialize(serializer)
}

impl OccurrenceTransform {
    fn from_source(source: Transform, binding: UnitBinding) -> Self {
        match binding {
            UnitBinding::Millimeters(scale) => crate::instances::scale_translation(source, scale)
                .map_or(Self::Source(source), Self::Millimeters),
            UnitBinding::Native | UnitBinding::Unavailable => Self::Source(source),
        }
    }
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

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

fn external_record(definition_uuid: Uuid, link: &LinkSource) -> Option<ExternalReferenceRecord> {
    let definition = definition_id(definition_uuid);
    let (full_path, relative_path, relative_path_preferred) = match link {
        LinkSource::None => return None,
        LinkSource::Structured(value) => {
            return Some(ExternalReferenceRecord {
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
            })
        }
        LinkSource::LegacyFull(path) => (path.as_str(), "", false),
        LinkSource::LegacyRelative {
            full_path,
            relative_path,
        } => (
            full_path.as_ref().map_or("", |path| path.as_str()),
            relative_path.as_str(),
            true,
        ),
    };
    Some(ExternalReferenceRecord {
        id: external_id(definition_uuid),
        definition_uuid: definition_uuid.to_string(),
        full_path: full_path.to_owned(),
        relative_path: relative_path.to_owned(),
        relative_path_preferred,
        byte_count: None,
        hash_time: None,
        content_time: None,
        name_sha1: None,
        content_sha1: None,
        path_status: None,
        embedded_file_uuid: None,
        links: vec![definition],
    })
}

/// Installs the source product graph without requiring occurrence expansion.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let mut object_records = BTreeMap::<Uuid, Vec<(usize, String)>>::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(identity) = object.identity() {
            object_records.entry(identity.object_id).or_default().push((
                source_order,
                format!("rhino:object:record#{source_order:06}"),
            ));
        }
    }

    let mut definitions = Vec::new();
    let mut external = Vec::new();
    for definition in &scan.definitions.definitions {
        let external_reference = external_record(definition.id, &definition.link);
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
            kind: definition.kind,
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

    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
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
        let Some(object) = object.framed() else {
            continue;
        };
        if !crate::instances::is_reference_class(object.class_uuid) {
            continue;
        }
        let identity = &object.identity;
        let reference = match crate::instances::parse_reference(
            scan.data,
            object.class_data_range.clone(),
        ) {
            Ok(reference) => reference,
            Err(error) => {
                losses.push(RhinoLossCode::ProductOccurrenceDropped.note(format!(
                    "product occurrence {} at offset {} (class {}) could not be transferred: {error}",
                    identity.source_id, object.range.start, object.class_uuid
                )).with_provenance(
                    SourceProvenance::root("rhino", object.range.start as u64).with_tag(format!(
                        "PRODUCT_OCCURRENCE/source={}/class={}", identity.source_id, object.class_uuid
                    ))
                ));
                continue;
            }
        };
        let transform = OccurrenceTransform::from_source(reference.transform, binding);
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
            parent_definition_uuids: parents,
            name: identity.name.clone(),
            visible: identity.effective_visible,
            links,
        });
    }

    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena("product_definitions", &definitions)?;
    namespace.set_arena("product_occurrences", &occurrences)?;
    namespace.set_arena("external_references", &external)?;
    Ok(losses)
}

#[cfg(test)]
mod tests {
    use super::install;
    use crate::test_support::test_dump::{
        object_record_with_payload, scan_with_objects, INSTANCE_REFERENCE_CLASS,
    };
    use cadmpeg_ir::document::CadIr;

    #[test]
    fn malformed_reference_is_reported_with_its_source_record() {
        let scan = scan_with_objects(&[object_record_with_payload(
            crate::chunks::ArchiveVersion::V5,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &[],
        )]);
        let source_offset = scan.objects[0].range().start;
        let source_id = scan.objects[0]
            .identity()
            .expect("test object identity")
            .source_id
            .clone();
        let mut ir = CadIr::empty();
        let losses = install(&scan, &mut ir).expect("other product records remain transferable");
        assert_eq!(losses.len(), 1);
        let message = &losses[0].message;
        assert!(message.contains(&source_id));
        assert!(message.contains(&format!("at offset {source_offset}")));
        assert!(message.contains("could not be transferred"));
        let provenance = losses[0]
            .provenance
            .as_ref()
            .expect("located occurrence loss");
        assert_eq!(provenance.offset, source_offset as u64);
        assert!(ir.native.namespace("rhino").unwrap().arenas()["product_occurrences"].is_empty());
    }

    #[test]
    fn complete_occurrences_keep_transform_units_and_finite_source_values_together() {
        use crate::test_support::test_dump as support;
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        let archive = crate::chunks::ArchiveVersion::V8;
        for (unit, translation, expected_translation, expected_units) in [
            (Some(2), [3.0, -4.0, 5.0], [3.0, -4.0, 5.0], "millimeter"),
            (Some(3), [3.0, -4.0, 5.0], [30.0, -40.0, 50.0], "millimeter"),
            (
                Some(0),
                [3.0, -4.0, 5.0],
                [3.0, -4.0, 5.0],
                "source_length_unit",
            ),
            (
                Some(255),
                [3.0, -4.0, 5.0],
                [3.0, -4.0, 5.0],
                "source_length_unit",
            ),
            (
                None,
                [3.0, -4.0, 5.0],
                [3.0, -4.0, 5.0],
                "source_length_unit",
            ),
            (
                Some(3),
                [f64::MAX, -4.0, 5.0],
                [f64::MAX, -4.0, 5.0],
                "source_length_unit",
            ),
        ] {
            let source = [
                [2.0, 1.0, 0.0, translation[0]],
                [0.0, 1.0, 0.0, translation[1]],
                [0.0, 0.0, 1.0, translation[2]],
                [0.0, 0.0, 0.0, 1.0],
            ];
            let record = support::object_record_with_payload(
                archive,
                0x1000,
                INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload([0x51; 16], source),
            );
            let settings = unit
                .map(|unit| vec![support::units_record(archive, unit)])
                .unwrap_or_default();
            let bytes = support::minimal_document(
                "80",
                &[
                    support::table(archive, 0x1000_0014, &[]),
                    support::table(archive, 0x1000_0015, &settings),
                    support::table(archive, 0x1000_0013, std::slice::from_ref(&record)),
                ],
            );
            let decoded = crate::RhinoCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .expect("complete occurrence decode");
            let ir: CadIr = serde_json::from_slice(
                &serde_json::to_vec(decoded.ir()).expect("occurrence CADIR serialization"),
            )
            .expect("occurrence CADIR admission");
            let namespace = ir
                .native
                .namespace("rhino")
                .expect("Rhino native namespace");
            let occurrences = &namespace.arenas()["product_occurrences"];
            assert_eq!(occurrences.len(), 1, "unit={unit:?}");
            let occurrence = serde_json::to_value(&occurrences[0]).expect("occurrence JSON");
            assert_eq!(occurrence["transform_units"], expected_units);
            assert_eq!(
                occurrence["transform"],
                serde_json::json!([
                    [2.0, 1.0, 0.0, expected_translation[0]],
                    [0.0, 1.0, 0.0, expected_translation[1]],
                    [0.0, 0.0, 1.0, expected_translation[2]],
                    [0.0, 0.0, 0.0, 1.0],
                ])
            );
            assert_eq!(
                occurrence["links"],
                serde_json::json!(["rhino:object:record#000000"])
            );
            assert_eq!(
                decoded
                    .source_fidelity()
                    .retained_record("rhino:object:record#000000")
                    .expect("retained occurrence source")
                    .data(),
                Some(record.as_slice())
            );
            // An unresolved definition is legal in the retained native graph.
            assert!(ir.model.bodies.is_empty());
        }
    }

    #[test]
    fn complete_product_recovers_after_malformed_occurrences_with_located_losses() {
        use crate::test_support::test_dump as support;
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        let archive = crate::chunks::ArchiveVersion::V8;
        let definition = [0x51; 16];
        let valid = cadmpeg_ir::transform::Transform::identity().rows();
        let mut singular = valid;
        singular[2][2] = 0.0;
        let mut nonfinite = valid;
        nonfinite[0][0] = f64::NAN;
        let objects = [
            support::object_record_with_payload(archive, 0x1000, INSTANCE_REFERENCE_CLASS, &[]),
            support::object_record_with_payload(
                archive,
                0x1000,
                INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(definition, singular),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(definition, nonfinite),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(definition, valid),
            ),
            support::object_record_with_payload(
                archive,
                1,
                support::POINT_CLASS,
                &support::point_payload([1.0, 2.0, 3.0]),
            ),
        ];
        let bytes = support::minimal_document(
            "80",
            &[
                support::table(archive, 0x1000_0014, &[]),
                support::table(archive, 0x1000_0015, &[support::units_record(archive, 2)]),
                support::table(archive, 0x1000_0013, &objects),
            ],
        );
        let scan = crate::container::scan_owned(bytes.clone()).expect("complete product framing");
        let decoded = crate::RhinoCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("later valid records recover");
        let ir: CadIr = serde_json::from_slice(
            &serde_json::to_vec(decoded.ir()).expect("product CADIR serialization"),
        )
        .expect("product CADIR admission");
        assert_eq!(ir.model.points.len(), 1);
        let records = &ir.native.namespace("rhino").unwrap().arenas()["product_occurrences"];
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].field("source_offset"),
            Some(serde_json::json!(scan.objects[3].range().start))
        );
        let losses = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == super::RhinoLossCode::ProductOccurrenceDropped.kind())
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 3);
        for (index, loss) in losses.iter().enumerate() {
            let source = scan.objects[index]
                .framed()
                .expect("framed malformed occurrence");
            let provenance = loss.provenance.as_ref().expect("located product loss");
            assert_eq!(provenance.format(), "rhino");
            assert_eq!(provenance.offset, source.range.start as u64);
            assert!(loss.message.contains(&source.identity.source_id));
            assert_eq!(
                provenance.tag.as_deref(),
                Some(
                    format!(
                        "PRODUCT_OCCURRENCE/source={}/class={}",
                        source.identity.source_id, source.class_uuid
                    )
                    .as_str()
                )
            );
            assert_eq!(
                decoded
                    .source_fidelity()
                    .retained_record(&format!("rhino:object:record#{index:06}"))
                    .expect("complete malformed source retained")
                    .data(),
                Some(objects[index].as_slice())
            );
        }
    }
}
