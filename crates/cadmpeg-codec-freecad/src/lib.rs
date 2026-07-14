// SPDX-License-Identifier: Apache-2.0
//! Read ZIP-packaged `FreeCAD` `.FCStd` documents.

mod brep;
mod container;
mod native;
mod persistence;

use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
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

    let mut findings = Vec::new();
    let object_ids = objects
        .iter()
        .map(|record| record.id.as_str())
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
            namespace.set_arena("objects", &graph.objects)?;
            namespace.set_arena("extensions", &graph.extensions)?;
            namespace.set_arena("properties", &graph.properties)?;
            namespace.set_arena("entries", &entry_records)?;
            namespace.set_arena("logical_ledger", &logical_ledger)?;
            namespace.set_arena("shape_payloads", &shape_payloads)?;
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
            transfer_text_topology(&mut ir, &shape_payloads)?;
        }
        let losses = if options.container_only {
            Vec::new()
        } else {
            vec![LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Blocking,
                message: "FCStd persistence and exact-shape decoding are not implemented yet"
                    .into(),
                provenance: None,
            }]
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

fn transfer_text_topology(
    ir: &mut CadIr,
    payloads: &[brep::ShapePayloadRecord],
) -> Result<(), CodecError> {
    for payload in payloads {
        let tables = if let Some(text) = &payload.text {
            TopologyTables {
                locations: &text.locations,
                curve2ds: &text.curve2ds,
                tshapes: &text.tshapes,
                roots: &text.roots,
            }
        } else if let Some(binary) = &payload.binary {
            TopologyTables {
                locations: &binary.locations,
                curve2ds: &binary.curve2ds,
                tshapes: &binary.tshapes,
                roots: &binary.roots,
            }
        } else {
            continue;
        };
        let mut vertices = HashMap::new();
        for shape in tables.tshapes {
            let brep::TextTShapeGeometry::Vertex {
                tolerance, point, ..
            } = &shape.geometry
            else {
                continue;
            };
            let point_id = PointId(format!("{}:point#{}", payload.id, shape.index));
            let vertex_id = VertexId(format!("{}:vertex#{}", payload.id, shape.index));
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: *point,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: Some(*tolerance),
            });
            vertices.insert(shape.index, vertex_id);
        }

        let mut edges = HashMap::new();
        for shape in tables.tshapes {
            let brep::TextTShapeGeometry::Edge {
                tolerance,
                degenerated,
                representations,
                ..
            } = &shape.geometry
            else {
                continue;
            };
            let endpoint = |orientation: brep::TextOrientation| {
                shape
                    .children
                    .iter()
                    .find(|child| child.orientation == orientation)
                    .or_else(|| shape.children.first())
                    .and_then(|child| vertices.get(&child.shape))
                    .cloned()
            };
            let Some(start) = endpoint(brep::TextOrientation::Forward) else {
                continue;
            };
            let end = endpoint(brep::TextOrientation::Reversed).unwrap_or_else(|| start.clone());
            let curve_representation = representations
                .iter()
                .find(|representation| representation.kind == 1 && representation.location == 0);
            let curve = curve_representation.map(|representation| {
                CurveId(format!("{}:curve#{}", payload.id, representation.primary))
            });
            let edge_id = EdgeId(format!("{}:edge#{}", payload.id, shape.index));
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: (!*degenerated).then_some(curve).flatten(),
                start,
                end,
                param_range: curve_representation
                    .and_then(|representation| representation.parameter_range),
                tolerance: Some(*tolerance),
            });
            edges.insert(shape.index, edge_id);
            for (representation_index, representation) in representations.iter().enumerate() {
                if !matches!(representation.kind, 2 | 3) {
                    continue;
                }
                ir.model.pcurves.push(Pcurve {
                    id: text_pcurve_id(payload, shape.index, representation_index, false),
                    geometry: text_pcurve_geometry(&tables.curve2ds[representation.primary - 1]),
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: representation.parameter_range,
                    fit_tolerance: None,
                });
                if let Some(secondary) = representation.secondary {
                    ir.model.pcurves.push(Pcurve {
                        id: text_pcurve_id(payload, shape.index, representation_index, true),
                        geometry: text_pcurve_geometry(&tables.curve2ds[secondary - 1]),
                        wrapper_reversed: None,
                        native_tail_flags: None,
                        parameter_range: representation.parameter_range,
                        fit_tolerance: None,
                    });
                }
            }
        }

        let body_roots = collect_body_roots(&tables)?;
        for root in body_roots {
            append_body_topology(ir, payload, &tables, &root, &edges);
        }
    }
    close_radial_rings(&mut ir.model.coedges);
    Ok(())
}

struct TopologyTables<'a> {
    locations: &'a [brep::TextLocation],
    curve2ds: &'a [brep::TextCurve2d],
    tshapes: &'a [brep::TextTShape],
    roots: &'a [brep::TextShapeUse],
}

fn text_pcurve_id(
    payload: &brep::ShapePayloadRecord,
    edge: usize,
    representation: usize,
    secondary: bool,
) -> PcurveId {
    PcurveId(format!(
        "{}:pcurve#{}:{}:{}",
        payload.id,
        edge,
        representation + 1,
        usize::from(secondary) + 1
    ))
}

fn text_pcurve_geometry(curve: &brep::TextCurve2d) -> PcurveGeometry {
    match curve {
        brep::TextCurve2d::Line { origin, direction } => PcurveGeometry::Line {
            origin: *origin,
            direction: *direction,
        },
        brep::TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => PcurveGeometry::Circle {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            radius: *radius,
        },
        brep::TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PcurveGeometry::Ellipse {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        brep::TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => PcurveGeometry::Parabola {
            vertex: *vertex,
            x_axis: *x_axis,
            y_axis: *y_axis,
            focal_distance: *focal_distance,
        },
        brep::TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PcurveGeometry::Hyperbola {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        brep::TextCurve2d::Nurbs(nurbs) => PcurveGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots.clone(),
            control_points: nurbs.control_points.clone(),
            weights: nurbs.weights.clone(),
            periodic: nurbs.periodic,
        },
        brep::TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => PcurveGeometry::Trimmed {
            parameter_range: *parameter_range,
            basis: Box::new(text_pcurve_geometry(basis)),
        },
        brep::TextCurve2d::Offset { distance, basis } => PcurveGeometry::Offset {
            distance: *distance,
            basis: Box::new(text_pcurve_geometry(basis)),
        },
    }
}

fn collect_body_roots(tables: &TopologyTables<'_>) -> Result<Vec<brep::TextShapeUse>, CodecError> {
    fn visit(
        tables: &TopologyTables<'_>,
        shape_use: &brep::TextShapeUse,
        output: &mut Vec<brep::TextShapeUse>,
    ) -> Result<(), CodecError> {
        let shape = tables
            .tshapes
            .get(shape_use.shape - 1)
            .ok_or_else(|| CodecError::Malformed(format!("missing TShape {}", shape_use.shape)))?;
        match shape.kind {
            brep::TextShapeKind::Solid
            | brep::TextShapeKind::Shell
            | brep::TextShapeKind::Wire
            | brep::TextShapeKind::Face => output.push(shape_use.clone()),
            brep::TextShapeKind::CompSolid | brep::TextShapeKind::Compound => {
                for child in &shape.children {
                    visit(tables, child, output)?;
                }
            }
            brep::TextShapeKind::Vertex | brep::TextShapeKind::Edge => {}
        }
        Ok(())
    }

    let mut output = Vec::new();
    for root in tables.roots {
        visit(tables, root, &mut output)?;
    }
    Ok(output)
}

fn append_body_topology(
    ir: &mut CadIr,
    payload: &brep::ShapePayloadRecord,
    tables: &TopologyTables<'_>,
    root: &brep::TextShapeUse,
    edges: &HashMap<usize, EdgeId>,
) {
    let root_shape = &tables.tshapes[root.shape - 1];
    let body_id = BodyId(format!("{}:body#{}", payload.id, root.shape));
    let region_id = RegionId(format!("{}:region#{}", payload.id, root.shape));
    let kind = match root_shape.kind {
        brep::TextShapeKind::Solid => BodyKind::Solid,
        brep::TextShapeKind::Wire => BodyKind::Wire,
        brep::TextShapeKind::Shell | brep::TextShapeKind::Face => BodyKind::Sheet,
        _ => BodyKind::General,
    };
    let transform = (root.location != 0).then(|| tables.locations[root.location - 1].transform);
    let shell_uses = match root_shape.kind {
        brep::TextShapeKind::Solid => root_shape
            .children
            .iter()
            .filter(|child| tables.tshapes[child.shape - 1].kind == brep::TextShapeKind::Shell)
            .cloned()
            .collect::<Vec<_>>(),
        brep::TextShapeKind::Shell => vec![root.clone()],
        brep::TextShapeKind::Face | brep::TextShapeKind::Wire => vec![root.clone()],
        _ => Vec::new(),
    };
    let mut shell_ids = Vec::new();
    for shell_use in shell_uses {
        let shell_id = append_shell_topology(ir, payload, tables, &region_id, &shell_use, edges);
        shell_ids.push(shell_id);
    }
    if shell_ids.is_empty() {
        return;
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind,
        regions: vec![region_id.clone()],
        transform,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id,
        body: body_id,
        shells: shell_ids,
    });
}

fn append_shell_topology(
    ir: &mut CadIr,
    payload: &brep::ShapePayloadRecord,
    tables: &TopologyTables<'_>,
    region_id: &RegionId,
    shell_use: &brep::TextShapeUse,
    edges: &HashMap<usize, EdgeId>,
) -> ShellId {
    let shape = &tables.tshapes[shell_use.shape - 1];
    let shell_id = ShellId(format!("{}:shell#{}", payload.id, shell_use.shape));
    let face_uses = match shape.kind {
        brep::TextShapeKind::Shell => shape
            .children
            .iter()
            .filter(|child| tables.tshapes[child.shape - 1].kind == brep::TextShapeKind::Face)
            .cloned()
            .collect::<Vec<_>>(),
        brep::TextShapeKind::Face => vec![shell_use.clone()],
        _ => Vec::new(),
    };
    let mut face_ids = Vec::new();
    for face_use in face_uses {
        if let Some(face_id) =
            append_face_topology(ir, payload, tables, &shell_id, &face_use, edges)
        {
            face_ids.push(face_id);
        }
    }
    let wire_edges = if shape.kind == brep::TextShapeKind::Wire {
        shape
            .children
            .iter()
            .filter_map(|child| edges.get(&child.shape).cloned())
            .collect()
    } else {
        Vec::new()
    };
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: face_ids,
        wire_edges,
        free_vertices: Vec::new(),
    });
    shell_id
}

fn append_face_topology(
    ir: &mut CadIr,
    payload: &brep::ShapePayloadRecord,
    tables: &TopologyTables<'_>,
    shell_id: &ShellId,
    face_use: &brep::TextShapeUse,
    edges: &HashMap<usize, EdgeId>,
) -> Option<FaceId> {
    let shape = &tables.tshapes[face_use.shape - 1];
    let brep::TextTShapeGeometry::Face {
        tolerance,
        surface,
        location,
        ..
    } = &shape.geometry
    else {
        return None;
    };
    if *surface == 0 || *location != 0 || face_use.location != 0 {
        return None;
    }
    let face_id = FaceId(format!("{}:face#{}", payload.id, face_use.shape));
    let mut loop_ids = Vec::new();
    for (loop_index, wire_use) in shape
        .children
        .iter()
        .filter(|child| tables.tshapes[child.shape - 1].kind == brep::TextShapeKind::Wire)
        .enumerate()
    {
        let wire = &tables.tshapes[wire_use.shape - 1];
        let loop_id = LoopId(format!(
            "{}:loop#{}:{}",
            payload.id,
            face_use.shape,
            loop_index + 1
        ));
        let edge_uses = wire
            .children
            .iter()
            .filter(|child| edges.contains_key(&child.shape))
            .collect::<Vec<_>>();
        let coedge_ids = (0..edge_uses.len())
            .map(|index| {
                CoedgeId(format!(
                    "{}:coedge#{}:{}:{}",
                    payload.id,
                    face_use.shape,
                    loop_index + 1,
                    index + 1
                ))
            })
            .collect::<Vec<_>>();
        for (index, edge_use) in edge_uses.iter().enumerate() {
            let id = coedge_ids[index].clone();
            let next = coedge_ids[(index + 1) % coedge_ids.len()].clone();
            let previous = coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone();
            let edge_shape = &tables.tshapes[edge_use.shape - 1];
            let pcurve = match &edge_shape.geometry {
                brep::TextTShapeGeometry::Edge {
                    representations, ..
                } => representations
                    .iter()
                    .enumerate()
                    .find(|(_, representation)| {
                        matches!(representation.kind, 2 | 3)
                            && representation.surface == Some(*surface)
                            && representation.location == *location
                    })
                    .map(|(representation_index, representation)| {
                        let secondary = representation.secondary.is_some()
                            && edge_use.orientation == brep::TextOrientation::Reversed;
                        text_pcurve_id(payload, edge_use.shape, representation_index, secondary)
                    }),
                _ => None,
            };
            ir.model.coedges.push(Coedge {
                id: id.clone(),
                owner_loop: loop_id.clone(),
                edge: edges[&edge_use.shape].clone(),
                next,
                previous,
                radial_next: id,
                sense: use_sense(edge_use.orientation),
                pcurve,
            });
        }
        if !coedge_ids.is_empty() {
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                coedges: coedge_ids,
            });
            loop_ids.push(loop_id);
        }
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: SurfaceId(format!("{}:surface#{}", payload.id, surface)),
        sense: use_sense(face_use.orientation),
        loops: loop_ids,
        name: None,
        color: None,
        tolerance: Some(*tolerance),
    });
    Some(face_id)
}

fn use_sense(orientation: brep::TextOrientation) -> Sense {
    match orientation {
        brep::TextOrientation::Reversed => Sense::Reversed,
        brep::TextOrientation::Forward
        | brep::TextOrientation::Internal
        | brep::TextOrientation::External => Sense::Forward,
    }
}

fn close_radial_rings(coedges: &mut [Coedge]) {
    let mut by_edge: HashMap<EdgeId, Vec<usize>> = HashMap::new();
    for (index, coedge) in coedges.iter().enumerate() {
        by_edge.entry(coedge.edge.clone()).or_default().push(index);
    }
    for indices in by_edge.values() {
        for (position, index) in indices.iter().enumerate() {
            let next = indices[(position + 1) % indices.len()];
            coedges[*index].radial_next = coedges[next].id.clone();
        }
    }
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
