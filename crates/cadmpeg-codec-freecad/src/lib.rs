// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write ZIP-packaged `FreeCAD` `.FCStd` documents.
//!
//! [`FcstdCodec`] implements [`cadmpeg_ir::codec::Codec`] and
//! [`cadmpeg_ir::codec::write::Encoder`]. Retained writes preserve
//! unedited persistence records and named side entries when their current typed
//! projection agrees with CADIR. Synthesis writes neutral content and native-only
//! typed records; unsupported content is omitted with an export loss.
//!
//! <!-- generated: capability fcstd -->
//! Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#freecad-fcstd)).
//! <!-- /generated: capability fcstd -->

mod annotation;
mod application;
mod application_geometry;
mod archive_recovery;
mod attachment;
mod attributes;
mod brep;
mod builder;
mod container;
mod control_links;
mod design;
mod dialect;
mod drawing;
mod element_map;
mod entry_sources;
mod fidelity;
mod gui;
mod joint;
/// Byte-offset constants generated from `docs/layouts/freecad.toml`.
mod layout;
mod loss;
mod mutation;
mod native;
mod parameter_names;
mod persistence;
mod placement;
mod product;
mod property_policy;
mod property_reader;
mod source_archive;
mod topology_transfer;
mod writer;
mod xml_framing;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
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
    ///
    /// This edits native content only. A neutral counterpart remains the
    /// authority during export; callers edit that counterpart to change it.
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

fn sponsor_unformed(
    ctx: &DecodeContext<'_>,
    failures: &mut cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry,
    omissions: Vec<design::UnformedProjection>,
) -> Result<(), CodecError> {
    for omission in omissions {
        if omission.ids.is_empty() {
            continue;
        }
        let home = omission.homes.first().ok_or_else(|| {
            CodecError::malformed("unformed dependent record has no actual source cause")
        })?;
        failures.sponsor_unformed(ctx, home, omission.ids)?;
    }
    Ok(())
}

fn validate_native(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(Vec::new());
    };
    macro_rules! arena {
        ($read:expr) => {
            match $read {
                Ok(records) => records,
                Err(cadmpeg_ir::native::NativeConvertError::Resource(error))
                    if matches!(error, CodecError::ResourceLimit(_)) =>
                {
                    return Err(error)
                }
                Err(error) => {
                    return Ok(vec![finding(Check::NativeLinks, error.to_string(), None)]);
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
    let mut string_table_records =
        arena!(namespace.arena_as_for_decode::<native::StringTableRecord>(ctx, "string_tables"));
    string_table_records.sort_by_key(|left| left.index);
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
    let shape_payloads = arena!(brep::records::payloads(ctx, namespace));
    let carrier_census =
        arena!(namespace.arena_as_for_decode::<native::CarrierCensusRecord>(ctx, "carrier_census"));
    let mut design_census =
        arena!(namespace.arena_as_for_decode::<native::DesignCensusRecord>(ctx, "design_census"));
    design_census.sort_by(|left, right| {
        left.object
            .cmp(&right.object)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut findings = Vec::new();
    for (id, detail) in product::semantic_issues(&product_nodes, &properties)? {
        findings.push(finding(Check::PayloadIntegrity, detail, Some(id)));
    }
    for (id, detail) in arena!(brep::records::validate(ctx, namespace, &shape_payloads)) {
        findings.push(finding(Check::PayloadIntegrity, detail, Some(id)));
    }
    for (id, detail) in application_geometry::parts::validate(&entries) {
        findings.push(finding(Check::PayloadIntegrity, detail, Some(id)));
    }
    for arena in namespace.arenas().keys() {
        if arena != "unknowns" && !native::ARENAS.contains(&arena.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                format!("unregistered FCStd native arena {arena}"),
                None,
            ));
        }
    }
    if carrier_census != brep::carrier_census(ctx, &shape_payloads)? {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd carrier census does not match parsed shape payloads",
            None,
        ));
    }
    for record in &design_census {
        if !objects.iter().any(|object| {
            object.id().as_str() == record.object && object.type_name == record.type_name
        }) {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has an absent or incompatible typed design object",
                    record.id
                ),
                Some(record.id.clone()),
            ));
        }
    }
    let derived_census = ir.scope.design.then(|| {
        design::census(&objects, &ir.model).map(|mut expected| {
            expected.sort_by(|left, right| left.object.cmp(&right.object));
            if expected.len() == design_census.len() {
                for (derived, stored) in expected.iter_mut().zip(&design_census) {
                    if derived.object == stored.object {
                        derived.id.clone_from(&stored.id);
                    }
                }
            }
            expected
        })
    });
    match derived_census {
        None => {}
        Some(Ok(expected)) if design_census == expected => {}
        Some(Ok(expected)) => {
            let detail = design_census
                .iter()
                .zip(&expected)
                .find(|(stored, derived)| stored != derived)
                .map_or_else(
                    || {
                        format!(
                            "stored {} records and derived {} records",
                            design_census.len(),
                            expected.len()
                        )
                    },
                    |(stored, derived)| format!("stored {stored:?} but derived {derived:?}"),
                );
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("FCStd design census does not match projected feature semantics: {detail}"),
                None,
            ));
        }
        Some(Err(error)) => findings.push(finding(
            Check::ReferentialIntegrity,
            error.to_string(),
            None,
        )),
    }
    let object_ids = objects
        .iter()
        .map(|record| record.id().as_str())
        .collect::<HashSet<_>>();
    let entry_names = entries
        .iter()
        .map(crate::native::EntryRecord::name)
        .collect::<HashSet<_>>();
    let property_ids = properties
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    let extension_ids = extensions
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    if object_ids.len() != objects.len()
        || property_ids.len() != properties.len()
        || extension_ids.len() != extensions.len()
    {
        findings.push(finding(
            Check::Identity,
            "duplicate FCStd native identity",
            None,
        ));
    }
    for object in &objects {
        for dependency in &object.dependencies {
            if !object_ids.contains(dependency.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing dependency {dependency}", object.id()),
                    Some(object.id().clone()),
                ));
            }
        }
    }
    let object_by_id = objects
        .iter()
        .map(|object| (object.id().as_str(), object))
        .collect::<HashMap<_, _>>();
    let applications_match =
        match application::matches_native(ctx, namespace, &objects, &properties, &entries) {
            Ok(matches) => matches,
            Err(cadmpeg_ir::native::NativeConvertError::Resource(CodecError::ResourceLimit(
                limit,
            ))) => {
                return Err(CodecError::ResourceLimit(limit));
            }
            Err(error) => return Ok(vec![finding(Check::NativeLinks, error.to_string(), None)]),
        };
    if !applications_match {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd application records do not match their typed objects and evidence",
            None,
        ));
    }
    for attachment in &attachments {
        let missing_support = attachment
            .supports
            .as_deref()
            .unwrap_or_default()
            .iter()
            .flatten()
            .any(|support| {
                support.document().is_none()
                    && support
                        .object()
                        .is_some_and(|object| !object_ids.contains(object))
            });
        if !object_ids.contains(attachment.object.as_str()) || missing_support {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has an invalid attachment target or frame",
                    attachment.id
                ),
                Some(attachment.id.clone()),
            ));
        }
    }
    match attachment::transfer(&objects, &properties) {
        Ok(expected)
            if attachments.len() != expected.len()
                || attachments.iter().any(|actual| {
                    expected
                        .iter()
                        .find(|expected| expected.id == actual.id)
                        .is_none_or(|expected| !actual.agrees_with(expected))
                }) =>
        {
            findings.push(finding(
                Check::NativeLinks,
                "FCStd attachment graph does not match the application property graph",
                None,
            ))
        }
        Err(error) => findings.push(finding(
            Check::NativeLinks,
            format!("FCStd attachment properties are malformed: {error}"),
            None,
        )),
        _ => {}
    }
    let gui_provider_ids = gui_providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect::<HashSet<_>>();
    let has_gui_entry = entry_names.contains("GuiDocument.xml");
    if gui_documents.len() > usize::from(has_gui_entry) {
        findings.push(finding(
            Check::Counts,
            "FCStd GUI document record does not match GuiDocument.xml presence",
            None,
        ));
    }
    for document in &gui_documents {
        if document
            .states
            .iter()
            .any(|state| state.values.iter().any(|value| value.item_index.is_some()))
        {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has an enumeration item ordinal outside an enumeration property",
                    document.id
                ),
                Some(document.id.clone()),
            ));
        }
        if document.states.iter().any(|state| {
            state
                .side_entries
                .iter()
                .any(|entry| !entry_names.contains(entry.as_str()))
        }) {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} has a missing GUI state asset", document.id),
                Some(document.id.clone()),
            ));
        }
    }
    for provider in &gui_providers {
        if provider
            .object
            .as_ref()
            .is_some_and(|object| !object_ids.contains(object.as_str()))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} references a missing application object", provider.id),
                Some(provider.id.clone()),
            ));
        }
    }
    for property in &gui_properties {
        if let Err(error) = gui::validate_property_links(property) {
            findings.push(finding(
                Check::PayloadIntegrity,
                error.to_string(),
                Some(property.id.clone()),
            ));
        }
        if !(gui_provider_ids.contains(property.owner.as_str())
            || extensions.iter().any(|extension| {
                extension.id == property.owner
                    && gui_provider_ids.contains(extension.owner.as_str())
            }))
            || property
                .side_entries
                .iter()
                .any(|entry| !entry_names.contains(entry.as_str()))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a missing GUI owner or side entry", property.id),
                Some(property.id.clone()),
            ));
        }
    }
    let mut product_by_object = HashMap::new();
    for node in &product_nodes {
        product_by_object.insert(node.object.as_str(), node);
    }
    let cyclic_products = product::product_cycle_nodes(&product_by_object)?;
    for node in &product_nodes {
        if !object_ids.contains(node.object.as_str())
            || node
                .members()
                .iter()
                .any(|member| !object_ids.contains(member.as_str()))
            || node.prototype().is_some_and(|prototype| {
                !object_ids.contains(prototype) && node.external_document().is_none()
            })
            || node
                .placement_property()
                .is_some_and(|property| !property_ids.contains(property))
            || [node.copy_on_change_source(), node.copy_on_change_group()]
                .into_iter()
                .flatten()
                .filter(|target| target.document().is_none())
                .filter_map(|target| target.object())
                .chain(node.element_objects().iter().map(String::as_str))
                .any(|object| !object_ids.contains(object))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a missing product-structure link", node.id),
                Some(node.id.clone()),
            ));
        }
        if cyclic_products.contains(node.object.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} participates in a product-structure cycle", node.id),
                Some(node.id.clone()),
            ));
        }
    }
    for joint in &joints {
        let missing_link = !object_ids.contains(joint.object())
            || joint.references().any(|reference| {
                reference.document().is_none()
                    && reference
                        .object()
                        .is_some_and(|object| !object_ids.contains(object))
            });
        if missing_link {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has missing operands or invalid connector frames",
                    joint.id()
                ),
                Some(joint.id().to_owned()),
            ));
        }
    }
    for drawing in &drawings {
        let missing_object = !object_ids.contains(drawing.object.as_str())
            || drawing.sources.iter().flatten().any(|source| {
                source.document().is_none()
                    && source
                        .object()
                        .is_some_and(|object| !object_ids.contains(object))
            });
        let missing_entry = drawing
            .side_entries
            .iter()
            .any(|entry| !entry_names.contains(entry.as_str()));
        let missing_relationship = drawing
            .relationships
            .values()
            .flatten()
            .flatten()
            .any(|link| {
                link.document().is_none()
                    && link
                        .object()
                        .is_some_and(|object| !object_ids.contains(object))
            });
        if missing_object || missing_entry || missing_relationship {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} has a missing drawing object or side entry", drawing.id),
                Some(drawing.id.clone()),
            ));
        }
    }
    for annotation in &annotations {
        let object = object_by_id.get(annotation.object.as_str());
        let missing_reference =
            annotation
                .references
                .values()
                .flatten()
                .flatten()
                .any(|reference| {
                    reference.document().is_none()
                        && reference
                            .object()
                            .is_some_and(|object| !object_ids.contains(object))
                });
        let missing_entry = annotation
            .side_entries
            .iter()
            .any(|entry| !entry_names.contains(entry.as_str()));
        if object.is_none_or(|object| object.type_name != annotation.kind.as_str())
            || missing_reference
            || missing_entry
        {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has a missing annotation object, target, or asset",
                    annotation.id
                ),
                Some(annotation.id.clone()),
            ));
        }
    }
    let expected_annotation_objects = objects
        .iter()
        .filter(|object| annotation::is_annotation_type(&object.type_name))
        .map(|object| object.id().as_str())
        .collect::<HashSet<_>>();
    let annotation_objects = annotations
        .iter()
        .map(|annotation| annotation.object.as_str())
        .collect::<HashSet<_>>();
    if annotation_objects.len() != annotations.len()
        || annotation_objects != expected_annotation_objects
    {
        findings.push(finding(
            Check::Identity,
            "FCStd semantic annotation graph does not cover every annotation object exactly once",
            None,
        ));
    }
    let mut extension_names = HashSet::new();
    let mut extension_types = HashSet::new();
    for extension in &extensions {
        if !object_ids.contains(extension.owner.as_str())
            && !gui_provider_ids.contains(extension.owner.as_str())
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has missing owner {}", extension.id, extension.owner),
                Some(extension.id.clone()),
            ));
        }
        if !extension_names.insert((extension.owner.as_str(), extension.name.as_str())) {
            findings.push(finding(
                Check::Identity,
                format!(
                    "{} duplicates extension name {}",
                    extension.id, extension.name
                ),
                Some(extension.id.clone()),
            ));
        }
        if !extension_types.insert((extension.owner.as_str(), extension.type_name.as_str())) {
            findings.push(finding(
                Check::Identity,
                format!(
                    "{} duplicates extension type {}",
                    extension.id, extension.type_name
                ),
                Some(extension.id.clone()),
            ));
        }
    }
    for property in &properties {
        if property.owner != native::native_id("document", "0")
            && !object_ids.contains(property.owner.as_str())
            && !extension_ids.contains(property.owner.as_str())
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has missing owner {}", property.id, property.owner),
                Some(property.id.clone()),
            ));
        }
        for target in property
            .links()
            .iter()
            .flatten()
            .filter(|target| target.document().is_none())
            .filter_map(crate::native::LinkTarget::object)
        {
            if !object_ids.contains(target) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing link target {target}", property.id),
                    Some(property.id.clone()),
                ));
            }
        }
        for entry in property.side_entries() {
            if !entry_names.contains(entry.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing side entry {entry}", property.id),
                    Some(property.id.clone()),
                ));
            }
        }
    }
    for table in string_tables {
        if table
            .owner_property
            .as_ref()
            .is_some_and(|owner| !property_ids.contains(owner.as_str()))
        {
            let table_id = table.id();
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{table_id} has a missing property link"),
                Some(table_id),
            ));
        }
    }
    for map in &element_maps {
        if !property_ids.contains(map.property.as_str())
            || map
                .string_table
                .as_ref()
                .is_some_and(|id| !string_tables.iter().any(|table| table.id() == *id))
            || map.hasher_index.is_some() != map.string_table.is_some()
            || map.string_table.as_ref().is_some_and(|id| {
                string_tables
                    .iter()
                    .find(|table| table.id() == *id)
                    .is_some_and(|table| Some(table.index) != map.hasher_index)
            })
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a missing property or string table", map.id),
                Some(map.id.clone()),
            ));
        }
        let known_string_ids = map
            .string_table
            .as_ref()
            .and_then(|id| string_tables.iter().find(|table| table.id() == *id))
            .and_then(native::StringTableRecord::entries)
            .map(|entries| {
                entries
                    .iter()
                    .flatten()
                    .map(|entry| entry.string_id)
                    .collect::<HashSet<_>>()
            });
        for name in map
            .maps
            .root()
            .groups
            .iter()
            .flatten()
            .flat_map(|group| &group.names)
            .flatten()
            .flatten()
        {
            if let Some(known_ids) = &known_string_ids {
                if name.string_ids.iter().any(|id| !known_ids.contains(id)) {
                    findings.push(finding(
                        Check::ReferentialIntegrity,
                        format!("{} references a missing persistent string id", map.id),
                        Some(map.id.clone()),
                    ));
                }
            }
        }
    }
    let asset_owner_ids = property_ids
        .iter()
        .copied()
        .chain(gui_properties.iter().map(|property| property.id.as_str()))
        .chain(
            gui_documents
                .iter()
                .flat_map(|document| document.states.iter().map(|state| state.id.as_str())),
        )
        .collect::<HashSet<_>>();
    let mut expected_references = HashMap::<String, Vec<String>>::new();
    for property in &properties {
        for entry_name in property.side_entries() {
            let owners = expected_references.entry(entry_name.clone()).or_default();
            if !owners.contains(&property.id) {
                owners.push(property.id.clone());
            }
        }
    }
    for property in &gui_properties {
        for entry_name in &property.side_entries {
            let owners = expected_references.entry(entry_name.clone()).or_default();
            if !owners.contains(&property.id) {
                owners.push(property.id.clone());
            }
        }
    }
    for document in &gui_documents {
        for state in &document.states {
            for entry_name in &state.side_entries {
                let owners = expected_references.entry(entry_name.clone()).or_default();
                if !owners.contains(&state.id) {
                    owners.push(state.id.clone());
                }
            }
        }
    }
    for entry in &entries {
        for owner in entry.referenced_by() {
            if !asset_owner_ids.contains(owner.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing referencing record {owner}", entry.id()),
                    Some(entry.id().to_owned()),
                ));
            }
        }
        let expected = expected_references
            .get(entry.name())
            .map_or(&[][..], Vec::as_slice);
        if !entry
            .referenced_by()
            .iter()
            .map(String::as_str)
            .eq(expected.iter().map(String::as_str))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a stale side-entry reference relation", entry.id()),
                Some(entry.id().to_owned()),
            ));
        }
    }
    Ok(findings)
}

fn finding(check: Check, message: impl Into<String>, entity: Option<String>) -> Finding {
    Finding {
        related: Vec::new(),
        check,
        severity: FindingSeverity::Error,
        message: message.into(),
        entity,
    }
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
            if cadmpeg_container::ArchiveSnapshot::has_footer(prefix) {
                match cadmpeg_container::ArchiveSnapshot::contains_matching_name(
                    ctx,
                    cadmpeg_core::decode::View::over_retained(prefix),
                    |name| name == "Document.xml",
                ) {
                    Ok(true) => return Ok(Confidence::Medium),
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Ok(false) | Err(_) => {}
                }
            }
            return Ok(Confidence::No);
        }
        if container::has_document_markers(ctx, prefix)? {
            Ok(Confidence::High)
        } else if contains(prefix, b"Document.xml") {
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
        let (decoded, mut summary) = Self::decode_source(ctx, root)?;
        summary.losses = decoded.body.losses;
        summary.notes = decoded.body.notes;
        Ok(summary)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        Self::decode_source(ctx, root).map(|(decoded, _)| decoded)
    }
}

impl FcstdCodec {
    fn decode_source(
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<(Decoded, ContainerSummary), CodecError> {
        let (mut scan, mut parts) = match container::attempt(ctx, root)? {
            container::ScanOutcome::Readable { scan, parts } => (scan, parts),
            container::ScanOutcome::GivenUp {
                summary,
                parts,
                pointers,
                source,
            } => {
                let inspected = summary.clone();
                let decoded =
                    archive_recovery::decoded(ctx, root, summary, parts, pointers, source)?;
                return Ok((decoded, inspected));
            }
        };
        let summary = container::summarize(&scan);
        let gui_source = scan
            .data
            .get("GuiDocument.xml")
            .map(|source| gui::source::parse(ctx, *source))
            .transpose()?;
        let mut admitted_entities = 0_u64;
        let mut attributes = container::source_attributes(&scan)?;
        let summary_notes = container::summary_notes(&scan);
        let thumbnail = scan
            .data
            .get("thumbnails/Thumbnail.png")
            .map(|view| ("thumbnails/Thumbnail.png", *view))
            .or_else(|| {
                scan.data
                    .get("Thumbnail.png")
                    .map(|view| ("Thumbnail.png", *view))
            });
        if let Some((_, thumbnail)) = thumbnail {
            attributes.insert(
                cadmpeg_core::nonblank_literal!("thumbnail_bytes"),
                thumbnail.window().len().to_string(),
            );
        }
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
        let mut layout_payloads = BTreeMap::new();
        let mut layout_pointers = BTreeMap::new();
        for key in [
            "object_count",
            "archive_entry_count",
            "physical_ledger_spans",
            "physical_archive_bytes",
            "thumbnail_bytes",
        ] {
            if let Some(value) = attributes.remove(key) {
                source_fidelity
                    .metadata
                    .insert(format!("fcstd.{key}"), value);
            }
        }
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(root.window().len()),
            "retain FCStd source image",
        )?;
        let archive_capture = ctx.capture_part_extent(root)?;
        fidelity::retain(
            &mut source_fidelity,
            fidelity::SOURCE_IMAGE,
            archive_capture.stream(),
            archive_capture.offset(),
            root.window(),
        )?;

        let entry_sources = entry_sources::EntrySources::new(ctx, &scan.data)?
            .with_parents(&scan.entry_parents)
            .with_owners(&scan.entry_owners);
        let mut geometry_transferred = false;
        let mut cycle_affected_design_objects = BTreeSet::new();
        let gui_losses;
        let mut transfer_losses = Vec::new();
        let mut model_dependency_closure_done = false;
        // One `classify` call feeds the report identity, loss, and notes.
        let primary = dialect::FcstdDialect::classify(&scan.document, &scan.schema_version);
        let dialects = cadmpeg_core::dialect::DialectLayers::of(primary);
        let mut ir = CadIr::decoded(SourceMeta::classified(dialects.clone(), attributes));
        ir.set_scope(container::located_scope(ctx.depth(), parts.fidelity()))
            .map_err(CodecError::malformed)?;
        if let Some((name, source)) = thumbnail {
            let capture = ctx.capture_part_extent(source)?;
            let bytes = source.window();
            let id = UnknownId::compose(
                &cadmpeg_ir::identity_namespace!("fcstd", "native", "thumbnail"),
                cadmpeg_ir::ids::IdentityKey::encode_segment(name),
            );
            let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::resume(
                std::mem::take(&mut source_fidelity.annotations),
            );
            annotations.annotate(
                &id,
                capture.stream(),
                capture.offset(),
                "thumbnail",
                cadmpeg_ir::Exactness::ByteExact,
            )?;
            source_fidelity.annotations = annotations.build();
            let unknown = ctx.record(id.as_str(), source, || {
                Ok(UnknownRecord::unavailable(
                    id.clone(),
                    capture.offset(),
                    cadmpeg_core::decode::u64_from_index(bytes.len()),
                    cadmpeg_ir::hash::sha256_hex(bytes),
                    vec![native::native_id("document", "0")],
                ))
            })?;
            source_fidelity.attach_native_unknown_records(
                &mut ir,
                "fcstd",
                [unknown].into(),
                ctx,
            )?;
        }
        let mut part_losses = Vec::new();
        let mut source_failures;
        let capture_pruning;
        {
            let mut entry_updates = std::mem::take(
                &mut scan
                    .object_declarations
                    .as_mut()
                    .ok_or_else(|| {
                        CodecError::malformed("decoded object part schedule is unavailable")
                    })?
                    .entry_contents,
            );
            let mut entry_interpretations = std::mem::take(
                &mut scan
                    .object_declarations
                    .as_mut()
                    .ok_or_else(|| CodecError::malformed("entry source schedule is unavailable"))?
                    .entry_interpretations,
            );
            let mut graph = persistence::graph(
                scan.document_properties.as_ref(),
                scan.object_declarations.as_ref().ok_or_else(|| {
                    CodecError::malformed("decoded document has no object part schedule")
                })?,
            );
            let mut expression_bindings = design::ExpressionBindings::default();
            let declared_object_ids = &scan
                .object_declarations
                .as_ref()
                .ok_or_else(|| CodecError::malformed("decoded declaration census is unavailable"))?
                .declared_objects;
            let admitted_object_ids = &scan
                .object_declarations
                .as_ref()
                .ok_or_else(|| {
                    CodecError::malformed("decoded admitted object census is unavailable")
                })?
                .admitted_objects;
            let object_sources = &scan
                .object_declarations
                .as_ref()
                .ok_or_else(|| {
                    CodecError::malformed("decoded object source authorities are unavailable")
                })?
                .sources;
            scan.losses.append(&mut graph.losses);
            let property_xml = scan
                .object_declarations
                .as_ref()
                .and_then(|declarations| declarations.xml_source.as_deref())
                .ok_or_else(|| CodecError::malformed("original property scanner is unavailable"))?;
            let property_reader = property_reader::PropertyReader::typed(ctx)
                .with_xml_source(property_xml)
                .with_object_sources(object_sources)
                .with_property_sources(&graph.property_sources);
            for (id, range) in &graph.object_sources {
                fidelity::captured_object_layout_payloads(
                    ctx,
                    *scan
                        .data
                        .get("Document.xml")
                        .ok_or_else(|| CodecError::malformed("object data has no source entry"))?,
                    id,
                    range.clone(),
                    &mut layout_payloads,
                    &mut layout_pointers,
                )?;
            }
            for property in &graph.properties {
                let source = graph.property_sources.get(&property.id).ok_or_else(|| {
                    CodecError::malformed("property has no original source range")
                })?;
                fidelity::captured_property_layout_payloads(
                    ctx,
                    *scan
                        .data
                        .get("Document.xml")
                        .ok_or_else(|| CodecError::malformed("property has no source entry"))?,
                    source,
                    property,
                    &mut layout_payloads,
                    &mut layout_pointers,
                )?;
            }
            for (id, range) in &graph.extension_sources {
                fidelity::captured_xml_layout_payload(
                    ctx,
                    *scan
                        .data
                        .get("Document.xml")
                        .ok_or_else(|| CodecError::malformed("extension has no source entry"))?,
                    id,
                    range.clone(),
                    &mut layout_payloads,
                )?;
            }
            let mut entry_records = std::mem::take(&mut scan.entry_records);
            for entry in &entry_records {
                let Some(source) = scan.data.get(entry.name()) else {
                    continue;
                };
                if layout_pointers
                    .insert(
                        entry.id().to_owned(),
                        source_archive::entry_pointers(
                            scan.archive.entry(entry.name()).ok_or_else(|| {
                                CodecError::malformed("entry has no original ZIP declaration")
                            })?,
                            cadmpeg_core::decode::u64_from_index(source.window().len()),
                        )?,
                    )
                    .is_some()
                {
                    return Err(CodecError::malformed(
                        "duplicate archive entry layout pointers",
                    ));
                }
            }
            let mut brep_projection_sources =
                brep::source::projection::ProjectionSources::default();
            let shape_payloads = brep::source::decode_payloads(
                &property_reader,
                &graph.properties,
                &entry_records,
                &entry_sources,
                &mut parts,
                &mut brep_projection_sources,
            )?;
            entry_updates.extend(application_geometry::parts::decode(
                &property_reader,
                &graph.properties,
                &entry_records,
                &entry_sources,
                &mut entry_interpretations,
                &mut parts,
            )?);
            let mut element_maps = parts
                .native()
                .namespace("fcstd")
                .map(|namespace| {
                    namespace.arena_as_for_decode::<native::element_map::ElementMapRecord>(
                        ctx,
                        "element_maps",
                    )
                })
                .transpose()?
                .unwrap_or_default();
            let declared_entries = container::parts::declared_entries(&entry_records);
            let (mut gui_graph, gui_whitespace, gui_entry_interpretations) =
                if let Some(gui_source) = gui_source {
                    gui_source.decode_with_whitespace(
                        ctx,
                        &gui::GuiSources {
                            entries: &scan.data,
                            declared_entries: Some(&declared_entries),
                            declared_objects: Some(declared_object_ids),
                            admitted_objects: Some(admitted_object_ids),
                            object_sources: Some(object_sources),
                            entry_parents: Some(&scan.entry_parents),
                            entry_owners: Some(&scan.entry_owners),
                            entry_interpretations: Some(&entry_interpretations),
                            gui_entry_interpretations: None,
                            objects: &graph.objects,
                            properties: &graph.properties,
                            payloads: &shape_payloads,
                            element_maps: &element_maps,
                            topology_occurrences: &[],
                            requires_alpha_conversion: gui::requires_alpha_conversion(
                                scan.document.program_version.as_deref(),
                            ),
                        },
                        &mut parts,
                    )?
                } else {
                    (gui::Graph::default(), Vec::new(), Default::default())
                };
            crate::xml_framing::append_whitespace(
                ctx,
                &mut scan.source_whitespace,
                gui_whitespace,
            )?;
            brep::source::decode_gui_payloads(
                &property_reader,
                &gui_graph.properties,
                &entry_records,
                &entry_sources,
                &mut parts,
                &mut brep_projection_sources,
            )?;
            entry_updates.append(&mut gui_graph.entry_contents);
            container::parts::merge_entry_contents(ctx, &mut entry_records, entry_updates)?;
            container::parts::opaque_side_entries(ctx, &scan, &mut parts)?;
            container::parts::stage_entries(
                ctx,
                entry_records,
                &graph.properties,
                &gui_graph,
                &mut parts,
            )?;
            let (dropped, failures, pruning) = container::parts::append(
                ctx,
                parts,
                &mut ir,
                &mut source_fidelity,
                &mut part_losses,
            )?;
            source_failures = failures;
            capture_pruning = pruning;
            for id in dropped {
                layout_payloads.remove(&id);
                layout_pointers.remove(&id);
                gui_graph.provider_sources.retain(|(owner, _)| owner != &id);
                gui_graph.value_sources.retain(|source| source.id() != id);
                gui_graph.state_sources.remove(&id);
                gui_graph.property_sources.remove(&id);
            }
            let mut failure_homes =
                property_reader::failure_homes(ctx, &ir, &source_fidelity, &source_failures)?;
            scan.object_declarations
                .as_ref()
                .ok_or_else(|| {
                    CodecError::malformed("decoded object source authorities are unavailable")
                })?
                .bind_failure_homes(ctx, &source_failures, &mut failure_homes)?;
            let namespace = ir.native.namespace_mut("fcstd");
            graph.objects = namespace.arena_as_for_decode(ctx, "objects")?;
            graph.extensions = namespace.arena_as_for_decode(ctx, "extensions")?;
            graph.properties = namespace.arena_as_for_decode(ctx, "properties")?;
            let shape_payloads = brep::records::payloads(ctx, namespace)?;
            let string_tables =
                namespace.arena_as_for_decode::<native::StringTableRecord>(ctx, "string_tables")?;
            element_maps = namespace.arena_as_for_decode(ctx, "element_maps")?;
            entry_records = namespace.arena_as_for_decode(ctx, "entries")?;
            gui_graph.documents = namespace.arena_as_for_decode(ctx, "gui_documents")?;
            gui_graph.providers = namespace.arena_as_for_decode(ctx, "gui_view_providers")?;
            gui_graph.properties = namespace.arena_as_for_decode(ctx, "gui_properties")?;
            let payload_sources = &scan
                .object_declarations
                .as_ref()
                .ok_or_else(|| CodecError::malformed("payload source suppliers are unavailable"))?
                .payload_sources;
            let product_nodes = product::transfer_source(
                ctx,
                &graph.objects,
                &graph.properties,
                &entry_records,
                object_sources,
                payload_sources,
            )?;
            let property_reader = property_reader
                .with_object_sources(object_sources)
                .with_property_sources(&graph.property_sources)
                .with_payload_sources(payload_sources)
                .with_entries(entry_records.as_slice())
                .with_failures(&failure_homes);
            namespace.set_arena(ctx, "product_nodes", &product_nodes)?;
            let (product_nodes, unavailable_products) =
                product::for_projection(product_nodes, &graph.properties)?;
            let (joint_records, joint_unformed) = joint::transfer_source(
                &property_reader,
                &graph.objects,
                &graph.properties,
                object_sources,
            )?;
            let mut unavailable_joints = BTreeSet::new();
            for omission in &joint_unformed {
                for id in &omission.ids {
                    unavailable_joints.insert(id.clone());
                }
            }
            sponsor_unformed(ctx, &mut source_failures, joint_unformed)?;
            namespace.set_arena(ctx, "joints", &joint_records)?;
            let drawings = drawing::transfer_source(
                &property_reader,
                &graph.objects,
                &graph.properties,
                object_sources,
            )?;
            if !ctx.container_only() {
                drawing::transfer_neutral(
                    &property_reader,
                    &mut ir.model,
                    &drawings,
                    &graph.properties,
                )?;
            }
            namespace.set_arena(ctx, "drawings", &drawings)?;
            let (annotations, annotation_unformed) = annotation::transfer_source(
                &property_reader,
                &graph.objects,
                &graph.properties,
                object_sources,
            )?;
            sponsor_unformed(ctx, &mut source_failures, annotation_unformed)?;
            if !ctx.container_only() {
                annotation::transfer_neutral(
                    &property_reader,
                    &mut ir.model,
                    &annotations,
                    &graph.properties,
                    &drawings,
                )?;
            }
            namespace.set_arena(ctx, "annotations", &annotations)?;
            application::install_source(
                ctx,
                namespace,
                &graph.objects,
                &graph.properties,
                &entry_records,
                object_sources,
                &scan.entry_owners,
            )?;
            let (attachments, attachment_unformed) = attachment::transfer_source(
                &property_reader,
                &graph.objects,
                &graph.properties,
                object_sources,
            )?;
            sponsor_unformed(ctx, &mut source_failures, attachment_unformed)?;
            namespace.set_arena(ctx, "attachments", &attachments)?;
            let gui_shape_owners = brep::owners::gui(ctx, ir.native.namespace("fcstd"))?;
            let topology_occurrences = if ctx.container_only() {
                Vec::new()
            } else {
                let mut curve_transfer = brep::transfer_text_curves(
                    ctx,
                    &shape_payloads,
                    brep::owners::Properties::new(&graph.properties, &gui_shape_owners),
                    Some(&brep_projection_sources),
                )?;
                let surface_transfer = brep::transfer_text_surfaces(
                    ctx,
                    &shape_payloads,
                    brep::owners::Properties::new(&graph.properties, &gui_shape_owners),
                    &mut curve_transfer,
                    Some(&brep_projection_sources),
                )?;
                geometry_transferred =
                    !curve_transfer.curves.is_empty() || !surface_transfer.surfaces.is_empty();
                ir.model.curves = curve_transfer.curves;
                ir.model
                    .add_procedural_curves(curve_transfer.procedural)?
                    .map_err(CodecError::malformed)?;
                ir.model.surfaces = surface_transfer.surfaces;
                ir.model
                    .add_procedural_surfaces(surface_transfer.procedural)?
                    .map_err(CodecError::malformed)?;
                geometry_transferred |= application_geometry::parts::transfer(
                    &property_reader,
                    &mut ir,
                    &graph.properties,
                    &mut admitted_entities,
                    Some(&entry_sources),
                    scan.data.get("GuiDocument.xml").copied(),
                    &gui_graph.property_sources,
                )?;
                let topology_occurrences = topology_transfer::transfer_with_sources(
                    ctx,
                    &mut ir,
                    &shape_payloads,
                    brep::owners::Properties::new(&graph.properties, &gui_shape_owners),
                    &mut transfer_losses,
                    Some(&brep_projection_sources),
                )?;
                topology_transfer::apply_body_names(&mut ir, &shape_payloads, &graph.properties);
                if ctx.depth() == DecodeDepth::Full {
                    let (cycle_affected, design_losses, unformed, bindings) = design::transfer(
                        &property_reader,
                        &mut ir,
                        &graph.objects,
                        &graph.properties,
                        &shape_payloads,
                        scan.document.program_version.as_deref(),
                        Some(&property_reader::EngineFailureSources::new(
                            ctx,
                            &source_fidelity,
                            &source_failures,
                        )?),
                    )?;
                    sponsor_unformed(ctx, &mut source_failures, unformed)?;
                    cycle_affected_design_objects = cycle_affected;
                    transfer_losses.extend(design_losses);
                    expression_bindings = bindings;
                }
                ctx.admit_entities(
                    cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                    &mut admitted_entities,
                    "admit FCStd entities",
                )?;
                let link_occurrences = product::link_occurrence_count(&product_nodes)?;
                ctx.admit_entities(
                    cadmpeg_core::decode::u64_from_index(ir.model.entity_count())
                        .checked_add(link_occurrences)
                        .ok_or_else(|| CodecError::malformed("FCStd entity count overflows u64"))?,
                    &mut admitted_entities,
                    "admit FCStd link occurrences",
                )?;
                let (product_definitions, occurrences) = product::transfer_neutral(
                    &property_reader,
                    &product_nodes,
                    &joint_records,
                    &graph.objects,
                    &graph.properties,
                    &shape_payloads,
                    &ir.model.bodies,
                    &unavailable_products,
                )?;
                ir.model.product_definitions = product_definitions;
                ir.model.occurrences = occurrences;
                ir.model.assembly_joints = joint::transfer_neutral(
                    &joint_records,
                    &ir.model.occurrences,
                    &graph.properties,
                    &unavailable_joints,
                )?;
                for feature in &mut ir.model.features {
                    if feature.native_ref.as_deref().is_some_and(|object| {
                        joint_records.iter().any(|record| {
                            record.object() == object
                                && ir
                                    .model
                                    .assembly_joints
                                    .iter()
                                    .any(|joint| joint.native_ref.as_deref() == Some(record.id()))
                        })
                    }) {
                        feature.suppressed = None;
                    }
                }
                writer::ordinals::apply(&graph.properties, &mut ir)?;
                ctx.admit_entities(
                    cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                    &mut admitted_entities,
                    "admit FCStd entities",
                )?;
                topology_occurrences
            };
            if !ctx.container_only() {
                gui::project_records(
                    ctx,
                    &mut ir,
                    &mut gui_graph,
                    &gui::GuiSources {
                        entries: &scan.data,
                        declared_entries: Some(&declared_entries),
                        declared_objects: Some(declared_object_ids),
                        admitted_objects: Some(admitted_object_ids),
                        object_sources: Some(object_sources),
                        entry_parents: Some(&scan.entry_parents),
                        entry_owners: Some(&scan.entry_owners),
                        entry_interpretations: Some(&entry_interpretations),
                        gui_entry_interpretations: Some(&gui_entry_interpretations),
                        objects: &graph.objects,
                        properties: &graph.properties,
                        payloads: &shape_payloads,
                        element_maps: &element_maps,
                        topology_occurrences: &topology_occurrences,
                        requires_alpha_conversion: gui::requires_alpha_conversion(
                            scan.document.program_version.as_deref(),
                        ),
                    },
                )?;
            }
            fidelity::gui_layout_payloads(
                ctx,
                &scan.data,
                &gui_graph,
                &mut layout_payloads,
                &mut layout_pointers,
            )?;
            if !ctx.container_only() {
                writer::body_styles::apply(&mut ir, &shape_payloads, &graph.properties)?;
                let property_reader = property_reader.with_part_failures(&source_failures);
                let (attribute_losses, attribute_unformed) = attributes::transfer_source(
                    ctx,
                    &mut ir,
                    &graph.objects,
                    &graph.properties,
                    *scan.data.get("Document.xml").ok_or_else(|| {
                        CodecError::malformed("attribute projection has no source document")
                    })?,
                    &graph.property_sources,
                    &property_reader,
                    &expression_bindings,
                )?;
                sponsor_unformed(ctx, &mut source_failures, attribute_unformed)?;
                transfer_losses.extend(attribute_losses);
            }
            source_failures.prune_late_model(ctx, &mut ir.model)?;
            model_dependency_closure_done = true;
            if ctx.depth() == DecodeDepth::Full {
                let (design_census, census_unformed) = design::census_source(
                    &property_reader,
                    &graph.objects,
                    &graph.properties,
                    &ir.model,
                    scan.document.program_version.as_deref(),
                    &expression_bindings,
                    &source_failures,
                    Some(&property_reader::EngineFailureSources::new(
                        ctx,
                        &source_fidelity,
                        &source_failures,
                    )?),
                )?;
                sponsor_unformed(ctx, &mut source_failures, census_unformed)?;
                ir.native
                    .namespace_mut("fcstd")
                    .set_arena(ctx, "design_census", &design_census)?;
            }
            gui_losses = std::mem::take(&mut gui_graph.losses);
            ctx.admit_entities(
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            let logical_ledger = container::logical_ledger(
                &entry_records,
                &graph.properties,
                &graph.property_sources,
                &entry_sources,
                &gui_graph,
                &shape_payloads,
                string_tables.as_slice(),
                &element_maps,
            )?;

            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.span.end());
            let coverage = container::byte_coverage(
                &scan.ledger,
                &entry_records,
                &entry_sources,
                &logical_ledger,
                physical_byte_len,
            )?;
            source_fidelity.insert_freecad_ledgers(
                archive_capture.stream().to_owned(),
                cadmpeg_ir::source_fidelity::freecad::FreecadSourceLedgers {
                    physical_ledger: std::mem::take(&mut scan.ledger),
                    logical_ledger,
                    byte_coverage: coverage,
                },
            )?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena(ctx, "element_maps", &element_maps)?;
        }
        if !model_dependency_closure_done {
            source_failures.prune_late_model(ctx, &mut ir.model)?;
        }
        source_failures.apply(ctx, &source_fidelity, &mut part_losses)?;
        let mut losses = if ctx.container_only() {
            gui_losses
        } else {
            semantic_losses(&ir, &cycle_affected_design_objects, gui_losses)
        };
        // Charged on both decode branches: a schema outside the declared rows
        // is read with the schema-4 strategy on either path, so the charge is
        // not conditioned on the branch.
        losses.extend(transfer_losses);
        losses.append(&mut scan.losses);
        let dialect_losses = dialect::FcstdDialect::dialect_loss(dialects.primary());
        losses.extend(dialect_losses);
        losses.extend(part_losses);
        let document_digest = writer::target::document_local_sha256(ctx, &ir)?;
        source_fidelity.metadata.insert(
            cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY.into(),
            document_digest,
        );
        ctx.admit_entities(
            cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
            &mut admitted_entities,
            "admit FCStd entities",
        )?;
        let inventory = fidelity::b1::prepare(ctx, root, &scan)?;
        fidelity::install_layouts(
            ctx,
            &mut source_fidelity,
            layout_payloads,
            layout_pointers,
            Some(capture_pruning),
        )?;
        inventory.finalize(ctx, &ir, &mut source_fidelity, &mut losses)?;
        let scope = ir.scope;
        Ok((
            Decoded {
                ir,
                body: DecodeBody {
                    transfer: cadmpeg_ir::report::decode::DecodeTransfer {
                        scope,
                        geometry_transferred,
                    },
                    coverage: cadmpeg_ir::report::decode::Coverage::default(),
                    losses,
                    notes: summary_notes,
                    transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
                },
                source_fidelity,
            },
            summary,
        ))
    }
}

impl EncoderBackend for FcstdCodec {
    const REUSES_SOURCE: bool = true;
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Request both neutral classes for a complete native design export.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const WRITES_RECOMPUTE: bool = true;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, Some(0));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::target::plan(input, &target)
    }
}

fn semantic_losses(
    ir: &CadIr,
    cycle_affected_design_objects: &BTreeSet<String>,
    gui_losses: Vec<LossNote>,
) -> Vec<LossNote> {
    let mut losses = gui_losses;
    for feature in &ir.model.features {
        let cycle_affected = feature
            .native_ref
            .as_ref()
            .is_some_and(|id| cycle_affected_design_objects.contains(id));
        if cycle_affected {
            push_semantic_loss(
                &mut losses,
                FreecadLossCode::FeatureCyclicHistory,
                &[
                    "FCStd design dependencies contain a cycle; the current dependency graph is retained",
                ],
                feature.native_ref.as_deref(),
            );
        }
        let definition = match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. }
            | cadmpeg_ir::features::FeatureDefinition::Operation(operation) => operation,
        };
        let cadmpeg_ir::features::FeatureOperation::Native { kind, .. } = definition else {
            continue;
        };
        if design::is_registered_design_kind(kind.as_str()) {
            continue;
        }
        let code = FreecadLossCode::FeatureNativeKindRetained;
        let suffix = " is retained natively but has no neutral semantics";
        push_semantic_loss(
            &mut losses,
            code,
            &["FCStd design operation ", kind.as_str(), suffix],
            feature.native_ref.as_deref(),
        );
    }
    for entity in &ir.model.sketch_entities {
        let cadmpeg_ir::sketches::SketchGeometryDefinition::Native { native_kind } =
            entity.geometry.definition()
        else {
            continue;
        };
        push_semantic_loss(
            &mut losses,
            FreecadLossCode::SketchNativeGeometry,
            &[
                "FCStd sketch geometry ",
                native_kind.as_str(),
                " is retained natively but is not neutralized",
            ],
            entity.native_ref.as_deref(),
        );
    }
    for constraint in &ir.model.sketch_constraints {
        let cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { native_kind, .. } =
            constraint.definition.kind()
        else {
            continue;
        };
        push_semantic_loss(
            &mut losses,
            FreecadLossCode::SketchNativeConstraint,
            &[
                "FCStd sketch constraint ",
                native_kind.as_str(),
                " is retained natively but is not neutralized",
            ],
            constraint.native_ref.as_deref(),
        );
    }
    losses
}

fn push_semantic_loss(
    losses: &mut Vec<LossNote>,
    code: FreecadLossCode,
    message_parts: &[&str],
    tag: Option<&str>,
) {
    let message = message_parts.join("");
    let tag = tag.map(ToOwned::to_owned);
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
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
