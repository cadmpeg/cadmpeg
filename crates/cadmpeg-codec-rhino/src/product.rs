// SPDX-License-Identifier: Apache-2.0
//! Persistent Rhino definition, occurrence, and external-reference graph.

use std::collections::{BTreeSet, HashMap, HashSet};

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

#[derive(Debug, Serialize)]
struct DefinitionRecord<'a> {
    id: String,
    source_offset: u64,
    source_uuid: String,
    archive_index: Option<i32>,
    name: &'a str,
    description: &'a str,
    url: &'a str,
    url_tag: &'a str,
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
struct OccurrenceRecord<'a> {
    id: String,
    source_offset: u64,
    source_uuid: String,
    definition_uuid: String,
    #[serde(flatten)]
    transform: OccurrenceTransform,
    parent_definition_uuids: Vec<String>,
    name: &'a str,
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
struct ExternalReferenceRecord<'a> {
    id: String,
    definition_uuid: String,
    full_path: &'a str,
    relative_path: &'a str,
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
    ctx.format_retained(
        format_args!("rhino:product:definition#{id}"),
        "Rhino product definition ID",
    )
}

fn external_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: Uuid,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("rhino:product:external#{id}"),
        "Rhino product external ID",
    )
}

fn external_record<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition_uuid: Uuid,
    link: &'a LinkSource,
) -> Result<Option<ExternalReferenceRecord<'a>>, CodecError> {
    if matches!(link, LinkSource::None) {
        return Ok(None);
    }
    let definition = definition_id(ctx, definition_uuid)?;
    let mut links = ctx.collection_vec(1, "Rhino external reference links")?;
    links.push(definition);
    let (full_path, relative_path, relative_path_preferred) = match link {
        LinkSource::None => return Ok(None),
        LinkSource::Structured(value) => {
            return Ok(Some(ExternalReferenceRecord {
                id: external_id(ctx, definition_uuid)?,
                definition_uuid: ctx.format_retained(
                    format_args!("{definition_uuid}"),
                    "Rhino external definition UUID",
                )?,
                full_path: &value.full_path,
                relative_path: &value.relative_path,
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
                        ctx.format_retained(format_args!("{id}"), "Rhino external embedded UUID")
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
        definition_uuid: ctx.format_retained(
            format_args!("{definition_uuid}"),
            "Rhino external definition UUID",
        )?,
        full_path,
        relative_path,
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
    let mut object_records = HashMap::<Uuid, Option<usize>>::new();
    let mut object_workspace = ctx.reserve_scoped(0, "Rhino product object workspace")?;
    for (source_order, object) in ctx
        .admit_iter(&scan.objects[..], "Rhino install traversal")?
        .enumerate()
    {
        if let Some(identity) = object.identity() {
            object_workspace.with_storage(|| -> Result<(), CodecError> {
                match ctx.entry_hash_map(
                    &mut object_records,
                    identity.object_id,
                    "Rhino product object keys",
                )? {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Some(source_order));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.insert(None);
                    }
                }
                Ok(())
            })?;
        }
    }

    let mut staging = ctx.reserve_scoped(0, "Rhino product staging records")?;
    let mut definitions = Vec::new();
    let mut external = Vec::new();
    staging.with_storage(|| -> Result<(), CodecError> {
        for definition in ctx.admit_iter(
            scan.definitions.definitions(),
            "Rhino install borrowed traversal",
        )? {
            let external_reference = external_record(ctx, definition.id(), &definition.link)?;
            let external_id = external_reference
                .as_ref()
                .map(|value| ctx.copy_retained_text(&value.id, "Rhino definition external ID"))
                .transpose()?;
            if let Some(value) = external_reference {
                ctx.reserve_vec(&mut external, 1, "Rhino external references")?;
                external.push(value);
            }
            let mut links = Vec::new();
            let mut member_seen = HashSet::new();
            let mut member_workspace =
                ctx.reserve_scoped(0, "Rhino definition member workspace")?;
            for member in ctx.admit_iter(&definition.members[..], "Rhino install traversal")? {
                if let Some(Some(source_order)) =
                    ctx.get_hash_map(&object_records, member, "Rhino definition member lookup")?
                {
                    if !member_workspace.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut member_seen,
                            *source_order,
                            "Rhino definition member identities",
                        )
                    })? {
                        continue;
                    }
                    ctx.reserve_vec(&mut links, 1, "Rhino definition links")?;
                    links.push(ctx.format_retained(
                        format_args!("rhino:object:record#{source_order:06}"),
                        "Rhino definition member link",
                    )?);
                }
            }
            if let Some(id) = &external_id {
                ctx.reserve_vec(&mut links, 1, "Rhino definition links")?;
                links.push(ctx.copy_retained_text(id, "Rhino definition external link")?);
            }
            ctx.stable_sort_by(
                &mut links,
                |value| value,
                Ord::cmp,
                "Rhino definition links sort",
            )?;
            let mut member_object_ids = Vec::new();
            for id in ctx.admit_iter(&definition.members[..], "Rhino install traversal")? {
                ctx.reserve_vec(&mut member_object_ids, 1, "Rhino definition member UUIDs")?;
                member_object_ids.push(
                    ctx.format_retained(format_args!("{id}"), "Rhino definition member UUID text")?,
                );
            }
            ctx.reserve_vec(&mut definitions, 1, "Rhino product definitions")?;
            definitions.push(DefinitionRecord {
                id: definition_id(ctx, definition.id())?,
                source_offset: cadmpeg_core::decode::u64_from_index(definition.source_range.start),
                source_uuid: ctx.format_retained(
                    format_args!("{}", definition.id()),
                    "Rhino definition source UUID",
                )?,
                archive_index: definition.index,
                name: &definition.name,
                description: &definition.description,
                url: &definition.url,
                url_tag: &definition.url_tag,
                kind: definition.kind,
                member_object_ids,
                units: &definition.units,
                linked_depth: definition.linked_depth,
                linked_component_appearance: definition.linked_appearance,
                external_reference: external_id,
                links,
            });
        }

        Ok(())
    })?;

    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
    // Each member's parent definitions, ordered and unique by UUID; the
    // table only serves keyed lookups and is dropped before install returns.
    let mut member_definitions = HashMap::<Uuid, BTreeSet<Uuid>>::new();
    let mut definition_ids = HashSet::new();
    let mut definition_workspace = ctx.reserve_scoped(0, "Rhino product definition workspace")?;
    for definition in ctx.admit_iter(
        scan.definitions.definitions(),
        "Rhino install borrowed traversal",
    )? {
        definition_workspace.with_storage(|| -> Result<(), CodecError> {
            ctx.insert_hash_set(
                &mut definition_ids,
                definition.id(),
                "Rhino product definition keys",
            )?;
            for member in ctx.admit_iter(&definition.members[..], "Rhino install traversal")? {
                let parents = ctx
                    .entry_hash_map(
                        &mut member_definitions,
                        *member,
                        "Rhino product member keys",
                    )?
                    .or_default();
                ctx.insert_btree_set(parents, definition.id(), "Rhino product member parents")?;
            }
            Ok(())
        })?;
    }
    let mut occurrences = Vec::new();
    for (source_order, object) in ctx
        .admit_iter(&scan.objects[..], "Rhino install traversal")?
        .enumerate()
    {
        let Some(object) = object.framed() else {
            continue;
        };
        if !crate::instances::is_reference_class(object.class_uuid) {
            continue;
        }
        let identity = &object.identity;
        let reference = match crate::instances::parse_reference(
            ctx,
            scan.data,
            object.class_data_range.clone(),
        ) {
            Ok(reference) => reference,
            Err(crate::chunks::FramingError::Resource(limit)) => {
                return Err(CodecError::ResourceLimit(limit))
            }
            Err(error) => {
                ctx.reserve_vec(&mut losses, 1, "Rhino product occurrence losses")?;
                let loss = crate::wire::admitted_loss(
                    ctx,
                    RhinoLossCode::ProductOccurrenceDropped,
                    format_args!(
                        "product occurrence {} at offset {} (class {}) could not be transferred: {error}",
                        identity.source_id, object.range.start, object.class_uuid
                    ),
                    "Rhino product occurrence loss text",
                )?;
                let tag = ctx.format_retained(
                    format_args!(
                        "PRODUCT_OCCURRENCE/source={}/class={}",
                        identity.source_id, object.class_uuid
                    ),
                    "Rhino product occurrence loss tag",
                )?;
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
        staging.with_storage(|| -> Result<(), CodecError> {
            let transform = OccurrenceTransform::from_source(reference.transform(), binding);
            let object_record = ctx.format_retained(
                format_args!("rhino:object:record#{source_order:06}"),
                "Rhino occurrence object ID",
            )?;
            let mut parents = Vec::new();
            if let Some(source_parents) = ctx.get_hash_map(
                &member_definitions,
                &identity.object_id,
                "Rhino occurrence parent lookup",
            )? {
                ctx.reserve_vec(
                    &mut parents,
                    source_parents.len(),
                    "Rhino occurrence parents",
                )?;
                for parent in ctx.admit_iter(source_parents, "Rhino install borrowed traversal")? {
                    parents.push(ctx.format_retained(
                        format_args!("{parent}"),
                        "Rhino occurrence parent UUID",
                    )?);
                }
            }
            let (key, _key_workspace) = if identity.object_id.is_nil()
                || ctx
                    .get_hash_map(
                        &object_records,
                        &identity.object_id,
                        "Rhino occurrence identity lookup",
                    )?
                    .is_some_and(Option::is_none)
            {
                ctx.format_scoped(
                    format_args!("record-{source_order:06}"),
                    "Rhino occurrence key",
                )?
            } else {
                ctx.format_scoped(
                    format_args!("{}", identity.object_id),
                    "Rhino occurrence key",
                )?
            };
            let mut links = ctx.collection_vec(1, "Rhino occurrence links")?;
            links.push(object_record);
            if ctx.contains_hash_set(
                &definition_ids,
                &reference.definition_id(),
                "Rhino occurrence definition lookup",
            )? {
                ctx.reserve_vec(&mut links, 1, "Rhino occurrence links")?;
                links.push(definition_id(ctx, reference.definition_id())?);
            }
            ctx.stable_sort_by(
                &mut links,
                |value| value,
                Ord::cmp,
                "Rhino occurrence links sort",
            )?;
            ctx.reserve_vec(&mut occurrences, 1, "Rhino product occurrences")?;
            occurrences.push(OccurrenceRecord {
                id: ctx.format_retained(
                    format_args!("rhino:product:occurrence#{key}"),
                    "Rhino product occurrence ID",
                )?,
                source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                source_uuid: ctx.format_retained(
                    format_args!("{}", identity.object_id),
                    "Rhino occurrence source UUID",
                )?,
                definition_uuid: ctx.format_retained(
                    format_args!("{}", reference.definition_id()),
                    "Rhino occurrence definition UUID",
                )?,
                transform,
                parent_definition_uuids: parents,
                name: &identity.name,
                visible: identity.effective_visible,
                links,
            });
            Ok(())
        })?;
    }

    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena(ctx, "product_definitions", &definitions)?;
    namespace.set_arena(ctx, "product_occurrences", &occurrences)?;
    namespace.set_arena(ctx, "external_references", &external)?;
    Ok(losses)
}

#[cfg(test)]
mod tests {
    mod budget_repairs;
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
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product object keys", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product object keys"
        ));
    }

    #[test]
    // One unique object key precedes the occurrence link slot.
    fn occurrence_links_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino occurrence links", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino occurrence links"
        ));
    }

    #[test]
    // One object key and one link precede the occurrence slot.
    fn product_occurrences_refuse_collection_limit() {
        let scan = one_reference_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product occurrences", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product occurrences"
        ));
    }

    #[test]
    fn definition_member_uuids_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino definition member UUIDs", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino definition member UUIDs"
        ));
    }

    #[test]
    fn product_definitions_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product definitions", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product definitions"
        ));
    }

    #[test]
    fn product_definition_keys_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product definition keys", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product definition keys"
        ));
    }

    #[test]
    fn product_member_keys_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product member keys", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino product member keys"
        ));
    }

    #[test]
    fn product_member_parents_refuse_collection_limit() {
        let scan = one_definition_scan();
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product member parents", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
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
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino external reference links", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino external reference links"
        ));
    }

    #[test]
    fn external_references_refuse_collection_limit() {
        let scan = one_linked_definition_scan(6);
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino external references", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
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
        let run = |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                    .expect("root bytes admitted");
            let error = super::external_record(&ctx, scan.definitions.definitions()[0].id(), link)
                .expect_err("name digest exceeds retained limit");
            error
        };
        let error = run(crate::test_support::retained_limit_at(
            "Rhino external name SHA-1",
            0,
            |cap| match run(cap) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            },
        ));
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
    // One unique object key precedes the loss slot.
    fn malformed_occurrence_loss_refuses_collection_limit() {
        let scan = scan_with_objects(&[object_record_with_payload(
            crate::chunks::ArchiveVersion::V5,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &[],
        )]);
        assert!(matches!(
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems, "Rhino product occurrence losses", |cap| {
                    with_collection_limit(&scan, cap, |ctx| install(ctx, &scan, &mut CadIr::empty()))
                }
            ),
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
            // The loss text reserves its formatted length once, so the
            // ladder meets its boundary exactly once.
            if item.operation == "Rhino product occurrence loss text" {
                return;
            }
            limit = (item.used + item.additional).max(limit + 1);
        }
        panic!("product loss note copy was not reached");
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
    #[test]
    fn definition_member_links_format_each_unique_object_once() {
        use crate::chunks::ArchiveVersion;
        use crate::test_support::test_dump as support;
        use crate::wire::Uuid;
        let archive = ArchiveVersion::V5;
        let member = [0x62; 16];
        let payload =
            support::v5_definition_payload(archive, 6, [0x51; 16], &[member, member], false);
        let definition = support::definition_record(archive, &payload);
        let mut scan = crate::container::scan_owned(support::document_with_definitions(
            "50",
            archive,
            &[definition],
            &[],
        ))
        .expect("definition framing");
        let bytes = support::fixed_attributes(1, 0, None);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut attributes = crate::objects::parse_attributes(
            &ctx,
            &bytes,
            0..bytes.len(),
            0..bytes.len(),
            archive,
            None,
            &mut crate::loss::Diagnostics::new(),
        )
        .expect("member attributes");
        attributes.object_id = Uuid::from_wire(member);
        scan.objects = crate::objects::resolve_identities(
            &ctx,
            vec![crate::objects::ObjectRecord::Framed(Box::new(
                support::descriptor(attributes, 0),
            ))],
            &scan.metadata,
            &mut crate::loss::Diagnostics::new(),
        )
        .expect("member identity");
        for operation in [
            "Rhino definition member identities",
            "Rhino definition links",
        ] {
            // One object key and one distinct source-position entry precede the link slot.
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                |cap| {
                    with_collection_limit(&scan, cap, |ctx| {
                        install(ctx, &scan, &mut CadIr::empty())
                    })
                },
            );
        }
        let mut ir = CadIr::empty();
        install(&ctx, &scan, &mut ir).expect("product projection");
        let records = &ir.native.namespace("rhino").unwrap().arenas()["product_definitions"];
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].field("links"),
            Some(serde_json::json!(["rhino:object:record#000000"]))
        );
        let uuid = Uuid::from_wire(member).to_string();
        assert_eq!(
            records[0].field("member_object_ids"),
            Some(serde_json::json!([uuid, uuid]))
        );
    }
}
