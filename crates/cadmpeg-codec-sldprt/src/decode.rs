// SPDX-License-Identifier: Apache-2.0
//! High-level `.sldprt` decoding.
//!
//! [`decode`] scans the outer [`crate::container`], groups related Parasolid
//! `partition` and `deltas` streams, and selects the group that yields the
//! richest B-rep. It then adds appearances, display meshes, document attributes,
//! feature history, feature-input lanes, provenance, and retained source data.
//!
//! The returned [`DecodeResult`] contains both the IR and its diagnostics.
//! Untyped surface and curve carriers become opaque geometry linked to the
//! retained partition. If no body stream yields geometry, decoding returns a
//! metadata-only IR and blocking loss notes. [`DecodeOptions::container_only`]
//! requests the metadata-only path.

use std::collections::BTreeMap;

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{AppearanceId, UnknownId};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use crate::brep::{self, Brep};
use crate::container::{self, Block, ContainerScan};
use crate::parasolid::StreamHeader;

struct BodyStream<'a> {
    block: &'a Block,
    payload: &'a [u8],
    header: StreamHeader,
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through [`DecodeResult::report`] when a partial result can be represented.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        if let Some((selected, decoded, report)) = try_decode_brep(&scan, &streams) {
            let ir = build_geometry_ir(
                &scan,
                streams[selected].block,
                &streams[selected].header,
                decoded,
            );
            return Ok(DecodeResult::new(ir, report));
        }
    }

    let ir = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    Ok(DecodeResult::new(ir, report))
}

/// Decode the active Parasolid stream's B-rep. Returns `None` when the stream
/// frames but yields no geometry, so the caller falls back to metadata.
fn active_body_streams(scan: &ContainerScan) -> Vec<BodyStream<'_>> {
    let mut streams: Vec<_> = scan
        .blocks
        .iter()
        .flat_map(|block| {
            block.ps_streams.iter().filter_map(move |payload| {
                let header = crate::parasolid::stream_header(payload)?;
                let section = block.section.as_deref().unwrap_or("").to_ascii_lowercase();
                if crate::parasolid::is_body_stream(&header)
                    && !section.contains("ghost")
                    && !section.contains("resolvedfeatures")
                {
                    Some(BodyStream {
                        block,
                        payload,
                        header,
                    })
                } else {
                    None
                }
            })
        })
        .collect();
    streams.sort_by_key(|stream| {
        let section = stream
            .block
            .section
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        (
            !section.contains("partition"),
            !stream
                .header
                .description
                .to_ascii_lowercase()
                .contains("partition"),
        )
    });
    streams
}

fn try_decode_brep(
    scan: &ContainerScan,
    streams: &[BodyStream<'_>],
) -> Option<(usize, Brep, DecodeReport)> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        sites.entry(site_key(stream.block)).or_default().push(index);
    }
    let mut best: Option<(usize, Brep)> = None;
    for indices in sites.values() {
        let first = indices[0];
        let name = streams[first]
            .block
            .section
            .clone()
            .unwrap_or_else(|| format!("block@{}", streams[first].block.offset));
        let bodies: Vec<_> = indices
            .iter()
            .map(|index| (streams[*index].payload, &streams[*index].header))
            .collect();
        let decoded = brep::decode_bodies(&bodies, &name);
        let score = (
            decoded.faces.len(),
            decoded.bodies.len(),
            decoded.points.len(),
        );
        let best_score = best
            .as_ref()
            .map(|(_, brep)| (brep.faces.len(), brep.bodies.len(), brep.points.len()));
        if best_score.is_none_or(|current| score > current) {
            best = Some((first, decoded));
        }
    }
    let (selected, decoded) = best?;
    if decoded.faces.is_empty() && decoded.surfaces.is_empty() && decoded.points.is_empty() {
        return None;
    }
    let report = build_geometry_report(scan, &decoded);
    Some((selected, decoded, report))
}

fn site_key(block: &Block) -> String {
    let mut key = block
        .section
        .clone()
        .unwrap_or_else(|| format!("block@{}", block.offset))
        .to_ascii_lowercase();
    for suffix in ["partition", "deltas"] {
        if let Some(at) = key.rfind(suffix) {
            key.truncate(at);
            break;
        }
    }
    key.trim_end_matches(['-', '/', '_']).to_string()
}

fn build_geometry_ir(
    scan: &ContainerScan,
    block: &Block,
    header: &StreamHeader,
    mut brep: Brep,
) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    let materials = crate::appearance::materials(scan);
    if let Some(material) = materials.first() {
        for body in &mut brep.bodies {
            body.color = Some(material.color);
            if body.name.is_none() {
                body.name = Some(material.name.clone());
            }
        }
    }
    ir.source = Some(source_meta(scan, block, header));
    ir.annotations = std::mem::take(&mut brep.annotations);
    let histories = crate::history::histories(scan, &mut ir.annotations);
    let lanes = crate::resolved_features::lanes(scan, &mut ir.annotations);
    let attributes = crate::metadata::attributes(scan, &mut ir.annotations);
    ir.native
        .sldprt
        .get_or_insert_with(cadmpeg_ir::SldprtNative::default)
        .feature_histories = histories;
    ir.native
        .sldprt
        .get_or_insert_with(cadmpeg_ir::SldprtNative::default)
        .feature_input_lanes = lanes;
    ir.model.attributes = attributes;

    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    ir.unknowns = brep.unknowns;
    for face_color in brep.face_colors {
        let id = AppearanceId(format!(
            "sldprt:appearance:entity53#{}",
            face_color.color_attr
        ));
        crate::annotations::note(
            &mut ir.annotations,
            id.0.clone(),
            header.description.clone(),
            face_color.offset as u64,
            "00_53_color",
            Exactness::ByteExact,
        );
        if !ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == id)
        {
            ir.model.appearances.push(Appearance {
                id: id.clone(),
                name: None,
                asset_guid: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("entity-53".into()),
                category: None,
                base_color: Some(face_color.color),
                properties: BTreeMap::new(),
            });
        }
        if let Some(target) = face_color.target {
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!(
                    "sldprt:appearance:binding#face:{}:{}",
                    face_color.face_attr, face_color.color_attr
                ),
                target: AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(target)),
                appearance: id,
                source_entity_id: Some(face_color.face_attr.to_string()),
                object_type: Some("Face".into()),
                channels: BTreeMap::new(),
            });
        }
    }
    for (index, material) in materials.into_iter().enumerate() {
        let id = AppearanceId(format!("sldprt:appearance:material#{index}"));
        let material_stream = format!("block@{}", material.block_offset);
        crate::annotations::note(
            &mut ir.annotations,
            id.0.clone(),
            material_stream.clone(),
            material.record_offset as u64,
            "moVisualProperties_c",
            Exactness::ByteExact,
        );
        ir.model.appearances.push(Appearance {
            id: id.clone(),
            name: Some(material.name),
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("moVisualProperties_c".to_string()),
            category: None,
            base_color: Some(material.color),
            properties: BTreeMap::new(),
        });
        if index == 0 {
            for (body_index, body) in ir.model.bodies.iter().enumerate() {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: format!("sldprt:appearance:binding#body:{body_index}:{index}"),
                    target: AppearanceTarget::Body(body.id.clone()),
                    appearance: id.clone(),
                    source_entity_id: None,
                    object_type: Some("Body".to_string()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for display in scan
        .blocks
        .iter()
        .filter(|block| crate::tessellation::block_summary(block).is_some())
    {
        for (index, mesh) in crate::tessellation::block_meshes(display)
            .into_iter()
            .enumerate()
        {
            let id = format!("sldprt:displaylist:record#{}:{index}", display.offset);
            let display_stream = display
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", display.offset));
            crate::annotations::note(
                &mut ir.annotations,
                id.clone(),
                display_stream,
                0,
                "displaylist_tessellation",
                Exactness::ByteExact,
            );
            ir.model
                .tessellations
                .push(cadmpeg_ir::tessellation::Tessellation {
                    id,
                    source_object: None,
                    vertices: mesh.vertices,
                    triangles: mesh.triangles,
                    strip_lengths: mesh.strip_lengths,
                    normals: mesh.normals,
                    channels: mesh.channels,
                });
        }
        let display_id = format!("sldprt:displaylist:record#{}", display.offset);
        crate::annotations::note(
            &mut ir.annotations,
            display_id.clone(),
            display
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", display.offset)),
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        ir.unknowns.push(UnknownRecord {
            id: UnknownId(display_id),
            offset: display.offset as u64,
            byte_len: display.uncomp_sz as u64,
            sha256: sha256_hex(&display.payload),
            data: Some(display.payload.clone()),
            links: Vec::new(),
        });
    }
    for source_block in &scan.blocks {
        if ir
            .unknowns
            .iter()
            .any(|record| record.id.0 == format!("sldprt:file:block#{}", source_block.offset))
        {
            continue;
        }
        let id = format!("sldprt:file:block#{}", source_block.offset);
        crate::annotations::note(
            &mut ir.annotations,
            id.clone(),
            source_block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", source_block.offset)),
            source_block.offset as u64,
            source_block.family,
            Exactness::ByteExact,
        );
        ir.unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: 0,
            byte_len: source_block.payload.len() as u64,
            sha256: sha256_hex(&source_block.payload),
            data: Some(source_block.payload.clone()),
            links: Vec::new(),
        });
    }
    let partition_id = UnknownId(format!("sldprt:file:block#{}", block.offset));
    let opaque_surfaces = ir
        .model
        .surfaces
        .iter_mut()
        .filter_map(|surface| match &mut surface.geometry {
            SurfaceGeometry::Unknown { record } => {
                *record = Some(partition_id.clone());
                Some(surface.id.0.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let opaque_curves = ir
        .model
        .curves
        .iter_mut()
        .filter_map(|curve| match &mut curve.geometry {
            cadmpeg_ir::geometry::CurveGeometry::Unknown { record } => {
                *record = Some(partition_id.clone());
                Some(curve.id.0.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !opaque_surfaces.is_empty() || !opaque_curves.is_empty() {
        let partition = ir
            .unknowns
            .iter_mut()
            .find(|record| record.id == partition_id)
            .expect("active partition block is retained");
        partition.links.extend(opaque_surfaces);
        partition.links.extend(opaque_curves);
    }
    preserve_source_image(scan, &mut ir);
    set_semantic_hash(&mut ir);
    ir
}

fn source_meta(scan: &ContainerScan, block: &Block, header: &StreamHeader) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    let display = crate::tessellation::summary(scan);
    if display.vertices > 0 {
        attributes.insert(
            "displaylist_vertices".to_string(),
            display.vertices.to_string(),
        );
        attributes.insert(
            "displaylist_triangles".to_string(),
            display.triangles.to_string(),
        );
    }
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    attributes.insert(
        "active_parasolid_block".to_string(),
        block
            .section
            .clone()
            .unwrap_or_else(|| format!("block@{}", block.offset)),
    );
    attributes.insert("parasolid_schema".to_string(), header.schema.clone());
    attributes.insert(
        "parasolid_description".to_string(),
        header.description.clone(),
    );
    add_preview_metadata(scan, &mut attributes);
    add_solidworks_xml_metadata(scan, &mut attributes);
    SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    }
}

fn add_preview_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    let mut png_index = 0;
    let mut bmp_index = 0;
    for block in &scan.blocks {
        match block.family {
            "png-preview" => {
                let payload = &block.payload;
                if payload.get(8..16) != Some(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']) {
                    continue;
                }
                let Some(width) = be_u32(payload, 16) else {
                    continue;
                };
                let Some(height) = be_u32(payload, 20) else {
                    continue;
                };
                let Some(fields) = payload.get(24..29) else {
                    continue;
                };
                let prefix = format!("png_preview_{png_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_bit_depth"), fields[0].to_string());
                attributes.insert(format!("{prefix}_color_type"), fields[1].to_string());
                attributes.insert(format!("{prefix}_compression"), fields[2].to_string());
                attributes.insert(format!("{prefix}_filter"), fields[3].to_string());
                attributes.insert(format!("{prefix}_interlace"), fields[4].to_string());
                png_index += 1;
            }
            "bmp-thumbnail" => {
                let payload = &block.payload;
                let (Some(width), Some(height), Some(image_size)) =
                    (le_i32(payload, 8), le_i32(payload, 12), le_u32(payload, 24))
                else {
                    continue;
                };
                let (Some(planes), Some(bits_per_pixel), Some(compression)) = (
                    le_u16(payload, 16),
                    le_u16(payload, 18),
                    le_u32(payload, 20),
                ) else {
                    continue;
                };
                let prefix = format!("bmp_thumbnail_{bmp_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_planes"), planes.to_string());
                attributes.insert(format!("{prefix}_bit_count"), bits_per_pixel.to_string());
                attributes.insert(format!("{prefix}_compression"), compression.to_string());
                attributes.insert(format!("{prefix}_image_size"), image_size.to_string());
                bmp_index += 1;
            }
            _ => {}
        }
    }
    attributes.insert("png_preview_count".into(), png_index.to_string());
    attributes.insert("bmp_thumbnail_count".into(), bmp_index.to_string());
}

fn add_solidworks_xml_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    for block in &scan.blocks {
        if block.family != "xml" || !block.payload.windows(12).any(|w| w == b"swSolidWorks") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&block.payload) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(text) else {
            continue;
        };
        let root = document.root_element();
        if root.tag_name().name() != "swSolidWorks" {
            continue;
        }
        for (source, target) in [
            ("swVersion", "sw_version"),
            ("swCreationTime", "sw_creation_time_unix"),
            ("swPath", "sw_path"),
        ] {
            if let Some(value) = root.attribute(source) {
                attributes.insert(target.into(), value.into());
            }
        }
        if let Some(model) = root.descendants().find(|node| node.has_tag_name("swModel")) {
            if let Some(value) = model.attribute("swName") {
                attributes.insert("sw_name".into(), value.into());
            }
            if let Some(value) = model.attribute("swConfigurationName") {
                attributes.insert("sw_configuration_name".into(), value.into());
            }
        }
        break;
    }
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.stats;
    let mut losses = Vec::new();

    if s.unknown_surface_faces > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} face(s) rest on a support surface this codec does not type (offset, swept, \
                 blended, intersection, or spline-on-surface); \
                 the face, its loops, and trims are emitted with an unknown-geometry surface \
                 linking to the preserved record bytes. Topology is transferred; the underlying \
                 surface shape is not.",
                s.unknown_surface_faces
            ),
            provenance: None,
        });
    }
    if s.unknown_curve_edges > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            ),
            provenance: None,
        });
    }
    if s.single_sample_carriers > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} cone/torus carrier(s) were decoded from a single observed field layout; the \
                 field order satisfies the analytic relations (sin^2+cos^2=1, major>minor>0) but \
                 has not been cross-checked against a second sample, so treat these carriers as \
                 lower-confidence than the plane/cylinder/sphere set.",
                s.single_sample_carriers
            ),
            provenance: None,
        });
    }
    if s.synthetic_body_grouping {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: "No body record was available; one body/region/shell hierarchy was derived."
                .to_string(),
            provenance: None,
        });
    }
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: "Deltas tombstones are not reconstructed.".to_string(),
        provenance: None,
    });
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: "Stored curve-on-surface families and non-isoparametric NURBS trims are not \
                  reconstructed. Planar lines, cylindrical and spherical analytic trims, and \
                  byte-matching NURBS boundary isocurves receive derived pcurves."
            .to_string(),
        provenance: None,
    });
    losses.push(LossNote {
        category: LossCategory::Material,
        severity: Severity::Warning,
        message: "Conflicting per-face appearance carriers have unresolved override precedence; UnQLite document metadata is preserved but not typed."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "sldprt".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    let histories = crate::history::histories(scan, &mut ir.annotations);
    let lanes = crate::resolved_features::lanes(scan, &mut ir.annotations);
    let model_attributes = crate::metadata::attributes(scan, &mut ir.annotations);
    ir.native
        .sldprt
        .get_or_insert_with(cadmpeg_ir::SldprtNative::default)
        .feature_histories = histories;
    ir.native
        .sldprt
        .get_or_insert_with(cadmpeg_ir::SldprtNative::default)
        .feature_input_lanes = lanes;
    ir.model.attributes = model_attributes;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());

    if let Some((block, header)) = container::select_active_parasolid(scan) {
        attributes.insert(
            "active_parasolid_block".to_string(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
        );
        attributes.insert("parasolid_schema".to_string(), header.schema.clone());
        let id = format!("sldprt:file:block#{}", block.offset);
        crate::annotations::note(
            &mut ir.annotations,
            id.clone(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        ir.unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: block.offset as u64,
            byte_len: block.uncomp_sz as u64,
            sha256: sha256_hex(&block.payload),
            data: Some(block.payload.clone()),
            links: Vec::new(),
        });
    }

    ir.source = Some(SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    });
    preserve_source_image(scan, &mut ir);
    set_semantic_hash(&mut ir);
    ir
}

fn set_semantic_hash(ir: &mut CadIr) {
    let brep_hash = brep_semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("brep_semantic_sha256".into(), brep_hash);
    }
    let hash = semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
    }
}

pub(crate) fn brep_semantic_hash(ir: &CadIr) -> String {
    use cadmpeg_ir::appearance::AppearanceTarget;

    // Normalize with a field-by-field clone so the dropped namespaces (source
    // image, native records, annotations) are never copied.
    let mut normalized = CadIr {
        ir_version: ir.ir_version.clone(),
        source: None,
        units: ir.units.clone(),
        tolerances: ir.tolerances,
        model: ir.model.clone(),
        annotations: Annotations::default(),
        native: cadmpeg_ir::Native::default(),
        unknowns: Vec::new(),
    };
    normalized.model.bodies.iter_mut().for_each(|body| {
        body.name = None;
        body.color = None;
    });
    let face_appearances = normalized
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            matches!(binding.target, AppearanceTarget::Face(_))
                .then_some(binding.appearance.clone())
        })
        .collect::<std::collections::HashSet<_>>();
    normalized
        .model
        .appearance_bindings
        .retain(|binding| matches!(binding.target, AppearanceTarget::Face(_)));
    normalized
        .model
        .appearances
        .retain(|appearance| face_appearances.contains(&appearance.id));
    normalized.model.tessellations.clear();
    normalized.model.attributes.clear();
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

pub(crate) fn semantic_hash(ir: &CadIr) -> String {
    // Normalize with a field-by-field clone so the retained source image (the
    // largest single payload) is filtered out instead of copied and dropped.
    let normalized = CadIr {
        ir_version: ir.ir_version.clone(),
        source: ir.source.as_ref().map(|source| {
            let mut source = source.clone();
            source.attributes.remove("semantic_sha256");
            source
        }),
        units: ir.units.clone(),
        tolerances: ir.tolerances,
        model: ir.model.clone(),
        annotations: ir.annotations.clone(),
        native: ir.native.clone(),
        unknowns: ir
            .unknowns
            .iter()
            .filter(|record| record.id.0 != "sldprt:file:source-image#0")
            .cloned()
            .collect(),
    };
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

fn preserve_source_image(scan: &ContainerScan, ir: &mut CadIr) {
    crate::annotations::note(
        &mut ir.annotations,
        "sldprt:file:source-image#0",
        "source",
        0,
        "source_image",
        Exactness::ByteExact,
    );
    ir.unknowns.push(UnknownRecord {
        id: UnknownId("sldprt:file:source-image#0".into()),
        offset: 0,
        byte_len: scan.source_image.len() as u64,
        sha256: sha256_hex(&scan.source_image),
        data: Some(scan.source_image.clone()),
        links: Vec::new(),
    });
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let parasolid_blocks = scan
        .blocks
        .iter()
        .filter(|b| b.family == "parasolid")
        .count();

    let mut losses = vec![
        LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "Parasolid B-rep geometry was not transferred: no partition/deltas stream resolved \
                 into a topology graph. {} block(s) were CRC-validated and enumerated, {} of them \
                 Parasolid-family.",
                scan.blocks.len(),
                parasolid_blocks
            ),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message:
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                      built for this file."
                    .to_string(),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Material,
            severity: Severity::Warning,
            message: "Materials/appearances, tessellation, and document/feature metadata were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    ];

    if container::select_active_parasolid(scan).is_none() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Error,
            message: "no Parasolid partition/deltas stream was located in the container"
                .to_string(),
            provenance: None,
        });
    }

    DecodeReport {
        format: "sldprt".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary.notes,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}
