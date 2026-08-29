// SPDX-License-Identifier: Apache-2.0
//! Semantic SLDPRT writer for analytic B-reps.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;

pub(crate) mod target;

use crate::native::SldprtNative;
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry,
};
use cadmpeg_ir::topology::{BodyKind, Color, Sense};
use cadmpeg_ir::Annotations;

use crate::SourceRecord;

use crate::container::MARKER;

const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];
const TYPED_BODY_LINEAR_RESOLUTION: f64 = 1.0e-8;

pub(crate) const SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE: &str = "sldprt_swobjects_local_sha256";
pub(crate) const SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE: &str =
    "sldprt_swobjects_material_local_sha256";
pub(crate) const SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE: &str =
    "sldprt_swobjects_metadata_identity_local_sha256";
pub(crate) const PMI_LOCAL_DIGEST_ATTRIBUTE: &str = "sldprt_pmi_local_sha256";

/// Writes a semantic document and returns the dialect it wrote.
///
/// The returned id is read back out of the `swSolidWorks` envelope this call
/// emits, whether that envelope survived from the retained blocks or was
/// generated here, so a re-decode of these bytes classifies exactly what the
/// caller reports as `ExportReport::target`.
pub(crate) fn write_semantic_with_records(
    ir: &CadIr,
    annotations: &Annotations,
    retained_records: &[SourceRecord<'_>],
    writer: &mut dyn Write,
) -> Result<cadmpeg_core::dialect::DialectId, CodecError> {
    let mut native = ir
        .native
        .namespace("sldprt")
        .map(|namespace| {
            if !crate::native::native_version_supported(namespace.version) {
                let version = namespace.version;
                return Err(CodecError::malformed(format_args!(
                    "unsupported SLDPRT native namespace version {version}"
                )));
            }
            SldprtNative::load(namespace).map_err(Into::into)
        })
        .transpose()?;
    let mut normalized = ir.clone();
    drop_synthesized_configuration_snapshot(&mut normalized);
    sort_arenas(&mut normalized);
    // Export precondition on the normalized input. Keeps full validate_neutral:
    // writer refusal depends on non-core Checks (e.g. Counts for duplicate
    // configuration source indices). SLDPRT_EXPORT_PRECONDITION_CHECKS records
    // the draft/topology floor; narrowing further needs reject-fixture coverage.
    // The postcondition after bake/prepare (below) also keeps full validate_neutral.
    let validation = cadmpeg_ir::validate::validate_neutral(&normalized, Vec::new());
    if !validation.is_ok() {
        let detail = validation
            .findings
            .iter()
            .find(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
            .map_or("IR validation failed", |finding| finding.message.as_str());
        return Err(CodecError::Malformed(detail.into()));
    }
    crate::writer_transform::bake(&mut normalized)?;
    sort_arenas(&mut normalized);
    assign_configuration_indices(&mut normalized.model.configurations)?;
    let retained_partition = retained_partition(&normalized, retained_records);
    let feature_name_changes = crate::history::feature_name_changes(&normalized, native.as_ref());
    let feature_parameter_changes_authorized = !feature_name_changes.is_empty()
        && crate::history::native_parameters_match_source(&normalized, native.as_ref());
    crate::history::apply_feature_name_changes(
        &mut normalized.model.parameters,
        &feature_name_changes,
    );
    let ir = &normalized;
    crate::history::prepare_features_for_write(ir, &mut native)?;
    crate::resolved_features::write_prepare::prepare_sketches_for_write(ir, &mut native)?;
    crate::history::prepare_parameters_for_write(
        ir,
        &mut native,
        feature_parameter_changes_authorized,
    )?;
    crate::history::prepare_configurations_for_write(ir, &mut native, annotations)?;
    let validation = cadmpeg_ir::validate::validate_neutral(ir, Vec::new());
    if !validation.is_ok() {
        let detail = validation
            .findings
            .iter()
            .find(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
            .map_or("transformed IR validation failed", |finding| {
                finding.message.as_str()
            });
        return Err(CodecError::Malformed(detail.into()));
    }
    check_semantic_support(ir, annotations)?;
    if ir.model.faces.is_empty() {
        return Err(CodecError::NotImplemented(
            "semantic SLDPRT writing requires a B-rep".into(),
        ));
    }
    // The IR stores canonical millimetres; Parasolid stores metres.
    let length_scale = 0.001;
    let patched_partition = if retained_partition.is_none() {
        crate::writer_patch::patch_partition(ir, annotations, retained_records, length_scale)?
    } else {
        None
    };
    let retain_native_brep = retained_partition.is_some() || patched_partition.is_some();
    let partition_sections = if let Some(retained) = retained_partition {
        vec![retained]
    } else if let Some(patched) = patched_partition {
        vec![patched]
    } else if !ir.model.configurations.is_empty() {
        configuration_partitions(ir, length_scale)?
    } else {
        let schema_32001 = ir
            .model
            .bodies
            .iter()
            .any(|body| body.kind == BodyKind::Sheet);
        let body = brep_body(ir, length_scale, schema_32001)?;
        let schema = if schema_32001 {
            "SCH_SW_32001_11000"
        } else {
            "SCH_SW_33103_11000"
        };
        vec![(
            "Contents/Config-0-Partition".to_string(),
            parasolid_stream(&body, schema),
        )]
    };
    let active_partition_section = partition_sections
        .first()
        .map(|(section, _)| section.clone())
        .unwrap_or_default();
    let mut sections = partition_sections;
    let materials = materials_payload(ir)?;
    let (objects, units) = metadata_payloads(ir, length_scale)?;
    let mut retained_swobjects = retained_swobjects_sections(retained_records, annotations)?;
    let replay_swobjects = if retained_swobjects.is_empty() {
        false
    } else {
        let baseline = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE));
        let current = swobjects_local_sha256(ir)?;
        if baseline != Some(&current) {
            let material_baseline = ir.source.as_ref().and_then(|source| {
                source
                    .attributes
                    .get(SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE)
            });
            let identity_baseline = ir.source.as_ref().and_then(|source| {
                source
                    .attributes
                    .get(SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE)
            });
            if material_baseline != Some(&swobjects_material_local_sha256(ir)?)
                || identity_baseline != Some(&swobjects_metadata_identity_local_sha256(ir))
            {
                return Err(CodecError::NotImplemented(
                    "SLDPRT writer cannot edit retained SWObjects semantics without replacing opaque record bytes"
                        .into(),
                ));
            }
            patch_retained_swobjects_metadata(
                ir,
                annotations,
                &mut retained_swobjects,
                length_scale,
            )?;
        }
        true
    };
    if replay_swobjects {
        sections.extend(
            retained_swobjects
                .into_iter()
                .map(|(_, section, payload)| (section, payload)),
        );
    } else {
        if !materials.is_empty() {
            sections.push(("SWObjects".into(), materials));
        }
        if !objects.is_empty() {
            sections.push(("SWObjects/DocumentMetadata".into(), objects));
        }
    }
    if let Some(units) = units {
        sections.push(("Units".into(), units));
    }
    if !ir.model.tessellations.is_empty() {
        sections.push((
            "Contents/DisplayLists".into(),
            tessellation_payload(ir, length_scale)?,
        ));
    }
    for (index, history) in native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .enumerate()
    {
        let section = annotations
            .provenance
            .get(&history.id)
            .and_then(|provenance| annotations.streams.get(provenance.stream as usize))
            .cloned()
            .unwrap_or_else(|| format!("Contents/Keywords-{index}"));
        sections.push((section, history_payload(history)?));
    }
    for lane in native.iter().flat_map(|native| &native.feature_input_lanes) {
        let section = lane.configuration.as_ref().map_or_else(
            || {
                annotations
                    .provenance
                    .get(&lane.id)
                    .and_then(|provenance| {
                        annotations.streams.get(provenance.stream as usize).cloned()
                    })
                    .unwrap_or_else(|| "Contents/ResolvedFeatures".into())
            },
            |configuration| format!("Contents/Config-{configuration}-ResolvedFeatures"),
        );
        let histories = native
            .as_ref()
            .map_or(&[][..], |native| native.feature_histories.as_slice());
        sections.push((section, resolved_feature_payload(lane, histories)?));
    }
    let opaque = opaque_blocks(
        ir,
        retained_records,
        annotations,
        &active_partition_section,
        retain_native_brep,
    )?;
    if let Some(active) = ir
        .model
        .configurations
        .iter()
        .find(|value| value.active.is_active())
    {
        let active_name = active.name.resolved().ok_or_else(|| {
            CodecError::Malformed("active SLDPRT configuration has no resolved name".into())
        })?;
        let has_document_envelope = opaque
            .iter()
            .any(|(_, payload)| payload.windows(12).any(|window| window == b"swSolidWorks"));
        if !has_document_envelope {
            sections.push((
                "Contents/SolidWorks".into(),
                generated_solidworks_xml(ir, active_name),
            ));
        }
    }
    for (section, payload) in opaque {
        sections.push((section, payload));
    }

    let type_ids = section_type_ids(retained_records, &sections)?;
    writer.write_all(&outer_header(retained_records))?;
    for ((section, payload), type_id) in sections.iter().zip(&type_ids) {
        writer.write_all(&block(payload, section, *type_id)?)?;
    }
    for cell in retained_cache_cells(retained_records, &sections) {
        writer.write_all(&cell)?;
    }
    for entry in section_directory_entries(retained_records, &sections, &type_ids)? {
        writer.write_all(&entry)?;
    }
    // The identity claim, read out of the bytes just written rather than
    // inferred from which branch produced them. A retained envelope carries the
    // source's own `swVersion` through unchanged; a generated one declares
    // none; a document with no active configuration and no retained envelope
    // carries no envelope at all. The last two classify onto the totality row,
    // which is the whole synthesis catalog. The decoder's first-envelope fold
    // classifies the written bytes here and on re-decode.
    Ok(crate::dialect::SldprtDialect::from_declaration(
        crate::container::first_solidworks_envelope(
            sections.iter().map(|(_, payload)| payload.as_slice()),
        )
        .and_then(|envelope| envelope.sw_version)
        .as_deref(),
    )
    .id())
}

fn assign_configuration_indices(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
) -> Result<(), CodecError> {
    let mut used = HashSet::new();
    let mut names = HashSet::new();
    for configuration in configurations.iter() {
        let Some(name) = configuration.name.resolved() else {
            return Err(CodecError::Malformed(
                "SLDPRT configuration has no resolved name".into(),
            ));
        };
        if name.trim().is_empty() {
            return Err(CodecError::Malformed(
                "SLDPRT configuration has an empty name".into(),
            ));
        }
        if !names.insert(name) {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT repeats configuration name {name:?}"
            )));
        }
        if let Some(index) = configuration.source_index {
            if !used.insert(index) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate SLDPRT configuration source index {index}"
                )));
            }
        }
    }
    let mut positions = (0..configurations.len()).collect::<Vec<_>>();
    positions.sort_by_key(|position| configurations[*position].ordinal);
    let mut next = 0;
    for position in positions {
        if configurations[position].source_index.is_none()
            && configurations[position].bodies.resolved().is_some()
        {
            configurations[position].source_index =
                Some(reserve_configuration_index(&mut used, &mut next)?);
        }
    }
    Ok(())
}

fn generated_solidworks_xml(ir: &CadIr, active: &str) -> Vec<u8> {
    let model = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_name"))
        .map_or("", String::as_str);
    let mut output = String::from("<?xml version=\"1.0\"?><swSolidWorks><swModel swName=\"");
    push_xml_attribute_value(&mut output, model);
    output.push_str("\" swConfigurationName=\"");
    push_xml_attribute_value(&mut output, active);
    output.push_str("\"/></swSolidWorks>");
    output.into_bytes()
}

fn push_xml_attribute_value(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

/// Drop the design-state snapshot the decoder fabricates on the active
/// configuration when the native records carry none.
///
/// Nothing in the file encodes that snapshot, so it cannot be written back.
fn drop_synthesized_configuration_snapshot(ir: &mut CadIr) {
    let Some(id) = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_snapshot_synthesized")
            .cloned()
    }) else {
        return;
    };
    for configuration in &mut ir.model.configurations {
        if configuration.id.0 == id {
            configuration.feature_states.clear();
            configuration.parameter_values.clear();
        }
    }
}

fn sort_arenas(ir: &mut CadIr) {
    ir.model.bodies.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.regions.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.shells.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.faces.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.loops.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.coedges.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.edges.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.vertices.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.points.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.curves.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.subds.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.pcurves.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.procedural_surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.procedural_curves.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.tessellations.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.appearances.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.attributes.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model
        .appearance_bindings
        .sort_by_key(|binding| format!("{:?}:{}", binding.target, binding.appearance.0));
}

fn source_image<'a>(records: &[SourceRecord<'a>]) -> Option<&'a [u8]> {
    records
        .iter()
        .find(|record| record.id.0 == "sldprt:file:source-image#0")?
        .data
}

fn section_directory_entries(
    records: &[SourceRecord<'_>],
    sections: &[(String, Vec<u8>)],
    type_ids: &[u32],
) -> Result<Vec<Vec<u8>>, CodecError> {
    let source = source_image(records);
    let source_scan = source.map(crate::container::scan_bytes);
    let source_trailer = if let Some(scan) = source_scan.as_ref() {
        let mut trailers = scan.directory.iter().map(|entry| entry.trailer);
        if let Some(first) = trailers.next() {
            if trailers.any(|trailer| trailer != first) {
                return Err(CodecError::Malformed(
                    "SLDPRT source directory has inconsistent entry trailers".into(),
                ));
            }
            Some(first)
        } else {
            None
        }
    } else {
        None
    };
    sections
        .iter()
        .zip(type_ids)
        .map(|((section, payload), type_id)| {
            let size = u32::try_from(payload.len())
                .map_err(|_| CodecError::Malformed("SLDPRT section exceeds 4 GiB".into()))?;
            let source_entry = source_scan.as_ref().and_then(|scan| {
                scan.directory
                    .iter()
                    .find(|entry| entry.name == *section && entry.type_id == *type_id)
            });
            if let (Some(entry), Some(source)) =
                (source_entry.filter(|entry| entry.size == size), source)
            {
                let end = entry
                    .offset
                    .checked_add(46 + entry.name.len())
                    .ok_or_else(|| {
                        CodecError::Malformed("SLDPRT directory entry extent overflow".into())
                    })?;
                if let Some(retained) = source.get(entry.offset..end) {
                    return Ok(retained.to_vec());
                }
            }
            Ok(directory_entry(
                *type_id,
                size,
                section,
                source_entry.map_or([0; 14], |entry| entry.descriptor),
                source_trailer.unwrap_or([0xe5, 0x4b, 0x57, 0x5b, 0, 0]),
            ))
        })
        .collect()
}

fn retained_cache_cells(
    records: &[SourceRecord<'_>],
    sections: &[(String, Vec<u8>)],
) -> Vec<Vec<u8>> {
    let Some(source) = source_image(records) else {
        return Vec::new();
    };
    let scan = crate::container::scan_bytes(source);
    scan.cache_cells
        .iter()
        .filter(|cell| {
            let original_matches = scan.blocks.iter().any(|block| {
                block.section.as_deref() == Some(cell.name.as_str())
                    && sections.iter().any(|(section, payload)| {
                        section == &cell.name && payload == &block.payload
                    })
            });
            original_matches
        })
        .filter_map(|cell| {
            let end = cell.offset.checked_add(26 + cell.name.len())?;
            source.get(cell.offset..end).map(<[u8]>::to_vec)
        })
        .collect()
}

/// The retained Parasolid partition to replay, when the decoded B-rep is
/// untouched since the decode that recorded its baseline.
///
/// The comparison is bitwise against the `brep_local_sha256` baseline, which is
/// what an edit-detection question needs and all it can be; see
/// [`crate::decode::brep_local_sha256`]. A document without the baseline — one
/// synthesized rather than decoded, or decoded by a binary that named the
/// attribute differently — has no partition to replay and is written out in full.
fn retained_partition(ir: &CadIr, records: &[SourceRecord<'_>]) -> Option<(String, Vec<u8>)> {
    let source = ir.source.as_ref()?;
    let expected = source.attributes.get("brep_local_sha256")?;
    if crate::decode::brep_local_sha256(ir) != *expected {
        return None;
    }
    let source_image = source_image(records)?;
    let scan = crate::container::scan_bytes(source_image);
    let (block, _) = crate::container::select_active_parasolid(&scan)?;
    let original_section = block
        .section
        .clone()
        .unwrap_or_else(|| format!("block@{}", block.offset));
    let section = remapped_partition_section(ir, &original_section).unwrap_or(original_section);
    Some((section, block.payload.clone()))
}

fn remapped_partition_section(ir: &CadIr, section: &str) -> Option<String> {
    let old_index = crate::container::configuration_index(section)?;
    let native = SldprtNative::load(ir.native.namespace("sldprt")?).ok()?;
    let native_id = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.configurations)
        .find(|configuration| configuration.source_index == u32::try_from(old_index).ok())?
        .id
        .as_str();
    let new_index = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.native_ref.as_deref() == Some(native_id))?
        .source_index?;
    Some(format!("Contents/Config-{new_index}-Partition"))
}

fn outer_header(records: &[SourceRecord<'_>]) -> [u8; 8] {
    source_image(records)
        .and_then(|source| source.get(..8))
        .and_then(|header| header.try_into().ok())
        .unwrap_or_else(|| {
            let mut header = [0; 8];
            header[..4].copy_from_slice(&1u32.to_le_bytes());
            header[4..].copy_from_slice(&4u32.to_be_bytes());
            header
        })
}

fn section_type_ids(
    records: &[SourceRecord<'_>],
    sections: &[(String, Vec<u8>)],
) -> Result<Vec<u32>, CodecError> {
    let mut source_ids: HashMap<String, VecDeque<u32>> = HashMap::new();
    if let Some(source) = source_image(records) {
        for block in crate::container::scan_bytes(source).blocks {
            if let Some(section) = block.section {
                source_ids
                    .entry(section)
                    .or_default()
                    .push_back(block.type_id);
            }
        }
    }
    sections
        .iter()
        .enumerate()
        .map(|(index, (section, _))| {
            source_ids
                .get_mut(section)
                .and_then(VecDeque::pop_front)
                .map_or_else(
                    || {
                        0x20u32
                            .checked_add(u32::try_from(index).map_err(|_| {
                                CodecError::Malformed(
                                    "SLDPRT section count exceeds type-id space".into(),
                                )
                            })?)
                            .ok_or_else(|| CodecError::Malformed("SLDPRT type-id overflow".into()))
                    },
                    Ok,
                )
        })
        .collect()
}

fn check_semantic_support(ir: &CadIr, annotations: &Annotations) -> Result<(), CodecError> {
    let pmi_baseline = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(PMI_LOCAL_DIGEST_ATTRIBUTE));
    if (pmi_baseline.is_some() || !ir.model.pmi.is_empty())
        && pmi_baseline != Some(&pmi_local_sha256(ir)?)
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT writer cannot edit retained SWIFT semantic PMI".into(),
        ));
    }
    if ir.source.as_ref().is_some_and(|source| {
        source
            .attributes
            .contains_key("sldprt_active_partition_unresolved")
    }) && ir
        .model
        .configurations
        .iter()
        .all(|configuration| !configuration.active.is_active())
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT semantic writing requires an active partition identity for a multi-partition source"
                .into(),
        ));
    }
    if !ir.model.configurations.is_empty()
        && ir
            .model
            .configurations
            .iter()
            .filter(|configuration| configuration.active.is_active())
            .count()
            != 1
    {
        return Err(CodecError::Malformed(
            "SLDPRT writing requires exactly one active configuration".into(),
        ));
    }
    if !ir.model.subds.is_empty() {
        return Err(CodecError::NotImplemented(
            "SLDPRT semantic writer does not support SubD surfaces".into(),
        ));
    }
    for surface in &ir.model.surfaces {
        match &surface.geometry {
            SurfaceGeometry::Cone {
                ratio, half_angle, ..
            } => {
                if *ratio != 1.0 {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT surface {} has elliptical cone ratio {}; compact cone carriers encode circular cones only",
                        surface.id.0, ratio
                    )));
                }
                if !(*half_angle > 0.0 && *half_angle < std::f64::consts::FRAC_PI_2) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT surface {} has cone half-angle {}; compact cone carriers require an acute positive half-angle",
                        surface.id.0, half_angle
                    )));
                }
            }
            SurfaceGeometry::Sphere { radius, .. } if *radius < 0.0 => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT surface {} has signed sphere radius {}; compact sphere carriers require a positive radius",
                    surface.id.0, radius
                )));
            }
            SurfaceGeometry::Torus {
                major_radius,
                minor_radius,
                ..
            } if !(*major_radius > *minor_radius && *minor_radius > 0.0) => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT surface {} has torus radii ({}, {}); compact torus carriers require major > minor > 0",
                    surface.id.0, major_radius, minor_radius
                )));
            }
            _ => {}
        }
    }
    for body in &ir.model.bodies {
        if body.name.is_some() && body.color.is_none() {
            return Err(CodecError::NotImplemented(
                "SLDPRT semantic writer cannot encode a body name without a material".into(),
            ));
        }
    }
    if ir.model.faces.iter().any(|face| face.name.is_some()) {
        return Err(CodecError::NotImplemented(
            "SLDPRT semantic writer does not encode face names".into(),
        ));
    }
    if ir.model.edges.iter().any(|edge| {
        edge.param_range.is_some()
            && annotations
                .exactness
                .get(&edge.id.0)
                .is_none_or(|note| note.entity != cadmpeg_ir::Exactness::Derived)
    }) {
        return Err(CodecError::NotImplemented(
            "SLDPRT semantic writer does not encode explicit edge parameter ranges".into(),
        ));
    }
    for appearance in &ir.model.appearances {
        if appearance.asset_guid.is_some()
            || appearance.visual_guid.is_some()
            || appearance.physical_token.is_some()
            || appearance.category.is_some()
            || !appearance.properties.is_empty()
        {
            return Err(CodecError::NotImplemented(
                "SLDPRT semantic writer supports appearance names and base colors only".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn pmi_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let bytes = serde_json::to_vec(&ir.model.pmi)
        .map_err(|error| CodecError::malformed(format_args!("cannot hash SLDPRT PMI: {error}")))?;
    Ok(cadmpeg_ir::hash::sha256_hex(&bytes))
}

fn configuration_partitions(
    ir: &CadIr,
    length_scale: f64,
) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    let configured = ir
        .model
        .configurations
        .iter()
        .flat_map(|configuration| {
            configuration
                .bodies
                .resolved()
                .unwrap_or_default()
                .iter()
                .cloned()
        })
        .collect::<HashSet<_>>();
    if let Some(body) = ir
        .model
        .bodies
        .iter()
        .find(|body| !configured.contains(&body.id))
    {
        return Err(CodecError::malformed(format_args!(
            "SLDPRT body {} belongs to no configuration",
            body.id.0
        )));
    }
    let mut configurations = ir.model.configurations.iter().collect::<Vec<_>>();
    configurations.sort_by_key(|configuration| configuration.ordinal);
    configurations
        .into_iter()
        .filter_map(|configuration| {
            let bodies = configuration.bodies.resolved()?;
            (!bodies.is_empty()).then_some((configuration, bodies))
        })
        .map(|(configuration, bodies)| {
            let index = configuration.source_index.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT configuration {} has no assigned source index",
                    configuration.id.0
                ))
            })?;
            let subset = body_subset(ir, bodies)?;
            let schema_32001 = subset
                .model
                .bodies
                .iter()
                .any(|body| body.kind == BodyKind::Sheet);
            let body = brep_body(&subset, length_scale, schema_32001)?;
            let schema = if schema_32001 {
                "SCH_SW_32001_11000"
            } else {
                "SCH_SW_33103_11000"
            };
            Ok((
                format!("Contents/Config-{index}-Partition"),
                parasolid_stream(&body, schema),
            ))
        })
        .collect()
}

pub(super) fn reserve_configuration_index(
    used: &mut HashSet<u32>,
    next: &mut u32,
) -> Result<u32, CodecError> {
    loop {
        let index = *next;
        if used.insert(index) {
            if let Some(successor) = index.checked_add(1) {
                *next = successor;
            }
            return Ok(index);
        }
        *next = index.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("SLDPRT configuration source index space is exhausted".into())
        })?;
    }
}

fn body_subset(ir: &CadIr, selected: &[cadmpeg_ir::ids::BodyId]) -> Result<CadIr, CodecError> {
    let selected = selected.iter().cloned().collect::<HashSet<_>>();
    if let Some(id) = selected
        .iter()
        .find(|id| !ir.model.bodies.iter().any(|body| &body.id == *id))
    {
        return Err(CodecError::malformed(format_args!(
            "configuration references missing body {}",
            id.0
        )));
    }
    let mut subset = ir.clone();
    subset
        .model
        .bodies
        .retain(|body| selected.contains(&body.id));
    let regions = subset
        .model
        .bodies
        .iter()
        .flat_map(|body| body.regions.iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .model
        .regions
        .retain(|region| regions.contains(&region.id));
    let shells = subset
        .model
        .regions
        .iter()
        .flat_map(|region| region.shells.iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .model
        .shells
        .retain(|shell| shells.contains(&shell.id));
    let faces = subset
        .model
        .shells
        .iter()
        .flat_map(|shell| shell.faces.iter().cloned())
        .collect::<HashSet<_>>();
    subset.model.faces.retain(|face| faces.contains(&face.id));
    let loops = subset
        .model
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().cloned())
        .collect::<HashSet<_>>();
    subset.model.loops.retain(|loop_| loops.contains(&loop_.id));
    let coedges = subset
        .model
        .loops
        .iter()
        .flat_map(|loop_| loop_.coedges.iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .model
        .coedges
        .retain(|coedge| coedges.contains(&coedge.id));
    let mut edges = subset
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<HashSet<_>>();
    for shell in &subset.model.shells {
        edges.extend(shell.wire_edges.iter().cloned());
    }
    subset.model.edges.retain(|edge| edges.contains(&edge.id));
    let mut vertices = subset
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .collect::<HashSet<_>>();
    for shell in &subset.model.shells {
        vertices.extend(shell.free_vertices.iter().cloned());
    }
    subset
        .model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points = subset
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<HashSet<_>>();
    subset
        .model
        .points
        .retain(|point| points.contains(&point.id));
    let surfaces = subset
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect::<HashSet<_>>();
    subset
        .model
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    let curves = subset
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect::<HashSet<_>>();
    subset
        .model
        .curves
        .retain(|curve| curves.contains(&curve.id));
    let pcurves = subset
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.clone()))
        .collect::<HashSet<_>>();
    subset
        .model
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
    subset.model.finalize();
    Ok(subset)
}

fn opaque_blocks(
    ir: &CadIr,
    records: &[SourceRecord<'_>],
    annotations: &Annotations,
    active_partition: &str,
    retain_native_brep: bool,
) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    let mut seen = HashSet::new();
    records
        .iter()
        .filter(|record| record.id.0.starts_with("sldprt:file:block#"))
        .filter_map(|record| {
            let provenance = annotations.provenance.get(&record.id.0)?;
            let section = annotations
                .streams
                .get(usize::try_from(provenance.stream).ok()?)?
                .as_str();
            let lower = section.to_ascii_lowercase();
            if section == active_partition {
                return None;
            }
            if lower.ends_with("-partition")
                && remapped_partition_section(ir, section).as_deref() == Some(active_partition)
            {
                return None;
            }
            if lower.contains("deltas") && !retain_native_brep {
                return None;
            }
            if [
                "swobjects",
                "displaylists",
                "keywords",
                "units",
                "resolvedfeatures",
            ]
            .iter()
            .any(|token| lower.contains(token))
            {
                return None;
            }
            let mut payload = record.data?.to_vec();
            if lower.contains("pmisemanticdatadb") {
                if let Err(error) = crate::pmi::patch_payload(ir, &record.id.0, &mut payload) {
                    return Some(Err(error));
                }
            }
            if let Some(active) = ir
                .model
                .configurations
                .iter()
                .find(|value| value.active.is_active())
            {
                let Some(active_name) = active.name.resolved() else {
                    return Some(Err(CodecError::Malformed(
                        "active SLDPRT configuration has no resolved name".into(),
                    )));
                };
                match patch_active_configuration_xml(&payload, active_name) {
                    Ok(Some(patched)) => payload = patched,
                    Ok(None) => {}
                    Err(error) => return Some(Err(error)),
                }
            }
            seen.insert((section.to_string(), record.sha256))
                .then_some(Ok((section.to_string(), payload)))
        })
        .collect()
}

fn retained_swobjects_sections(
    records: &[SourceRecord<'_>],
    annotations: &Annotations,
) -> Result<Vec<(u32, String, Vec<u8>)>, CodecError> {
    let mut sections = records
        .iter()
        .filter_map(|record| {
            let provenance = annotations.provenance.get(&record.id.0)?;
            let section = annotations
                .streams
                .get(usize::try_from(provenance.stream).ok()?)?
                .as_str();
            section
                .to_ascii_lowercase()
                .contains("swobjects")
                .then_some((provenance.stream, section, record))
        })
        .collect::<Vec<_>>();
    sections.sort_by_key(|(stream, _, _)| *stream);
    let mut seen = HashSet::new();
    sections
        .into_iter()
        .filter_map(|(stream, section, record)| {
            seen.insert((section, record.sha256))
                .then_some((stream, section, record))
        })
        .map(|(stream, section, record)| {
            let data = record.data.ok_or_else(|| {
                CodecError::Malformed("retained SLDPRT SWObjects section has no bytes".into())
            })?;
            Ok((stream, section.to_string(), data.to_vec()))
        })
        .collect()
}

fn patch_retained_swobjects_metadata(
    ir: &CadIr,
    annotations: &Annotations,
    sections: &mut [(u32, String, Vec<u8>)],
    length_scale: f64,
) -> Result<(), CodecError> {
    use cadmpeg_ir::attributes::AttributeValue;
    use std::cmp::Reverse;

    let mut attributes = metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name != "source_linear_unit_code")
        .map(|attribute| {
            let provenance = annotations.provenance.get(&attribute.id.0).ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT metadata attribute {} has no retained record provenance",
                    attribute.id
                ))
            })?;
            Ok((provenance.stream, provenance.offset, attribute))
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    attributes.sort_by_key(|(stream, offset, _)| (*stream, Reverse(*offset)));

    for (stream, offset, attribute) in attributes {
        let payload = sections
            .iter_mut()
            .find_map(|(candidate, _, payload)| (*candidate == stream).then_some(payload))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT metadata attribute {} references a missing SWObjects section",
                    attribute.id
                ))
            })?;
        let offset = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("SLDPRT metadata offset exceeds address space".into())
        })?;
        match attribute.name.as_str() {
            "bounding_envelope" => {
                let [AttributeValue::Vector(values)] = attribute.values.as_slice() else {
                    return Err(CodecError::Malformed("invalid bounding envelope".into()));
                };
                if values.len() != 4 {
                    return Err(CodecError::Malformed("invalid bounding envelope".into()));
                }
                let token = b"moBBoxCenterData_c";
                require_token(payload, offset, token)?;
                let mut bytes = Vec::with_capacity(32);
                for value in values {
                    bytes.extend((value * length_scale).to_le_bytes());
                }
                overwrite_bytes(payload, offset + token.len() + 4, &bytes)?;
            }
            "default_reference_plane" => {
                let [AttributeValue::Vector(origin), AttributeValue::Vector(frame)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid default reference plane".into(),
                    ));
                };
                if origin.len() != 3 || frame.len() != 6 {
                    return Err(CodecError::Malformed(
                        "invalid default reference plane".into(),
                    ));
                }
                let token = b"moDefaultRefPlnData_c";
                require_token(payload, offset, token)?;
                let mut bytes = Vec::with_capacity(72);
                for value in origin {
                    bytes.extend((value * length_scale).to_le_bytes());
                }
                for value in frame {
                    bytes.extend(value.to_le_bytes());
                }
                overwrite_bytes(payload, offset + token.len(), &bytes)?;
            }
            "transformed_reference_plane" => {
                let [AttributeValue::Vector(center), AttributeValue::Vector(extents), AttributeValue::Vector(auxiliary), AttributeValue::Float(diagonal)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid transformed reference plane".into(),
                    ));
                };
                if center.len() != 3 || extents.len() != 2 || auxiliary.len() != 3 {
                    return Err(CodecError::Malformed(
                        "invalid transformed reference plane".into(),
                    ));
                }
                let token = b"moTransRefPlaneData_c";
                require_token(payload, offset, token)?;
                let mut bytes = Vec::with_capacity(72);
                for value in center.iter().chain(extents) {
                    bytes.extend((value * length_scale).to_le_bytes());
                }
                for value in auxiliary {
                    bytes.extend(value.to_le_bytes());
                }
                bytes.extend((diagonal * length_scale).to_le_bytes());
                overwrite_bytes(payload, offset + token.len() + 8, &bytes)?;
            }
            "part_record" => {
                let [AttributeValue::Integer(id), AttributeValue::Integer(version)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed("invalid part record".into()));
                };
                let token = b"moPart_c";
                require_token(payload, offset, token)?;
                overwrite_bytes(
                    payload,
                    offset + token.len(),
                    &u32::try_from(*id)
                        .map_err(|_| CodecError::Malformed("invalid part id".into()))?
                        .to_le_bytes(),
                )?;
                overwrite_bytes(
                    payload,
                    offset + token.len() + 8,
                    &u32::try_from(*version)
                        .map_err(|_| CodecError::Malformed("invalid part version".into()))?
                        .to_le_bytes(),
                )?;
            }
            "configuration_manager" => {
                let [AttributeValue::Integer(minor), AttributeValue::Integer(states), AttributeValue::Integer(filetime)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid configuration manager".into(),
                    ));
                };
                let token = b"moConfigurationMgr_c";
                require_token(payload, offset, token)?;
                let start = offset + token.len();
                overwrite_bytes(
                    payload,
                    start + 66,
                    &u32::try_from(*minor)
                        .map_err(|_| {
                            CodecError::Malformed("invalid configuration minor version".into())
                        })?
                        .to_le_bytes(),
                )?;
                overwrite_bytes(
                    payload,
                    start + 107,
                    &[u8::try_from(*states).map_err(|_| {
                        CodecError::Malformed("invalid configuration state count".into())
                    })?],
                )?;
                overwrite_bytes(
                    payload,
                    start + 117,
                    &u64::try_from(*filetime)
                        .map_err(|_| {
                            CodecError::Malformed("invalid configuration timestamp".into())
                        })?
                        .to_le_bytes(),
                )?;
            }
            "source_linear_unit_name" => {
                let [AttributeValue::String(name)] = attribute.values.as_slice() else {
                    return Err(CodecError::Malformed(
                        "invalid source linear unit name".into(),
                    ));
                };
                let token = b"moLengthUserUnits_c";
                require_token(payload, offset, token)?;
                let marker = offset + token.len();
                if payload.get(marker..marker + 3) != Some(&[0xff, 0xfe, 0xff]) {
                    return Err(CodecError::Malformed(
                        "retained SLDPRT unit-name marker does not match its provenance".into(),
                    ));
                }
                let old_len = usize::from(*payload.get(marker + 3).ok_or_else(|| {
                    CodecError::Malformed("truncated retained SLDPRT unit name".into())
                })?);
                let units = name.encode_utf16().collect::<Vec<_>>();
                let byte_len = units.len().checked_mul(2).ok_or_else(|| {
                    CodecError::Malformed("source linear unit name is too long".into())
                })?;
                let new_len = u8::try_from(byte_len).map_err(|_| {
                    CodecError::Malformed("source linear unit name is too long".into())
                })?;
                if units.is_empty() {
                    return Err(CodecError::Malformed(
                        "source linear unit name is empty".into(),
                    ));
                }
                if usize::from(new_len) != old_len {
                    return Err(CodecError::NotImplemented(
                        "SLDPRT writer cannot resize a retained source linear unit name".into(),
                    ));
                }
                let mut replacement = vec![0xff, 0xfe, 0xff, new_len];
                replacement.extend(units.into_iter().flat_map(u16::to_le_bytes));
                let end = marker.checked_add(4 + old_len).ok_or_else(|| {
                    CodecError::Malformed("retained SLDPRT unit-name range overflow".into())
                })?;
                if end > payload.len() {
                    return Err(CodecError::Malformed(
                        "truncated retained SLDPRT unit name".into(),
                    ));
                }
                payload.splice(marker..end, replacement);
            }
            name => {
                return Err(CodecError::NotImplemented(format!(
                    "unsupported retained SLDPRT metadata attribute {name}"
                )));
            }
        }
    }
    Ok(())
}

fn require_token(payload: &[u8], offset: usize, token: &[u8]) -> Result<(), CodecError> {
    if payload.get(offset..offset + token.len()) == Some(token) {
        Ok(())
    } else {
        Err(CodecError::Malformed(
            "retained SLDPRT metadata token does not match its provenance".into(),
        ))
    }
}

fn overwrite_bytes(payload: &mut [u8], offset: usize, bytes: &[u8]) -> Result<(), CodecError> {
    let target = payload
        .get_mut(offset..offset + bytes.len())
        .ok_or_else(|| {
            CodecError::Malformed("retained SLDPRT metadata field is truncated".into())
        })?;
    target.copy_from_slice(bytes);
    Ok(())
}

fn patch_active_configuration_xml(
    payload: &[u8],
    name: &str,
) -> Result<Option<Vec<u8>>, CodecError> {
    if !payload.windows(12).any(|window| window == b"swSolidWorks") {
        return Ok(None);
    }
    let text = std::str::from_utf8(payload)
        .map_err(|_| CodecError::Malformed("invalid retained SolidWorks XML".into()))?;
    let document = roxmltree::Document::parse(text)
        .map_err(|_| CodecError::Malformed("invalid retained SolidWorks XML".into()))?;
    if document.root_element().tag_name().name() != "swSolidWorks" {
        return Ok(None);
    }
    let attribute = document
        .descendants()
        .find(|node| node.has_tag_name("swModel"))
        .ok_or_else(|| CodecError::Malformed("SolidWorks XML has no model record".into()))?
        .attributes()
        .find(|attribute| attribute.name() == "swConfigurationName")
        .ok_or_else(|| {
            CodecError::Malformed("SolidWorks XML has no active configuration".into())
        })?;
    let range = attribute.range();
    let mut output = String::with_capacity(text.len() + name.len());
    output.push_str(&text[..range.start]);
    output.push_str("swConfigurationName=\"");
    push_xml_attribute_value(&mut output, name);
    output.push('"');
    output.push_str(&text[range.end..]);
    Ok(Some(output.into_bytes()))
}

fn resolved_feature_payload(
    lane: &crate::records::FeatureInputLane,
    histories: &[crate::records::FeatureHistory],
) -> Result<Vec<u8>, CodecError> {
    const MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];
    let expected_classes =
        crate::resolved_features::names::class_declarations(&lane.native_payload, &lane.id);
    if lane.classes != expected_classes {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited class declarations",
            lane.id
        )));
    }
    let expected_names =
        crate::resolved_features::names::object_names(&lane.native_payload, &lane.id);
    if lane.names.len() != expected_names.len()
        || lane
            .names
            .iter()
            .zip(&expected_names)
            .any(|(actual, expected)| {
                actual.id != expected.id
                    || actual.parent != expected.parent
                    || actual.ordinal != expected.ordinal
                    || actual.offset != expected.offset
            })
    {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited object-name structure",
            lane.id
        )));
    }
    let mut expected_lane = lane.clone();
    expected_lane.scalars = crate::resolved_features::scalars::named_scalars(
        &lane.native_payload,
        &lane.id,
        &lane.names,
    );
    expected_lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
        &lane.id,
        &lane.classes,
        &expected_lane.scalars,
    );
    expected_lane.references = crate::resolved_features::markers::reference_cells(
        &expected_lane.scalars,
        &expected_lane.classes,
    );
    crate::resolved_features::bindings::bind_scalar_operands(
        histories,
        std::slice::from_mut(&mut expected_lane),
    );
    if !crate::resolved_features::scalars::scalar_indices_match(
        &lane.scalars,
        &expected_lane.scalars,
    ) {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited named scalars",
            lane.id
        )));
    }
    if lane.relation_bindings != expected_lane.relation_bindings {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited relation bindings",
            lane.id
        )));
    }
    if lane.relation_instances != expected_lane.relation_instances {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited relation instances",
            lane.id
        )));
    }
    if lane.references != expected_lane.references {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited reference cells",
            lane.id
        )));
    }
    let expected_offsets = lane
        .native_payload
        .windows(MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == MARKER).then_some(offset))
        .collect::<Vec<_>>();
    if expected_offsets.len() != lane.sketch_entities.len() {
        return Err(CodecError::malformed(format_args!(
            "feature-input lane {} has {} markers but {} native records",
            lane.id,
            expected_offsets.len(),
            lane.sketch_entities.len()
        )));
    }
    for (ordinal, ((entity, expected_entity), expected_offset)) in lane
        .sketch_entities
        .iter()
        .zip(&expected_lane.sketch_entities)
        .zip(&expected_offsets)
        .enumerate()
    {
        if entity.ordinal != ordinal as u32
            || usize::try_from(entity.offset) != Ok(*expected_offset)
            || entity.feature_ref != expected_entity.feature_ref
            || entity.links != expected_entity.links
            || entity.link_selector != expected_entity.link_selector
            || entity.object_index
                != crate::resolved_features::markers::marker_object_index(
                    &lane.native_payload,
                    *expected_offset,
                )
            || entity.local_id
                != crate::resolved_features::markers::marker_local_id(
                    &lane.native_payload,
                    *expected_offset,
                )
        {
            return Err(CodecError::malformed(format_args!(
                "feature-input lane {} has inconsistent marker order",
                lane.id
            )));
        }
    }
    let mut payload = lane.native_payload.clone();
    for entity in &lane.sketch_entities {
        let offset = usize::try_from(entity.offset).map_err(|_| {
            CodecError::Malformed("feature-input offset exceeds address space".into())
        })?;
        let marker_end = offset
            .checked_add(MARKER.len())
            .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
        if payload.get(offset..marker_end) != Some(MARKER) {
            return Err(CodecError::Malformed(
                "feature-input marker does not match retained payload".into(),
            ));
        }
        let field_start = offset
            .checked_add(17)
            .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
        let field_end = offset
            .checked_add(21)
            .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
        let field = payload.get_mut(field_start..field_end).ok_or_else(|| {
            CodecError::Malformed("feature-input type field exceeds retained payload".into())
        })?;
        field.copy_from_slice(&entity.kind.native_code().to_le_bytes());
        if let Some(value) = entity.state_value {
            if !value.is_finite() {
                return Err(CodecError::Malformed(
                    "feature-input state value must be finite".into(),
                ));
            }
            let state_start = offset
                .checked_add(48)
                .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
            let state_end = state_start
                .checked_add(8)
                .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
            let state = payload.get_mut(state_start..state_end).ok_or_else(|| {
                CodecError::Malformed("feature-input state field exceeds retained payload".into())
            })?;
            state.copy_from_slice(&value.to_le_bytes());
        }
        if let Some(coordinates) = entity.coordinates_m {
            if !coordinates.iter().all(|value| value.is_finite()) {
                return Err(CodecError::Malformed(
                    "feature-input marker coordinates must be finite".into(),
                ));
            }
            if crate::resolved_features::markers::marker_coordinates(&lane.native_payload, offset)
                .is_none()
            {
                return Err(CodecError::NotImplemented(
                    "feature-input marker does not carry editable coordinate fields".into(),
                ));
            }
            for (relative, value) in [(66usize, coordinates[0]), (74, coordinates[1])] {
                let start = offset.checked_add(relative).ok_or_else(|| {
                    CodecError::Malformed("feature-input coordinate offset overflow".into())
                })?;
                let end = start.checked_add(8).ok_or_else(|| {
                    CodecError::Malformed("feature-input coordinate offset overflow".into())
                })?;
                payload
                    .get_mut(start..end)
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "feature-input coordinate field exceeds retained payload".into(),
                        )
                    })?
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    for (name, expected) in lane.names.iter().zip(&expected_names).rev() {
        if name.value == expected.value {
            continue;
        }
        let utf16 = name.value.encode_utf16().collect::<Vec<_>>();
        let length = u8::try_from(utf16.len()).map_err(|_| {
            CodecError::NotImplemented("feature-input object name exceeds 255 UTF-16 units".into())
        })?;
        let start = usize::try_from(expected.offset).map_err(|_| {
            CodecError::Malformed("feature-input name offset exceeds address space".into())
        })?;
        let end = start
            .checked_add(6 + expected.value.encode_utf16().count() * 2)
            .ok_or_else(|| CodecError::Malformed("feature-input name range overflow".into()))?;
        if payload.get(start..start + 5) != Some(&[0x04, 0x80, 0xff, 0xfe, 0xff]) {
            return Err(CodecError::Malformed(
                "feature-input name marker does not match retained payload".into(),
            ));
        }
        let mut replacement = vec![0x04, 0x80, 0xff, 0xfe, 0xff, length];
        for unit in utf16 {
            replacement.extend_from_slice(&unit.to_le_bytes());
        }
        payload.splice(start..end, replacement);
    }
    Ok(payload)
}

/// The record position a `sldprt:metadata:*` identifier carries: the ordinal of
/// the section the record was read from and its byte offset inside that
/// section's payload.
fn metadata_source_position(id: &str) -> Option<(u64, u64)> {
    let (section, offset) = id.rsplit_once('#')?.1.split_once(':')?;
    Some((section.parse().ok()?, offset.parse().ok()?))
}

/// This codec's document attributes, ordered by source payload position.
///
/// Arena order is by identifier name; writing that order would move every
/// record and rename attributes minted from byte offsets.
fn metadata_attributes(ir: &CadIr) -> Vec<&cadmpeg_ir::attributes::SourceAttribute> {
    let mut attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.starts_with("sldprt:"))
        .collect::<Vec<_>>();
    attributes.sort_by_key(|attribute| {
        let position = metadata_source_position(&attribute.id.0);
        (position.is_none(), position)
    });
    attributes
}

fn metadata_payloads(
    ir: &CadIr,
    length_scale: f64,
) -> Result<(Vec<u8>, Option<Vec<u8>>), CodecError> {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

    let mut objects = Vec::new();
    let mut unit_code = None;
    for attribute in metadata_attributes(ir) {
        if attribute.target != AttributeTarget::Document {
            return Err(CodecError::NotImplemented(
                "SLDPRT semantic writer does not support entity attributes".into(),
            ));
        }
        match attribute.name.as_str() {
            "bounding_envelope" => {
                let [AttributeValue::Vector(values)] = attribute.values.as_slice() else {
                    return Err(CodecError::Malformed("invalid bounding envelope".into()));
                };
                if values.len() != 4 {
                    return Err(CodecError::Malformed("invalid bounding envelope".into()));
                }
                objects.extend_from_slice(b"moBBoxCenterData_c");
                objects.extend_from_slice(&1u32.to_le_bytes());
                for value in values {
                    objects.extend_from_slice(&(value * length_scale).to_le_bytes());
                }
            }
            "default_reference_plane" => {
                let [AttributeValue::Vector(origin), AttributeValue::Vector(frame)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid default reference plane".into(),
                    ));
                };
                if origin.len() != 3 || frame.len() != 6 {
                    return Err(CodecError::Malformed(
                        "invalid default reference plane".into(),
                    ));
                }
                objects.extend_from_slice(b"moDefaultRefPlnData_c");
                for value in origin {
                    objects.extend_from_slice(&(value * length_scale).to_le_bytes());
                }
                for value in frame {
                    objects.extend_from_slice(&value.to_le_bytes());
                }
            }
            "transformed_reference_plane" => {
                let [AttributeValue::Vector(center), AttributeValue::Vector(extents), AttributeValue::Vector(auxiliary), AttributeValue::Float(diagonal)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid transformed reference plane".into(),
                    ));
                };
                if center.len() != 3 || extents.len() != 2 || auxiliary.len() != 3 {
                    return Err(CodecError::Malformed(
                        "invalid transformed reference plane".into(),
                    ));
                }
                objects.extend_from_slice(b"moTransRefPlaneData_c");
                objects.extend_from_slice(&[0xff; 8]);
                for value in center
                    .iter()
                    .chain(extents)
                    .chain(std::iter::once(diagonal))
                {
                    if !value.is_finite() {
                        return Err(CodecError::Malformed(
                            "invalid transformed reference plane".into(),
                        ));
                    }
                }
                if auxiliary.iter().any(|value| !value.is_finite()) {
                    return Err(CodecError::Malformed(
                        "invalid transformed reference plane".into(),
                    ));
                }
                for value in center.iter().chain(extents) {
                    objects.extend_from_slice(&(value * length_scale).to_le_bytes());
                }
                for value in auxiliary {
                    objects.extend_from_slice(&value.to_le_bytes());
                }
                objects.extend_from_slice(&(diagonal * length_scale).to_le_bytes());
            }
            "part_record" => {
                let [AttributeValue::Integer(id), AttributeValue::Integer(version)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed("invalid part record".into()));
                };
                objects.extend_from_slice(b"moPart_c");
                objects.extend_from_slice(
                    &u32::try_from(*id)
                        .map_err(|_| CodecError::Malformed("invalid part id".into()))?
                        .to_le_bytes(),
                );
                objects.extend_from_slice(&0u32.to_le_bytes());
                objects.extend_from_slice(
                    &u32::try_from(*version)
                        .map_err(|_| CodecError::Malformed("invalid part version".into()))?
                        .to_le_bytes(),
                );
                objects.push(0);
            }
            "configuration_manager" => {
                let [AttributeValue::Integer(minor), AttributeValue::Integer(states), AttributeValue::Integer(filetime)] =
                    attribute.values.as_slice()
                else {
                    return Err(CodecError::Malformed(
                        "invalid configuration manager".into(),
                    ));
                };
                let mut record = [0u8; 125];
                record[66..70].copy_from_slice(
                    &u32::try_from(*minor)
                        .map_err(|_| {
                            CodecError::Malformed("invalid configuration minor version".into())
                        })?
                        .to_le_bytes(),
                );
                record[107] = u8::try_from(*states).map_err(|_| {
                    CodecError::Malformed("invalid configuration state count".into())
                })?;
                record[117..125].copy_from_slice(
                    &u64::try_from(*filetime)
                        .map_err(|_| {
                            CodecError::Malformed("invalid configuration timestamp".into())
                        })?
                        .to_le_bytes(),
                );
                objects.extend_from_slice(b"moConfigurationMgr_c");
                objects.extend_from_slice(&record);
            }
            "source_linear_unit_code" => {
                let [AttributeValue::Integer(code)] = attribute.values.as_slice() else {
                    return Err(CodecError::Malformed(
                        "invalid source linear unit code".into(),
                    ));
                };
                unit_code = Some(*code);
            }
            "source_linear_unit_name" => {
                let [AttributeValue::String(name)] = attribute.values.as_slice() else {
                    return Err(CodecError::Malformed(
                        "invalid source linear unit name".into(),
                    ));
                };
                let bytes = name
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>();
                let length = u8::try_from(bytes.len()).map_err(|_| {
                    CodecError::Malformed("source linear unit name is too long".into())
                })?;
                if bytes.is_empty() {
                    return Err(CodecError::Malformed(
                        "source linear unit name is empty".into(),
                    ));
                }
                objects.extend_from_slice(b"moLengthUserUnits_c");
                objects.extend_from_slice(&[0xff, 0xfe, 0xff, length]);
                objects.extend_from_slice(&bytes);
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "unsupported SLDPRT attribute {}",
                    attribute.name
                )))
            }
        }
    }
    let units = unit_code.map(|code| {
        format!("<Metadata><Property Name=\"SW_UnitsLinear\" Value=\"{code}\"/></Metadata>")
            .into_bytes()
    });
    Ok((objects, units))
}

pub(crate) fn swobjects_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let attributes = metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name != "source_linear_unit_code")
        .collect::<Vec<_>>();
    let materials = swobjects_materials(ir)?;
    Ok(cadmpeg_ir::hash::canonical_json_sha256(&(
        attributes, materials,
    )))
}

pub(crate) fn swobjects_material_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    Ok(cadmpeg_ir::hash::canonical_json_sha256(
        &swobjects_materials(ir)?,
    ))
}

pub(crate) fn swobjects_metadata_identity_local_sha256(ir: &CadIr) -> String {
    let identities = metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name != "source_linear_unit_code")
        .map(|attribute| (&attribute.id, attribute.name.as_str()))
        .collect::<Vec<_>>();
    cadmpeg_ir::hash::canonical_json_sha256(&identities)
}

fn history_payload(history: &crate::records::FeatureHistory) -> Result<Vec<u8>, CodecError> {
    validate_feature_graph(&history.features)?;
    let mut out = String::from("<Keywords");
    if let Some(name) = &history.part_name {
        xml_attribute(&mut out, "Name", name);
    }
    for (name, value) in &history.properties {
        xml_attribute(&mut out, name, value);
    }
    out.push('>');
    let write_configuration = |out: &mut String, configuration: &crate::records::Configuration| {
        out.push_str("<Configuration");
        xml_attribute(out, "Name", &configuration.name);
        if let Some(source_index) = configuration.source_index {
            xml_attribute(out, "SourceIndex", &source_index.to_string());
        }
        if let Some(material) = &configuration.material {
            xml_attribute(out, "Material", material);
        }
        for (name, value) in &configuration.properties {
            xml_attribute(out, name, value);
        }
        out.push_str("/>");
    };
    let mut roots = history
        .features
        .iter()
        .filter(|feature| feature.tree_parent.is_none() && feature.parent_source_id.is_none())
        .collect::<Vec<_>>();
    roots.sort_by_key(|feature| feature.ordinal);
    let mut emitted_configurations = HashSet::new();
    let mut emitted_features = HashSet::new();
    for item in &history.content {
        match item {
            crate::records::HistoryContent::Configuration(id) => {
                if let Some(configuration) = history
                    .configurations
                    .iter()
                    .find(|configuration| configuration.id == *id)
                {
                    write_configuration(&mut out, configuration);
                    emitted_configurations.insert(configuration.id.as_str());
                }
            }
            crate::records::HistoryContent::Feature(id) => {
                if let Some(feature) = roots.iter().find(|feature| feature.id == *id) {
                    write_feature_xml(&mut out, feature, &history.features);
                    emitted_features.insert(feature.id.as_str());
                }
            }
            crate::records::HistoryContent::Text(text) => xml_text(&mut out, text),
        }
    }
    for configuration in &history.configurations {
        if emitted_configurations.insert(configuration.id.as_str()) {
            write_configuration(&mut out, configuration);
        }
    }
    for feature in roots {
        if emitted_features.insert(feature.id.as_str()) {
            write_feature_xml(&mut out, feature, &history.features);
        }
    }
    out.push_str("</Keywords>");
    Ok(out.into_bytes())
}

pub(crate) fn validate_feature_graph(
    features: &[crate::records::Feature],
) -> Result<(), CodecError> {
    if features
        .iter()
        .any(|feature| !valid_xml_name(&feature.xml_tag))
    {
        return Err(CodecError::Malformed(
            "invalid feature XML element name".into(),
        ));
    }
    let by_id = features
        .iter()
        .filter_map(|feature| feature.source_id.as_ref().map(|id| (id.as_str(), feature)))
        .collect::<HashMap<_, _>>();
    if by_id.len()
        != features
            .iter()
            .filter(|feature| feature.source_id.is_some())
            .count()
    {
        return Err(CodecError::Malformed("duplicate feature source id".into()));
    }
    let by_record = features
        .iter()
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    if by_record.len() != features.len() {
        return Err(CodecError::Malformed("duplicate feature record id".into()));
    }
    for feature in features {
        let mut seen = HashSet::new();
        let mut parent = feature.parent_source_id.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(CodecError::Malformed("feature parent cycle".into()));
            }
            let node = by_id
                .get(id)
                .ok_or_else(|| CodecError::Malformed("feature references missing parent".into()))?;
            parent = node.parent_source_id.as_deref();
        }
        let mut seen = HashSet::new();
        let mut parent = feature.tree_parent.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(CodecError::Malformed("feature tree cycle".into()));
            }
            let node = by_record.get(id).ok_or_else(|| {
                CodecError::Malformed("feature references missing tree parent".into())
            })?;
            parent = node.tree_parent.as_deref();
        }
    }
    Ok(())
}

fn valid_xml_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':'))
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.'))
}

fn write_feature_xml(
    out: &mut String,
    feature: &crate::records::Feature,
    features: &[crate::records::Feature],
) {
    out.push('<');
    out.push_str(&feature.xml_tag);
    if let Some(id) = &feature.source_id {
        xml_attribute(out, "id", id);
    }
    xml_attribute(out, "Name", &feature.name);
    xml_attribute(out, "Type", &feature.kind);
    if feature.suppressed {
        xml_attribute(out, "Suppressed", "true");
    }
    for (name, value) in &feature.properties {
        xml_attribute(out, name, value);
    }
    out.push('>');
    let write_dimension = |out: &mut String, name: &str, value: &str| {
        out.push_str("<Dimension");
        xml_attribute(out, "Name", name);
        if let Some(properties) = feature.dimension_properties.get(name) {
            for (property, value) in properties {
                xml_attribute(out, property, value);
            }
        }
        out.push('>');
        xml_text(out, value);
        out.push_str("</Dimension>");
    };
    let mut children = features
        .iter()
        .filter(|child| {
            child.tree_parent.as_deref() == Some(feature.id.as_str())
                || (child.tree_parent.is_none()
                    && child.parent_source_id.as_deref() == feature.source_id.as_deref()
                    && feature.source_id.is_some())
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|child| child.ordinal);
    let mut emitted_dimensions = HashSet::new();
    let mut emitted_children = HashSet::new();
    if feature.content.is_empty() {
        for (name, value) in &feature.parameters {
            write_dimension(out, name, value);
            emitted_dimensions.insert(name.as_str());
        }
        if let Some(text) = &feature.text {
            xml_text(out, text);
        }
    } else {
        for item in &feature.content {
            match item {
                crate::records::FeatureContent::Dimension(name) => {
                    if let Some(value) = feature.parameters.get(name) {
                        write_dimension(out, name, value);
                        emitted_dimensions.insert(name.as_str());
                    }
                }
                crate::records::FeatureContent::Feature(id) => {
                    if let Some(child) = children.iter().find(|child| child.id == *id) {
                        write_feature_xml(out, child, features);
                        emitted_children.insert(child.id.as_str());
                    }
                }
                crate::records::FeatureContent::Text(text) => xml_text(out, text),
            }
        }
    }
    for (name, value) in &feature.parameters {
        if emitted_dimensions.insert(name) {
            write_dimension(out, name, value);
        }
    }
    for child in children {
        if !emitted_children.insert(child.id.as_str()) {
            continue;
        }
        write_feature_xml(out, child, features);
    }
    out.push_str("</");
    out.push_str(&feature.xml_tag);
    out.push('>');
}

fn xml_attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\n' => out.push_str("&#10;"),
            _ => out.push(character),
        }
    }
    out.push('"');
}

fn xml_text(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(character),
        }
    }
}

fn tessellation_payload(ir: &CadIr, length_scale: f64) -> Result<Vec<u8>, CodecError> {
    let meshes = ir
        .model
        .tessellations
        .iter()
        .map(sequential_tessellation)
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = b"uoTempBodyTessData_c".to_vec();
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(b"uoTempFaceTessData_c");
    let (triangle_count, strip_count) = match meshes.first() {
        Some(mesh) => (
            u32::try_from(mesh.triangles.len()).map_err(|_| {
                CodecError::Malformed("tessellation triangle count overflow".into())
            })?,
            u32::try_from(mesh.strip_lengths.len())
                .map_err(|_| CodecError::Malformed("tessellation strip count overflow".into()))?,
        ),
        None => (0, 0),
    };
    out.extend_from_slice(&triangle_count.to_le_bytes());
    out.extend_from_slice(&strip_count.to_le_bytes());
    for mesh in &meshes {
        let strips = mesh
            .strip_lengths
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        descriptor(&mut out, 4, 8, 2, mesh.strip_lengths.len(), &strips);
        let mut positions = Vec::with_capacity(mesh.vertices.len() * 12);
        for point in &mesh.vertices {
            for value in [point.x, point.y, point.z] {
                positions.extend_from_slice(
                    &tessellation_f32(value * length_scale, "position")?.to_le_bytes(),
                );
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.vertices.len(), &positions);
        let mut normals = Vec::with_capacity(mesh.normals.len() * 12);
        for normal in &mesh.normals {
            for value in [normal.x, normal.y, normal.z] {
                normals.extend_from_slice(&tessellation_f32(value, "normal")?.to_le_bytes());
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.normals.len(), &normals);
        let auxiliary_start = usize::from(has_core_tessellation_channels(&mesh.channels)) * 3;
        let auxiliary_count = mesh.channels.len().saturating_sub(auxiliary_start).min(3);
        let auxiliary = &mesh.channels[auxiliary_start..auxiliary_start + auxiliary_count];
        let strip_lengths = mesh
            .strip_lengths
            .iter()
            .map(|length| usize::try_from(*length))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CodecError::Malformed("tessellation strip length overflow".into()))?;
        if !auxiliary.is_empty()
            && !crate::tessellation::auxiliary_channels_are_consistent(&strip_lengths, auxiliary)
        {
            return Err(CodecError::Malformed(
                "tessellation auxiliary channels are incomplete or inconsistent".into(),
            ));
        }
        for channel in auxiliary {
            descriptor(
                &mut out,
                channel.item_size,
                channel.kind,
                channel.flags,
                channel.count as usize,
                &channel.data,
            );
        }
        let list_c = mesh
            .strip_lengths
            .iter()
            .map(|length| {
                length
                    .checked_mul(2)
                    .and_then(|value| value.checked_sub(2))
                    .ok_or_else(|| {
                        CodecError::Malformed("tessellation strip is too short or too large".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for index in auxiliary_count..3 {
            match index {
                0 => descriptor(&mut out, 4, 8, 2, 0, &[]),
                1 => {
                    let data = list_c
                        .iter()
                        .flat_map(|value| value.to_le_bytes())
                        .collect::<Vec<_>>();
                    descriptor(&mut out, 4, 8, 2, list_c.len(), &data);
                }
                2 => descriptor(&mut out, 1, 8, 2, 0, &[]),
                _ => unreachable!("three auxiliary descriptors"),
            }
        }
    }
    Ok(out)
}

fn has_core_tessellation_channels(
    channels: &[cadmpeg_ir::tessellation::TessellationChannel],
) -> bool {
    matches!(channels, [strips, positions, normals, ..]
        if (strips.item_size, strips.kind) == (4, 8)
            && (positions.item_size, positions.kind) == (12, 100)
            && (normals.item_size, normals.kind) == (12, 100))
}

pub(super) fn sequential_tessellation(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
) -> Result<cadmpeg_ir::tessellation::Tessellation, CodecError> {
    if !mesh.triangle_groups.is_empty() || !mesh.texture_assignments.is_empty() {
        return Err(CodecError::NotImplemented(
            "SLDPRT display tessellation triangle groups and texture assignments are not writable"
                .into(),
        ));
    }
    if !mesh.feature_edges.is_empty() {
        return Err(CodecError::NotImplemented(
            "SLDPRT display tessellation feature edges are not writable".into(),
        ));
    }
    let expected = triangles_from_strips(&mesh.strip_lengths)?;
    if expected == mesh.triangles
        && mesh.strip_lengths.iter().sum::<u32>() as usize == mesh.vertices.len()
        && mesh.corner_normals.is_empty()
    {
        return Ok(mesh.clone());
    }
    let indices = mesh
        .triangles
        .iter()
        .flat_map(|triangle| triangle.iter().copied())
        .map(|index| {
            usize::try_from(index)
                .ok()
                .filter(|index| *index < mesh.vertices.len())
                .ok_or_else(|| CodecError::Malformed("tessellation index is out of bounds".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let vertices = indices.iter().map(|index| mesh.vertices[*index]).collect();
    let normals = if !mesh.corner_normals.is_empty() {
        if mesh.corner_normals.len() != indices.len() {
            return Err(CodecError::Malformed(
                "tessellation corner normals are not parallel to triangle corners".into(),
            ));
        }
        mesh.corner_normals.clone()
    } else if mesh.normals.is_empty() {
        Vec::new()
    } else {
        if mesh.normals.len() != mesh.vertices.len() {
            return Err(CodecError::Malformed(
                "tessellation normals are not parallel to vertices".into(),
            ));
        }
        indices.iter().map(|index| mesh.normals[*index]).collect()
    };
    let mut channels = mesh.channels.clone();
    for channel in &mut channels {
        if channel.count as usize != mesh.vertices.len() {
            continue;
        }
        let item_size = usize::try_from(channel.item_size)
            .map_err(|_| CodecError::Malformed("tessellation channel item size overflow".into()))?;
        let expected_len =
            mesh.vertices.len().checked_mul(item_size).ok_or_else(|| {
                CodecError::Malformed("tessellation channel size overflow".into())
            })?;
        if channel.data.len() != expected_len {
            return Err(CodecError::Malformed(
                "tessellation channel payload length is inconsistent".into(),
            ));
        }
        channel.data = indices
            .iter()
            .flat_map(|index| {
                let start = index * item_size;
                channel.data[start..start + item_size].iter().copied()
            })
            .collect();
        channel.count = u32::try_from(indices.len())
            .map_err(|_| CodecError::Malformed("tessellation vertex count overflow".into()))?;
    }
    let triangle_count = u32::try_from(mesh.triangles.len())
        .map_err(|_| CodecError::Malformed("tessellation triangle count overflow".into()))?;
    let strip_lengths = alloc_filled(
        triangle_count as usize,
        3_u32,
        "SLDPRT tessellation triangle strips",
    )?;
    let triangles = triangles_from_strips(&strip_lengths)?;
    Ok(cadmpeg_ir::tessellation::Tessellation {
        id: mesh.id.clone(),
        body: mesh.body.clone(),
        faces: mesh.faces.clone(),
        chordal_deflection: mesh.chordal_deflection,
        source_object: mesh.source_object.clone(),
        vertices,
        triangles,
        feature_edges: Vec::new(),
        strip_lengths,
        normals,
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels,
    })
}

fn tessellation_f32(value: f64, role: &str) -> Result<f32, CodecError> {
    let narrowed = value as f32;
    if narrowed.is_finite() {
        Ok(narrowed)
    } else {
        Err(CodecError::malformed(format_args!(
            "SLDPRT tessellation {role} exceeds f32 range"
        )))
    }
}

fn triangles_from_strips(strips: &[u32]) -> Result<Vec<[u32; 3]>, CodecError> {
    let mut triangles = Vec::new();
    let mut base = 0u32;
    for &length in strips {
        for index in 0..length.saturating_sub(2) {
            triangles.push(if index % 2 == 0 {
                [base + index, base + index + 1, base + index + 2]
            } else {
                [base + index, base + index + 2, base + index + 1]
            });
        }
        base = base
            .checked_add(length)
            .ok_or_else(|| CodecError::Malformed("tessellation index overflow".into()))?;
    }
    Ok(triangles)
}

fn descriptor(out: &mut Vec<u8>, item_size: u32, kind: u32, flags: u32, count: usize, data: &[u8]) {
    out.extend_from_slice(&item_size.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(count as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn body_material(ir: &CadIr) -> Result<Option<(String, Color)>, CodecError> {
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|appearance| (&appearance.id, appearance))
        .collect::<HashMap<_, _>>();
    let mut selected: Option<(String, Color)> = None;
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Body(_) = &binding.target else {
            continue;
        };
        let appearance = appearances.get(&binding.appearance).ok_or_else(|| {
            CodecError::Malformed("body binding references missing appearance".into())
        })?;
        let color = appearance.base_color.ok_or_else(|| {
            CodecError::NotImplemented("SLDPRT body appearance has no base color".into())
        })?;
        let material = (
            appearance.name.clone().unwrap_or_else(|| "Material".into()),
            color,
        );
        if selected
            .as_ref()
            .is_some_and(|current| current != &material)
        {
            return Err(CodecError::NotImplemented(
                "SLDPRT writer cannot encode distinct body materials in SWObjects".into(),
            ));
        }
        selected = Some(material);
    }
    if selected.is_none() {
        for body in &ir.model.bodies {
            let Some(color) = body.color else { continue };
            let material = (
                body.name.clone().unwrap_or_else(|| "Material".into()),
                color,
            );
            if selected
                .as_ref()
                .is_some_and(|current| current != &material)
            {
                return Err(CodecError::NotImplemented(
                    "SLDPRT writer cannot encode distinct body materials in SWObjects".into(),
                ));
            }
            selected = Some(material);
        }
    }
    Ok(selected)
}

fn materials_payload(ir: &CadIr) -> Result<Vec<u8>, CodecError> {
    let mut payload = Vec::new();
    for (name, color) in swobjects_materials(ir)? {
        payload.extend(material_payload(&name, color)?);
    }
    Ok(payload)
}

fn swobjects_materials(ir: &CadIr) -> Result<Vec<(String, Color)>, CodecError> {
    let mut materials = Vec::<(String, Color)>::new();
    if let Some(material) = body_material(ir)? {
        materials.push(material);
    }
    for appearance in &ir.model.appearances {
        if appearance.schema.as_deref() != Some("moVisualProperties_c") {
            continue;
        }
        let Some(color) = appearance.base_color else {
            return Err(CodecError::NotImplemented(
                "SLDPRT material appearance has no base color".into(),
            ));
        };
        let material = (
            appearance.name.clone().unwrap_or_else(|| "Material".into()),
            color,
        );
        if !materials.contains(&material) {
            materials.push(material);
        }
    }
    Ok(materials)
}

fn material_payload(name: &str, color: Color) -> Result<Vec<u8>, CodecError> {
    let name = name.encode_utf16().collect::<Vec<_>>();
    let length = u8::try_from(name.len())
        .map_err(|_| CodecError::Malformed("SLDPRT material name is too long".into()))?;
    if name.is_empty() {
        return Err(CodecError::Malformed(
            "SLDPRT material name is empty".into(),
        ));
    }
    let mut out = b"moVisualProperties_c".to_vec();
    let component = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    out.extend_from_slice(
        &u32::from_le_bytes([
            component(color.r),
            component(color.g),
            component(color.b),
            0,
        ])
        .to_le_bytes(),
    );
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0x00c0_c0c0u32.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
    out.extend_from_slice(&[0xff, 0xfe, 0xff, length]);
    for unit in name {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(out)
}

pub(crate) fn brep_body(
    ir: &CadIr,
    length_scale: f64,
    schema_32001: bool,
) -> Result<Vec<u8>, CodecError> {
    let mut next = 2u16;
    let derived_sphere_seam_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            matches!(curve.geometry, CurveGeometry::Degenerate { .. })
                && curve
                    .id
                    .0
                    .starts_with("sldprt:brep:curve#sphere-seam-face:")
        })
        .map(|curve| curve.id.clone())
        .collect::<HashSet<_>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let curves = ir
        .model
        .curves
        .iter()
        .filter(|item| !derived_sphere_seam_curves.contains(&item.id))
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let points = ir
        .model
        .points
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let vertices = ir
        .model
        .vertices
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let edges = ir
        .model
        .edges
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let loops = ir
        .model
        .loops
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let faces = ir
        .model
        .faces
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let mut out = Vec::new();
    for surface in &ir.model.surfaces {
        if let SurfaceGeometry::Nurbs(nurbs) = &surface.geometry {
            write_nurbs_surface(
                &mut out,
                surfaces[&surface.id],
                nurbs,
                &mut next,
                length_scale,
                &surface.id.0,
            )?;
            continue;
        }
        let reference = surface_reference(&surface.geometry);
        let (kind, values) = surface_values(&surface.geometry, reference, length_scale)?;
        compact(&mut out, kind, surfaces[&surface.id], &values);
    }
    for curve in &ir.model.curves {
        if derived_sphere_seam_curves.contains(&curve.id) {
            continue;
        }
        if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
            write_nurbs_curve(
                &mut out,
                curves[&curve.id],
                nurbs,
                &mut next,
                length_scale,
                &curve.id.0,
            )?;
            continue;
        }
        let (kind, values) = curve_values(&curve.geometry, length_scale)?;
        compact(&mut out, kind, curves[&curve.id], &values);
    }
    let face_owners = ir
        .model
        .faces
        .iter()
        .map(|face| Ok((face.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let face_colors = face_colors(ir)?;
    let mut colored_faces = face_colors.keys().collect::<Vec<_>>();
    colored_faces.sort();
    let color_attrs = colored_faces
        .into_iter()
        .map(|face| Ok((face.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let face_color_definition = if color_attrs.is_empty() {
        None
    } else {
        let key_node = take_attr(&mut next)?;
        let definition_node = take_attr(&mut next)?;
        Some((key_node, definition_node))
    };
    for point in &ir.model.points {
        tag(&mut out, 0x1d);
        be16(&mut out, points[&point.id]);
        be32(&mut out, 0);
        out.extend_from_slice(&[0; 8]);
        for value in [point.position.x, point.position.y, point.position.z] {
            bef64(&mut out, value * length_scale);
        }
    }
    for vertex in &ir.model.vertices {
        tag(&mut out, 0x12);
        be16(&mut out, vertices[&vertex.id]);
        be32(&mut out, 0);
        for value in [0, 0, 0, 0, points[&vertex.point]] {
            be16(&mut out, value);
        }
        out.extend_from_slice(&MAGIC);
    }
    for edge in &ir.model.edges {
        tag(&mut out, 0x10);
        be16(&mut out, edges[&edge.id]);
        be32(&mut out, 0);
        be16(&mut out, 0);
        out.extend_from_slice(&MAGIC);
        for value in [
            0,
            0,
            0,
            edge.curve
                .as_ref()
                .and_then(|id| curves.get(id))
                .copied()
                .unwrap_or(0),
            0,
            0,
        ] {
            be16(&mut out, value);
        }
    }
    for coedge in &ir.model.coedges {
        tag(&mut out, 0x11);
        be16(&mut out, coedges[&coedge.id]);
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .ok_or_else(|| CodecError::Malformed("coedge references missing edge".into()))?;
        let start = if coedge.sense == Sense::Forward {
            &edge.start
        } else {
            &edge.end
        };
        for value in [
            0,
            loops[&coedge.owner_loop],
            coedges[&coedge.previous],
            coedges[&coedge.next],
            vertices[start],
            coedges[&coedge.radial_next],
            edges[&coedge.edge],
            0,
            0,
        ] {
            be16(&mut out, value);
        }
        out.push(if coedge.sense == Sense::Forward {
            0x2b
        } else {
            0x2d
        });
    }
    for lp in &ir.model.loops {
        let face = ir
            .model
            .faces
            .iter()
            .find(|face| face.id == lp.face)
            .ok_or_else(|| CodecError::Malformed("loop references missing face".into()))?;
        let position = face
            .loops
            .iter()
            .position(|id| id == &lp.id)
            .ok_or_else(|| CodecError::Malformed("face does not own referenced loop".into()))?;
        let next_loop = face.loops.get(position + 1).map_or(0, |id| loops[id]);
        tag(&mut out, 0x0f);
        be16(&mut out, loops[&lp.id]);
        be32(&mut out, 0);
        for value in [
            0,
            coedges[lp
                .coedges
                .first()
                .ok_or_else(|| CodecError::Malformed("empty loop".into()))?],
            faces[&lp.face],
            next_loop,
        ] {
            be16(&mut out, value);
        }
    }
    for face in &ir.model.faces {
        tag(&mut out, 0x0e);
        be16(&mut out, faces[&face.id]);
        be32(&mut out, 0);
        be16(&mut out, face_owners[&face.id]);
        out.extend_from_slice(&MAGIC);
        let first = face
            .loops
            .first()
            .ok_or_else(|| CodecError::Malformed("face has no loop".into()))?;
        for value in [0, 0, loops[first], 0, surfaces[&face.surface]] {
            be16(&mut out, value);
        }
        out.push(if face.sense == Sense::Forward {
            0x2b
        } else {
            0x2d
        });
        out.extend_from_slice(&[0; 10]);
    }
    write_body_hierarchy(
        ir,
        &faces,
        &surfaces,
        &face_owners,
        &color_attrs,
        face_color_definition,
        schema_32001,
        &mut next,
        &mut out,
    )?;
    let mut colored_faces = face_colors.into_iter().collect::<Vec<_>>();
    colored_faces.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (face, color) in colored_faces {
        entity53(&mut out, color_attrs[&face], color);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)] // The native and typed hierarchy writers share these identity maps.
fn write_body_hierarchy(
    ir: &CadIr,
    faces: &HashMap<cadmpeg_ir::ids::FaceId, u16>,
    surfaces: &HashMap<cadmpeg_ir::ids::SurfaceId, u16>,
    face_owners: &HashMap<cadmpeg_ir::ids::FaceId, u16>,
    color_attrs: &HashMap<cadmpeg_ir::ids::FaceId, u16>,
    face_color_definition: Option<(u16, u16)>,
    schema_32001: bool,
    next: &mut u16,
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let shells = ir
        .model
        .shells
        .iter()
        .map(|shell| (shell.id.clone(), shell))
        .collect::<HashMap<_, _>>();
    let regions = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.clone(), region))
        .collect::<HashMap<_, _>>();
    let mut assigned = HashSet::new();
    for body in &ir.model.bodies {
        let root = take_attr(next)?;
        let mut native_regions = Vec::new();
        for region_id in &body.regions {
            let region = regions
                .get(region_id)
                .ok_or_else(|| CodecError::Malformed("body references missing region".into()))?;
            if body.kind == BodyKind::Sheet && region.shells.len() != 1 {
                return Err(CodecError::NotImplemented(
                    "SLDPRT sheet regions require exactly one shell".into(),
                ));
            }
            let native_region = take_attr(next)?;
            native_regions.push(native_region);
            let mut native_lumps = Vec::new();
            for shell_id in &region.shells {
                let shell = shells.get(shell_id).ok_or_else(|| {
                    CodecError::Malformed("region references missing shell".into())
                })?;
                let mut owned = Vec::new();
                for face in &shell.faces {
                    if !faces.contains_key(face) {
                        return Err(CodecError::Malformed(
                            "shell references missing face".into(),
                        ));
                    }
                    if !assigned.insert(face.clone()) {
                        return Err(CodecError::Malformed(
                            "face belongs to multiple bodies".into(),
                        ));
                    }
                    owned.push(face_owners[face]);
                }
                let head = write_face_list(
                    out,
                    &owned,
                    next,
                    if schema_32001 { 0x0015 } else { 0x0013 },
                )?;
                if body.kind == BodyKind::Sheet {
                    entity51(out, 1, native_region, 0x001d, &[head, 0, 0, 0, 0, 0]);
                    continue;
                }
                let lump = take_attr(next)?;
                let shell_node = take_attr(next)?;
                let shell_link = take_attr(next)?;
                native_lumps.push(lump);
                entity51(out, 2, lump, 0x001f, &[shell_node, 0, 0, 0, 0, 0]);
                entity51(out, 2, shell_node, 0x0021, &[shell_link, 0, 0, 0, 0, 0]);
                entity51(out, 2, shell_link, 0x0023, &[head, 0, 0, 0, 0, 0]);
            }
            if body.kind != BodyKind::Sheet {
                entity51(
                    out,
                    1,
                    native_region,
                    0x001b,
                    &fixed_refs(&native_lumps, "SLDPRT region has more than six shells")?,
                );
            }
        }
        let mut root_refs = [0; 6];
        if native_regions.is_empty() {
            return Err(CodecError::Malformed("SLDPRT body has no regions".into()));
        }
        if native_regions.len() > 5 {
            return Err(CodecError::NotImplemented(
                "SLDPRT body has more than five regions".into(),
            ));
        }
        root_refs[1..=native_regions.len()].copy_from_slice(&native_regions);
        entity51(out, 2, root, 0x0017, &root_refs);
    }
    if assigned.len() != ir.model.faces.len() {
        return Err(CodecError::Malformed(
            "face is not assigned to a body".into(),
        ));
    }
    write_typed_body_hierarchy(ir, faces, surfaces, next, out)?;
    if let Some((key_node, definition_node)) = face_color_definition {
        attribute_definition(out, key_node, definition_node);
    }
    let mut face_owner_items = face_owners.iter().collect::<Vec<_>>();
    face_owner_items.sort_by_key(|(left, _)| *left);
    for (face, owner) in face_owner_items {
        let mut refs = [0; 6];
        refs[5] = color_attrs.get(face).copied().unwrap_or(0);
        let disc = if color_attrs.contains_key(face) {
            face_color_definition
                .map(|(_, definition_node)| definition_node)
                .ok_or_else(|| {
                    CodecError::Malformed("colored face has no color attribute definition".into())
                })?
        } else if schema_32001 {
            0x001f
        } else {
            0x0015
        };
        entity51(out, 1, *owner, disc, &refs);
    }
    Ok(())
}

/// Write the typed XT ownership graph used by the decoder for body membership.
///
/// The compact topology records use the same transmit attributes as the
/// neutral arenas.  BODY, REGION, and SHELL receive fresh attributes because
/// those nodes are ownership records rather than compact topology identities;
/// FACE keeps the compact bridge attribute so the two record families join
/// without a second identity map.
fn write_typed_body_hierarchy(
    ir: &CadIr,
    faces: &HashMap<cadmpeg_ir::ids::FaceId, u16>,
    surfaces: &HashMap<cadmpeg_ir::ids::SurfaceId, u16>,
    next: &mut u16,
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let body_attrs = ir
        .model
        .bodies
        .iter()
        .map(|body| Ok((body.id.clone(), take_attr(next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let region_attrs = ir
        .model
        .regions
        .iter()
        .map(|region| Ok((region.id.clone(), take_attr(next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let shell_attrs = ir
        .model
        .shells
        .iter()
        .map(|shell| Ok((shell.id.clone(), take_attr(next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let mut face_shells = HashMap::new();
    for shell in &ir.model.shells {
        let shell_attr = shell_attrs[&shell.id];
        for face in &shell.faces {
            if face_shells.insert(face.clone(), shell_attr).is_some() {
                return Err(CodecError::Malformed(
                    "face belongs to multiple typed shells".into(),
                ));
            }
        }
    }

    for (index, body) in ir.model.bodies.iter().enumerate() {
        let body_attr = body_attrs[&body.id];
        let first_region = body
            .regions
            .first()
            .ok_or_else(|| CodecError::Malformed("body has no typed region".into()))?;
        let first_region = region_attrs[first_region];
        let first_shell = ir
            .model
            .regions
            .iter()
            .find(|region| region.id.0 == body.regions[0].0)
            .and_then(|region| region.shells.first())
            .map(|shell| shell_attrs[shell])
            .ok_or_else(|| CodecError::Malformed("typed region has no shell".into()))?;
        typed_prefix(out, 0x0c, body_attr, 0x1000_0000 + index as u32);
        for value in [5_u32, 6, 1, 1, 1, 1] {
            typed_ref(out, value)?;
        }
        bef64(out, 1000.0);
        bef64(out, TYPED_BODY_LINEAR_RESOLUTION);
        for value in [1_u32, 1, 1] {
            typed_ref(out, value)?;
        }
        out.push(1);
        typed_ref(out, 2_u32)?;
        out.push(match body.kind {
            BodyKind::Solid => 1,
            BodyKind::Wire => 2,
            BodyKind::Sheet => 3,
            BodyKind::General => 6,
        });
        out.push(1);
        for value in [first_shell, 1, 1, 1, 1, 1, 1] {
            typed_ref(out, value)?;
        }
        for value in [first_region, 1, 1, 1] {
            typed_ref(out, value)?;
        }
    }

    for (index, shell) in ir.model.shells.iter().enumerate() {
        let body = ir
            .model
            .regions
            .iter()
            .find(|region| region.shells.iter().any(|candidate| candidate == &shell.id))
            .map(|region| &region.body)
            .ok_or_else(|| CodecError::Malformed("typed shell has no region".into()))?;
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.shells.iter().any(|candidate| candidate == &shell.id))
            .ok_or_else(|| CodecError::Malformed("typed shell has no region".into()))?;
        typed_prefix(
            out,
            0x0d,
            shell_attrs[&shell.id],
            0x2000_0000 + index as u32,
        );
        for value in [1, body_attrs[body], 1, 1, 1, 1, region_attrs[&region.id], 1] {
            typed_ref(out, value)?;
        }
    }

    for (index, region) in ir.model.regions.iter().enumerate() {
        let body = body_attrs[&region.body];
        let body_regions = ir
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id == region.body)
            .ok_or_else(|| CodecError::Malformed("typed region has no body".into()))?;
        let position = body_regions
            .regions
            .iter()
            .position(|candidate| candidate == &region.id)
            .ok_or_else(|| CodecError::Malformed("body does not reference typed region".into()))?;
        let next_region = body_regions
            .regions
            .get(position + 1)
            .map_or(1, |id| region_attrs[id]);
        let previous_region = position
            .checked_sub(1)
            .and_then(|position| body_regions.regions.get(position))
            .map_or(1, |id| region_attrs[id]);
        let shell_head = region
            .shells
            .first()
            .map(|id| shell_attrs[id])
            .ok_or_else(|| CodecError::Malformed("typed region has no shell".into()))?;
        typed_prefix(
            out,
            0x13,
            region_attrs[&region.id],
            0x3000_0000 + index as u32,
        );
        for value in [1, body, next_region, previous_region, shell_head] {
            typed_ref(out, value)?;
        }
        out.push(b'S');
    }

    for (index, face) in ir.model.faces.iter().enumerate() {
        let shell = face_shells
            .get(&face.id)
            .copied()
            .ok_or_else(|| CodecError::Malformed("typed face has no shell".into()))?;
        typed_prefix(out, 0x0e, faces[&face.id], 0x4000_0000 + index as u32);
        typed_ref(out, 1_u32)?;
        out.extend_from_slice(&MAGIC);
        for value in [1, 1, 0, shell, surfaces[&face.surface]] {
            typed_ref(out, value)?;
        }
        out.push(match face.sense {
            Sense::Forward => 0x2b,
            Sense::Reversed => 0x2d,
        });
    }
    Ok(())
}

fn typed_prefix(out: &mut Vec<u8>, kind: u8, attr: u16, node_id: u32) {
    tag(out, kind);
    out.push(0xff);
    be16(out, attr);
    be32(out, node_id);
}

/// Emit one Parasolid XT pointer in its compact or extended two-cell form.
///
/// The high bit of the first cell is the form discriminator, so `0x7fff` is
/// the first value that requires two cells. Two u16 cells provide 31 payload
/// bits; refusing a larger value keeps the writer from emitting a truncated
/// pointer.
fn typed_ref(out: &mut Vec<u8>, value: impl Into<u32>) -> Result<(), CodecError> {
    let value = value.into();
    if value > 0x7fff_ffff {
        return Err(CodecError::NotImplemented(
            "SLDPRT typed reference exceeds the 31-bit XT pointer range".into(),
        ));
    }
    if value <= 0x7ffe {
        be16(out, value as u16);
    } else {
        be16(out, 0x8000 | (value as u16 & 0x7fff));
        be16(out, (value >> 15) as u16);
    }
    Ok(())
}

fn fixed_refs(values: &[u16], message: &str) -> Result<[u16; 6], CodecError> {
    if values.len() > 6 {
        return Err(CodecError::NotImplemented(message.into()));
    }
    let mut refs = [0; 6];
    refs[..values.len()].copy_from_slice(values);
    Ok(refs)
}

fn face_colors(ir: &CadIr) -> Result<HashMap<cadmpeg_ir::ids::FaceId, Color>, CodecError> {
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|appearance| (&appearance.id, appearance))
        .collect::<HashMap<_, _>>();
    let mut colors = ir
        .model
        .faces
        .iter()
        .filter_map(|face| face.color.map(|color| (face.id.clone(), color)))
        .collect::<HashMap<_, _>>();
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Face(face) = &binding.target else {
            continue;
        };
        let appearance = appearances.get(&binding.appearance).ok_or_else(|| {
            CodecError::Malformed("face binding references missing appearance".into())
        })?;
        let color = appearance.base_color.ok_or_else(|| {
            CodecError::NotImplemented("SLDPRT face appearance has no base color".into())
        })?;
        if colors
            .insert(face.clone(), color)
            .is_some_and(|old| old != color)
        {
            return Err(CodecError::Malformed(
                "face has conflicting appearance colors".into(),
            ));
        }
    }
    Ok(colors)
}

fn entity53(out: &mut Vec<u8>, attr: u16, color: Color) {
    tag(out, 0x53);
    be32(out, 3);
    be16(out, attr);
    for value in [color.r, color.g, color.b] {
        bef64(out, f64::from(value));
    }
}

fn attribute_definition(out: &mut Vec<u8>, key_node: u16, definition_node: u16) {
    const FAMILY: &[u8] = b"SDL/TYSA_COLOUR";
    tag(out, 0x4f);
    be32(out, FAMILY.len() as u32);
    be16(out, key_node);
    out.extend_from_slice(FAMILY);
    tag(out, 0x50);
    be32(out, 2);
    be16(out, definition_node);
}

fn write_face_list(
    out: &mut Vec<u8>,
    owners: &[u16],
    next: &mut u16,
    disc: u16,
) -> Result<u16, CodecError> {
    let chunks = owners.chunks(5).collect::<Vec<_>>();
    let attrs = (0..chunks.len().max(1))
        .map(|_| take_attr(next))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, attr) in attrs.iter().enumerate() {
        let mut refs = [0u16; 6];
        refs[0] = attrs.get(index + 1).copied().unwrap_or(0);
        if let Some(chunk) = chunks.get(index) {
            refs[1..=chunk.len()].copy_from_slice(chunk);
        }
        entity51(out, 2, *attr, disc, &refs);
    }
    Ok(attrs[0])
}

fn entity51(out: &mut Vec<u8>, flags: u32, attr: u16, disc: u16, refs: &[u16; 6]) {
    tag(out, 0x51);
    be32(out, flags);
    be16(out, attr);
    be32(out, 1);
    be16(out, disc);
    for reference in refs {
        be16(out, *reference);
    }
}

pub(super) fn surface_values(
    geometry: &SurfaceGeometry,
    reference: cadmpeg_ir::math::Vector3,
    length_scale: f64,
) -> Result<(u8, Vec<f64>), CodecError> {
    let scaled = |value: f64| value * length_scale;
    let result = match geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => (
            0x32,
            vec![
                scaled(origin.x),
                scaled(origin.y),
                scaled(origin.z),
                normal.x,
                normal.y,
                normal.z,
                reference.x,
                reference.y,
                reference.z,
            ],
        ),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => (
            0x33,
            vec![
                scaled(origin.x),
                scaled(origin.y),
                scaled(origin.z),
                axis.x,
                axis.y,
                axis.z,
                scaled(*radius),
                reference.x,
                reference.y,
                reference.z,
            ],
        ),
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            ratio,
            half_angle,
            ..
        } => {
            if *ratio != 1.0 {
                return Err(CodecError::NotImplemented(
                    "SLDPRT compact cone carriers encode circular cones only".into(),
                ));
            }
            if !(*half_angle > 0.0 && *half_angle < std::f64::consts::FRAC_PI_2) {
                return Err(CodecError::NotImplemented(
                    "SLDPRT compact cone carriers require an acute positive half-angle".into(),
                ));
            }
            (
                0x34,
                vec![
                    scaled(origin.x),
                    scaled(origin.y),
                    scaled(origin.z),
                    axis.x,
                    axis.y,
                    axis.z,
                    scaled(*radius),
                    half_angle.sin(),
                    half_angle.cos(),
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            radius,
            ..
        } => {
            if *radius < 0.0 {
                return Err(CodecError::NotImplemented(
                    "SLDPRT compact sphere carriers require a positive radius".into(),
                ));
            }
            let axis = *axis;
            (
                0x35,
                vec![
                    scaled(center.x),
                    scaled(center.y),
                    scaled(center.z),
                    scaled(*radius),
                    axis.x,
                    axis.y,
                    axis.z,
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            if !(*major_radius > *minor_radius && *minor_radius > 0.0) {
                return Err(CodecError::NotImplemented(
                    "SLDPRT compact torus carriers require major > minor > 0".into(),
                ));
            }
            (
                0x36,
                vec![
                    scaled(center.x),
                    scaled(center.y),
                    scaled(center.z),
                    axis.x,
                    axis.y,
                    axis.z,
                    scaled(*major_radius),
                    scaled(*minor_radius),
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support this surface carrier".into(),
            ))
        }
    };
    Ok(result)
}

fn write_nurbs_curve(
    out: &mut Vec<u8>,
    wrapper: u16,
    nurbs: &NurbsCurve,
    next: &mut u16,
    length_scale: f64,
    entity: &str,
) -> Result<(), CodecError> {
    if nurbs.periodic {
        return Err(CodecError::NotImplemented(
            "semantic SLDPRT writer does not support periodic NURBS curves".into(),
        ));
    }
    let descriptor = take_attr(next)?;
    let control = take_attr(next)?;
    let multiplicity = take_attr(next)?;
    let knots = take_attr(next)?;
    let degree = u16::try_from(nurbs.degree).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS curve {entity} degree {} exceeds the native u16 field",
            nurbs.degree
        ))
    })?;
    let control_count = u32::try_from(nurbs.control_points.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS curve {entity} pole count exceeds the native u32 field"
        ))
    })?;
    tag(out, 0x86);
    be16(out, wrapper);
    be16(out, descriptor);
    out.extend_from_slice(&[0; 8]);
    tag(out, 0x88);
    be16(out, descriptor);
    be16(out, degree);
    be32(out, control_count);
    be16(out, if nurbs.weights.is_some() { 4 } else { 3 });
    be32(out, 2);
    out.push(0);
    be32(out, 0);
    for attr in [control, multiplicity, knots] {
        be16(out, attr);
    }
    let poles = homogeneous_poles(
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        length_scale,
    )?;
    f64_array(out, 0x2d, control, &poles, entity)?;
    let (unique, mult) = unique_knots(&nurbs.knots, entity)?;
    u16_array(out, multiplicity, &mult, entity)?;
    f64_array(out, 0x80, knots, &unique, entity)?;
    Ok(())
}

fn write_nurbs_surface(
    out: &mut Vec<u8>,
    wrapper: u16,
    nurbs: &NurbsSurface,
    next: &mut u16,
    length_scale: f64,
    entity: &str,
) -> Result<(), CodecError> {
    if nurbs.u_periodic || nurbs.v_periodic {
        return Err(CodecError::NotImplemented(
            "semantic SLDPRT writer does not support periodic NURBS surfaces".into(),
        ));
    }
    let u_degree = u16::try_from(nurbs.u_degree).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} u degree exceeds the native u16 field"
        ))
    })?;
    let v_degree = u16::try_from(nurbs.v_degree).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} v degree exceeds the native u16 field"
        ))
    })?;
    if u_degree == 0 || v_degree == 0 {
        return Err(CodecError::Malformed(
            "NURBS surface degree must be positive".into(),
        ));
    }
    let u_count = usize::try_from(nurbs.u_count).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} u pole count exceeds the host address space"
        ))
    })?;
    let v_count = usize::try_from(nurbs.v_count).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} v pole count exceeds the host address space"
        ))
    })?;
    let expected_poles = u_count.checked_mul(v_count).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} pole grid exceeds the host address space"
        ))
    })?;
    if nurbs.control_points.len() != expected_poles {
        return Err(CodecError::Malformed(
            "invalid NURBS surface pole count".into(),
        ));
    }
    let (u_unique, u_mult) = unique_knots(&nurbs.u_knots, entity)?;
    let (v_unique, v_mult) = unique_knots(&nurbs.v_knots, entity)?;
    if u_count <= usize::from(u_degree) || v_count <= usize::from(v_degree) {
        return Err(CodecError::Malformed(
            "NURBS surface pole count must exceed its degree".into(),
        ));
    }
    let expected_u_knots = u_count
        .checked_add(usize::from(u_degree))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("NURBS surface u knot count overflow".into()))?;
    let expected_v_knots = v_count
        .checked_add(usize::from(v_degree))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("NURBS surface v knot count overflow".into()))?;
    if nurbs.u_knots.len() != expected_u_knots || nurbs.v_knots.len() != expected_v_knots {
        return Err(CodecError::Malformed(
            "NURBS surface knot vector length does not match its stored shape".into(),
        ));
    }
    if !nurbs
        .u_knots
        .iter()
        .chain(&nurbs.v_knots)
        .all(|value| value.is_finite())
        || !knots_nondecreasing(&nurbs.u_knots)
        || !knots_nondecreasing(&nurbs.v_knots)
    {
        return Err(CodecError::Malformed(
            "NURBS surface knot vectors must be finite and nondecreasing".into(),
        ));
    }
    let poles = homogeneous_poles(
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        length_scale,
    )?;
    let dimension = if nurbs.weights.is_some() { 4 } else { 3 };
    let u_knot_count = u32::try_from(u_unique.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} u distinct knot count exceeds the native u32 field"
        ))
    })?;
    let v_knot_count = u32::try_from(v_unique.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} v distinct knot count exceeds the native u32 field"
        ))
    })?;
    let descriptor = take_attr(next)?;
    let control = take_attr(next)?;
    let u_multiplicity = take_attr(next)?;
    let v_multiplicity = take_attr(next)?;
    let u_knots = take_attr(next)?;
    let v_knots = take_attr(next)?;
    tag(out, 0x7c);
    be16(out, wrapper);
    be32(out, 1);
    out.extend_from_slice(&[0; 10]);
    out.push(0x2b);
    be16(out, descriptor);
    be16(out, 0);
    tag(out, 0x7e);
    be16(out, descriptor);
    out.extend_from_slice(&[0, 0]);
    be16(out, u_degree);
    be16(out, v_degree);
    be32(out, nurbs.u_count);
    be32(out, nurbs.v_count);
    out.extend_from_slice(&[1, 1]);
    be32(out, u_knot_count);
    be32(out, v_knot_count);
    out.push(u8::from(nurbs.weights.is_some()));
    out.extend_from_slice(&[0, 0]);
    out.push(0x0c);
    be16(out, dimension);
    for attr in [control, u_multiplicity, v_multiplicity, u_knots, v_knots] {
        be16(out, attr);
    }
    f64_array(out, 0x2d, control, &poles, entity)?;
    u16_array(out, u_multiplicity, &u_mult, entity)?;
    u16_array(out, v_multiplicity, &v_mult, entity)?;
    f64_array(out, 0x80, u_knots, &u_unique, entity)?;
    f64_array(out, 0x80, v_knots, &v_unique, entity)?;
    Ok(())
}

fn take_attr(next: &mut u16) -> Result<u16, CodecError> {
    let attr = *next;
    *next = next
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("SLDPRT attribute space exhausted".into()))?;
    Ok(attr)
}

fn homogeneous_poles(
    points: &[cadmpeg_ir::math::Point3],
    weights: Option<&[f64]>,
    length_scale: f64,
) -> Result<Vec<f64>, CodecError> {
    if weights.is_some_and(|values| values.len() != points.len()) {
        return Err(CodecError::Malformed("invalid NURBS weight count".into()));
    }
    let mut out = Vec::with_capacity(points.len() * if weights.is_some() { 4 } else { 3 });
    for (index, point) in points.iter().enumerate() {
        let weight = weights.map_or(1.0, |values| values[index]);
        out.extend([
            point.x * length_scale * weight,
            point.y * length_scale * weight,
            point.z * length_scale * weight,
        ]);
        if weights.is_some() {
            out.push(weight);
        }
    }
    Ok(out)
}

fn unique_knots(knots: &[f64], entity: &str) -> Result<(Vec<f64>, Vec<u16>), CodecError> {
    let mut unique = Vec::new();
    let mut multiplicities: Vec<u16> = Vec::new();
    for &knot in knots {
        if unique.last() == Some(&knot) {
            let multiplicity = multiplicities.last_mut().expect("matching unique knot");
            *multiplicity = multiplicity.checked_add(1).ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT NURBS carrier {entity} knot multiplicity exceeds the native u16 field"
                ))
            })?;
        } else {
            unique.push(knot);
            multiplicities.push(1);
        }
    }
    Ok((unique, multiplicities))
}

fn f64_array(
    out: &mut Vec<u8>,
    kind: u8,
    attr: u16,
    values: &[f64],
    entity: &str,
) -> Result<(), CodecError> {
    let count = u32::try_from(values.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS carrier {entity} array length exceeds the native u32 field"
        ))
    })?;
    tag(out, kind);
    out.push(0x2b);
    be32(out, count);
    be16(out, attr);
    for value in values {
        bef64(out, *value);
    }
    Ok(())
}

fn u16_array(out: &mut Vec<u8>, attr: u16, values: &[u16], entity: &str) -> Result<(), CodecError> {
    let count = u32::try_from(values.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS carrier {entity} array length exceeds the native u32 field"
        ))
    })?;
    tag(out, 0x7f);
    out.push(0x2b);
    be32(out, count);
    be16(out, attr);
    for value in values {
        be16(out, *value);
    }
    Ok(())
}

pub(super) fn curve_values(
    geometry: &CurveGeometry,
    length_scale: f64,
) -> Result<(u8, Vec<f64>), CodecError> {
    let scaled = |value: f64| value * length_scale;
    let result = match geometry {
        CurveGeometry::Line { origin, direction } => (
            0x1e,
            vec![
                scaled(origin.x),
                scaled(origin.y),
                scaled(origin.z),
                direction.x,
                direction.y,
                direction.z,
            ],
        ),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let reference = *ref_direction;
            (
                0x1f,
                vec![
                    scaled(center.x),
                    scaled(center.y),
                    scaled(center.z),
                    axis.x,
                    axis.y,
                    axis.z,
                    reference.x,
                    reference.y,
                    reference.z,
                    scaled(*radius),
                ],
            )
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            0x20,
            vec![
                scaled(center.x),
                scaled(center.y),
                scaled(center.z),
                axis.x,
                axis.y,
                axis.z,
                major_direction.x,
                major_direction.y,
                major_direction.z,
                scaled(*major_radius),
                scaled(*minor_radius),
            ],
        ),
        CurveGeometry::Parabola { .. } | CurveGeometry::Hyperbola { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support parabola or hyperbola curves".into(),
            ))
        }
        CurveGeometry::Degenerate { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support degenerate curves".into(),
            ))
        }
        CurveGeometry::Composite { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support composite curves".into(),
            ))
        }
        CurveGeometry::Nurbs(_) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support NURBS curves".into(),
            ))
        }
        CurveGeometry::Polyline { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support polyline curve carriers".into(),
            ))
        }
        CurveGeometry::Procedural { .. } | CurveGeometry::Transformed { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support transformed curve carriers".into(),
            ))
        }
        CurveGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer cannot regenerate an opaque curve".into(),
            ))
        }
    };
    Ok(result)
}

pub(super) fn surface_reference(geometry: &SurfaceGeometry) -> cadmpeg_ir::math::Vector3 {
    match geometry {
        SurfaceGeometry::Plane { u_axis, .. } => *u_axis,
        SurfaceGeometry::Cylinder {
            axis: _,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            axis: _,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Torus {
            axis: _,
            ref_direction,
            ..
        } => *ref_direction,
        SurfaceGeometry::Sphere {
            axis: _,
            ref_direction,
            ..
        } => *ref_direction,
        SurfaceGeometry::Transformed { basis, .. } => surface_reference(basis),
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => cadmpeg_ir::math::Vector3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

fn compact(out: &mut Vec<u8>, kind: u8, attr: u16, values: &[f64]) {
    tag(out, kind);
    be16(out, attr);
    be32(out, 0);
    out.extend_from_slice(&[0; 10]);
    out.push(0x2b);
    for value in values {
        bef64(out, *value);
    }
}
pub(crate) fn parasolid_stream(body: &[u8], schema: &str) -> Vec<u8> {
    parasolid_stream_named(body, schema, "partition body")
}

pub(crate) fn parasolid_stream_named(body: &[u8], schema: &str, description: &str) -> Vec<u8> {
    let description = description.as_bytes();
    let schema = schema.as_bytes();
    let mut out = b"PS\0\0".to_vec();
    be16(&mut out, description.len() as u16);
    out.extend_from_slice(description);
    out.extend_from_slice(&[0, 0]);
    out.push(schema.len() as u8);
    out.extend_from_slice(schema);
    out.extend_from_slice(body);
    out
}
fn block(payload: &[u8], section: &str, type_id: u32) -> Result<Vec<u8>, CodecError> {
    use flate2::write::DeflateEncoder;
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload)?;
    let compressed = encoder.finish()?;
    let preamble: Vec<_> = section.bytes().map(|byte| byte.rotate_left(4)).collect();
    let mut out = MARKER.to_vec();
    out.extend_from_slice(&type_id.to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&(preamble.len() as u32).to_le_bytes());
    out.extend_from_slice(&preamble);
    out.extend_from_slice(&compressed);
    Ok(out)
}

fn directory_entry(
    type_id: u32,
    size: u32,
    section: &str,
    descriptor: [u8; 14],
    trailer: [u8; 6],
) -> Vec<u8> {
    let name = section
        .bytes()
        .map(|byte| byte.rotate_left(4))
        .collect::<Vec<_>>();
    let mut out = MARKER.to_vec();
    out.extend_from_slice(&type_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(&descriptor);
    out.extend_from_slice(&name);
    out.extend_from_slice(&trailer);
    out
}
fn tag(out: &mut Vec<u8>, kind: u8) {
    out.extend_from_slice(&[0, kind]);
}
fn be16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn be32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn bef64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod nurbs_write_tests {
    use super::*;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn retained_swift_pmi_requires_an_unchanged_semantic_baseline() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.pmi.push(cadmpeg_ir::PmiAnnotation {
            id: cadmpeg_ir::ids::PmiId("sldprt:model:pmi#A1".into()),
            name: Some("datum A".into()),
            visible: None,
            targets: vec![cadmpeg_ir::PmiTarget::ShapeAspect {
                source_id: "F1".into(),
            }],
            definition: cadmpeg_ir::PmiDefinition::Datum {
                identification: "A".into(),
            },
        });
        let hash = pmi_local_sha256(&ir).expect("PMI baseline hash");
        ir.source = Some(cadmpeg_ir::document::SourceMeta {
            declared: std::collections::BTreeMap::new(),
            dialect: None,
            format: "sldprt".into(),
            attributes: std::collections::BTreeMap::from([(
                PMI_LOCAL_DIGEST_ATTRIBUTE.into(),
                hash,
            )]),
        });
        assert!(check_semantic_support(&ir, &Annotations::default()).is_ok());

        let cadmpeg_ir::PmiDefinition::Datum { identification } =
            &mut ir.model.pmi.first_mut().expect("PMI annotation").definition
        else {
            panic!("datum definition");
        };
        *identification = "B".into();
        assert!(matches!(
            check_semantic_support(&ir, &Annotations::default()),
            Err(CodecError::NotImplemented(message)) if message.contains("SWIFT semantic PMI")
        ));

        ir.model.pmi.clear();
        assert!(matches!(
            check_semantic_support(&ir, &Annotations::default()),
            Err(CodecError::NotImplemented(message)) if message.contains("SWIFT semantic PMI")
        ));
    }

    #[test]
    fn writes_surface_degree_from_stored_descriptor() {
        let surface = NurbsSurface {
            u_degree: 9,
            v_degree: 1,
            u_knots: vec![0.0; 20],
            v_knots: vec![0.0; 4],
            u_count: 10,
            v_count: 2,
            control_points: vec![Point3::new(0.0, 0.0, 0.0); 20],
            weights: None,
            normal_reversed: false,
            u_periodic: false,
            v_periodic: false,
        };

        let mut bytes = Vec::new();
        write_nurbs_surface(
            &mut bytes,
            2,
            &surface,
            &mut 3,
            0.001,
            "test:surface#high-degree",
        )
        .expect("stored degree is representable");
        let carrier = crate::brep::spline::scan_surface_carriers(&bytes)
            .remove(&2)
            .expect("surface carrier");
        let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(decoded)) =
            carrier.geometry
        else {
            panic!("expected NURBS surface");
        };
        assert_eq!((decoded.u_degree, decoded.v_degree), (9, 1));
        assert_eq!((decoded.u_count, decoded.v_count), (10, 2));
    }

    #[test]
    fn writes_surface_shape_from_stored_counts() {
        let surface = NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
            u_count: 3,
            v_count: 4,
            control_points: vec![Point3::new(0.0, 0.0, 0.0); 12],
            weights: None,
            normal_reversed: false,
            u_periodic: false,
            v_periodic: false,
        };

        let mut bytes = Vec::new();
        write_nurbs_surface(
            &mut bytes,
            2,
            &surface,
            &mut 3,
            0.001,
            "test:surface#ambiguous-shape",
        )
        .expect("stored counts disambiguate the surface");
        let carrier = crate::brep::spline::scan_surface_carriers(&bytes)
            .remove(&2)
            .expect("surface carrier");
        let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(decoded)) =
            carrier.geometry
        else {
            panic!("expected NURBS surface");
        };
        assert_eq!(
            (
                decoded.u_count,
                decoded.v_count,
                decoded.u_degree,
                decoded.v_degree
            ),
            (3, 4, 2, 1)
        );
    }

    #[test]
    fn writes_typed_reference_boundaries() {
        for (value, expected) in [
            (0x7ffe_u32, vec![0x7f, 0xfe]),
            (0x7fff_u32, vec![0xff, 0xff, 0, 0]),
            (0x8000_u32, vec![0x80, 0, 0, 1]),
            (0x7fff_ffff_u32, vec![0xff, 0xff, 0xff, 0xff]),
        ] {
            let mut bytes = Vec::new();
            typed_ref(&mut bytes, value).expect("reference is representable");
            assert_eq!(bytes, expected, "encoded reference {value:#x}");
        }

        assert!(matches!(
            typed_ref(&mut Vec::new(), 0x8000_0000_u32),
            Err(CodecError::NotImplemented(message))
                if message.contains("31-bit XT pointer range")
        ));
    }

    #[test]
    fn rejects_curve_degree_and_knot_multiplicity_overflow() {
        let curve = NurbsCurve {
            degree: u32::from(u16::MAX) + 1,
            knots: Vec::new(),
            control_points: Vec::new(),
            weights: None,
            periodic: false,
        };
        let degree_error = write_nurbs_curve(
            &mut Vec::new(),
            2,
            &curve,
            &mut 3,
            0.001,
            "test:curve#high-degree",
        )
        .expect_err("expected error");
        let multiplicity_error = unique_knots(
            &vec![0.0; usize::from(u16::MAX) + 1],
            "test:curve#high-multiplicity",
        )
        .expect_err("expected error");

        assert!(matches!(
            degree_error,
            CodecError::NotImplemented(message)
                if message.contains("test:curve#high-degree")
                    && message.contains("native u16 field")
        ));
        assert!(matches!(
            multiplicity_error,
            CodecError::NotImplemented(message)
                if message.contains("test:curve#high-multiplicity")
                    && message.contains("knot multiplicity")
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests;
