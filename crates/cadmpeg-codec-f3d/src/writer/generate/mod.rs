// SPDX-License-Identifier: Apache-2.0
//! Source-less F3D archive generation: assemble a ZIP archive from a neutral
//! `CadIr` with no retained source.

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use zip::write::SimpleFileOptions;

use crate::writer::primitives::{f3d_native, validate_configuration_projection};
pub(crate) mod attributes;
pub(crate) mod native_bytes;
pub(crate) mod native_geometry;
pub(crate) mod preconditions;
pub(crate) mod records;
pub(crate) mod smbh;
use preconditions::{
    validate_source_less_act, validate_source_less_auxiliary_geometry,
    validate_source_less_design_bindings, validate_source_less_design_links,
    validate_source_less_design_ownership, validate_source_less_history_graph,
    validate_source_less_procedural_carriers, validate_source_less_recipes,
    validate_source_less_sketch_graph, validate_source_less_topology_tolerances,
};
use records::{encode_act_bulkstream, encode_design_bulkstream, encode_design_metastream};
use smbh::encode_planar_triangle_smbh;

/// Write a canonical source-less F3D archive for the currently supported
/// native construction profile.
pub(crate) fn write_new(target: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
    let native = f3d_native(target)?;
    if !target.model.subds.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation does not support SubD surfaces".into(),
        ));
    }
    validate_source_less_procedural_carriers(target)?;
    validate_source_less_topology_tolerances(target)?;
    validate_source_less_auxiliary_geometry(target)?;
    let design_bindings = if let Some(native) = &native {
        validate_configuration_projection(target, native)?;
        validate_source_less_history_graph(target, native)?;
        validate_source_less_act(native)?;
        let design_bindings = validate_source_less_design_bindings(native)?;
        validate_source_less_design_ownership(native)?;
        validate_source_less_sketch_graph(native)?;
        validate_source_less_recipes(native)?;
        validate_source_less_design_links(target, native)?;
        Some(design_bindings)
    } else {
        None
    };
    let smbh = encode_planar_triangle_smbh(target)?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive
        .start_file("Manifest.dat", options)
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D manifest: {error}")))?;
    archive.write_all(b"cadmpeg-generated-f3d")?;
    archive
        .start_file("Properties.dat", options)
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D properties: {error}")))?;
    archive.write_all(&0u32.to_le_bytes())?;
    archive
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh",
            options,
        )
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D BREP entry: {error}")))?;
    archive.write_all(&smbh)?;
    if let Some(native) = &native {
        let mut configuration_names = BTreeSet::new();
        let mut configuration_ids = BTreeSet::new();
        for configuration in &native.design_configurations {
            if !configuration_names.insert(configuration.entry_name.as_str())
                || !configuration_ids.insert(configuration.id.as_str())
            {
                return Err(CodecError::Malformed(format!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name
                )));
            }
            crate::design::configurations::validate_configuration_payload(
                &configuration.entry_name,
                configuration.kind,
                &configuration.payload,
            )?;
            let valid_name = match configuration.kind {
                crate::records::DesignConfigurationKind::Table => {
                    configuration.entry_name.ends_with(".dsgcfg")
                }
                crate::records::DesignConfigurationKind::Rule => {
                    configuration.entry_name.ends_with(".dsgcfgrule")
                }
            };
            if !valid_name {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration kind conflicts with entry name: {}",
                    configuration.entry_name
                )));
            }
            archive
                .start_file(&configuration.entry_name, options)
                .map_err(|error| {
                    CodecError::Malformed(format!("cannot create F3D configuration entry: {error}"))
                })?;
            let payload = serde_json::to_vec(&configuration.payload).map_err(|error| {
                CodecError::Malformed(format!("cannot encode F3D configuration JSON: {error}"))
            })?;
            archive.write_all(&payload)?;
        }
    }
    if let Some(bulk_stream) = encode_design_bulkstream(target)? {
        archive
            .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D Design BulkStream: {error}"))
            })?;
        archive.write_all(&bulk_stream)?;
    }
    if let Some(design_bindings) = design_bindings {
        if let Some(meta_stream) = encode_design_metastream(design_bindings)? {
            archive
                .start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
                .map_err(|error| {
                    CodecError::Malformed(format!("cannot create F3D Design MetaStream: {error}"))
                })?;
            archive.write_all(&meta_stream)?;
        }
    }
    if let Some(act_stream) = encode_act_bulkstream(target)? {
        archive
            .start_file(
                "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
                options,
            )
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D ACT BulkStream: {error}"))
            })?;
        archive.write_all(&act_stream)?;
    }
    for (ordinal, appearance) in target.model.appearances.iter().enumerate() {
        let protein = crate::materials::encode_protein(appearance)?;
        archive
            .start_file(
                format!(
                    "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
                ),
                options,
            )
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D Protein asset: {error}"))
            })?;
        archive.write_all(&protein)?;
    }
    let bytes = archive
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D archive: {error}")))?
        .into_inner();
    writer.write_all(&bytes)?;
    Ok(())
}
