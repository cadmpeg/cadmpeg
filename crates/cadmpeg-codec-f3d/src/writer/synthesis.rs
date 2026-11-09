// SPDX-License-Identifier: Apache-2.0
//! Synthesis admission and reported reductions. Retained bytes never enter here.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    ArenaDispositions, EncodeError, NativeDispositions, ValidatedFinding,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

mod act;
mod appearance;
mod body_keys;
mod design;
mod geometry;
mod materials;
mod native;
mod normalize;
mod prepare;
mod streams;
mod topology;

pub(super) struct Synthesized {
    pub(super) bytes: Vec<u8>,
    pub(super) coverage: ArenaDispositions,
    pub(super) losses: Vec<LossNote>,
    pub(super) native_dispositions: NativeDispositions,
}

/// Which writer failures a reduction step absorbs.
#[derive(Clone, Copy)]
pub(super) enum Reduction {
    /// Only an encoder limitation.
    Limitations,
    /// Any content the writer cannot write; each omitted record is named.
    Content,
}

impl Reduction {
    pub(super) fn reduces(self, error: &crate::writer::WriteError) -> bool {
        use crate::writer::WriteError;
        match self {
            Self::Limitations => declines(error),
            Self::Content => matches!(
                error,
                WriteError::Unsupported(_)
                    | WriteError::Unwritable(_)
                    | WriteError::Codec(
                        CodecError::NotImplemented(_)
                            | CodecError::Malformed(_)
                            | CodecError::InvalidInput(_)
                            | CodecError::Truncated { .. }
                    )
            ),
        }
    }
}

/// Author an archive from the document (#217 §5.0).
///
/// A document whose content the writer cannot write as it stands is written
/// in reduced form, each reduction with a loss that names what it omits:
/// first without its F3D native records, then without each body or face the
/// writer cannot write, and last as an empty geometry carrier. Only a
/// non-finite written value, a caller resource limit, or a target refusal
/// fails the write.
pub(super) fn write(
    target: &CadIr,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<Synthesized, EncodeError> {
    face_key_capacity(target)?;
    let error = match write_document(target, target.clone(), limits, Reduction::Limitations) {
        Ok(synthesized) => return Ok(synthesized),
        Err(error) => content_error(target, error)?,
    };
    let mut canonical = target.clone();
    let mut reductions = Vec::new();
    if let Some(namespace) = canonical.native.0.remove("f3d") {
        let mut omitted: Vec<&str> = namespace
            .arenas()
            .values()
            .flatten()
            .map(cadmpeg_ir::native::NativeRecord::id)
            .collect();
        omitted.sort_unstable();
        reductions.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
            "the F3D native records cannot be written ({error}); omitted {} record(s): {}",
            omitted.len(),
            omitted.join(", ")
        )));
        match write_document(target, canonical.clone(), limits, Reduction::Limitations) {
            Ok(synthesized) => return Ok(reduced(synthesized, reductions)),
            Err(error) => {
                content_error(target, error)?;
            }
        }
    }
    let error = match write_document(target, canonical, limits, Reduction::Content) {
        Ok(synthesized) => return Ok(reduced(synthesized, reductions)),
        Err(error) => content_error(target, error)?,
    };
    reductions.push(F3dLossCode::WriterNoExportableSolids.note(format!(
        "unsupported writer content remains after reduction: {error}; wrote an empty geometry carrier"
    )));
    match write_document(target, CadIr::empty(), limits, Reduction::Content) {
        Ok(synthesized) => Ok(reduced(synthesized, reductions)),
        Err(error) => {
            let error = content_error(target, error)?;
            debug_assert!(false, "the F3D writer writes an empty document: {error}");
            Ok(Synthesized {
                bytes: Vec::new(),
                coverage: super::generate::coverage(
                    false,
                    &CadIr::empty(),
                    &crate::native::F3dNative::default(),
                ),
                losses: reductions,
                native_dispositions: NativeDispositions::for_namespace(
                    "f3d",
                    crate::native::F3D_ARENA_NAMES,
                    cadmpeg_ir::codec::write::NativeDisposition::Dropped,
                ),
            })
        }
    }
}

/// Each written face's native key fits its stored field.
fn face_key_capacity(target: &CadIr) -> Result<(), EncodeError> {
    let records = target
        .native
        .namespace("f3d")
        .and_then(|namespace| namespace.arenas().get("face_native_keys"))
        .into_iter()
        .flatten();
    for record in records {
        let written = target
            .model
            .faces
            .iter()
            .any(|face| face.native_ref.as_deref() == Some(record.id()));
        let key = record.field("asm_face_key").and_then(|key| key.as_u64());
        if let (true, Some(key)) = (written, key) {
            super::capacity::asm_face_key(key)?;
        }
    }
    Ok(())
}

fn reduced(mut synthesized: Synthesized, mut reductions: Vec<LossNote>) -> Synthesized {
    reductions.append(&mut synthesized.losses);
    synthesized.losses = reductions;
    synthesized
}

/// The content failure in `error`, or the encode failure that ends the write.
fn content_error(
    document: &CadIr,
    error: crate::writer::WriteError,
) -> Result<crate::writer::WriteError, EncodeError> {
    use crate::writer::WriteError;
    match error {
        WriteError::Codec(CodecError::ResourceLimit(limit)) => {
            Err(EncodeError::ResourceLimit(limit))
        }
        WriteError::Codec(CodecError::Io(error)) => Err(EncodeError::Io(error)),
        WriteError::Codec(CodecError::UnsupportedTarget(refusal)) => {
            Err(EncodeError::UnsupportedTarget(refusal))
        }
        error => ValidatedFinding::non_finite(document)
            .map_or(Ok(error), |proof| Err(EncodeError::InvalidInput(proof))),
    }
}

/// Author an archive from `prepared`, reducing unsupported records locally.
/// Arena and native coverage are stated against `target`.
fn write_document(
    target: &CadIr,
    mut prepared: CadIr,
    limits: cadmpeg_core::decode::ResourceLimits,
    reduction: Reduction,
) -> Result<Synthesized, crate::writer::WriteError> {
    let mut losses = Vec::new();
    topology::repair_body_regions(&mut prepared, &mut losses, limits)?;
    topology::omit_ambiguous_points(&mut prepared, &mut losses)?;
    topology::omit_unresolved_coedge_faces(&mut prepared, &mut losses, limits)?;
    normalize::feature_suppression(&mut prepared, &mut losses);
    normalize::occurrence_placements(&mut prepared, &mut losses);
    normalize::feature_parameter_driving(&mut prepared, &mut losses);
    normalize::sketch_placements(&mut prepared, &mut losses);
    normalize::asset_media_types(&mut prepared, &mut losses);
    normalize::mesh_import_vertices(&mut prepared, &mut losses);
    normalize::stale_sketch_distances(&mut prepared, &mut losses);
    let original = super::primitives::f3d_native(&prepared)?.unwrap_or_default();
    prepare::auxiliary(&mut prepared, &original, &mut losses, limits)?;
    appearance::prepare(&mut prepared, &original, &mut losses);
    native::reconcile(&mut prepared, original, &mut losses, limits)?;
    let mut bytes = Vec::new();
    let mut regenerated_parameters = std::collections::BTreeSet::new();
    let mut written_recipes = std::collections::BTreeSet::new();
    let mut dropped_records = std::collections::BTreeSet::new();
    let coverage = match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
        Ok(generated) => {
            losses.extend(generated.losses);
            regenerated_parameters = generated.regenerated_parameters;
            written_recipes = generated.written_recipes;
            dropped_records = generated.dropped_records;
            generated.coverage
        }
        Err(error) if reduction.reduces(&error) => {
            prepare::solved_carriers(&mut prepared, &mut losses)?;
            prepare::unreferenced_composite_curves(&mut prepared, &mut losses, limits)?;
            prepare::pcurves(&mut prepared, &mut losses)?;
            bytes.clear();
            match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
                Ok(generated) => {
                    losses.extend(generated.losses);
                    regenerated_parameters = generated.regenerated_parameters;
                    written_recipes = generated.written_recipes;
                    dropped_records = generated.dropped_records;
                    generated.coverage
                }
                Err(error) if reduction.reduces(&error) => {
                    prepared = topology::admit_bodies(&prepared, &mut losses, limits, reduction)?;
                    native::repair_keys(&mut prepared, limits)?;
                    bytes.clear();
                    match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
                        Ok(generated) => {
                            losses.extend(generated.losses);
                            regenerated_parameters = generated.regenerated_parameters;
                            written_recipes = generated.written_recipes;
                            dropped_records = generated.dropped_records;
                            generated.coverage
                        }
                        Err(error) if reduction.reduces(&error) => {
                            // §5.0: a limitation the reductions cannot
                            // localize is still not a refusal. Shed the
                            // native design the generator cannot express
                            // and retry; every omission is reported.
                            native::shed_design(&mut prepared, limits)?;
                            bytes.clear();
                            match super::generate::write_with_limits(&prepared, &mut bytes, limits)
                            {
                                Ok(generated) => {
                                    losses.extend(generated.losses);
                                    regenerated_parameters = generated.regenerated_parameters;
                                    written_recipes = generated.written_recipes;
                                    dropped_records = generated.dropped_records;
                                    generated.coverage
                                }
                                Err(error) if reduction.reduces(&error) => {
                                    losses.push(F3dLossCode::WriterNoExportableSolids.note(format!(
                                        "unsupported writer content remains after shedding design: {error}; wrote an empty geometry carrier"
                                    )));
                                    prepared = CadIr::empty();
                                    bytes.clear();
                                    match super::generate::write_with_limits(
                                        &prepared, &mut bytes, limits,
                                    ) {
                                        Ok(generated) => {
                                            losses.extend(generated.losses);
                                            regenerated_parameters =
                                                generated.regenerated_parameters;
                                            written_recipes = generated.written_recipes;
                                            dropped_records = generated.dropped_records;
                                            generated.coverage
                                        }
                                        // Terminal §5.0 reduction: even the empty
                                        // carrier is unwritable. Report it and
                                        // return the empty dispositions; only a
                                        // §5.0 precondition still refuses.
                                        Err(error) if reduction.reduces(&error) => {
                                            bytes.clear();
                                            losses.push(
                                                F3dLossCode::WriterNoExportableSolids.note(format!(
                                                    "unsupported writer content remains after shedding design: {error}; wrote no exportable geometry"
                                                )),
                                            );
                                            super::generate::coverage(
                                                false,
                                                &prepared,
                                                &crate::native::F3dNative::default(),
                                            )
                                        }
                                        Err(error) => return Err(error),
                                    }
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let native = super::primitives::f3d_native(&prepared)?.unwrap_or_default();
    let attributes = super::generate::attributes::represented_attributes(&prepared, &native);
    for attribute in &prepared.model.attributes {
        if attributes.contains(attribute.id.as_str())
            && crate::brep::creation_timestamp_value(attribute).is_some()
            && matches!(attribute.values.first(), Some(Some(cadmpeg_ir::attributes::AttributeValue::Reference(reference))) if !reference.ends_with(":entity#-1"))
        {
            losses.push(F3dLossCode::WriterValueNormalized.note(format!(
                "attribute {}: timestamp successor reference is not representable; wrote the terminal reference",
                attribute.id
            )));
        }
    }
    let invalidated_timestamps = native
        .creation_timestamps
        .iter()
        .filter(|timestamp| {
            prepared.model.attributes.iter().any(|attribute| {
                attributes.contains(attribute.id.as_str())
                    && cadmpeg_asm::brep::attributes::attribute_key(attribute)
                        == timestamp.record_index.to_string()
                    && crate::brep::link_target_for(
                        &prepared.model.bodies,
                        &prepared.model.faces,
                        &prepared.model.edges,
                        &prepared.model.vertices,
                        &prepared.model.coedges,
                        &attribute.target,
                    )
                    .is_some_and(|(row, coedge)| {
                        row == timestamp.target.as_str() && coedge == timestamp.target_coedge
                    })
                    && crate::brep::creation_timestamp_value(attribute).is_none()
            })
        })
        .map(|timestamp| timestamp.id.clone())
        .collect();
    let native_dispositions = native::coverage(
        target,
        &prepared,
        &invalidated_timestamps,
        &regenerated_parameters,
        &written_recipes,
        &dropped_records,
    );
    let coverage = prepare::dispositions(target, &prepared, coverage, &attributes, &mut losses);
    if prepared.model.bodies.is_empty() {
        losses.push(F3dLossCode::WriterNoExportableSolids.note(
            "the document has no body representable by the F3D writer; wrote an empty geometry carrier",
        ));
    }
    Ok(Synthesized {
        bytes,
        coverage,
        losses,
        native_dispositions,
    })
}

/// Internal encoder limitations are reductions. Invalid records and capacity
/// limits remain preconditions. Native/neutral disagreement is an edit, so it
/// selects neutral regeneration rather than an invalid-document refusal.
pub(super) fn declines(error: &crate::writer::WriteError) -> bool {
    matches!(
        error,
        crate::writer::WriteError::Unsupported(_)
            | crate::writer::WriteError::Codec(CodecError::NotImplemented(_))
    )
}
