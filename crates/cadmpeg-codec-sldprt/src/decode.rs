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

use std::cmp::Reverse;
use std::collections::BTreeMap;

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::be::u32_at as be_u32;
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AppearanceId, UnknownId};
use cadmpeg_ir::le::{i32_at as le_i32, u16_at as le_u16, u32_at as le_u32};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use crate::container::configuration_index;

use crate::brep::{self, Brep};
use crate::container::{self, Block, ContainerScan};
use crate::parasolid::StreamHeader;

struct BodyStream<'a> {
    block: &'a Block,
    payload: &'a [u8],
    header: StreamHeader,
}

struct DecodedBrep {
    selected: usize,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through [`DecodeResult::report`] when a partial result can be represented.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    decode_inner(reader, options)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn decode_inner(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return decode_result(ir, report, annotations, unknowns);
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        if let Some((decoded, report)) = try_decode_brep(&scan, &streams) {
            let (ir, annotations, unknowns) = build_geometry_ir(
                &scan,
                streams[decoded.selected].block,
                &streams[decoded.selected].header,
                decoded.brep,
                &decoded.configuration_bodies,
            )?;
            return decode_result(ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
    let report = build_container_report(&scan, false);
    decode_result(ir, report, annotations, unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..cadmpeg_ir::SourceFidelity::default()
    };
    let source_image = unknowns
        .iter()
        .position(|record| record.id.0 == "sldprt:file:source-image#0")
        .map(|index| unknowns.remove(index));
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", &unknowns)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("sldprt", std::slice::from_ref(&source_image));
    }
    set_semantic_hash(&mut ir);
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
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
) -> Option<(DecodedBrep, DecodeReport)> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        sites.entry(site_key(stream.block)).or_default().push(index);
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
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
        decoded_sites.push((site.clone(), first, score, decoded));
    }
    let selected_site = decoded_sites
        .iter()
        .enumerate()
        .max_by_key(|(index, (_, _, score, _))| (*score, Reverse(*index)))
        .map(|(index, _)| index)?;
    if decoded_sites[selected_site].3.faces.is_empty()
        && decoded_sites[selected_site].3.surfaces.is_empty()
        && decoded_sites[selected_site].3.points.is_empty()
    {
        return None;
    }
    let (_, selected, _, mut decoded) = decoded_sites.swap_remove(selected_site);
    let mut configuration_bodies = Vec::new();
    if let Some(index) = streams[selected]
        .block
        .section
        .as_deref()
        .and_then(configuration_index)
    {
        configuration_bodies.push((
            index,
            decoded.bodies.iter().map(|body| body.id.clone()).collect(),
        ));
    }
    for (site, first, _, mut alternate) in decoded_sites {
        alternate.qualify_ids(&site);
        if let Some(index) = streams[first]
            .block
            .section
            .as_deref()
            .and_then(configuration_index)
        {
            configuration_bodies.push((
                index,
                alternate
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect(),
            ));
        }
        merge_brep(&mut decoded, alternate);
    }
    let report = build_geometry_report(scan, &decoded);
    Some((
        DecodedBrep {
            selected,
            brep: decoded,
            configuration_bodies,
        },
        report,
    ))
}

fn merge_brep(target: &mut Brep, mut source: Brep) {
    let stream_base = target.annotations.streams.len() as u32;
    target
        .annotations
        .streams
        .append(&mut source.annotations.streams);
    for provenance in source.annotations.provenance.values_mut() {
        provenance.stream += stream_base;
    }
    target
        .annotations
        .provenance
        .append(&mut source.annotations.provenance);
    target
        .annotations
        .exactness
        .append(&mut source.annotations.exactness);
    target.bodies.append(&mut source.bodies);
    target.regions.append(&mut source.regions);
    target.shells.append(&mut source.shells);
    target.faces.append(&mut source.faces);
    target.loops.append(&mut source.loops);
    target.coedges.append(&mut source.coedges);
    target.edges.append(&mut source.edges);
    target.vertices.append(&mut source.vertices);
    target.points.append(&mut source.points);
    target.surfaces.append(&mut source.surfaces);
    target.curves.append(&mut source.curves);
    target.pcurves.append(&mut source.pcurves);
    target.unknowns.append(&mut source.unknowns);
    target.face_colors.append(&mut source.face_colors);
    target.stats.unknown_surface_faces += source.stats.unknown_surface_faces;
    target.stats.unknown_curve_edges += source.stats.unknown_curve_edges;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
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
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let materials = crate::appearance::materials(scan);
    let unique_material = materials.len() == 1;
    if let [material] = materials.as_slice() {
        for body in &mut brep.bodies {
            body.color = Some(material.color);
            if body.name.is_none() {
                body.name = Some(material.name.clone());
            }
        }
    }
    ir.source = Some(source_meta(scan, block, header));
    let mut annotations = std::mem::take(&mut brep.annotations);
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
    project_design_history(&mut ir, &histories, &lanes);
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    stamp_parameter_baseline(&mut ir);
    let (mut sketches, sketch_entities, mut sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut annotations);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &histories,
        &lanes,
        &annotations,
    );
    crate::history::bind_unique_sketch_feature(&mut ir.model.features, &sketches);
    crate::resolved_features::project_relation_bindings(
        &mut sketch_constraints,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    stamp_feature_baseline(&mut ir);
    let attributes = crate::metadata::attributes(scan, &mut annotations);
    let mut native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    ir.model.attributes = attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    stamp_sketch_baseline(&mut ir, &native);

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
    crate::history::bind_topology_selections(
        &mut ir.model.features,
        &histories,
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.edges,
        &ir.model.curves,
    );
    stamp_feature_baseline(&mut ir);
    assign_configuration_bodies(&mut ir, configuration_bodies);
    mark_active_configuration(&mut ir);
    assign_native_configuration_indices(&ir, &mut native);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(&native.feature_histories),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(&native.feature_histories),
        );
    }
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_configuration_baseline(&mut ir);
    let mut unknowns = brep.unknowns;
    for face_color in brep.face_colors {
        let id = AppearanceId(format!(
            "sldprt:appearance:entity53#{}",
            face_color.color_attr
        ));
        crate::annotations::note(
            &mut annotations,
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
            let site = target
                .split_once('@')
                .map(|(_, site)| format!("@{site}"))
                .unwrap_or_default();
            let binding_id = format!(
                "sldprt:appearance:binding#face:{}:{}{}",
                face_color.face_attr, face_color.color_attr, site
            );
            if !ir
                .model
                .appearance_bindings
                .iter()
                .any(|binding| binding.id == binding_id)
            {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: binding_id,
                    target: AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(target)),
                    appearance: id,
                    source_entity_id: Some(face_color.face_attr.to_string()),
                    object_type: Some("Face".into()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for (index, material) in materials.into_iter().enumerate() {
        let id = AppearanceId(format!("sldprt:appearance:material#{index}"));
        let material_stream = format!("block@{}", material.block_offset);
        crate::annotations::note(
            &mut annotations,
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
        if unique_material {
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
                &mut annotations,
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
                    body: None,
                    faces: Vec::new(),
                    chordal_deflection: None,
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
            &mut annotations,
            display_id.clone(),
            display
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", display.offset)),
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(display_id),
            offset: display.offset as u64,
            byte_len: display.uncomp_sz as u64,
            sha256: sha256_hex(&display.payload),
            data: Some(display.payload.clone()),
            links: Vec::new(),
        });
    }
    for source_block in &scan.blocks {
        if unknowns
            .iter()
            .any(|record| record.id.0 == format!("sldprt:file:block#{}", source_block.offset))
        {
            continue;
        }
        let id = format!("sldprt:file:block#{}", source_block.offset);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            source_block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", source_block.offset)),
            source_block.offset as u64,
            source_block.family,
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord {
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
        let partition = unknowns
            .iter_mut()
            .find(|record| record.id == partition_id)
            .expect("active partition block is retained");
        partition.links.extend(opaque_surfaces);
        partition.links.extend(opaque_curves);
    }
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    set_semantic_hash(&mut ir);
    Ok((ir, annotations, unknowns))
}

fn assign_native_configuration_indices(ir: &CadIr, native: &mut crate::native::SldprtNative) {
    for configuration in &ir.model.configurations {
        let Some(native_ref) = configuration.native_ref.as_deref() else {
            continue;
        };
        if let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|record| record.id == native_ref)
        {
            record.source_index = configuration.source_index;
        }
    }
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
    if s.synthetic_body_grouping {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: "No body record was available; one body/region/shell hierarchy was derived."
                .to_string(),
            provenance: None,
        });
    }
    DecodeReport {
        format: "sldprt".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(
    scan: &ContainerScan,
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
    let (sketches, sketch_entities, sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut annotations);
    let model_attributes = crate::metadata::attributes(scan, &mut annotations);
    ir.model.attributes = model_attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    add_solidworks_xml_metadata(scan, &mut attributes);

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
            &mut annotations,
            id.clone(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
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
    project_design_history(&mut ir, &histories, &lanes);
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    stamp_parameter_baseline(&mut ir);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &histories,
        &lanes,
        &annotations,
    );
    crate::history::bind_unique_sketch_feature(&mut ir.model.features, &ir.model.sketches);
    crate::resolved_features::project_relation_bindings(
        &mut ir.model.sketch_constraints,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    stamp_feature_baseline(&mut ir);
    let native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native);
    mark_active_configuration(&mut ir);
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    set_semantic_hash(&mut ir);
    Ok((ir, annotations, unknowns))
}

fn project_design_history(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    let mut projection = histories.to_vec();
    crate::resolved_features::enrich_history_parameters(&mut projection, lanes);
    ir.model.features = crate::history::project_features(&projection);
    ir.model.configurations = crate::history::project_configurations(&projection);
    ir.model.parameters = crate::history::project_parameters(&projection);
    crate::resolved_features::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        histories,
        lanes,
    );
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_neutral_feature_sha256".into(),
            crate::history::feature_hash(&ir.model.features),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(histories),
        );
        source.attributes.insert(
            "sldprt_neutral_configuration_sha256".into(),
            crate::history::configuration_hash(&ir.model.configurations),
        );
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(histories),
        );
        source.attributes.insert(
            "sldprt_neutral_parameter_sha256".into(),
            crate::history::parameter_hash(&ir.model.parameters),
        );
        source.attributes.insert(
            "sldprt_native_parameter_sha256".into(),
            crate::history::native_parameter_hash(histories),
        );
    }
}

fn stamp_parameter_baseline(ir: &mut CadIr) {
    let hash = crate::history::parameter_hash(&ir.model.parameters);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_parameter_sha256".into(), hash);
    }
}

fn mark_active_configuration(ir: &mut CadIr) {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"))
        .cloned();
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section));
    for configuration in &mut ir.model.configurations {
        configuration.active = active_name.as_ref() == Some(&configuration.name)
            || active_name.is_none()
                && active_index.is_some_and(|index| {
                    configuration.source_index == u32::try_from(index).ok()
                        || configuration.source_index.is_none()
                            && configuration.ordinal == u32::try_from(index).unwrap_or(u32::MAX)
                });
    }
}

fn stamp_feature_baseline(ir: &mut CadIr) {
    let hash = crate::history::feature_hash(&ir.model.features);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_feature_sha256".into(), hash);
    }
}

fn assign_configuration_bodies(
    ir: &mut CadIr,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) {
    let mut partitions = configuration_bodies
        .iter()
        .filter_map(|(index, bodies)| {
            u32::try_from(*index)
                .ok()
                .map(|index| (index, bodies.clone()))
        })
        .collect::<Vec<_>>();
    partitions.sort_by_key(|(index, _)| *index);
    let mut configurations = (0..ir.model.configurations.len()).collect::<Vec<_>>();
    configurations.sort_by_key(|position| ir.model.configurations[*position].ordinal);
    if configurations.len() == partitions.len() {
        for (position, (source_index, bodies)) in configurations.into_iter().zip(partitions) {
            let configuration = &mut ir.model.configurations[position];
            configuration.source_index = Some(source_index);
            configuration.bodies = bodies;
        }
        return;
    }
    for (source_index, bodies) in partitions {
        if let Some(configuration) = ir.model.configurations.iter_mut().find(|configuration| {
            configuration.source_index == Some(source_index)
                || configuration.source_index.is_none() && configuration.ordinal == source_index
        }) {
            configuration.source_index = Some(source_index);
            configuration.bodies = bodies;
            continue;
        }
        let ordinal = ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        ir.model
            .configurations
            .push(cadmpeg_ir::features::DesignConfiguration {
                id: cadmpeg_ir::features::ConfigurationId(format!(
                    "sldprt:model:configuration#partition:{source_index}"
                )),
                ordinal,
                active: false,
                source_index: Some(source_index),
                name: format!("Config-{source_index}"),
                material: None,
                properties: std::collections::BTreeMap::new(),
                bodies,
                native_ref: None,
            });
    }
}

fn stamp_configuration_baseline(ir: &mut CadIr) {
    let hash = crate::history::configuration_hash(&ir.model.configurations);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_configuration_sha256".into(), hash);
    }
}

fn stamp_sketch_baseline(ir: &mut CadIr, native: &crate::native::SldprtNative) {
    let neutral_hash = crate::resolved_features::sketch_hash(ir);
    let constraint_hash = crate::resolved_features::constraint_hash(ir);
    let native_hash = crate::resolved_features::lane_hash(native);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_sketch_sha256".into(), neutral_hash);
        source
            .attributes
            .insert("sldprt_native_sketch_sha256".into(), native_hash);
        source.attributes.insert(
            "sldprt_neutral_sketch_constraint_sha256".into(),
            constraint_hash,
        );
    }
}

fn set_semantic_hash(ir: &mut CadIr) {
    ir.finalize();
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
        native: cadmpeg_ir::Native::default(),
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
    normalized.model.features.clear();
    normalized.model.parameters.clear();
    normalized.model.sketches.clear();
    normalized.model.sketch_entities.clear();
    normalized.model.sketch_constraints.clear();
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
    let mut normalized = ir.clone();
    normalized.finalize();
    normalized.source = ir.source.as_ref().map(|source| {
        let mut source = source.clone();
        source.attributes.remove("semantic_sha256");
        source
    });
    let unknowns = ir
        .native_unknowns("sldprt")
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.id.0 != "sldprt:file:source-image#0")
        .collect::<Vec<_>>();
    normalized
        .set_native_unknowns("sldprt", &unknowns)
        .expect("SLDPRT unknown records serialize");
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

fn preserve_source_image(
    scan: &ContainerScan,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) {
    crate::annotations::note(
        annotations,
        "sldprt:file:source-image#0",
        "source",
        0,
        "source_image",
        Exactness::ByteExact,
    );
    unknowns.push(UnknownRecord {
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
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: summary.notes,
    }
}
