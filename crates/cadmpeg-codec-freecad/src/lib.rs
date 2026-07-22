// SPDX-License-Identifier: Apache-2.0
//! Read and write ZIP-packaged `FreeCAD` `.FCStd` documents.
//!
//! [`FcstdCodec`] implements [`Codec`] and [`Encoder`]. Retained writes preserve
//! unedited persistence records and named side entries, while checked mutation
//! methods update typed values. [`FcstdDocumentBuilder`] creates source-less
//! schema-4/file-1 application graphs. Other target bands and edits without a
//! lossless serializer are rejected explicitly.
//!
//! Support level: [L9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
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
mod mutation;
mod native;
mod persistence;
mod product;
mod topology_transfer;
mod writer;

use std::collections::{BTreeMap, BTreeSet};
use std::collections::{HashMap, HashSet};

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Check, Finding, Severity as FindingSeverity, SourceObjectAssociation};

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
        Ok(_) => findings.push(finding(
            Check::ReferentialIntegrity,
            "FCStd design census does not match projected feature semantics",
            None,
        )),
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
    if object_ids.len() != objects.len() || property_ids.len() != properties.len() {
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
        let effective_mismatch = attachment.placement.or(attachment.offset).unwrap_or([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]) != attachment.effective_frame;
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
    if attachments != attachment::transfer(&objects, &properties) {
        findings.push(finding(
            Check::NativeLinks,
            "FCStd attachment graph does not match the application property graph",
            None,
        ));
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
    let cyclic_products = product_cycle_nodes(&product_by_object);
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
            || drawing
                .views
                .iter()
                .any(|view| !object_ids.contains(view.as_str()))
            || drawing
                .template
                .as_ref()
                .is_some_and(|template| !object_ids.contains(template.as_str()))
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
    for extension in &extensions {
        if !object_ids.contains(extension.owner.as_str()) {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has missing owner {}", extension.id, extension.owner),
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
            if name.topology_ids.is_empty() {
                findings.push(finding(
                    Check::NativeLinks,
                    format!("{} has an unbound persistent element name", map.id),
                    Some(map.id.clone()),
                ));
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
    let expected_coverage = byte_coverage(
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

fn product_cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a native::ProductNodeRecord>,
) -> HashSet<&'a str> {
    let edges = |name: &'a str| {
        nodes.get(name).into_iter().flat_map(|node| {
            node.members
                .iter()
                .map(String::as_str)
                .chain(node.prototype.as_deref())
                .filter(|target| nodes.contains_key(target))
        })
    };
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    for &source in nodes.keys() {
        reverse.entry(source).or_default();
        for target in edges(source) {
            reverse.entry(target).or_default().push(source);
        }
    }

    let mut visited = HashSet::new();
    let mut finish = Vec::with_capacity(nodes.len());
    for &root in nodes.keys() {
        if !visited.insert(root) {
            continue;
        }
        let mut stack = vec![(root, edges(root).collect::<Vec<_>>(), 0_usize)];
        while let Some((current, targets, next)) = stack.last_mut() {
            if let Some(&target) = targets.get(*next) {
                *next += 1;
                if visited.insert(target) {
                    stack.push((target, edges(target).collect(), 0));
                }
            } else {
                finish.push(*current);
                stack.pop();
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cyclic = HashSet::new();
    while let Some(root) = finish.pop() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![root];
        while let Some(current) = stack.pop() {
            component.push(current);
            for &source in reverse.get(current).into_iter().flatten() {
                if assigned.insert(source) {
                    stack.push(source);
                }
            }
        }
        let self_cycle = component.len() == 1 && edges(component[0]).any(|target| target == root);
        if component.len() > 1 || self_cycle {
            cyclic.extend(component);
        }
    }
    cyclic
}

#[cfg(test)]
mod product_cycle_tests {
    use super::*;

    fn node(object: &str, members: &[&str]) -> native::ProductNodeRecord {
        native::ProductNodeRecord {
            id: format!("product:{object}"),
            object: object.into(),
            kind: "group".into(),
            members: members.iter().map(|member| (*member).into()).collect(),
            prototype: None,
            external_document: None,
            external_document_attribute: None,
            local_transform: None,
            placement_property: None,
            element_count: None,
            link_transform: None,
            element_transforms: Vec::new(),
            element_scales: Vec::new(),
            linked_subelements: Vec::new(),
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            scale: None,
            element_visibility: Vec::new(),
            element_objects: Vec::new(),
        }
    }

    #[test]
    fn reconvergent_product_graph_is_not_a_cycle() {
        let records = [node("A", &["C", "B"]), node("B", &["C"]), node("C", &[])];
        let nodes = records
            .iter()
            .map(|record| (record.object.as_str(), record))
            .collect();
        assert!(product_cycle_nodes(&nodes).is_empty());
    }

    #[test]
    fn product_cycle_marks_only_the_strongly_connected_component() {
        let records = [node("A", &["B"]), node("B", &["C"]), node("C", &["B"])];
        let nodes = records
            .iter()
            .map(|record| (record.object.as_str(), record))
            .collect();
        assert_eq!(product_cycle_nodes(&nodes), HashSet::from(["B", "C"]));
    }
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

impl Codec for FcstdCodec {
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

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        container::scan(reader).map(|scan| container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        let scan = container::scan(reader)?;
        if !options.container_only
            && (scan.document.schema_version != "4" || scan.document.file_version != "1")
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
            .map(|bytes| ("thumbnails/Thumbnail.png", bytes))
            .or_else(|| {
                scan.data
                    .get("Thumbnail.png")
                    .map(|bytes| ("Thumbnail.png", bytes))
            });
        if let Some((_, thumbnail)) = thumbnail {
            attributes.insert("thumbnail_bytes".into(), thumbnail.len().to_string());
        }
        let mut ir = CadIr::empty(Units::default());
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
        let mut geometry_transferred = false;
        ir.source = Some(SourceMeta {
            format: "fcstd".into(),
            attributes,
        });
        if let Some((name, bytes)) = thumbnail {
            source_fidelity.attach_native_unknown_records(
                &mut ir,
                "fcstd",
                &[UnknownRecord {
                    id: UnknownId(native::native_id("thumbnail", name)),
                    offset: 0,
                    byte_len: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                    data: Some(bytes.clone()),
                    links: vec![native::native_id("document", "0")],
                }],
            )?;
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.version = native::VERSION;
        namespace.set_arena("document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena("physical_ledger", &scan.ledger)?;
        #[allow(clippy::if_not_else)] // The full-decode path remains the primary linear flow.
        if !options.container_only {
            let document_bytes = scan.data.get("Document.xml").ok_or_else(|| {
                CodecError::Malformed("Document.xml disappeared after scan".into())
            })?;
            let graph = persistence::parse(document_bytes)?;
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
                    let bytes = scan.data.get(&entry.name).ok_or_else(|| {
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
                    Ok(native::EntryRecord {
                        id: native::native_id("entry", &entry.name),
                        name: entry.name.clone(),
                        role: entry.role.clone(),
                        byte_len: bytes.len() as u64,
                        sha256: sha256_hex(bytes),
                        referenced_by,
                        data: bytes.clone(),
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
            let joint_records = joint::transfer(&graph.objects, &graph.properties);
            namespace.set_arena("joints", &joint_records)?;
            let drawings = drawing::transfer(&graph.objects, &graph.properties);
            drawing::transfer_neutral(&mut ir.model, &drawings, &graph.properties);
            namespace.set_arena("drawings", &drawings)?;
            let annotations = annotation::transfer(&graph.objects, &graph.properties);
            annotation::transfer_neutral(&mut ir.model, &annotations, &graph.properties, &drawings);
            namespace.set_arena("annotations", &annotations)?;
            namespace.set_arena(
                "applications",
                &application::transfer(&graph.objects, &graph.properties, &entry_records),
            )?;
            namespace.set_arena(
                "attachments",
                &attachment::transfer(&graph.objects, &graph.properties),
            )?;
            let mut curve_transfer = transfer_text_curves(&shape_payloads, &graph.properties);
            let surface_transfer =
                transfer_text_surfaces(&shape_payloads, &graph.properties, &mut curve_transfer);
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
            topology_transfer::transfer(&mut ir, &shape_payloads, &graph.properties)?;
            design::transfer(
                &mut ir,
                &graph.objects,
                &graph.properties,
                &shape_payloads,
                &entry_records,
            )?;
            let (components, occurrences) = product::transfer_neutral(
                &product_nodes,
                &joint_records,
                &graph.objects,
                &graph.properties,
            )?;
            ir.model.components = components;
            ir.model.occurrences = occurrences;
            ir.model.assembly_joints = joint::transfer_neutral(
                &joint_records,
                &ir.model.components,
                &ir.model.occurrences,
            );
            let design_census = design::census(&graph.objects, &ir.model.features)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("design_census", &design_census)?;
            let payload_ids = shape_payloads
                .iter()
                .map(|payload| (payload.property.as_str(), payload.id.as_str()))
                .collect::<HashMap<_, _>>();
            element_map::bind_topology(&mut element_maps, &payload_ids, &ir);
            let gui_graph = if let Some(gui_bytes) = scan.data.get("GuiDocument.xml") {
                gui::transfer(
                    &mut ir,
                    gui_bytes,
                    &scan.data,
                    &graph.objects,
                    &graph.properties,
                    &shape_payloads,
                    &element_maps,
                )?
            } else {
                gui::Graph::default()
            };
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
                    entry.referenced_by.push(owner.to_owned());
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
            let logical_ledger = logical_ledger(
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
            let coverage = byte_coverage(
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
            let coverage = byte_coverage(&scan.ledger, &[], &[], physical_byte_len);
            ir.native
                .namespace_mut("fcstd")
                .set_arena("byte_coverage", std::slice::from_ref(&coverage))?;
        }
        let losses = if options.container_only {
            Vec::new()
        } else {
            semantic_losses(&ir)
        };
        Ok(DecodeResult::with_source_fidelity(
            ir,
            DecodeReport {
                format: "fcstd".into(),
                container_only: options.container_only,
                geometry_transferred,
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

    fn encode(
        &self,
        ir: &CadIr,
        output: &mut dyn std::io::Write,
    ) -> Result<ExportReport, CodecError> {
        self.encode_with_options(ir, output, FcstdWriteOptions::default())
    }
}

fn semantic_losses(ir: &CadIr) -> Vec<LossNote> {
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
            Some(LossNote {
                category: LossCategory::Other,
                severity: Severity::Blocking,
                message: format!(
                    "FCStd design operation {kind} is retained natively but has no neutral semantics"
                ),
                provenance: Some(cadmpeg_ir::LossProvenance {
                    format: "fcstd".into(),
                    stream: "Document.xml".into(),
                    offset: 0,
                    tag: feature.native_ref.clone(),
                }),
            })
        })
        .collect::<Vec<_>>();
    losses.extend(ir.model.features.iter().filter_map(|feature| {
        let definition = match &feature.definition {
            cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. } => {
                operation.as_ref()
            }
            definition => definition,
        };
        let cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern:
                cadmpeg_ir::features::PatternKind::Linear {
                    direction: None, ..
                },
            ..
        } = definition
        else {
            return None;
        };
        Some(LossNote {
            category: LossCategory::Other,
            severity: Severity::Blocking,
            message: "FCStd linear-pattern direction is retained as a native reference but is not geometrically resolved".into(),
            provenance: Some(cadmpeg_ir::LossProvenance {
                format: "fcstd".into(),
                stream: "Document.xml".into(),
                offset: 0,
                tag: feature.native_ref.clone(),
            }),
        })
    }));
    losses.extend(ir.model.sketch_entities.iter().filter_map(|entity| {
        let cadmpeg_ir::sketches::SketchGeometry::Native { native_kind } = &entity.geometry else {
            return None;
        };
        Some(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "FCStd sketch geometry {native_kind} is retained natively but is not neutralized"
            ),
            provenance: Some(cadmpeg_ir::LossProvenance {
                format: "fcstd".into(),
                stream: "Document.xml".into(),
                offset: 0,
                tag: entity.native_ref.clone(),
            }),
        })
    }));
    losses.extend(ir.model.sketch_constraints.iter().filter_map(|constraint| {
        let cadmpeg_ir::sketches::SketchConstraintDefinition::Native { native_kind, .. } =
            &constraint.definition
        else {
            return None;
        };
        Some(LossNote {
            category: LossCategory::Other,
            severity: Severity::Blocking,
            message: format!(
                "FCStd sketch constraint {native_kind} is retained natively but is not neutralized"
            ),
            provenance: Some(cadmpeg_ir::LossProvenance {
                format: "fcstd".into(),
                stream: "Document.xml".into(),
                offset: 0,
                tag: constraint.native_ref.clone(),
            }),
        })
    }));
    losses
}

#[derive(Default)]
struct CurveTransfer {
    curves: Vec<Curve>,
    procedural: Vec<ProceduralCurve>,
}

fn transfer_text_curves(
    payloads: &[brep::ShapePayloadRecord],
    properties: &[native::PropertyRecord],
) -> CurveTransfer {
    let mut transfer = CurveTransfer::default();
    for payload in payloads {
        let curves = if let Some(text) = &payload.text {
            &text.curves
        } else if let Some(binary) = &payload.binary {
            &binary.curves
        } else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or_else(
                || payload.property.clone(),
                |property| property.owner.clone(),
            );
        let association = SourceObjectAssociation {
            format: "fcstd".into(),
            object_id,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, curve) in curves.iter().enumerate() {
            let id = CurveId(native::model_id(
                "curve",
                &payload.id,
                (index + 1).to_string(),
            ));
            append_text_curve(curve, id, &association, &mut transfer);
        }
    }
    transfer
}

fn append_text_curve(
    curve: &brep::TextCurve,
    id: CurveId,
    association: &SourceObjectAssociation,
    transfer: &mut CurveTransfer,
) -> CurveGeometry {
    let geometry = match curve {
        brep::TextCurve::Line { origin, direction } => CurveGeometry::Line {
            origin: *origin,
            direction: *direction,
        },
        brep::TextCurve::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => CurveGeometry::Circle {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        brep::TextCurve::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Ellipse {
            center: *center,
            axis: *axis,
            major_direction: *major_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        brep::TextCurve::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => CurveGeometry::Parabola {
            vertex: *vertex,
            axis: *axis,
            major_direction: *major_direction,
            focal_distance: *focal_distance,
        },
        brep::TextCurve::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Hyperbola {
            center: *center,
            axis: *axis,
            major_direction: *major_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        brep::TextCurve::Nurbs(nurbs) => CurveGeometry::Nurbs(nurbs.clone()),
        brep::TextCurve::Trimmed {
            parameter_range,
            basis,
        } => {
            let basis_id = CurveId(format!("{}:basis", id.0));
            let basis_geometry = append_text_curve(basis, basis_id.clone(), association, transfer);
            transfer.procedural.push(ProceduralCurve {
                id: ProceduralCurveId(format!("{}:construction", id.0)),
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Subset {
                    source: basis_id,
                    parameter_range: *parameter_range,
                },
                cache_fit_tolerance: None,
            });
            basis_geometry
        }
        brep::TextCurve::Offset {
            distance,
            direction,
            basis,
        } => {
            let basis_id = CurveId(format!("{}:basis", id.0));
            append_text_curve(basis, basis_id.clone(), association, transfer);
            transfer.procedural.push(ProceduralCurve {
                id: ProceduralCurveId(format!("{}:construction", id.0)),
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Offset {
                    source: basis_id,
                    distance: *distance,
                    direction: Some(*direction),
                    support: None,
                    distance_law: None,
                    normal: None,
                    parameter_range: None,
                },
                cache_fit_tolerance: None,
            });
            CurveGeometry::Unknown { record: None }
        }
    };
    transfer.curves.push(Curve {
        id,
        geometry: geometry.clone(),
        source_object: Some(association.clone()),
    });
    geometry
}

#[derive(Default)]
struct SurfaceTransfer {
    surfaces: Vec<Surface>,
    procedural: Vec<ProceduralSurface>,
}

fn transfer_text_surfaces(
    payloads: &[brep::ShapePayloadRecord],
    properties: &[native::PropertyRecord],
    curve_transfer: &mut CurveTransfer,
) -> SurfaceTransfer {
    let mut transfer = SurfaceTransfer::default();
    for payload in payloads {
        let surfaces = if let Some(text) = &payload.text {
            &text.surfaces
        } else if let Some(binary) = &payload.binary {
            &binary.surfaces
        } else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or_else(
                || payload.property.clone(),
                |property| property.owner.clone(),
            );
        let association = SourceObjectAssociation {
            format: "fcstd".into(),
            object_id,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, surface) in surfaces.iter().enumerate() {
            append_text_surface(
                surface,
                SurfaceId(native::model_id(
                    "surface",
                    &payload.id,
                    (index + 1).to_string(),
                )),
                &association,
                curve_transfer,
                &mut transfer,
            );
        }
    }
    transfer
}

fn append_text_surface(
    surface: &brep::TextSurface,
    id: SurfaceId,
    association: &SourceObjectAssociation,
    curve_transfer: &mut CurveTransfer,
    transfer: &mut SurfaceTransfer,
) -> SurfaceGeometry {
    let geometry = match surface {
        brep::TextSurface::Plane {
            origin,
            axis,
            u_axis,
            ..
        } => SurfaceGeometry::Plane {
            origin: *origin,
            normal: *axis,
            u_axis: *u_axis,
        },
        brep::TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
            ..
        } => SurfaceGeometry::Cylinder {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        brep::TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            half_angle,
        } => SurfaceGeometry::Cone {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
            ratio: 1.0,
            half_angle: *half_angle,
        },
        brep::TextSurface::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => SurfaceGeometry::Sphere {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        brep::TextSurface::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => SurfaceGeometry::Torus {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        brep::TextSurface::Nurbs(nurbs) => SurfaceGeometry::Nurbs(nurbs.clone()),
        brep::TextSurface::Extrusion {
            direction,
            directrix,
        } => {
            let directrix_id = CurveId(format!("{}:directrix", id.0));
            append_text_curve(directrix, directrix_id.clone(), association, curve_transfer);
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: directrix_id,
                    parameter_interval: None,
                    direction: *direction,
                    native_position: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
        brep::TextSurface::Revolution {
            axis_origin,
            axis_direction,
            directrix,
        } => {
            let directrix_id = CurveId(format!("{}:directrix", id.0));
            append_text_curve(directrix, directrix_id.clone(), association, curve_transfer);
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: directrix_id,
                    axis_origin: *axis_origin,
                    axis_direction: *axis_direction,
                    angular_interval: [0.0, std::f64::consts::TAU],
                    parameter_interval: None,
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
        brep::TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => {
            let basis_id = SurfaceId(format!("{}:basis", id.0));
            let basis_geometry = append_text_surface(
                basis,
                basis_id.clone(),
                association,
                curve_transfer,
                transfer,
            );
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Subset {
                    support: basis_id,
                    parameter_ranges: *parameter_ranges,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            basis_geometry
        }
        brep::TextSurface::Offset { distance, basis } => {
            let basis_id = SurfaceId(format!("{}:basis", id.0));
            append_text_surface(
                basis,
                basis_id.clone(),
                association,
                curve_transfer,
                transfer,
            );
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support: basis_id,
                    distance: *distance,
                    u_sense: None,
                    v_sense: None,
                    extension_flags: Vec::new(),
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
    };
    transfer.surfaces.push(Surface {
        id,
        geometry: geometry.clone(),
        source_object: Some(association.clone()),
    });
    geometry
}

fn logical_ledger(
    entries: &[native::EntryRecord],
    properties: &[native::PropertyRecord],
    gui: &gui::Graph,
    shape_payloads: &[brep::ShapePayloadRecord],
    string_tables: &[native::StringTableRecord],
    element_maps: &[native::ElementMapRecord],
) -> Result<Vec<native::LogicalSpan>, CodecError> {
    let typed_entries = shape_payloads
        .iter()
        .map(|payload| (payload.entry.as_str(), payload.id.as_str()))
        .chain(string_tables.iter().filter_map(|table| {
            table
                .source_entry
                .as_deref()
                .map(|entry| (entry, table.id.as_str()))
        }))
        .chain(element_maps.iter().filter_map(|map| {
            map.source_entry
                .as_deref()
                .map(|entry| (entry, map.id.as_str()))
        }))
        .collect::<HashMap<_, _>>();
    let mut output = Vec::new();
    for entry in entries {
        let typed_owner = typed_entries
            .get(entry.id.as_str())
            .or_else(|| typed_entries.get(entry.name.as_str()));
        if let Some(owner) = typed_owner {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len,
                "typed",
                Some((*owner).to_owned()),
            );
        } else if entry.name == "Document.xml" || entry.name == "GuiDocument.xml" {
            let mut ranges = if entry.name == "Document.xml" {
                properties
                    .iter()
                    .map(|property| {
                        (
                            property.byte_start,
                            property.byte_end,
                            if property.family == native::PropertyFamily::Unknown {
                                "named_opaque"
                            } else {
                                "typed"
                            },
                            property.id.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                gui.properties
                    .iter()
                    .map(|property| {
                        (
                            property.byte_start,
                            property.byte_end,
                            "typed",
                            property.id.clone(),
                        )
                    })
                    .chain(gui.documents.iter().flat_map(|document| {
                        document.states.iter().map(|state| {
                            (state.byte_start, state.byte_end, "typed", state.id.clone())
                        })
                    }))
                    .collect::<Vec<_>>()
            };
            ranges.sort_by_key(|range| range.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len {
                    return Err(CodecError::Malformed(format!(
                        "overlapping or invalid {} record spans",
                        entry.name
                    )));
                }
                push_logical_span(&mut output, entry, cursor, start, "structural", None);
                push_logical_span(&mut output, entry, start, end, classification, Some(owner));
                cursor = end;
            }
            push_logical_span(
                &mut output,
                entry,
                cursor,
                entry.byte_len,
                "structural",
                None,
            );
        } else {
            let owner = entry
                .referenced_by
                .first()
                .cloned()
                .unwrap_or_else(|| entry.id.clone());
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len,
                "named_opaque",
                Some(owner),
            );
        }
    }
    Ok(output)
}

fn byte_coverage(
    physical: &[native::ArchiveSpan],
    entries: &[native::EntryRecord],
    logical: &[native::LogicalSpan],
    physical_byte_len: u64,
) -> native::ByteCoverageRecord {
    let mut classification_bytes = BTreeMap::new();
    let mut named_opaque_entries = BTreeSet::new();
    for span in logical {
        *classification_bytes
            .entry(span.classification.clone())
            .or_insert(0) += span.end.saturating_sub(span.start);
        if span.classification == "named_opaque" {
            named_opaque_entries.insert(span.entry.clone());
        }
    }
    let mut ordered_physical = physical.iter().collect::<Vec<_>>();
    ordered_physical.sort_by_key(|span| span.start);
    let physical_exact = ordered_physical.first().is_some_and(|span| span.start == 0)
        && ordered_physical.iter().all(|span| span.start < span.end)
        && ordered_physical
            .windows(2)
            .all(|pair| pair[0].end == pair[1].start)
        && ordered_physical
            .last()
            .is_some_and(|span| span.end == physical_byte_len);
    let logical_exact = logical.iter().all(|span| {
        entries.iter().any(|entry| entry.name == span.entry)
            && span.start < span.end
            && matches!(
                span.classification.as_str(),
                "structural" | "typed" | "named_opaque"
            )
    }) && entries.iter().all(|entry| {
        let mut spans = logical
            .iter()
            .filter(|span| span.entry == entry.name)
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start);
        if entry.byte_len == 0 {
            spans.is_empty()
        } else {
            spans.first().is_some_and(|span| span.start == 0)
                && spans.windows(2).all(|pair| pair[0].end == pair[1].start)
                && spans.last().is_some_and(|span| span.end == entry.byte_len)
        }
    });
    native::ByteCoverageRecord {
        id: native::native_id("byte-coverage", "0"),
        physical_byte_len,
        physical_span_count: physical.len(),
        logical_entry_count: entries.len(),
        logical_byte_len: entries.iter().map(|entry| entry.byte_len).sum(),
        logical_span_count: logical.len(),
        classification_bytes,
        named_opaque_entries: named_opaque_entries.into_iter().collect(),
        exact: physical_exact && logical_exact,
    }
}

fn push_logical_span(
    output: &mut Vec<native::LogicalSpan>,
    entry: &native::EntryRecord,
    start: u64,
    end: u64,
    classification: &str,
    owner: Option<String>,
) {
    if start < end {
        output.push(native::LogicalSpan {
            id: native::native_id("logical-span", output.len().to_string()),
            entry: entry.name.clone(),
            start,
            end,
            classification: classification.into(),
            owner,
        });
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests;
