// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_ir::codec::write::{
    target::{ResolvedTarget, ResolvedWrite},
    ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption, EncodeError, EncodeInput,
    ExportBody, WritePath,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::loss::IgesLossCode;

pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, EncodeError> {
    debug_assert!(
        super::capacity::CAPACITY_FIELDS.len() == 2,
        "IGES card layout declares payload and sequence rows"
    );
    debug_assert!(
        crate::writer::REDUCED_FIELDS.is_empty(),
        "IGES stores no reduced fields"
    );
    match resolved.target() {
        ResolvedTarget::Explicit { index, .. }
        | ResolvedTarget::Inherited { index, .. }
        | ResolvedTarget::Default { index, .. } => {
            let version = crate::IgesVersion::ALL[*index];
            if resolved.preserves_source() {
                replay_or_synthesize(input, version)
            } else {
                synthesized_body(
                    input,
                    version,
                    SynthesisCause::from_resolution(input, resolved),
                )
            }
        }
    }
}

/// A same-format source at the resolved target: replay its retained image when
/// it is intact, otherwise regenerate and report why replay was declined.
fn replay_or_synthesize(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
) -> Result<ExportBody, EncodeError> {
    let replay_failure = match replay_bytes(input) {
        Replay::Replayed { bytes } => return Ok(replayed_body(input.ir, bytes)),
        Replay::Declined(reason) => reason,
    };
    synthesized_body(
        input,
        version,
        SynthesisCause::ReplayDeclined(replay_failure),
    )
}

fn replayed_body(ir: &CadIr, bytes: Vec<u8>) -> ExportBody {
    let mut body = super::body(
        bytes,
        WritePath::VerbatimReplay,
        ArenaCoverage::Complete,
        Vec::new(),
        "preserved source container replayed verbatim",
        super::counts_for_ir(ir),
    );
    body.native_dispositions = cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
        "iges",
        crate::native::ARENAS,
        cadmpeg_ir::codec::write::NativeDisposition::Written,
    );
    body
}

/// Synthesize the document (#217 §5.0).
///
/// A document whose content the writer cannot write as it stands is written
/// in reduced form, each reduction with a loss that names what it omits:
/// first without its IGES native records, last as an empty file. Only a
/// non-finite written value, a caller resource limit, a target refusal or a
/// stored-field capacity fails the write.
fn synthesized_body(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    cause: SynthesisCause,
) -> Result<ExportBody, EncodeError> {
    let error = match synthesized_document(input, version, cause.clone()) {
        Ok(body) => return Ok(body),
        Err(error) => content_failure(input.ir, error)?,
    };
    let mut reductions = Vec::new();
    let mut canonical = input.ir.clone();
    if let Some(namespace) = canonical.native.0.remove("iges") {
        let mut omitted: Vec<&str> = namespace
            .arenas()
            .values()
            .flatten()
            .map(cadmpeg_ir::native::NativeRecord::id)
            .collect();
        omitted.sort_unstable();
        reductions.push(IgesLossCode::WriterArenaOmitted.note(format!(
            "the IGES native records cannot be written ({error}); omitted {} record(s): {}",
            omitted.len(),
            omitted.join(", ")
        )));
        let reduced = EncodeInput {
            ir: &canonical,
            ..input
        };
        match synthesized_document(reduced, version, cause.clone()) {
            Ok(body) => return Ok(with_reductions(body, reductions)),
            Err(error) => {
                content_failure(input.ir, error)?;
            }
        }
    }
    reductions.push(IgesLossCode::NoExportableSolids.note(format!(
        "the document content cannot be written ({error}); wrote an empty file"
    )));
    let empty = CadIr::empty();
    let reduced = EncodeInput {
        ir: &empty,
        ..input
    };
    let mut body = match synthesized_document(reduced, version, cause) {
        Ok(body) => body,
        Err(error) => {
            let error = content_failure(input.ir, error)?;
            debug_assert!(false, "the IGES writer writes an empty document: {error}");
            ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete)
        }
    };
    body.coverage = ArenaCoverage::Declared(ArenaDispositions::uniform(ArenaDisposition::Omitted));
    body.native_dispositions = cadmpeg_ir::codec::write::NativeDispositions::default();
    Ok(with_reductions(body, reductions))
}

fn with_reductions(mut body: ExportBody, mut reductions: Vec<LossNote>) -> ExportBody {
    reductions.append(&mut body.losses);
    body.losses = reductions;
    body
}

/// The content the writer cannot write, or the encode failure that ends the
/// write.
fn content_failure(document: &CadIr, error: super::WriteError) -> Result<String, EncodeError> {
    let reason = match EncodeError::try_from(error) {
        Ok(error) => return Err(error),
        Err(content) => content.to_string(),
    };
    match cadmpeg_ir::codec::write::ValidatedFinding::non_finite(document) {
        Some(finding) => Err(EncodeError::InvalidInput(finding)),
        None => Ok(reason),
    }
}

fn synthesized_document(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    cause: SynthesisCause,
) -> Result<ExportBody, super::WriteError> {
    let synthesis_cause = cause.report_cause();
    let (consumption, loss) = cause.into_fidelity();
    let mut losses = loss.into_iter().collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits: input.limits,
        ..cadmpeg_core::decode::DecodePolicy::default()
    };
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(&ctx, input.ir)?;
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx)?;
    let synthesis =
        super::synthesize_with_admission(&ctx, &recovered.document, version, |entities| {
            for entity in entities {
                if entity.transform.is_some() {
                    session.charge_entities(1, "write IGES transformation entity")?;
                }
                session.charge_entities(1, "write IGES entity")?;
            }
            Ok(())
        })?;
    session.finish()?;
    losses.extend(synthesis.losses.clone());
    if !recovered.sampled_agreements.is_empty() {
        losses.push(IgesLossCode::EdgePcurveAgreementSampled.note(format!(
            "{} recovered edge(s) have coedge curves compared at five parameters; agreement between samples is not established",
            recovered.sampled_agreements.len()
        )));
    }
    if !recovered.sampled_fits.is_empty() {
        losses.push(IgesLossCode::EdgePcurveSampledToBspline.note(format!(
            "{} recovered edge(s) were fitted as B-splines with sampled residual at most {} model length units",
            recovered.sampled_fits.len(), recovered.residual_limit
        )));
    }
    let mut written_native = synthesis.written_native;
    written_native.extend(exact_physical_records(
        input.ir,
        &synthesis.bytes,
        input.limits,
    ));
    let mut regenerated_native = synthesis.regenerated_native;
    regenerated_native.retain(|id| !written_native.contains(id));
    let withdrawn =
        unclaim_dangling_records(input.ir, &mut regenerated_native, &mut written_native);
    let mut body = super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(synthesis.coverage),
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    );
    body.native_dispositions = super::synthesized_native_dispositions(
        input.ir,
        &body.losses,
        &regenerated_native,
        &written_native,
        &withdrawn,
    );
    body.synthesis_cause = synthesis_cause;
    Ok(body)
}

/// Withdraw the written or regenerated claim of each native record that names a
/// native record without one, until no claimed record does, and return the
/// withdrawn identities. A record cannot reappear while a record it names is
/// not written (#209 §2.3 W1).
fn unclaim_dangling_records(
    ir: &CadIr,
    regenerated: &mut std::collections::BTreeSet<String>,
    written: &mut std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    fn named(value: &serde_json::Value, names: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => names.push(text.clone()),
            serde_json::Value::Array(values) => {
                for value in values {
                    named(value, names);
                }
            }
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if key != "id" {
                        named(value, names);
                    }
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }
    let mut withdrawn = std::collections::BTreeSet::new();
    let Some(namespace) = ir.native.namespace("iges") else {
        return withdrawn;
    };
    let records = namespace
        .arenas()
        .values()
        .flatten()
        .map(|record| {
            let mut names = Vec::new();
            if let Ok(fields) = record.fields() {
                named(&serde_json::Value::Object(fields), &mut names);
            }
            (record.id(), names)
        })
        .collect::<Vec<_>>();
    let native = records
        .iter()
        .map(|(id, _)| *id)
        .collect::<std::collections::BTreeSet<_>>();
    loop {
        let claimed = |id: &str| regenerated.contains(id) || written.contains(id);
        let dangling = records
            .iter()
            .filter(|(id, names)| {
                claimed(id)
                    && names
                        .iter()
                        .any(|name| native.contains(name.as_str()) && !claimed(name))
            })
            .map(|(id, _)| (*id).to_owned())
            .collect::<Vec<_>>();
        if dangling.is_empty() {
            return withdrawn;
        }
        for id in dangling {
            regenerated.remove(&id);
            written.remove(&id);
            withdrawn.insert(id);
        }
    }
}

/// Confirm which current physical and Directory records are exactly present in
/// the generated file, comparing output to the document, not to a baseline.
fn exact_physical_records(
    ir: &CadIr,
    bytes: &[u8],
    limits: cadmpeg_core::decode::ResourceLimits,
) -> std::collections::BTreeSet<String> {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let Some(before) = ir.native.namespace("iges") else {
        return std::collections::BTreeSet::new();
    };
    if ["cards", "entities"]
        .iter()
        .all(|arena| before.arenas().get(*arena).is_none_or(Vec::is_empty))
    {
        return std::collections::BTreeSet::new();
    }
    let options = DecodeOptions {
        policy: cadmpeg_core::decode::DecodePolicy {
            limits,
            ..Default::default()
        },
        ..Default::default()
    };
    let Ok(decoded) = crate::IgesCodec.decode(&mut std::io::Cursor::new(bytes), &options) else {
        return std::collections::BTreeSet::new();
    };
    let Some(after) = decoded.ir().native.namespace("iges") else {
        return std::collections::BTreeSet::new();
    };
    let mut exact = std::collections::BTreeSet::new();
    if let (Some(input_cards), Some(output_cards)) =
        (before.arenas().get("cards"), after.arenas().get("cards"))
    {
        // A card's identity is its line number; its content is its section. The
        // output holds an input card while it holds an unclaimed card of the
        // same content, claimed in record order.
        let content = |record: &cadmpeg_ir::NativeRecord| {
            record.fields().ok().map(|mut fields| {
                fields.remove("id");
                serde_json::Value::Object(fields).to_string()
            })
        };
        let mut unclaimed = std::collections::BTreeMap::<String, usize>::new();
        for card in output_cards {
            if let Some(content) = content(card) {
                *unclaimed.entry(content).or_default() += 1;
            }
        }
        for card in input_cards {
            if let Some(count) = content(card)
                .and_then(|content| unclaimed.get_mut(&content))
                .filter(|count| **count > 0)
            {
                *count -= 1;
                exact.insert(card.id().to_owned());
            }
        }
    }
    for arena in ["entities"] {
        let Some(input_records) = before.arenas().get(arena) else {
            continue;
        };
        let Some(output_records) = after.arenas().get(arena) else {
            continue;
        };
        let by_id = output_records
            .iter()
            .map(|record| (record.id(), record))
            .collect::<std::collections::BTreeMap<_, _>>();
        exact.extend(
            input_records
                .iter()
                .filter(|record| {
                    by_id
                        .get(record.id())
                        .is_some_and(|other| *other == *record)
                })
                .map(|record| record.id().to_owned()),
        );
    }
    exact
}

#[derive(Clone)]
enum SynthesisCause {
    Fresh,
    Displaced(String),
    ReplayDeclined(DeclineReason),
}

impl SynthesisCause {
    fn from_resolution(input: EncodeInput<'_>, resolved: &ResolvedWrite<'_>) -> Self {
        if let Some(message) = resolved.displacement_message() {
            Self::Displaced(message)
        } else if resolved.source_preservation_eligible()
            && !source_record_available(input.fidelity)
        {
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable)
        } else {
            Self::Fresh
        }
    }

    fn report_cause(&self) -> Option<cadmpeg_ir::codec::write::SynthesisCause> {
        let reason = match self {
            Self::ReplayDeclined(DeclineReason::Rejected(reason)) => reason.clone(),
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable) => {
                "preserved IGES source image is unavailable".to_owned()
            }
            Self::Fresh | Self::Displaced(_) => return None,
        };
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason })
    }

    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::Displaced(message) => (
                Consumption::NotConsumed,
                Some(IgesLossCode::SourceDialectDisplaced.note(message)),
            ),
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable) => (
                Consumption::Degraded {
                    reason: "preserved IGES source image is unavailable".into(),
                },
                Some(IgesLossCode::PreservedSourceUnavailable.note(
                    "preserved IGES source image is unavailable; semantic regeneration is required",
                )),
            ),
            Self::ReplayDeclined(DeclineReason::Rejected(reason)) => {
                let loss = IgesLossCode::PreservedSourceUnavailable
                    .note(format!("{reason}; IGES is synthesized from the document"));
                (Consumption::Degraded { reason }, Some(loss))
            }
        }
    }
}

fn source_record_available(fidelity: Option<&SourceFidelity>) -> bool {
    fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some()
}

enum Replay {
    Replayed { bytes: Vec<u8> },
    Declined(DeclineReason),
}

#[derive(Clone)]
enum DeclineReason {
    SourceImageUnavailable,
    Rejected(String),
}

impl Replay {
    fn declined_for_record(
        record: Option<&cadmpeg_ir::RetainedSourceRecord>,
        reason: impl Into<String>,
    ) -> Self {
        if record.is_some() {
            Self::Declined(DeclineReason::Rejected(reason.into()))
        } else {
            Self::Declined(DeclineReason::SourceImageUnavailable)
        }
    }
}

fn replay_bytes(input: EncodeInput<'_>) -> Replay {
    let ir = input.ir;
    let fidelity = input.fidelity;
    let Some(_source) = ir
        .source
        .as_ref()
        .filter(|source| source.format() == crate::dialect::FORMAT)
    else {
        return Replay::Declined(DeclineReason::SourceImageUnavailable);
    };
    if fidelity.is_some_and(|fidelity| !fidelity.unrepresented().is_empty()) {
        return Replay::Declined(DeclineReason::Rejected(
            "retained source contains untyped content; synthesis is required".to_owned(),
        ));
    }
    let record = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID));
    let Some(data) = record.and_then(|record| record.data()) else {
        return Replay::Declined(DeclineReason::SourceImageUnavailable);
    };
    let depth = match (ir.scope.model, ir.scope.design) {
        (true, false) => DecodeDepth::Geometry,
        (false, false) => DecodeDepth::Container,
        _ => DecodeDepth::Full,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits: input.limits,
        ..DecodePolicy::default()
    };
    let replays = DecodeContext::from_root_bytes_at(data, &arena, &policy, depth)
        .ok()
        .is_some_and(|(ctx, root)| {
            let fresh = cadmpeg_ir::Codec::decode_with_context(&crate::IgesCodec, &ctx, root);
            let session = ctx.finish_session();
            fresh.is_ok_and(|fresh| fresh.ir() == ir) && session.is_ok()
        });
    if !replays {
        return Replay::declined_for_record(
            record,
            "decoded model no longer matches the preserved IGES source; byte replay skipped",
        );
    }
    Replay::Replayed {
        bytes: data.to_vec(),
    }
}

#[cfg(test)]
mod tests;
