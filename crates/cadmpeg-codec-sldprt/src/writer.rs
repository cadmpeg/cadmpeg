// SPDX-License-Identifier: Apache-2.0
//! Semantic SLDPRT writer for analytic B-reps.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::num::NonZeroU16;

mod accounting;
mod body_drop;
mod default_properties;
mod edge_direction;
mod envelope;
mod history_drop;
mod materials;
mod metadata_owners;
mod native;
mod ownership;
mod reduce;
pub(crate) mod target;

pub(crate) use accounting::{Held, Unwritten, UnwrittenAttributes};

use crate::native::SldprtNative;
use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{index_from_u32, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::write::capacity::{TextCapacity, TextUnit};
use cadmpeg_ir::codec::write::{ArenaDisposition, ArenaDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{BodyKind, Color, LoopBoundary, Sense};
use cadmpeg_ir::Annotations;

use crate::SourceRecord;

use crate::container::MARKER;
use crate::history::write::xml::valid_xml_name;

const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];
const TYPED_BODY_LINEAR_RESOLUTION: f64 = 1.0e-8;

pub(crate) const SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE: &str = "sldprt_swobjects_local_sha256";
pub(crate) const SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE: &str =
    "sldprt_swobjects_material_local_sha256";
pub(crate) const SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE: &str =
    "sldprt_swobjects_metadata_identity_local_sha256";
pub(crate) const PMI_LOCAL_DIGEST_ATTRIBUTE: &str = "sldprt_pmi_local_sha256";

/// Opaque solved carriers require a retained native partition.
pub(crate) fn requires_native_partition(ir: &CadIr) -> bool {
    ir.geometry.curves.iter().any(|curve| {
        matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        )
    }) || ir.geometry.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        )
    })
}

/// What one semantic write produced.
pub(crate) struct SemanticOutput {
    /// The dialect classified from the final section payloads through the same
    /// `swSolidWorks` envelope parser that decode uses, so a re-decode of the
    /// bytes classifies exactly the dialect stored in the serialized export
    /// report's `identity.target` field.
    pub(crate) dialect: cadmpeg_core::dialect::DialectId,
    /// Which model arenas the written document carries.
    pub(crate) coverage: ArenaDispositions,
    /// The metadata attributes the write did not write, each with why.
    pub(crate) unwritten: UnwrittenAttributes,
    /// The write's other losses.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Writes a semantic document and returns the dialect and arena coverage it
/// wrote.
pub(crate) fn write_semantic_with_records(
    ir: &CadIr,
    annotations: &Annotations,
    retained_records: &[SourceRecord<'_>],
    writer: &mut dyn Write,
) -> Result<SemanticOutput, CodecError> {
    let digest_arena = DecodeArena::new();
    let (digest_ctx, _) =
        DecodeContext::from_root_bytes(&[], &digest_arena, &DecodePolicy::desktop())?;
    let mut native = ir
        .native
        .namespace("sldprt")
        .map(|namespace| SldprtNative::load(namespace).map_err(CodecError::from))
        .transpose()?;
    // A feature history or feature-input lane the input already holds comes
    // from a SolidWorks part; the write generates its own for any other source.
    let solidworks_history = native.as_ref().is_some_and(|native| {
        !native.feature_histories.is_empty() || !native.feature_input_lanes.is_empty()
    });
    let input_active_configuration = active_configuration_intent(ir);
    let source_features = ir
        .model
        .features
        .iter()
        .map(|feature| native::from_ir::SourceFeature {
            id: feature.id.as_str().to_owned(),
            outputs: feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| body.as_str().to_owned())
                .collect(),
        })
        .collect::<Vec<_>>();
    let mut normalized = ir.clone();
    normalized.model.configurations = configurations_without_synthesized_snapshot(&normalized);
    sort_arenas(&mut normalized);
    let mut content_losses =
        reduce::unwritable_content(&mut normalized, !retained_records.is_empty())?;
    content_losses.extend(reduce::unwritable_carriers(&mut normalized, false)?);
    crate::writer_transform::bake(&mut normalized)?;
    sort_arenas(&mut normalized);
    let assigned = assign_configuration_indices(&mut normalized.model.configurations)?;
    if !assigned.is_empty() {
        content_losses.push(
            crate::loss::SldprtLossCode::WriterConfigurationIdentityAssigned.note(format!(
            "{} configuration name(s) or partition index(es) SLDPRT cannot store as stated are \
             written as the writer assigned them: {}",
            assigned.len(),
            assigned.join("; ")
        )),
        );
    }
    let source_arena = DecodeArena::new();
    let source_context = source_image(retained_records)
        .map(|bytes| DecodeContext::from_root_bytes(bytes, &source_arena, &DecodePolicy::desktop()))
        .transpose()?;
    let source_scan = source_context
        .as_ref()
        .map(|(ctx, root)| crate::container::scan(ctx, *root))
        .transpose()?;
    if source_scan.as_ref().is_some_and(|scan| {
        scan.sections()
            .any(|section| section.ps_auxiliary_refusal().is_some())
    }) {
        return Err(CodecError::NotImplemented(
            "SLDPRT semantic writes require parsed retained auxiliary Parasolid owners".into(),
        ));
    }
    let member_records = source_scan
        .as_ref()
        .map(|scan| member_records(scan, retained_records, annotations))
        .transpose()?
        .flatten();
    let (retained_records, annotations): (&[SourceRecord<'_>], &Annotations) = match &member_records
    {
        Some((records, annotations)) => (records.as_slice(), annotations),
        None => (retained_records, annotations),
    };
    let mut source_baseline = None;
    let mut unwritten = UnwrittenAttributes::default();
    let mut losses = content_losses;
    // `export:value.unwritable` entries: texts cut to their field's width.
    let mut cut = Vec::new();
    for (id, held) in default_properties::unwritten(&normalized, retained_records, annotations)? {
        unwritten.record(&id, Unwritten::SavedProperty, held);
    }
    let retained_partition =
        retained_partition(&normalized, source_scan.as_ref(), native.as_ref())?;
    // The IR stores canonical millimetres; Parasolid stores metres.
    let length_scale = 0.001;
    let patched_partition = if retained_partition.is_none() {
        crate::writer_patch::patch_partition(
            &normalized,
            annotations,
            retained_records,
            length_scale,
        )?
    } else {
        None
    };
    let retain_native_brep = retained_partition.is_some() || patched_partition.is_some();
    if !retain_native_brep {
        losses.extend(reduce::unwritable_carriers(&mut normalized, true)?);
    }
    losses.extend(history_drop::foreign_native_operations(&mut normalized));
    let reducible = history_drop::HistoryDrop::applies(&normalized);
    let mut displayed_values = history_drop::DisplayedValues::default();
    if reducible {
        history_drop::clear_foreign_native_refs(&mut normalized, native.as_ref());
        let (displayed, notes) = history_drop::equations_as_displayed_values(&mut normalized);
        displayed_values = displayed;
        losses.extend(notes);
    }
    let mut dropped_history = history_drop::HistoryDrop::default();
    let feature_input_renames = loop {
        let snapshot = reducible.then(|| (normalized.model.parameters.clone(), native.clone()));
        let mut attempt_cut = Vec::new();
        match prepare_history(
            &mut normalized,
            &mut native,
            annotations,
            retained_records,
            &mut attempt_cut,
        ) {
            Ok(renames) => {
                cut.extend(attempt_cut);
                break renames;
            }
            Err(error) => {
                let Some((parameters, source_native)) = snapshot else {
                    return Err(error);
                };
                normalized.model.parameters = parameters;
                native = source_native;
                dropped_history.degrade(&mut normalized, error)?;
            }
        }
    };
    losses.extend(dropped_history.finish());
    losses.extend(displayed_values.note(&normalized));
    let ir = &normalized;
    let validation = cadmpeg_ir::validate::validate_neutral(ir, Vec::new())?;
    losses.extend(reduce::invalid_records(ir, &validation));
    if !retain_native_brep {
        losses.extend(
            reduce::edge_ranges_omitted(ir, annotations).map_err(CodecError::ResourceLimit)?,
        );
    }
    losses.extend(reduce::empty_output(ir));
    let brep_empty = ir.geometry.faces.is_empty();
    let coverage = semantic_coverage(retained_partition.is_some());
    let mut bodiless_partitions = Vec::new();
    let mut encodable = None;
    let partition_sections = if let Some(retained) = retained_partition {
        vec![retained]
    } else if let Some(patched) = patched_partition {
        vec![patched]
    } else if brep_empty {
        losses.extend(reduce::faceless_bodies(ir));
        if source_scan
            .as_ref()
            .is_some_and(|scan| crate::container::select_active_parasolid_site(scan).is_none())
        {
            // The source holds no partition either.
            Vec::new()
        } else {
            empty_partitions(ir, &mut bodiless_partitions)?
        }
    } else {
        let encoded = body_drop::encode_without_unwritable_bodies(ir, |ir| {
            generated_partitions(ir, length_scale)
        })?;
        losses.extend(encoded.losses);
        let written = encoded.reduced.as_ref().unwrap_or(ir);
        losses.extend(reduce::cache_tolerances_omitted(written));
        losses.extend(reduce::unchained_face_loops(written));
        encodable = encoded.reduced;
        let (partitions, bodiless) = encoded.partitions;
        bodiless_partitions.extend(bodiless);
        partitions
    };
    let ir = encodable.as_ref().unwrap_or(ir);
    losses.extend(cadmpeg_ir::codec::write::record_residual_unavailable(
        "SLDPRT",
        "writes each of these partitions, which hold no body, as a partition and a deltas \
         stream of the schema header alone; sldprt.md states no form for a partition with no body",
        bodiless_partitions,
    ));
    let mut generated_partition_sections = partition_sections
        .iter()
        .map(|(section, _)| section.clone())
        .collect::<HashSet<_>>();
    let mut sections = partition_sections;
    let materials = materials::materials_payload(ir);
    let owners = metadata_owners::metadata_owners(
        ir,
        retained_records,
        annotations,
        source_scan.as_ref(),
        &generated_partition_sections,
    );
    let mut retained_swobjects = owners.patchable.clone();
    let mut members = MetadataMembers {
        originals: owners.patchable.iter().cloned().collect(),
        written: BTreeMap::new(),
    };
    let replay_swobjects = if owners.replay() {
        let baseline = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE));
        let current = swobjects_local_sha256(&digest_ctx, ir)?;
        if baseline != Some(&current) {
            let material_baseline = ir.source.as_ref().and_then(|source| {
                source
                    .attributes
                    .get(SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE)
            });
            if material_baseline != Some(&swobjects_material_local_sha256(&digest_ctx, ir)?) {
                let baseline =
                    retained_source_baseline(&mut source_baseline, source_context.as_ref())?;
                losses.extend(materials::retained_materials_kept(
                    ir,
                    baseline.map(|baseline| &baseline.ir),
                ));
            }
            members.written = patch_retained_swobjects_metadata(
                ir,
                annotations,
                &owners,
                &mut retained_swobjects,
                length_scale,
                &mut unwritten,
            );
        }
        for attribute in metadata_attributes(ir) {
            if attribute.name != "source_linear_unit_code"
                && annotations
                    .provenance
                    .get(attribute.id.as_str())
                    .is_some_and(|provenance| owners.unavailable.contains(provenance.stream()))
            {
                unwritten.record(
                    attribute.id.as_str(),
                    Unwritten::OwnerUnavailable,
                    Held::Nothing,
                );
            }
        }
        true
    } else {
        false
    };
    let (objects, units) =
        metadata_payloads(ir, length_scale, replay_swobjects, &mut unwritten, &mut cut);
    if replay_swobjects {
        generated_partition_sections
            .extend(retained_swobjects.iter().map(|(name, _)| name.clone()));
        for (name, payload) in retained_swobjects {
            members.push(&mut sections, &mut unwritten, name, payload);
        }
    } else {
        if !materials.bytes.is_empty() {
            sections.push(("SWObjects".into(), materials.bytes));
        }
        cut.extend(materials.cut);
        losses.extend(materials.omitted);
        if !objects.is_empty() {
            sections.push(("SWObjects/DocumentMetadata".into(), objects));
        }
    }
    if let Some(units) = units {
        losses.push(crate::loss::SldprtLossCode::WriterUnitsMemberGenerated.note(
            "the document's linear unit code is written as SW_UnitsLinear in a generated Units \
             member, a member no SolidWorks file is known to hold; SolidWorks files state it as \
             SW_UnitsLinear of the default saved-property carrier",
        ));
        members.push(&mut sections, &mut unwritten, "Units".into(), units);
    }
    if !ir.model.tessellations.is_empty() {
        let baseline = retained_source_baseline(&mut source_baseline, source_context.as_ref())?;
        if let Some(retained) = retained_display_sections(ir, baseline, source_scan.as_ref())? {
            for (section, payload) in retained {
                members.push(&mut sections, &mut unwritten, section, payload);
            }
        } else {
            let (payload, omitted) = tessellation_payload(ir, length_scale)?;
            if !omitted.is_empty() {
                losses.push(
                    crate::loss::SldprtLossCode::WriterTessellationMetadataOmitted.note(format!(
                        "{} tessellation(s) state auxiliary channels other than the three the \
                         SLDPRT display-list table carries after the strip, position and normal \
                         channels; they are written with the table's default channels: {}",
                        omitted.len(),
                        omitted.join(", ")
                    )),
                );
            }
            members.push(
                &mut sections,
                &mut unwritten,
                "Contents/DisplayLists".into(),
                payload,
            );
        }
    }
    let retained_histories = if native
        .as_ref()
        .is_some_and(|native| !native.feature_histories.is_empty())
    {
        let baseline = retained_source_baseline(&mut source_baseline, source_context.as_ref())?;
        match (baseline, source_scan.as_ref()) {
            (Some(baseline), Some(scan)) => retained_history_sections(baseline, scan)?,
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    for (index, history) in native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .enumerate()
    {
        let section = annotations.provenance.get(&history.id).map_or_else(
            || format!("Contents/Keywords-{index}"),
            |provenance| provenance.stream().to_owned(),
        );
        members.push(
            &mut sections,
            &mut unwritten,
            section.clone(),
            retained_history_payload(history, &section, &retained_histories)?,
        );
    }
    for lane in native.iter().flat_map(|native| &native.feature_input_lanes) {
        let section = match &lane.configuration {
            Some(configuration)
                if crate::resolved_features::assembly::is_supplemental_config_lane(lane) =>
            {
                format!("Contents/Config-{configuration}")
            }
            Some(configuration) => format!("Contents/Config-{configuration}-ResolvedFeatures"),
            None => annotations.provenance.get(&lane.id).map_or_else(
                || "Contents/ResolvedFeatures".into(),
                |provenance| provenance.stream().to_owned(),
            ),
        };
        let histories = native
            .as_ref()
            .map_or(&[][..], |native| native.feature_histories.as_slice());
        members.push(
            &mut sections,
            &mut unwritten,
            section,
            resolved_feature_payload(lane, histories, &feature_input_renames)?,
        );
    }
    let patch_active_configuration = if input_active_configuration.is_some() {
        let baseline = retained_source_baseline(&mut source_baseline, source_context.as_ref())?;
        baseline.is_none_or(|baseline| {
            active_configuration_intent(&baseline.ir) != input_active_configuration
        })
    } else {
        false
    };
    let mut emitted_sections = HashMap::<String, usize>::new();
    for (name, _) in &sections {
        *emitted_sections.entry(name.clone()).or_default() += 1;
    }
    let opaque = opaque_blocks(
        ir,
        retained_records,
        annotations,
        &generated_partition_sections,
        &emitted_sections,
        retain_native_brep,
        patch_active_configuration,
    )?;
    let document_envelope = crate::container::first_solidworks_envelope(
        opaque.blocks.iter().map(|(_, payload)| payload.as_slice()),
    );
    let skeleton = retained_records.is_empty() && {
        let kept = ir
            .model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<HashSet<_>>();
        let left_out = source_features
            .iter()
            .filter(|feature| !kept.contains(feature.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let written = native_skeleton(
            ir,
            native.as_ref(),
            solidworks_history,
            &left_out,
            &mut sections,
        );
        let skeleton = written.is_ok();
        losses.extend(written.unwrap_or_else(|reason| {
            vec![crate::loss::SldprtLossCode::WriterNativeSkeletonOmitted
                .note(format!("the native skeleton is not written: {reason}"))]
        }));
        skeleton
    };
    if let Some(active) = ir.model.configurations.iter().find(|value| value.active) {
        let active_name = active.name.as_deref().ok_or_else(|| {
            CodecError::Malformed("active SLDPRT configuration has no resolved name".into())
        })?;
        if document_envelope.is_none() && !skeleton {
            members.push(
                &mut sections,
                &mut unwritten,
                "Contents/SolidWorks".into(),
                generated_solidworks_xml(ir, active_name),
            );
        }
    }
    for (section, payload) in opaque.blocks {
        members.push(&mut sections, &mut unwritten, section, payload);
    }
    let written_declaration = document_envelope.and_then(|envelope| envelope.sw_version);
    let written_dialect =
        crate::dialect::SldprtDialect::from_declaration(written_declaration.as_deref()).id();

    let type_ids = section_type_ids(source_scan.as_ref(), &sections);
    let envelope = envelope::build(&sections, &type_ids, source_scan.as_ref())?;
    writer.write_all(&envelope.bytes)?;
    losses.extend(envelope.losses());
    let written_streams = sections
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(generated_partition_sections.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    losses.extend(cadmpeg_ir::codec::write::native_records_omitted(
        "SLDPRT",
        "because a native envelope holds no compound-file stream",
        retained_records
            .iter()
            .filter(|record| {
                record
                    .id
                    .as_str()
                    .starts_with("sldprt:file:compound-stream#")
            })
            .filter(|record| {
                annotations
                    .provenance
                    .get(record.id.as_str())
                    .is_none_or(|provenance| !written_streams.contains(provenance.stream()))
            })
            .map(|record| record.id.as_str()),
    ));
    losses.extend(cadmpeg_ir::codec::write::value_unwritable(
        "SLDPRT",
        "cuts these texts at a character boundary to their field's width",
        cut,
    ));
    losses.extend(cadmpeg_ir::codec::write::native_records_omitted(
        "SLDPRT",
        "because the write generates fewer members of their stream than the source stores",
        opaque.replaced,
    ));
    losses.extend(cadmpeg_ir::codec::write::native_records_omitted(
        "SLDPRT",
        "because the source fidelity holds no bytes of them",
        opaque.unavailable,
    ));
    Ok(SemanticOutput {
        dialect: written_dialect,
        coverage,
        unwritten,
        losses,
    })
}

/// The `ResolvedFeatures` stream the skeleton's default tree opens, ahead of
/// a generated sketch lane already written there.
const SKELETON_RESOLVED: &str = "Contents/Config-0-ResolvedFeatures";

/// Add the native object structure of a generated part (`writer::native`)
/// to `sections`. A generated sketch lane already in
/// [`SKELETON_RESOLVED`] follows the default tree there. Returns the losses
/// the skeleton states, or why it is not written; `sections` is then
/// unchanged.
///
/// `left_out` holds the source features the write leaves out.
/// `solidworks_history` states that the input holds a feature history or
/// lane of a `SolidWorks` part. Its lane's class tags index the class map of
/// that part's `Contents/Config-0`, which a write without the source's
/// bytes does not hold; a skeleton `Contents/Config-0` would give them
/// another map, and its tree would stand in front of the lane's objects.
fn native_skeleton(
    ir: &CadIr,
    native: Option<&SldprtNative>,
    solidworks_history: bool,
    left_out: &[native::from_ir::SourceFeature],
    sections: &mut Vec<(String, Vec<u8>)>,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, String> {
    if solidworks_history {
        return Err(
            "the document's feature history or feature-input lanes come from a \
                    SolidWorks part, whose lanes refer to the class map of a Contents/Config-0 \
                    stream the write does not hold"
                .into(),
        );
    }
    let name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_name"))
        .map(|name| native_part_name(name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Part1".to_owned());
    let active = ir
        .model
        .configurations
        .iter()
        .find(|value| value.active)
        .and_then(|value| value.name.clone())
        .unwrap_or_else(|| "Default".to_owned());
    // The skeleton's object identifiers follow the history summary's, so the
    // decoder binds no summary feature to a skeleton object.
    let id_base = native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .filter_map(crate::records::Feature::source_value)
        .max()
        .unwrap_or_default();
    let (features, unwritten_extrusion, lane_reason) =
        match native::from_ir::features(ir, native, left_out).map_err(|error| error.to_string())? {
            native::from_ir::Features::Native {
                features,
                unwritten_extrusion,
            } => (features, unwritten_extrusion, None),
            native::from_ir::Features::Lane(reason) => (Vec::new(), None, Some(reason)),
        };
    // The body a native extrusion makes is not also written as a stored body.
    let extruded = features
        .iter()
        .any(|feature| matches!(feature, native::Feature::Extrusion(_)));
    let stored = if extruded {
        0
    } else {
        ir.geometry.bodies.len()
    };
    let streams = ir
        .geometry
        .bodies
        .iter()
        .take(stored)
        .enumerate()
        .map(|(index, body)| {
            native::Body::numbered(&name, id_base, index, body.kind == BodyKind::Sheet)
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|bodies| {
            native::skeleton(&native::Facts {
                id_base,
                name: &name,
                active: &active,
                features: &features,
                bodies: &bodies,
            })
        })
        .map_err(|error| error.to_string())?;
    let native_features = features
        .iter()
        .map(|feature| match feature {
            native::Feature::Sketch(sketch) => sketch.feature_id,
            native::Feature::Extrusion(extrusion) => extrusion.feature_id,
        })
        .collect::<HashSet<_>>();
    if let Some((stream, _)) = streams.iter().find(|(stream, _)| {
        stream != SKELETON_RESOLVED && sections.iter().any(|(candidate, _)| candidate == stream)
    }) {
        return Err(format!("the write already holds a {stream} stream"));
    }
    let mut lane_merged = false;
    for (stream, payload) in streams {
        if let Some((_, existing)) = sections
            .iter_mut()
            .find(|(candidate, _)| *candidate == stream)
        {
            // The generated lane holds the sketches' marker records; the
            // tree holds them as native sketches when it writes them.
            let mut merged = payload;
            if lane_reason.is_some() {
                merged.append(existing);
                lane_merged = true;
            }
            *existing = merged;
        } else {
            sections.push((stream, payload));
        }
    }
    let summaries = sections
        .iter()
        .map(|(stream, _)| stream.as_str())
        .filter(|stream| stream.starts_with("Contents/Keywords"))
        .collect::<Vec<_>>();
    let summarized_features = ir
        .model
        .features
        .iter()
        .filter(|feature| !native_feature_written(native, feature, &native_features))
        .map(|feature| feature.id.as_str().to_owned())
        .collect::<Vec<_>>();
    let summarized = (!summaries.is_empty() && !summarized_features.is_empty()).then(|| {
        (
            summaries.join(", "),
            summarized_features,
            unwritten_extrusion,
        )
    });
    Ok(native_skeleton_losses(
        summarized,
        lane_reason.filter(|_| lane_merged),
        !features.is_empty(),
        extruded.then_some(left_out),
    ))
}

/// Whether the native tree writes `feature`: its history record's source id
/// is one of `written`.
fn native_feature_written(
    native: Option<&SldprtNative>,
    feature: &cadmpeg_ir::features::Feature,
    written: &HashSet<u32>,
) -> bool {
    let record = feature.native_ref.clone().unwrap_or_else(|| {
        crate::history::write::features::generated_feature_record_id(&feature.id)
    });
    native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .find(|candidate| candidate.id == record)
        .and_then(crate::records::Feature::source_value)
        .is_some_and(|source| written.contains(&source))
}

/// The part name a `sw_name` attribute states: its last path component
/// without the `.sldprt` extension.
fn native_part_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base
        .len()
        .checked_sub(".sldprt".len())
        .filter(|&cut| base.is_char_boundary(cut) && base[cut..].eq_ignore_ascii_case(".sldprt"))
        .map_or(base, |cut| &base[..cut]);
    stem.to_owned()
}

/// The losses of a written native skeleton. `summarized` holds the history
/// summary streams and the features written only in them, when the write
/// holds one. `features` states that the tree writes native sketches;
/// `extruded` holds, when it writes a native extrusion, the source features
/// the write leaves out.
fn native_skeleton_losses(
    summarized: Option<(String, Vec<String>, Option<String>)>,
    lane_follows: Option<String>,
    features: bool,
    extruded: Option<&[native::from_ir::SourceFeature]>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::loss::SldprtLossCode;
    let mut losses = Vec::new();
    if let Some((streams, features, unwritten_extrusion)) = summarized {
        losses.push(SldprtLossCode::WriterNativeHistorySummary.note(format!(
            "{} source feature(s) are written only in the cadmpeg history summary {streams}, \
             a stream SolidWorks parts do not hold; the native feature tree holds the default \
             tree, the features it writes natively and one moBaseBody_c stored body per body it \
             makes no extrusion of: {}{}",
            features.len(),
            features.join(", "),
            unwritten_extrusion
                .map(|reason| format!("; the tree writes no extrusion natively because {reason}"))
                .unwrap_or_default()
        )));
    }
    let (runs, bytes) = native::detailing_zero_runs();
    losses.extend([
        SldprtLossCode::WriterNativeSkeletonUnlaidRuns.note(format!(
            "the native skeleton writes as zero bytes the byte runs whose fields sldprt.md does \
             not lay out, and native parts of swVersion 14000 hold values in most of them: {runs} \
             runs ({bytes} bytes) of the moRelMgr_c detailing settings (EV-19), {}{}; \
             swXmlContents/Features declares no swVersion, so the part names 14000 only in the \
             _MO_VERSION_14000 storage name and in moPart_c's version word",
            native::OTHER_ZERO_RUNS,
            if features {
                format!("; {}", native::sketch::FEATURE_ZERO_RUNS)
            } else {
                String::new()
            }
        )),
        SldprtLossCode::WriterNativeStreamsOmitted.note(format!(
            "the native skeleton leaves out these streams, which every native part of \
             swVersion 14000 holds: {}; and the streams native parts hold in some files \
             (PreviewPNG, Contents/Config-0-GhostPartition, swXmlContents/MATERIALTREE, \
             swXmlContents/Tables, the ThirdPty and _DL_VERSION streams), and the object \
             streams of configurations other than the first",
            native::OMITTED_STREAMS.join(", ")
        )),
    ]);
    if let Some(reason) = lane_follows {
        losses.push(SldprtLossCode::WriterNativeSketchLaneOrphan.note(format!(
            "the generated sketch marker records follow the native feature tree in \
             Contents/Config-0-ResolvedFeatures with no owning moProfileFeature_c object, so an \
             exact walk of the stream stops at the first of them; the tree writes no sketch \
             natively because {reason}"
        )));
    }
    if features {
        losses.push(SldprtLossCode::WriterNativeSketchProfileBodyKept.note(
            "each native sketch writes, after its u32 3, flag byte 0, its profile body and no \
             edge reference, a form no native sketch of swVersion 14000 holds: a native top-level \
             sketch writes flag byte 1 and no body, and a sketch written in place writes flag \
             byte 0, the body and one or more edge references; the body stays because cadmpeg \
             reads a written sketch's geometry and placement from it",
        ));
    }
    let Some(left_out) = extruded else {
        return losses;
    };
    if !left_out.is_empty() {
        losses.push(SldprtLossCode::WriterBodiesOmitted.note(format!(
            "the native extrusion makes the written part's one body; {} source feature(s) the \
             write leaves out made only bodies it omits with them, which the native tree does \
             not hold: {}",
            left_out.len(),
            left_out
                .iter()
                .map(|feature| format!("{} made {}", feature.id, feature.outputs.join(", ")))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    losses.push(SldprtLossCode::WriterNativeBodyIdentityOmitted.note(
        "the native extrusion names no face of the body it makes: its moPerBodyChooserData_c \
         holds no face reference, where native parts of swVersion 14000 hold two whose hops name \
         the extrusion and a sketch entity, and the partition's faces carry no FACE_ID_2001 \
         attribute naming the feature that made them; its moBBoxCenterData_c writes a zero point, \
         a zero length and identifier 0, where native parts hold the centre of the body's box, a \
         non-zero length and a non-zero identifier",
    ));
    losses
}

/// What the semantic writer does with each model arena.
/// `subds` are `Dropped`: `WriterSubdOmitted` names every subdivision surface.
///
/// `retained_brep` states that the retained Parasolid partition was replayed
/// because `brep_local_sha256` still matches. Only that partition carries
/// pcurves and procedural constructions: the generated and patched partitions
/// do not encode them. `pmi` is `Reported`: the retained `PMISemanticDataDB`
/// block carries the PMI its baseline digests, `WriterPmiOmitted` names PMI
/// with no such block or baseline, and `export:edit.dropped` names PMI that
/// differs from its baseline. Design
/// records reach the output through the native history lanes, synchronized
/// from the neutral arenas declared `Written`; the other document arenas are
/// not read.
/// `appearances`, `appearance_bindings` and `attributes` are `Reported` because
/// `WriterUnboundAppearancesOmitted`, `WriterAppearanceBindingsOmitted`, and
/// `WriterAttributesOmitted` with `export:edit.dropped` account for their
/// withheld records, respectively.
/// `curves` is `Reported` unless `retained_brep` is true because
/// `WriterSphereSeamCurvesOmitted` accounts for withheld degenerate sphere-seam curves.
/// `tessellations` stays `Written`: every mesh is written, and
/// `WriterTessellationMetadataOmitted` charges only fields of written meshes.
fn semantic_coverage(retained_brep: bool) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Ignored, Omitted, Reported, Written};
    let brep_carried = if retained_brep { Written } else { Omitted };
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
        curves: if retained_brep { Written } else { Reported },
        subds: Dropped,
        pcurves: brep_carried,
        procedural_surfaces: brep_carried,
        procedural_curves: brep_carried,
        assets: Omitted,
        features: Written,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Written,
        parameters: Written,
        sketches: Written,
        sketch_entities: Written,
        sketch_constraints: Written,
        spatial_sketches: Written,
        spatial_sketch_entities: Written,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: Omitted,
        occurrences: Omitted,
        assembly_joints: Omitted,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Written,
        point_clouds: Omitted,
        history_bodies: Ignored,
        appearances: Reported,
        appearance_bindings: Reported,
        attributes: Reported,
        pmi: Reported,
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

/// Give each configuration the distinct name and partition index SLDPRT
/// stores, and return an entry for each one the writer assigned.
fn assign_configuration_indices(
    configurations: &mut [cadmpeg_ir::features::parameters::DesignConfiguration],
) -> Result<Vec<String>, CodecError> {
    let mut assigned = Vec::new();
    let mut used = HashSet::new();
    let mut names = HashSet::new();
    for (position, configuration) in configurations.iter_mut().enumerate() {
        let stated = configuration
            .name
            .clone()
            .filter(|name| !name.trim().is_empty());
        let base = stated
            .clone()
            .unwrap_or_else(|| format!("Configuration {}", position + 1));
        let mut name = base.clone();
        let mut copy = 2_usize;
        while names.contains(&name) {
            name = format!("{base} ({copy})");
            copy += 1;
        }
        if stated.as_deref() != Some(name.as_str()) {
            assigned.push(format!(
                "{} name {:?} written as {name:?}",
                configuration.id.as_str(),
                configuration.name.as_deref().unwrap_or_default()
            ));
            configuration.name = Some(name.clone());
        }
        names.insert(name);
        if let Some(index) = configuration.source_index {
            if !used.insert(index) {
                assigned.push(format!(
                    "{} source index {index} repeats another configuration's and is reassigned",
                    configuration.id.as_str()
                ));
                configuration.source_index = None;
            }
        }
    }
    let mut positions = (0..configurations.len()).collect::<Vec<_>>();
    positions.sort_by_key(|position| configurations[*position].ordinal);
    let mut next = 0;
    for position in positions {
        if configurations[position].source_index.is_none()
            && configurations[position].bodies.as_deref().is_some()
        {
            configurations[position].source_index =
                Some(reserve_configuration_index(&mut used, &mut next)?);
        }
    }
    Ok(assigned)
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
pub(crate) fn configurations_without_synthesized_snapshot(
    ir: &CadIr,
) -> Vec<cadmpeg_ir::features::parameters::DesignConfiguration> {
    let mut configurations = ir.model.configurations.clone();
    let Some(id) = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_snapshot_synthesized")
            .cloned()
    }) else {
        return configurations;
    };
    for configuration in &mut configurations {
        if configuration.id.as_str() == id {
            configuration.feature_states.clear();
            configuration.parameter_values.clear();
        }
    }
    configurations
}

fn sort_arenas(ir: &mut CadIr) {
    ir.geometry.bodies.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.regions.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.shells.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.faces.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.loops.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.coedges.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.edges.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.vertices.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.points.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.curves.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.subds.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry.pcurves.sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry
        .procedural_surfaces
        .sort_by(|a, b| a.id.cmp(&b.id));
    ir.geometry
        .procedural_curves
        .sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.tessellations.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.appearances.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model.attributes.sort_by(|a, b| a.id.cmp(&b.id));
    ir.model
        .appearance_bindings
        .sort_by(|left, right| left.id.cmp(&right.id));
}

/// `retained` and a record of each member of the source image that no
/// retained record holds, with the provenance the decode notes for a retained
/// member; `None` when every member has a record. A record holds the member
/// of its id, or else a member of its provenance stream. A part decoded with
/// no face keeps records of its source image and active Parasolid site; with
/// these records the write carries or names each of its other members as it
/// does for a part with faces.
fn member_records<'a>(
    scan: &'a crate::container::ContainerScan<'_>,
    retained: &[SourceRecord<'a>],
    annotations: &Annotations,
) -> Result<Option<(Vec<SourceRecord<'a>>, Annotations)>, CodecError> {
    let members = scan
        .sections()
        .map(|member| (member.native_id(), member))
        .collect::<Vec<_>>();
    let member_ids = members
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<HashSet<_>>();
    let record_ids = retained
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    let mut unplaced = HashMap::<&str, usize>::new();
    for record in retained
        .iter()
        .filter(|record| !member_ids.contains(record.id.as_str()))
    {
        if let Some(provenance) = annotations.provenance.get(record.id.as_str()) {
            *unplaced.entry(provenance.stream()).or_default() += 1;
        }
    }
    let mut unrecorded = Vec::new();
    for (id, member) in members {
        if record_ids.contains(id.as_str()) {
            continue;
        }
        match unplaced.get_mut(member.source_stream().as_str()) {
            Some(count) if *count > 0 => *count -= 1,
            _ => unrecorded.push((id, member)),
        }
    }
    if unrecorded.is_empty() {
        return Ok(None);
    }
    let mut annotations = annotations.clone();
    let mut records = retained
        .iter()
        .map(|record| SourceRecord {
            id: record.id.clone(),
            data: record.data,
        })
        .collect::<Vec<_>>();
    for (id, member) in unrecorded {
        crate::annotations::note_member_record(&mut annotations, member)?;
        records.push(SourceRecord {
            id,
            data: Some(member.stored_payload()),
        });
    }
    Ok(Some((records, annotations)))
}

fn source_image<'a>(records: &[SourceRecord<'a>]) -> Option<&'a [u8]> {
    records
        .iter()
        .find(|record| record.id.as_str() == "sldprt:file:source-image#0")?
        .data
}

/// The retained Parasolid partition to replay, when the decoded B-rep is
/// untouched since the decode that recorded its baseline.
///
/// The comparison is bitwise against the `brep_local_sha256` baseline, which is
/// what an edit-detection question needs and all it can be; see
/// [`crate::decode::brep_local_sha256`]. A document without the baseline — one
/// synthesized rather than decoded, or decoded by a binary that named the
/// attribute differently — has no partition to replay and is written out in full.
fn retained_partition(
    ir: &CadIr,
    source_scan: Option<&crate::container::ContainerScan<'_>>,
    native: Option<&SldprtNative>,
) -> Result<Option<(String, Vec<u8>)>, CodecError> {
    let Some(source) = ir.source.as_ref() else {
        return Ok(None);
    };
    let Some(expected) = source.attributes.get("brep_local_sha256") else {
        return Ok(None);
    };
    if crate::decode::brep_local_sha256(ir)? != *expected {
        return Ok(None);
    }
    let Some(scan) = source_scan else {
        return Ok(None);
    };
    let Some(site) = crate::container::select_active_parasolid_site(scan) else {
        return Ok(None);
    };
    let original_section = site.name();
    let section = native
        .and_then(|native| remapped_partition_section(ir, native, &original_section))
        .unwrap_or(original_section);
    Ok(Some((section, site.section.payload().to_vec())))
}

fn remapped_partition_section(ir: &CadIr, native: &SldprtNative, section: &str) -> Option<String> {
    let old_index = crate::container::configuration_index(section)?;
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

/// Each section's time/date word: the next one the source stores for a
/// member of its name, `None` past them.
fn section_type_ids(
    source_scan: Option<&crate::container::ContainerScan<'_>>,
    sections: &[(String, Vec<u8>)],
) -> Vec<Option<u32>> {
    let mut source_ids: HashMap<String, VecDeque<u32>> = HashMap::new();
    if let Some(scan) = source_scan {
        for block in &scan.blocks {
            if let Some(section) = block.section.name() {
                source_ids
                    .entry(section.to_owned())
                    .or_default()
                    .push_back(block.type_id);
            }
        }
    }
    sections
        .iter()
        .map(|(section, _)| source_ids.get_mut(section).and_then(VecDeque::pop_front))
        .collect()
}

pub(crate) fn pmi_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let bytes = serde_json::to_vec(&ir.model.pmi)
        .map_err(|error| CodecError::malformed(format_args!("cannot hash SLDPRT PMI: {error}")))?;
    Ok(cadmpeg_ir::hash::sha256_hex(&bytes))
}

/// Named member payloads.
type Sections = Vec<(String, Vec<u8>)>;

/// The partitions the writer generates from the document's bodies, and the
/// sections of those that hold no body.
fn generated_partitions(
    ir: &CadIr,
    length_scale: f64,
) -> Result<(Sections, Vec<String>), CodecError> {
    let mut bodiless = Vec::new();
    if ir.geometry.faces.is_empty() {
        let partitions = empty_partitions(ir, &mut bodiless)?;
        return Ok((partitions, bodiless));
    }
    if !ir.model.configurations.is_empty() {
        let partitions = configuration_partitions(ir, length_scale, &mut bodiless)?;
        return Ok((partitions, bodiless));
    }
    let schema_32001 = ir
        .geometry
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Sheet);
    let body = brep_body(ir, length_scale, schema_32001)?;
    let schema = if schema_32001 {
        "SCH_SW_32001_11000"
    } else {
        "SCH_SW_33103_11000"
    };
    Ok((
        vec![(
            "Contents/Config-0-Partition".to_string(),
            parasolid_stream(&body, schema)?,
        )],
        bodiless,
    ))
}

fn configuration_partitions(
    ir: &CadIr,
    length_scale: f64,
    bodiless: &mut Vec<String>,
) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    let configured = ir
        .model
        .configurations
        .iter()
        .flat_map(|configuration| {
            configuration
                .bodies
                .as_deref()
                .unwrap_or_default()
                .iter()
                .cloned()
        })
        .collect::<HashSet<_>>();
    if let Some(body) = ir
        .geometry
        .bodies
        .iter()
        .find(|body| !configured.contains(&body.id))
    {
        return Err(CodecError::malformed(format_args!(
            "SLDPRT body {} belongs to no configuration",
            body.id.as_str()
        )));
    }
    let mut configurations = ir.model.configurations.iter().collect::<Vec<_>>();
    configurations.sort_by_key(|configuration| configuration.ordinal);
    configurations
        .into_iter()
        .filter_map(|configuration| {
            configuration
                .bodies
                .as_deref()
                .map(|bodies| (configuration, bodies))
        })
        .map(|(configuration, bodies)| {
            let index = configuration.source_index.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT configuration {} has no assigned source index",
                    configuration.id.as_str()
                ))
            })?;
            if bodies.is_empty() {
                return bodiless_partition(index, bodiless);
            }
            let subset = body_subset(ir, bodies)?;
            let schema_32001 = subset
                .geometry
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
                parasolid_stream(&body, schema)?,
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
            CodecError::NotImplemented(
                "SLDPRT configuration source index space is exhausted".into(),
            )
        })?;
    }
}

/// One partition per indexed configuration, or `Config-0`, each holding no
/// body. A file with no partition is not an SLDPRT part its readers detect.
fn empty_partitions(
    ir: &CadIr,
    bodiless: &mut Vec<String>,
) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    let mut indices = ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        indices.push(0);
    }
    indices
        .into_iter()
        .map(|index| bodiless_partition(index, bodiless))
        .collect()
}

/// The partition of configuration `index` that holds no body, recorded in
/// `bodiless`.
fn bodiless_partition(
    index: u32,
    bodiless: &mut Vec<String>,
) -> Result<(String, Vec<u8>), CodecError> {
    let section = format!("Contents/Config-{index}-Partition");
    let mut payload = parasolid_stream(&[], "SCH_SW_33103_11000")?;
    payload.extend(parasolid_stream_named(
        &[],
        "SCH_SW_33103_11000",
        "deltas body",
    )?);
    bodiless.push(section.clone());
    Ok((section, payload))
}

fn body_subset(ir: &CadIr, selected: &[cadmpeg_ir::ids::BodyId]) -> Result<CadIr, CodecError> {
    let selected = selected.iter().cloned().collect::<HashSet<_>>();
    if let Some(id) = selected
        .iter()
        .find(|id| !ir.geometry.bodies.iter().any(|body| &body.id == *id))
    {
        return Err(CodecError::malformed(format_args!(
            "configuration references missing body {}",
            id.as_str()
        )));
    }
    let mut subset = ir.clone();
    subset
        .geometry
        .bodies
        .retain(|body| selected.contains(&body.id));
    for definition in &mut subset.model.product_definitions {
        definition.bodies.retain(|body| selected.contains(body));
    }
    subset
        .geometry
        .regions
        .retain(|region| selected.contains(&region.body));
    let shells = subset
        .geometry
        .regions
        .iter()
        .flat_map(|region| region.shells.iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .shells
        .retain(|shell| shells.contains(&shell.id));
    subset
        .geometry
        .faces
        .retain(|face| shells.contains(&face.shell));
    let loops = subset
        .geometry
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .loops
        .retain(|loop_| loops.contains(&loop_.id));
    let coedges = subset
        .geometry
        .loops
        .iter()
        .flat_map(|loop_| loop_.coedges().iter().cloned())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .coedges
        .retain(|coedge| coedges.contains(&coedge.id));
    let mut edges = subset
        .geometry
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<HashSet<_>>();
    for shell in &subset.geometry.shells {
        edges.extend(shell.wire_edges().cloned());
    }
    subset
        .geometry
        .edges
        .retain(|edge| edges.contains(&edge.id));
    let mut vertices = subset
        .geometry
        .edges
        .iter()
        .flat_map(|edge| edge.vertices().cloned())
        .collect::<HashSet<_>>();
    for shell in &subset.geometry.shells {
        vertices.extend(shell.free_vertices().cloned());
    }
    for loop_ in &subset.geometry.loops {
        vertices.extend(loop_.vertices().cloned());
    }
    subset
        .geometry
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points = subset
        .geometry
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .points
        .retain(|point| points.contains(&point.id));
    let surfaces = subset
        .geometry
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    let curves = subset
        .geometry
        .edges
        .iter()
        .filter_map(|edge| edge.curve().cloned())
        .collect::<HashSet<_>>();
    subset
        .geometry
        .curves
        .retain(|curve| curves.contains(&curve.id));
    let pcurves = subset
        .geometry
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.clone()))
        .collect::<HashSet<_>>();
    subset
        .geometry
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
    subset.finalize();
    Ok(subset)
}

/// The retained blocks the output holds as stored, and the retained blocks it
/// omits.
fn opaque_blocks(
    ir: &CadIr,
    records: &[SourceRecord<'_>],
    annotations: &Annotations,
    generated_partitions: &HashSet<String>,
    emitted_sections: &HashMap<String, usize>,
    retain_native_brep: bool,
    patch_active_configuration: bool,
) -> Result<OpaqueBlocks, CodecError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()
        .map_err(CodecError::from)?;
    let active_name = match ir
        .model
        .configurations
        .iter()
        .find(|value| value.active && patch_active_configuration)
    {
        Some(active) => Some(active.name.as_deref().ok_or_else(|| {
            CodecError::Malformed("active SLDPRT configuration has no resolved name".into())
        })?),
        None => None,
    };
    let mut seen = HashSet::new();
    let mut blocks = Vec::new();
    let mut unavailable = Vec::new();
    let mut replaced = HashMap::<&str, Vec<&str>>::new();
    for record in records
        .iter()
        .filter(|record| record.id.as_str().starts_with("sldprt:file:block#"))
    {
        let Some(provenance) = annotations.provenance.get(record.id.as_str()) else {
            continue;
        };
        let section = provenance.stream();
        let lower = section.to_ascii_lowercase();
        if generated_partitions.contains(section) {
            continue;
        }
        if lower.ends_with("-partition")
            && native
                .as_ref()
                .and_then(|native| remapped_partition_section(ir, native, section))
                .is_some_and(|remapped| generated_partitions.contains(&remapped))
        {
            continue;
        }
        if lower.contains("deltas") && !retain_native_brep {
            continue;
        }
        if emitted_sections.contains_key(section) {
            replaced
                .entry(section)
                .or_default()
                .push(record.id.as_str());
            continue;
        }
        let Some(data) = record.data else {
            unavailable.push(format!("{section} ({})", record.id.as_str()));
            continue;
        };
        let mut payload = data.to_vec();
        if lower.contains("pmisemanticdatadb") {
            crate::pmi::patch_payload(ir, record.id.as_str(), &mut payload)?;
        }
        if let Some(active_name) = active_name {
            if let Some(patched) = patch_active_configuration_xml(&payload, active_name)? {
                payload = patched;
            }
        }
        if !seen.insert(record.id.as_str()) {
            return Err(CodecError::Malformed(
                "duplicate retained SLDPRT source record identity".into(),
            ));
        }
        blocks.push((section.to_string(), payload));
    }
    let replaced = replaced
        .into_iter()
        .flat_map(|(section, ids)| {
            let written = emitted_sections.get(section).copied().unwrap_or_default();
            ids.into_iter().skip(written).map(str::to_owned)
        })
        .collect();
    Ok(OpaqueBlocks {
        blocks,
        replaced,
        unavailable,
    })
}

/// [`opaque_blocks`]'s result.
struct OpaqueBlocks {
    /// The blocks written as stored, with their streams.
    blocks: Vec<(String, Vec<u8>)>,
    /// The ids of retained blocks of a generated stream past the number of
    /// members the write generates for it.
    replaced: Vec<String>,
    /// Each retained block whose bytes the source fidelity does not hold,
    /// as its stream and id.
    unavailable: Vec<String>,
}

/// The patchable metadata members of one write, as the source stores them,
/// and the attribute ids the patch wrote into each.
struct MetadataMembers {
    originals: BTreeMap<String, Vec<u8>>,
    written: BTreeMap<String, Vec<String>>,
}

impl MetadataMembers {
    /// Add `generated` as member `name`. Where the patch already wrote that
    /// member, the generated payload replaces it unless it equals the
    /// original or the patched payload; each attribute the patch wrote into a
    /// replaced member is charged.
    fn push(
        &self,
        sections: &mut Vec<(String, Vec<u8>)>,
        unwritten: &mut UnwrittenAttributes,
        name: String,
        generated: Vec<u8>,
    ) {
        let Some(original) = self.originals.get(&name) else {
            sections.push((name, generated));
            return;
        };
        let Some(patched) = sections
            .iter_mut()
            .find_map(|(candidate, payload)| (*candidate == name).then_some(payload))
        else {
            sections.push((name, generated));
            return;
        };
        if generated == *original || generated == *patched {
            return;
        }
        if patched != original {
            for id in self.written.get(&name).into_iter().flatten() {
                unwritten.record(id, Unwritten::OwnerRegenerated, Held::Source);
            }
        }
        *patched = generated;
    }
}

/// Patch each metadata attribute into its retained member in `sections`,
/// and record each one the patch does not write. Returns the attribute ids
/// written into each member.
fn patch_retained_swobjects_metadata(
    ir: &CadIr,
    annotations: &Annotations,
    owners: &metadata_owners::MetadataOwners,
    sections: &mut [(String, Vec<u8>)],
    length_scale: f64,
    unwritten: &mut UnwrittenAttributes,
) -> BTreeMap<String, Vec<String>> {
    use std::cmp::Reverse;

    let mut attributes = Vec::new();
    for attribute in metadata_attributes(ir) {
        if attribute.name == "source_linear_unit_code" {
            continue;
        }
        let provenance = annotations.provenance.get(attribute.id.as_str());
        if let Some(reason) = unwritable(attribute) {
            let held = if provenance.is_some() {
                Held::Source
            } else {
                Held::Nothing
            };
            unwritten.record(attribute.id.as_str(), reason, held);
            continue;
        }
        let Some(provenance) = provenance else {
            // No retained record holds it, and none can be inserted.
            let reason = MetadataRecord::project(attribute, length_scale, |_| Ok(()))
                .err()
                .unwrap_or(Unwritten::NoSerializer);
            unwritten.record(attribute.id.as_str(), reason, Held::Nothing);
            continue;
        };
        if let Some((reason, held)) = owners.unpatched(provenance.stream()) {
            unwritten.record(attribute.id.as_str(), reason, held);
            continue;
        }
        attributes.push((provenance.stream().to_owned(), provenance.offset, attribute));
    }
    attributes.sort_by(|left, right| (&left.0, Reverse(left.1)).cmp(&(&right.0, Reverse(right.1))));

    let mut written = BTreeMap::<String, Vec<String>>::new();
    for (stream, offset, attribute) in attributes {
        let id = attribute.id.as_str();
        let Some(payload) = sections
            .iter_mut()
            .find_map(|(candidate, payload)| (*candidate == stream).then_some(payload))
        else {
            unwritten.record(id, Unwritten::OwnerUnavailable, Held::Nothing);
            continue;
        };
        let result = usize::try_from(offset)
            .map_err(|_| Unwritten::RetainedMismatch)
            .and_then(|offset| patch_retained_record(payload, offset, attribute, length_scale));
        match result {
            Ok(()) => written.entry(stream).or_default().push(id.to_owned()),
            Err(reason) => unwritten.record(id, reason, Held::Source),
        }
    }

    // A record the document no longer states keeps its retained bytes.
    let stated = metadata_attributes(ir)
        .into_iter()
        .map(|attribute| attribute.id.as_str())
        .collect::<HashSet<_>>();
    for (id, provenance) in &annotations.provenance {
        let stream = provenance.stream();
        if id.starts_with("sldprt:metadata:")
            && !id.starts_with("sldprt:metadata:source_linear_unit_code#")
            && !stated.contains(id.as_str())
            && (owners.carried.contains(stream) || sections.iter().any(|(name, _)| name == stream))
        {
            unwritten.record(id, Unwritten::Removed, Held::Source);
        }
    }
    written
}

/// Write `attribute` into its retained record at `offset` of `payload`, or
/// leave the record untouched and say why it was not written.
fn patch_retained_record(
    payload: &mut Vec<u8>,
    offset: usize,
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    length_scale: f64,
) -> Result<(), Unwritten> {
    if attribute.name == "configuration_manager"
        && crate::metadata::configuration_manager::values(payload, offset).is_some_and(|values| {
            attribute.values == values.map(|value| Some(AttributeValue::Integer(value)))
        })
    {
        return Ok(());
    }
    // The retained record's token and unit-name marker are checked once
    // the values have the record's shape and before they are admitted.
    let retained = |token: &'static [u8]| {
        require_token(payload, offset, token)?;
        if token == UNIT_NAME_TOKEN {
            resident_unit_name_len(payload, offset + token.len())?;
        }
        Ok(())
    };
    let mismatch = |_: CodecError| Unwritten::RetainedMismatch;
    match MetadataRecord::project(attribute, length_scale, retained)? {
        MetadataRecord::BoundingEnvelope(values) => {
            let [Some(AttributeValue::Vector(current))] = attribute.values.as_slice() else {
                return Err(Unwritten::Unrepresentable);
            };
            let current = current.iter().map(|value| value.get()).collect::<Vec<_>>();
            patch_retained_metadata_reals(
                payload,
                offset + BOUNDING_ENVELOPE_TOKEN.len() + 4,
                &values,
                &current,
                4,
                length_scale,
            )
            .map_err(mismatch)
        }
        MetadataRecord::DefaultReferencePlane { origin, frame } => {
            let [Some(AttributeValue::Vector(current_origin)), Some(AttributeValue::Vector(current_frame))] =
                attribute.values.as_slice()
            else {
                return Err(Unwritten::Unrepresentable);
            };
            let current = current_origin
                .iter()
                .chain(current_frame)
                .map(|value| value.get())
                .collect::<Vec<_>>();
            let values = origin.into_iter().chain(frame).collect::<Vec<_>>();
            patch_retained_metadata_reals(
                payload,
                offset + DEFAULT_REFERENCE_PLANE_TOKEN.len(),
                &values,
                &current,
                3,
                length_scale,
            )
            .map_err(mismatch)
        }
        MetadataRecord::TransformedReferenceEnclosure {
            center,
            upper,
            lower,
            radius,
        } => {
            let resident = crate::metadata::transformed_enclosure(payload, offset)
                .ok_or(Unwritten::NoSerializer)?;
            let [Some(AttributeValue::Vector(ir_center)), Some(AttributeValue::Vector(ir_upper)), Some(AttributeValue::Vector(ir_lower)), Some(AttributeValue::Float(ir_radius))] =
                attribute.values.as_slice()
            else {
                return Err(Unwritten::Unrepresentable);
            };
            let ir_values = ir_center
                .iter()
                .chain(ir_upper)
                .chain(ir_lower)
                .map(|v| v.get())
                .chain([ir_radius.get()]);
            let source_values = center.into_iter().chain(upper).chain(lower).chain([radius]);
            let mut bytes = Vec::with_capacity(80);
            for ((value, current), original) in source_values.zip(ir_values).zip(resident) {
                let value = if original * (1.0 / length_scale) == current {
                    original
                } else {
                    value
                };
                bytes.extend(value.to_le_bytes());
            }
            overwrite_bytes(
                payload,
                offset
                    + TRANSFORMED_REFERENCE_PLANE_TOKEN.len()
                    + crate::layout::transformed_reference_plane_metadata::LEN,
                &bytes,
            )
            .map_err(mismatch)
        }
        MetadataRecord::PartRecord {
            first_word,
            trailing_word,
        } => {
            let crate::metadata::part::Root::Supported(resident) =
                crate::metadata::part::parse(payload)
            else {
                return Err(Unwritten::NoSerializer);
            };
            // The Part edit cannot change root ownership or scalar form.
            if offset != resident.token_offset
                || trailing_word.is_some() != resident.trailing_word.is_some()
            {
                return Err(Unwritten::NoSerializer);
            }
            // The trailing word lies past the first, so a truncated record
            // fails before anything is written.
            if let Some(word) = trailing_word {
                overwrite_bytes(
                    payload,
                    resident.body_offset + crate::layout::part_root_long::TRAILING_WORD,
                    &word.to_le_bytes(),
                )
                .map_err(mismatch)?;
            }
            overwrite_bytes(payload, resident.body_offset, &first_word.to_le_bytes())
                .map_err(mismatch)
        }
        MetadataRecord::SourceLinearUnitName {
            units,
            byte_len,
            cut,
        } => {
            if cut.is_some() {
                return Err(Unwritten::Unrepresentable);
            }
            let marker = offset + UNIT_NAME_TOKEN.len();
            let old_len = resident_unit_name_len(payload, marker).map_err(mismatch)?;
            if usize::from(byte_len) != old_len {
                // The writer cannot resize a retained unit name.
                return Err(Unwritten::NoSerializer);
            }
            let end = marker
                .checked_add(4 + old_len)
                .filter(|end| *end <= payload.len())
                .ok_or(Unwritten::RetainedMismatch)?;
            let mut replacement = vec![0xff, 0xfe, 0xff, byte_len];
            replacement.extend(units.into_iter().flat_map(u16::to_le_bytes));
            payload.splice(marker..end, replacement);
            Ok(())
        }
        MetadataRecord::SourceLinearUnitCode(_) => Err(Unwritten::NoSerializer),
    }
}

fn patch_retained_metadata_reals(
    payload: &mut [u8],
    start: usize,
    values: &[f64],
    current: &[f64],
    scaled: usize,
    length_scale: f64,
) -> Result<(), CodecError> {
    if values.len() != current.len() || scaled > values.len() {
        return Err(CodecError::Malformed(
            "invalid retained metadata real fields".into(),
        ));
    }
    // Every field is read before any is written, so a refused record is
    // left as it was.
    let mut writes = Vec::with_capacity(values.len());
    for (index, (&value, &current)) in values.iter().zip(current).enumerate() {
        let offset = index
            .checked_mul(8)
            .and_then(|relative| start.checked_add(relative))
            .ok_or_else(|| CodecError::Malformed("retained metadata offset overflow".into()))?;
        let original = cadmpeg_core::decode::View::f64_le_at(payload, offset)
            .filter(|value| value.is_finite())
            .ok_or_else(|| CodecError::Malformed("invalid retained metadata real".into()))?;
        let projected = if index < scaled {
            original * (1.0 / length_scale)
        } else {
            original
        };
        if projected != current {
            writes.push((offset, value));
        }
    }
    for (offset, value) in writes {
        overwrite_bytes(payload, offset, &value.to_le_bytes())?;
    }
    Ok(())
}

/// The byte length of the retained unit name after its marker at `marker`.
fn resident_unit_name_len(payload: &[u8], marker: usize) -> Result<usize, CodecError> {
    if payload.get(marker..marker + 3) != Some(&[0xff, 0xfe, 0xff]) {
        return Err(CodecError::Malformed(
            "retained SLDPRT unit-name marker does not match its provenance".into(),
        ));
    }
    Ok(usize::from(*payload.get(marker + 3).ok_or_else(|| {
        CodecError::Malformed("truncated retained SLDPRT unit name".into())
    })?))
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
    let Some(text) = crate::container::xml_text(payload) else {
        return Ok(None);
    };
    let Ok(document) = roxmltree::Document::parse(&text) else {
        return Ok(None);
    };
    if document.root_element().tag_name().name() != "swSolidWorks" {
        return Ok(None);
    }
    let mut models = document
        .descendants()
        .filter(|node| node.has_tag_name("swModel"));
    let model = models
        .next()
        .ok_or_else(|| CodecError::Malformed("SolidWorks XML has no model record".into()))?;
    if models.next().is_some()
        || document
            .descendants()
            .any(|node| node.has_tag_name("swConfiguration"))
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT active configuration edits require a complete configuration manifest rewrite"
                .into(),
        ));
    }
    let attribute = model
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

/// A feature-input object name: a `CString` whose u8 count counts UTF-16
/// code units.
const FEATURE_INPUT_OBJECT_NAME: TextCapacity =
    TextCapacity::new("feature-input object name", TextUnit::Utf16CodeUnit, 255);

/// The resident unit name of `moLengthUserUnits_c`: a u8 byte count of
/// UTF-16 code units.
const UNIT_NAME: TextCapacity = TextCapacity::new("unit name", TextUnit::Utf16CodeUnit, 127);

/// Every text field this writer fits to its width (§3.4). The widths test
/// checks each against its stored count.
#[cfg(test)]
pub(crate) const CAPACITY_FIELDS: &[TextCapacity] = &[
    materials::MATERIAL_NAME,
    FEATURE_INPUT_OBJECT_NAME,
    UNIT_NAME,
    crate::resolved_features::write_prepare::GENERATED_NAME,
];

/// Synchronize the native history, sketches, parameters and configuration
/// states with the document, and return the feature-input renames to write.
fn prepare_history(
    normalized: &mut CadIr,
    native: &mut Option<SldprtNative>,
    annotations: &Annotations,
    retained_records: &[SourceRecord<'_>],
    cut: &mut Vec<String>,
) -> Result<Vec<crate::history::write::features::FeatureInputRename>, CodecError> {
    let feature_name_changes =
        crate::history::write::feature_name_changes(normalized, native.as_ref());
    let feature_parameter_changes_authorized = !feature_name_changes.is_empty()
        && crate::history::write::native_parameters_match_source(normalized, native.as_ref())?;
    crate::history::write::apply_feature_name_changes(
        &mut normalized.model.parameters,
        &feature_name_changes,
    );
    let ir = &*normalized;
    let mut feature_input_renames = crate::history::write::prepare_features_for_write(ir, native)?;
    fit_feature_input_renames(&mut feature_input_renames, cut);
    crate::resolved_features::write_prepare::prepare_sketches_for_write(ir, native, cut)?;
    crate::history::write::parameters::prepare_parameters_for_write(
        ir,
        native,
        feature_parameter_changes_authorized,
    )?;
    // The configuration states are projected again with the profile bodies
    // the decode bound, read from the retained source image.
    crate::history::write::configurations::prepare_configurations_for_write(
        ir,
        native,
        annotations,
        |lanes| {
            source_image(retained_records)
                .map(|image| {
                    crate::resolved_features::assembly::source_image_profiles(image, lanes)
                })
                .transpose()
        },
    )?;
    Ok(feature_input_renames)
}

/// Cut each rename to [`FEATURE_INPUT_OBJECT_NAME`]. A rename whose cut is
/// blank is not written.
fn fit_feature_input_renames(
    renames: &mut Vec<crate::history::write::features::FeatureInputRename>,
    cut: &mut Vec<String>,
) {
    renames.retain_mut(|rename| {
        let fitted = FEATURE_INPUT_OBJECT_NAME.fit(rename.value.as_str());
        let Some(entry) = fitted.unwritable_entry(&format!(
            "{} object name {}",
            rename.lane, rename.name_index
        )) else {
            return true;
        };
        cut.push(entry);
        match cadmpeg_core::text::NonBlankString::new(fitted.text) {
            Some(value) => {
                rename.value = value;
                true
            }
            None => false,
        }
    });
}

/// The bytes written for one feature-input lane.
///
/// Every object-name record states the payload bytes at its own offset, so the
/// lane is written as it stands. `renames` carries the write-side object-name
/// changes that `synchronize_feature_input_names` derived from the neutral
/// feature names; the splice loop at the end of this function writes each one
/// into the emitted payload.
fn resolved_feature_payload(
    lane: &crate::records::FeatureInputLane,
    histories: &[crate::records::FeatureHistory],
    renames: &[crate::history::write::features::FeatureInputRename],
) -> Result<Vec<u8>, CodecError> {
    const MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane.native_payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    let expected_classes =
        crate::resolved_features::names::class_declarations(&lane.native_payload, &lane.id)?;
    if lane.classes != expected_classes {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited class declarations",
            lane.id
        )));
    }
    // Every field of an object-name record, the value included, states the
    // payload bytes at `offset`.
    let expected_names =
        crate::resolved_features::names::object_names(&ctx, &lane.native_payload, &lane.id)?;
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
                    || actual.object_id != expected.object_id
            })
    {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited object-name structure",
            lane.id
        )));
    }
    if let Some((index, actual, expected)) =
        lane.names.iter().zip(&expected_names).enumerate().find_map(
            |(index, (actual, expected))| {
                (actual.value != expected.value).then_some((index, actual, expected))
            },
        )
    {
        return Err(CodecError::malformed(format_args!(
            "feature-input name value does not match its native payload: lane {} name {index} states {:?}, its payload states {:?}",
            lane.id, actual.value, expected.value
        )));
    }
    let mut expected_lane = lane.clone();
    // Reconstruct marker identities from bytes before binding history ownership.
    // Cloning the supplied markers would make the comparison below circular.
    expected_lane.sketch_entities = crate::resolved_features::markers::admit_sketch_input_entities(
        &lane.native_payload,
        &lane.id,
    )?;
    expected_lane.scalars = crate::resolved_features::scalars::named_scalars(
        &lane.native_payload,
        &lane.id,
        &lane.names,
    )?;
    expected_lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
        &lane.id,
        &lane.classes,
        &expected_lane.scalars,
        &[],
    )?;
    expected_lane.references = crate::resolved_features::markers::reference_cells(
        &expected_lane.scalars,
        &expected_lane.classes,
    )?;
    crate::resolved_features::bindings::bind_scalar_operands(
        &ctx,
        histories,
        std::slice::from_mut(&mut expected_lane),
    )?;
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
    if lane.sketch_entities.len() != expected_lane.sketch_entities.len() {
        return Err(CodecError::NotImplemented(format!(
            "feature-input lane {} has edited marker membership",
            lane.id
        )));
    }
    for (entity, expected_entity) in lane
        .sketch_entities
        .iter()
        .zip(&expected_lane.sketch_entities)
    {
        if entity.id() != expected_entity.id()
            || entity.parent() != expected_entity.parent()
            || entity.ordinal() != expected_entity.ordinal()
            || entity.offset() != expected_entity.offset()
            || entity.object_index() != expected_entity.object_index()
            || entity.local_id() != expected_entity.local_id()
            || entity.feature_ref != expected_entity.feature_ref
            || entity.links != expected_entity.links
        {
            return Err(CodecError::malformed(format_args!(
                "feature-input lane {} has inconsistent marker identity or ownership",
                lane.id
            )));
        }
    }
    let mut payload = lane.native_payload.clone();
    let mut scanned = None;
    for (entity, expected_entity) in lane
        .sketch_entities
        .iter()
        .zip(&expected_lane.sketch_entities)
    {
        // All decoder-supported layouts can replay an unchanged marker. Writing
        // current-family offsets into legacy markers would alter unrelated bytes.
        if entity == expected_entity {
            continue;
        }
        entity
            .validate_against_payload(&lane.native_payload)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "feature-input lane {} entity {}: {error}",
                    lane.id,
                    entity.id()
                ))
            })?;
        let offset = usize::try_from(entity.offset()).map_err(|_| {
            CodecError::Malformed("feature-input offset exceeds address space".into())
        })?;
        let marker_end = offset
            .checked_add(MARKER.len())
            .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
        if payload.get(offset..marker_end) != Some(MARKER) {
            return Err(CodecError::NotImplemented(
                "editing this feature-input marker layout is not implemented".into(),
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
        let code = entity.kind().native_code().to_le_bytes();
        // The scanned kind of a marker is not always the kind whose code is
        // stored (the scan's code-to-kind reading is not `native_code`'s
        // inverse), and decode reclassifies some markers from their
        // neighbourhood (`bindings.rs`), only ever into `Point` or
        // `LineOrCircle`. Where the marker's kind is the scanned one or such a
        // reclassification, the payload keeps its own code. A marker's kind is
        // not edited by any writer path, so this reads no edit as unchanged.
        if field != code {
            let scanned = match &mut scanned {
                Some(scanned) => scanned,
                slot @ None => slot.insert(
                    crate::resolved_features::markers::admit_sketch_input_entities(
                        &lane.native_payload,
                        &lane.id,
                    )?,
                ),
            };
            let code_retained = scanned.iter().any(|read| {
                use crate::records::SketchInputKind::{LineOrCircle, Point};
                read.offset() == entity.offset()
                    && (read.kind() == entity.kind()
                        || matches!(entity.kind(), Point | LineOrCircle))
            });
            if !code_retained {
                field.copy_from_slice(&code);
            }
        }
        if let Some(value) = entity.state_value {
            let state_start = offset
                .checked_add(48)
                .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
            let state_end = state_start
                .checked_add(8)
                .ok_or_else(|| CodecError::Malformed("feature-input offset overflow".into()))?;
            let state = payload.get_mut(state_start..state_end).ok_or_else(|| {
                CodecError::Malformed("feature-input state field exceeds retained payload".into())
            })?;
            state.copy_from_slice(&value.get().to_le_bytes());
        }
        if let Some(coordinates) = entity.coordinates_m {
            let Some(stored) =
                crate::resolved_features::markers::marker_coordinates(&lane.native_payload, offset)
            else {
                return Err(CodecError::NotImplemented(
                    "feature-input marker does not carry editable coordinate fields".into(),
                ));
            };
            // Unchanged coordinates stay as stored, wherever the layout keeps
            // them. A change is written at +66 and +74, and only to a layout
            // that keeps its pair there.
            if stored == coordinates {
                continue;
            }
            let at_66 = (
                cadmpeg_core::decode::View::f64_le_at(&lane.native_payload, offset + 66),
                cadmpeg_core::decode::View::f64_le_at(&lane.native_payload, offset + 74),
            );
            if at_66 != (Some(stored[0]), Some(stored[1])) {
                return Err(CodecError::NotImplemented(format!(
                    "feature-input lane {} edits the coordinates of sketch marker {}, whose layout keeps them elsewhere",
                    lane.id,
                    entity.id()
                )));
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
    let mut lane_renames = renames
        .iter()
        .filter(|rename| rename.lane == lane.id)
        .map(|rename| {
            let name = lane.names.get(rename.name_index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "feature-input lane {} has no object name {}",
                    lane.id, rename.name_index
                ))
            })?;
            Ok((name, rename))
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    // Descending offset order: a spliced name changes the length of the payload
    // after it, and every remaining name sits before the one just written.
    lane_renames.sort_by_key(|(name, _)| std::cmp::Reverse(name.offset));
    for (name, rename) in lane_renames {
        let utf16 = rename.value.as_str().encode_utf16().collect::<Vec<_>>();
        let length = u8::try_from(utf16.len()).map_err(|_| {
            CodecError::NotImplemented("feature-input object name exceeds 255 UTF-16 units".into())
        })?;
        let start = usize::try_from(name.offset).map_err(|_| {
            CodecError::Malformed("feature-input name offset exceeds address space".into())
        })?;
        let end = start
            .checked_add(6 + name.value.encode_utf16().count() * 2)
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
    Ok(payload.into_inner())
}

/// The record position a `sldprt:metadata:*` identifier carries: the ordinal of
/// the section the record was read from and its byte offset inside that
/// section's payload.
fn metadata_source_position(id: &str) -> Option<(u64, u64)> {
    let (section, offset) = id.rsplit_once('#')?.1.split_once(':')?;
    Some((section.parse().ok()?, offset.parse().ok()?))
}

const BOUNDING_ENVELOPE_TOKEN: &[u8] = b"moBBoxCenterData_c";
const DEFAULT_REFERENCE_PLANE_TOKEN: &[u8] = b"moDefaultRefPlnData_c";
const TRANSFORMED_REFERENCE_PLANE_TOKEN: &[u8] = b"moTransRefPlaneData_c";
const PART_RECORD_TOKEN: &[u8] = b"moPart_c";
const UNIT_NAME_TOKEN: &[u8] = b"moLengthUserUnits_c";

/// One `SWObjects` metadata attribute admitted for writing. Its values have
/// the shape the record states, and each length, multiplied into source
/// units, is finite. Both metadata writers take their bytes from here.
enum MetadataRecord {
    /// Four envelope lengths in source units.
    BoundingEnvelope([f64; 4]),
    /// The plane origin in source units and its six unitless frame values.
    DefaultReferencePlane { origin: [f64; 3], frame: [f64; 6] },
    /// Stored center, upper and lower corners, and radius in source units.
    TransformedReferenceEnclosure {
        center: [f64; 3],
        upper: [f64; 3],
        lower: [f64; 3],
        radius: f64,
    },
    PartRecord {
        first_word: u32,
        trailing_word: Option<u32>,
    },
    /// Source linear unit code.
    SourceLinearUnitCode(i64),
    /// A non-empty UTF-16 unit name and its byte length.
    SourceLinearUnitName {
        units: Vec<u16>,
        byte_len: u8,
        /// The `export:value.unwritable` entry when the name was cut.
        cut: Option<String>,
    },
}

impl MetadataRecord {
    /// Admit one metadata attribute, or say why it has no record.
    ///
    /// The values' shape is checked first. `retained` then receives the
    /// record's `SWObjects` token, and the values are admitted after it
    /// returns; its refusal is [`Unwritten::RetainedMismatch`].
    fn project(
        attribute: &cadmpeg_ir::attributes::SourceAttribute,
        length_scale: f64,
        retained: impl FnOnce(&'static [u8]) -> Result<(), CodecError>,
    ) -> Result<Self, Unwritten> {
        use cadmpeg_ir::attributes::AttributeValue;

        let invalid = Unwritten::Unrepresentable;
        let retained = |token| retained(token).map_err(|_| Unwritten::RetainedMismatch);
        let values = attribute.values.as_slice();
        Ok(match attribute.name.as_str() {
            "bounding_envelope" => {
                let [Some(AttributeValue::Vector(values))] = values else {
                    return Err(invalid);
                };
                let values = ratios::<4>(values).ok_or(invalid)?;
                retained(BOUNDING_ENVELOPE_TOKEN)?;
                Self::BoundingEnvelope(scaled(values, length_scale).ok_or(invalid)?)
            }
            "default_reference_plane" => {
                let [Some(AttributeValue::Vector(origin)), Some(AttributeValue::Vector(frame))] =
                    values
                else {
                    return Err(invalid);
                };
                let (Some(origin), Some(frame)) = (ratios::<3>(origin), ratios(frame)) else {
                    return Err(invalid);
                };
                retained(DEFAULT_REFERENCE_PLANE_TOKEN)?;
                let origin = scaled(origin, length_scale).ok_or(invalid)?;
                Self::DefaultReferencePlane { origin, frame }
            }
            "transformed_reference_enclosure" => {
                let [Some(AttributeValue::Vector(center)), Some(AttributeValue::Vector(upper)), Some(AttributeValue::Vector(lower)), Some(AttributeValue::Float(radius))] =
                    values
                else {
                    return Err(invalid);
                };
                let (Some(center), Some(upper), Some(lower), Some([radius])) = (
                    ratios::<3>(center).and_then(|v| scaled(v, length_scale)),
                    ratios::<3>(upper).and_then(|v| scaled(v, length_scale)),
                    ratios::<3>(lower).and_then(|v| scaled(v, length_scale)),
                    scaled([radius.get()], length_scale),
                ) else {
                    return Err(invalid);
                };
                let values = [
                    center[0], center[1], center[2], upper[0], upper[1], upper[2], lower[0],
                    lower[1], lower[2], radius,
                ];
                if !crate::metadata::valid_transformed_enclosure(&values) {
                    return Err(invalid);
                }
                retained(TRANSFORMED_REFERENCE_PLANE_TOKEN)?;
                Self::TransformedReferenceEnclosure {
                    center,
                    upper,
                    lower,
                    radius,
                }
            }
            "part_record" => {
                retained(PART_RECORD_TOKEN)?;
                let (first, trailing) = match values {
                    [Some(AttributeValue::Integer(first))] => (*first, None),
                    [Some(AttributeValue::Integer(first)), Some(AttributeValue::Integer(last))] => {
                        (*first, Some(*last))
                    }
                    _ => return Err(invalid),
                };
                Self::PartRecord {
                    first_word: u32::try_from(first).map_err(|_| Unwritten::TooWide)?,
                    trailing_word: trailing
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| Unwritten::TooWide)?,
                }
            }
            // No serializer owns the version-specific manager fields or
            // their scalar roles, so the record is neither edited nor
            // generated.
            "configuration_manager" => return Err(Unwritten::NoSerializer),
            "source_linear_unit_code" => {
                let [Some(AttributeValue::Integer(code))] = values else {
                    return Err(invalid);
                };
                Self::SourceLinearUnitCode(*code)
            }
            "source_linear_unit_name" => {
                let [Some(AttributeValue::String(name))] = values else {
                    return Err(invalid);
                };
                retained(UNIT_NAME_TOKEN)?;
                let fitted = UNIT_NAME.fit(name);
                if fitted.units == 0 {
                    return Err(invalid);
                }
                let units = fitted.text.encode_utf16().collect::<Vec<_>>();
                // The resident u16 vector occupies two bytes per unit.
                let byte_len = u8::try_from(units.len() * 2)
                    .expect("UNIT_NAME.width fits the u8 byte count (writer::tests::capacity)");
                Self::SourceLinearUnitName {
                    units,
                    byte_len,
                    cut: fitted.unwritable_entry(attribute.id.as_str()),
                }
            }
            _ => return Err(Unwritten::UnknownName),
        })
    }
}

/// `N` unitless values, or `None` for another count.
fn ratios<const N: usize>(values: &[FiniteReal]) -> Option<[f64; N]> {
    let values: &[FiniteReal; N] = values.try_into().ok()?;
    Some(values.map(FiniteReal::get))
}

/// Each value multiplied by `scale`, or `None` when a product is not finite.
fn scaled<const N: usize>(values: [f64; N], scale: f64) -> Option<[f64; N]> {
    let products = values.map(|value| value * scale);
    products
        .iter()
        .all(|product| product.is_finite())
        .then_some(products)
}

/// Why no metadata record can hold `attribute`, judged from the attribute
/// alone: a null at some position, or a target other than the document.
fn unwritable(attribute: &cadmpeg_ir::attributes::SourceAttribute) -> Option<Unwritten> {
    if attribute.values.iter().any(Option::is_none) {
        Some(Unwritten::NoValue)
    } else if attribute.target.entity != cadmpeg_ir::attributes::AttributeTarget::Document {
        Some(Unwritten::EntityTarget)
    } else {
        None
    }
}

/// This codec's document attributes, ordered by source payload position.
///
/// Arena order is by identifier name; writing that order would move every
/// record and rename attributes minted from byte offsets.
fn metadata_attributes(ir: &CadIr) -> Vec<&cadmpeg_ir::attributes::SourceAttribute> {
    let mut positioned = Vec::new();
    for attribute in &ir.model.attributes {
        if attribute.id.as_str().starts_with("sldprt:")
            && !crate::metadata::is_saved_property_attribute(attribute)
        {
            let position = metadata_source_position(attribute.id.as_str());
            positioned.push(((position.is_none(), position), attribute));
        }
    }
    positioned.sort_by_key(|left| left.0);
    positioned
        .into_iter()
        .map(|(_, attribute)| attribute)
        .collect()
}

/// The generated `SWObjects` metadata records and the `Units` member.
///
/// With `retained_swobjects` the retained section holds every record but the
/// unit code, replayed or patched in place, so only the unit code is
/// generated. An attribute already in `unwritten` is not generated.
fn metadata_payloads(
    ir: &CadIr,
    length_scale: f64,
    retained_swobjects: bool,
    unwritten: &mut UnwrittenAttributes,
    cut: &mut Vec<String>,
) -> (Vec<u8>, Option<Vec<u8>>) {
    let mut objects = Vec::new();
    let mut unit_code = None;
    for attribute in metadata_attributes(ir) {
        let id = attribute.id.as_str();
        if (retained_swobjects && attribute.name != "source_linear_unit_code")
            || unwritten.contains(id)
        {
            continue;
        }
        if let Some(reason) = unwritable(attribute) {
            unwritten.record(id, reason, Held::Nothing);
            continue;
        }
        match MetadataRecord::project(attribute, length_scale, |_| Ok(())) {
            Ok(MetadataRecord::BoundingEnvelope(values)) => {
                objects.extend_from_slice(BOUNDING_ENVELOPE_TOKEN);
                objects.extend_from_slice(&1u32.to_le_bytes());
                for value in values {
                    objects.extend_from_slice(&value.to_le_bytes());
                }
            }
            Ok(MetadataRecord::DefaultReferencePlane { origin, frame }) => {
                objects.extend_from_slice(DEFAULT_REFERENCE_PLANE_TOKEN);
                for value in origin.into_iter().chain(frame) {
                    objects.extend_from_slice(&value.to_le_bytes());
                }
            }
            // The enclosure's opaque serializer prologue and the Part root
            // serializer exist only as retained bytes.
            Ok(
                MetadataRecord::TransformedReferenceEnclosure { .. }
                | MetadataRecord::PartRecord { .. },
            ) => unwritten.record(id, Unwritten::NoSerializer, Held::Nothing),
            Ok(MetadataRecord::SourceLinearUnitCode(code)) => unit_code = Some(code),
            Ok(MetadataRecord::SourceLinearUnitName {
                units,
                byte_len,
                cut: name_cut,
            }) => {
                cut.extend(name_cut);
                objects.extend_from_slice(UNIT_NAME_TOKEN);
                objects.extend_from_slice(&[0xff, 0xfe, 0xff, byte_len]);
                objects.extend(units.into_iter().flat_map(u16::to_le_bytes));
            }
            Err(reason) => unwritten.record(id, reason, Held::Nothing),
        }
    }
    let units = unit_code.map(|code| {
        format!("<Metadata><Property Name=\"SW_UnitsLinear\" Value=\"{code}\"/></Metadata>")
            .into_bytes()
    });
    (objects, units)
}

pub(crate) fn swobjects_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<String, CodecError> {
    let mut attributes = metadata_attributes(ir);
    attributes.retain(|attribute| attribute.name != "source_linear_unit_code");
    let views = (attributes, materials::swobjects_materials(ir).digest_view());
    Ok(cadmpeg_ir::hash::canonical_json_sha256(ctx, &views)?)
}

pub(crate) fn swobjects_material_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<String, CodecError> {
    let materials = materials::swobjects_materials(ir).digest_view();
    Ok(cadmpeg_ir::hash::canonical_json_sha256(ctx, &materials)?)
}

pub(crate) fn swobjects_metadata_identity_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<String, CodecError> {
    let identities = metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name != "source_linear_unit_code")
        .map(|attribute| (&attribute.id, attribute.name.as_str()))
        .collect::<Vec<_>>();
    Ok(cadmpeg_ir::hash::canonical_json_sha256(ctx, &identities)?)
}

type ActiveConfigurationIntent<'a> = (&'a str, Option<&'a str>, Option<u32>, Option<&'a str>);

fn active_configuration_intent(ir: &CadIr) -> Option<ActiveConfigurationIntent<'_>> {
    ir.model
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .map(|configuration| {
            (
                configuration.id.as_str(),
                configuration.name.as_deref(),
                configuration.source_index,
                configuration.native_ref.as_deref(),
            )
        })
}

fn retained_source_baseline<'a>(
    baseline: &'a mut Option<cadmpeg_ir::codec::Decoded>,
    source: Option<&(DecodeContext<'_>, cadmpeg_core::decode::View<'_>)>,
) -> Result<Option<&'a cadmpeg_ir::codec::Decoded>, CodecError> {
    if baseline.is_none() {
        let Some((ctx, root)) = source else {
            return Ok(None);
        };
        let mut decoded = crate::decode::decode(ctx, *root)?;
        sort_arenas(&mut decoded.ir);
        *baseline = Some(decoded);
    }
    Ok(baseline.as_ref())
}

type RetainedDisplaySections = Vec<(String, Vec<u8>)>;

fn retained_display_sections(
    ir: &CadIr,
    baseline: Option<&cadmpeg_ir::codec::Decoded>,
    scan: Option<&crate::container::ContainerScan<'_>>,
) -> Result<Option<RetainedDisplaySections>, CodecError> {
    let (Some(baseline), Some(scan)) = (baseline, scan) else {
        return Ok(None);
    };
    let mut original = baseline.ir.model.tessellations.iter().collect::<Vec<_>>();
    let mut current = ir.model.tessellations.iter().collect::<Vec<_>>();
    original.sort_by(|left, right| left.id.cmp(&right.id));
    current.sort_by(|left, right| left.id.cmp(&right.id));
    if original != current
        || baseline.ir.model.appearances != ir.model.appearances
        || baseline.ir.model.appearance_bindings != ir.model.appearance_bindings
    {
        return Ok(None);
    }
    let mut names = HashSet::new();
    let mut result = Vec::new();
    for mesh in original {
        let provenance = baseline
            .source_fidelity
            .annotations
            .provenance
            .get(mesh.id.as_str())
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "retained tessellation has no proved source member".into(),
                )
            })?;
        let name = provenance.stream();
        if !names.insert(name.to_owned()) {
            continue;
        }
        let mut sources = scan.sections().filter(|source| source.name() == Some(name));
        let source = sources.next().ok_or_else(|| {
            CodecError::NotImplemented("retained display requires a named source member".into())
        })?;
        if sources.next().is_some() {
            return Err(CodecError::NotImplemented(
                "ambiguous retained display member identity".into(),
            ));
        }
        result.push((name.to_owned(), source.payload().to_vec()));
    }
    Ok(Some(result))
}

fn retained_history_sections(
    baseline: &cadmpeg_ir::codec::Decoded,
    scan: &crate::container::ContainerScan<'_>,
) -> Result<Vec<(crate::records::FeatureHistory, String, Vec<u8>)>, CodecError> {
    // Compare the same source-only decode stage that supplied the caller's
    // native records. In particular, decoding infers configuration source
    // indices from the active partition after parsing the Keywords XML.
    let Some(namespace) = baseline.ir.native.namespace("sldprt") else {
        return Ok(Vec::new());
    };
    let native = SldprtNative::load(namespace)?;
    let annotations = &baseline.source_fidelity.annotations;
    native
        .feature_histories
        .into_iter()
        .map(|history| {
            let provenance = annotations.provenance.get(&history.id).ok_or_else(|| {
                CodecError::Malformed("reparsed SLDPRT history has no source owner".into())
            })?;
            let name = provenance.stream();
            let mut sources = scan.sections().filter(|source| source.name() == Some(name));
            let source = sources.next().ok_or_else(|| {
                CodecError::NotImplemented("SLDPRT history requires a named retained member".into())
            })?;
            if sources.next().is_some() {
                return Err(CodecError::NotImplemented(
                    "SLDPRT history has ambiguous retained member identity".into(),
                ));
            }
            Ok((history, name.to_owned(), source.payload().to_vec()))
        })
        .collect()
}

fn retained_history_payload(
    history: &crate::records::FeatureHistory,
    section: &str,
    retained: &[(crate::records::FeatureHistory, String, Vec<u8>)],
) -> Result<Vec<u8>, CodecError> {
    let mut originals = retained
        .iter()
        .filter(|(original, name, _)| original.id == history.id && name == section);
    let original = originals.next();
    if originals.next().is_some() {
        return Err(CodecError::NotImplemented(
            "ambiguous retained SLDPRT history".into(),
        ));
    }
    if let Some((original, _, payload)) = original {
        if original == history {
            return Ok(payload.clone());
        }
    }
    history_payload(history)
}

fn history_payload(history: &crate::records::FeatureHistory) -> Result<Vec<u8>, CodecError> {
    validate_feature_graph(&history.features)?;
    let mut out = String::from("<Keywords");
    if let Some(name) = &history.part_name {
        xml_attribute(&mut out, "Name", name);
    }
    for (name, value) in &history.properties {
        xml_attribute(&mut out, name.as_str(), value);
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
            xml_attribute(out, name.as_str(), value);
        }
        out.push_str("/>");
    };
    let mut roots = history
        .features
        .iter()
        .filter(|feature| feature.tree_parent.is_none())
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
            crate::records::HistoryContent::Element(element) => {
                write_history_element(&mut out, element)?;
            }
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
    let mut by_id = HashMap::new();
    for feature in features {
        if let Some(id) = feature.source_id {
            by_id.insert(id, feature);
        }
    }
    if by_id.len()
        != features
            .iter()
            .filter(|feature| feature.source_id.is_some())
            .count()
    {
        return Err(CodecError::Malformed("duplicate feature source id".into()));
    }
    let mut by_record = HashMap::new();
    for feature in features {
        by_record.insert(feature.id.as_str(), feature);
    }
    if by_record.len() != features.len() {
        return Err(CodecError::Malformed("duplicate feature record id".into()));
    }
    for feature in features {
        let mut seen = HashSet::new();
        let mut parent = feature.parent_source_id();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(CodecError::Malformed("feature parent cycle".into()));
            }
            let node = by_id
                .get(&id)
                .ok_or_else(|| CodecError::Malformed("feature references missing parent".into()))?;
            parent = node.parent_source_id();
        }
        let mut seen = HashSet::new();
        let mut parent = feature.tree_parent_record_id();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(CodecError::Malformed("feature tree cycle".into()));
            }
            let node = by_record.get(id).ok_or_else(|| {
                CodecError::Malformed("feature references missing tree parent".into())
            })?;
            parent = node.tree_parent_record_id();
        }
    }
    Ok(())
}

fn write_feature_xml(
    out: &mut String,
    feature: &crate::records::Feature,
    features: &[crate::records::Feature],
) {
    out.push('<');
    out.push_str(&feature.xml_tag);
    if let Some(id) = feature.source_id {
        xml_attribute(out, "id", &String::from(id));
    }
    xml_attribute(out, "Name", &feature.name);
    xml_attribute(out, "Type", &feature.kind);
    if feature.suppressed {
        xml_attribute(out, "Suppressed", "true");
    }
    for (name, value) in &feature.properties {
        xml_attribute(out, name.as_str(), value);
    }
    out.push('>');
    let write_dimension = |out: &mut String, name: &str, value: &str| {
        out.push_str("<Dimension");
        xml_attribute(out, "Name", name);
        if let Some(properties) = feature.dimension_properties.get(name) {
            for (property, value) in properties {
                xml_attribute(out, property.as_str(), value);
            }
        }
        out.push('>');
        xml_text(out, value);
        out.push_str("</Dimension>");
    };
    let mut children = features
        .iter()
        .filter(|child| {
            child.tree_parent_record_id() == Some(feature.id.as_str())
                || (child.tree_parent_record_id().is_none()
                    && child.parent_source_id() == feature.source_id
                    && feature.source_id.is_some())
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|child| child.ordinal);
    let mut emitted_dimensions = HashSet::new();
    let mut emitted_children = HashSet::new();
    if feature.content.is_empty() {
        for (name, value) in &feature.parameters {
            write_dimension(out, name.as_str(), value);
            emitted_dimensions.insert(name.as_str());
        }
        if let Some(text) = &feature.text {
            xml_text(out, text);
        }
    } else {
        for item in &feature.content {
            match item {
                crate::records::FeatureContent::Dimension(name) => {
                    if let Some(value) = feature.parameters.get(name.as_str()) {
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
        if emitted_dimensions.insert(name.as_str()) {
            write_dimension(out, name.as_str(), value);
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

fn write_history_element(
    out: &mut String,
    element: &crate::records::HistoryElement,
) -> Result<(), CodecError> {
    if !valid_xml_name(&element.tag) {
        return Err(CodecError::Malformed(
            "invalid history XML element name".into(),
        ));
    }
    out.push('<');
    out.push_str(&element.tag);
    for (name, value) in &element.attributes {
        xml_attribute(out, name.as_str(), value);
    }
    if element.content.is_empty() {
        out.push_str("/>");
        return Ok(());
    }
    out.push('>');
    for item in &element.content {
        match item {
            crate::records::HistoryElementContent::Element(child) => {
                write_history_element(out, child)?;
            }
            crate::records::HistoryElementContent::Text(text) => xml_text(out, text),
        }
    }
    out.push_str("</");
    out.push_str(&element.tag);
    out.push('>');
    Ok(())
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

/// The display-list member, and the meshes whose auxiliary channels it does
/// not hold.
fn tessellation_payload(
    ir: &CadIr,
    length_scale: f64,
) -> Result<(Vec<u8>, Vec<String>), CodecError> {
    type AuxiliaryWriter = fn(&mut Vec<u8>, &[u32]) -> Result<(), CodecError>;
    let meshes = ir
        .model
        .tessellations
        .iter()
        .map(sequential_tessellation)
        .collect::<Result<Vec<_>, _>>()?;
    let mut omitted = Vec::new();
    let mut out = b"uoTempBodyTessData_c".to_vec();
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(b"uoTempFaceTessData_c");
    let (triangle_count, strip_count) = match meshes.first() {
        Some(mesh) => (
            u32::try_from(mesh.triangle_count()).map_err(|_| {
                CodecError::NotImplemented("tessellation triangle count overflow".into())
            })?,
            u32::try_from(mesh.strip_lengths().len()).map_err(|_| {
                CodecError::NotImplemented("tessellation strip count overflow".into())
            })?,
        ),
        None => (0, 0),
    };
    out.extend_from_slice(&triangle_count.to_le_bytes());
    out.extend_from_slice(&strip_count.to_le_bytes());
    for mesh in &meshes {
        let strips = mesh
            .strip_lengths()
            .iter()
            .flat_map(|run| run.to_le_bytes())
            .collect::<Vec<_>>();
        descriptor(&mut out, 4, 8, 2, mesh.strip_lengths().len(), &strips)?;
        let mut positions = Vec::with_capacity(mesh.vertex_count() * 12);
        for point in mesh.vertices() {
            for value in [point.x, point.y, point.z] {
                positions.extend_from_slice(
                    &tessellation_f32(value * length_scale, "position")?.to_le_bytes(),
                );
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.vertex_count(), &positions)?;
        let mut normals = Vec::with_capacity(mesh.vertex_normals().len() * 12);
        for normal in mesh.vertex_normals() {
            for value in [normal.x, normal.y, normal.z] {
                normals.extend_from_slice(&tessellation_f32(value, "normal")?.to_le_bytes());
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.vertex_normals().len(), &normals)?;
        // `has_core_tessellation_channels` matches at least three channels, so
        // the remainder after the core three is the complete auxiliary run.
        // The display-list table carries exactly three auxiliary channels; a
        // run of another shape is left out whole and the table's defaults are
        // written.
        let auxiliary_start = usize::from(has_core_tessellation_channels(mesh.channels())) * 3;
        let auxiliary = &mesh.channels()[auxiliary_start..];
        let strip_lengths = mesh
            .strip_lengths()
            .iter()
            .map(|run| usize::try_from(*run))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CodecError::Malformed("tessellation strip length overflow".into()))?;
        let auxiliary = if auxiliary.is_empty()
            || crate::tessellation::auxiliary_channels_are_consistent(&strip_lengths, auxiliary)
        {
            auxiliary
        } else {
            omitted.push(mesh.id.as_str().to_owned());
            &[]
        };
        for channel in auxiliary {
            descriptor(
                &mut out,
                channel.item_size(),
                channel.kind(),
                channel.flags(),
                index_from_u32(channel.count()),
                channel.data(),
            )?;
        }
        let list_c = mesh
            .strip_lengths()
            .iter()
            .map(|run| {
                run.checked_mul(2)
                    .and_then(|value| value.checked_sub(2))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(
                            "tessellation strip is too short or too large".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let append: [AuxiliaryWriter; 3] = [
            |out, _| descriptor(out, 4, 8, 2, 0, &[]),
            |out, list_c| {
                let data = list_c
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<_>>();
                descriptor(out, 4, 8, 2, list_c.len(), &data)
            },
            |out, _| descriptor(out, 1, 8, 2, 0, &[]),
        ];
        // The run is empty or the exact three the table carries, so the
        // synthesized defaults fill in only for a mesh that states none.
        for append in append.iter().skip(auxiliary.len()) {
            append(&mut out, &list_c)?;
        }
    }
    Ok((out, omitted))
}

fn has_core_tessellation_channels(
    channels: &[cadmpeg_ir::tessellation::TessellationChannel],
) -> bool {
    matches!(channels, [strips, positions, normals, ..]
        if (strips.item_size(), strips.kind()) == (4, 8)
            && (positions.item_size(), positions.kind()) == (12, 100)
            && (normals.item_size(), normals.kind()) == (12, 100))
}

fn sequential_tessellation(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
) -> Result<cadmpeg_ir::tessellation::Tessellation, CodecError> {
    if matches!(
        mesh.mesh(),
        cadmpeg_ir::tessellation::TessellationMesh::Strips { .. }
            | cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { .. }
    ) {
        return Ok(mesh.clone());
    }
    let source_vertices = mesh.vertices();
    let source_normals = mesh.vertex_normals();
    let corner_normals = mesh.per_corner_normals();
    let triangles = mesh.triangles();
    let indices = triangles
        .iter()
        .flat_map(|triangle| triangle.iter().copied())
        .map(|index| {
            usize::try_from(index)
                .map_err(|_| CodecError::Malformed("tessellation index is out of bounds".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let positions = indices
        .iter()
        .map(|index| source_vertices[*index])
        .collect::<Vec<_>>();
    let normals = if corner_normals.is_empty() {
        if source_normals.is_empty() {
            Vec::new()
        } else {
            indices
                .iter()
                .map(|index| source_normals[*index])
                .collect::<Vec<_>>()
        }
    } else {
        corner_normals
    };
    let mut channels = Vec::new();
    for channel in mesh.channels() {
        if usize::try_from(channel.count()).ok() != Some(source_vertices.len()) {
            channels.push(channel.clone());
            continue;
        }
        let item_size = usize::try_from(channel.item_size())
            .map_err(|_| CodecError::Malformed("tessellation channel item size overflow".into()))?;
        let data = indices
            .iter()
            .flat_map(|index| {
                let start = index * item_size;
                channel.data()[start..start + item_size].iter().copied()
            })
            .collect();
        channels.push(
            cadmpeg_ir::tessellation::TessellationChannel::new(
                channel.addressing().clone(),
                channel.item_size(),
                channel.kind(),
                channel.flags(),
                data,
            )
            .map_err(|err| CodecError::Malformed(err.to_string()))?,
        );
    }
    // One triangle per strip: the SLDPRT display list states strips, so a
    // triangle list is written as three-vertex runs.
    let rows = if normals.is_empty() {
        strip_rows(&positions)?.map_or(
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            |strips| cadmpeg_ir::tessellation::TessellationMesh::Strips { strips },
        )
    } else {
        let shaded = positions
            .into_iter()
            .zip(normals)
            .map(|(position, normal)| cadmpeg_ir::tessellation::ShadedVertex { position, normal })
            .collect::<Vec<_>>();
        strip_rows(&shaded)?.map_or(
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            |strips| cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { strips },
        )
    };
    // The display list holds no face grouping, texture or feature-edge
    // record; the export accounting charges each.
    mesh.clone()
        .with_triangle_groups(Vec::new())
        .and_then(|mesh| mesh.with_texture_assignments(Vec::new()))
        .and_then(|mesh| mesh.with_texture_coordinates(Vec::new()))
        .and_then(|mesh| mesh.with_feature_edges(Vec::new()))
        .and_then(|mesh| mesh.with_mesh(rows, channels))
        .map_err(|err| CodecError::Malformed(err.to_string()))
}

/// Cut a corner-expanded vertex run into one three-vertex strip per triangle.
fn strip_rows<V: Clone>(
    vertices: &[V],
) -> Result<Option<cadmpeg_ir::tessellation::Strips<V>>, CodecError> {
    let mut strips = Vec::with_capacity(vertices.len() / 3);
    for corners in vertices.chunks(3) {
        strips.push(
            cadmpeg_ir::tessellation::Strip::new(corners.to_vec()).ok_or_else(|| {
                CodecError::Malformed("tessellation triangle spans fewer than three corners".into())
            })?,
        );
    }
    Ok(cadmpeg_ir::tessellation::Strips::new(strips))
}

fn tessellation_f32(value: f64, role: &str) -> Result<f32, CodecError> {
    f32_from_f64(value).ok_or_else(|| {
        CodecError::malformed(format_args!("SLDPRT tessellation {role} exceeds f32 range"))
    })
}

fn descriptor(
    out: &mut Vec<u8>,
    item_size: u32,
    kind: u32,
    flags: u32,
    count: usize,
    data: &[u8],
) -> Result<(), CodecError> {
    let count = u32::try_from(count).map_err(|_| {
        CodecError::NotImplemented("SLDPRT display-list channel count exceeds u32".into())
    })?;
    out.extend_from_slice(&item_size.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(data);
    Ok(())
}

/// `error` with the record it concerns named as a whole token (§3.2), so the
/// encode retry can find the body that owns it.
fn named_record(error: CodecError, id: &str) -> CodecError {
    match error {
        CodecError::NotImplemented(message) => {
            CodecError::NotImplemented(format!("{message}: {id}"))
        }
        CodecError::Malformed(message) => CodecError::malformed(format_args!("{message}: {id}")),
        CodecError::InvalidInput(message) => CodecError::InvalidInput(format!("{message}: {id}")),
        other => other,
    }
}

pub(crate) fn brep_body(
    ir: &CadIr,
    length_scale: f64,
    schema_32001: bool,
) -> Result<Vec<u8>, CodecError> {
    if let Some(body) = ir
        .geometry
        .bodies
        .iter()
        .find(|body| matches!(body.kind, BodyKind::Wire | BodyKind::General))
    {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT cannot write {:?} body {} as a solid lump",
            body.kind, body.id
        )));
    }
    let mut next = 2u16;
    let derived_sphere_seam_curves = ir
        .geometry
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
            ) && curve
                .id
                .as_str()
                .starts_with("sldprt:brep:curve#sphere-seam-face:")
        })
        .map(|curve| curve.id.clone())
        .collect::<HashSet<_>>();
    let surfaces = ir
        .geometry
        .surfaces
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let curves = ir
        .geometry
        .curves
        .iter()
        .filter(|item| !derived_sphere_seam_curves.contains(&item.id))
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let points = ir
        .geometry
        .points
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let vertices = ir
        .geometry
        .vertices
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let edges = ir
        .geometry
        .edges
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let coedges = ir
        .geometry
        .coedges
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    // The sole FIN of each vertex loop (§4, coedge `00 11` marker `0x3f`).
    let vertex_loop_fins = ir
        .geometry
        .loops
        .iter()
        .filter_map(|item| match &item.boundary {
            LoopBoundary::Vertex { vertex, .. } => Some((item, vertex)),
            LoopBoundary::Ring(_) => None,
        })
        .map(|(item, vertex)| Ok((item.id.clone(), (take_attr(&mut next)?, vertex))))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let loops = ir
        .geometry
        .loops
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|item| Ok((item.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let body_attrs = ir
        .geometry
        .bodies
        .iter()
        .map(|body| Ok((body.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let vertex_owners = ownership::vertex_body_owners(ir, &body_attrs)?;
    let reversed_edges = edge_direction::reversed_edges(ir)?;
    let mut out = Vec::new();
    for surface in &ir.geometry.surfaces {
        // A procedural carrier is written as the solved shape the source
        // retains for it; its construction is charged by the arena coverage.
        let solved = surface.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "carrier has no solved geometry: {}",
                surface.id.as_str()
            ))
        })?;
        if let SolvedSurfaceGeometry::Nurbs(nurbs) = solved {
            write_nurbs_surface(
                &mut out,
                surfaces[&surface.id],
                nurbs,
                &mut next,
                length_scale,
                surface.id.as_str(),
            )?;
            continue;
        }
        let (kind, values) = surface_values(
            &SurfaceGeometry::Solved(solved.clone()),
            surface_reference(solved),
            length_scale,
        )
        .map_err(|error| named_record(error, surface.id.as_str()))?;
        compact(
            &mut out,
            kind,
            surfaces[&surface.id],
            surface_marker(solved),
            &values,
        );
    }
    for curve in &ir.geometry.curves {
        if derived_sphere_seam_curves.contains(&curve.id) {
            continue;
        }
        let solved = curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "carrier has no solved geometry: {}",
                curve.id.as_str()
            ))
        })?;
        if let SolvedCurveGeometry::Nurbs(nurbs) = solved {
            write_nurbs_curve(
                &mut out,
                curves[&curve.id],
                nurbs,
                &mut next,
                length_scale,
                curve.id.as_str(),
            )?;
            continue;
        }
        let (kind, values) = curve_values(&CurveGeometry::Solved(solved.clone()), length_scale)
            .map_err(|error| named_record(error, curve.id.as_str()))?;
        compact(
            &mut out,
            kind,
            curves[&curve.id],
            if reversed_edges.curves.contains(&curve.id) {
                CURVE_AGAINST_EDGES_MARKER
            } else {
                CURVE_ALONG_EDGES_MARKER
            },
            &values,
        );
    }
    let face_owners = ir
        .geometry
        .faces
        .iter()
        .map(|face| Ok((face.id.clone(), take_attr(&mut next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let face_colors = face_colors(ir);
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
    for point in &ir.geometry.points {
        tag(&mut out, 0x1d);
        be16(&mut out, points[&point.id]);
        be32(&mut out, 0);
        out.extend_from_slice(&[0; 8]);
        for value in [
            point.position().get().x,
            point.position().get().y,
            point.position().get().z,
        ] {
            bef64(&mut out, value * length_scale);
        }
    }
    for vertex in &ir.geometry.vertices {
        tag(&mut out, 0x12);
        be16(&mut out, vertices[&vertex.id]);
        be32(&mut out, 0);
        for value in [0, 0, 0, 0, points[&vertex.point]] {
            be16(&mut out, value);
        }
        if let Some(tolerance) = vertex.tolerance {
            let value = tolerance.get() * length_scale;
            if !value.is_finite() || value <= 0.0 {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT vertex tolerance is outside the finite positive source range: {}",
                    vertex.id
                )));
            }
            bef64(&mut out, value);
        } else {
            out.extend_from_slice(&MAGIC);
        }
        be16(&mut out, vertex_owners[&vertex.id]);
    }
    let mut canonical_fins = HashMap::new();
    for coedge in &ir.geometry.coedges {
        if reversed_edges.stored_sense(&coedge.edge, coedge.sense) == Sense::Forward {
            canonical_fins
                .entry(&coedge.edge)
                .or_insert(coedges[&coedge.id]);
        }
    }
    for edge in &ir.geometry.edges {
        let ends = edge_ends(edge).map_err(|error| named_record(error, edge.id.as_str()))?;
        let owner = vertex_owners.get(&ends.start).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "SLDPRT EDGE start has no BODY owner: {}",
                edge.id
            ))
        })?;
        if vertex_owners.get(&ends.end) != Some(owner) {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT EDGE endpoints do not have one BODY owner: {}",
                edge.id
            )));
        }
        tag(&mut out, 0x10);
        be16(&mut out, edges[&edge.id]);
        be32(&mut out, 0);
        be16(&mut out, 0);
        out.extend_from_slice(&MAGIC);
        // The canonical forward FIN, stated so that an edge with several
        // forward FINs still names one (§4).
        let canonical = canonical_fins.get(&edge.id).copied().unwrap_or(0);
        for value in [
            canonical,
            0,
            0,
            edge.curve()
                .and_then(|id| curves.get(id))
                .copied()
                .unwrap_or(0),
            0,
            0,
        ] {
            be16(&mut out, value);
        }
        be16(&mut out, *owner);
    }
    let mut edges_by_id = HashMap::new();
    for edge in &ir.geometry.edges {
        edges_by_id.entry(&edge.id).or_insert(edge);
    }
    let mut loops_by_id = HashMap::<_, Vec<_>>::new();
    for lp in &ir.geometry.loops {
        loops_by_id.entry(&lp.id).or_default().push(lp);
    }
    let mut faces_by_id = HashMap::new();
    for face in &ir.geometry.faces {
        faces_by_id.entry(&face.id).or_insert(face);
    }
    for coedge in &ir.geometry.coedges {
        tag(&mut out, 0x11);
        be16(&mut out, coedges[&coedge.id]);
        let edge = edges_by_id.get(&coedge.edge).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "coedge references missing edge: {}",
                coedge.id
            ))
        })?;
        let ends = edge_ends(edge).map_err(|error| named_record(error, edge.id.as_str()))?;
        let forward_vertex = if coedge.sense == Sense::Forward {
            &ends.end
        } else {
            &ends.start
        };
        let (next, previous) = loops_by_id
            .get(&coedge.owner_loop)
            .into_iter()
            .flatten()
            .find_map(|lp| lp.ring_neighbors_of(coedge))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "coedge {} is absent from its owning loop ring",
                    coedge.id
                ))
            })?;
        for value in [
            0,
            loops[&coedge.owner_loop],
            coedges[&next],
            coedges[&previous],
            vertices[forward_vertex],
            coedges[&coedge.radial_next],
            edges[&coedge.edge],
            0,
            0,
        ] {
            be16(&mut out, value);
        }
        out.push(
            if reversed_edges.stored_sense(&coedge.edge, coedge.sense) == Sense::Forward {
                0x2b
            } else {
                0x2d
            },
        );
    }
    for lp in &ir.geometry.loops {
        let Some((fin, vertex)) = vertex_loop_fins.get(&lp.id) else {
            continue;
        };
        let vertex = vertices.get(*vertex).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "vertex loop references a missing vertex: {}",
                lp.id
            ))
        })?;
        tag(&mut out, 0x11);
        be16(&mut out, *fin);
        for value in [0, loops[&lp.id], *fin, *fin, vertex, 0, 0, 0, 0] {
            be16(&mut out, value);
        }
        out.push(0x3f);
    }
    for lp in &ir.geometry.loops {
        let face = faces_by_id.get(&lp.face).copied().ok_or_else(|| {
            CodecError::malformed(format_args!("loop references missing face: {}", lp.id))
        })?;
        let position = face
            .loops
            .iter()
            .position(|id| id == &lp.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("face does not own referenced loop: {}", lp.id))
            })?;
        let next_loop = face
            .loops
            .iter()
            .nth(position + 1)
            .map_or(0, |id| loops[id]);
        let first_fin = match vertex_loop_fins.get(&lp.id) {
            Some((fin, _)) => *fin,
            None => {
                coedges[lp
                    .coedges()
                    .first()
                    .ok_or_else(|| CodecError::malformed(format_args!("empty loop: {}", lp.id)))?]
            }
        };
        tag(&mut out, 0x0f);
        be16(&mut out, loops[&lp.id]);
        be32(&mut out, 0);
        for value in [0, first_fin, faces[&lp.face], next_loop] {
            be16(&mut out, value);
        }
    }
    for face in &ir.geometry.faces {
        tag(&mut out, 0x0e);
        be16(&mut out, faces[&face.id]);
        be32(&mut out, 0);
        be16(&mut out, face_owners[&face.id]);
        out.extend_from_slice(&MAGIC);
        let first =
            face.loops.iter().next().ok_or_else(|| {
                CodecError::malformed(format_args!("face has no loop: {}", face.id))
            })?;
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
        &HierarchyIdentities {
            faces: &faces,
            surfaces: &surfaces,
            body_attrs: &body_attrs,
            face_owners: &face_owners,
            color_attrs: &color_attrs,
            face_color_definition,
            schema_32001,
        },
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

/// Attribute identities of the faces and surfaces that the native and typed
/// hierarchy writers share.
#[derive(Clone, Copy)]
struct HierarchyIdentities<'a> {
    body_attrs: &'a HashMap<cadmpeg_ir::ids::BodyId, u16>,
    faces: &'a HashMap<cadmpeg_ir::ids::FaceId, u16>,
    surfaces: &'a HashMap<cadmpeg_ir::ids::SurfaceId, u16>,
    face_owners: &'a HashMap<cadmpeg_ir::ids::FaceId, u16>,
    color_attrs: &'a HashMap<cadmpeg_ir::ids::FaceId, u16>,
    face_color_definition: Option<(u16, u16)>,
    schema_32001: bool,
}

fn write_body_hierarchy(
    ir: &CadIr,
    identities: &HierarchyIdentities<'_>,
    next: &mut u16,
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let HierarchyIdentities {
        body_attrs,
        faces,
        surfaces,
        face_owners,
        color_attrs,
        face_color_definition,
        schema_32001,
    } = *identities;
    let shells = ir
        .geometry
        .shells
        .iter()
        .map(|shell| (shell.id.clone(), shell))
        .collect::<HashMap<_, _>>();
    let mut assigned = HashSet::new();
    for body in &ir.geometry.bodies {
        let root = take_attr(next)?;
        let mut native_regions = Vec::new();
        for region in ir.geometry.body_regions(&body.id) {
            if body.kind == BodyKind::Sheet && region.shells.len() != 1 {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT sheet regions require exactly one shell: {}",
                    body.id
                )));
            }
            let native_region = take_attr(next)?;
            native_regions.push(native_region);
            let mut native_lumps = Vec::new();
            for shell_id in &region.shells {
                let shell = shells.get(shell_id).ok_or_else(|| {
                    CodecError::Malformed("region references missing shell".into())
                })?;
                let mut owned = Vec::new();
                for face in ir.geometry.shell_faces(&shell.id) {
                    let face = &face.id;
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
                    entity51(out, 1, native_region, 0x001d, &[head, 0, 0, 0, 0, 0])?;
                    continue;
                }
                let lump = take_attr(next)?;
                let shell_node = take_attr(next)?;
                let shell_link = take_attr(next)?;
                native_lumps.push(lump);
                entity51(out, 2, lump, 0x001f, &[shell_node, 0, 0, 0, 0, 0])?;
                entity51(out, 2, shell_node, 0x0021, &[shell_link, 0, 0, 0, 0, 0])?;
                entity51(out, 2, shell_link, 0x0023, &[head, 0, 0, 0, 0, 0])?;
            }
            if body.kind != BodyKind::Sheet {
                entity51(
                    out,
                    1,
                    native_region,
                    0x001b,
                    &fixed_refs(&native_lumps, "SLDPRT region has more than six shells")
                        .map_err(|error| named_record(error, body.id.as_str()))?,
                )?;
            }
        }
        let mut root_refs = [0; 6];
        if native_regions.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT body has no regions: {}",
                body.id
            )));
        }
        if native_regions.len() > 5 {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT body has more than five regions: {}",
                body.id
            )));
        }
        root_refs[1..=native_regions.len()].copy_from_slice(&native_regions);
        entity51(out, 2, root, 0x0017, &root_refs)?;
    }
    if assigned.len() != ir.geometry.faces.len() {
        return Err(CodecError::Malformed(
            "face is not assigned to a body".into(),
        ));
    }
    write_typed_body_hierarchy(ir, faces, surfaces, body_attrs, next, out)?;
    if let Some((key_node, definition_node)) = face_color_definition {
        attribute_definition(out, key_node, definition_node)?;
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
        entity51(out, 1, *owner, disc, &refs)?;
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
    body_attrs: &HashMap<cadmpeg_ir::ids::BodyId, u16>,
    next: &mut u16,
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let region_attrs = ir
        .geometry
        .regions
        .iter()
        .map(|region| Ok((region.id.clone(), take_attr(next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let shell_attrs = ir
        .geometry
        .shells
        .iter()
        .map(|shell| Ok((shell.id.clone(), take_attr(next)?)))
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let mut face_shells = HashMap::new();
    for face in &ir.geometry.faces {
        if let Some(shell_attr) = shell_attrs.get(&face.shell) {
            face_shells.insert(face.id.clone(), *shell_attr);
        }
    }

    for (index, body) in ir.geometry.bodies.iter().enumerate() {
        let body_attr = body_attrs[&body.id];
        let first_body_region = ir
            .geometry
            .body_regions(&body.id)
            .next()
            .ok_or_else(|| CodecError::Malformed("body has no typed region".into()))?;
        let first_region = region_attrs[&first_body_region.id];
        let first_shell = first_body_region
            .shells
            .first()
            .map(|shell| shell_attrs[shell])
            .ok_or_else(|| CodecError::Malformed("typed region has no shell".into()))?;
        typed_prefix(
            out,
            0x0c,
            body_attr,
            u32::try_from(index)
                .ok()
                .and_then(|index| 0x1000_0000_u32.checked_add(index))
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT typed topology node index exceeds the 32-bit range".into(),
                    )
                })?,
        );
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

    for (index, shell) in ir.geometry.shells.iter().enumerate() {
        let body = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.shells.iter().any(|candidate| candidate == &shell.id))
            .map(|region| &region.body)
            .ok_or_else(|| CodecError::Malformed("typed shell has no region".into()))?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.shells.iter().any(|candidate| candidate == &shell.id))
            .ok_or_else(|| CodecError::Malformed("typed shell has no region".into()))?;
        typed_prefix(
            out,
            0x0d,
            shell_attrs[&shell.id],
            u32::try_from(index)
                .ok()
                .and_then(|index| 0x2000_0000_u32.checked_add(index))
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT typed topology node index exceeds the 32-bit range".into(),
                    )
                })?,
        );
        let next_shell = region
            .shells
            .iter()
            .skip_while(|candidate| *candidate != &shell.id)
            .nth(1)
            .map_or(1, |id| shell_attrs[id]);
        for value in [
            1,
            body_attrs[body],
            next_shell,
            1,
            1,
            1,
            region_attrs[&region.id],
            1,
        ] {
            typed_ref(out, value)?;
        }
    }

    for (index, region) in ir.geometry.regions.iter().enumerate() {
        let body = body_attrs[&region.body];
        let body_regions = ir
            .geometry
            .body_regions(&region.body)
            .map(|candidate| &candidate.id)
            .collect::<Vec<_>>();
        let position = body_regions
            .iter()
            .position(|candidate| *candidate == &region.id)
            .ok_or_else(|| CodecError::Malformed("body does not reference typed region".into()))?;
        let next_region = body_regions
            .get(position + 1)
            .map_or(1, |id| region_attrs[*id]);
        let previous_region = position
            .checked_sub(1)
            .and_then(|position| body_regions.get(position))
            .map_or(1, |id| region_attrs[*id]);
        let shell_head = region
            .shells
            .first()
            .map(|id| shell_attrs[id])
            .ok_or_else(|| CodecError::Malformed("typed region has no shell".into()))?;
        typed_prefix(
            out,
            0x13,
            region_attrs[&region.id],
            u32::try_from(index)
                .ok()
                .and_then(|index| 0x3000_0000_u32.checked_add(index))
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT typed topology node index exceeds the 32-bit range".into(),
                    )
                })?,
        );
        for value in [1, body, next_region, previous_region, shell_head] {
            typed_ref(out, value)?;
        }
        out.push(b'S');
    }

    for (index, face) in ir.geometry.faces.iter().enumerate() {
        let shell = face_shells
            .get(&face.id)
            .copied()
            .ok_or_else(|| CodecError::Malformed("typed face has no shell".into()))?;
        typed_prefix(
            out,
            0x0e,
            faces[&face.id],
            u32::try_from(index)
                .ok()
                .and_then(|index| 0x4000_0000_u32.checked_add(index))
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT typed topology node index exceeds the 32-bit range".into(),
                    )
                })?,
        );
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

/// Emit one Parasolid XT pointer in its shortest encoded form (`parasolid.md` §1).
fn typed_ref(out: &mut Vec<u8>, value: impl Into<u32>) -> Result<(), CodecError> {
    cadmpeg_parasolid::pointer::write(out, value.into()).ok_or_else(|| {
        CodecError::NotImplemented("SLDPRT typed reference exceeds the XT pointer range".into())
    })
}

fn fixed_refs(values: &[u16], message: &str) -> Result<[u16; 6], CodecError> {
    if values.len() > 6 {
        return Err(CodecError::NotImplemented(message.into()));
    }
    let mut refs = [0; 6];
    refs[..values.len()].copy_from_slice(values);
    Ok(refs)
}

/// The colour of each face that states one: its own colour, else that of its
/// first face appearance binding with a base colour.
pub(crate) fn face_colors(ir: &CadIr) -> HashMap<cadmpeg_ir::ids::FaceId, Color> {
    face_colors_and_unwritten_bindings(ir).0
}

/// The face colours, and the face appearance bindings the colours do not
/// hold: one whose appearance is missing or states no base colour, or whose
/// colour differs from one the face already states.
pub(super) fn face_colors_and_unwritten_bindings(
    ir: &CadIr,
) -> (HashMap<cadmpeg_ir::ids::FaceId, Color>, Vec<String>) {
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|appearance| (&appearance.id, appearance))
        .collect::<HashMap<_, _>>();
    let mut colors = ir
        .geometry
        .faces
        .iter()
        .filter_map(|face| face.color.map(|color| (face.id.clone(), color)))
        .collect::<HashMap<_, _>>();
    let mut unwritten = Vec::new();
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Face(face) = &binding.target.entity else {
            continue;
        };
        let Some(color) = appearances
            .get(&binding.appearance)
            .and_then(|appearance| appearance.base_color)
        else {
            unwritten.push(binding.id.as_str().to_owned());
            continue;
        };
        if *colors.entry(face.clone()).or_insert(color) != color {
            unwritten.push(binding.id.as_str().to_owned());
        }
    }
    (colors, unwritten)
}

fn entity53(out: &mut Vec<u8>, attr: u16, color: Color) {
    tag(out, 0x53);
    be32(out, 3);
    be16(out, attr);
    for value in [color.r(), color.g(), color.b()] {
        bef64(out, value);
    }
}

fn attribute_definition(
    out: &mut Vec<u8>,
    key_node: u16,
    definition_node: u16,
) -> Result<(), CodecError> {
    const FAMILY: &[u8] = b"SDL/TYSA_COLOUR";
    tag(out, 0x4f);
    be32(
        out,
        u32::try_from(FAMILY.len()).map_err(|_| {
            CodecError::NotImplemented("SLDPRT attribute family name is too long".into())
        })?,
    );
    be16(out, key_node);
    out.extend_from_slice(FAMILY);
    tag(out, 0x50);
    be32(out, 1);
    be16(out, definition_node);
    be16(out, 1);
    be16(out, key_node);
    be32(out, 8001);
    out.extend([0; 8]);
    be16(out, 1);
    let mut legal_owners = [0; 14];
    legal_owners[4] = 1;
    out.extend(legal_owners);
    out.push(2);
    Ok(())
}

fn write_face_list(
    out: &mut Vec<u8>,
    owners: &[u16],
    next: &mut u16,
    disc: u16,
) -> Result<u16, CodecError> {
    let chunks = owners.chunks(5).collect::<Vec<_>>();
    // The list always states a head node: an empty owner run writes the one
    // node that names no face. The head is a value, so the answer is not an
    // index into a list whose length the function would have to floor first.
    let head = take_attr(next)?;
    let attrs = std::iter::once(Ok(head))
        .chain((1..chunks.len()).map(|_| take_attr(next)))
        .collect::<Result<Vec<_>, CodecError>>()?;
    for (index, attr) in attrs.iter().enumerate() {
        let mut refs = [0u16; 6];
        refs[0] = attrs.get(index + 1).copied().unwrap_or(0);
        if let Some(chunk) = chunks.get(index) {
            refs[1..=chunk.len()].copy_from_slice(chunk);
        }
        entity51(out, 2, *attr, disc, &refs)?;
    }
    Ok(head)
}

fn entity51(
    out: &mut Vec<u8>,
    flags: u8,
    attr: u16,
    disc: u16,
    refs: &[u16; 6],
) -> Result<(), CodecError> {
    if !(1..=0x20).contains(&flags) {
        return Err(CodecError::NotImplemented(
            "SLDPRT ATTRIBUTE count is unsupported".into(),
        ));
    }
    tag(out, 0x51);
    be32(out, u32::from(flags));
    be16(out, attr);
    be32(out, 1);
    be16(out, disc);
    for reference in refs {
        be16(out, *reference);
    }
    for _ in 1..flags {
        be16(out, 1);
    }
    Ok(())
}

pub(super) fn surface_values(
    geometry: &SurfaceGeometry,
    reference: cadmpeg_ir::math::Vector3,
    length_scale: f64,
) -> Result<(u8, Vec<f64>), CodecError> {
    let scaled = |value: f64| value * length_scale;
    let result = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            (
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
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            (
                0x33,
                vec![
                    scaled(origin.x),
                    scaled(origin.y),
                    scaled(origin.z),
                    axis.x,
                    axis.y,
                    axis.z,
                    scaled(radius),
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            if ratio != 1.0 {
                return Err(CodecError::NotImplemented(
                    "SLDPRT compact cone carriers encode circular cones only".into(),
                ));
            }
            if !(half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2) {
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
                    scaled(radius),
                    half_angle.sin(),
                    half_angle.cos(),
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            // A negative radius is the same sphere with its normal reversed,
            // which the orientation marker states (`surface_marker`).
            let radius = sphere_surface.radius().get().abs();
            let axis = *axis;
            (
                0x35,
                vec![
                    scaled(center.x),
                    scaled(center.y),
                    scaled(center.z),
                    scaled(radius),
                    axis.x,
                    axis.y,
                    axis.z,
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            // A negative tube radius is the same torus with its minor angle
            // moved by a half turn; its natural normal still points out of
            // the tube, so the magnitude keeps the face sense.
            let minor_radius = torus_surface.minor_radius().get().abs();
            (
                0x36,
                vec![
                    scaled(center.x),
                    scaled(center.y),
                    scaled(center.z),
                    axis.x,
                    axis.y,
                    axis.z,
                    scaled(major_radius),
                    scaled(minor_radius),
                    reference.x,
                    reference.y,
                    reference.z,
                ],
            )
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => {
            let kind = match geometry {
                SurfaceGeometry::Procedural { .. } => "procedural",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => "nurbs",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(_)) => "polygonal",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(_)) => "transformed",
                SurfaceGeometry::Solved(_) => "unknown",
            };
            return Err(CodecError::NotImplemented(format!(
                "semantic SLDPRT writer does not support this surface carrier ({kind})"
            )));
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
    let descriptor = take_attr(next)?;
    let control = take_attr(next)?;
    let multiplicity = take_attr(next)?;
    let knots = take_attr(next)?;
    let degree = u16::try_from(nurbs.degree()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS curve {entity} degree {} exceeds the native u16 field",
            nurbs.degree()
        ))
    })?;
    let control_count = u32::try_from(nurbs.control_points().len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS curve {entity} pole count exceeds the native u32 field"
        ))
    })?;
    let unique = unique_knots(nurbs.knots(), entity)?;
    let knot_count = u32::try_from(unique.len()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS curve {entity} distinct-knot count exceeds the native u32 field"
        ))
    })?;
    let rational = nurbs.weights().is_some();
    tag(out, 0x86);
    be16(out, wrapper);
    be16(out, descriptor);
    out.extend_from_slice(&[0; 8]);
    tag(out, 0x88);
    be16(out, descriptor);
    be16(out, degree);
    be32(out, control_count);
    be16(out, if rational { 4 } else { 3 });
    be32(out, knot_count);
    out.push(0);
    out.extend_from_slice(&[u8::from(nurbs.periodic()), 0, u8::from(rational), 0]);
    for attr in [control, multiplicity, knots] {
        be16(out, attr);
    }
    let poles = homogeneous_poles(nurbs.pole_rows(), length_scale)?;
    f64_array(out, 0x2d, control, poles.into_iter(), entity)?;
    u16_array(
        out,
        multiplicity,
        unique.iter().map(|(_, multiplicity)| multiplicity.get()),
        entity,
    )?;
    f64_array(
        out,
        0x80,
        knots,
        unique.iter().map(|(knot, _)| *knot),
        entity,
    )?;
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
    let u_degree = u16::try_from(nurbs.u_degree()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} u degree exceeds the native u16 field"
        ))
    })?;
    let v_degree = u16::try_from(nurbs.v_degree()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} v degree exceeds the native u16 field"
        ))
    })?;
    if u_degree == 0 || v_degree == 0 {
        return Err(CodecError::Malformed(
            "NURBS surface degree must be positive".into(),
        ));
    }
    let u_count = u32::try_from(nurbs.u_count()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} u pole count exceeds the native u32 field"
        ))
    })?;
    let v_count = u32::try_from(nurbs.v_count()).map_err(|_| {
        CodecError::NotImplemented(format!(
            "SLDPRT NURBS surface {entity} v pole count exceeds the native u32 field"
        ))
    })?;
    let u_unique = unique_knots(nurbs.u_knots(), entity)?;
    let v_unique = unique_knots(nurbs.v_knots(), entity)?;
    let poles = homogeneous_grid_poles(nurbs.pole_grid(), length_scale)?;
    let dimension = if nurbs.weights().is_some() { 4 } else { 3 };
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
    out.push(if nurbs.normal_reversed() { b'-' } else { b'+' });
    be16(out, descriptor);
    be16(out, 0);
    tag(out, 0x7e);
    be16(out, descriptor);
    out.extend_from_slice(&[u8::from(nurbs.u_periodic()), u8::from(nurbs.v_periodic())]);
    be16(out, u_degree);
    be16(out, v_degree);
    be32(out, u_count);
    be32(out, v_count);
    out.extend_from_slice(&[1, 1]);
    be32(out, u_knot_count);
    be32(out, v_knot_count);
    out.push(u8::from(nurbs.weights().is_some()));
    out.extend_from_slice(&[0, 0]);
    out.push(0x0c);
    be16(out, dimension);
    for attr in [control, u_multiplicity, v_multiplicity, u_knots, v_knots] {
        be16(out, attr);
    }
    f64_array(out, 0x2d, control, poles.into_iter(), entity)?;
    u16_array(
        out,
        u_multiplicity,
        u_unique.iter().map(|(_, multiplicity)| multiplicity.get()),
        entity,
    )?;
    u16_array(
        out,
        v_multiplicity,
        v_unique.iter().map(|(_, multiplicity)| multiplicity.get()),
        entity,
    )?;
    f64_array(
        out,
        0x80,
        u_knots,
        u_unique.iter().map(|(knot, _)| *knot),
        entity,
    )?;
    f64_array(
        out,
        0x80,
        v_knots,
        v_unique.iter().map(|(knot, _)| *knot),
        entity,
    )?;
    Ok(())
}

fn take_attr(next: &mut u16) -> Result<u16, CodecError> {
    let attr = *next;
    // Bare records receive each attr and reference as a one-word cell.
    if attr > 0x7fff {
        return Err(CodecError::NotImplemented(
            "SLDPRT attribute space exhausted".into(),
        ));
    }
    *next = next
        .checked_add(1)
        .ok_or_else(|| CodecError::NotImplemented("SLDPRT attribute space exhausted".into()))?;
    Ok(attr)
}

fn append_homogeneous_pole(
    out: &mut Vec<f64>,
    point: FinitePoint3,
    weight: Option<f64>,
    length_scale: f64,
    index: usize,
) -> Result<(), CodecError> {
    let point = point.get();
    let homogeneous =
        [point.x, point.y, point.z].map(|value| value * length_scale * weight.unwrap_or(1.0));
    if !homogeneous.iter().all(|value| value.is_finite()) {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT NURBS pole {index} scaled by its weight has no finite coordinate"
        )));
    }
    out.extend(homogeneous);
    if let Some(weight) = weight {
        out.push(weight);
    }
    Ok(())
}

fn homogeneous_poles(
    poles: &NurbsPoles3<FinitePoint3>,
    length_scale: f64,
) -> Result<Vec<f64>, CodecError> {
    let mut out = Vec::with_capacity(
        poles.count()
            * if matches!(poles, NurbsPoles3::Rational { .. }) {
                4
            } else {
                3
            },
    );
    match poles {
        NurbsPoles3::Polynomial { points } => {
            for (index, point) in points.iter().enumerate() {
                append_homogeneous_pole(&mut out, *point, None, length_scale, index)?;
            }
        }
        NurbsPoles3::Rational { points } => {
            for (index, pole) in points.iter().enumerate() {
                append_homogeneous_pole(
                    &mut out,
                    pole.point,
                    Some(pole.weight.get()),
                    length_scale,
                    index,
                )?;
            }
        }
    }
    Ok(out)
}

fn homogeneous_grid_poles(
    poles: &NurbsPoleGrid<FinitePoint3>,
    length_scale: f64,
) -> Result<Vec<f64>, CodecError> {
    let mut out = Vec::new();
    match poles {
        NurbsPoleGrid::Polynomial { rows } => {
            for (index, point) in rows.iter().flatten().enumerate() {
                append_homogeneous_pole(&mut out, *point, None, length_scale, index)?;
            }
        }
        NurbsPoleGrid::Rational { rows } => {
            for (index, pole) in rows.iter().flatten().enumerate() {
                append_homogeneous_pole(
                    &mut out,
                    pole.point,
                    Some(pole.weight.get()),
                    length_scale,
                    index,
                )?;
            }
        }
    }
    Ok(out)
}

fn unique_knots(knots: &[f64], entity: &str) -> Result<Vec<(f64, NonZeroU16)>, CodecError> {
    let mut out: Vec<(f64, NonZeroU16)> = Vec::new();
    for &knot in knots {
        if let Some((previous, multiplicity)) = out.last_mut() {
            if *previous == knot {
                *multiplicity = multiplicity.checked_add(1).ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT NURBS carrier {entity} knot multiplicity exceeds the native u16 field"
                    ))
                })?;
                continue;
            }
        }
        out.push((knot, NonZeroU16::MIN));
    }
    Ok(out)
}

fn f64_array(
    out: &mut Vec<u8>,
    kind: u8,
    attr: u16,
    values: impl ExactSizeIterator<Item = f64>,
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
        bef64(out, value);
    }
    Ok(())
}

fn u16_array(
    out: &mut Vec<u8>,
    attr: u16,
    values: impl ExactSizeIterator<Item = u16>,
    entity: &str,
) -> Result<(), CodecError> {
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
        be16(out, value);
    }
    Ok(())
}

pub(super) fn curve_values(
    geometry: &CurveGeometry,
    length_scale: f64,
) -> Result<(u8, Vec<f64>), CodecError> {
    let scaled = |value: f64| value * length_scale;
    let result = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            (
                0x1e,
                vec![
                    scaled(origin.x),
                    scaled(origin.y),
                    scaled(origin.z),
                    direction.x,
                    direction.y,
                    direction.z,
                ],
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
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
                    scaled(radius),
                ],
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            (
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
                    scaled(major_radius),
                    scaled(minor_radius),
                ],
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support parabola or hyperbola curves".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support parabola or hyperbola curves".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support degenerate curves".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support composite curves".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support NURBS curves".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support polyline curve carriers".into(),
            ))
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_)) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer does not support transformed curve carriers".into(),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => {
            return Err(CodecError::NotImplemented(
                "semantic SLDPRT writer cannot regenerate an opaque curve".into(),
            ))
        }
    };
    Ok(result)
}

/// Reference direction the compact carrier record states for `geometry`.
///
/// A transformed carrier states the default direction and no basis is read.
/// Both callers pass the same carrier to `surface_values`, which refuses every
/// transformed surface, so a transformed carrier leaves the writer through
/// `CodecError::NotImplemented` and the direction stated here reaches no
/// record.
pub(super) fn surface_reference(geometry: &SolvedSurfaceGeometry) -> cadmpeg_ir::math::Vector3 {
    const DEFAULT_REFERENCE: cadmpeg_ir::math::Vector3 = cadmpeg_ir::math::Vector3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let u_axis = plane_surface.frame().reference().as_raw();
            *u_axis
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            *ref_direction
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let ref_direction = cone_surface.frame().reference().as_raw();
            *ref_direction
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let ref_direction = torus_surface.frame().reference().as_raw();
            *ref_direction
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let ref_direction = sphere_surface.frame().reference().as_raw();
            *ref_direction
        }
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => DEFAULT_REFERENCE,
    }
}

/// Orientation marker of a written compact surface.
const SURFACE_MARKER: u8 = 0x2b;
const SURFACE_REVERSED_MARKER: u8 = 0x2d;

/// The orientation marker of a compact surface record (sldprt.md §7.1):
/// `0x2d` reverses the natural normal of the stored carrier. A sphere of
/// negative radius has its natural normal pointing at its centre, so its
/// magnitude is written reversed. A torus of negative tube radius keeps its
/// normal pointing out of the tube and needs no reversal.
fn surface_marker(surface: &SolvedSurfaceGeometry) -> u8 {
    let reversed =
        matches!(surface, SolvedSurfaceGeometry::Sphere(sphere) if sphere.radius().get() < 0.0);
    if reversed {
        SURFACE_REVERSED_MARKER
    } else {
        SURFACE_MARKER
    }
}

/// Orientation marker of a written compact curve whose stored edges run along
/// its parameter.
const CURVE_ALONG_EDGES_MARKER: u8 = 0x2b;

/// Orientation marker of a written compact curve whose stored edges run against
/// its parameter.
const CURVE_AGAINST_EDGES_MARKER: u8 = 0x2d;

fn compact(out: &mut Vec<u8>, kind: u8, attr: u16, marker: u8, values: &[f64]) {
    tag(out, kind);
    be16(out, attr);
    be32(out, 0);
    out.extend_from_slice(&[0; 10]);
    out.push(marker);
    for value in values {
        bef64(out, *value);
    }
}
fn parasolid_stream(body: &[u8], schema: &str) -> Result<Vec<u8>, CodecError> {
    parasolid_stream_named(body, schema, "partition body")
}

pub(crate) fn parasolid_stream_named(
    body: &[u8],
    schema: &str,
    description: &str,
) -> Result<Vec<u8>, CodecError> {
    let description = description.as_bytes();
    let schema = schema.as_bytes();
    let description_length = u16::try_from(description.len()).map_err(|_| {
        CodecError::NotImplemented("Parasolid stream description exceeds 65535 bytes".into())
    })?;
    let schema_length = u8::try_from(schema.len()).map_err(|_| {
        CodecError::NotImplemented("Parasolid stream schema name exceeds 255 bytes".into())
    })?;
    let mut out = b"PS\0\0".to_vec();
    be16(&mut out, description_length);
    out.extend_from_slice(description);
    out.extend_from_slice(&[0, 0]);
    out.push(schema_length);
    out.extend_from_slice(schema);
    out.extend_from_slice(body);
    Ok(out)
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
    let too_large = || CodecError::NotImplemented("SLDPRT block section is too large".into());
    out.extend_from_slice(
        &u32::try_from(compressed.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(payload.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(preamble.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    out.extend_from_slice(&preamble);
    out.extend_from_slice(&compressed);
    Ok(out)
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
    use super::{
        pmi_local_sha256, take_attr, typed_ref, unique_knots, write_nurbs_surface,
        PMI_LOCAL_DIGEST_ATTRIBUTE,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::nurbs::NurbsSurface;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn pmi_without_its_baseline_is_dropped_and_named_and_an_edit_of_it_is_named() {
        let mut ir = CadIr::empty();
        ir.model.pmi.push(cadmpeg_ir::PmiAnnotation {
            id: cadmpeg_ir::ids::PmiId::mint("sldprt:model:pmi#A1").expect("identity grammar"),
            name: Some("datum A".into()),
            visible: None,
            targets: vec![cadmpeg_ir::PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new("F1")
                    .expect("nonempty source identity"),
            }
            .into()],
            definition: cadmpeg_ir::PmiDefinition::Datum {
                identification: "A".into(),
            },
            native_ref: None,
            native_options: cadmpeg_ir::design::options::NativeOptions::new(),
            source_settings: cadmpeg_ir::design::options::NativeOptions::new(),
        });
        let hash = pmi_local_sha256(&ir).expect("PMI baseline hash");
        ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
                    "sldprt:test"
                )),
            ),
            std::collections::BTreeMap::from([(
                cadmpeg_core::nonblank_const!(PMI_LOCAL_DIGEST_ATTRIBUTE),
                hash,
            )]),
        ));
        assert!(super::reduce::unwritable_content(&mut ir, true)
            .expect("PMI digest")
            .is_empty());
        assert_eq!(ir.model.pmi.len(), 1);

        let cadmpeg_ir::PmiDefinition::Datum { identification } =
            &mut ir.model.pmi.first_mut().expect("PMI annotation").definition
        else {
            panic!("datum definition");
        };
        *identification = "B".into();
        let losses = super::reduce::unwritable_content(&mut ir, true).expect("PMI digest");
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "edit.dropped");
        assert!(losses[0].message.contains("sldprt:model:pmi#A1"));
        assert_eq!(ir.model.pmi.len(), 1);

        // Without the retained SWIFT block, no PMI is written.
        let mut unretained = ir.clone();
        let losses = super::reduce::unwritable_content(&mut unretained, false).expect("PMI digest");
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "writer.pmi-omitted");
        assert!(losses[0].message.contains("sldprt:model:pmi#A1"));
        assert!(unretained.model.pmi.is_empty());

        ir.source = None;
        let losses = super::reduce::unwritable_content(&mut ir, true).expect("PMI digest");
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "writer.pmi-omitted");
        assert!(losses[0].message.contains("sldprt:model:pmi#A1"));
        assert!(ir.model.pmi.is_empty());
    }

    #[test]
    fn writes_surface_degree_from_stored_descriptor() {
        let surface = NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(9, vec![0.0; 20], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0; 4], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![Point3::new(0.0, 0.0, 0.0); 20]
                    .chunks(2_usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                None,
            ),
            false,
        )
        .expect("valid high-degree surface");

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
        let carrier = crate::brep::spline::scan_surface_carriers(&bytes, &mut Vec::new())
            .remove(&2)
            .expect("surface carrier");
        let Some(SolvedSurfaceGeometry::Nurbs(decoded)) = carrier.geometry.solved() else {
            panic!("expected NURBS surface");
        };
        assert_eq!((decoded.u_degree(), decoded.v_degree()), (9, 1));
        assert_eq!((decoded.u_count(), decoded.v_count()), (10, 2));
    }

    #[test]
    fn writes_surface_shape_from_stored_counts() {
        let surface = NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                1,
                vec![0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![Point3::new(0.0, 0.0, 0.0); 12]
                    .chunks(4_usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                None,
            ),
            false,
        )
        .expect("valid asymmetric surface");

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
        let carrier = crate::brep::spline::scan_surface_carriers(&bytes, &mut Vec::new())
            .remove(&2)
            .expect("surface carrier");
        let Some(SolvedSurfaceGeometry::Nurbs(decoded)) = carrier.geometry.solved() else {
            panic!("expected NURBS surface");
        };
        assert_eq!(
            (
                decoded.u_count(),
                decoded.v_count(),
                decoded.u_degree(),
                decoded.v_degree()
            ),
            (3, 4, 2, 1)
        );
    }

    #[test]
    fn writes_typed_reference_boundaries() {
        for (value, expected) in [
            (0x7ffe_u32, vec![0x7f, 0xfe]),
            (0x7fff_u32, vec![0x7f, 0xff]),
            (0x8000_u32, vec![0xff, 0xff, 0, 1]),
            (36_225_u32, vec![0xf2, 0x7e, 0, 1]),
            (65_535_u32, vec![0xff, 0xff, 0, 2]),
            (32_768 * 32_767_u32, vec![0x80, 0x01, 0x7f, 0xff]),
        ] {
            let mut bytes = Vec::new();
            typed_ref(&mut bytes, value).expect("reference is representable");
            assert_eq!(bytes, expected, "encoded reference {value:#x}");
        }

        assert!(matches!(
            typed_ref(&mut Vec::new(), cadmpeg_parasolid::pointer::MAX_VALUE + 1),
            Err(CodecError::NotImplemented(message))
                if message.contains("XT pointer range")
        ));
    }

    #[test]
    fn attribute_space_ends_at_the_one_word_cell_range() {
        let mut next = 0x7fff;
        assert_eq!(take_attr(&mut next).expect("one-word attr"), 0x7fff);
        assert!(matches!(
            take_attr(&mut next),
            Err(CodecError::NotImplemented(message)) if message.contains("attribute space")
        ));
    }

    #[test]
    fn rejects_knot_multiplicity_overflow() {
        let multiplicity_error = unique_knots(
            &vec![0.0; usize::from(u16::MAX) + 1],
            "test:curve#high-multiplicity",
        )
        .expect_err("expected error");

        assert!(matches!(
            multiplicity_error,
            CodecError::NotImplemented(message)
                if message.contains("test:curve#high-multiplicity")
                    && message.contains("knot multiplicity")
        ));
    }
}

/// The two vertices that bound an edge. A `SolidWorks` edge record names both.
fn edge_ends(
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<&cadmpeg_ir::topology::EdgeEnds, CodecError> {
    edge.ends.as_ref().ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "SLDPRT writer does not encode edge {} with no vertex",
            edge.id
        ))
    })
}

#[cfg(test)]
mod conversion_correction_tests {
    use super::tessellation_f32;

    #[test]
    fn tessellation_f32_refuses_a_value_above_f32_max_that_the_cast_rounded_down() {
        // A value a hair above `f32::MAX` rounded to `f32::MAX` under `as`; the
        // sum with `1.0` would not do, since `1.0` is below the spacing of
        // `f64` at that magnitude.
        let just_above = f64::from(f32::MAX) * (1.0 + 1e-10);
        assert!(just_above > f64::from(f32::MAX));
        assert!(tessellation_f32(just_above, "position").is_err());
        assert_eq!(
            tessellation_f32(f64::from(f32::MAX), "position").expect("exact maximum"),
            f32::MAX
        );
        assert!(tessellation_f32(f64::NAN, "position").is_err());
        assert!(tessellation_f32(f64::INFINITY, "position").is_err());
    }
}

#[cfg(test)]
pub(crate) mod tests;
