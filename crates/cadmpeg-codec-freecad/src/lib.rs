// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write ZIP-packaged `FreeCAD` `.FCStd` documents.
//!
//! [`FcstdCodec`] implements [`Codec`] and [`Encoder`]. Retained writes preserve
//! unedited persistence records and named side entries, while checked mutation
//! methods update typed values. [`FcstdDocumentBuilder`] creates source-less
//! schema-4/file-1 application graphs. Other target bands and edits without a
//! lossless serializer are rejected explicitly.
//!
//! Support level: [L5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.

mod annotation;
mod application;
mod application_geometry;
mod attachment;
mod brep;
mod builder;
mod container;
mod design;
mod drawing;
mod element_map;
mod gui;
mod joint;
/// Byte-offset constants generated from `docs/layouts/freecad.toml`.
pub(crate) mod layout;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod mutation;
mod native;
mod persistence;
mod product;
mod topology_transfer;
mod writer;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, Encoder, ExportPlan,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::report::{DecodeReport, LossNote};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::FidelityResolution;
use cadmpeg_ir::{Check, Finding, Severity as FindingSeverity};

use crate::loss::FreecadLossCode;

/// `FCStd` document codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct FcstdCodec;

pub use builder::{FcstdDocumentBuilder, FcstdPropertyValue};
pub use mutation::FcstdPropertyOwner;

/// Selects the persistence band emitted by [`FcstdCodec::encode_with_options`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcstdWriteOptions {
    /// `Document.xml` schema version.
    pub schema_version: u32,
    /// `Document.xml` file version.
    pub file_version: u32,
}

impl Default for FcstdWriteOptions {
    fn default() -> Self {
        Self {
            schema_version: 4,
            file_version: 1,
        }
    }
}

impl FcstdCodec {
    /// Write a document for an explicitly selected persistence band.
    pub fn encode_with_options(
        &self,
        ir: &CadIr,
        writer: &mut dyn std::io::Write,
        options: FcstdWriteOptions,
    ) -> Result<ExportReport, CodecError> {
        writer::write(ir, writer, options)
    }

    /// Change one attribute on an ordered native property value.
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

    /// Change the text content of one ordered native property value.
    pub fn set_property_value_text(
        &self,
        ir: &mut CadIr,
        owner: FcstdPropertyOwner<'_>,
        property: &str,
        value_order: usize,
        text: Option<String>,
    ) -> Result<(), CodecError> {
        mutation::set_value_text(ir, owner, property, value_order, text)
    }

    /// Replace one named side-entry payload while retaining its graph identity.
    pub fn replace_side_entry(
        &self,
        ir: &mut CadIr,
        entry: &str,
        bytes: Vec<u8>,
    ) -> Result<(), CodecError> {
        mutation::replace_entry(ir, entry, bytes)
    }
}

/// Validate FCStd-native identities, graph links, payloads, and byte ledgers.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Vec::new();
    };
    if namespace.version != native::VERSION {
        return vec![finding(
            Check::Version,
            format!(
                "unsupported FCStd native namespace version {}",
                namespace.version
            ),
            None,
        )];
    }
    let objects = match namespace.arena_as::<native::ObjectRecord>("objects") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let properties = match namespace.arena_as::<native::PropertyRecord>("properties") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let extensions = match namespace.arena_as::<native::ExtensionRecord>("extensions") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let entries = match namespace.arena_as::<native::EntryRecord>("entries") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let physical = match namespace.arena_as::<native::ArchiveSpan>("physical_ledger") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let logical = match namespace.arena_as::<native::LogicalSpan>("logical_ledger") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let coverage_records = match namespace.arena_as::<native::ByteCoverageRecord>("byte_coverage") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let string_tables = match namespace.arena_as::<native::StringTableRecord>("string_tables") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let element_maps = match namespace.arena_as::<native::ElementMapRecord>("element_maps") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let gui_providers =
        match namespace.arena_as::<native::GuiViewProviderRecord>("gui_view_providers") {
            Ok(records) => records,
            Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
        };
    let gui_documents = match namespace.arena_as::<native::GuiDocumentRecord>("gui_documents") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let gui_properties = match namespace.arena_as::<native::GuiPropertyRecord>("gui_properties") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let product_nodes = match namespace.arena_as::<native::ProductNodeRecord>("product_nodes") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let joints = match namespace.arena_as::<native::JointRecord>("joints") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let drawings = match namespace.arena_as::<native::DrawingRecord>("drawings") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let annotations = match namespace.arena_as::<native::SemanticAnnotationRecord>("annotations") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let applications = match namespace.arena_as::<native::ApplicationRecord>("applications") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let attachments = match namespace.arena_as::<native::AttachmentRecord>("attachments") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let shape_payloads = match namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let carrier_census = match namespace.arena_as::<native::CarrierCensusRecord>("carrier_census") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let design_census = match namespace.arena_as::<native::DesignCensusRecord>("design_census") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };

    let mut findings = Vec::new();
    if carrier_census != brep::carrier_census(&shape_payloads) {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd carrier census does not match parsed shape payloads",
            None,
        ));
    }
    match design::census(&objects, &ir.model.features) {
        Ok(expected) if design_census == expected => {}
        Ok(expected) => {
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
        Err(error) => findings.push(finding(
            Check::ReferentialIntegrity,
            error.to_string(),
            None,
        )),
    }
    let object_ids = objects
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    let entry_names = entries
        .iter()
        .map(|entry| entry.name.as_str())
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
        let valid_object_bytes = match (&object.raw_xml, object.byte_start, object.byte_end) {
            (Some(raw), Some(start), Some(end)) => start < end && end - start == raw.len() as u64,
            _ => false,
        };
        if !valid_object_bytes {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("{} has inconsistent retained object bytes", object.id),
                Some(object.id.clone()),
            ));
        }
        if object
            .dependency_allow_partial
            .is_some_and(|value| value <= 0)
        {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} has invalid partial-load capability", object.id),
                Some(object.id.clone()),
            ));
        }
        for dependency in &object.dependencies {
            if !object_ids.contains(dependency.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing dependency {dependency}", object.id),
                    Some(object.id.clone()),
                ));
            }
        }
    }
    let object_by_id = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<HashMap<_, _>>();
    let property_by_id = properties
        .iter()
        .map(|property| (property.id.as_str(), property))
        .collect::<HashMap<_, _>>();
    let application_objects = applications
        .iter()
        .map(|record| record.object.as_str())
        .collect::<HashSet<_>>();
    if applications != application::transfer(&objects, &properties, &entries) {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd application preservation records do not match authoritative bytes",
            None,
        ));
    }
    if application_objects.len() != applications.len()
        || application_objects.len() != objects.len()
        || application_objects != object_ids
    {
        findings.push(finding(
            Check::Identity,
            "FCStd application census does not cover every object exactly once",
            None,
        ));
    }
    for record in &applications {
        let Some(object) = object_by_id.get(record.object.as_str()) else {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} references a missing application object", record.id),
                Some(record.id.clone()),
            ));
            continue;
        };
        let expected_domain = object
            .type_name
            .split_once("::")
            .map_or("Unqualified", |(domain, _)| domain);
        let mut owned = properties
            .iter()
            .filter(|property| property.owner == object.id)
            .collect::<Vec<_>>();
        owned.sort_by_key(|property| (property.byte_start, property.byte_end));
        let expected_properties = owned
            .iter()
            .map(|property| property.id.as_str())
            .collect::<Vec<_>>();
        let expected_side_entries = owned
            .iter()
            .flat_map(|property| property.side_entries.iter().map(String::as_str))
            .collect::<Vec<_>>();
        let expected_inert_payload = owned.iter().any(|property| {
            property.family == native::PropertyFamily::PythonObject
                || property.type_name.contains("PropertyPythonObject")
        });
        let invalid_properties = record.properties.iter().any(|property| {
            property_by_id
                .get(property.as_str())
                .is_none_or(|property| property.owner != object.id)
        });
        let mut mismatches = Vec::new();
        if record.type_name != object.type_name {
            mismatches.push("type");
        }
        if record.domain != expected_domain {
            mismatches.push("domain");
        }
        if record.dependencies != object.dependencies {
            mismatches.push("dependencies");
        }
        if record
            .properties
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != expected_properties
        {
            mismatches.push("properties");
        }
        if record
            .side_entries
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != expected_side_entries
        {
            mismatches.push("side entries");
        }
        if record.inert_payload != expected_inert_payload {
            mismatches.push("inert payload classification");
        }
        if invalid_properties {
            mismatches.push("property ownership");
        }
        if record
            .side_entries
            .iter()
            .any(|entry| !entry_names.contains(entry.as_str()))
        {
            mismatches.push("side-entry references");
        }
        if !mismatches.is_empty() {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} does not match its application object graph: {}",
                    record.id,
                    mismatches.join(", ")
                ),
                Some(record.id.clone()),
            ));
        }
    }
    for attachment in &attachments {
        let missing_support = attachment.supports.iter().any(|support| {
            support.document.is_none()
                && support.object.as_ref().is_some_and(|object| {
                    !object.is_empty() && !object_ids.contains(object.as_str())
                })
        });
        let non_finite = attachment
            .placement
            .iter()
            .chain(attachment.offset.iter())
            .chain(std::iter::once(&attachment.effective_frame))
            .flat_map(|matrix| matrix.iter().flatten())
            .any(|value| !value.is_finite());
        let effective_mismatch =
            crate::attachment::effective_frame(attachment.placement, attachment.offset)
                != attachment.effective_frame;
        if !object_ids.contains(attachment.object.as_str())
            || missing_support
            || non_finite
            || effective_mismatch
        {
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
        Ok(expected) if attachments != expected => findings.push(finding(
            Check::NativeLinks,
            "FCStd attachment graph does not match the application property graph",
            None,
        )),
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
    if gui_documents.len() != usize::from(has_gui_entry) {
        findings.push(finding(
            Check::Counts,
            "FCStd GUI document record does not match GuiDocument.xml presence",
            None,
        ));
    }
    for document in &gui_documents {
        if document.states.iter().enumerate().any(|(order, state)| {
            state.order != order
                || state.byte_start >= state.byte_end
                || state
                    .side_entries
                    .iter()
                    .any(|entry| !entry_names.contains(entry.as_str()))
        }) {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has invalid GUI state order, span, or asset",
                    document.id
                ),
                Some(document.id.clone()),
            ));
        }
    }
    for provider in &gui_providers {
        if provider
            .object
            .as_ref()
            .is_some_and(|object| !object.is_empty() && !object_ids.contains(object.as_str()))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} references a missing application object", provider.id),
                Some(provider.id.clone()),
            ));
        }
    }
    for property in &gui_properties {
        if !gui_provider_ids.contains(property.owner.as_str())
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
    let product_by_object = product_nodes
        .iter()
        .map(|node| (node.object.as_str(), node))
        .collect::<HashMap<_, _>>();
    let cyclic_products = product::product_cycle_nodes(&product_by_object);
    for node in &product_nodes {
        if !object_ids.contains(node.object.as_str())
            || node
                .members
                .iter()
                .any(|member| !object_ids.contains(member.as_str()))
            || node.prototype.as_ref().is_some_and(|prototype| {
                !object_ids.contains(prototype.as_str()) && node.external_document.is_none()
            })
            || node
                .placement_property
                .as_ref()
                .is_some_and(|property| !property_ids.contains(property.as_str()))
            || [
                node.copy_on_change_source.as_ref(),
                node.copy_on_change_group.as_ref(),
            ]
            .into_iter()
            .flatten()
            .chain(node.element_objects.iter())
            .any(|object| !object_ids.contains(object.as_str()))
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
        let invalid_array_count = node.element_count.is_some_and(|count| {
            count < 0
                || [
                    node.element_transforms.len(),
                    node.element_scales.len(),
                    node.element_visibility.len(),
                    node.element_objects.len(),
                ]
                .into_iter()
                .any(|length| length != 0 && i64::try_from(length).ok() != Some(count))
        });
        let non_finite_array = node
            .element_transforms
            .iter()
            .flatten()
            .flatten()
            .chain(node.element_scales.iter().flatten())
            .any(|value| !value.is_finite());
        if invalid_array_count || non_finite_array {
            findings.push(finding(
                Check::Counts,
                format!("{} has invalid link-array count or values", node.id),
                Some(node.id.clone()),
            ));
        }
    }
    for joint in &joints {
        let missing_link = !object_ids.contains(joint.object.as_str())
            || joint.references.iter().any(|reference| {
                reference.document.is_none()
                    && reference.object.as_ref().is_some_and(|object| {
                        !object.is_empty() && !object_ids.contains(object.as_str())
                    })
            });
        let expected_placements = if joint.kind == "grounded" { 1 } else { 2 };
        let invalid_frames = joint.placements.len() != expected_placements
            || (!joint.offsets.is_empty() && joint.offsets.len() != expected_placements)
            || joint
                .placements
                .iter()
                .flatten()
                .flatten()
                .chain(joint.offsets.iter().flatten().flatten())
                .any(|value| !value.is_finite());
        if missing_link || invalid_frames {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "{} has missing operands or invalid connector frames",
                    joint.id
                ),
                Some(joint.id.clone()),
            ));
        }
    }
    for drawing in &drawings {
        let missing_object = !object_ids.contains(drawing.object.as_str())
            || drawing.sources.iter().any(|source| {
                source.document.is_none()
                    && source.object.as_ref().is_some_and(|object| {
                        !object.is_empty() && !object_ids.contains(object.as_str())
                    })
            });
        let missing_entry = drawing
            .side_entries
            .iter()
            .any(|entry| !entry_names.contains(entry.as_str()));
        let missing_relationship = drawing.relationships.values().flatten().any(|link| {
            link.document.is_none()
                && link.object.as_ref().is_some_and(|object| {
                    !object.is_empty() && !object_ids.contains(object.as_str())
                })
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
        let missing_reference = annotation.references.values().flatten().any(|reference| {
            reference.document.is_none()
                && reference.object.as_ref().is_some_and(|object| {
                    !object.is_empty() && !object_ids.contains(object.as_str())
                })
        });
        let missing_entry = annotation
            .side_entries
            .iter()
            .any(|entry| !entry_names.contains(entry.as_str()));
        if object.is_none_or(|object| object.type_name != annotation.kind)
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
        .map(|object| object.id.as_str())
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
        if !object_ids.contains(extension.owner.as_str()) {
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
            .links
            .iter()
            .filter_map(|link| link.object.as_deref())
        {
            if target.starts_with("fcstd:native:object#") && !object_ids.contains(target) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing link target {target}", property.id),
                    Some(property.id.clone()),
                ));
            }
        }
    }
    for (expected_table_index, table) in string_tables.iter().enumerate() {
        if table.index != expected_table_index || table.declared_count != table.entries.len() {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} has invalid index or entry count", table.id),
                Some(table.id.clone()),
            ));
        }
        if table
            .owner_property
            .as_ref()
            .is_some_and(|owner| !property_ids.contains(owner.as_str()))
            || table
                .source_entry
                .as_ref()
                .is_some_and(|entry| !entry_names.contains(entry.as_str()))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a missing property or side-entry link", table.id),
                Some(table.id.clone()),
            ));
        }
        let mut known_string_ids = HashSet::new();
        for entry in &table.entries {
            if !known_string_ids.insert(entry.string_id)
                || entry
                    .components
                    .iter()
                    .any(|id| !known_string_ids.contains(id))
            {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has duplicate or forward string-id references", table.id),
                    Some(table.id.clone()),
                ));
            }
        }
    }
    let topology_ids = ir
        .model
        .vertices
        .iter()
        .map(|entity| entity.id.0.as_str())
        .chain(ir.model.edges.iter().map(|entity| entity.id.0.as_str()))
        .chain(ir.model.loops.iter().map(|entity| entity.id.0.as_str()))
        .chain(ir.model.faces.iter().map(|entity| entity.id.0.as_str()))
        .chain(ir.model.shells.iter().map(|entity| entity.id.0.as_str()))
        .chain(ir.model.bodies.iter().map(|entity| entity.id.0.as_str()))
        .collect::<HashSet<_>>();
    for map in &element_maps {
        if !property_ids.contains(map.property.as_str())
            || map
                .hasher_index
                .is_some_and(|index| index >= string_tables.len())
            || map
                .source_entry
                .as_ref()
                .is_some_and(|entry| !entry_names.contains(entry.as_str()))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!(
                    "{} has a missing property, string table, or side entry",
                    map.id
                ),
                Some(map.id.clone()),
            ));
        }
        for name in map
            .maps
            .last()
            .into_iter()
            .flat_map(|node| &node.groups)
            .flat_map(|group| &group.names)
            .flatten()
        {
            if let Some(table) = map.hasher_index.and_then(|index| string_tables.get(index)) {
                let known_ids = table
                    .entries
                    .iter()
                    .map(|entry| entry.string_id)
                    .collect::<HashSet<_>>();
                if name.string_ids.iter().any(|id| !known_ids.contains(id)) {
                    findings.push(finding(
                        Check::ReferentialIntegrity,
                        format!("{} references a missing persistent string id", map.id),
                        Some(map.id.clone()),
                    ));
                }
            }
            if name
                .topology_ids
                .iter()
                .any(|id| !topology_ids.contains(id.as_str()))
            {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} references missing neutral topology", map.id),
                    Some(map.id.clone()),
                ));
            }
        }
    }
    let mut entry_lengths = HashMap::new();
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
        for entry_name in &property.side_entries {
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
        entry_lengths.insert(entry.name.as_str(), entry.byte_len);
        if entry.byte_len != entry.data.len() as u64 || entry.sha256 != sha256_hex(&entry.data) {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("{} failed length or digest validation", entry.id),
                Some(entry.id.clone()),
            ));
        }
        for owner in &entry.referenced_by {
            if !asset_owner_ids.contains(owner.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing referencing record {owner}", entry.id),
                    Some(entry.id.clone()),
                ));
            }
        }
        let expected = expected_references
            .get(entry.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        if !entry
            .referenced_by
            .iter()
            .map(String::as_str)
            .eq(expected.iter().map(String::as_str))
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a stale side-entry reference relation", entry.id),
                Some(entry.id.clone()),
            ));
        }
    }
    let physical_end = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("physical_archive_bytes"))
        .and_then(|value| value.parse().ok());
    validate_span_chain("physical archive", &physical, physical_end, &mut findings);
    let logical_owner_ids = property_ids
        .iter()
        .copied()
        .chain(gui_properties.iter().map(|record| record.id.as_str()))
        .chain(
            gui_documents
                .iter()
                .flat_map(|document| document.states.iter().map(|record| record.id.as_str())),
        )
        .chain(shape_payloads.iter().map(|record| record.id.as_str()))
        .chain(string_tables.iter().map(|record| record.id.as_str()))
        .chain(element_maps.iter().map(|record| record.id.as_str()))
        .chain(entries.iter().map(|record| record.id.as_str()))
        .collect::<HashSet<_>>();
    let mut logical_by_entry = BTreeMap::<&str, Vec<&native::LogicalSpan>>::new();
    for span in &logical {
        logical_by_entry.entry(&span.entry).or_default().push(span);
        if !matches!(
            span.classification.as_str(),
            "structural" | "typed" | "named_opaque"
        ) {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("{} has invalid logical classification", span.id),
                Some(span.id.clone()),
            ));
        }
        let owner_valid = if span.classification == "structural" {
            span.owner.is_none()
        } else {
            span.owner
                .as_ref()
                .is_some_and(|owner| logical_owner_ids.contains(owner.as_str()))
        };
        if !entry_lengths.contains_key(span.entry.as_str()) || !owner_valid {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("{} has an invalid logical entry or owner", span.id),
                Some(span.id.clone()),
            ));
        }
    }
    let covered_entries = logical_by_entry.keys().copied().collect::<HashSet<_>>();
    for entry in &entries {
        if entry.byte_len > 0 && !covered_entries.contains(entry.name.as_str()) {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("logical ledger omits nonempty entry {}", entry.name),
                Some(entry.id.clone()),
            ));
        }
    }
    for (name, mut spans) in logical_by_entry {
        spans.sort_by_key(|span| span.start);
        let expected = entry_lengths.get(name).copied();
        validate_logical_chain(name, &spans, expected, &mut findings);
    }
    let expected_coverage = container::byte_coverage(
        &physical,
        &entries,
        &logical,
        physical_end.unwrap_or_default(),
    );
    if coverage_records.as_slice() != [expected_coverage.clone()] || !expected_coverage.exact {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd byte coverage report is stale or does not prove exact closure",
            None,
        ));
    }
    findings
}

fn finding(check: Check, message: impl Into<String>, entity: Option<String>) -> Finding {
    Finding {
        check,
        severity: FindingSeverity::Error,
        message: message.into(),
        entity,
    }
}

fn validate_span_chain(
    label: &str,
    spans: &[native::ArchiveSpan],
    expected_end: Option<u64>,
    findings: &mut Vec<Finding>,
) {
    let mut ordered = spans.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|span| span.start);
    let valid = ordered.first().is_some_and(|span| span.start == 0)
        && ordered.iter().all(|span| span.start < span.end)
        && ordered.windows(2).all(|pair| pair[0].end == pair[1].start)
        && expected_end.is_none_or(|end| ordered.last().is_some_and(|span| span.end == end));
    if !valid {
        findings.push(finding(
            Check::PayloadIntegrity,
            format!("{label} ledger has a gap, overlap, or invalid boundary"),
            None,
        ));
    }
}

fn validate_logical_chain(
    name: &str,
    spans: &[&native::LogicalSpan],
    expected_end: Option<u64>,
    findings: &mut Vec<Finding>,
) {
    let valid = expected_end.is_some()
        && spans.first().is_some_and(|span| span.start == 0)
        && spans.iter().all(|span| span.start < span.end)
        && spans.windows(2).all(|pair| pair[0].end == pair[1].start)
        && expected_end.is_some_and(|end| spans.last().is_some_and(|span| span.end == end));
    if !valid {
        findings.push(finding(
            Check::PayloadIntegrity,
            format!("logical ledger for {name} has a gap, overlap, or invalid boundary"),
            None,
        ));
    }
}

impl CodecBackend for FcstdCodec {
    fn id(&self) -> &'static str {
        "fcstd"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(b"PK\x03\x04") {
            return Confidence::No;
        }
        if container::has_document_markers(prefix) {
            Confidence::High
        } else if contains(prefix, b"Document.xml") {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        container::scan(ctx, root).map(|scan| container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let options = DecodeOptions {
            container_only: ctx.container_only(),
            policy: *ctx.policy(),
        };
        let scan = container::scan(ctx, root)?;
        // Charge document object cardinality before persistence/geometry work.
        ctx.charge_entities(
            scan.document.object_count as u64,
            "admit FCStd document objects",
        )?;
        let mut admitted_entities = 0_u64;
        if !options.container_only
            && !matches!(scan.document.schema_version.as_str(), "2" | "3" | "4")
        {
            return Err(CodecError::NotImplemented(format!(
                "FCStd SchemaVersion={} FileVersion={} persistence layout",
                scan.document.schema_version, scan.document.file_version
            )));
        }
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "schema_version".into(),
            scan.document.schema_version.clone(),
        );
        attributes.insert("file_version".into(), scan.document.file_version.clone());
        attributes.insert("document_root".into(), scan.document.root_name.clone());
        attributes.insert(
            "object_count".into(),
            scan.document.object_count.to_string(),
        );
        attributes.insert("document_kind".into(), scan.document.document_kind.clone());
        attributes.insert(
            "application_domains".into(),
            scan.document.domains.join(","),
        );
        attributes.insert("archive_entry_count".into(), scan.entries.len().to_string());
        attributes.insert(
            "physical_ledger_spans".into(),
            scan.ledger.len().to_string(),
        );
        if let Some(last) = scan.ledger.last() {
            attributes.insert("physical_archive_bytes".into(), last.end.to_string());
        }
        if let Some(value) = &scan.document.program_version {
            attributes.insert("program_version".into(), value.clone());
        }
        let thumbnail = scan
            .data
            .get("thumbnails/Thumbnail.png")
            .map(|view| ("thumbnails/Thumbnail.png", view.window()))
            .or_else(|| {
                scan.data
                    .get("Thumbnail.png")
                    .map(|view| ("Thumbnail.png", view.window()))
            });
        if let Some((_, thumbnail)) = thumbnail {
            attributes.insert("thumbnail_bytes".into(), thumbnail.len().to_string());
        }
        let mut ir = CadIr::empty(Units::default());
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
        let mut geometry_transferred = false;
        let mut cycle_affected_design_objects = BTreeSet::new();
        ir.source = Some(SourceMeta {
            format: "fcstd".into(),
            attributes,
        });
        if let Some((name, bytes)) = thumbnail {
            ctx.charge_retained(bytes.len() as u64, "retain FCStd thumbnail", None)?;
            source_fidelity.attach_native_unknown_records(
                &mut ir,
                "fcstd",
                [UnknownRecord {
                    id: UnknownId(native::native_id("thumbnail", name)),
                    offset: 0,
                    byte_len: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                    data: Some(bytes.to_vec()),
                    links: vec![native::native_id("document", "0")],
                }],
            )?;
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.version = native::VERSION;
        namespace.set_arena("document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena("physical_ledger", &scan.ledger)?;
        #[allow(clippy::if_not_else)]
        if !options.container_only {
            let document_bytes = scan
                .data
                .get("Document.xml")
                .map(|view| view.window())
                .ok_or_else(|| {
                    CodecError::Malformed("Document.xml disappeared after scan".into())
                })?;
            let graph = persistence::parse_with_context(document_bytes, Some(ctx))?;
            for property in &graph.properties {
                for side_entry in &property.side_entries {
                    if !scan.data.contains_key(side_entry) {
                        return Err(CodecError::Malformed(format!(
                            "property {} references missing side entry {side_entry}",
                            property.id
                        )));
                    }
                }
            }
            let mut entry_records = scan
                .entries
                .iter()
                .map(|entry| {
                    let bytes = scan
                        .data
                        .get(&entry.name)
                        .map(|view| view.window())
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "entry {} disappeared after scan",
                                entry.name
                            ))
                        })?;
                    let referenced_by = graph
                        .properties
                        .iter()
                        .filter(|property| property.side_entries.contains(&entry.name))
                        .map(|property| property.id.clone())
                        .collect();
                    ctx.charge_retained(bytes.len() as u64, "retain FCStd entry", None)?;
                    Ok(native::EntryRecord {
                        id: native::native_id("entry", &entry.name),
                        name: entry.name.clone(),
                        role: entry.role.clone(),
                        byte_len: bytes.len() as u64,
                        sha256: sha256_hex(bytes),
                        referenced_by,
                        data: bytes.to_vec(),
                    })
                })
                .collect::<Result<Vec<_>, CodecError>>()?;
            let shape_payloads = brep::parse_payloads(&graph.properties, &entry_records)?;
            let (string_tables, mut element_maps) =
                element_map::parse(document_bytes, &graph.properties, &entry_records)?;
            namespace.set_arena("objects", &graph.objects)?;
            namespace.set_arena("extensions", &graph.extensions)?;
            namespace.set_arena("properties", &graph.properties)?;
            namespace.set_arena("entries", &entry_records)?;
            namespace.set_arena("shape_payloads", &shape_payloads)?;
            namespace.set_arena("carrier_census", &brep::carrier_census(&shape_payloads))?;
            namespace.set_arena("string_tables", &string_tables)?;
            let product_nodes = product::transfer(&graph.objects, &graph.properties, &scan.data)?;
            namespace.set_arena("product_nodes", &product_nodes)?;
            let joint_records = joint::transfer(&graph.objects, &graph.properties)?;
            namespace.set_arena("joints", &joint_records)?;
            let drawings = drawing::transfer(&graph.objects, &graph.properties)?;
            drawing::transfer_neutral(&mut ir.model, &drawings, &graph.properties)?;
            namespace.set_arena("drawings", &drawings)?;
            let annotations = annotation::transfer(&graph.objects, &graph.properties);
            annotation::transfer_neutral(
                &mut ir.model,
                &annotations,
                &graph.properties,
                &drawings,
            )?;
            namespace.set_arena("annotations", &annotations)?;
            namespace.set_arena(
                "applications",
                &application::transfer(&graph.objects, &graph.properties, &entry_records),
            )?;
            let attachments = attachment::transfer(&graph.objects, &graph.properties)?;
            namespace.set_arena("attachments", &attachments)?;
            let mut curve_transfer = brep::transfer_text_curves(&shape_payloads, &graph.properties);
            let surface_transfer = brep::transfer_text_surfaces(
                &shape_payloads,
                &graph.properties,
                &mut curve_transfer,
            );
            geometry_transferred =
                !curve_transfer.curves.is_empty() || !surface_transfer.surfaces.is_empty();
            ir.model.curves.extend(curve_transfer.curves);
            ir.model.procedural_curves.extend(curve_transfer.procedural);
            ir.model.surfaces.extend(surface_transfer.surfaces);
            ir.model
                .procedural_surfaces
                .extend(surface_transfer.procedural);
            geometry_transferred |=
                application_geometry::transfer(&mut ir, &graph.properties, &entry_records)?;
            let topology_occurrences =
                topology_transfer::transfer(ctx, &mut ir, &shape_payloads, &graph.properties)?;
            cycle_affected_design_objects = design::transfer(
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
                joint::transfer_neutral(&joint_records, &ir.model.occurrences);
            ctx.admit_entities(
                ir.model.entity_count() as u64,
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            let design_census = design::census(&graph.objects, &ir.model.features)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("design_census", &design_census)?;
            element_map::bind_topology(&mut element_maps, &topology_occurrences);
            let gui_graph = if let Some(gui_view) = scan.data.get("GuiDocument.xml") {
                gui::transfer(
                    &mut ir,
                    gui_view.window(),
                    &scan.data,
                    &graph.objects,
                    &graph.properties,
                    &shape_payloads,
                    &element_maps,
                    gui::requires_alpha_conversion(scan.document.program_version.as_deref()),
                )?
            } else {
                gui::Graph::default()
            };
            ctx.admit_entities(
                ir.model.entity_count() as u64,
                &mut admitted_entities,
                "admit FCStd entities",
            )?;
            for (entry_name, owner) in gui_graph
                .properties
                .iter()
                .flat_map(|property| {
                    property
                        .side_entries
                        .iter()
                        .map(move |entry| (entry.as_str(), property.id.as_str()))
                })
                .chain(gui_graph.documents.iter().flat_map(|document| {
                    document.states.iter().flat_map(|state| {
                        state
                            .side_entries
                            .iter()
                            .map(move |entry| (entry.as_str(), state.id.as_str()))
                    })
                }))
            {
                if let Some(entry) = entry_records
                    .iter_mut()
                    .find(|entry| entry.name == entry_name)
                {
                    if !entry
                        .referenced_by
                        .iter()
                        .any(|candidate| candidate == owner)
                    {
                        entry.referenced_by.push(owner.to_owned());
                    }
                }
            }
            ir.native
                .namespace_mut("fcstd")
                .set_arena("entries", &entry_records)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("gui_documents", &gui_graph.documents)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("gui_view_providers", &gui_graph.providers)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("gui_properties", &gui_graph.properties)?;
            let logical_ledger = container::logical_ledger(
                &entry_records,
                &graph.properties,
                &gui_graph,
                &shape_payloads,
                &string_tables,
                &element_maps,
            )?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("logical_ledger", &logical_ledger)?;
            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.end);
            let coverage = container::byte_coverage(
                &scan.ledger,
                &entry_records,
                &logical_ledger,
                physical_byte_len,
            );
            ir.native
                .namespace_mut("fcstd")
                .set_arena("byte_coverage", std::slice::from_ref(&coverage))?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("element_maps", &element_maps)?;
        } else {
            let physical_byte_len = scan.ledger.last().map_or(0, |span| span.end);
            let coverage = container::byte_coverage(&scan.ledger, &[], &[], physical_byte_len);
            ir.native
                .namespace_mut("fcstd")
                .set_arena("byte_coverage", std::slice::from_ref(&coverage))?;
        }
        let losses = if options.container_only {
            Vec::new()
        } else {
            semantic_losses(&ir, &cycle_affected_design_objects)
        };
        ctx.admit_entities(
            ir.model.entity_count() as u64,
            &mut admitted_entities,
            "admit FCStd entities",
        )?;
        Ok(DecodeResult::new(
            ir,
            DecodeReport {
                format: "fcstd".into(),
                container_only: options.container_only,
                geometry_transferred,
                coverage: std::collections::BTreeMap::new(),
                transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
                losses,
                notes: container::summarize(&scan).notes,
            },
            source_fidelity,
        ))
    }
}

impl Encoder for FcstdCodec {
    fn id(&self) -> &'static str {
        "fcstd"
    }

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let mut bytes = Vec::new();
        let mut report =
            self.encode_with_options(input.ir, &mut bytes, FcstdWriteOptions::default())?;
        // `encode_with_options` takes no fidelity sidecar, so the report it
        // returns states the only resolution it can see. Whether the caller
        // supplied one is known here, and only here.
        report.fidelity = if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        };
        Ok(ExportPlan::buffered(report, bytes))
    }
}

fn semantic_losses(ir: &CadIr, cycle_affected_design_objects: &BTreeSet<String>) -> Vec<LossNote> {
    let mut losses = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let definition = match &feature.definition {
                cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. } => {
                    operation.as_ref()
                }
                definition => definition,
            };
            let cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } = definition
            else {
                return None;
            };
            let cycle_affected = feature
                .native_ref
                .as_ref()
                .is_some_and(|id| cycle_affected_design_objects.contains(id));
            let (code, message) = if cycle_affected {
                (
                    FreecadLossCode::FeatureCyclicHistory,
                    format!(
                        "FCStd design operation {kind} is retained natively because history ordering is cycle-affected"
                    ),
                )
            } else {
                (
                    FreecadLossCode::FeatureNativeKindRetained,
                    format!(
                        "FCStd design operation {kind} is retained natively but has no neutral semantics"
                    ),
                )
            };
            Some(
                code.note(message)
                    .with_provenance(cadmpeg_ir::SourceProvenance {
                        format: "fcstd".into(),
                        stream: "Document.xml".into(),
                        offset: 0,
                        tag: feature.native_ref.clone(),
                    }),
            )
        })
        .collect::<Vec<_>>();
    losses.extend(ir.model.sketch_entities.iter().filter_map(|entity| {
        let cadmpeg_ir::sketches::SketchGeometry::Native { native_kind } = &entity.geometry else {
            return None;
        };
        Some(
            FreecadLossCode::SketchNativeGeometry
                .note(format!(
                    "FCStd sketch geometry {native_kind} is retained natively but is not neutralized"
                ))
                .with_provenance(cadmpeg_ir::SourceProvenance {
                    format: "fcstd".into(),
                    stream: "Document.xml".into(),
                    offset: 0,
                    tag: entity.native_ref.clone(),
                }),
        )
    }));
    losses.extend(ir.model.sketch_constraints.iter().filter_map(|constraint| {
        let cadmpeg_ir::sketches::SketchConstraintDefinition::Native { native_kind, .. } =
            &constraint.definition
        else {
            return None;
        };
        Some(
            FreecadLossCode::SketchNativeConstraint
                .note(format!(
                    "FCStd sketch constraint {native_kind} is retained natively but is not neutralized"
                ))
                .with_provenance(cadmpeg_ir::SourceProvenance {
                    format: "fcstd".into(),
                    stream: "Document.xml".into(),
                    offset: 0,
                    tag: constraint.native_ref.clone(),
                }),
        )
    }));
    losses
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
