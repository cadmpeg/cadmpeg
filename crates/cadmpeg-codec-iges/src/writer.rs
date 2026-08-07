// SPDX-License-Identifier: Apache-2.0
//! Bounded IGES Fixed ASCII writing.
//!
//! The writer has two deliberately separate paths. An unchanged decode with a
//! verified document baseline replays its retained source image byte for byte.
//! Otherwise the semantic writer emits the current supported neutral profile
//! and refuses a model or native record it cannot represent. A caller never
//! receives a plausible file after an unsupported value was silently dropped.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::report::{
    CensusBasis, EntityCensus, ExportReport, FidelityResolution, LossKind, LossNote, Severity,
    WritePath,
};
use cadmpeg_ir::{CadIr, SourceFidelity};
use std::collections::BTreeMap;

const ALLOWED_NATIVE_ARENAS: &[&str] = &[
    "cards",
    "display_attributes",
    "entities",
    "product_occurrence_expansion",
];

/// Plan an IGES export, selecting replay only after checking the document
/// baseline and retained source-image integrity.
pub(crate) fn plan(input: EncodeInput<'_>) -> Result<ExportPlan<'_>, CodecError> {
    if let Some(bytes) = replay_bytes(input.ir, input.fidelity)? {
        return Ok(ExportPlan::buffered(
            report(
                input.ir,
                FidelityResolution::Replayed,
                WritePath::VerbatimReplay,
                Vec::new(),
                "preserved source container replayed verbatim",
            ),
            bytes,
        ));
    }

    let source_expected = input
        .ir
        .source
        .as_ref()
        .is_some_and(|source| source.format == "iges");
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some();
    let mut losses = Vec::new();
    if source_expected && !source_available {
        losses.push(
            LossNote::new(
                LossKind::PreservedSourceUnavailable,
                "preserved IGES source image is unavailable; semantic regeneration is required",
            )
            .with_severity(Severity::Blocking),
        );
    }
    let bytes = synthesize(input.ir)?;
    let fidelity = if source_expected && !source_available {
        FidelityResolution::Degraded {
            reason: "preserved IGES source image is unavailable".into(),
        }
    } else if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    Ok(ExportPlan::buffered(
        report(
            input.ir,
            fidelity,
            WritePath::Synthesized,
            losses,
            "IGES Fixed ASCII container regenerated from neutral point geometry",
        ),
        bytes,
    ))
}

fn replay_bytes(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(expected) = ir
        .source
        .as_ref()
        .filter(|source| source.format == "iges")
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
    else {
        return Ok(None);
    };
    if crate::document_digest(ir) != *expected {
        return Ok(None);
    }
    let Some(record) = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID))
    else {
        return Ok(None);
    };
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained IGES source image has no bytes".into(),
        ));
    };
    if record.byte_len != data.len() as u64 || record.sha256 != sha256_hex(data) {
        return Err(CodecError::Malformed(
            "retained IGES source image failed integrity validation".into(),
        ));
    }
    Ok(Some(data.to_vec()))
}

fn report(
    ir: &CadIr,
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<LossNote>,
    note: &str,
) -> ExportReport {
    let mut counts = BTreeMap::new();
    counts.insert("116_point".into(), ir.model.points.len());
    ExportReport {
        format: "iges".into(),
        census: EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts,
        },
        fidelity,
        write_path,
        losses,
        notes: vec![note.into()],
    }
}

fn synthesize(ir: &CadIr) -> Result<Vec<u8>, CodecError> {
    reject_unsupported_model(ir)?;
    reject_unsupported_native(ir)?;

    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    for point in &points {
        let coordinates = [point.position.x, point.position.y, point.position.z];
        if coordinates.iter().any(|value| !value.is_finite()) {
            return Err(CodecError::Malformed(format!(
                "IGES point {} has non-finite coordinates",
                point.id.0
            )));
        }
    }
    let mut entities = Vec::with_capacity(points.len());
    for point in points {
        entities.push(Entity {
            type_code: 116,
            form: 0,
            label: "POINT",
            parameters: format!(
                "116,{},{},{};",
                number(point.position.x),
                number(point.position.y),
                number(point.position.z)
            )
            .into_bytes(),
        });
    }
    encode_file(&entities)
}

fn reject_unsupported_model(ir: &CadIr) -> Result<(), CodecError> {
    let unsupported = [
        ("faces", !ir.model.faces.is_empty()),
        ("loops", !ir.model.loops.is_empty()),
        ("coedges", !ir.model.coedges.is_empty()),
        ("edges", !ir.model.edges.is_empty()),
        ("surfaces", !ir.model.surfaces.is_empty()),
        ("curves", !ir.model.curves.is_empty()),
        ("pcurves", !ir.model.pcurves.is_empty()),
        (
            "procedural_surfaces",
            !ir.model.procedural_surfaces.is_empty(),
        ),
        ("procedural_curves", !ir.model.procedural_curves.is_empty()),
        ("assets", !ir.model.assets.is_empty()),
        ("features", !ir.model.features.is_empty()),
        (
            "feature_input_topologies",
            !ir.model.feature_input_topologies.is_empty(),
        ),
        (
            "feature_result_topologies",
            !ir.model.feature_result_topologies.is_empty(),
        ),
        ("configurations", !ir.model.configurations.is_empty()),
        ("parameters", !ir.model.parameters.is_empty()),
        ("sketches", !ir.model.sketches.is_empty()),
        ("sketch_entities", !ir.model.sketch_entities.is_empty()),
        (
            "sketch_constraints",
            !ir.model.sketch_constraints.is_empty(),
        ),
        ("spatial_sketches", !ir.model.spatial_sketches.is_empty()),
        (
            "spatial_sketch_entities",
            !ir.model.spatial_sketch_entities.is_empty(),
        ),
        (
            "spatial_sketch_constraints",
            !ir.model.spatial_sketch_constraints.is_empty(),
        ),
        ("spreadsheets", !ir.model.spreadsheets.is_empty()),
        (
            "product_definitions",
            !ir.model.product_definitions.is_empty(),
        ),
        ("occurrences", !ir.model.occurrences.is_empty()),
        ("assembly_joints", !ir.model.assembly_joints.is_empty()),
        ("drawings", !ir.model.drawings.is_empty()),
        (
            "semantic_annotations",
            !ir.model.semantic_annotations.is_empty(),
        ),
        (
            "presentation_documents",
            !ir.model.presentation_documents.is_empty(),
        ),
        (
            "view_presentations",
            !ir.model.view_presentations.is_empty(),
        ),
        ("tessellations", !ir.model.tessellations.is_empty()),
        ("appearances", !ir.model.appearances.is_empty()),
        (
            "appearance_bindings",
            !ir.model.appearance_bindings.is_empty(),
        ),
        ("attributes", !ir.model.attributes.is_empty()),
        ("pmi", !ir.model.pmi.is_empty()),
        (
            "presentation_layers",
            !ir.model.presentation_layers.is_empty(),
        ),
    ];
    if let Some((arena, _)) = unsupported.into_iter().find(|(_, present)| *present) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode model arena {arena}"
        )));
    }
    Ok(())
}

fn reject_unsupported_native(ir: &CadIr) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(());
    };
    if let Some((arena, _)) = namespace.arenas.iter().find(|(arena, records)| {
        !records.is_empty() && !ALLOWED_NATIVE_ARENAS.contains(&arena.as_str())
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer cannot preserve native arena {arena}"
        )));
    }
    if let Some(record) = namespace
        .arenas
        .get("entities")
        .into_iter()
        .flatten()
        .find(|record| record.field("entity_type").and_then(|value| value.as_i64()) != Some(116))
    {
        let entity_type = record
            .field("entity_type")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode native entity type {entity_type}"
        )));
    }
    Ok(())
}

struct Entity {
    type_code: u32,
    form: i64,
    label: &'static str,
    parameters: Vec<u8>,
}

fn encode_file(entities: &[Entity]) -> Result<Vec<u8>, CodecError> {
    let global = b"1H,,1H;,7Hcadmpeg,13Hgenerated.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260807.000000,0.001,1000.0,6Hauthor,7Hcadmpeg,11,0,0H,0H;";
    let global_count = global.len().div_ceil(72);
    let mut parameter_sequence = 1_u32;
    let mut directory = Vec::with_capacity(entities.len() * 2);
    let mut parameters = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        let directory_sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES directory sequence overflows".into()))?;
        let parameter_count = entity.parameters.len().div_ceil(64);
        let parameter_count = u32::try_from(parameter_count)
            .map_err(|_| CodecError::Malformed("IGES parameter count overflows".into()))?;
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                parameter_sequence.to_string(),
                "0".into(),
                "0".into(),
                "0".into(),
                "0".into(),
                "0".into(),
                "0".into(),
                "00000000".into(),
            ],
            directory_sequence,
        )?);
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                "0".into(),
                "0".into(),
                parameter_count.to_string(),
                entity.form.to_string(),
                String::new(),
                String::new(),
                entity.label.to_owned(),
                "0".into(),
            ],
            directory_sequence + 1,
        )?);
        for chunk in entity.parameters.chunks(64) {
            parameters.push(parameter_card(
                chunk,
                directory_sequence,
                parameter_sequence,
            ));
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES parameter sequence overflows".into()))?;
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(card(b"Generated by cadmpeg", b'S', 1));
    for (index, chunk) in global.chunks(72).enumerate() {
        bytes.extend(card(
            chunk,
            b'G',
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        ));
    }
    for card_bytes in directory {
        bytes.extend(card_bytes);
    }
    for card_bytes in parameters {
        bytes.extend(card_bytes);
    }
    let directory_count = entities
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::Malformed("IGES directory count overflows".into()))?;
    let directory_count = u32::try_from(directory_count)
        .map_err(|_| CodecError::Malformed("IGES directory count overflows".into()))?;
    let parameter_count = parameter_sequence - 1;
    let terminate = format!(
        "S{start_count:07}G{global_count:07}D{directory_count:07}P{parameter_count:07}",
        start_count = 1
    );
    bytes.extend(card(terminate.as_bytes(), b'T', 1));
    Ok(bytes)
}

fn directory_card(fields: [String; 9], sequence: u32) -> Result<Vec<u8>, CodecError> {
    let mut payload = Vec::with_capacity(72);
    for field in fields {
        if field.len() > 8 {
            return Err(CodecError::Malformed(format!(
                "IGES Directory field is wider than eight bytes: {field}"
            )));
        }
        payload.extend_from_slice(format!("{field:>8}").as_bytes());
    }
    Ok(card(&payload, b'D', sequence))
}

fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> Vec<u8> {
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    let pointer = format!("{directory_sequence:>8}");
    payload[64..].copy_from_slice(pointer.as_bytes());
    card(&payload, b'P', sequence)
}

fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    let width = 72;
    let mut payload = vec![b' '; 80];
    payload[..data.len().min(width)].copy_from_slice(&data[..data.len().min(width)]);
    payload[72] = section;
    let sequence = format!("{sequence:>7}");
    payload[73..].copy_from_slice(sequence.as_bytes());
    payload.push(b'\n');
    payload
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.17}")
    }
}
