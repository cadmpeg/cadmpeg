// SPDX-License-Identifier: Apache-2.0
//! Neutral-first synthesis after a retained layout cannot express an edit.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    ArenaCoverage, ArenaDisposition, Consumption, EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{Annotations, CadIr};

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

mod design;
mod geometry;
pub(super) mod parameters;
mod presentation;

/// Why synthesis replaced an eligible retained layout.
pub(super) enum SynthesisCause {
    Fresh,
    SourceImageUnavailable,
    SourceUnrepresented {
        classes: cadmpeg_ir::document::ArenaClasses,
    },
    PatchDeclined {
        reason: String,
    },
}

/// A validation Error the synthesis cascade resolves by dropping its holder
/// instead of refusing: write what the document holds after the W1 cascade
/// (#217, #237). Anything else keeps its refusal below.
fn cascade_validation_errors(
    projected: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::report::check::Check;
    // Errors the writer carries as stated need no drop: acknowledging the
    // exact finding keeps the cascade from re-finding it on the next pass,
    // while a later finding on the same record still blocks.
    let mut acknowledged = std::collections::BTreeSet::new();
    let acked = |finding: &&cadmpeg_ir::report::check::Finding,
                 acknowledged: &std::collections::BTreeSet<(String, String)>| {
        acknowledged.contains(&(
            finding.entity.clone().unwrap_or_default(),
            finding.message.clone(),
        ))
    };
    for _ in 0..1024 {
        let validation = cadmpeg_ir::validate::validate_neutral(projected, Vec::new())?;
        let errors: Vec<_> = validation
            .findings
            .iter()
            .filter(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
            .collect();
        let pending: Vec<_> = errors
            .iter()
            .filter(|finding| !acked(finding, &acknowledged))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        // Cascade the first resolvable holder: a record that needs a record
        // the document does not hold, a repeated identity, or a wire edge
        // that lost its shell membership drops with its loss instead of
        // refusing the write. Anything else keeps its refusal below.
        let cascaded = pending.iter().find(|finding| {
            let entity = finding.entity.clone().unwrap_or_default();
            if entity.is_empty() {
                return false;
            }
            matches!(
                finding.check,
                Check::ReferentialIntegrity
                    | Check::Identity
                    | Check::WireTopology
                    | Check::CarrierReachability
            )
        });
        let Some(finding) = cascaded else {
            let carried = pending
                .iter()
                .find(|finding| super::write_through_validation_error(finding, projected));
            if let Some(finding) = carried {
                acknowledged.insert((
                    finding.entity.clone().unwrap_or_default(),
                    finding.message.clone(),
                ));
                continue;
            }
            let first = pending[0];
            return Err(CodecError::Malformed(first.message.clone()));
        };
        let entity = finding.entity.clone().unwrap_or_default();
        match finding.check {
            Check::ReferentialIntegrity => {
                drop_model_record(projected, &entity)?;
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "record {entity} dropped; it needs a record the document does not hold"
                )));
            }
            Check::Identity => {
                drop_duplicate_model_record(projected, &entity)?;
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "record {entity} dropped; its identity repeats another record"
                )));
            }
            Check::WireTopology => {
                drop_model_record(projected, &entity)?;
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "record {entity} dropped; it lost its shell membership"
                )));
            }
            Check::CarrierReachability => {
                drop_model_record(projected, &entity)?;
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "record {entity} dropped; no topology or construction reaches it"
                )));
            }
            _ => {
                return Err(CodecError::Malformed(finding.message.clone()));
            }
        }
    }
    Err(CodecError::Malformed(
        "SLDPRT synthesis cascade exceeded its work cap".into(),
    ))
}

/// Drop one model record by id through JSON, keeping admission order.
/// Dropping a feature also drops the features it parents: regeneration and
/// tree links stay valid at every step because model admission validates
/// them, while looser references resurface as findings for the next pass.
fn drop_model_record(projected: &mut CadIr, id: &str) -> Result<(), CodecError> {
    let mut document = serde_json::to_value(&projected.model).map_err(CodecError::malformed)?;
    let mut dropped = std::collections::BTreeSet::from([id.to_owned()]);
    if let Some(model) = document.as_object() {
        if let Some(features) = model.get("features").and_then(|rows| rows.as_array()) {
            let is_feature = features
                .iter()
                .any(|row| row.get("id").and_then(|id| id.as_str()) == Some(id));
            if is_feature {
                loop {
                    let mut added = false;
                    for row in features {
                        let child = row.get("id").and_then(|id| id.as_str()).unwrap_or_default();
                        if dropped.contains(child) {
                            continue;
                        }
                        if row
                            .get("regeneration_parent")
                            .and_then(|parent| parent.as_str())
                            .is_some_and(|parent| dropped.contains(parent))
                        {
                            dropped.insert(child.to_owned());
                            added = true;
                        }
                    }
                    if !added {
                        break;
                    }
                }
            }
        }
    }
    let mut removed = false;
    if let Some(model) = document.as_object_mut() {
        for rows in model.values_mut() {
            if let Some(rows) = rows.as_array_mut() {
                let before = rows.len();
                rows.retain(|row| {
                    row.get("id")
                        .and_then(|id| id.as_str())
                        .is_none_or(|row| !dropped.contains(row))
                });
                removed = removed || rows.len() != before;
            }
        }
    }
    if !removed {
        return Err(CodecError::malformed(format_args!(
            "SLDPRT cascade cannot drop unheld record {id}"
        )));
    }
    let model = serde_json::from_value(document).map_err(CodecError::malformed)?;
    projected.model = model;
    Ok(())
}

/// Drop every later repeat of one model identity, keeping the first row.
fn drop_duplicate_model_record(projected: &mut CadIr, id: &str) -> Result<(), CodecError> {
    let mut document = serde_json::to_value(&projected.model).map_err(CodecError::malformed)?;
    let mut dropped = false;
    if let Some(model) = document.as_object_mut() {
        for rows in model.values_mut() {
            if let Some(rows) = rows.as_array_mut() {
                let mut kept = false;
                rows.retain(|row| {
                    if row.get("id").and_then(|id| id.as_str()) != Some(id) {
                        return true;
                    }
                    if kept {
                        dropped = true;
                        return false;
                    }
                    kept = true;
                    true
                });
            }
        }
    }
    if !dropped {
        return Err(CodecError::malformed(format_args!(
            "SLDPRT cascade cannot drop unrepeated record {id}"
        )));
    }
    let model = serde_json::from_value(document).map_err(CodecError::malformed)?;
    projected.model = model;
    Ok(())
}

pub(super) fn plan(
    input: EncodeInput<'_>,
    resolved: &cadmpeg_ir::codec::write::target::ResolvedWrite<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let synthesis_target = resolved.for_synthesis()?;
    let resolved = synthesis_target.resolution();
    let mut projected = input.ir.clone();
    let mut losses = Vec::new();
    // Write the canonical side of an active configuration that carries state
    // instead of refusing on its validation Error (#217, #237).
    crate::history::write::configurations::normalize_active_configuration_state(
        &mut projected.model.configurations,
        &mut losses,
    );
    // Native lane and history records require names; default blank feature
    // names to their identity as configurations do (#237).
    normalize_feature_names(&mut projected.model.features, &mut losses);
    // A native reference that names no record any retained namespace holds
    // cannot be re-emitted or regenerated from. Clear such references and
    // write the neutral content without its native link (#237).
    null_unresolvable_native_refs(&mut projected.model, &projected.native, false, &mut losses)?;
    cascade_validation_errors(&mut projected, &mut losses)?;
    // D13: the synthesis base is the active configuration. A document with no
    // active configuration still writes its base state instead of refusing at
    // the semantic support gate below. design::prepare repeats this for the
    // native lanes; the fidelity path keeps the decoder's association.
    if !projected.model.configurations.is_empty()
        && !projected
            .model
            .configurations
            .iter()
            .any(|configuration| configuration.active)
    {
        projected.model.configurations[0].active = true;
        losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
            "configuration {}: no configuration was active; the base configuration acts",
            projected.model.configurations[0].id
        )));
    }
    let retained_native = projected
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let source_trust = input
        .fidelity
        .map_or(super::SourceTrust::Authored, |fidelity| {
            super::trust_source_image(
                &fidelity.decode_digest,
                fidelity
                    .retained_record(crate::SOURCE_IMAGE_ID)
                    .and_then(|image| image.data()),
            )
        });
    let image_missing = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some_and(|image| image.data().is_none());
    if image_missing && !matches!(&cause, SynthesisCause::SourceImageUnavailable) {
        losses.push(
            SldprtLossCode::SourcePreservedImageUnavailable.note(
                "preserved SLDPRT source image is unavailable; regenerated from the document",
            ),
        );
    }
    projected.source = None;
    let slot_arena = DecodeArena::new();
    let slot_policy = DecodePolicy {
        limits: input.limits,
        ..DecodePolicy::default()
    };
    let slot_context = DecodeContext::for_encode(&slot_arena, &slot_policy);
    // The encode session ceiling bounds the work on the input document: charge
    // its records so a caller ceiling refuses instead of truncating (#217).
    slot_context.charge_entities(
        u64::try_from(input.ir.model.entity_count())
            .map_err(|_| CodecError::Malformed("SLDPRT model entity count exceeds u64".into()))?,
        "sldprt synthesis",
    )?;
    prepare_native(&slot_context, &mut projected, &mut losses)?;
    crate::pmi::write::reduce(&mut projected, &mut losses)?;
    super::previews::reduce(&mut projected, &mut losses)?;
    reduce_fields(&mut projected, &mut losses);
    presentation::reduce(&mut projected, &mut losses)?;
    // A solved cache is the document's carrier, independently of the source partition.
    for surface in &mut projected.model.surfaces {
        if let Some(cache) = surface.geometry.solved_cache() {
            losses.push(SldprtLossCode::WriterProceduralReduced.note(format!(
                "surface {} was written from its solved carrier",
                surface.id
            )));
            surface.geometry = SurfaceGeometry::Solved(cache.clone());
        }
    }
    for curve in &mut projected.model.curves {
        if let Some(cache) = curve.geometry.solved_cache() {
            losses.push(SldprtLossCode::WriterProceduralReduced.note(format!(
                "curve {} was written from its solved carrier",
                curve.id
            )));
            curve.geometry = CurveGeometry::Solved(cache.clone());
        }
    }
    geometry::resolve_placements(&mut projected, &mut losses);
    projected.model.procedural_surfaces.clear();
    projected.model.procedural_curves.clear();
    geometry::flatten_composite_curves(&mut projected, &mut losses);
    let mut supported = Vec::new();
    for body in projected.model.bodies.clone() {
        match geometry::preflight(&projected, &body) {
            Ok(()) => supported.push(body.id.clone()),
            Err(CodecError::UnsupportedContent(reason)) => {
                if body.kind == cadmpeg_ir::topology::BodyKind::Sheet
                    && geometry::reduce_sheet(&mut projected, &body, &mut losses)?
                {
                    supported.push(body.id.clone());
                } else {
                    losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                        "body {} was omitted because its complete topology cannot be encoded: {reason}", body.id
                    )));
                }
            }
            Err(error) => return Err(error),
        }
    }
    projected = super::body_subset(&projected, &supported)?;
    if supported.len() != input.ir.model.bodies.len() {
        // Design remains meaningful when solved results could not be exported.
        for feature in &mut projected.model.features {
            feature
                .evaluation
                .edit(|_, outputs| outputs.retain(|body| supported.contains(body)));
        }
        for configuration in &mut projected.model.configurations {
            if let Some(bodies) = &configuration.bodies {
                let retained = bodies
                    .iter()
                    .filter(|body| supported.contains(body))
                    .cloned()
                    .collect::<Vec<_>>();
                if retained.len() != bodies.len() {
                    losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                        "configuration {}: omitted bodies removed from membership",
                        configuration.id
                    )));
                    configuration.bodies =
                        Some(retained.try_into().map_err(CodecError::malformed)?);
                }
            }
        }
        projected
            .model
            .appearance_bindings
            .retain(|binding| match &binding.target {
                cadmpeg_ir::appearance::AppearanceTarget::Body(body) => supported.contains(body),
                cadmpeg_ir::appearance::AppearanceTarget::Face(face) => {
                    projected.model.faces.iter().any(|value| &value.id == face)
                }
                _ => true,
            });
    }
    projected.model.tessellations.retain(|mesh| {
        mesh.body
            .as_ref()
            .is_none_or(|body| supported.contains(body))
    });
    crate::history::write::configurations::ensure_default_configuration(
        &mut projected,
        &supported,
        &mut losses,
    )?;
    geometry::reduce_feature_references(input.ir, &mut projected, &mut losses)?;
    let mut prepared_native =
        design::prepare(&mut projected, &mut losses, input.fidelity.is_none())?;
    let mut native_dispositions = {
        use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
        let mut declared = NativeDispositions::for_namespace(
            "sldprt",
            crate::native::SLDPRT_ARENA_NAMES,
            NativeDisposition::Dropped,
        );
        for arena in ["feature_histories", "features", "configurations"] {
            declared.set(
                "sldprt",
                arena,
                if input.ir.scope.design {
                    NativeDisposition::Regenerated
                } else {
                    NativeDisposition::Written
                },
            );
        }
        declared.set("sldprt", "pmi_dimensions", NativeDisposition::Regenerated);
        if let Some(native) = retained_native.as_ref() {
            for record in &native.pmi_dimensions {
                if crate::pmi::annotation_id(record).is_ok_and(|id| {
                    !projected
                        .model
                        .pmi
                        .iter()
                        .any(|annotation| annotation.id == id)
                }) {
                    declared.set_record(
                        "sldprt",
                        "pmi_dimensions",
                        &record.id,
                        NativeDisposition::Dropped,
                    );
                }
            }
        }
        if !input.ir.scope.model
            && retained_native.as_ref().is_some_and(|native| {
                !native.document_metadata.is_empty()
                    && native.document_metadata.iter().all(|record| {
                        record.pmi.is_empty() && record.semantic_annotations.is_empty()
                    })
            })
        {
            declared.set("sldprt", "document_metadata", NativeDisposition::Written);
        }
        declared
    };
    if let Some(source) = retained_native.as_ref() {
        let mut native = prepared_native.take().unwrap_or_default();
        crate::resolved_features::native_lane_write::regenerate(
            input.ir,
            &projected,
            source,
            &mut native,
            &mut losses,
            &mut native_dispositions,
        )?;
        prepared_native = Some(native);
    }
    let mut bytes = Vec::new();
    let selected = crate::dialect::SldprtDialect::from_id(resolved.target_id());
    debug_assert!(
        selected.is_some(),
        "SLDPRT catalog rows match envelope grammars"
    );
    let target = match selected {
        Some(target) => target,
        None => crate::dialect::SldprtDialect::Unknown,
    };
    let source_declaration = input
        .ir
        .source
        .as_ref()
        .and_then(|source| source.dialect())
        .and_then(|dialect| dialect.declared().get("sw_version"))
        .map(String::as_str)
        .filter(|declaration| {
            crate::dialect::SldprtDialect::from_declaration(Some(declaration)) == target
        });
    let declaration = source_declaration.or(target.write_declaration());
    let output = super::write_semantic_at(
        &projected,
        &Annotations::default(),
        &[],
        super::ClassLayouts::Authored,
        declaration,
        prepared_native,
        source_trust,
        &mut bytes,
    )?;
    debug_assert_eq!(
        output.dialect,
        *resolved.target_id(),
        "SLDPRT synthesis emits its selected catalog envelope"
    );
    let mut coverage = output.coverage;
    macro_rules! changed_arena {
        ($($arena:ident),* $(,)?) => {$(
            if input.ir.model.$arena.len() > projected.model.$arena.len() {
                coverage.$arena = ArenaDisposition::Reported;
                push_changed_arena_loss(
                    &mut losses,
                    stringify!($arena),
                    &input.ir.model.$arena,
                    &projected.model.$arena,
                    SldprtLossCode::WriterEntityOmitted,
                    "while synthesizing supported bodies",
                );
            }
        )*};
    }
    changed_arena!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    macro_rules! changed_design_arena {
        ($($arena:ident),* $(,)?) => {$(
            if input.ir.model.$arena.len() > projected.model.$arena.len() {
                coverage.$arena = ArenaDisposition::Reported;
                push_changed_arena_loss(
                    &mut losses,
                    stringify!($arena),
                    &input.ir.model.$arena,
                    &projected.model.$arena,
                    SldprtLossCode::WriterDesignReduced,
                    "while synthesizing supported design",
                );
            }
        )*};
    }
    changed_design_arena!(
        sketches,
        sketch_entities,
        spatial_sketches,
        spatial_sketch_entities
    );
    push_unnamed_drop_losses(input.ir, &projected, &mut losses);
    if input.ir.model.tessellations != projected.model.tessellations {
        coverage.tessellations = ArenaDisposition::Reported;
    }
    if input.ir.model.pmi != projected.model.pmi {
        coverage.pmi = ArenaDisposition::Reported;
    }
    coverage.features = ArenaDisposition::Reported;
    coverage.parameters = ArenaDisposition::Reported;
    coverage.sketch_constraints = ArenaDisposition::Reported;
    coverage.spatial_sketch_constraints = ArenaDisposition::Dropped;
    if supported.len() != input.ir.model.bodies.len() {
        coverage.bodies = ArenaDisposition::Reported;
        coverage.regions = ArenaDisposition::Reported;
        coverage.shells = ArenaDisposition::Reported;
        coverage.faces = ArenaDisposition::Reported;
        coverage.loops = ArenaDisposition::Reported;
        coverage.coedges = ArenaDisposition::Reported;
        coverage.edges = ArenaDisposition::Reported;
        coverage.vertices = ArenaDisposition::Reported;
        coverage.points = ArenaDisposition::Reported;
        coverage.surfaces = ArenaDisposition::Reported;
        coverage.curves = ArenaDisposition::Reported;
        coverage.configurations = ArenaDisposition::Reported;
    }
    if input.ir.model.configurations != projected.model.configurations {
        coverage.configurations = ArenaDisposition::Reported;
    }
    if projected.model.faces.is_empty() {
        losses.push(
            SldprtLossCode::WriterNoExportableSolids
                .note("the document has no exportable B-rep solids"),
        );
    }
    losses.extend(super::accounting::losses(input.ir, true));
    losses.extend(super::accounting::arena_losses(input.ir, &coverage));
    let synthesis_cause = match &cause {
        SynthesisCause::PatchDeclined { reason } => {
            Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined {
                reason: reason.clone(),
            })
        }
        SynthesisCause::SourceUnrepresented { classes } => Some(
            cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented {
                classes: classes.clone(),
            },
        ),
        SynthesisCause::Fresh | SynthesisCause::SourceImageUnavailable => None,
    };
    let consumption = match cause {
        SynthesisCause::PatchDeclined { reason } => Consumption::Degraded { reason },
        SynthesisCause::SourceImageUnavailable => {
            losses.push(
                SldprtLossCode::SourcePreservedImageUnavailable
                    .note("preserved SLDPRT source image is unavailable; regenerated from IR"),
            );
            Consumption::NotConsumed
        }
        SynthesisCause::Fresh | SynthesisCause::SourceUnrepresented { .. } => {
            Consumption::NotConsumed
        }
    };
    let notes = vec!["source container regenerated from IR".into()];
    if let Some(message) = resolved.displacement_message() {
        losses.push(SldprtLossCode::SourceDialectDisplaced.note(message));
    }
    Ok(ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: projected.census(),
        },
        write_path: WritePath::Synthesized { consumption },
        coverage: ArenaCoverage::Declared(coverage),
        native_dispositions,
        synthesis_cause,
        losses,
        notes,
    })
}

fn prepare_native(
    slot_context: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop())?;
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let mut native = native.unwrap_or_default();
    native.feature_input_lanes.clear();
    parameters::move_unreferenced(&ctx, ir, &mut native, losses)?;
    for record in &mut native.pmi_dimensions {
        if record.item_count.get() > 1 {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "native PMI {}: {} additional items have no typed document payload and were omitted", record.id, record.item_count.get() - 1
            )));
            record.item_count = std::num::NonZeroU32::MIN;
        }
    }
    // Typed feature records retain fields with no neutral equivalent. They do
    // not restore deleted neutral features, and raw input lanes never supply
    // content. Every field with a neutral form is synchronized before emission.
    let referenced = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<std::collections::HashSet<_>>();
    for history in &mut native.feature_histories {
        history.features.retain(|record| {
            !ir.scope.design
                || referenced.contains(record.id.as_str())
                || crate::history::classify::is_custom_property(record)
        });
    }
    if !ir.scope.model {
        let pmi = crate::pmi::apply_to_parameters(
            slot_context,
            &mut Vec::new(),
            &[],
            &native.pmi_dimensions,
        )?;
        ir.model.pmi = pmi.annotations;
        losses.extend(pmi.losses);
        let mut attributes = crate::history::project::custom_property_attributes(
            slot_context,
            &native.feature_histories,
        )?;
        for metadata in &native.document_metadata {
            for attribute in &metadata.attributes {
                if let Some(existing) = attributes
                    .iter()
                    .find(|existing| existing.id == attribute.id)
                {
                    if existing != attribute {
                        return Err(CodecError::malformed("native document metadata repeats an attribute identity with conflicting content"));
                    }
                } else {
                    attributes.push(attribute.clone());
                }
            }
            let omitted = metadata.pmi.len() + metadata.semantic_annotations.len();
            if omitted != 0 {
                losses.push(SldprtLossCode::WriterNativeRecordsOmitted.note(format!(
                    "{omitted} typed native PMI/semantic annotation record(s) have no synthesized database encoding")));
            }
        }
        attributes.extend(pmi.attributes);
        if !attributes.is_empty() || !ir.model.pmi.is_empty() {
            ir.scope.model = true;
            ir.model.attributes = attributes;
        }
    }
    crate::resolved_features::write_prepare::synthesize_feature_classes(&mut native)?;
    ir.native = cadmpeg_ir::native::Native::default();
    native.store(&ctx, ir.native.namespace_mut("sldprt"))?;
    // The wipe orphaned references into the removed namespaces; write the
    // neutral content without those links (#237).
    null_unresolvable_native_refs(&mut ir.model, &ir.native, true, losses)?;
    Ok(())
}

/// Default blank feature names to their identity, as configurations do.
///
/// Native lane and history records require names. A feature without one
/// cannot be authored; the identity stands in and the loss names it (#237).
fn normalize_feature_names(
    features: &mut [cadmpeg_ir::features::Feature],
    losses: &mut Vec<LossNote>,
) {
    for feature in features {
        if feature
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
        {
            feature.name = Some(feature.id.as_str().into());
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "feature {} uses its identity as the required native name",
                feature.id
            )));
        }
    }
}

/// Native record identities across every namespace of `native`, read through
/// its serialized form so the codec never names another namespace.
fn native_record_identities(
    native: &cadmpeg_ir::native::Native,
) -> std::collections::HashSet<String> {
    fn collect_ids(value: &serde_json::Value, owned: &mut std::collections::HashSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(id) = fields.get("id").and_then(serde_json::Value::as_str) {
                    owned.insert(id.to_owned());
                }
                for child in fields.values() {
                    collect_ids(child, owned);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect_ids(item, owned);
                }
            }
            _ => {}
        }
    }
    let mut owned = std::collections::HashSet::new();
    if let Ok(document) = serde_json::to_value(native) {
        collect_ids(&document, &mut owned);
    }
    owned
}

/// Clear native references the write cannot resolve.
///
/// A neutral record whose `native_ref` names a record the write does not
/// retain can neither be re-emitted nor regenerated from its link; the
/// neutral content is written without it (#237). References carried inside
/// definitions keep their refusal below. Returns without touching the model
/// when every reference resolves.
///
/// Before validation every unresolvable reference is cleared: the validator
/// would refuse each of them, so the cleared set is exactly its finding set.
/// After the foreign-namespace wipe only references outside the retained
/// namespaces are cleared (`foreign_only`); references into retained records
/// dropped by lane clearing keep their existing silent paths, so retained
/// source-layout references change nothing.
fn null_unresolvable_native_refs(
    model: &mut cadmpeg_ir::document::Model,
    native: &cadmpeg_ir::native::Native,
    foreign_only: bool,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    fn walk(
        value: &mut serde_json::Value,
        owned: &std::collections::HashSet<String>,
        present: &std::collections::HashSet<String>,
        foreign_only: bool,
        cleared: &mut Vec<(String, String)>,
    ) {
        match value {
            serde_json::Value::Object(fields) => {
                if let (Some(id), Some(serde_json::Value::String(target))) = (
                    fields
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    fields.get("native_ref"),
                ) {
                    let namespace = target.split(':').next().unwrap_or(target.as_str());
                    if !owned.contains(target)
                        && (!foreign_only || !present.iter().any(|present| present == namespace))
                    {
                        cleared.push((id, target.clone()));
                        fields.remove("native_ref");
                    }
                }
                for child in fields.values_mut() {
                    walk(child, owned, present, foreign_only, cleared);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, owned, present, foreign_only, cleared);
                }
            }
            _ => {}
        }
    }
    let owned = native_record_identities(native);
    let present: std::collections::HashSet<String> =
        if let Ok(serde_json::Value::Object(spaces)) = serde_json::to_value(native) {
            spaces.keys().cloned().collect()
        } else {
            std::collections::HashSet::new()
        };
    let mut document =
        serde_json::to_value(&*model).map_err(|error| CodecError::Malformed(error.to_string()))?;
    let mut cleared = Vec::new();
    // `semantic_annotations.native_ref` is mandatory: clearing it breaks
    // deserialization, so its dangling links keep their refusal below.
    if let serde_json::Value::Object(spaces) = &mut document {
        for (arena, rows) in spaces.iter_mut() {
            if arena != "semantic_annotations" {
                walk(rows, &owned, &present, foreign_only, &mut cleared);
            }
        }
    } else {
        walk(&mut document, &owned, &present, foreign_only, &mut cleared);
    }
    if cleared.is_empty() {
        return Ok(());
    }
    *model = serde_json::from_value(document)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    cleared.sort();
    for (id, target) in &cleared {
        losses.push(SldprtLossCode::WriterNativeReferenceCleared.note(format!(
            "record {id} referenced native record {target} that the write does not retain; the dangling native reference was cleared"
        )));
    }
    Ok(())
}

/// Record ids of one model arena through its serialized form.
fn arena_ids(records: &impl serde::Serialize) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(serde_json::Value::Array(rows)) = serde_json::to_value(records) {
        for row in rows {
            if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                ids.push(id.to_owned());
            }
        }
    }
    ids
}

/// Charge the counted-arena omission loss with the dropped record ids named.
///
/// A drop the target cannot represent is reported as a loss that names what
/// it affects (#209 §5.2). Ids an earlier loss already names stay out.
fn push_changed_arena_loss(
    losses: &mut Vec<LossNote>,
    arena: &str,
    before: &impl serde::Serialize,
    after: &impl serde::Serialize,
    code: SldprtLossCode,
    reason: &str,
) {
    let before_ids = arena_ids(before);
    let after_ids: std::collections::HashSet<_> = arena_ids(after).into_iter().collect();
    let dropped: Vec<&String> = before_ids
        .iter()
        .filter(|id| !after_ids.contains(id.as_str()))
        .collect();
    if dropped.is_empty() {
        return;
    }
    let mut unnamed: Vec<String> = dropped
        .iter()
        .filter(|id| !losses.iter().any(|loss| loss.message.contains(id.as_str())))
        .map(|id| (*id).clone())
        .collect();
    unnamed.sort();
    let mut message = format!(
        "model.{arena}: {} of {} record(s) omitted {reason}",
        dropped.len(),
        before_ids.len(),
    );
    if !unnamed.is_empty() {
        message.push_str(": ");
        message.push_str(&unnamed.join(", "));
    }
    losses.push(code.note(message));
}

/// Name dropped records of arenas without a dedicated omission loss.
///
/// Attributes, presentation layers, procedural carriers and cleared
/// association arenas drop records with no other loss naming them. One loss
/// per arena names every unnamed drop so no drop goes unnamed (#209 §5.2).
fn push_unnamed_drop_losses(input: &CadIr, projected: &CadIr, losses: &mut Vec<LossNote>) {
    let before = serde_json::to_value(&input.model).unwrap_or_default();
    let after = serde_json::to_value(&projected.model).unwrap_or_default();
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return;
    };
    for (arena, rows) in before {
        let Some(rows) = rows.as_array() else {
            continue;
        };
        let after_ids: std::collections::HashSet<&str> = after
            .get(arena)
            .and_then(|rows| rows.as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        let mut unnamed: Vec<String> = rows
            .iter()
            .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
            .filter(|id| !after_ids.contains(id))
            .filter(|id| !losses.iter().any(|loss| loss.message.contains(id)))
            .map(str::to_owned)
            .collect();
        if unnamed.is_empty() {
            continue;
        }
        unnamed.sort();
        let code = if matches!(
            arena.as_str(),
            "features"
                | "parameters"
                | "configurations"
                | "spreadsheets"
                | "sketch_constraints"
                | "spatial_sketch_constraints"
        ) {
            SldprtLossCode::WriterDesignReduced
        } else if matches!(
            arena.as_str(),
            "attributes" | "presentation_layers" | "presentation_documents" | "view_presentations"
        ) {
            SldprtLossCode::WriterFieldsOmitted
        } else {
            SldprtLossCode::WriterEntityOmitted
        };
        losses.push(code.note(format!(
            "model.{arena}: {} record(s) dropped as unrepresentable in SLDPRT: {}",
            unnamed.len(),
            unnamed.join(", ")
        )));
    }
}

fn reduce_fields(ir: &mut CadIr, losses: &mut Vec<LossNote>) {
    let mut count = 0;
    for parameter in &mut ir.model.parameters {
        parameter.native_ref = None;
        let original = parameter.properties.len();
        parameter
            .properties
            .retain(|key, _| crate::history::write::xml::valid_xml_name(key.as_str()));
        count += original - parameter.properties.len();
    }
    for feature in &mut ir.model.features {
        let original = feature.source_properties.len();
        feature
            .source_properties
            .retain(|key, _| crate::history::write::xml::valid_xml_name(key.as_str()));
        count += original - feature.source_properties.len();
        if feature.suppressed.is_none() {
            feature.suppressed = Some(false);
            count += 1;
        }
        if feature
            .source_tag
            .as_deref()
            .is_some_and(|tag| !crate::history::write::xml::valid_xml_name(tag))
        {
            feature.source_tag = None;
            count += 1;
        }
    }
    for body in &mut ir.model.bodies {
        if body.color.is_none() {
            count += usize::from(body.name.take().is_some());
        }
    }
    for face in &mut ir.model.faces {
        count += usize::from(face.name.take().is_some());
    }
    for edge in &mut ir.model.edges {
        count += usize::from(edge.param_range().is_some());
        edge.set_param_range(None);
    }
    for appearance in &mut ir.model.appearances {
        count += usize::from(appearance.library_id.take().is_some());
        count += appearance.textures.len();
        appearance.textures.clear();
        count += usize::from(appearance.asset_guid.take().is_some());
        count += usize::from(appearance.visual_guid.take().is_some());
        count += usize::from(appearance.physical_token.take().is_some());
        count += usize::from(appearance.category.take().is_some());
        count += appearance.properties.len();
        appearance.properties.clear();
    }
    if count != 0 {
        losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
            "{count} name, range, and appearance field(s) have no SLDPRT encoding"
        )));
    }
    for sketch in &mut ir.model.sketches {
        sketch.native_ref = None;
    }
    for sketch in &mut ir.model.spatial_sketches {
        sketch.native_ref = None;
    }
    for entity in &mut ir.model.sketch_entities {
        entity.native_ref = None;
        entity.geometry_ref = None;
        entity.endpoint_refs.clear();
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        entity.native_ref = None;
        entity.geometry_ref = None;
        entity.endpoint_refs.clear();
    }
    for constraint in &mut ir.model.sketch_constraints {
        constraint.native_ref = None;
    }
    for coedge in &mut ir.model.coedges {
        coedge.pcurves.clear();
    }
    macro_rules! dropped_arenas { ($($arena:ident),* $(,)?) => { $(ir.model.$arena.clear();)* }; }
    dropped_arenas!(
        subds,
        feature_result_topologies,
        spreadsheets,
        product_definitions,
        occurrences,
        assembly_joints,
        drawings,
        semantic_annotations,
        presentation_documents,
        view_presentations,
        presentation_layers,
        pcurves
    );
}

fn reduce_design(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    repair: bool,
) -> Result<(), CodecError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let prepare = |candidate: &mut CadIr, losses: &mut Vec<LossNote>| {
        let mut native = native.clone();
        let orphaned = crate::history::write::parameters::orphaned_scalar_parameters(
            candidate,
            native.as_ref(),
        )
        .map_err(|error| {
            CodecError::malformed(format_args!("SLDPRT orphaned scalar scan failed: {error}"))
        })?;
        design::synthesize_neutral_features_repairing(
            candidate,
            &mut native,
            &orphaned,
            losses,
            repair,
        )?;
        Ok(())
    };
    match prepare(&mut *ir, &mut *losses) {
        Ok(_) => return Ok(()),
        Err(CodecError::UnsupportedContent(_)) => {}
        Err(error) => return Err(error),
    }
    let original = ir.model.features.clone();
    for feature in &mut ir.model.features {
        reduce_feature_to_solved(feature)?;
    }
    for (index, feature) in original.into_iter().enumerate() {
        let solved = ir.model.features[index].clone();
        ir.model.features[index] = feature;
        match prepare(&mut *ir, &mut *losses) {
            Ok(_) => {}
            Err(CodecError::UnsupportedContent(reason)) => {
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "feature {} retains its solved result instead of its operation: {reason}",
                    ir.model.features[index].id
                )));
                ir.model.features[index] = solved;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn reduce_feature_to_solved(feature: &mut cadmpeg_ir::features::Feature) -> Result<(), CodecError> {
    // The degraded neutral definition replaces its earlier native operation.
    feature.native_ref = None;
    feature.source_tag = None;
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        ));
    feature.dependencies = cadmpeg_ir::features::DistinctMembers::default();
    feature.source_content = feature
        .source_content
        .iter()
        .filter(|content| {
            !matches!(
                content,
                cadmpeg_ir::features::FeatureSourceContent::Feature(_)
            )
        })
        .cloned()
        .collect::<Vec<_>>()
        .try_into()
        .map_err(CodecError::malformed)?;
    Ok(())
}

#[cfg(test)]
mod tests;
