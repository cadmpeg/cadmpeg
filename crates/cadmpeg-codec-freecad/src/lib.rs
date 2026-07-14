// SPDX-License-Identifier: Apache-2.0
//! Read ZIP-packaged `FreeCAD` `.FCStd` documents.

mod brep;
mod container;
mod design;
mod element_map;
mod gui;
mod native;
mod persistence;
mod product;
mod topology_transfer;

use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Check, Finding, Severity as FindingSeverity, SourceObjectAssociation};

/// Input-only `FCStd` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct FcstdCodec;

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
    let gui_properties = match namespace.arena_as::<native::GuiPropertyRecord>("gui_properties") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };
    let product_nodes = match namespace.arena_as::<native::ProductNodeRecord>("product_nodes") {
        Ok(records) => records,
        Err(error) => return vec![finding(Check::NativeLinks, error.to_string(), None)],
    };

    let mut findings = Vec::new();
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
    let gui_provider_ids = gui_providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect::<HashSet<_>>();
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
        {
            findings.push(finding(
                Check::ReferentialIntegrity,
                format!("{} has a missing product-structure link", node.id),
                Some(node.id.clone()),
            ));
        }
        if product_cycle(node.object.as_str(), &product_by_object) {
            findings.push(finding(
                Check::NativeLinks,
                format!("{} participates in a product-structure cycle", node.id),
                Some(node.id.clone()),
            ));
        }
        let invalid_array_count = node.element_count.is_some_and(|count| {
            count < 0
                || [node.element_transforms.len(), node.element_scales.len()]
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
        if property.owner != "fcstd:document#0"
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
            if target.starts_with("fcstd:object:") && !object_ids.contains(target) {
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
            .iter()
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
            if !property_ids.contains(owner.as_str()) {
                findings.push(finding(
                    Check::ReferentialIntegrity,
                    format!("{} has missing referencing property {owner}", entry.id),
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
    }
    for (name, mut spans) in logical_by_entry {
        spans.sort_by_key(|span| span.start);
        let expected = entry_lengths.get(name).copied();
        validate_logical_chain(name, &spans, expected, &mut findings);
    }
    findings
}

fn product_cycle(start: &str, nodes: &HashMap<&str, &native::ProductNodeRecord>) -> bool {
    fn visit<'a>(
        current: &'a str,
        start: &str,
        nodes: &HashMap<&'a str, &'a native::ProductNodeRecord>,
        seen: &mut HashSet<&'a str>,
    ) -> bool {
        let Some(node) = nodes.get(current) else {
            return false;
        };
        node.members
            .iter()
            .map(String::as_str)
            .chain(node.prototype.as_deref())
            .any(|target| {
                target == start || (seen.insert(target) && visit(target, start, nodes, seen))
            })
    }
    visit(start, start, nodes, &mut HashSet::from([start]))
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
        if contains(prefix, b"Document.xml")
            && contains(prefix, b"SchemaVersion")
            && contains(prefix, b"FileVersion")
        {
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
        let mut geometry_transferred = false;
        ir.source = Some(SourceMeta {
            format: "fcstd".into(),
            attributes,
        });
        if let Some((name, bytes)) = thumbnail {
            ir.set_native_unknowns(
                "fcstd",
                &[UnknownRecord {
                    id: UnknownId(format!("fcstd:entry:{name}")),
                    offset: 0,
                    byte_len: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                    data: Some(bytes.clone()),
                    links: vec!["fcstd:document#0".into()],
                }],
            )?;
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.version = native::VERSION;
        namespace.set_arena("document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena("physical_ledger", &scan.ledger)?;
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
            let entry_records = scan
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
                        id: format!("fcstd:entry:{}", entry.name),
                        name: entry.name.clone(),
                        role: entry.role.clone(),
                        byte_len: bytes.len() as u64,
                        sha256: sha256_hex(bytes),
                        referenced_by,
                        data: bytes.clone(),
                    })
                })
                .collect::<Result<Vec<_>, CodecError>>()?;
            let logical_ledger = logical_ledger(&entry_records, &graph.properties)?;
            let shape_payloads = brep::parse_payloads(&graph.properties, &entry_records)?;
            let (string_tables, mut element_maps) =
                element_map::parse(document_bytes, &graph.properties, &entry_records)?;
            namespace.set_arena("objects", &graph.objects)?;
            namespace.set_arena("extensions", &graph.extensions)?;
            namespace.set_arena("properties", &graph.properties)?;
            namespace.set_arena("entries", &entry_records)?;
            namespace.set_arena("logical_ledger", &logical_ledger)?;
            namespace.set_arena("shape_payloads", &shape_payloads)?;
            namespace.set_arena("string_tables", &string_tables)?;
            namespace.set_arena(
                "product_nodes",
                &product::transfer(&graph.objects, &graph.properties, &scan.data)?,
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
            ir.model.tessellations.extend(transfer_text_tessellations(
                &shape_payloads,
                &graph.properties,
            ));
            topology_transfer::transfer(&mut ir, &shape_payloads)?;
            design::transfer(&mut ir, &graph.objects, &graph.properties, &shape_payloads)?;
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
            ir.native
                .namespace_mut("fcstd")
                .set_arena("gui_view_providers", &gui_graph.providers)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("gui_properties", &gui_graph.properties)?;
            ir.native
                .namespace_mut("fcstd")
                .set_arena("element_maps", &element_maps)?;
        }
        let losses = if options.container_only {
            Vec::new()
        } else {
            semantic_losses(&ir)
        };
        Ok(DecodeResult::new(
            ir,
            DecodeReport {
                format: "fcstd".into(),
                container_only: options.container_only,
                geometry_transferred,
                losses,
                notes: container::summarize(&scan).notes,
            },
        ))
    }
}

fn semantic_losses(ir: &CadIr) -> Vec<LossNote> {
    let mut losses = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } = &feature.definition
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
            let id = CurveId(format!("{}:curve#{}", payload.id, index + 1));
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
                SurfaceId(format!("{}:surface#{}", payload.id, index + 1)),
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
                },
                cache_fit_tolerance: None,
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
                },
                cache_fit_tolerance: None,
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

fn transfer_text_tessellations(
    payloads: &[brep::ShapePayloadRecord],
    properties: &[native::PropertyRecord],
) -> Vec<Tessellation> {
    payloads
        .iter()
        .filter_map(|payload| {
            payload
                .text
                .as_ref()
                .map(|text| &text.triangulations)
                .or_else(|| payload.binary.as_ref().map(|binary| &binary.triangulations))
                .map(|triangulations| (payload, triangulations))
        })
        .flat_map(|(payload, triangulations)| {
            let object_id = properties
                .iter()
                .find(|property| property.id == payload.property)
                .map_or_else(
                    || payload.property.clone(),
                    |property| property.owner.clone(),
                );
            triangulations
                .iter()
                .enumerate()
                .map(move |(index, triangulation)| Tessellation {
                    id: format!("{}:triangulation#{}", payload.id, index + 1),
                    body: None,
                    source_object: Some(SourceObjectAssociation {
                        format: "fcstd".into(),
                        object_id: object_id.clone(),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                    vertices: triangulation.nodes.clone(),
                    triangles: triangulation
                        .triangles
                        .iter()
                        .map(|triangle| [triangle[0] - 1, triangle[1] - 1, triangle[2] - 1])
                        .collect(),
                    strip_lengths: Vec::new(),
                    normals: triangulation.normals.clone().unwrap_or_default(),
                    channels: Vec::new(),
                })
        })
        .collect()
}

fn logical_ledger(
    entries: &[native::EntryRecord],
    properties: &[native::PropertyRecord],
) -> Result<Vec<native::LogicalSpan>, CodecError> {
    let mut output = Vec::new();
    for entry in entries {
        if entry.name == "Document.xml" {
            let mut ranges = properties
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
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| range.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len {
                    return Err(CodecError::Malformed(
                        "overlapping or invalid Document.xml property spans".into(),
                    ));
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
            id: format!("fcstd:logical-span#{}", output.len()),
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
