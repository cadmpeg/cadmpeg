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
            if super::write_through_validation_error(projected, finding) {
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
                .find(|finding| super::write_through_validation_error(projected, finding));
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
                for dropped in drop_model_record(projected, &entity)? {
                    losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                        "record {dropped} dropped; it needs a record the document does not hold"
                    )));
                }
            }
            Check::Identity => {
                drop_duplicate_model_record(projected, &entity)?;
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "record {entity} dropped; its identity repeats another record"
                )));
            }
            Check::WireTopology => {
                for dropped in drop_model_record(projected, &entity)? {
                    losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                        "record {dropped} dropped; it lost its shell membership"
                    )));
                }
            }
            Check::CarrierReachability => {
                for dropped in drop_model_record(projected, &entity)? {
                    losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                        "record {dropped} dropped; no topology or construction reaches it"
                    )));
                }
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
fn drop_model_record(
    projected: &mut CadIr,
    id: &str,
) -> Result<std::collections::BTreeSet<String>, CodecError> {
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
    Ok(dropped)
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
) -> Result<ExportBody, super::write_error::WriteError> {
    let mut projected = input.ir.clone();
    let mut losses = Vec::new();
    repair_multiple_active_configurations(&mut projected.model.configurations, &mut losses);
    // Write the canonical side of an active configuration that carries state
    // instead of refusing on its validation Error (#217, #237).
    crate::history::write::configurations::normalize_active_configuration_state(
        &mut projected.model.configurations,
        &mut losses,
    );
    repair_duplicate_configuration_indices(&mut projected.model.configurations, &mut losses);
    repair_configuration_names(&mut projected.model.configurations, &mut losses);
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
    prepare_native(&mut projected, &mut losses)?;
    crate::pmi::write::reduce(&mut projected, &mut losses)?;
    super::previews::reduce(&mut projected, &mut losses)?;
    reduce_fields(
        &slot_context,
        &mut projected,
        &mut losses,
        retained_native.as_ref(),
    )?;
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
    geometry::convert_polylines(&mut projected, &mut losses);
    geometry::resolve_placements(&mut projected, &mut losses);
    projected.model.procedural_surfaces.clear();
    projected.model.procedural_curves.clear();
    geometry::flatten_composite_curves(&mut projected, &mut losses)?;
    let mut supported = Vec::new();
    for body in projected.model.bodies.clone() {
        if body.kind == cadmpeg_ir::topology::BodyKind::Wire {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "wire body {} omitted: its native encoding is unavailable",
                body.id
            )));
            continue;
        }
        match geometry::preflight(&projected, &body) {
            Ok(()) => supported.push(body.id.clone()),
            Err(super::write_error::WriteError::Decline(reason)) => {
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
    let mut before_subset = projected;
    before_subset
        .model
        .bodies
        .sort_by_key(|body| !supported.contains(&body.id));
    projected = super::body_subset(&before_subset, &supported)?;
    let kept_curves = projected
        .model
        .curves
        .iter()
        .map(|curve| &curve.id)
        .collect::<std::collections::HashSet<_>>();
    for curve in &before_subset.model.curves {
        if !kept_curves.contains(&curve.id) {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "curve {} omitted because no selected partition carries it",
                curve.id
            )));
        }
    }
    macro_rules! report_omitted_owners {
        ($records:ident, $owners:ident, $owner:ident) => {{
            let records = projected
                .model
                .$records
                .iter()
                .map(|record| &record.id)
                .collect::<std::collections::HashSet<_>>();
            let owners = projected
                .model
                .$owners
                .iter()
                .map(|record| &record.id)
                .collect::<std::collections::HashSet<_>>();
            for record in &before_subset.model.$records {
                if !records.contains(&record.id) && !owners.contains(&record.$owner) {
                    losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                        "record {} dropped because its owner {} was omitted",
                        record.id, record.$owner
                    )));
                }
            }
        }};
    }
    report_omitted_owners!(regions, bodies, body);
    report_omitted_owners!(shells, regions, region);
    report_omitted_owners!(faces, shells, shell);
    report_omitted_owners!(loops, faces, face);
    report_omitted_owners!(coedges, loops, owner_loop);
    let wire_bodies = before_subset
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Wire)
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let wire_regions = before_subset
        .model
        .regions
        .iter()
        .filter(|region| wire_bodies.contains(&region.body))
        .map(|region| &region.id)
        .collect::<std::collections::HashSet<_>>();
    let mut wire_edges = std::collections::HashSet::new();
    let mut wire_vertices = std::collections::HashSet::new();
    for shell in before_subset
        .model
        .shells
        .iter()
        .filter(|shell| wire_regions.contains(&shell.region))
    {
        wire_edges.extend(shell.wire_edges().iter());
        wire_vertices.extend(shell.free_vertices().iter());
    }
    let kept_edges = projected
        .model
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<std::collections::HashSet<_>>();
    for edge in &before_subset.model.edges {
        if wire_edges.contains(&edge.id) && !kept_edges.contains(&edge.id) {
            losses.push(
                SldprtLossCode::WriterEntityOmitted
                    .note(format!("wire edge {} omitted with its wire body", edge.id)),
            );
            wire_vertices.insert(&edge.start);
            wire_vertices.insert(&edge.end);
        }
    }
    let kept_vertices = projected
        .model
        .vertices
        .iter()
        .map(|vertex| &vertex.id)
        .collect::<std::collections::HashSet<_>>();
    let mut wire_points = std::collections::HashSet::new();
    for vertex in &before_subset.model.vertices {
        if wire_vertices.contains(&vertex.id) && !kept_vertices.contains(&vertex.id) {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "wire vertex {} omitted with its wire body",
                vertex.id
            )));
            wire_points.insert(&vertex.point);
        }
    }
    let kept_points = projected
        .model
        .points
        .iter()
        .map(|point| &point.id)
        .collect::<std::collections::HashSet<_>>();
    for point in &before_subset.model.points {
        if wire_points.contains(&point.id) && !kept_points.contains(&point.id) {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "wire point {} omitted with its wire body",
                point.id
            )));
        }
    }
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
        if !input.ir.scope.model
            && prepared_native.as_ref().is_some_and(|native| {
                native
                    .document_metadata
                    .iter()
                    .any(|record| !record.attributes.is_empty())
            })
        {
            declared.set(
                "sldprt",
                "document_metadata",
                NativeDisposition::Regenerated,
            );
        }
        declared.set("sldprt", "pmi_dimensions", NativeDisposition::Regenerated);
        if let Some(native) = retained_native.as_ref().filter(|_| input.ir.scope.model) {
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
    losses.extend(output.normalization_losses);
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
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), super::write_error::WriteError> {
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
        for metadata in &mut native.document_metadata {
            let mut retained = Vec::new();
            for attribute in std::mem::take(&mut metadata.attributes) {
                if super::MetadataRecord::project(&attribute, 0.001, |_| Ok(()))?.is_some() {
                    retained.push(attribute);
                } else {
                    losses.push(SldprtLossCode::WriterNativeRecordsOmitted.note(format!(
                        "native document metadata {}: attribute {} has no encoding",
                        metadata.id, attribute.id
                    )));
                }
            }
            metadata.attributes = retained;
            if !metadata.pmi.is_empty() || !metadata.semantic_annotations.is_empty() {
                losses.push(SldprtLossCode::WriterNativeRecordsOmitted.note(format!(
                    "native document metadata {}: PMI and semantic annotations have no design-only encoding", metadata.id
                )));
                metadata.pmi.clear();
                metadata.semantic_annotations.clear();
            }
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

fn repair_multiple_active_configurations(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    losses: &mut Vec<LossNote>,
) {
    let mut order = (0..configurations.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| configurations[*index].ordinal);
    let mut active_seen = false;
    for position in order {
        let configuration = &mut configurations[position];
        if configuration.active {
            if active_seen {
                configuration.active = false;
                losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                    "configuration {}: an additional active state was cleared",
                    configuration.id
                )));
            }
            active_seen = true;
        }
    }
}

fn repair_configuration_names(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    losses: &mut Vec<LossNote>,
) {
    let mut reserved = configurations
        .iter()
        .filter_map(|configuration| configuration.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::new();
    let mut order = (0..configurations.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| configurations[*index].ordinal);
    for position in order {
        let configuration = &mut configurations[position];
        if let Some(name) = configuration
            .name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
        {
            if seen.insert(name.clone()) {
                continue;
            }
        }
        let mut replacement = configuration.id.as_str().to_owned();
        while reserved.contains(&replacement) {
            replacement.push('_');
        }
        reserved.insert(replacement.clone());
        seen.insert(replacement.clone());
        configuration.name = Some(replacement);
        losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
            "configuration {}: missing or duplicate name was replaced by a unique identity",
            configuration.id
        )));
    }
}

fn repair_duplicate_configuration_indices(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    losses: &mut Vec<LossNote>,
) {
    let mut order = (0..configurations.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| configurations[*index].ordinal);
    let mut seen = std::collections::HashSet::new();
    for position in order {
        let configuration = &mut configurations[position];
        let Some(index) = configuration.source_index else {
            continue;
        };
        if !seen.insert(index) {
            configuration.source_index = None;
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "configuration {}: duplicate source index {index} was cleared",
                configuration.id
            )));
        }
    }
}

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

fn reduce_fields(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    native: Option<&SldprtNative>,
) -> Result<(), CodecError> {
    let mut bounded = std::collections::HashSet::new();
    for edge in &ir.model.edges {
        if super::bounded_curve::derived_sphere_seam(ir, edge)
            || super::bounded_curve::from_edge(ctx, ir, edge, 0.001)?.is_some()
        {
            bounded.insert(edge.id.clone());
        }
    }
    for parameter in &mut ir.model.parameters {
        if !crate::history::write::parameters::is_pmi_parameter(parameter, native) {
            parameter.native_ref = None;
        }
        parameter.properties.retain(|key, _| {
            let supported = crate::history::write::xml::valid_xml_name(key.as_str());
            if !supported {
                losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                    "parameter {}: property {} omitted",
                    parameter.id, key
                )));
            }
            supported
        });
    }
    for feature in &mut ir.model.features {
        feature.source_properties.retain(|key, _| {
            let supported = crate::history::write::xml::valid_xml_name(key.as_str());
            if !supported {
                losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                    "feature {}: source property {} omitted",
                    feature.id, key
                )));
            }
            supported
        });
        if feature.suppressed.is_none() {
            feature.suppressed = Some(false);
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "feature {}: unresolved suppression written as active",
                feature.id
            )));
        }
        if feature
            .source_tag
            .as_deref()
            .is_some_and(|tag| !crate::history::write::xml::valid_xml_name(tag))
        {
            feature.source_tag = None;
            losses.push(
                SldprtLossCode::WriterFieldsOmitted
                    .note(format!("feature {}: source tag omitted", feature.id)),
            );
        }
    }
    for body in &mut ir.model.bodies {
        if body.name.take().is_some() {
            losses.push(
                SldprtLossCode::WriterFieldsOmitted.note(format!("body {}: name omitted", body.id)),
            );
        }
    }
    for face in &mut ir.model.faces {
        if face.name.take().is_some() {
            losses.push(
                SldprtLossCode::WriterFieldsOmitted.note(format!("face {}: name omitted", face.id)),
            );
        }
    }
    for edge in &mut ir.model.edges {
        if edge.param_range().is_some() && !bounded.contains(&edge.id) {
            losses.push(
                SldprtLossCode::WriterFieldsOmitted
                    .note(format!("edge {}: carrier.param_range omitted", edge.id)),
            );
            edge.set_param_range(None);
        }
    }
    for appearance in &mut ir.model.appearances {
        let mut fields = Vec::new();
        if appearance.library_id.take().is_some() {
            fields.push("library_id");
        }
        if !appearance.textures.is_empty() {
            fields.push("textures");
            appearance.textures.clear();
        }
        if appearance.asset_guid.take().is_some() {
            fields.push("asset_guid");
        }
        if appearance.visual_guid.take().is_some() {
            fields.push("visual_guid");
        }
        if appearance.physical_token.take().is_some() {
            fields.push("physical_token");
        }
        if appearance.category.take().is_some() {
            fields.push("category");
        }
        if !appearance.properties.is_empty() {
            fields.push("properties");
            appearance.properties.clear();
        }
        if !fields.is_empty() {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "appearance {}: {} omitted",
                appearance.id,
                fields.join(", ")
            )));
        }
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
    Ok(())
}

fn reduce_design(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    repair: bool,
) -> Result<(), super::write_error::WriteError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let prepare = |candidate: &mut CadIr,
                   losses: &mut Vec<LossNote>|
     -> Result<(), super::write_error::WriteError> {
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
        Err(super::write_error::WriteError::Decline(_)) => {}
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
            Err(super::write_error::WriteError::Decline(reason)) => {
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
