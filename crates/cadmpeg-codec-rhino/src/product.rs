// SPDX-License-Identifier: Apache-2.0
//! Persistent Rhino definition, occurrence, and external-reference graph.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;

use crate::container::Scan;
use crate::instances::{hex, DefinitionKind, LinkSource, UnitDetail};
use crate::loss::RhinoLossCode;
use crate::settings::UnitBinding;
use crate::wire::Uuid;
use crate::wire::{admitted_format, copy_retained_string, reserve_collection};

fn reserve_map<K: Eq + std::hash::Hash, V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &mut HashMap<K, V>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    map.try_reserve(1).map_err(|_| {
        CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
            dimension: cadmpeg_core::decode::ResourceDimension::CollectionItems,
            reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: 1,
            operation,
        })
    })
}

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

fn definition_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: Uuid,
) -> Result<String, CodecError> {
    admitted_format(
        ctx,
        format_args!("rhino:product:definition#{id}"),
        "Rhino product definition ID",
    )
}

fn external_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: Uuid,
) -> Result<String, CodecError> {
    admitted_format(
        ctx,
        format_args!("rhino:product:external#{id}"),
        "Rhino product external ID",
    )
}

fn external_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition_uuid: Uuid,
    link: &LinkSource,
) -> Result<Option<ExternalReferenceRecord>, CodecError> {
    if matches!(link, LinkSource::None) {
        return Ok(None);
    }
    let definition = definition_id(ctx, definition_uuid)?;
    let mut links = Vec::new();
    reserve_collection(ctx, &mut links, 1, "Rhino external reference links")?;
    links.push(definition);
    let (full_path, relative_path, relative_path_preferred) = match link {
        LinkSource::None => return Ok(None),
        LinkSource::Structured(value) => {
            return Ok(Some(ExternalReferenceRecord {
                id: external_id(ctx, definition_uuid)?,
                definition_uuid: admitted_format(
                    ctx,
                    format_args!("{definition_uuid}"),
                    "Rhino external definition UUID",
                )?,
                full_path: copy_retained_string(ctx, &value.full_path, "Rhino external full path")?,
                relative_path: copy_retained_string(
                    ctx,
                    &value.relative_path,
                    "Rhino external relative path",
                )?,
                relative_path_preferred: false,
                byte_count: Some(value.content_hash.byte_count),
                hash_time: Some(value.content_hash.hash_time),
                content_time: Some(value.content_hash.content_time),
                name_sha1: Some(hex(
                    ctx,
                    &value.content_hash.name_sha1,
                    "Rhino external name SHA-1",
                )?),
                content_sha1: Some(hex(
                    ctx,
                    &value.content_hash.content_sha1,
                    "Rhino external content SHA-1",
                )?),
                path_status: Some(value.path_status),
                embedded_file_uuid: value
                    .embedded_file_id
                    .map(|id| {
                        admitted_format(ctx, format_args!("{id}"), "Rhino external embedded UUID")
                    })
                    .transpose()?,
                links,
            }))
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
    Ok(Some(ExternalReferenceRecord {
        id: external_id(ctx, definition_uuid)?,
        definition_uuid: admitted_format(
            ctx,
            format_args!("{definition_uuid}"),
            "Rhino external definition UUID",
        )?,
        full_path: copy_retained_string(ctx, full_path, "Rhino external full path")?,
        relative_path: copy_retained_string(ctx, relative_path, "Rhino external relative path")?,
        relative_path_preferred,
        byte_count: None,
        hash_time: None,
        content_time: None,
        name_sha1: None,
        content_sha1: None,
        path_status: None,
        embedded_file_uuid: None,
        links,
    }))
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
            if !object_records.contains_key(&identity.object_id) {
                reserve_map(ctx, &mut object_records, "Rhino product object keys")?;
            }
            let rows = object_records.entry(identity.object_id).or_default();
            reserve_collection(ctx, rows, 1, "Rhino product object positions")?;
            rows.push((
                source_order,
                admitted_format(
                    ctx,
                    format_args!("rhino:object:record#{source_order:06}"),
                    "Rhino product object ID",
                )?,
            ));
        }
    }

    let mut definitions = Vec::new();
    let mut external = Vec::new();
    for definition in scan.definitions.definitions() {
        let external_reference = external_record(ctx, definition.id(), &definition.link)?;
        let external_id = external_reference
            .as_ref()
            .map(|value| copy_retained_string(ctx, &value.id, "Rhino definition external ID"))
            .transpose()?;
        if let Some(value) = external_reference {
            reserve_collection(ctx, &mut external, 1, "Rhino external references")?;
            external.push(value);
        }
        let mut links = Vec::new();
        for matches in definition
            .members
            .iter()
            .filter_map(|id| object_records.get(id))
            .filter(|matches| matches.len() == 1)
        {
            reserve_collection(ctx, &mut links, 1, "Rhino definition links")?;
            links.push(copy_retained_string(
                ctx,
                &matches[0].1,
                "Rhino definition member link",
            )?);
        }
        if let Some(id) = &external_id {
            reserve_collection(ctx, &mut links, 1, "Rhino definition links")?;
            links.push(copy_retained_string(
                ctx,
                id,
                "Rhino definition external link",
            )?);
        }
        links.sort();
        links.dedup();
        let mut member_object_ids = Vec::new();
        for id in &definition.members {
            reserve_collection(
                ctx,
                &mut member_object_ids,
                1,
                "Rhino definition member UUIDs",
            )?;
            member_object_ids.push(admitted_format(
                ctx,
                format_args!("{id}"),
                "Rhino definition member UUID text",
            )?);
        }
        reserve_collection(ctx, &mut definitions, 1, "Rhino product definitions")?;
        definitions.push(DefinitionRecord {
            id: definition_id(ctx, definition.id())?,
            source_offset: definition.source_range.start as u64,
            source_uuid: admitted_format(
                ctx,
                format_args!("{}", definition.id()),
                "Rhino definition source UUID",
            )?,
            archive_index: definition.index,
            name: copy_retained_string(ctx, &definition.name, "Rhino product definition name")?,
            description: copy_retained_string(
                ctx,
                &definition.description,
                "Rhino product definition description",
            )?,
            url: copy_retained_string(ctx, &definition.url, "Rhino product definition URL")?,
            url_tag: copy_retained_string(
                ctx,
                &definition.url_tag,
                "Rhino product definition URL tag",
            )?,
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
        if !definition_ids.contains(&definition.id()) {
            ctx.charge_collection_items(1, "Rhino product definition keys")?;
            definition_ids.try_reserve(1).map_err(|_| {
                CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
                    dimension: cadmpeg_core::decode::ResourceDimension::CollectionItems,
                    reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
                    limit: u64::MAX,
                    used: 0,
                    additional: 1,
                    operation: "Rhino product definition keys",
                })
            })?;
        }
        definition_ids.insert(definition.id());
        for member in &definition.members {
            if !member_definitions.contains_key(member) {
                reserve_map(ctx, &mut member_definitions, "Rhino product member keys")?;
            }
            let parents = member_definitions.entry(*member).or_default();
            reserve_collection(ctx, parents, 1, "Rhino product member parents")?;
            parents.push(admitted_format(
                ctx,
                format_args!("{}", definition.id()),
                "Rhino product parent UUID",
            )?);
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
                reserve_collection(ctx, &mut losses, 1, "Rhino product occurrence losses")?;
                let loss = crate::wire::admitted_loss(
                    ctx,
                    RhinoLossCode::ProductOccurrenceDropped,
                    format_args!(
                        "product occurrence {} at offset {} (class {}) could not be transferred: {error}",
                        identity.source_id, object.range.start, object.class_uuid
                    ),
                    "Rhino product occurrence loss text",
                )?;
                let tag = admitted_format(
                    ctx,
                    format_args!(
                        "PRODUCT_OCCURRENCE/source={}/class={}",
                        identity.source_id, object.class_uuid
                    ),
                    "Rhino product occurrence loss tag",
                )?;
                losses.push(loss.with_provenance(
                    SourceProvenance::root("rhino", object.range.start as u64).with_tag(tag),
                ));
                continue;
            }
        };
        let transform = OccurrenceTransform::from_source(reference.transform(), binding);
        let definition = definition_id(ctx, reference.definition_id())?;
        let object_record = admitted_format(
            ctx,
            format_args!("rhino:object:record#{source_order:06}"),
            "Rhino occurrence object ID",
        )?;
        let mut parents = Vec::new();
        if let Some(source_parents) = member_definitions.get(&identity.object_id) {
            reserve_collection(
                ctx,
                &mut parents,
                source_parents.len(),
                "Rhino occurrence parents",
            )?;
            for parent in source_parents {
                parents.push(copy_retained_string(
                    ctx,
                    parent,
                    "Rhino occurrence parent UUID",
                )?);
            }
        }
        let key = if identity.object_id.is_nil()
            || object_records
                .get(&identity.object_id)
                .is_some_and(|matches| matches.len() != 1)
        {
            admitted_format(
                ctx,
                format_args!("record-{source_order:06}"),
                "Rhino occurrence key",
            )?
        } else {
            admitted_format(
                ctx,
                format_args!("{}", identity.object_id),
                "Rhino occurrence key",
            )?
        };
        let mut links = Vec::new();
        reserve_collection(ctx, &mut links, 1, "Rhino occurrence links")?;
        links.push(object_record);
        if definition_ids.contains(&reference.definition_id()) {
            reserve_collection(ctx, &mut links, 1, "Rhino occurrence links")?;
            links.push(definition);
        }
        links.sort();
        reserve_collection(ctx, &mut occurrences, 1, "Rhino product occurrences")?;
        occurrences.push(OccurrenceRecord {
            id: admitted_format(
                ctx,
                format_args!("rhino:product:occurrence#{key}"),
                "Rhino product occurrence ID",
            )?,
            source_offset: object.range.start as u64,
            source_uuid: admitted_format(
                ctx,
                format_args!("{}", identity.object_id),
                "Rhino occurrence source UUID",
            )?,
            definition_uuid: admitted_format(
                ctx,
                format_args!("{}", reference.definition_id()),
                "Rhino occurrence definition UUID",
            )?,
            transform,
            parent_definition_uuids: parents,
            name: copy_retained_string(ctx, &identity.name, "Rhino occurrence name")?,
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

#[cfg(test)]
mod tests {
    use super::install;
    use crate::test_support::test_dump::{
        object_record_with_payload, scan_with_objects, INSTANCE_REFERENCE_CLASS,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_test_support::{wire, EditableDecodeResult};

    fn with_collection_limit<T>(
        scan: &crate::container::Scan<'_>,
        limit: u64,
        test: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
    ) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .expect("root bytes admitted");
        test(&ctx)
    }

    fn product_limit_error(
        scan: &crate::container::Scan<'_>,
        limit: u64,
    ) -> cadmpeg_core::CodecError {
        with_collection_limit(scan, limit, |ctx| {
            install(ctx, scan, &mut CadIr::empty()).expect_err("product collection exceeds limit")
        })
    }

    fn one_reference_scan() -> crate::container::Scan<'static> {
        let archive = crate::chunks::ArchiveVersion::V5;
        let payload = crate::test_support::test_dump::instance_reference_payload(
            [0x51; 16],
            cadmpeg_ir::transform::Transform::identity().rows(),
        );
        scan_with_objects(&[object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &payload,
        )])
    }

    fn one_definition_scan() -> crate::container::Scan<'static> {
        use crate::test_support::test_dump as support;
        let archive = crate::chunks::ArchiveVersion::V5;
        let payload = support::v5_definition_payload(archive, 6, [0x51; 16], &[[0x62; 16]], false);
        let record = support::definition_record(archive, &payload);
        crate::container::scan_owned(support::document_with_definitions(
            "50",
            archive,
            &[record],
            &[],
        ))
        .expect("definition document is framed")
    }

    fn one_linked_definition_scan(minor: u8) -> crate::container::Scan<'static> {
        use crate::test_support::test_dump as support;
        let archive = crate::chunks::ArchiveVersion::V5;
        let payload = support::v5_definition_payload(archive, minor, [0x51; 16], &[], true);
        let record = support::definition_record(archive, &payload);
        crate::container::scan_owned(support::document_with_definitions(
            "50",
            archive,
            &[record],
            &[],
        ))
        .expect("linked definition document is framed")
    }

    #[test]
    fn product_object_keys_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            product_limit_error(&scan, 0),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product object keys"
        ));
    }

    #[test]
    fn product_object_positions_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            product_limit_error(&scan, 1),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product object positions"
        ));
    }

    #[test]
    fn occurrence_links_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            product_limit_error(&scan, 2),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino occurrence links"
        ));
    }

    #[test]
    fn product_occurrences_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            product_limit_error(&scan, 3),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product occurrences"
        ));
    }

    #[test]
    fn definition_member_uuids_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            product_limit_error(&scan, 0),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino definition member UUIDs"
        ));
    }

    #[test]
    fn product_definitions_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            product_limit_error(&scan, 1),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product definitions"
        ));
    }

    #[test]
    fn product_definition_keys_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            product_limit_error(&scan, 2),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product definition keys"
        ));
    }

    #[test]
    fn product_member_keys_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            product_limit_error(&scan, 3),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product member keys"
        ));
    }

    #[test]
    fn product_member_parents_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            product_limit_error(&scan, 4),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product member parents"
        ));
        let mut ir = CadIr::empty();
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("service profile admits product definition");
        assert_eq!(
            ir.native.namespace("rhino").unwrap().arenas()["product_definitions"].len(),
            1
        );
    }

    #[test]
    fn external_reference_links_refuse_collection_limit() {
        let scan = one_linked_definition_scan(6);
        assert!(matches!(
            product_limit_error(&scan, 0),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino external reference links"
        ));
    }

    #[test]
    fn external_references_refuse_collection_limit() {
        let scan = one_linked_definition_scan(6);
        assert!(matches!(
            product_limit_error(&scan, 1),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino external references"
        ));
        let mut ir = CadIr::empty();
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("service profile admits external reference");
        assert_eq!(
            ir.native.namespace("rhino").unwrap().arenas()["external_references"].len(),
            1
        );
    }

    #[test]
    fn external_name_sha1_refuses_retained_limit() {
        let scan = one_linked_definition_scan(7);
        let link = &scan.definitions.definitions()[0].link;
        assert!(matches!(link, crate::instances::LinkSource::Structured(_)));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        let definition_id_len = format!(
            "rhino:product:definition#{}",
            scan.definitions.definitions()[0].id()
        )
        .len();
        let external_id_len = format!(
            "rhino:product:external#{}",
            scan.definitions.definitions()[0].id()
        )
        .len();
        let crate::instances::LinkSource::Structured(reference) = link else {
            return;
        };
        policy.limits.max_retained_bytes = u64::try_from(
            definition_id_len
                + external_id_len
                + 36
                + reference.full_path.len()
                + reference.relative_path.len(),
        )
        .expect("test budget fits u64");
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .expect("root bytes admitted");
        let error = super::external_record(&ctx, scan.definitions.definitions()[0].id(), link)
            .expect_err("name digest exceeds retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino external name SHA-1"
        ));
        assert!(super::external_record(
            &cadmpeg_test_support::service_decode_context(),
            scan.definitions.definitions()[0].id(),
            link,
        )
        .expect("service profile admits digest")
        .is_some());
    }

    #[test]
    fn malformed_occurrence_loss_refuses_collection_limit() {
        let scan = scan_with_objects(&[object_record_with_payload(
            crate::chunks::ArchiveVersion::V5,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &[],
        )]);
        assert!(matches!(
            product_limit_error(&scan, 2),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product occurrence losses"
        ));
    }

    #[test]
    fn malformed_occurrence_loss_copy_refuses_retained_limit() {
        let scan = scan_with_objects(&[object_record_with_payload(
            crate::chunks::ArchiveVersion::V5,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &[],
        )]);
        let mut limit = 0_u64;
        let mut loss_text_refusals = 0;
        for _ in 0..128 {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                    .expect("root bytes admitted");
            let refusal = install(&ctx, &scan, &mut CadIr::empty())
                .expect_err("loss text exceeds the retained limit");
            let cadmpeg_core::CodecError::ResourceLimit(item) = refusal else {
                panic!("expected a retained-byte refusal, got {refusal:?}");
            };
            if item.operation == "Rhino product occurrence loss text" {
                loss_text_refusals += 1;
                if loss_text_refusals == 2 {
                    return;
                }
            }
            limit = (item.used + item.additional).max(limit + 1);
        }
        panic!("product loss note copy was not reached");
    }

    #[test]
    fn product_object_id_refuses_retained_limit() {
        let scan = one_reference_scan();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .expect("root bytes admitted");
        let error = install(&ctx, &scan, &mut CadIr::empty())
            .expect_err("object ID exceeds retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product object ID"
        ));
    }

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
