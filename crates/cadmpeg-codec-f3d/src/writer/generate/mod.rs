// SPDX-License-Identifier: Apache-2.0
//! Source-less F3D archive generation: assemble a ZIP archive from a neutral
//! `CadIr` with no retained source.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::subset_lowering::{lower_subset_curves, SubsetLowering};
use cadmpeg_ir::codec::write::{
    ArenaDisposition, ArenaDispositions, MissingStateForm, NativeDispositions,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::manifest::{self, GENERATED_DESIGN_ASSET_FOLDER as DESIGN_FOLDER};
use crate::writer::primitives::{
    f3d_native, validate_assembly_projection, validate_configuration_projection,
};
mod attributes;
mod design;
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
use presentation::{BodySelection, GeneratedDesignRegistry};
use records::{encode_design_bulkstream, encode_design_metastream};
use reduce::{Failure, Reduction, Stage};
use smbh::encode_smbh;

/// What [`write_new`] wrote: the model arenas it carries and the losses it
/// charged for the content it dropped or degraded.
pub(super) struct Generated {
    pub(super) coverage: ArenaDispositions,
    pub(super) native: NativeDispositions,
    pub(super) losses: Vec<LossNote>,
    /// Whether the file holds the current geometry in the geometry-true
    /// form where the request names the history-true form.
    pub(super) geometry_true: bool,
}

/// Write a source-less F3D archive of everything in `target` the generator
/// can encode, in the form `missing_states` names for the features whose
/// states it cannot write (cadir-v11.md §3.4), and report the rest as losses
/// (see [`reduce`]).
pub(super) fn write_new(
    target: &CadIr,
    missing_states: MissingStateForm,
    writer: &mut dyn Write,
) -> Result<Generated, CodecError> {
    // The history-true form writes the state at its timeline marker, which
    // an evaluation can hold in place of the current state.
    let staged = match missing_states {
        MissingStateForm::HistoryTrue => design::state_at_marker(target),
        MissingStateForm::GeometryTrue => None,
    };
    let (target, staged_loss) = match &staged {
        Some((staged, loss)) => (staged, loss.clone()),
        None => (target, None),
    };
    // `subset_int_cur` states its interval and cache on the parameter of the
    // curve it trims.
    let SubsetLowering {
        ir: lowered,
        unlowered,
    } = lower_subset_curves(target, |_| true);
    let mut reduction = Reduction::new(lowered.as_ref(), missing_states);
    reduction.reduce_unlowered_subsets(unlowered);
    reduction.prepare()?;
    let (mut staged, mut coverage, design_losses, unwritten_native) = loop {
        let mut staged = tempfile::tempfile()?;
        match attempt(
            reduction.ir(),
            reduction.writes_design(),
            reduction.writes_neutral_design(),
            missing_states,
            reduction.fallback(),
            &mut staged,
        ) {
            Ok((coverage, losses, unwritten_native)) => {
                break (staged, coverage, losses, unwritten_native)
            }
            Err(failure) => reduction.degrade(failure, writes_alone)?,
        }
    };
    if reduction.drops_neutral_design() {
        coverage.design = design::DROPPED;
    }
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, writer)?;
    let geometry_true = missing_states == MissingStateForm::HistoryTrue
        && reduction.drops_neutral_design()
        && !reduction.ir().geometry.bodies.is_empty()
        && design::plan(
            reduction.ir(),
            MissingStateForm::HistoryTrue,
            &design::Fallback::default(),
        )
        .ok()
        .flatten()
        .is_some_and(|design| design.rolls_back());
    let (mut losses, mut native) = reduction.finish();
    losses.extend(staged_loss);
    losses.extend(design_losses);
    for arena in unwritten_native {
        native.drop_arena(
            "f3d",
            arena,
            "whose record indices the written Design records reuse",
        );
    }
    Ok(Generated {
        coverage,
        native,
        losses,
        geometry_true,
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
        Stage::Design | Stage::NeutralDesign | Stage::Archive => attempt(
            target,
            false,
            false,
            MissingStateForm::GeometryTrue,
            &design::Fallback::default(),
            &mut std::io::Cursor::new(Vec::new()),
        )
        .is_ok(),
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
/// records are written, and `write_neutral` whether its design scope is, in
/// the form `missing_states` names; `fallback` names the parts of the design
/// scope's form dropped after a failure. A failure names the stage that
/// failed. Returns the arena coverage, the losses of the design scope's
/// records, and the native arenas the written Design records leave out.
fn attempt<W: Write + Seek>(
    target: &CadIr,
    write_design: bool,
    write_neutral: bool,
    missing_states: MissingStateForm,
    fallback: &design::Fallback,
    staged: W,
) -> Result<(ArenaDispositions, Vec<LossNote>, &'static [&'static str]), Failure> {
    #[cfg(test)]
    if let Some(error) = write_neutral.then(design::requested_test_failure).flatten() {
        return Err(Failure::neutral_design(error));
    }
    let mut neutral = if write_neutral {
        design::plan(target, missing_states, fallback).map_err(Failure::neutral_design)?
    } else {
        None
    };
    // On the design scope's path no native record is written, so a Design
    // stream failure is the scope's.
    let design_failure: fn(CodecError) -> Failure = if neutral.is_some() {
        Failure::neutral_design
    } else {
        Failure::design
    };
    let loaded_native = f3d_native(target).map_err(Failure::design)?;
    let has_native = loaded_native.is_some();
    let mut native = loaded_native.unwrap_or_default();
    let stamped = neutral
        .as_ref()
        .is_some_and(design::NeutralDesign::has_bodies);
    let timeline = neutral
        .as_ref()
        .is_some_and(design::NeutralDesign::writes_timeline);
    // The history-true design whose state at the timeline marker holds no
    // body writes no B-rep stream.
    let body_less = timeline && !stamped;
    if let Some(neutral) = neutral.as_mut() {
        neutral
            .stamp(&mut native)
            .map_err(Failure::neutral_design)?;
    }
    if write_design {
        validate_assembly_projection(target, Some(&native)).map_err(Failure::design)?;
    }
    // A Base Feature design keys its history stream's bodies from `0`.
    let attributes = attributes::AttributeIndex::new(target, &native);
    let attributes = if stamped {
        attributes.with_body_keys(target, |ordinal| ordinal)
    } else {
        attributes
    };
    if has_native {
        validate_source_less_design_links(target, &native, &attributes).map_err(Failure::design)?;
    }
    let brep = if body_less {
        Ok(Vec::new())
    } else {
        encode_brep(target, &native, &attributes)
    };
    let smbh = match brep {
        Ok(smbh) => smbh,
        // The design scope's names fail the B-rep stream when it writes
        // without them.
        Err(error)
            if stamped
                && writes_alone(target, &crate::native::F3dNative::default(), Stage::Brep) =>
        {
            return Err(Failure::neutral_design(error));
        }
        Err(error) => return Err(Failure::brep(error)),
    };
    let (smbh, history_notes) = match neutral.as_ref().filter(|_| stamped) {
        Some(neutral) => {
            design::history::with_insertion_history(&smbh, &neutral.history_states(target))
                .map_err(Failure::neutral_design)?
        }
        None => (smbh, Vec::new()),
    };
    let design_bindings = validate_source_less_design_bindings(&native).map_err(Failure::design)?;
    let mut native_parameters = None;
    if write_design {
        let native_design = || -> Result<design::parameters::ParameterSource, CodecError> {
            validate_configuration_projection(target, &native)?;
            validate_source_less_history_graph(target, &native)?;
            let parameters = design::parameters::native_or_neutral(target, &native)?;
            validate_source_less_design_ownership(&native)?;
            validate_source_less_sketch_graph(&native)?;
            validate_source_less_recipes(&native)?;
            Ok(parameters)
        };
        native_parameters = Some(native_design().map_err(Failure::design)?);
    }
    let selection = if neutral.is_some() {
        BodySelection::Every
    } else {
        BodySelection::Stated
    };
    let mut design_registry =
        GeneratedDesignRegistry::new(target, design_bindings, &attributes, selection)
            .map_err(design_failure)?;
    let unstated_visibility = cadmpeg_ir::codec::write::value_unwritable(
        "F3D",
        "writes these bodies as visible: the source did not record their visibility",
        std::mem::take(&mut design_registry.guessed_visible),
    );
    if let Some(mut design) = if timeline { neutral.take() } else { None } {
        let snapshot = if body_less {
            None
        } else {
            Some(snapshot_stream(target, &native).map_err(Failure::neutral_design)?)
        };
        let members = design
            .archive(
                target,
                snapshot.as_deref().map(|snapshot| (&*smbh, snapshot)),
            )
            .map_err(Failure::neutral_design)?;
        write_archive(staged, &ArchiveEntries::BaseFeature { members: &members })
            .map_err(Failure::archive)?;
        let mut coverage = coverage(
            false,
            ArenaDisposition::Reported,
            true,
            ArenaDisposition::Reported,
        );
        // The written `Sketch` items carry the design's sketches; the plan
        // charges each sketch, entity and relation it does not write.
        if design.writes_sketches() {
            coverage.design.sketches = ArenaDisposition::Reported;
            coverage.design.sketch_entities = ArenaDisposition::Reported;
            coverage.design.sketch_constraints = ArenaDisposition::Reported;
        }
        let mut losses = appearance_losses(target);
        losses.extend(design.into_losses());
        losses.extend(history_notes);
        if body_less {
            omit_geometry(&mut coverage);
            losses.push(crate::loss::F3dLossCode::WriterGeometryOmitted.note(format!(
                "the history-true design holds no body at its timeline marker, so the archive stores no B-rep stream and these geometry records were not written: {}",
                reduce::geometry_records_by_arena(&target.geometry).join("; ")
            )));
            return Ok((coverage, losses, BASE_FEATURE_UNWRITTEN_ARENAS));
        }
        losses.extend(unstated_visibility);
        losses.extend(cadmpeg_ir::codec::write::value_unwritable(
            "F3D",
            "writes these bodies as visible: the written Design records hold no body visibility",
            target
                .geometry
                .bodies
                .iter()
                .filter(|body| body.visible == Some(false))
                .map(|body| body.id.to_string()),
        ));
        return Ok((coverage, losses, BASE_FEATURE_UNWRITTEN_ARENAS));
    }
    let mut neutral_records = None;
    let parameter_bytes = match (neutral.as_mut(), &native_parameters) {
        (Some(neutral), _) => {
            let (parameter_bytes, records) = neutral
                .records(&mut design_registry)
                .map_err(Failure::neutral_design)?;
            neutral_records = Some(records);
            parameter_bytes
        }
        (None, Some(source)) => design::parameters::encode(source, &native, &mut design_registry)
            .map_err(Failure::design)?,
        (None, None) => Vec::new(),
    };
    let mut configurations = Vec::new();
    let mut renamed = BTreeMap::<EntryNameFault, Vec<String>>::new();
    let mut inactive = Vec::new();
    if write_design {
        let names = configuration_entry_names(&native.design_configurations);
        for (configuration, (name, fault)) in native.design_configurations.iter().zip(names) {
            if configuration.active_row().is_some() {
                inactive.push(configuration.id());
            }
            let payload =
                crate::records::configuration::encode_configuration_payload(configuration)
                    .map_err(Failure::design)?;
            if let Some(fault) = fault {
                renamed.entry(fault).or_default().push(format!(
                    "{} from {} to {name}",
                    configuration.id(),
                    configuration.entry_name()
                ));
            }
            configurations.push((name, payload));
        }
    }
    let configuration_losses = renamed
        .into_iter()
        .filter_map(|(fault, entries)| {
            cadmpeg_ir::codec::write::value_unwritable("F3D", &fault.reason(), entries)
        })
        .chain(cadmpeg_ir::codec::write::native_records_omitted(
            "F3D",
            "writes no DesignConfigurationRoot record, so no row of these configuration tables \
             is active",
            inactive,
        ))
        .collect::<Vec<_>>();
    let design_bulk = encode_design_bulkstream(
        target,
        &native,
        &design_registry,
        parameter_bytes,
        neutral_records.as_ref(),
    )
    .map_err(design_failure)?;
    let primary_records = design_bulk.as_ref().map_or(&[][..], |bulk_stream| {
        bulk_stream.primary_records.as_slice()
    });
    let meta_stream =
        encode_design_metastream(&design_registry, primary_records).map_err(design_failure)?;
    let proteins = target
        .model
        .appearances
        .iter()
        .map(crate::materials::encode_protein)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Failure::design)?;
    write_archive(
        staged,
        &ArchiveEntries::Generated {
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
    let parameters = match native_parameters {
        Some(design::parameters::ParameterSource::Native) => ArenaDisposition::Written,
        Some(design::parameters::ParameterSource::Neutral(plan)) => {
            let mut losses = plan.into_losses();
            losses.extend(unstated_visibility);
            losses.extend(configuration_losses);
            return Ok((
                coverage(
                    write_design,
                    ArenaDisposition::Reported,
                    false,
                    ArenaDisposition::Written,
                ),
                losses,
                &[],
            ));
        }
        None if neutral.is_some() => ArenaDisposition::Reported,
        None => ArenaDisposition::Dropped,
    };
    let written = neutral.is_some();
    let mut losses = neutral.map_or_else(Vec::new, design::NeutralDesign::into_losses);
    losses.extend(unstated_visibility);
    losses.extend(configuration_losses);
    Ok((
        coverage(write_design, parameters, written, ArenaDisposition::Written),
        losses,
        &[],
    ))
}

/// The snapshot stream of a Base Feature design: the B-rep stream of
/// `target` whose bodies carry no name and the ASM body key `-1`.
fn snapshot_stream(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Result<Vec<u8>, CodecError> {
    let mut unnamed = native.clone();
    unnamed.persistent_design_links.retain(|link| {
        !matches!(
            link.target,
            cadmpeg_ir::attributes::AttributeTarget::Body(_)
        )
    });
    let attributes =
        attributes::AttributeIndex::new(target, &unnamed).with_body_keys(target, |_| -1);
    design::history::snapshot(&encode_brep(target, &unnamed, &attributes)?)
}
/// The native arenas a Base Feature archive leaves out: it writes its own
/// Design records, whose indices these records would reuse (f3d.md §3.1,
/// "Records of a Base Feature design").
const BASE_FEATURE_UNWRITTEN_ARENAS: &[&str] = &[
    "construction_recipes",
    "persistent_references",
    "lost_edge_references",
    "sketch_points",
    "sketch_curve_identities",
    "sketch_relations",
    "sketch_texts",
    "design_body_members",
    "design_entity_headers",
    "design_record_headers",
    "design_material_assignments",
];

/// The losses of the appearances of a Base Feature archive, whose Protein
/// package is the design's own.
fn appearance_losses(target: &CadIr) -> Vec<LossNote> {
    reduce::named_loss(
        crate::loss::F3dLossCode::WriterAppearanceOmitted,
        "appearance",
        "the written design stores the one Protein package its asset manager names",
        target
            .model
            .appearances
            .iter()
            .map(|appearance| appearance.id.to_string()),
    )
    .into_iter()
    .collect()
}

/// The folder of the design asset that holds its configuration documents.
const CONFIGURATION_FOLDER: &str = "DesignConfigurationTable.BlobParts";
/// The largest value of a ZIP u16 length field.
const ZIP_U16_FIELD_MAX: usize = 0xFFFF;
/// The fixed fields of a ZIP local file header, its signature included.
const ZIP_LOCAL_HEADER_BYTES: usize = 30;
/// The longest entry name the archive writer accepts: the `zip` crate
/// writes a local header and its name only when together they are less
/// than the u16 maximum.
const ENTRY_NAME_LIMIT: usize = ZIP_U16_FIELD_MAX - 1 - ZIP_LOCAL_HEADER_BYTES;

/// Why a configuration document is written under another entry name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EntryNameFault {
    /// The name is longer than a ZIP local header holds.
    ContainerWidth,
    /// The name is outside the folder of the written design asset.
    OutsideDesign,
    /// An earlier configuration document has the same name.
    Repeated,
}

impl EntryNameFault {
    fn reason(self) -> String {
        match self {
            Self::ContainerWidth => format!(
                "writes these configuration documents under other entry names: the ZIP \
                 container holds an entry name and its {ZIP_LOCAL_HEADER_BYTES}-byte local \
                 header in fewer than {ZIP_U16_FIELD_MAX} bytes, the maximum of its u16 length \
                 field, so a name is at most {ENTRY_NAME_LIMIT} bytes"
            ),
            Self::OutsideDesign => "writes these configuration documents under other entry \
                 names: the written archive holds one design asset, and an entry outside its \
                 folder is not a document of that asset"
                .to_owned(),
            Self::Repeated => "writes these configuration documents under other entry names: \
                 Fusion reads a repeated configuration entry name as invalid"
                .to_owned(),
        }
    }
}

/// The entry name each configuration document is written under, with the
/// fault of its own name where that differs: its own name where the
/// container holds it in the design folder as the first of its spelling,
/// and otherwise a free name in the design's configuration folder that keeps
/// its base name where it can.
fn configuration_entry_names(
    configurations: &[crate::records::configuration::DesignConfiguration],
) -> Vec<(String, Option<EntryNameFault>)> {
    use crate::records::configuration::DesignConfigurationKind;
    let mut taken = BTreeSet::new();
    let faults = configurations
        .iter()
        .map(|configuration| {
            let name = configuration.entry_name();
            entry_name_fault(name)
                .or_else(|| (!taken.insert(name.clone())).then_some(EntryNameFault::Repeated))
        })
        .collect::<Vec<_>>();
    let mut ordinal = 0usize;
    configurations
        .iter()
        .zip(faults)
        .map(|(configuration, fault)| {
            let Some(fault) = fault else {
                return (configuration.entry_name().clone(), None);
            };
            let base = configuration
                .entry_name()
                .rsplit('/')
                .next()
                .unwrap_or_default();
            let named = format!("{DESIGN_FOLDER}/{CONFIGURATION_FOLDER}/{base}");
            if entry_name_fault(&named).is_none() && taken.insert(named.clone()) {
                return (named, Some(fault));
            }
            let (stem, extension) = match configuration.kind() {
                DesignConfigurationKind::Table => ("DesignConfigurationTable", "dsgcfg"),
                DesignConfigurationKind::Rule => ("DesignConfigurationRule", "dsgcfgrule"),
            };
            loop {
                ordinal += 1;
                let numbered =
                    format!("{DESIGN_FOLDER}/{CONFIGURATION_FOLDER}/{stem}.{ordinal}.{extension}");
                if taken.insert(numbered.clone()) {
                    return (numbered, Some(fault));
                }
            }
        })
        .collect()
}

/// Why `name` cannot be written as an entry of the design asset, apart from
/// a repeat.
fn entry_name_fault(name: &str) -> Option<EntryNameFault> {
    if name.len() > ENTRY_NAME_LIMIT {
        Some(EntryNameFault::ContainerWidth)
    } else if name
        .strip_prefix(DESIGN_FOLDER)
        .and_then(|path| path.strip_prefix('/'))
        .is_none_or(str::is_empty)
    {
        Some(EntryNameFault::OutsideDesign)
    } else {
        None
    }
}

/// The members of a written archive.
enum ArchiveEntries<'a> {
    /// The generic form: the history stream, the configurations, one Design
    /// segment and one Protein package per appearance.
    Generated {
        smbh: &'a [u8],
        configurations: &'a [(String, Vec<u8>)],
        design_bulk: Option<&'a [u8]>,
        meta_stream: Option<&'a [u8]>,
        proteins: &'a [Vec<u8>],
    },
    /// A Base Feature design: the members of the design folder.
    BaseFeature { members: &'a [(String, Vec<u8>)] },
}

fn write_archive<W: Write + Seek>(
    staged: W,
    entries: &ArchiveEntries<'_>,
) -> Result<(), CodecError> {
    let mut archive = zip::ZipWriter::new(staged);
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    // A name ending in `/` is a directory entry.
    let mut entry = |name: String, bytes: &[u8]| -> Result<(), CodecError> {
        let created = if name.ends_with('/') {
            archive.add_directory(name.as_str(), options)
        } else {
            archive.start_file(name.as_str(), options)
        };
        created.map_err(|error| {
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
    match entries {
        ArchiveEntries::Generated {
            smbh,
            configurations,
            design_bulk,
            meta_stream,
            proteins,
        } => {
            entry(
                format!("{DESIGN_FOLDER}/Breps.BlobParts/BREP.generated.smbh"),
                smbh,
            )?;
            for (name, payload) in *configurations {
                entry(name.clone(), payload)?;
            }
            if let Some(bulk_stream) = design_bulk {
                entry(
                    format!("{DESIGN_FOLDER}/Design1/BulkStream.dat"),
                    bulk_stream,
                )?;
            }
            if let Some(meta_stream) = meta_stream {
                entry(
                    format!("{DESIGN_FOLDER}/Design1/MetaStream.dat"),
                    meta_stream,
                )?;
            }
            for (ordinal, protein) in proteins.iter().enumerate() {
                entry(
                    format!(
                        "{DESIGN_FOLDER}/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
                    ),
                    protein,
                )?;
            }
        }
        ArchiveEntries::BaseFeature { members } => {
            for (name, payload) in *members {
                entry(format!("{DESIGN_FOLDER}/{name}"), payload)?;
            }
        }
    }
    archive.finish().map_err(|error| {
        CodecError::malformed(format_args!("cannot finish F3D archive: {error}"))
    })?;
    Ok(())
}

/// Mark every geometry arena of `coverage` reported: the design writes no
/// B-rep stream.
fn omit_geometry(coverage: &mut ArenaDispositions) {
    for arena in [
        &mut coverage.bodies,
        &mut coverage.regions,
        &mut coverage.shells,
        &mut coverage.faces,
        &mut coverage.loops,
        &mut coverage.coedges,
        &mut coverage.edges,
        &mut coverage.vertices,
        &mut coverage.points,
        &mut coverage.surfaces,
        &mut coverage.curves,
        &mut coverage.pcurves,
        &mut coverage.procedural_surfaces,
        &mut coverage.procedural_curves,
    ] {
        *arena = ArenaDisposition::Reported;
    }
}

/// What [`write_new`] does with each model arena.
/// `subds`, assets, tessellations and assembly joints are `Dropped`: the
/// reduction removes them and charges a loss naming each. Configurations are
/// written from the native Design records after the neutral arena is proved
/// equal to its projection; without those records the reduction drops them
/// the same way. `parameters` states what the attempt did with the neutral
/// parameters. Where the root definition's design scope is written
/// (`neutral`), its scopes, containers, features and parameters are
/// `Reported`: the design module charges a loss for each record it does not
/// write. Sketches and the remaining design arenas are not represented.
fn coverage(
    write_design: bool,
    parameters: ArenaDisposition,
    neutral: bool,
    appearances: ArenaDisposition,
) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Ignored, Omitted, Reported, Written};
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
        parameters,
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
        appearances,
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
        design: if neutral {
            cadmpeg_ir::codec::write::DesignArenaDispositions {
                scopes: Reported,
                containers: Reported,
                features: Reported,
                parameters: Reported,
                evaluations: Dropped,
                ..cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted)
            }
        } else {
            cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted)
        },
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
    }
}
