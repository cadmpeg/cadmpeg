// SPDX-License-Identifier: Apache-2.0
//! Persistent Rhino definition, occurrence, and external-reference graph.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;

use crate::container::Scan;
use crate::instances::{hex, DefinitionKind, LinkSource, UnitDetail};
use crate::loss::RhinoLossCode;
use crate::settings::{MillimeterScale, UnitBinding};
use crate::wire::Uuid;

#[derive(Debug, Serialize)]
struct DefinitionRecord<'a> {
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
    #[serde(flatten)]
    units: &'a UnitDetail,
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

/// A native placement retains source units when the scaled translation is not finite.
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
        crate::instances::scale_translation(source, binding.model_scale())
            .map_or(Self::Source(source), Self::Millimeters)
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

fn external_record(definition_uuid: Uuid, link: &LinkSource) -> Option<ExternalReferenceRecord> {
    if matches!(link, LinkSource::None) {
        return None;
    }
    let definition = definition_id(definition_uuid);
    let links = vec![definition];
    let (full_path, relative_path, relative_path_preferred) = match link {
        LinkSource::None => return None,
        LinkSource::Structured(value) => {
            return Some(ExternalReferenceRecord {
                id: external_id(definition_uuid),
                definition_uuid: format!("{definition_uuid}"),
                full_path: value.full_path.clone(),
                relative_path: value.relative_path.clone(),
                relative_path_preferred: false,
                byte_count: Some(value.content_hash.byte_count),
                hash_time: Some(value.content_hash.hash_time),
                content_time: Some(value.content_hash.content_time),
                name_sha1: Some(hex(&value.content_hash.name_sha1)),
                content_sha1: Some(hex(&value.content_hash.content_sha1)),
                path_status: Some(value.path_status),
                embedded_file_uuid: value.embedded_file_id.map(|id| format!("{id}")),
                links,
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
        definition_uuid: format!("{definition_uuid}"),
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
        links,
    })
}

/// Installs the source product graph without requiring occurrence expansion.
pub(crate) fn install(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let mut object_records = HashMap::<Uuid, Vec<(usize, String)>>::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(identity) = object.identity() {
            let rows = object_records.entry(identity.object_id).or_default();

            rows.push((
                source_order,
                format!("rhino:object:record#{source_order:06}"),
            ));
        }
    }

    let mut definitions = Vec::new();
    let mut external = Vec::new();
    for definition in scan.definitions.definitions() {
        let external_reference = external_record(definition.id(), &definition.link);
        let external_id = external_reference.as_ref().map(|value| value.id.clone());
        if let Some(value) = external_reference {
            external.push(value);
        }
        let mut links = Vec::new();
        for matches in definition
            .members
            .iter()
            .filter_map(|id| object_records.get(id))
            .filter(|matches| matches.len() == 1)
        {
            links.push(matches[0].1.clone());
        }
        if let Some(id) = &external_id {
            links.push(id.to_owned());
        }
        links.sort();
        links.dedup();
        let mut member_object_ids = Vec::new();
        for id in &definition.members {
            member_object_ids.push(format!("{id}"));
        }

        definitions.push(DefinitionRecord {
            id: definition_id(definition.id()),
            source_offset: cadmpeg_core::decode::u64_from_index(definition.source_range.start),
            source_uuid: format!("{}", definition.id()),
            archive_index: definition.index,
            name: definition.name.clone(),
            description: definition.description.clone(),
            url: definition.url.clone(),
            url_tag: definition.url_tag.clone(),
            kind: definition.kind,
            member_object_ids,
            units: &definition.units,
            linked_depth: definition.linked_depth,
            linked_component_appearance: definition.linked_appearance,
            external_reference: external_id,
            links,
        });
    }

    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
    let mut member_definitions = HashMap::<Uuid, Vec<String>>::new();
    let mut definition_ids = HashSet::new();
    for definition in scan.definitions.definitions() {
        definition_ids.insert(definition.id());
        for member in &definition.members {
            let parents = member_definitions.entry(*member).or_default();

            parents.push(format!("{}", definition.id()));
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
                let loss = RhinoLossCode::ProductOccurrenceDropped.note(format_args!(
                    "product occurrence {} at offset {} (class {}) could not be transferred: {error}",
                    identity.source_id, object.range.start, object.class_uuid
                ));
                let tag = format!(
                    "PRODUCT_OCCURRENCE/source={}/class={}",
                    identity.source_id, object.class_uuid
                );
                losses.push(
                    loss.with_provenance(
                        SourceProvenance::root(
                            "rhino",
                            cadmpeg_core::decode::u64_from_index(object.range.start),
                        )
                        .with_tag(tag),
                    ),
                );
                continue;
            }
        };
        let transform = OccurrenceTransform::from_source(reference.transform(), binding);
        let definition = definition_id(reference.definition_id());
        let object_record = format!("rhino:object:record#{source_order:06}");
        let mut parents = Vec::new();
        if let Some(source_parents) = member_definitions.get(&identity.object_id) {
            for parent in source_parents {
                parents.push(parent.to_owned());
            }
        }
        let key = if identity.object_id.is_nil()
            || object_records
                .get(&identity.object_id)
                .is_some_and(|matches| matches.len() != 1)
        {
            format!("record-{source_order:06}")
        } else {
            format!("{}", identity.object_id)
        };
        let mut links = Vec::new();
        links.push(object_record);
        if definition_ids.contains(&reference.definition_id()) {
            links.push(definition);
        }
        links.sort();

        occurrences.push(OccurrenceRecord {
            id: format!("rhino:product:occurrence#{key}"),
            source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
            source_uuid: format!("{}", identity.object_id),
            definition_uuid: format!("{}", reference.definition_id()),
            transform,
            parent_definition_uuids: parents,
            name: identity.name.clone(),
            visible: identity.effective_visible,
            links,
        });
    }

    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena(ctx, "product_definitions", &definitions)?;
    namespace.set_arena(ctx, "product_occurrences", &occurrences)?;
    namespace.set_arena(ctx, "external_references", &external)?;
    Ok(losses)
}

/// Nesting depth at which the occurrence tree stops descending.
const MAX_OCCURRENCE_DEPTH: usize = 64;

/// Neutral reusable definitions and their placed-occurrence tree.
#[derive(Debug, Default)]
pub(crate) struct NeutralProduct {
    /// One reusable definition per decoded instance definition.
    pub(crate) definitions: Vec<ProductDefinition>,
    /// Placed occurrences in depth-first source order.
    pub(crate) occurrences: Vec<Occurrence>,
    /// Source order of the reference object behind each occurrence.
    pub(crate) owners: Vec<usize>,
    /// Placements left out of the tree, with their object source order.
    pub(crate) omitted: Vec<(usize, String)>,
}

/// One parsed reference object.
struct PlacedReference {
    definition: Uuid,
    transform: Transform,
}

fn neutral_definition_id(id: Uuid) -> Result<ProductDefinitionId, CodecError> {
    ProductDefinitionId::mint(format!("rhino:instance:definition#{id}"))
        .map_err(|error| CodecError::malformed(error.to_string()))
}

fn external_document(link: &LinkSource) -> ExternalDocument {
    match link {
        LinkSource::None => ExternalDocument::Missing {},
        LinkSource::LegacyFull(path) => ExternalDocument::path(path.as_str()),
        LinkSource::LegacyRelative { relative_path, .. } => {
            ExternalDocument::path(relative_path.as_str())
        }
        LinkSource::Structured(value) => {
            if value.full_path.is_empty() {
                ExternalDocument::path(value.relative_path.as_str())
            } else {
                ExternalDocument::path(value.full_path.as_str())
            }
        }
    }
}

struct TreeBuilder<'s> {
    ctx: &'s cadmpeg_core::decode::DecodeContext<'s>,
    definitions: HashMap<Uuid, &'s crate::instances::InstanceDefinition>,
    references: HashMap<usize, PlacedReference>,
    objects: HashMap<Uuid, Vec<usize>>,
    product: NeutralProduct,
}

impl TreeBuilder<'_> {
    fn prototype(&self, definition: Uuid) -> Result<PrototypeReference, CodecError> {
        Ok(match self.definitions.get(&definition) {
            None => PrototypeReference::Unresolved {},
            Some(found) if matches!(found.kind, DefinitionKind::Linked) => {
                PrototypeReference::External {
                    document: external_document(&found.link),
                    object: None,
                }
            }
            Some(_) => PrototypeReference::Local {
                definition: neutral_definition_id(definition)?,
            },
        })
    }

    fn place(
        &mut self,
        source_order: usize,
        key: &str,
        parent: OccurrenceParent,
        ordinal: u32,
        stack: &mut Vec<Uuid>,
        native_ref: &str,
    ) -> Result<(), CodecError> {
        let Some(reference) = self.references.get(&source_order) else {
            return Ok(());
        };
        let definition = reference.definition;
        let transform = reference.transform;
        self.ctx.charge_entities(1, "rhino_instance_occurrences")?;
        let id = OccurrenceId::mint(format!("rhino:instance:occurrence#{key}"))
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        let occurrence = Occurrence {
            id: id.clone(),
            prototype: self.prototype(definition)?,
            parent,
            ordinal,
            transform,
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(native_ref.to_owned()),
        };
        self.product.occurrences.push(occurrence);
        self.product.owners.push(source_order);
        let Some(found) = self.definitions.get(&definition).copied() else {
            return Ok(());
        };
        if stack.contains(&definition) {
            self.product.omitted.push((
                source_order,
                format!("definition cycle reaches {definition}"),
            ));
            return Ok(());
        }
        if stack.len() >= MAX_OCCURRENCE_DEPTH {
            self.product.omitted.push((
                source_order,
                format!("occurrence nesting exceeds {MAX_OCCURRENCE_DEPTH} levels"),
            ));
            return Ok(());
        }
        stack.push(definition);
        let mut ordinal = 0_u32;
        let mut seen = HashSet::new();
        for member in &found.members {
            if !seen.insert(*member) {
                continue;
            }
            let Some(&[member_order]) = self.objects.get(member).map(Vec::as_slice) else {
                continue;
            };
            if !self.references.contains_key(&member_order) {
                continue;
            }
            let child_key = format!("{key}/{member}");
            let child_native = format!("rhino:object:record#{member_order:06}");
            self.place(
                member_order,
                &child_key,
                OccurrenceParent::Occurrence {
                    occurrence: id.clone(),
                },
                ordinal,
                stack,
                &child_native,
            )?;
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("Rhino occurrence ordinal exceeds u32"))?;
        }
        stack.pop();
        Ok(())
    }
}

/// Builds the neutral product graph: one definition per decoded instance
/// definition and one occurrence per placed instance reference, independent
/// of geometry expansion.
pub(crate) fn neutral(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
    scale: MillimeterScale,
) -> Result<NeutralProduct, CodecError> {
    let mut builder = TreeBuilder {
        ctx,
        definitions: HashMap::new(),
        references: HashMap::new(),
        objects: HashMap::new(),
        product: NeutralProduct::default(),
    };
    for definition in scan.definitions.definitions() {
        builder.definitions.insert(definition.id(), definition);
        builder.product.definitions.push(ProductDefinition {
            id: neutral_definition_id(definition.id())?,
            kind: ProductDefinitionKind::Group,
            source_name: (!definition.name.is_empty()).then(|| definition.name.clone()),
            label: None,
            description: (!definition.description.is_empty())
                .then(|| definition.description.clone()),
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: Vec::new(),
            native_ref: None,
        });
    }
    let mut roots = Vec::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(identity) = object.identity() {
            builder
                .objects
                .entry(identity.object_id)
                .or_default()
                .push(source_order);
        }
        let Some(object) = object.framed() else {
            continue;
        };
        if !crate::instances::is_reference_class(object.class_uuid) {
            continue;
        }
        let Ok(reference) =
            crate::instances::parse_reference(scan.data, object.class_data_range.clone())
        else {
            continue;
        };
        let Some(transform) = crate::instances::scale_translation(reference.transform(), scale)
        else {
            builder.product.omitted.push((
                source_order,
                "scaled instance transform is invalid".to_owned(),
            ));
            continue;
        };
        builder.references.insert(
            source_order,
            PlacedReference {
                definition: reference.definition_id(),
                transform,
            },
        );
        if !scan.definitions.contains_member(object.identity.object_id) {
            roots.push((source_order, object.identity.object_id));
        }
    }
    let mut ordinal = 0_u32;
    for (source_order, object_id) in roots {
        let unique = builder
            .objects
            .get(&object_id)
            .is_some_and(|orders| orders.len() == 1);
        let key = if object_id.is_nil() || !unique {
            format!("record-{source_order:06}")
        } else {
            format!("{object_id}")
        };
        let native = format!("rhino:object:record#{source_order:06}");
        builder.place(
            source_order,
            &key,
            OccurrenceParent::Root {},
            ordinal,
            &mut Vec::new(),
            &native,
        )?;
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Rhino occurrence ordinal exceeds u32"))?;
    }
    Ok(builder.product)
}

#[cfg(test)]
mod tests {
    use super::install;
    use crate::test_support::test_dump::{
        object_record_with_payload, scan_with_objects, INSTANCE_REFERENCE_CLASS,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_test_support::{wire, EditableDecodeResult};

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
        let losses = install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("other product records remain transferable");
        assert_eq!(losses.len(), 1);
        let message = &losses[0].message;
        assert!(message.contains(&source_id));
        assert!(message.contains(&format!("at offset {source_offset}")));
        assert!(message.contains("could not be transferred"));
        let provenance = losses[0]
            .provenance
            .as_ref()
            .expect("located occurrence loss");
        assert_eq!(
            provenance.offset,
            cadmpeg_core::decode::u64_from_index(source_offset)
        );
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
            (Some(0), [3.0, -4.0, 5.0], [3.0, -4.0, 5.0], "millimeter"),
            (Some(255), [3.0, -4.0, 5.0], [3.0, -4.0, 5.0], "millimeter"),
            (None, [3.0, -4.0, 5.0], [3.0, -4.0, 5.0], "millimeter"),
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
            let decoded = EditableDecodeResult::from(
                crate::RhinoCodec
                    .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                    .expect("complete occurrence decode"),
            );
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
        let decoded = EditableDecodeResult::from(
            crate::RhinoCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .expect("later valid records recover"),
        );
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
            assert_eq!(wire::field::<String>(&provenance, "format"), "rhino");
            assert_eq!(
                provenance.offset,
                cadmpeg_core::decode::u64_from_index(source.range.start)
            );
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
