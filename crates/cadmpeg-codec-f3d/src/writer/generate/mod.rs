// SPDX-License-Identifier: Apache-2.0
//! Source-less F3D archive generation: assemble a ZIP archive from a neutral
//! `CadIr` with no retained source.

use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ArenaDisposition, ArenaDispositions, ExportLossCode};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::manifest::{self, GENERATED_DESIGN_ASSET_FOLDER as DESIGN_FOLDER};
use crate::writer::primitives::{
    f3d_native, validate_assembly_projection, validate_configuration_projection,
};
mod act;
pub(super) mod attributes;
pub(super) mod features;
mod index;
pub(in crate::writer) mod native_bytes;
pub(crate) mod native_geometry;
pub(crate) mod parameter_links;
mod parameters;
pub(in crate::writer) mod preconditions;
mod presentation;
pub(super) mod records;
mod reindex;
pub(super) mod sketches;
mod smbh;
pub(super) mod subd;
use parameters::encode_document_parameters;
use preconditions::{
    validate_source_less_auxiliary_geometry, validate_source_less_design_bindings,
    validate_source_less_design_links, validate_source_less_design_ownership,
    validate_source_less_face_keys, validate_source_less_history_graph,
    validate_source_less_procedural_carriers, validate_source_less_recipes,
    validate_source_less_sketch_graph, validate_source_less_topology_tolerances,
};
use presentation::GeneratedDesignRegistry;
use records::{encode_design_bulkstream, encode_design_metastream};
use smbh::encode_smbh;

pub(super) struct Generated {
    pub(super) coverage: ArenaDispositions,
    pub(super) losses: Vec<LossNote>,
    pub(super) regenerated_parameters: BTreeSet<String>,
}

#[cfg(test)]
pub(crate) fn write_new(
    target: &CadIr,
    writer: &mut dyn Write,
) -> Result<(ArenaDispositions, Vec<LossNote>), CodecError> {
    write_with_limits(
        target,
        writer,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .map(|generated| (generated.coverage, generated.losses))
}

/// Write a canonical source-less F3D archive for the currently supported
/// native construction profile, and state which model arenas it carries.
pub(super) fn write_with_limits(
    target: &CadIr,
    writer: &mut dyn Write,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<Generated, CodecError> {
    // `subset_int_cur` states its interval and cache on the parameter of the
    // curve it trims.
    let lowered = cadmpeg_ir::codec::write::subset_lowering::lower_subset_curves(target, |_| true)?;
    let target = lowered.as_ref();
    let loaded_native = f3d_native(target)?;
    validate_assembly_projection(target, loaded_native.as_ref())?;
    let has_native = loaded_native.is_some();
    let native = loaded_native.unwrap_or_default();
    validate_source_less_face_keys(&native)?;
    let attributes = attributes::AttributeIndex::new(target, &native);
    if !target.model.assets.is_empty() {
        return Err(CodecError::UnsupportedContent(
            "source-less F3D generation does not support document assets".into(),
        ));
    }
    validate_source_less_procedural_carriers(target)?;
    validate_source_less_topology_tolerances(target, &native)?;
    validate_source_less_auxiliary_geometry(target)?;
    let act_segments = act::encode(&native)?;
    let design_bindings = validate_source_less_design_bindings(&native)?;
    let mut parameter_bytes = Vec::new();
    let mut parameter_meta = None;
    let mut residual_losses = Vec::new();
    if has_native {
        validate_configuration_projection(target, &native)?;
        validate_source_less_history_graph(target, &native)?;
        if !native.design_parameter_owners.is_empty()
            || !native.design_parameter_companions.is_empty()
        {
            return Err(CodecError::UnsupportedContent(
                "source-less F3D parameter ownership requires a same-segment scope".into(),
            ));
        }
        if !native.design_parameters.is_empty() {
            let collection_index = native
                .design_parameters
                .iter()
                .map(|parameter| parameter.record_index)
                .max()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| {
                    CodecError::InvalidInput("F3D parameter collection index exceeds u32".into())
                })?;
            let mut primary = Vec::new();
            let mut indices = Vec::new();
            for parameter in &native.design_parameters {
                primary.push(crate::metastream::RecordIndexEntry {
                    entity_id: u64::from(parameter.record_index),
                    bulk_offset: u64::try_from(parameter_bytes.len()).map_err(|_| {
                        CodecError::InvalidInput("F3D parameter stream too large".into())
                    })?,
                });
                indices.push(parameter.record_index);
                parameter_bytes.extend_from_slice(&encode_document_parameters(
                    std::slice::from_ref(parameter),
                    collection_index,
                )?);
            }
            primary.push(crate::metastream::RecordIndexEntry {
                entity_id: u64::from(collection_index),
                bulk_offset: u64::try_from(parameter_bytes.len()).map_err(|_| {
                    CodecError::InvalidInput("F3D parameter stream too large".into())
                })?,
            });
            parameters::encode_collection(&mut parameter_bytes, "257", collection_index, &indices)?;
            residual_losses.push(collection_residual(
                "FusionDesignSegmentType2",
                collection_index,
            ));
            let registry = GeneratedDesignRegistry {
                types: [
                    (
                        "7A6A3D31-BE74-4E19-9115-20642944C2E4",
                        "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
                        5,
                        indices.iter().map(|index| u64::from(*index)).collect(),
                    ),
                    (
                        "B0556F58-EE09-46A7-B18F-E296F48E5B13",
                        "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                        3,
                        vec![u64::from(collection_index)],
                    ),
                ]
                .into_iter()
                .map(|(guid, base, version, entity_ids)| {
                    Ok(presentation::GeneratedDesignType {
                        type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
                        base_type_guid: Some(
                            base.to_owned().try_into().map_err(CodecError::Malformed)?,
                        ),
                        version,
                        module: "CommonData".into(),
                        entity_ids,
                    })
                })
                .collect::<Result<_, CodecError>>()?,
                body_map: None,
                browser_nodes: None,
            };
            parameter_meta = encode_design_metastream(&registry, &primary, &[])?;
        }
        validate_source_less_design_ownership(&native)?;
        validate_source_less_sketch_graph(&native)?;
        validate_source_less_recipes(&native)?;
        validate_source_less_design_links(target, &native, &attributes)?;
    }
    let generated_features = features::encode(target)?;
    let mut design_registry = GeneratedDesignRegistry::new(target, design_bindings, &attributes)?;
    if !records::needs_design_bulk(target, &native) {
        if let Some((_, meta, _, _, _)) = &generated_features {
            let generated = crate::metastream::parse(meta, "generated feature MetaStream")?;
            let mut ordinal = 0;
            design_registry.types.retain(|_| {
                let source = native.design_types.get(ordinal);
                ordinal += 1;
                !source.is_some_and(|source| {
                    source
                        .id()
                        .contains("/FusionDesignSegmentType3/MetaStream.dat:")
                        && generated.types.iter().any(|kind| {
                            kind.class_tag == source.class_tag
                                && kind.byte_offset == source.byte_offset
                                && kind.type_guid == source.type_guid
                                && kind.base_type_guid.value() == source.base_type_guid.value()
                                && kind.version == source.version
                                && kind.module == source.module
                                && kind.entities.values().eq(source.entities.values())
                        })
                })
            });
        }
    }
    let brep_name = source_brep_name(&native, design_registry.body_map.is_some());
    let smbh = encode_smbh(target, &native, &attributes, limits)?;
    let mut staged = tempfile::tempfile()?;
    let mut archive = zip::ZipWriter::new(&mut staged);
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    archive
        .start_file("Manifest.dat", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D manifest: {error}"))
        })?;
    archive.write_all(&manifest::generated_top_level()?)?;
    archive
        .start_file("Properties.dat", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D properties: {error}"))
        })?;
    archive.write_all(&0u32.to_le_bytes())?;
    archive
        .start_file(format!("{DESIGN_FOLDER}/Manifest.dat"), options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D asset manifest: {error}"))
        })?;
    archive.write_all(&manifest::generated_design_asset()?)?;
    archive
        .start_file(
            format!("{DESIGN_FOLDER}/Breps.BlobParts/{brep_name}"),
            options,
        )
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D BREP entry: {error}"))
        })?;
    archive.write_all(&smbh)?;
    let subd_arena = cadmpeg_core::decode::DecodeArena::new();
    let subd_policy = cadmpeg_core::decode::DecodePolicy {
        limits,
        ..Default::default()
    };
    let subd_ctx = cadmpeg_core::decode::DecodeContext::for_encode(&subd_arena, &subd_policy);
    let mut subd_names = BTreeSet::new();
    for (ordinal, surface) in target.model.subds.iter().enumerate() {
        let fallback = format!("{DESIGN_FOLDER}/TSplines.BlobParts/TSpline.{ordinal}.tsm");
        let prefix = format!("{DESIGN_FOLDER}/TSplines.BlobParts/");
        let requested = surface
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::F3d)
            .map(|source| source.object_id.as_str())
            .filter(|name| {
                name.strip_prefix(&prefix).is_some_and(|base| {
                    !base.is_empty() && !base.contains(['/', '\\']) && base.ends_with(".tsm")
                })
            })
            .unwrap_or(&fallback);
        let mut name = requested.to_owned();
        let mut discriminator = 0usize;
        while !subd_names.insert(name.clone()) {
            name =
                format!("{DESIGN_FOLDER}/TSplines.BlobParts/TSpline.{ordinal}-{discriminator}.tsm");
            discriminator = discriminator.checked_add(1).ok_or_else(|| {
                CodecError::UnsupportedContent("F3D cage name space exhausted".into())
            })?;
        }
        let Some(program) = subd::encode(&subd_ctx, surface, true)? else {
            return Err(CodecError::UnsupportedContent(format!(
                "F3D cage {} has unsupported topology or controls",
                surface.id
            )));
        };
        let source_key = name
            .strip_prefix(&prefix)
            .and_then(|base| base.strip_suffix(".tsm"))
            .unwrap_or_default();
        if surface.id.as_str() != format!("f3d:tspline:subd#{source_key}")
            || surface.native_ref.is_some()
            || surface.source_object.as_ref().is_none_or(|source| {
                source.object_id.as_str() != name
                    || source.format != cadmpeg_ir::CodecFormat::F3d
                    || source.geometry_role.is_some()
                    || !source.instance_path.is_empty()
            })
        {
            residual_losses.push(crate::loss::F3dLossCode::WriterSubdOmitted.note(format!(
                "subd {} receives a generated F3D cage identity and source association",
                surface.id
            )));
        }
        residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
            "subd {} uses generated T-spline grip weights and declaration fields",
            surface.id
        )));
        if surface.cage.symmetries().iter().any(|symmetry| matches!(
            symmetry.kind(), cadmpeg_ir::subd::SubdSymmetryKind::Radial(radial) if radial.radial_maps().is_empty()
        )) {
            residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
                "subd {} receives generated radial map records", surface.id
            )));
        }
        archive.start_file(&name, options).map_err(|error| {
            CodecError::malformed(format_args!("cannot create T-spline cage: {error}"))
        })?;
        archive.write_all(program.as_bytes())?;
    }
    if has_native {
        let mut configuration_names = BTreeSet::new();
        for configuration in &native.design_configurations {
            if !configuration_names.insert(configuration.entry_name().as_str()) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name()
                )));
            }
            let payload =
                crate::records::configuration::encode_configuration_payload(configuration)?;
            archive
                .start_file(configuration.entry_name(), options)
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "cannot create F3D configuration entry: {error}"
                    ))
                })?;
            archive.write_all(&payload)?;
        }
    }
    if !parameter_bytes.is_empty() {
        archive
            .start_file(
                format!("{DESIGN_FOLDER}/FusionDesignSegmentType2/BulkStream.dat"),
                options,
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D parameter stream: {error}"))
            })?;
        archive.write_all(&parameter_bytes)?;
        if let Some(meta) = parameter_meta {
            archive
                .start_file(
                    format!("{DESIGN_FOLDER}/FusionDesignSegmentType2/MetaStream.dat"),
                    options,
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "cannot create F3D parameter metadata: {error}"
                    ))
                })?;
            archive.write_all(&meta)?;
        }
    }
    for segment in act_segments {
        archive
            .start_file(format!("{}/BulkStream.dat", segment.folder), options)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        archive.write_all(&segment.bulk)?;
        archive
            .start_file(format!("{}/MetaStream.dat", segment.folder), options)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        archive.write_all(&segment.meta)?;
    }
    // The parameter segment owns its independent dynamic class tags. Browser
    // and geometry metadata use the main Design segment's generated registry.
    let design_bulk =
        encode_design_bulkstream(target, &native, &design_registry, Vec::new(), brep_name)?;
    if let Some(bulk_stream) = &design_bulk {
        residual_losses.extend(bulk_stream.losses.iter().cloned());
        archive
            .start_file(format!("{DESIGN_FOLDER}/Design1/BulkStream.dat"), options)
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Design BulkStream: {error}"))
            })?;
        archive.write_all(&bulk_stream.bytes)?;
    }
    let primary_records = design_bulk.as_ref().map_or(&[][..], |bulk_stream| {
        bulk_stream.primary_records.as_slice()
    });
    let design_meta = encode_design_metastream(&design_registry, primary_records, &[])?;
    if let Some(meta_stream) = design_meta {
        if design_bulk.is_none() {
            archive
                .start_file(format!("{DESIGN_FOLDER}/Design1/BulkStream.dat"), options)
                .map_err(|error| CodecError::malformed(error.to_string()))?;
        }
        archive
            .start_file(format!("{DESIGN_FOLDER}/Design1/MetaStream.dat"), options)
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Design MetaStream: {error}"))
            })?;
        archive.write_all(&meta_stream)?;
    }
    let mut written_features = Vec::new();
    for text in &native.sketch_texts {
        residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
            "sketch text {}: generated formatting state uses unspecified defaults",
            text.id
        )));
    }
    let mut written_owned_parameters = Vec::new();
    if let Some((bulk, meta, ids, parameter_ids, collection_index)) = generated_features {
        written_features = ids;
        written_owned_parameters = parameter_ids;
        residual_losses.push(collection_residual(
            "FusionDesignSegmentType3",
            collection_index,
        ));
        for (name, data) in [("BulkStream.dat", bulk), ("MetaStream.dat", meta)] {
            archive
                .start_file(
                    format!("{DESIGN_FOLDER}/FusionDesignSegmentType3/{name}"),
                    options,
                )
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            archive.write_all(&data)?;
        }
    }
    let written_sketches = sketches::encode(target, limits)?;
    if let Some(sketches) = &written_sketches {
        for (name, data) in [
            ("BulkStream.dat", &sketches.bulk),
            ("MetaStream.dat", &sketches.meta),
        ] {
            archive
                .start_file(
                    format!("{DESIGN_FOLDER}/FusionDesignSegmentType4/{name}"),
                    options,
                )
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            archive.write_all(data)?;
        }
        for id in &sketches.sketches {
            residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
                "sketch {id}: generated member-run records have unspecified state fields"
            )));
        }
        for id in &sketches.text_entities {
            residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
                "sketch text {id}: generated glyph color and formatting use unspecified defaults"
            )));
        }
        for id in &sketches.nurbs_entities {
            residual_losses.push(ExportLossCode::RecordResidualUnavailable.note(format!(
                "sketch NURBS {id}: generated fit tolerance uses an unspecified default"
            )));
        }
    }
    for (ordinal, appearance) in target.model.appearances.iter().enumerate() {
        let protein = crate::materials::encode_protein(appearance)?;
        archive
            .start_file(
                format!("{DESIGN_FOLDER}/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"),
                options,
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Protein asset: {error}"))
            })?;
        archive.write_all(&protein)?;
    }
    archive.finish().map_err(|error| {
        CodecError::malformed(format_args!("cannot finish F3D archive: {error}"))
    })?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, writer)?;
    let mut dispositions = coverage(has_native, target, &native);
    if let Some(bulk) = &design_bulk {
        if !bulk.written_materials.is_empty() {
            dispositions.appearance_bindings =
                if bulk.written_materials.len() == target.model.appearance_bindings.len() {
                    ArenaDisposition::Written
                } else {
                    ArenaDisposition::Reported
                };
        }
    }
    let mut omitted_owned_parameters = 0;
    for parameter in target
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_some())
    {
        if !written_owned_parameters.contains(&parameter.id) {
            omitted_owned_parameters += 1;
            residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                format!(
                    "feature-owned parameter {} has no supported F3D encoding",
                    parameter.id
                ),
            ));
        }
    }
    if omitted_owned_parameters != 0 {
        dispositions.parameters = if omitted_owned_parameters == target.model.parameters.len() {
            ArenaDisposition::Dropped
        } else {
            ArenaDisposition::Reported
        };
    }
    if let Some(sketches) = &written_sketches {
        for sketch in &target.model.sketches {
            if !sketches.sketches.contains(&sketch.id) {
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!("sketch {} has no supported F3D encoding", sketch.id),
                ));
            }
        }
        for entity in &target.model.sketch_entities {
            if !sketches.entities.contains(entity.id()) {
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!(
                        "sketch entity {} has no supported F3D encoding",
                        entity.id()
                    ),
                ));
            }
        }
        for constraint in &target.model.sketch_constraints {
            if !sketches.constraints.contains(&constraint.id) {
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!(
                        "sketch constraint {} has no supported F3D encoding",
                        constraint.id
                    ),
                ));
            }
        }
        dispositions.sketch_constraints =
            if sketches.constraints.len() == target.model.sketch_constraints.len() {
                ArenaDisposition::Written
            } else {
                ArenaDisposition::Reported
            };
        dispositions.sketches = if sketches.sketches.len() == target.model.sketches.len() {
            ArenaDisposition::Written
        } else {
            ArenaDisposition::Reported
        };
        dispositions.sketch_entities =
            if sketches.entities.len() == target.model.sketch_entities.len() {
                ArenaDisposition::Written
            } else {
                ArenaDisposition::Reported
            };
    }
    if !written_features.is_empty() {
        let mut partial = false;
        for feature in &target.model.features {
            if !written_features.contains(&feature.id) {
                partial = true;
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!("feature {} has no supported F3D encoding", feature.id),
                ));
                continue;
            }
            if feature.needs_recompute {
                partial = true;
            }
            if matches!(feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Primitive { solid, .. }
            ) if matches!(solid.kind(), cadmpeg_ir::features::PrimitiveSolidKind::Cylinder {
                radius: cadmpeg_ir::features::RadialDimension::Radius(_), ..
            })) {
                partial = true;
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!("feature {} encodes its radius as a diameter", feature.id),
                ));
            }
            if !feature.evaluation.outputs().is_empty() {
                partial = true;
                residual_losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
                    format!(
                        "feature {} retains its operation; its output-body links are not encoded",
                        feature.id
                    ),
                ));
            }
        }
        dispositions.features = if partial {
            ArenaDisposition::Reported
        } else {
            ArenaDisposition::Written
        };
    }
    let regenerated_parameters = target
        .model
        .parameters
        .iter()
        .filter_map(|parameter| {
            (parameter.owner.is_some() && written_owned_parameters.contains(&parameter.id))
                .then(|| parameter.native_ref.clone())
                .flatten()
        })
        .collect();
    Ok(Generated {
        coverage: dispositions,
        losses: residual_losses,
        regenerated_parameters,
    })
}

fn collection_residual(segment: &str, index: u32) -> LossNote {
    ExportLossCode::RecordResidualUnavailable.note(format!(
        "{DESIGN_FOLDER}/{segment}/BulkStream.dat:parameter-collection#{index} has unspecified control and reference fields"
    ))
}

fn source_brep_name(native: &crate::native::F3dNative, requires_body_map: bool) -> &str {
    let mut names = native
        .body_native_keys
        .iter()
        .filter_map(|key| key.source_brep.as_deref());
    let Some(first) = names.next() else {
        return "BREP.generated.smbh";
    };
    if (!requires_body_map || first.starts_with("BREP."))
        && first.ends_with(".smbh")
        && !first.contains(['/', '\\'])
        && first != "."
        && first != ".."
        && names.all(|name| name == first)
    {
        first
    } else {
        "BREP.generated.smbh"
    }
}

/// What [`write_new`] does with each model arena.
/// Assets, tessellations and assembly joints are `Dropped` because non-empty
/// arenas are refused; parameters are also `Dropped` without native Design
/// records because their required projection is empty.
///
/// The generator writes B-rep topology, geometry, procedural carriers and
/// appearances from the neutral model. Configurations and parameters are
/// written from the native Design records after the neutral arenas are proved
/// equal to their projection; without an `f3d` namespace a non-empty parameter
/// arena is refused and configurations are not read. Design history, sketches
/// and the remaining document arenas come only from native records, so the
/// neutral arenas are not represented.
pub(super) fn coverage(
    has_native: bool,
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Omitted, Reported, Written};
    ArenaDispositions {
        bodies: Written,
        regions: Written,
        shells: Written,
        faces: Written,
        loops: Written,
        coedges: Written,
        edges: Written,
        vertices: Written,
        points: Written,
        surfaces: Written,
        curves: Written,
        subds: Written,
        pcurves: Written,
        procedural_surfaces: Written,
        procedural_curves: Written,
        assets: Dropped,
        features: Omitted,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: if has_native { Written } else { Omitted },
        parameters: if has_native { Written } else { Dropped },
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: if native
            .act_component_links
            .iter()
            .filter(|link| link.tracks_document_root())
            .count()
            == 1
            && target.model.product_definitions.len() == 1
        {
            let product = &target.model.product_definitions[0];
            if product.source_name.is_none()
                && product.description.is_none()
                && product.part_number.is_none()
                && product.bom_properties.is_empty()
            {
                Written
            } else {
                Reported
            }
        } else {
            Omitted
        },
        occurrences: Omitted,
        assembly_joints: Dropped,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Dropped,
        appearances: Written,
        appearance_bindings: Omitted,
        attributes: Omitted,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
    }
}
