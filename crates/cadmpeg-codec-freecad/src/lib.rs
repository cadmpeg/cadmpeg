// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write ZIP-packaged `FreeCAD` `.FCStd` documents.
//!
//! [`FcstdCodec`] implements [`cadmpeg_ir::codec::Codec`] and
//! [`cadmpeg_ir::codec::write::Encoder`]. Retained writes preserve
//! unedited persistence records and named side entries. Other target bands and
//! edits without a lossless serializer are rejected explicitly.
//!
//! <!-- generated: capability fcstd -->
//! Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#freecad-fcstd)).
//! <!-- /generated: capability fcstd -->

mod annotation;
mod application;
mod application_geometry;
mod attachment;
mod brep;
mod builder;
mod container;
mod design;
mod dialect;
mod drawing;
mod element_map;
mod gui;
mod joint;
/// Byte-offset constants generated from `docs/layouts/freecad.toml`.
mod layout;
mod loss;
mod mutation;
mod native;
mod persistence;
mod placement;
mod product;
mod resource;
mod topology_transfer;
mod writer;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeBody, Decoded, FormatId};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity as FindingSeverity,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::ContainerSummary;

use crate::loss::FreecadLossCode;

/// `FCStd` document codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct FcstdCodec;

#[doc(hidden)]
pub use builder::{FcstdDocumentBuilder, FcstdPropertyValue};
#[doc(hidden)]
pub use mutation::FcstdPropertyOwner;

impl FcstdCodec {
    /// Change one attribute on an ordered native property value.
    #[doc(hidden)]
    pub fn set_property_value_attribute(
        &self,
        ir: &mut CadIr,
        owner: FcstdPropertyOwner<'_>,
        property: &str,
        value_order: usize,
        attribute: &str,
        value: impl Into<String>,
    ) -> Result<(), CodecError> {
        mutation::set_value_attribute(ir, owner, property, value_order, attribute, value.into())
    }
}

const FINDINGS: &str = "FreeCAD native validation findings";

fn validate_native(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(Vec::new());
    };
    let mut reader_storage = ctx.reserve_scoped(0, "FreeCAD native validation records")?;
    macro_rules! arena {
        ($read:expr) => {
            match reader_storage.with_storage(|| $read) {
                Ok(records) => records,
                Err(cadmpeg_ir::native::NativeConvertError::Resource(error))
                    if matches!(error, CodecError::ResourceLimit(_)) =>
                {
                    return Err(error)
                }
                Err(error) => {
                    return single_finding(ctx, Check::NativeLinks, format_args!("{error}"));
                }
            }
        };
    }
    let objects = arena!(namespace.arena_as_for_decode::<native::ObjectRecord>(ctx, "objects"));
    let properties =
        arena!(namespace.arena_as_for_decode::<native::PropertyRecord>(ctx, "properties"));
    let extensions =
        arena!(namespace.arena_as_for_decode::<native::ExtensionRecord>(ctx, "extensions"));
    let entries = arena!(namespace.arena_as_for_decode::<native::EntryRecord>(ctx, "entries"));
    let physical =
        arena!(namespace.arena_as_for_decode::<native::ArchiveSpan>(ctx, "physical_ledger"));
    let logical =
        arena!(namespace.arena_as_for_decode::<native::LogicalSpan>(ctx, "logical_ledger"));
    let coverage_records =
        arena!(namespace.arena_as_for_decode::<native::ByteCoverageRecord>(ctx, "byte_coverage"));
    let mut string_table_records =
        arena!(namespace.arena_as_for_decode::<native::StringTableRecord>(ctx, "string_tables"));
    ctx.stable_sort_by(
        &mut string_table_records,
        |value| &value.index,
        Ord::cmp,
        "FreeCAD native string tables sort",
    )?;
    // The shared conversion checks each record's index once.
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(string_table_records.len()),
        "FreeCAD native string table order",
    )?;
    let string_tables = arena!(native::StringTables::try_from(string_table_records));
    let string_tables = string_tables.as_slice();
    let element_maps =
        arena!(namespace
            .arena_as_for_decode::<native::element_map::ElementMapRecord>(ctx, "element_maps"));
    let gui_providers =
        arena!(namespace
            .arena_as_for_decode::<native::GuiViewProviderRecord>(ctx, "gui_view_providers"));
    let gui_documents =
        arena!(namespace.arena_as_for_decode::<native::GuiDocumentRecord>(ctx, "gui_documents"));
    let gui_properties =
        arena!(namespace.arena_as_for_decode::<native::GuiPropertyRecord>(ctx, "gui_properties"));
    let product_nodes =
        arena!(namespace.arena_as_for_decode::<native::ProductNodeRecord>(ctx, "product_nodes"));
    let joints = arena!(namespace.arena_as_for_decode::<native::joint::JointRecord>(ctx, "joints"));
    let drawings = arena!(namespace.arena_as_for_decode::<native::DrawingRecord>(ctx, "drawings"));
    let annotations = arena!(
        namespace.arena_as_for_decode::<native::SemanticAnnotationRecord>(ctx, "annotations")
    );
    let attachments =
        arena!(namespace.arena_as_for_decode::<native::AttachmentRecord>(ctx, "attachments"));
    let shape_payloads =
        arena!(namespace.arena_as_for_decode::<brep::ShapePayloadRecord>(ctx, "shape_payloads"));
    let carrier_census =
        arena!(namespace.arena_as_for_decode::<native::CarrierCensusRecord>(ctx, "carrier_census"));
    let design_census =
        arena!(namespace.arena_as_for_decode::<native::DesignCensusRecord>(ctx, "design_census"));

    let mut findings = Vec::new();
    let findings = &mut findings;
    {
        let (expected, _expected_storage) = ctx
            .with_scoped_storage("FreeCAD expected carrier census", || {
                brep::carrier_census(ctx, &shape_payloads)
            })?;
        if first_difference(
            ctx,
            &carrier_census,
            &expected,
            "FreeCAD carrier census comparison",
        )?
        .is_some()
        {
            push_finding(
                ctx,
                findings,
                Check::PayloadIntegrity,
                format_args!("FCStd carrier census does not match parsed shape payloads"),
                None,
            )?;
        }
    }
    match ctx.with_scoped_storage("FreeCAD expected design census", || {
        design::census(ctx, &objects, &ir.model.features)
    }) {
        Ok((expected, _expected_storage)) => {
            match first_difference(ctx, &design_census, &expected, "FreeCAD design census comparison")? {
                None => {}
                Some(Some(index)) => push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!(
                        "FCStd design census does not match projected feature semantics: stored {:?} but derived {:?}",
                        design_census[index], expected[index]
                    ),
                    None,
                )?,
                Some(None) => push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!(
                        "FCStd design census does not match projected feature semantics: stored {} records and derived {} records",
                        design_census.len(),
                        expected.len()
                    ),
                    None,
                )?,
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => push_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            format_args!("{error}"),
            None,
        )?,
    }
    // Identity sets borrow the records; their scoped storage ends with the
    // validation.
    const IDENTITIES: &str = "FreeCAD validation identities";
    let object_ids = reader_storage.with_storage(|| {
        ctx.collect_hash_set(
            objects.iter().map(|record| record.id().as_str()),
            IDENTITIES,
        )
    })?;
    let entry_names = reader_storage.with_storage(|| {
        ctx.collect_hash_set(entries.iter().map(native::EntryRecord::name), IDENTITIES)
    })?;
    let property_ids = reader_storage.with_storage(|| {
        ctx.collect_hash_set(
            properties.iter().map(|record| record.id.as_str()),
            IDENTITIES,
        )
    })?;
    let extension_ids = reader_storage.with_storage(|| {
        ctx.collect_hash_set(
            extensions.iter().map(|record| record.id.as_str()),
            IDENTITIES,
        )
    })?;
    let gui_provider_ids = reader_storage.with_storage(|| {
        ctx.collect_hash_set(
            gui_providers.iter().map(|provider| provider.id.as_str()),
            IDENTITIES,
        )
    })?;
    if object_ids.len() != objects.len()
        || property_ids.len() != properties.len()
        || extension_ids.len() != extensions.len()
    {
        push_finding(
            ctx,
            findings,
            Check::Identity,
            format_args!("duplicate FCStd native identity"),
            None,
        )?;
    }
    let has = |set: &HashSet<&str>, value: &str| {
        ctx.contains_hash_set(set, value, "FreeCAD validation identity lookup")
    };
    // A local link names an object that must be present.
    let missing_link = |link: &native::LinkTarget| -> Result<bool, CodecError> {
        match (link.document(), link.object()) {
            (None, Some(object)) => Ok(!has(&object_ids, object)?),
            _ => Ok(false),
        }
    };
    let missing_in = |links: &[Option<native::LinkTarget>]| {
        ctx.any_by(
            links,
            |link| link.as_ref().map_or(Ok(false), missing_link),
            "FreeCAD validation link search",
        )
    };
    let missing_in_groups = |groups: &BTreeMap<String, Vec<Option<native::LinkTarget>>>| {
        let mut groups = groups.values();
        while let Some(links) = ctx.next_charged(&mut groups, "FreeCAD validation link search")? {
            if missing_in(links)? {
                return Ok::<bool, CodecError>(true);
            }
        }
        Ok(false)
    };
    let missing_entry = |names: &[String]| {
        ctx.any_by(
            names,
            |name| Ok(!has(&entry_names, name)?),
            "FreeCAD validation entry search",
        )
    };
    for object in ctx.admit_iter(&objects, "FreeCAD validation objects")? {
        for dependency in ctx.admit_iter(&object.dependencies, "FreeCAD validation objects")? {
            if !has(&object_ids, dependency)? {
                push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!("{} has missing dependency {dependency}", object.id()),
                    Some(object.id()),
                )?;
            }
        }
    }
    let applications_match =
        match application::matches_native(ctx, namespace, &objects, &properties, &entries) {
            Ok(matches) => matches,
            Err(cadmpeg_ir::native::NativeConvertError::Resource(CodecError::ResourceLimit(
                limit,
            ))) => {
                return Err(CodecError::ResourceLimit(limit));
            }
            Err(error) => {
                return single_finding(ctx, Check::NativeLinks, format_args!("{error}"));
            }
        };
    if !applications_match {
        push_finding(
            ctx,
            findings,
            Check::PayloadIntegrity,
            format_args!("FCStd application preservation records do not match authoritative bytes"),
            None,
        )?;
    }
    for attachment in ctx.admit_iter(&attachments, "FreeCAD validation attachments")? {
        if !has(&object_ids, &attachment.object)? || missing_in(&attachment.supports)? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "{} has an invalid attachment target or frame",
                    attachment.id
                ),
                Some(&attachment.id),
            )?;
        }
    }
    match ctx.with_scoped_storage("FreeCAD expected attachment graph", || {
        attachment::transfer(ctx, &objects, &properties)
    }) {
        Ok((expected, _expected_storage)) => {
            if first_difference(
                ctx,
                &attachments,
                &expected,
                "FreeCAD attachment comparison",
            )?
            .is_some()
            {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!(
                        "FCStd attachment graph does not match the application property graph"
                    ),
                    None,
                )?;
            }
        }
        Err(CodecError::ResourceLimit(limit)) => return Err(CodecError::ResourceLimit(limit)),
        Err(error) => push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("FCStd attachment properties are malformed: {error}"),
            None,
        )?,
    }
    let has_gui_entry = has(&entry_names, "GuiDocument.xml")?;
    if gui_documents.len() != usize::from(has_gui_entry) {
        push_finding(
            ctx,
            findings,
            Check::Counts,
            format_args!("FCStd GUI document record does not match GuiDocument.xml presence"),
            None,
        )?;
    }
    for document in ctx.admit_iter(&gui_documents, "FreeCAD validation GUI documents")? {
        if ctx.any_by(
            &document.states,
            |state| missing_entry(&state.side_entries),
            "FreeCAD validation GUI states",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("{} has a missing GUI state asset", document.id),
                Some(&document.id),
            )?;
        }
    }
    for provider in ctx.admit_iter(&gui_providers, "FreeCAD validation GUI providers")? {
        let missing = match &provider.object {
            Some(object) => !has(&object_ids, object.as_str())?,
            None => false,
        };
        if missing {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} references a missing application object", provider.id),
                Some(&provider.id),
            )?;
        }
    }
    for property in ctx.admit_iter(&gui_properties, "FreeCAD validation GUI properties")? {
        if !has(&gui_provider_ids, &property.owner)? || missing_entry(&property.side_entries)? {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} has a missing GUI owner or side entry", property.id),
                Some(&property.id),
            )?;
        }
    }
    let (product_by_object, _product_storage) =
        ctx.with_scoped_storage("fcstd product validation index", || {
            ctx.collect_hash_map(
                product_nodes
                    .iter()
                    .map(|node| (node.object.as_str(), node)),
                "fcstd product validation index",
            )
        })?;
    let (cyclic_products, _cycle_storage) = ctx
        .with_scoped_storage("fcstd product cycle lookup", || {
            product::product_cycle_nodes(ctx, &product_nodes)
        })?;
    for node in ctx.admit_iter(&product_nodes, "FreeCAD validation product nodes")? {
        let missing_prototype = match node.prototype() {
            Some(prototype) => !has(&object_ids, prototype)? && node.external_document().is_none(),
            None => false,
        };
        let missing_placement = match node.placement_property() {
            Some(property) => !has(&property_ids, property)?,
            None => false,
        };
        // Two fixed slots.
        let missing_copy_on_change = match node.copy_on_change_source() {
            Some(link) => missing_link(link)?,
            None => false,
        } || match node.copy_on_change_group() {
            Some(link) => missing_link(link)?,
            None => false,
        };
        if !has(&object_ids, &node.object)?
            || ctx.any_by(
                node.members(),
                |member| Ok(!has(&object_ids, member)?),
                "FreeCAD validation product members",
            )?
            || missing_prototype
            || missing_placement
            || missing_copy_on_change
            || ctx.any_by(
                node.element_objects(),
                |object| Ok(!has(&object_ids, object)?),
                "FreeCAD validation product members",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} has a missing product-structure link", node.id),
                Some(&node.id),
            )?;
        }
        if has(&cyclic_products, &node.object)? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("{} participates in a product-structure cycle", node.id),
                Some(&node.id),
            )?;
        }
    }
    for joint in ctx.admit_iter(&joints, "FreeCAD validation joints")? {
        if !has(&object_ids, joint.object())?
            || ctx.any_by(
                joint.references(),
                missing_link,
                "FreeCAD validation link search",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "{} has missing operands or invalid connector frames",
                    joint.id()
                ),
                Some(joint.id()),
            )?;
        }
    }
    for drawing in ctx.admit_iter(&drawings, "FreeCAD validation drawings")? {
        if !has(&object_ids, &drawing.object)?
            || missing_in(&drawing.sources)?
            || missing_entry(&drawing.side_entries)?
            || missing_in_groups(&drawing.relationships)?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("{} has a missing drawing object or side entry", drawing.id),
                Some(&drawing.id),
            )?;
        }
    }
    let (object_by_id, _object_index_storage) =
        ctx.with_scoped_storage("FreeCAD validation object index", || {
            ctx.collect_hash_map(
                objects.iter().map(|object| (object.id().as_str(), object)),
                "FreeCAD validation object index",
            )
        })?;
    let mut annotation_objects_storage =
        ctx.reserve_scoped(0, "FreeCAD validation annotation objects")?;
    let mut annotation_objects = HashSet::new();
    let mut annotations_unique = true;
    for annotation in ctx.admit_iter(&annotations, "FreeCAD validation annotations")? {
        let kind_matches = match ctx.get_hash_map(
            &object_by_id,
            annotation.object.as_str(),
            "FreeCAD validation object index",
        )? {
            Some(object) => ctx.equal(
                object.type_name.as_str(),
                annotation.kind.as_str(),
                "FreeCAD validation annotation kind",
            )?,
            None => false,
        };
        if !kind_matches
            || missing_in_groups(&annotation.references)?
            || missing_entry(&annotation.side_entries)?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "{} has a missing annotation object, target, or asset",
                    annotation.id
                ),
                Some(&annotation.id),
            )?;
        }
        annotations_unique &= annotation_objects_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut annotation_objects,
                annotation.object.as_str(),
                "FreeCAD validation annotation objects",
            )
        })?;
    }
    // Every annotation object is annotated exactly once: the annotated
    // objects are distinct, and the annotation-typed objects are exactly them.
    let mut annotation_typed = 0_usize;
    let mut annotated_typed = 0_usize;
    for object in ctx.admit_iter(&objects, "FreeCAD validation objects")? {
        if annotation::is_annotation_type(&object.type_name) {
            annotation_typed += 1;
            annotated_typed += usize::from(has(&annotation_objects, object.id())?);
        }
    }
    if !annotations_unique
        || annotation_typed != annotated_typed
        || annotated_typed != annotation_objects.len()
    {
        push_finding(
            ctx,
            findings,
            Check::Identity,
            format_args!(
                "FCStd semantic annotation graph does not cover every annotation object exactly once"
            ),
            None,
        )?;
    }
    drop((annotation_objects, annotation_objects_storage));
    let mut extension_storage = ctx.reserve_scoped(0, "FreeCAD validation extensions")?;
    let mut extension_names = HashSet::new();
    let mut extension_types = HashSet::new();
    for extension in ctx.admit_iter(&extensions, "FreeCAD validation extensions")? {
        if !has(&object_ids, &extension.owner)? {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} has missing owner {}", extension.id, extension.owner),
                Some(&extension.id),
            )?;
        }
        if !extension_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut extension_names,
                (extension.owner.as_str(), extension.name.as_str()),
                "FreeCAD validation extensions",
            )
        })? {
            push_finding(
                ctx,
                findings,
                Check::Identity,
                format_args!(
                    "{} duplicates extension name {}",
                    extension.id, extension.name
                ),
                Some(&extension.id),
            )?;
        }
        if !extension_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut extension_types,
                (extension.owner.as_str(), extension.type_name.as_str()),
                "FreeCAD validation extensions",
            )
        })? {
            push_finding(
                ctx,
                findings,
                Check::Identity,
                format_args!(
                    "{} duplicates extension type {}",
                    extension.id, extension.type_name
                ),
                Some(&extension.id),
            )?;
        }
    }
    drop((extension_names, extension_types, extension_storage));
    let document_owner = native::native_id("document", "0");
    for property in ctx.admit_iter(&properties, "FreeCAD validation properties")? {
        if !ctx.equal(
            property.owner.as_str(),
            document_owner.as_str(),
            "FreeCAD validation property owner",
        )? && !has(&object_ids, &property.owner)?
            && !has(&extension_ids, &property.owner)?
        {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} has missing owner {}", property.id, property.owner),
                Some(&property.id),
            )?;
        }
        for link in ctx.admit_iter(property.links(), "FreeCAD validation properties")? {
            let Some(target) = link.as_ref().and_then(native::LinkTarget::object) else {
                continue;
            };
            if ctx.starts_with(
                target,
                "fcstd:native:object#",
                "FreeCAD validation link target",
            )? && !has(&object_ids, target)?
            {
                push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!("{} has missing link target {target}", property.id),
                    Some(&property.id),
                )?;
            }
        }
    }
    for table in ctx.admit_iter(string_tables, "FreeCAD validation string tables")? {
        let missing_owner = match &table.owner_property {
            Some(owner) => !has(&property_ids, owner)?,
            None => false,
        };
        let missing_source = match &table.source_entry {
            Some(entry) => !has(&entry_names, entry)?,
            None => false,
        };
        if missing_owner || missing_source {
            let table_id = table.id();
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{table_id} has a missing property or side-entry link"),
                Some(&table_id),
            )?;
        }
    }
    if !element_maps.is_empty() {
        validate_element_maps(
            ctx,
            ir,
            &element_maps,
            string_tables,
            &property_ids,
            &entry_names,
            findings,
        )?;
    }
    validate_side_entry_references(
        ctx,
        &properties,
        &gui_properties,
        &gui_documents,
        &entries,
        findings,
    )?;
    let physical_end = match &ir.source {
        Some(source) => match ctx.get_btree_map(
            &source.attributes,
            "physical_archive_bytes",
            "FreeCAD physical archive length",
        )? {
            Some(value) => ctx
                .parse_text::<u64>(value, "FreeCAD physical archive length")?
                .ok(),
            None => None,
        },
        None => None,
    };
    {
        let (mut ordered, _ordered_storage) = ctx
            .with_scoped_storage("FreeCAD archive span chain sort", || {
                ctx.collect_vec(physical.iter(), "FreeCAD archive span chain sort")
            })?;
        ctx.stable_sort_by_key(
            &mut ordered,
            |value| value.span.start(),
            Ord::cmp,
            "FreeCAD archive span chain sort",
        )?;
        // With no stated end, the last span's end closes the chain.
        let end = physical_end
            .or_else(|| ordered.last().map(|span| span.span.end()))
            .unwrap_or_default();
        if !container::chain_is_exact(
            ctx,
            ordered.iter().map(|span| &span.span),
            end,
            "FreeCAD archive span chain",
        )? {
            push_finding(
                ctx,
                findings,
                Check::PayloadIntegrity,
                format_args!("physical archive ledger has a gap, overlap, or invalid boundary"),
                None,
            )?;
        }
    }
    validate_logical_ledger(
        ctx,
        &logical,
        &LedgerOwners {
            entries: &entries,
            gui_properties: &gui_properties,
            gui_documents: &gui_documents,
            shape_payloads: &shape_payloads,
            string_tables,
            element_maps: &element_maps,
        },
        &property_ids,
        findings,
    )?;
    let (expected_coverage, _expected_coverage_storage) =
        ctx.with_scoped_storage("FreeCAD expected byte coverage", || {
            container::byte_coverage(
                ctx,
                &physical,
                &entries,
                &logical,
                physical_end.unwrap_or_default(),
            )
        })?;
    if first_difference(
        ctx,
        &coverage_records,
        std::slice::from_ref(&expected_coverage),
        "FreeCAD byte coverage comparison",
    )?
    .is_some()
        || !expected_coverage.exact
    {
        push_finding(
            ctx,
            findings,
            Check::PayloadIntegrity,
            format_args!("FCStd byte coverage report is stale or does not prove exact closure"),
            None,
        )?;
    }
    Ok(std::mem::take(findings))
}

/// Element maps name a present property, string table and side entry, and
/// their mapped names carry known string identities and neutral topology.
fn validate_element_maps(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    element_maps: &[native::element_map::ElementMapRecord],
    string_tables: &[native::StringTableRecord],
    property_ids: &HashSet<&str>,
    entry_names: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "FreeCAD validation element maps";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    // Each table's string identities, built once.
    let mut known_ids = Vec::new();
    for table in ctx.admit_iter(string_tables, OPERATION)? {
        let ids = storage.with_storage(|| {
            ctx.collect_hash_set(
                table.entries().iter().map(|entry| entry.string_id),
                OPERATION,
            )
        })?;
        ctx.push_scoped_vec(&mut storage, &mut known_ids, ids, OPERATION)?;
    }
    let model = &ir.model;
    let topology_ids = storage.with_storage(|| {
        ctx.collect_hash_set(
            model
                .vertices
                .iter()
                .map(|entity| entity.id.as_str())
                .chain(model.edges.iter().map(|entity| entity.id.as_str()))
                .chain(model.loops.iter().map(|entity| entity.id.as_str()))
                .chain(model.faces.iter().map(|entity| entity.id.as_str()))
                .chain(model.shells.iter().map(|entity| entity.id.as_str()))
                .chain(model.bodies.iter().map(|entity| entity.id.as_str())),
            OPERATION,
        )
    })?;
    for map in ctx.admit_iter(element_maps, OPERATION)? {
        let missing_source = match &map.source_entry {
            Some(entry) => !ctx.contains_hash_set(entry_names, entry.as_str(), OPERATION)?,
            None => false,
        };
        if !ctx.contains_hash_set(property_ids, map.property.as_str(), OPERATION)?
            || map
                .hasher_index
                .is_some_and(|index| index >= string_tables.len())
            || missing_source
        {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!(
                    "{} has a missing property, string table, or side entry",
                    map.id
                ),
                Some(&map.id),
            )?;
        }
        let known = map.hasher_index.and_then(|index| known_ids.get(index));
        for group in ctx.admit_iter(&map.maps.root().groups, OPERATION)? {
            for names in ctx.admit_iter(&group.names, OPERATION)? {
                for name in ctx.admit_iter(names, OPERATION)? {
                    if let Some(known) = known {
                        if ctx.any_by(
                            &name.string_ids,
                            |id| Ok(!ctx.contains_hash_set(known, id, OPERATION)?),
                            OPERATION,
                        )? {
                            push_finding(
                                ctx,
                                findings,
                                Check::ReferentialIntegrity,
                                format_args!(
                                    "{} references a missing persistent string id",
                                    map.id
                                ),
                                Some(&map.id),
                            )?;
                        }
                    }
                    if ctx.any_by(
                        &name.topology_ids,
                        |id| Ok(!ctx.contains_hash_set(&topology_ids, id.as_str(), OPERATION)?),
                        OPERATION,
                    )? {
                        push_finding(
                            ctx,
                            findings,
                            Check::ReferentialIntegrity,
                            format_args!("{} references missing neutral topology", map.id),
                            Some(&map.id),
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Each archive entry lists exactly the records that name it, once each, in
/// record order: document properties, then GUI properties, then GUI states.
fn validate_side_entry_references<'r>(
    ctx: &DecodeContext<'_>,
    properties: &'r [native::PropertyRecord],
    gui_properties: &'r [native::GuiPropertyRecord],
    gui_documents: &'r [native::GuiDocumentRecord],
    entries: &'r [native::EntryRecord],
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "FreeCAD validation side-entry references";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut owners = HashSet::new();
    let mut expected = HashMap::<&str, Vec<&str>>::new();
    let mut seen = HashSet::new();
    let mut add_record = |names: &'r [String], owner: &'r str| -> Result<(), CodecError> {
        storage.with_storage(|| ctx.insert_hash_set(&mut owners, owner, OPERATION))?;
        for name in ctx.admit_iter(names, OPERATION)? {
            if storage.with_storage(|| {
                ctx.insert_hash_set(&mut seen, (name.as_str(), owner), OPERATION)
            })? {
                storage.with_storage(|| {
                    let list = ctx
                        .entry_hash_map(&mut expected, name.as_str(), OPERATION)?
                        .or_default();
                    ctx.push_vec(list, owner, OPERATION)
                })?;
            }
        }
        Ok(())
    };
    for property in ctx.admit_iter(properties, OPERATION)? {
        add_record(property.side_entries(), &property.id)?;
    }
    for property in ctx.admit_iter(gui_properties, OPERATION)? {
        add_record(&property.side_entries, &property.id)?;
    }
    for document in ctx.admit_iter(gui_documents, OPERATION)? {
        for state in ctx.admit_iter(&document.states, OPERATION)? {
            add_record(&state.side_entries, &state.id)?;
        }
    }
    for entry in ctx.admit_iter(entries, OPERATION)? {
        for owner in ctx.admit_iter(entry.referenced_by(), OPERATION)? {
            if !ctx.contains_hash_set(&owners, owner.as_str(), OPERATION)? {
                push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!("{} has missing referencing record {owner}", entry.id()),
                    Some(entry.id()),
                )?;
            }
        }
        let expected = ctx
            .get_hash_map(&expected, entry.name(), OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        let stored = entry.referenced_by();
        let mut pairs = stored.iter().zip(expected);
        let mut matches = stored.len() == expected.len();
        while matches {
            let Some((stored, expected)) = ctx.next_charged(&mut pairs, OPERATION)? else {
                break;
            };
            matches = ctx.equal(stored.as_str(), *expected, OPERATION)?;
        }
        if !matches {
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{} has a stale side-entry reference relation", entry.id()),
                Some(entry.id()),
            )?;
        }
    }
    Ok(())
}

/// The records whose identities may own logical spans besides the document
/// properties.
struct LedgerOwners<'r> {
    entries: &'r [native::EntryRecord],
    gui_properties: &'r [native::GuiPropertyRecord],
    gui_documents: &'r [native::GuiDocumentRecord],
    shape_payloads: &'r [brep::ShapePayloadRecord],
    string_tables: &'r [native::StringTableRecord],
    element_maps: &'r [native::element_map::ElementMapRecord],
}

/// Every logical span names a present entry and owner, every nonempty entry
/// has spans, and each entry's spans tile it.
fn validate_logical_ledger(
    ctx: &DecodeContext<'_>,
    logical: &[native::LogicalSpan],
    records: &LedgerOwners<'_>,
    property_ids: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "FreeCAD validation logical ledger";
    let entries = records.entries;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut entry_lengths = HashMap::new();
    for entry in ctx.admit_iter(entries, OPERATION)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut entry_lengths,
                entry.name(),
                entry.byte_len(),
                OPERATION,
            )
        })?;
    }
    let string_table_ids = storage.with_storage(|| {
        ctx.collect_vec(
            records
                .string_tables
                .iter()
                .map(native::StringTableRecord::id),
            OPERATION,
        )
    })?;
    let mut owner_ids = HashSet::new();
    let mut add_owner = |owner| {
        storage
            .with_storage(|| ctx.insert_hash_set(&mut owner_ids, owner, OPERATION))
            .map(drop)
    };
    for record in ctx.admit_iter(records.gui_properties, OPERATION)? {
        add_owner(record.id.as_str())?;
    }
    for document in ctx.admit_iter(records.gui_documents, OPERATION)? {
        for state in ctx.admit_iter(&document.states, OPERATION)? {
            add_owner(state.id.as_str())?;
        }
    }
    for record in ctx.admit_iter(records.shape_payloads, OPERATION)? {
        add_owner(record.id.as_str())?;
    }
    for id in ctx.admit_iter(&string_table_ids, OPERATION)? {
        add_owner(id.as_str())?;
    }
    for record in ctx.admit_iter(records.element_maps, OPERATION)? {
        add_owner(record.id.as_str())?;
    }
    for entry in ctx.admit_iter(entries, OPERATION)? {
        add_owner(entry.id())?;
    }
    let mut by_entry = BTreeMap::<&str, Vec<&native::LogicalSpan>>::new();
    for span in ctx.admit_iter(logical, OPERATION)? {
        storage.with_storage(|| {
            let spans = ctx
                .entry_btree_map(&mut by_entry, span.entry.as_str(), OPERATION)?
                .or_default();
            ctx.push_vec(spans, span, OPERATION)
        })?;
        let owner_valid = match &span.classification {
            native::LogicalClassification::Structural => true,
            native::LogicalClassification::Typed { owner }
            | native::LogicalClassification::NamedOpaque { owner } => {
                ctx.contains_hash_set(property_ids, owner.as_str(), OPERATION)?
                    || ctx.contains_hash_set(&owner_ids, owner.as_str(), OPERATION)?
            }
        };
        if !ctx.contains_key_hash_map(&entry_lengths, span.entry.as_str(), OPERATION)?
            || !owner_valid
        {
            push_finding(
                ctx,
                findings,
                Check::PayloadIntegrity,
                format_args!("{} has an invalid logical entry or owner", span.id),
                Some(&span.id),
            )?;
        }
    }
    for entry in ctx.admit_iter(entries, OPERATION)? {
        if entry.byte_len() > 0
            && !ctx.contains_key_btree_map(&by_entry, entry.name(), OPERATION)?
        {
            push_finding(
                ctx,
                findings,
                Check::PayloadIntegrity,
                format_args!("logical ledger omits nonempty entry {}", entry.name()),
                Some(entry.id()),
            )?;
        }
    }
    for (name, mut spans) in ctx.admit_iter(by_entry, OPERATION)? {
        ctx.stable_sort_by_key(
            &mut spans,
            |value| value.span.start(),
            Ord::cmp,
            "fcstd logical spans sort",
        )?;
        let exact = match ctx.get_hash_map(&entry_lengths, name, OPERATION)? {
            Some(&end) => {
                container::chain_is_exact(ctx, spans.iter().map(|span| &span.span), end, OPERATION)?
            }
            None => false,
        };
        if !exact {
            push_finding(
                ctx,
                findings,
                Check::PayloadIntegrity,
                format_args!("logical ledger for {name} has a gap, overlap, or invalid boundary"),
                None,
            )?;
        }
    }
    Ok(())
}

/// The index of the first differing pair, `Some(None)` for equal prefixes of
/// different lengths, or `None` for equal slices; each pair compared is charged.
fn first_difference<T: PartialEq + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    stored: &[T],
    derived: &[T],
    operation: &'static str,
) -> Result<Option<Option<usize>>, CodecError> {
    let mut derived_values = derived.iter();
    let index = ctx.position_by(
        stored.iter().take(derived.len()),
        |stored| {
            let derived = derived_values
                .next()
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
            Ok(!ctx.equal(stored, derived, operation)?)
        },
        operation,
    )?;
    Ok(match index {
        Some(index) => Some(Some(index)),
        None if stored.len() != derived.len() => Some(None),
        None => None,
    })
}

fn push_finding(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    check: Check,
    message: std::fmt::Arguments<'_>,
    entity: Option<&str>,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(message, "FreeCAD native validation finding")?;
    let entity = entity
        .map(|entity| ctx.copy_retained_text(entity, "FreeCAD finding entity"))
        .transpose()?;
    ctx.push_vec(
        findings,
        Finding {
            check,
            severity: FindingSeverity::Error,
            message,
            entity,
        },
        FINDINGS,
    )
}

fn single_finding(
    ctx: &DecodeContext<'_>,
    check: Check,
    message: std::fmt::Arguments<'_>,
) -> Result<Vec<Finding>, CodecError> {
    let mut findings = Vec::new();
    push_finding(ctx, &mut findings, check, message, None)?;
    Ok(findings)
}

impl CodecBackend for FcstdCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<Vec<Finding>, CodecError> {
        crate::validate_native(ctx, ir)
    }

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let prefix = prefix.window();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(prefix.len()),
            "detect input",
        )?;
        if !prefix.starts_with(b"PK\x03\x04") {
            return Ok(Confidence::No);
        }
        if container::has_document_markers(ctx, prefix)? {
            Ok(Confidence::High)
        } else if ctx.contains_bytes(prefix, b"Document.xml", "detect FreeCAD document marker")? {
            Ok(Confidence::Medium)
        } else {
            Ok(Confidence::Low)
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        container::summarize(ctx, scan)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        let mut scan = container::scan(ctx, root)?;
        let mut admitted_entities = 0_u64;
        let mut attributes = container::source_attributes(ctx, &scan)?;
        let mut thumbnail = None;
        for name in ["thumbnails/Thumbnail.png", "Thumbnail.png"] {
            if let Some(view) = ctx.get_btree_map(&scan.data, name, "FCStd thumbnail lookup")? {
                thumbnail = Some((name, view.window()));
                break;
            }
        }
        if let Some((_, thumbnail)) = thumbnail {
            ctx.insert_btree_map(
                &mut attributes,
                cadmpeg_core::nonblank_literal!("thumbnail_bytes"),
                thumbnail.len().to_string(),
                "FCStd source attribute records",
            )?;
        }
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
        let mut geometry_transferred = false;
        let mut cycle_affected_design_objects = BTreeSet::new();
        let mut gui_losses = Vec::new();
        let mut topology_losses = Vec::new();
        // One `classify` call feeds the report identity, loss, and notes.
        let primary = dialect::FcstdDialect::classify(ctx, &scan.document, &scan.schema_version)?;
        let dialects = cadmpeg_core::dialect::DialectLayers::of(primary);
        let mut ir = CadIr::decoded(SourceMeta::classified(
            dialects.try_clone_for_decode(ctx, "copy FreeCAD dialect layers")?,
            attributes,
        ));
        if let Some((name, bytes)) = thumbnail {
            source_fidelity.attach_native_unknown_records(
                &mut ir,
                "fcstd",
                [UnknownRecord::retained(
                    UnknownId::compose(
                        &cadmpeg_ir::identity_namespace!("fcstd", "native", "thumbnail"),
                        cadmpeg_ir::ids::IdentityKey::encode_segment(name),
                    ),
                    0,
                    ctx.copy_retained(bytes, "retain FCStd thumbnail")?,
                    vec![native::native_id("document", "0")],
                )]
                .into(),
                ctx,
            )?;
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.set_arena(ctx, "document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena(ctx, "physical_ledger", &scan.ledger)?;
        let decode_document = !ctx.container_only();
        if decode_document {
            // The scan's Document.xml tree serves persistence and element maps,
            // then is dropped before the shape payloads are read.
            let document_xml = scan.document_xml.take().ok_or_else(|| {
                CodecError::Malformed("Document.xml disappeared after scan".into())
            })?;
            let graph = persistence::parse_document(
                document_xml.text,
                document_xml.xml.document(),
                dialect::FcstdDialect::from_schema_version(&scan.schema_version),
                ctx,
            )?;
            for property in ctx.admit_iter(&graph.properties, "FCStd side entry check")? {
                for side_entry in
                    ctx.admit_iter(property.side_entries(), "FCStd side entry check")?
                {
                    if !ctx.contains_key_btree_map(
                        &scan.data,
                        side_entry.as_str(),
                        "FCStd archive entry map",
                    )? {
                        return Err(CodecError::Malformed(ctx.format_retained(
                            format_args!(
                                "property {} references missing side entry {side_entry}",
                                property.id
                            ),
                            "FCStd missing side entry diagnostic",
                        )?));
                    }
                }
            }
            let mut entry_records = container::entry_records(ctx, &scan, &graph.properties)?;
            let (string_tables, mut element_maps) = element_map::parse(
                ctx,
                document_xml.xml.document(),
                scan.document.file_version.value(),
                &graph.properties,
                &entry_records,
            )?;
            drop(document_xml);
            let shape_payloads = brep::parse_payloads(ctx, &graph.properties, &entry_records)?;
            namespace.set_arena(ctx, "objects", &graph.objects)?;
            namespace.set_arena(ctx, "extensions", &graph.extensions)?;
            namespace.set_arena(ctx, "properties", &graph.properties)?;
            namespace.set_arena(ctx, "shape_payloads", &shape_payloads)?;
            namespace.set_arena(
                ctx,
                "carrier_census",
                &brep::carrier_census(ctx, &shape_payloads)?,
            )?;
            namespace.set_arena(ctx, "string_tables", string_tables.as_slice())?;
            let product_nodes =
                product::transfer(ctx, &graph.objects, &graph.properties, &scan.data)?;
            namespace.set_arena(ctx, "product_nodes", &product_nodes)?;
            let joint_records = joint::transfer(ctx, &graph.objects, &graph.properties)?;
            namespace.set_arena(ctx, "joints", &joint_records)?;
            let drawings = drawing::transfer(ctx, &graph.objects, &graph.properties)?;
            drawing::transfer_neutral(ctx, &mut ir.model, &drawings, &graph.properties)?;
            namespace.set_arena(ctx, "drawings", &drawings)?;
            let annotations = annotation::transfer(ctx, &graph.objects, &graph.properties)?;
            annotation::transfer_neutral(
                ctx,
                &mut ir.model,
                &annotations,
                &graph.properties,
                &drawings,
            )?;
            namespace.set_arena(ctx, "annotations", &annotations)?;
            application::install(
                ctx,
                namespace,
                &graph.objects,
                &graph.properties,
                &entry_records,
            )?;
            let attachments = attachment::transfer(ctx, &graph.objects, &graph.properties)?;
            namespace.set_arena(ctx, "attachments", &attachments)?;
            let (curve_transfer, surface_transfer) =
                brep::transfer_text_geometry(ctx, &shape_payloads, &graph.properties)?;
            geometry_transferred =
                !curve_transfer.curves.is_empty() || !surface_transfer.surfaces.is_empty();
            ir.model.curves = curve_transfer.curves;
            for (owner, procedural) in
                ctx.admit_iter(curve_transfer.procedural, "FCStd procedural curves")?
            {
                ir.model
                    .add_procedural_curve(ctx, &owner, procedural)?
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
            }
            ir.model.surfaces = surface_transfer.surfaces;
            for (owner, procedural) in
                ctx.admit_iter(surface_transfer.procedural, "FCStd procedural surfaces")?
            {
                ir.model
                    .add_procedural_surface(ctx, &owner, procedural)?
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
            }
            geometry_transferred |= application_geometry::transfer(
                ctx,
                &mut ir,
                &graph.properties,
                &entry_records,
                &mut admitted_entities,
            )?;
            let topology_occurrences = topology_transfer::transfer(
                ctx,
                &mut ir,
                &shape_payloads,
                &graph.properties,
                &mut topology_losses,
            )?;
            cycle_affected_design_objects = design::transfer(
                ctx,
                &mut ir,
                &graph.objects,
                &graph.properties,
                &shape_payloads,
                &entry_records,
                scan.document.program_version.as_deref(),
            )?;
            let (product_definitions, occurrences) = product::transfer_neutral(
                ctx,
                &product_nodes,
                &joint_records,
                &graph.objects,
                &graph.properties,
                &shape_payloads,
                &ir.model.bodies,
            )?;
            ir.model.product_definitions = product_definitions;
            ir.model.occurrences = occurrences;
            ir.model.assembly_joints =
                joint::transfer_neutral(ctx, &joint_records, &ir.model.occurrences)?;
            ctx.admit_entities(
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            let design_census = design::census(ctx, &graph.objects, &ir.model.features)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "design_census", &design_census)?;
            element_map::bind_topology(ctx, &mut element_maps, &topology_occurrences)?;
            let mut gui_graph = if let Some(gui_view) = scan.data.get("GuiDocument.xml") {
                gui::transfer(
                    ctx,
                    &mut ir,
                    gui_view.window(),
                    &gui::GuiSources {
                        entries: &scan.data,
                        objects: &graph.objects,
                        properties: &graph.properties,
                        payloads: &shape_payloads,
                        element_maps: &element_maps,
                        requires_alpha_conversion: gui::requires_alpha_conversion(
                            scan.document.program_version.as_deref(),
                        ),
                    },
                )?
            } else {
                gui::Graph::default()
            };
            gui_losses = std::mem::take(&mut gui_graph.losses);
            ctx.admit_entities(
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            bind_gui_entry_references(ctx, &mut entry_records, &gui_graph)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "entries", &entry_records)?;
            ir.native.namespace_mut("fcstd").set_arena(
                ctx,
                "gui_documents",
                &gui_graph.documents,
            )?;
            ir.native.namespace_mut("fcstd").set_arena(
                ctx,
                "gui_view_providers",
                &gui_graph.providers,
            )?;
            ir.native.namespace_mut("fcstd").set_arena(
                ctx,
                "gui_properties",
                &gui_graph.properties,
            )?;
            let logical_ledger = container::logical_ledger(
                ctx,
                &entry_records,
                &graph.properties,
                &gui_graph,
                &shape_payloads,
                string_tables.as_slice(),
                &element_maps,
            )?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "logical_ledger", &logical_ledger)?;
            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.span.end());
            let coverage = container::byte_coverage(
                ctx,
                &scan.ledger,
                &entry_records,
                &logical_ledger,
                physical_byte_len,
            )?;
            ir.native.namespace_mut("fcstd").set_arena(
                ctx,
                "byte_coverage",
                std::slice::from_ref(&coverage),
            )?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "element_maps", &element_maps)?;
        } else {
            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.span.end());
            let coverage =
                container::byte_coverage(ctx, &scan.ledger, &[], &[], physical_byte_len)?;
            ir.native.namespace_mut("fcstd").set_arena(
                ctx,
                "byte_coverage",
                std::slice::from_ref(&coverage),
            )?;
        }
        let mut losses = if ctx.container_only() {
            Vec::new()
        } else {
            semantic_losses(ctx, &ir, &cycle_affected_design_objects, gui_losses)?
        };
        // Charged on both decode branches: a schema outside the declared rows
        // is read with the schema-4 strategy on either path, so the charge is
        // not conditioned on the branch.
        ctx.append_vec(
            &mut losses,
            &mut topology_losses,
            "FCStd topology loss output",
        )?;
        if let Some(loss) = dialect::FcstdDialect::dialect_loss(dialects.primary()) {
            ctx.push_vec(&mut losses, loss, "FCStd dialect loss output")?;
        }
        ctx.admit_entities(
            cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
            &mut admitted_entities,
            "admit FCStd entities",
        )?;
        let summary_notes = container::summary_notes(ctx, &scan)?;
        Ok(Decoded {
            ir,
            body: DecodeBody {
                transfer: if ctx.container_only() {
                    cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
                } else {
                    cadmpeg_ir::report::decode::DecodeTransfer::full(geometry_transferred)
                },
                coverage: cadmpeg_ir::report::decode::Coverage::default(),
                losses,
                notes: summary_notes,
                transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
            },
            source_fidelity,
        })
    }
}

impl EncoderBackend for FcstdCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, None);

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::target::plan(input, &target)
    }
}

/// Adds each GUI property and state as a referencing owner of the archive
/// entries it names, once per entry, through one index of entry names.
fn bind_gui_entry_references<'g>(
    ctx: &DecodeContext<'_>,
    entry_records: &mut [native::EntryRecord],
    gui_graph: &'g gui::Graph,
) -> Result<(), CodecError> {
    const OPERATION: &str = "FCStd GUI entry references";
    let (entry_index, _entry_index_storage) = ctx.unique_index(
        ctx.admit_iter(&*entry_records, OPERATION)?
            .enumerate()
            .map(|(index, entry)| (entry.name(), index)),
        OPERATION,
    )?;
    let mut bound_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut bound = HashSet::new();
    let mut additions = Vec::new();
    let mut bind = |names: &'g [String], owner: &'g str| -> Result<(), CodecError> {
        for name in ctx.admit_iter(names, OPERATION)? {
            // Archive entry names are unique, so every named record is indexed.
            let Some(&Some(index)) = ctx.get_hash_map(&entry_index, name.as_str(), OPERATION)?
            else {
                continue;
            };
            if bound_storage
                .with_storage(|| ctx.insert_hash_set(&mut bound, (index, owner), OPERATION))?
            {
                ctx.push_scoped_vec(
                    &mut bound_storage,
                    &mut additions,
                    (index, owner),
                    OPERATION,
                )?;
            }
        }
        Ok(())
    };
    for property in ctx.admit_iter(&gui_graph.properties, OPERATION)? {
        bind(&property.side_entries, &property.id)?;
    }
    for document in ctx.admit_iter(&gui_graph.documents, OPERATION)? {
        for state in ctx.admit_iter(&document.states, OPERATION)? {
            bind(&state.side_entries, &state.id)?;
        }
    }
    drop(entry_index);
    for (index, owner) in ctx.admit_iter(additions, OPERATION)? {
        entry_records[index].push_reference(ctx, owner)?;
    }
    Ok(())
}

fn semantic_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    cycle_affected_design_objects: &BTreeSet<String>,
    gui_losses: Vec<LossNote>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = gui_losses;
    for feature in ctx.admit_iter(&ir.model.features, "FCStd feature semantic loss")? {
        let definition = match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. }
            | cadmpeg_ir::features::FeatureDefinition::Operation(operation) => operation,
        };
        let cadmpeg_ir::features::FeatureOperation::Native { kind, .. } = definition else {
            continue;
        };
        let cycle_affected = match &feature.native_ref {
            Some(id) => ctx.contains_btree_set(
                cycle_affected_design_objects,
                id.as_str(),
                "FCStd feature semantic loss",
            )?,
            None => false,
        };
        let (code, suffix) = if cycle_affected {
            (
                FreecadLossCode::FeatureCyclicHistory,
                " is retained natively because neutral dependency ordering is cycle-affected",
            )
        } else {
            (
                FreecadLossCode::FeatureNativeKindRetained,
                " is retained natively but has no neutral semantics",
            )
        };
        push_semantic_loss(
            ctx,
            &mut losses,
            code,
            &["FCStd design operation ", kind.as_str(), suffix],
            feature.native_ref.as_deref(),
            "FCStd feature semantic loss",
        )?;
    }
    for entity in ctx.admit_iter(
        &ir.model.sketch_entities,
        "FCStd sketch geometry semantic loss",
    )? {
        let cadmpeg_ir::sketches::SketchGeometryDefinition::Native { native_kind } =
            entity.geometry.definition()
        else {
            continue;
        };
        push_semantic_loss(
            ctx,
            &mut losses,
            FreecadLossCode::SketchNativeGeometry,
            &[
                "FCStd sketch geometry ",
                native_kind.as_str(),
                " is retained natively but is not neutralized",
            ],
            entity.native_ref.as_deref(),
            "FCStd sketch geometry semantic loss",
        )?;
    }
    for constraint in ctx.admit_iter(
        &ir.model.sketch_constraints,
        "FCStd sketch constraint semantic loss",
    )? {
        let cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { native_kind, .. } =
            constraint.definition.kind()
        else {
            continue;
        };
        push_semantic_loss(
            ctx,
            &mut losses,
            FreecadLossCode::SketchNativeConstraint,
            &[
                "FCStd sketch constraint ",
                native_kind.as_str(),
                " is retained natively but is not neutralized",
            ],
            constraint.native_ref.as_deref(),
            "FCStd sketch constraint semantic loss",
        )?;
    }
    Ok(losses)
}

fn push_semantic_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: FreecadLossCode,
    message_parts: &[&str],
    tag: Option<&str>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let message = ctx.join_retained(message_parts, "", operation)?;
    let tag = tag
        .map(|tag| ctx.copy_retained_text(tag, operation))
        .transpose()?;
    ctx.reserve_vec(losses, 1, "FCStd semantic loss output")?;
    losses.push(
        code.note(message).with_provenance(
            cadmpeg_ir::SourceProvenance::in_stream(
                "fcstd",
                cadmpeg_ir::stream_name!("Document.xml"),
                0,
            )
            .with_optional_tag(tag),
        ),
    );
    Ok(())
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
