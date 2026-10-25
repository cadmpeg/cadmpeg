// SPDX-License-Identifier: Apache-2.0
//! Neutral-first synthesis after a retained layout cannot express an edit.

use cadmpeg_core::decode::DecodeContext;
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

pub(super) fn plan(
    ctx: &DecodeContext<'_>,
    input: EncodeInput<'_>,
    resolved: &cadmpeg_ir::codec::write::target::ResolvedWrite<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let synthesis_target = resolved.for_synthesis()?;
    let resolved = synthesis_target.resolution();
    let mut projected = input.ir.clone();
    let mut losses = Vec::new();
    let validation = cadmpeg_ir::validate::validate_neutral(&projected, Vec::new())?;
    if let Some(finding) = validation
        .findings
        .iter()
        .find(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
    {
        return Err(CodecError::Malformed(finding.message.clone()));
    }
    let retained_native = projected
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
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
    prepare_native(ctx, &mut projected, &mut losses)?;
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
    projected.model.procedural_surfaces.clear();
    projected.model.procedural_curves.clear();
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
    geometry::reduce_feature_references(input.ir, &mut projected, &mut losses)?;
    let mut prepared_native = design::prepare(&mut projected, &mut losses)?;
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
        ctx,
        &projected,
        &Annotations::default(),
        &[],
        super::ClassLayouts::Authored,
        declaration,
        prepared_native,
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
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "model.{}: {} of {} record(s) omitted while synthesizing supported bodies",
                    stringify!($arena), input.ir.model.$arena.len() - projected.model.$arena.len(), input.ir.model.$arena.len()
                )));
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
                losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                    "model.{}: {} of {} record(s) omitted while synthesizing supported design",
                    stringify!($arena), input.ir.model.$arena.len() - projected.model.$arena.len(), input.ir.model.$arena.len()
                )));
            }
        )*};
    }
    changed_design_arena!(
        sketches,
        sketch_entities,
        spatial_sketches,
        spatial_sketch_entities
    );
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
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let mut native = native.unwrap_or_default();
    native.feature_input_lanes.clear();
    parameters::move_unreferenced(ctx, ir, &mut native, losses)?;
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
        let pmi = crate::pmi::apply_to_parameters(&mut Vec::new(), &[], &native.pmi_dimensions)?;
        ir.model.pmi = pmi.annotations;
        losses.extend(pmi.losses);
        let mut attributes =
            crate::history::project::custom_property_attributes(&native.feature_histories)?;
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
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    Ok(())
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

fn reduce_design(ir: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let prepare = |candidate: &CadIr| {
        let mut native = native.clone();
        crate::history::write::features::synthesize_neutral_features(
            &candidate.model,
            &candidate.model.parameters,
            &candidate.model.bodies,
            &mut native,
        )
    };
    match prepare(ir) {
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
        match prepare(ir) {
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
