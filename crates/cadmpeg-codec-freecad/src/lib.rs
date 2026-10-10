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
use crate::native::element_map::ScopedData;

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
    const IDENTITIES: &str = "FreeCAD validation identities";
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
    // The shared conversion admits each visited position and its diagnostic.
    let string_tables = arena!(native::StringTables::from_records_with_admission(
        string_table_records,
        ctx
    )
    .map_err(cadmpeg_ir::native::NativeConvertError::from)?);
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
        let expected = ctx.with_scoped_storage("FreeCAD expected carrier census", || {
            brep::carrier_census(ctx, &shape_payloads)
        })?;
        let _expected_storage = expected.1;
        let expected = expected.0;
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
        Ok(expected) => {
            let _expected_storage = expected.1;
            let expected = expected.0;
            match first_difference(ctx, &design_census, &expected, "FreeCAD design census comparison")? {
                None => {}
                Some(SliceDifference::Pair(index)) => push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!(
                        "FCStd design census does not match projected feature semantics: stored {:?} but derived {:?}",
                        design_census[index], expected[index]
                    ),
                    None,
                )?,
                Some(SliceDifference::Length) => push_finding(
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
        while groups.len() != 0 {
            let Some(links) = ctx.next_charged(&mut groups, "FreeCAD validation link search")?
            else {
                break;
            };
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
    let mut object_sources = objects.iter();
    while object_sources.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_sources, "FreeCAD validation objects")?
        else {
            break;
        };
        let mut dependency_sources = object.dependencies.iter();
        while dependency_sources.len() != 0 {
            let Some(dependency) =
                ctx.next_charged(&mut dependency_sources, "FreeCAD validation objects")?
            else {
                break;
            };
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
    let mut attachment_sources = attachments.iter();
    while attachment_sources.len() != 0 {
        let Some(attachment) =
            ctx.next_charged(&mut attachment_sources, "FreeCAD validation attachments")?
        else {
            break;
        };
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
        Ok(expected) => {
            let _expected_storage = expected.1;
            let expected = expected.0;
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
    let mut document_sources = gui_documents.iter();
    while document_sources.len() != 0 {
        let Some(document) =
            ctx.next_charged(&mut document_sources, "FreeCAD validation GUI documents")?
        else {
            break;
        };
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
    let mut provider_sources = gui_providers.iter();
    while provider_sources.len() != 0 {
        let Some(provider) =
            ctx.next_charged(&mut provider_sources, "FreeCAD validation GUI providers")?
        else {
            break;
        };
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
    let mut property_sources = gui_properties.iter();
    let first_property = if property_sources.len() == 0 {
        None
    } else {
        ctx.next_charged(&mut property_sources, "FreeCAD validation GUI properties")?
    };
    if let Some(mut property) = first_property {
        let ids = ctx.with_scoped_storage(IDENTITIES, || {
            ctx.collect_hash_set(
                gui_providers.iter().map(|provider| provider.id.as_str()),
                IDENTITIES,
            )
        })?;
        let _id_storage = ids.1;
        let gui_provider_ids = ids.0;
        loop {
            if !has(&gui_provider_ids, &property.owner)? || missing_entry(&property.side_entries)? {
                push_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    format_args!("{} has a missing GUI owner or side entry", property.id),
                    Some(&property.id),
                )?;
            }
            if property_sources.len() == 0 {
                break;
            }
            let Some(next) =
                ctx.next_charged(&mut property_sources, "FreeCAD validation GUI properties")?
            else {
                break;
            };
            property = next;
        }
    }
    let cyclic_products = ctx.with_scoped_storage("fcstd product cycle lookup", || {
        product::product_cycle_nodes(ctx, &product_nodes)
    })?;
    let _cycle_storage = cyclic_products.1;
    let cyclic_products = cyclic_products.0;
    let mut node_sources = product_nodes.iter();
    while node_sources.len() != 0 {
        let Some(node) = ctx.next_charged(&mut node_sources, "FreeCAD validation product nodes")?
        else {
            break;
        };
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
        if ctx.contains_btree_set(
            &cyclic_products,
            node.object.as_str(),
            "FreeCAD validation product cycles",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("{} participates in a product-structure cycle", node.id),
                Some(&node.id),
            )?;
        }
    }
    drop(cyclic_products);
    drop(_cycle_storage);
    let mut joint_sources = joints.iter();
    while joint_sources.len() != 0 {
        let Some(joint) = ctx.next_charged(&mut joint_sources, "FreeCAD validation joints")? else {
            break;
        };
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
    let mut drawing_sources = drawings.iter();
    while drawing_sources.len() != 0 {
        let Some(drawing) =
            ctx.next_charged(&mut drawing_sources, "FreeCAD validation drawings")?
        else {
            break;
        };
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
    let mut annotation_objects_storage =
        ctx.reserve_scoped(0, "FreeCAD validation annotation objects")?;
    let mut annotation_objects = HashSet::new();
    let mut annotations_unique = true;
    let mut annotation_sources = annotations.iter();
    let first_annotation = if annotation_sources.len() == 0 {
        None
    } else {
        ctx.next_charged(&mut annotation_sources, "FreeCAD validation annotations")?
    };
    if let Some(mut annotation) = first_annotation {
        let index = ctx.with_scoped_storage("FreeCAD validation object index", || {
            ctx.collect_hash_map(
                objects.iter().map(|object| (object.id().as_str(), object)),
                "FreeCAD validation object index",
            )
        })?;
        let _index_storage = index.1;
        let object_by_id = index.0;
        loop {
            let kind_matches = match ctx.get_hash_map(
                &object_by_id,
                annotation.object.as_str(),
                "FreeCAD validation object index",
            )? {
                Some(object) => object.type_name == annotation.kind.as_str(),
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
            if annotation_sources.len() == 0 {
                break;
            }
            let Some(next) =
                ctx.next_charged(&mut annotation_sources, "FreeCAD validation annotations")?
            else {
                break;
            };
            annotation = next;
        }
    }
    // Every annotation object is annotated exactly once: the annotated
    // objects are distinct, and the annotation-typed objects are exactly them.
    let mut annotation_typed = 0_usize;
    let mut annotated_typed = 0_usize;
    let mut object_sources = objects.iter();
    while object_sources.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_sources, "FreeCAD validation objects")?
        else {
            break;
        };
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
    let mut extension_sources = extensions.iter();
    while extension_sources.len() != 0 {
        let Some(extension) =
            ctx.next_charged(&mut extension_sources, "FreeCAD validation extensions")?
        else {
            break;
        };
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
    let mut property_sources = properties.iter();
    while property_sources.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut property_sources, "FreeCAD validation properties")?
        else {
            break;
        };
        if property.owner != document_owner
            && !has(&object_ids, &property.owner)?
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
        let mut link_sources = property.links().iter();
        while link_sources.len() != 0 {
            let Some(link) =
                ctx.next_charged(&mut link_sources, "FreeCAD validation properties")?
            else {
                break;
            };
            let Some(target) = link.as_ref().and_then(native::LinkTarget::object) else {
                continue;
            };
            if target.starts_with("fcstd:native:object#") && !has(&object_ids, target)? {
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
    let mut table_sources = string_tables.iter();
    while table_sources.len() != 0 {
        let Some(table) =
            ctx.next_charged(&mut table_sources, "FreeCAD validation string tables")?
        else {
            break;
        };
        let missing_owner = match &table.owner_property {
            Some(owner) => !has(&property_ids, owner)?,
            None => false,
        };
        let missing_source = match &table.source_entry {
            Some(entry) => !has(&entry_names, entry)?,
            None => false,
        };
        if missing_owner || missing_source {
            let mut table_id_storage =
                ctx.reserve_scoped(0, "FreeCAD validation string table identity")?;
            let table_id = table_id_storage.with_storage(|| table.id_with_admission(ctx))?;
            push_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                format_args!("{table_id} has a missing property or side-entry link"),
                Some(&table_id),
            )?;
            drop(table_id);
            drop(table_id_storage);
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
        let ordered = ctx.with_scoped_storage("FreeCAD archive span chain sort", || {
            ctx.collect_vec(physical.iter(), "FreeCAD archive span chain sort")
        })?;
        let _ordered_storage = ordered.1;
        let mut ordered = ordered.0;
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
    let expected_coverage = ctx.with_scoped_storage("FreeCAD expected byte coverage", || {
        container::byte_coverage(
            ctx,
            &physical,
            &entries,
            &logical,
            physical_end.unwrap_or_default(),
        )
    })?;
    let _expected_coverage_storage = expected_coverage.1;
    let expected_coverage = expected_coverage.0;
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
    // Each queried table owns one identity set; unqueried tables have no index.
    let mut known_ids = HashMap::<usize, HashSet<i64>>::new();
    let mut topology_ids = None;
    let mut map_sources = element_maps.iter();
    while map_sources.len() != 0 {
        let Some(map) = ctx.next_charged(&mut map_sources, OPERATION)? else {
            break;
        };
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
        let mut group_sources = map.maps.root().groups.iter();
        while group_sources.len() != 0 {
            let Some(group) = ctx.next_charged(&mut group_sources, OPERATION)? else {
                break;
            };
            let mut names_sources = group.names.iter();
            while names_sources.len() != 0 {
                let Some(names) = ctx.next_charged(&mut names_sources, OPERATION)? else {
                    break;
                };
                let mut name_sources = names.iter();
                while name_sources.len() != 0 {
                    let Some(name) = ctx.next_charged(&mut name_sources, OPERATION)? else {
                        break;
                    };
                    if let Some(index) = map.hasher_index.filter(|_| !name.string_ids.is_empty()) {
                        if let Some(table) = string_tables.get(index) {
                            let known = match storage.with_storage(|| {
                                ctx.entry_hash_map(&mut known_ids, index, OPERATION)
                            })? {
                                std::collections::hash_map::Entry::Occupied(slot) => {
                                    slot.into_mut()
                                }
                                std::collections::hash_map::Entry::Vacant(slot) => {
                                    let mut ids = HashSet::new();
                                    let mut entries = table.entries().iter();
                                    while entries.len() != 0 {
                                        let Some(entry) =
                                            ctx.next_charged(&mut entries, OPERATION)?
                                        else {
                                            break;
                                        };
                                        storage.with_storage(|| {
                                            ctx.insert_hash_set(
                                                &mut ids,
                                                entry.string_id,
                                                OPERATION,
                                            )
                                        })?;
                                    }
                                    slot.insert(ids)
                                }
                            };
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
                    }
                    if !name.topology_ids.is_empty() {
                        let topology_ids = match topology_ids {
                            Some(ref ids) => ids,
                            None => {
                                let model = &ir.model;
                                let mut sources = model
                                    .vertices
                                    .iter()
                                    .map(|entity| entity.id.as_str())
                                    .chain(model.edges.iter().map(|entity| entity.id.as_str()))
                                    .chain(model.loops.iter().map(|entity| entity.id.as_str()))
                                    .chain(model.faces.iter().map(|entity| entity.id.as_str()))
                                    .chain(model.shells.iter().map(|entity| entity.id.as_str()))
                                    .chain(model.bodies.iter().map(|entity| entity.id.as_str()));
                                let mut ids = HashSet::new();
                                while sources.size_hint().1 != Some(0) {
                                    let Some(id) = ctx.next_charged(&mut sources, OPERATION)?
                                    else {
                                        break;
                                    };
                                    storage.with_storage(|| {
                                        ctx.insert_hash_set(&mut ids, id, OPERATION)
                                    })?;
                                }
                                topology_ids.insert(ids)
                            }
                        };
                        if ctx.any_by(
                            &name.topology_ids,
                            |id| {
                                Ok(!ctx.contains_hash_set(topology_ids, id.as_str(), OPERATION)?)
                            },
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
        let mut name_sources = names.iter();
        while name_sources.len() != 0 {
            let Some(name) = ctx.next_charged(&mut name_sources, OPERATION)? else {
                break;
            };
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
    let mut property_sources = properties.iter();
    while property_sources.len() != 0 {
        let Some(property) = ctx.next_charged(&mut property_sources, OPERATION)? else {
            break;
        };
        add_record(property.side_entries(), &property.id)?;
    }
    let mut property_sources = gui_properties.iter();
    while property_sources.len() != 0 {
        let Some(property) = ctx.next_charged(&mut property_sources, OPERATION)? else {
            break;
        };
        add_record(&property.side_entries, &property.id)?;
    }
    let mut document_sources = gui_documents.iter();
    while document_sources.len() != 0 {
        let Some(document) = ctx.next_charged(&mut document_sources, OPERATION)? else {
            break;
        };
        let mut state_sources = document.states.iter();
        while state_sources.len() != 0 {
            let Some(state) = ctx.next_charged(&mut state_sources, OPERATION)? else {
                break;
            };
            add_record(&state.side_entries, &state.id)?;
        }
    }
    let mut entry_sources = entries.iter();
    while entry_sources.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entry_sources, OPERATION)? else {
            break;
        };
        let mut owner_sources = entry.referenced_by().iter();
        while owner_sources.len() != 0 {
            let Some(owner) = ctx.next_charged(&mut owner_sources, OPERATION)? else {
                break;
            };
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
        while matches && pairs.len() != 0 {
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
    if logical.is_empty() {
        let mut entry_sources = entries.iter();
        while entry_sources.len() != 0 {
            let Some(entry) = ctx.next_charged(&mut entry_sources, OPERATION)? else {
                break;
            };
            if entry.byte_len() > 0 {
                push_finding(
                    ctx,
                    findings,
                    Check::PayloadIntegrity,
                    format_args!("logical ledger omits nonempty entry {}", entry.name()),
                    Some(entry.id()),
                )?;
            }
        }
        return Ok(());
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut entry_lengths = HashMap::new();
    let mut entry_sources = entries.iter();
    while entry_sources.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entry_sources, OPERATION)? else {
            break;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut entry_lengths,
                entry.name(),
                entry.byte_len(),
                OPERATION,
            )
        })?;
    }
    let mut owner_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut owner_ids = None;
    let mut string_table_ids = None;
    let mut group_index_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut by_entry = BTreeMap::<&str, ScopedData<'_, Vec<&native::LogicalSpan>>>::new();
    let mut span_sources = logical.iter();
    while span_sources.len() != 0 {
        let Some(span) = ctx.next_charged(&mut span_sources, OPERATION)? else {
            break;
        };
        let group = match group_index_storage
            .with_storage(|| ctx.entry_btree_map(&mut by_entry, span.entry.as_str(), OPERATION))?
        {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(ScopedData {
                data: Vec::new(),
                _storage: ctx.reserve_scoped(0, OPERATION)?,
            }),
        };
        ctx.push_scoped_vec(&mut group._storage, &mut group.data, span, OPERATION)?;
        let owner_valid = match &span.classification {
            native::LogicalClassification::Structural => true,
            native::LogicalClassification::Typed { owner }
            | native::LogicalClassification::NamedOpaque { owner } => {
                if ctx.contains_hash_set(property_ids, owner.as_str(), OPERATION)? {
                    true
                } else {
                    let known = match owner_ids {
                        Some(ref ids) => ids,
                        None => {
                            let mut ids = HashSet::new();
                            let mut add_owner = |owner| {
                                owner_storage
                                    .with_storage(|| {
                                        ctx.insert_hash_set(&mut ids, owner, OPERATION)
                                    })
                                    .map(drop)
                            };
                            let mut sources = records.gui_properties.iter();
                            while sources.len() != 0 {
                                let Some(record) = ctx.next_charged(&mut sources, OPERATION)?
                                else {
                                    break;
                                };
                                add_owner(record.id.as_str())?;
                            }
                            let mut sources = records.gui_documents.iter();
                            while sources.len() != 0 {
                                let Some(document) = ctx.next_charged(&mut sources, OPERATION)?
                                else {
                                    break;
                                };
                                let mut states = document.states.iter();
                                while states.len() != 0 {
                                    let Some(state) = ctx.next_charged(&mut states, OPERATION)?
                                    else {
                                        break;
                                    };
                                    add_owner(state.id.as_str())?;
                                }
                            }
                            let mut sources = records.shape_payloads.iter();
                            while sources.len() != 0 {
                                let Some(record) = ctx.next_charged(&mut sources, OPERATION)?
                                else {
                                    break;
                                };
                                add_owner(record.id.as_str())?;
                            }
                            let mut sources = records.element_maps.iter();
                            while sources.len() != 0 {
                                let Some(record) = ctx.next_charged(&mut sources, OPERATION)?
                                else {
                                    break;
                                };
                                add_owner(record.id.as_str())?;
                            }
                            let mut sources = entries.iter();
                            while sources.len() != 0 {
                                let Some(entry) = ctx.next_charged(&mut sources, OPERATION)? else {
                                    break;
                                };
                                add_owner(entry.id())?;
                            }
                            owner_ids.insert(ids)
                        }
                    };
                    if ctx.contains_hash_set(known, owner.as_str(), OPERATION)? {
                        true
                    } else {
                        let known = match string_table_ids {
                            Some(ref ids) => ids,
                            None => {
                                let mut ids = HashSet::new();
                                let mut sources = records.string_tables.iter();
                                while sources.len() != 0 {
                                    let Some(table) = ctx.next_charged(&mut sources, OPERATION)?
                                    else {
                                        break;
                                    };
                                    owner_storage.with_storage(|| {
                                        let id = table.id_with_admission(ctx)?;
                                        ctx.insert_hash_set(&mut ids, id, OPERATION)
                                    })?;
                                }
                                string_table_ids.insert(ids)
                            }
                        };
                        ctx.contains_hash_set(known, owner.as_str(), OPERATION)?
                    }
                }
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
    drop(string_table_ids);
    drop(owner_ids);
    drop(owner_storage);
    let mut entry_sources = entries.iter();
    while entry_sources.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entry_sources, OPERATION)? else {
            break;
        };
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
    let mut name_sources = by_entry.into_iter();
    while name_sources.len() != 0 {
        let Some((name, group)) = ctx.next_charged(&mut name_sources, OPERATION)? else {
            break;
        };
        let ScopedData {
            _storage: storage,
            data: mut spans,
        } = group;
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
        drop(spans);
        drop(storage);
    }
    drop(name_sources);
    drop(group_index_storage);
    Ok(())
}

enum SliceDifference {
    Pair(usize),
    Length,
}

/// The first differing pair or different lengths after an equal prefix;
/// equal slices have no difference. Each pair compared is charged.
fn first_difference<T: PartialEq + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    stored: &[T],
    derived: &[T],
    operation: &'static str,
) -> Result<Option<SliceDifference>, CodecError> {
    let mut pairs = stored.iter().zip(derived).enumerate();
    while pairs.len() != 0 {
        let Some((index, (stored, derived))) = ctx.next_charged(&mut pairs, operation)? else {
            break;
        };
        if !ctx.equal(stored, derived, operation)? {
            return Ok(Some(SliceDifference::Pair(index)));
        }
    }
    Ok((stored.len() != derived.len()).then_some(SliceDifference::Length))
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
        if !prefix.starts_with(b"PK\x03\x04") {
            return Ok(Confidence::No);
        }
        if container::has_document_markers(ctx, prefix)? {
            Ok(Confidence::High)
        } else if ctx
            .position_by(
                prefix.windows(b"Document.xml".len()),
                |window| Ok(window == b"Document.xml"),
                "detect FreeCAD document marker",
            )?
            .is_some()
        {
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
        let mut scan_storage = ctx.reserve_scoped(0, "FCStd decode scan")?;
        let mut scan = scan_storage.with_storage(|| container::scan(ctx, root))?;
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
        let mut cycle_affected_design_storage =
            ctx.reserve_scoped(0, "FCStd design cycle object storage")?;
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
            let mut graph_storage = ctx.reserve_scoped(0, "FCStd persistence graph")?;
            let graph = graph_storage.with_storage(|| {
                persistence::parse_document(
                    document_xml.text,
                    document_xml.xml.document(),
                    dialect::FcstdDialect::from_schema_version(&scan.schema_version),
                    ctx,
                )
            })?;
            let mut property_sources = graph.properties.iter();
            while property_sources.len() != 0 {
                let Some(property) =
                    ctx.next_charged(&mut property_sources, "FCStd side entry check")?
                else {
                    break;
                };
                let mut side_entry_sources = property.side_entries().iter();
                while side_entry_sources.len() != 0 {
                    let Some(side_entry) =
                        ctx.next_charged(&mut side_entry_sources, "FCStd side entry check")?
                    else {
                        break;
                    };
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
            let mut entry_storage = ctx.reserve_scoped(0, "FCStd decode entry records")?;
            let mut entry_records = entry_storage
                .with_storage(|| container::entry_records(ctx, &scan, &graph.properties))?;
            let mut element_storage = ctx.reserve_scoped(0, "FCStd decode element records")?;
            let (string_tables, mut element_maps) = element_storage.with_storage(|| {
                element_map::parse(
                    ctx,
                    document_xml.xml.document(),
                    scan.document.file_version.value(),
                    &graph.properties,
                    &entry_records,
                )
            })?;
            drop(document_xml);
            let mut shape_storage = ctx.reserve_scoped(0, "FCStd decode shape payloads")?;
            let shape_payloads = shape_storage
                .with_storage(|| brep::parse_payloads(ctx, &graph.properties, &entry_records))?;
            namespace.set_arena(ctx, "objects", &graph.objects)?;
            namespace.set_arena(ctx, "extensions", &graph.extensions)?;
            namespace.set_arena(ctx, "properties", &graph.properties)?;
            namespace.set_arena(ctx, "shape_payloads", &shape_payloads)?;
            {
                let census = ctx.with_scoped_storage("FCStd decode carrier census", || {
                    brep::carrier_census(ctx, &shape_payloads)
                })?;
                let _census_storage = census.1;
                let census = census.0;
                namespace.set_arena(ctx, "carrier_census", &census)?;
            }
            namespace.set_arena(ctx, "string_tables", string_tables.as_slice())?;
            let mut product_storage = ctx.reserve_scoped(0, "FCStd decode product nodes")?;
            let product_nodes = product_storage.with_storage(|| {
                product::transfer(ctx, &graph.objects, &graph.properties, &scan.data)
            })?;
            namespace.set_arena(ctx, "product_nodes", &product_nodes)?;
            let mut joint_storage = ctx.reserve_scoped(0, "FCStd decode joints")?;
            let joint_records = joint_storage
                .with_storage(|| joint::transfer(ctx, &graph.objects, &graph.properties))?;
            namespace.set_arena(ctx, "joints", &joint_records)?;
            let mut drawing_storage = ctx.reserve_scoped(0, "FCStd decode drawings")?;
            let drawings = drawing_storage
                .with_storage(|| drawing::transfer(ctx, &graph.objects, &graph.properties))?;
            drawing::transfer_neutral(ctx, &mut ir.model, &drawings, &graph.properties)?;
            namespace.set_arena(ctx, "drawings", &drawings)?;
            let mut annotation_storage = ctx.reserve_scoped(0, "FCStd decode annotations")?;
            let annotations = annotation_storage
                .with_storage(|| annotation::transfer(ctx, &graph.objects, &graph.properties))?;
            annotation::transfer_neutral(
                ctx,
                &mut ir.model,
                &annotations,
                &graph.properties,
                &drawings,
            )?;
            namespace.set_arena(ctx, "annotations", &annotations)?;
            drop((annotations, annotation_storage, drawings, drawing_storage));
            application::install(
                ctx,
                namespace,
                &graph.objects,
                &graph.properties,
                &entry_records,
            )?;
            {
                let attachments = ctx.with_scoped_storage("FCStd decode attachments", || {
                    attachment::transfer(ctx, &graph.objects, &graph.properties)
                })?;
                let _attachment_storage = attachments.1;
                let attachments = attachments.0;
                namespace.set_arena(ctx, "attachments", &attachments)?;
            }
            let (curve_transfer, surface_transfer) =
                brep::transfer_text_geometry(ctx, &shape_payloads, &graph.properties)?;
            geometry_transferred =
                !curve_transfer.curves.is_empty() || !surface_transfer.surfaces.is_empty();
            ir.model.curves = curve_transfer.curves;
            let mut owner_sources = curve_transfer.procedural.into_iter();
            while owner_sources.len() != 0 {
                let Some((owner, procedural)) =
                    ctx.next_charged(&mut owner_sources, "FCStd procedural curves")?
                else {
                    break;
                };
                ir.model
                    .add_procedural_curve(ctx, &owner, procedural)?
                    .map_err(|error| {
                        resource::malformed_charged(
                            ctx,
                            format_args!("{error}"),
                            "FCStd procedural curve diagnostic",
                        )
                    })?;
            }
            drop(owner_sources);
            ir.model.surfaces = surface_transfer.surfaces;
            let mut owner_sources = surface_transfer.procedural.into_iter();
            while owner_sources.len() != 0 {
                let Some((owner, procedural)) =
                    ctx.next_charged(&mut owner_sources, "FCStd procedural surfaces")?
                else {
                    break;
                };
                ir.model
                    .add_procedural_surface(ctx, &owner, procedural)?
                    .map_err(|error| {
                        resource::malformed_charged(
                            ctx,
                            format_args!("{error}"),
                            "FCStd procedural surface diagnostic",
                        )
                    })?;
            }
            drop(owner_sources);
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
                element_map::has_topology_consumers(ctx, &element_maps)?,
            )?;
            (cycle_affected_design_objects, cycle_affected_design_storage) = design::transfer(
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
            drop((joint_records, joint_storage, product_nodes, product_storage));
            ctx.admit_entities(
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            {
                let census = ctx.with_scoped_storage("FCStd decode design census", || {
                    design::census(ctx, &graph.objects, &ir.model.features)
                })?;
                let _census_storage = census.1;
                let census = census.0;
                ir.native
                    .namespace_mut("fcstd")
                    .set_arena(ctx, "design_census", &census)?;
            }
            element_storage.with_storage(|| {
                element_map::bind_topology(ctx, &mut element_maps, &topology_occurrences.records)
            })?;
            drop(topology_occurrences);
            let mut gui_graph = if let Some(gui_view) =
                ctx.get_btree_map(&scan.data, "GuiDocument.xml", "FCStd GUI entry lookup")?
            {
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
            entry_storage
                .with_storage(|| bind_gui_entry_references(ctx, &mut entry_records, &gui_graph))?;
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
            let mut logical_storage = ctx.reserve_scoped(0, "FCStd decode logical ledger")?;
            let logical_ledger = logical_storage.with_storage(|| {
                container::logical_ledger(
                    ctx,
                    &entry_records,
                    &graph.properties,
                    &gui_graph,
                    &shape_payloads,
                    string_tables.as_slice(),
                    &element_maps,
                )
            })?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "logical_ledger", &logical_ledger)?;
            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.span.end());
            let coverage = ctx.with_scoped_storage("FCStd decode byte coverage", || {
                container::byte_coverage(
                    ctx,
                    &scan.ledger,
                    &entry_records,
                    &logical_ledger,
                    physical_byte_len,
                )
            })?;
            let _coverage_storage = coverage.1;
            let coverage = coverage.0;
            drop((logical_ledger, logical_storage));
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
            let coverage = ctx.with_scoped_storage("FCStd decode byte coverage", || {
                container::byte_coverage(ctx, &scan.ledger, &[], &[], physical_byte_len)
            })?;
            let _coverage_storage = coverage.1;
            let coverage = coverage.0;
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
        drop(cycle_affected_design_objects);
        drop(cycle_affected_design_storage);
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
        drop((scan, scan_storage));
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
    if entry_records.is_empty() {
        return Ok(());
    }
    let mut entry_index_storage = None;
    let mut entry_index: Option<HashMap<&str, Option<usize>>> = None;
    let mut bound_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut addition_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut bound = HashSet::new();
    let mut additions = Vec::new();
    let mut bind = |names: &'g [String], owner: &'g str| -> Result<(), CodecError> {
        let mut name_sources = names.iter();
        while name_sources.len() != 0 {
            let Some(name) = ctx.next_charged(&mut name_sources, OPERATION)? else {
                break;
            };
            let entry_index = match &mut entry_index {
                Some(index) => index,
                slot @ None => {
                    let (index, storage) = ctx.unique_index(
                        entry_records
                            .iter()
                            .enumerate()
                            .map(|(index, entry)| (entry.name(), index)),
                        OPERATION,
                    )?;
                    entry_index_storage = Some(storage);
                    slot.insert(index)
                }
            };
            // Archive entry names are unique, so every named record is indexed.
            let Some(&Some(index)) = ctx.get_hash_map(entry_index, name.as_str(), OPERATION)?
            else {
                continue;
            };
            if bound_storage
                .with_storage(|| ctx.insert_hash_set(&mut bound, (index, owner), OPERATION))?
            {
                ctx.push_scoped_vec(
                    &mut addition_storage,
                    &mut additions,
                    (index, owner),
                    OPERATION,
                )?;
            }
        }
        Ok(())
    };
    let mut property_sources = gui_graph.properties.iter();
    while property_sources.len() != 0 {
        let Some(property) = ctx.next_charged(&mut property_sources, OPERATION)? else {
            break;
        };
        bind(&property.side_entries, &property.id)?;
    }
    let mut document_sources = gui_graph.documents.iter();
    while document_sources.len() != 0 {
        let Some(document) = ctx.next_charged(&mut document_sources, OPERATION)? else {
            break;
        };
        let mut state_sources = document.states.iter();
        while state_sources.len() != 0 {
            let Some(state) = ctx.next_charged(&mut state_sources, OPERATION)? else {
                break;
            };
            bind(&state.side_entries, &state.id)?;
        }
    }
    drop(entry_index);
    drop(entry_index_storage);
    drop(bound);
    drop(bound_storage);
    let mut index_sources = additions.into_iter();
    while index_sources.len() != 0 {
        let Some((index, owner)) = ctx.next_charged(&mut index_sources, OPERATION)? else {
            break;
        };
        entry_records[index].push_reference(ctx, owner)?;
    }
    drop(index_sources);
    drop(addition_storage);
    Ok(())
}

fn semantic_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    cycle_affected_design_objects: &BTreeSet<String>,
    gui_losses: Vec<LossNote>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = gui_losses;
    let mut feature_sources = ir.model.features.iter();
    while feature_sources.len() != 0 {
        let Some(feature) =
            ctx.next_charged(&mut feature_sources, "FCStd feature semantic loss")?
        else {
            break;
        };
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
    let mut entity_sources = ir.model.sketch_entities.iter();
    while entity_sources.len() != 0 {
        let Some(entity) =
            ctx.next_charged(&mut entity_sources, "FCStd sketch geometry semantic loss")?
        else {
            break;
        };
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
    let mut constraint_sources = ir.model.sketch_constraints.iter();
    while constraint_sources.len() != 0 {
        let Some(constraint) = ctx.next_charged(
            &mut constraint_sources,
            "FCStd sketch constraint semantic loss",
        )?
        else {
            break;
        };
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
