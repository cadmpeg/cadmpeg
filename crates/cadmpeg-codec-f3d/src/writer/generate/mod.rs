// SPDX-License-Identifier: Apache-2.0
//! Source-less F3D archive generation: assemble a ZIP archive from a neutral
//! `CadIr` with no retained source.

use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ArenaDisposition, ArenaDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::manifest::{self, GENERATED_DESIGN_ASSET_FOLDER as DESIGN_FOLDER};
use crate::writer::primitives::{
    f3d_native, validate_assembly_projection, validate_configuration_projection,
};
mod attributes;
mod index;
mod native_bytes;
pub(crate) mod native_geometry;
pub(in crate::writer) mod pcurve_supports;
mod preconditions;
mod presentation;
mod records;
mod reduce;
mod smbh;
use preconditions::{
    validate_source_less_auxiliary_geometry, validate_source_less_design_bindings,
    validate_source_less_design_links, validate_source_less_design_ownership,
    validate_source_less_history_graph, validate_source_less_procedural_carriers,
    validate_source_less_recipes, validate_source_less_sketch_graph,
    validate_source_less_topology_tolerances,
};
use presentation::GeneratedDesignRegistry;
use records::{encode_design_bulkstream, encode_design_metastream, encode_document_parameters};
use reduce::{Failure, Reduction, Stage};
use smbh::encode_smbh;

/// What [`write_new`] wrote: the model arenas it carries and the losses it
/// charged for the content it dropped or degraded.
pub(super) struct Generated {
    pub(super) coverage: ArenaDispositions,
    pub(super) losses: Vec<LossNote>,
}

/// Write a source-less F3D archive of everything in `target` the generator
/// can encode, and report the rest as losses (see [`reduce`]).
pub(super) fn write_new(target: &CadIr, writer: &mut dyn Write) -> Result<Generated, CodecError> {
    // `subset_int_cur` states its interval and cache on the parameter of the
    // curve it trims.
    let lowered = cadmpeg_ir::codec::write::subset_lowering::lower_subset_curves(target, |_| true)?;
    let mut reduction = Reduction::new(lowered.as_ref());
    reduction.prepare()?;
    let (mut staged, coverage) = loop {
        let mut staged = tempfile::tempfile()?;
        match attempt(reduction.ir(), reduction.writes_design(), &mut staged) {
            Ok(coverage) => break (staged, coverage),
            Err(failure) => reduction.degrade(failure, writes_alone)?,
        }
    };
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, writer)?;
    Ok(Generated {
        coverage,
        losses: reduction.finish(),
    })
}

/// Whether `stage` writes `target` (one body of the document, without its
/// native namespace) on its own, with `native` for the B-rep stream.
fn writes_alone(target: &CadIr, native: &crate::native::F3dNative, stage: Stage) -> bool {
    match stage {
        Stage::Brep => {
            let attributes = attributes::AttributeIndex::new(target, native);
            encode_brep(target, native, &attributes).is_ok()
        }
        Stage::Design | Stage::Archive => {
            attempt(target, false, &mut std::io::Cursor::new(Vec::new())).is_ok()
        }
    }
}

fn encode_brep(
    target: &CadIr,
    native: &crate::native::F3dNative,
    attributes: &attributes::AttributeIndex<'_>,
) -> Result<Vec<u8>, CodecError> {
    validate_source_less_procedural_carriers(target)?;
    validate_source_less_topology_tolerances(target, native)?;
    validate_source_less_auxiliary_geometry(target)?;
    encode_smbh(target, native, attributes)
}

/// Write `target` once. `write_design` states whether its native Design
/// records are written; a failure names the stage that failed.
fn attempt<W: Write + Seek>(
    target: &CadIr,
    write_design: bool,
    staged: W,
) -> Result<ArenaDispositions, Failure> {
    let loaded_native = f3d_native(target).map_err(Failure::design)?;
    let has_native = loaded_native.is_some();
    let native = loaded_native.unwrap_or_default();
    if write_design {
        validate_assembly_projection(target, Some(&native)).map_err(Failure::design)?;
    }
    let attributes = attributes::AttributeIndex::new(target, &native);
    if has_native {
        validate_source_less_design_links(target, &native, &attributes).map_err(Failure::design)?;
    }
    let smbh = encode_brep(target, &native, &attributes).map_err(Failure::brep)?;
    let design_bindings = validate_source_less_design_bindings(&native).map_err(Failure::design)?;
    let mut parameter_bytes = Vec::new();
    if write_design {
        let design = || -> Result<Vec<u8>, CodecError> {
            validate_configuration_projection(target, &native)?;
            validate_source_less_history_graph(target, &native)?;
            let parameter_bytes = encode_document_parameters(&native.design_parameters)?;
            validate_source_less_design_ownership(&native)?;
            validate_source_less_sketch_graph(&native)?;
            validate_source_less_recipes(&native)?;
            Ok(parameter_bytes)
        };
        parameter_bytes = design().map_err(Failure::design)?;
    }
    let design_registry = GeneratedDesignRegistry::new(target, design_bindings, &attributes)
        .map_err(Failure::design)?;
    let mut configurations = Vec::new();
    if write_design {
        let mut configuration_names = BTreeSet::new();
        for configuration in &native.design_configurations {
            if !configuration_names.insert(configuration.entry_name().as_str()) {
                return Err(Failure::design(CodecError::malformed(format_args!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name()
                ))));
            }
            if configuration.active_row().is_some() {
                return Err(Failure::design(CodecError::NotImplemented(
                    "source-less F3D generation does not write the Design configuration root record"
                        .into(),
                )));
            }
            let payload =
                crate::records::configuration::encode_configuration_payload(configuration)
                    .map_err(Failure::design)?;
            configurations.push((configuration.entry_name(), payload));
        }
    }
    let design_bulk = encode_design_bulkstream(target, &native, &design_registry, parameter_bytes)
        .map_err(Failure::design)?;
    let primary_records = design_bulk.as_ref().map_or(&[][..], |bulk_stream| {
        bulk_stream.primary_records.as_slice()
    });
    let meta_stream =
        encode_design_metastream(&design_registry, primary_records).map_err(Failure::design)?;
    let proteins = target
        .model
        .appearances
        .iter()
        .map(crate::materials::encode_protein)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Failure::design)?;
    write_archive(
        staged,
        &ArchiveEntries {
            smbh: &smbh,
            configurations: &configurations,
            design_bulk: design_bulk
                .as_ref()
                .map(|bulk_stream| bulk_stream.bytes.as_slice()),
            meta_stream: meta_stream.as_deref(),
            proteins: &proteins,
        },
    )
    .map_err(Failure::archive)?;
    Ok(coverage(write_design))
}

struct ArchiveEntries<'a> {
    smbh: &'a [u8],
    configurations: &'a [(&'a String, Vec<u8>)],
    design_bulk: Option<&'a [u8]>,
    meta_stream: Option<&'a [u8]>,
    proteins: &'a [Vec<u8>],
}

fn write_archive<W: Write + Seek>(
    staged: W,
    entries: &ArchiveEntries<'_>,
) -> Result<(), CodecError> {
    let mut archive = zip::ZipWriter::new(staged);
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut entry = |name: String, bytes: &[u8]| -> Result<(), CodecError> {
        archive
            .start_file(name.as_str(), options)
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D entry {name}: {error}"))
            })?;
        archive.write_all(bytes)?;
        Ok(())
    };
    entry("Manifest.dat".into(), &manifest::generated_top_level()?)?;
    entry("Properties.dat".into(), &0u32.to_le_bytes())?;
    entry(
        format!("{DESIGN_FOLDER}/Manifest.dat"),
        &manifest::generated_design_asset()?,
    )?;
    entry(
        format!("{DESIGN_FOLDER}/Breps.BlobParts/BREP.generated.smbh"),
        entries.smbh,
    )?;
    for (name, payload) in entries.configurations {
        entry((*name).clone(), payload)?;
    }
    if let Some(bulk_stream) = entries.design_bulk {
        entry(
            format!("{DESIGN_FOLDER}/Design1/BulkStream.dat"),
            bulk_stream,
        )?;
    }
    if let Some(meta_stream) = entries.meta_stream {
        entry(
            format!("{DESIGN_FOLDER}/Design1/MetaStream.dat"),
            meta_stream,
        )?;
    }
    for (ordinal, protein) in entries.proteins.iter().enumerate() {
        entry(
            format!("{DESIGN_FOLDER}/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"),
            protein,
        )?;
    }
    archive.finish().map_err(|error| {
        CodecError::malformed(format_args!("cannot finish F3D archive: {error}"))
    })?;
    Ok(())
}

/// What [`write_new`] does with each model arena.
/// `subds`, assets, tessellations and assembly joints are `Dropped`: the
/// reduction removes them and charges a loss naming each. Configurations and
/// parameters are written from the native Design records after the neutral
/// arenas are proved equal to their projection; without those records the
/// reduction drops them the same way. Design history, sketches and the
/// remaining document arenas come only from native records, so the neutral
/// arenas are not represented.
fn coverage(write_design: bool) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Ignored, Omitted, Written};
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
        subds: Dropped,
        pcurves: Written,
        procedural_surfaces: Written,
        procedural_curves: Written,
        assets: Dropped,
        features: Omitted,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: if write_design { Written } else { Dropped },
        parameters: if write_design { Written } else { Dropped },
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: Omitted,
        occurrences: Omitted,
        assembly_joints: Dropped,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Dropped,
        point_clouds: Omitted,
        history_bodies: Ignored,
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
        design: cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted),
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
    }
}
