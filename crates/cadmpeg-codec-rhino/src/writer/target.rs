// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeMode, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, transitional_from_codec, ArenaCoverage, ArenaDisposition,
    ArenaDispositions, CapacityField, Consumption, EncodeError, EncodeInput, ExportBody, WritePath,
};

mod coverage;

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

/// Synthesize the selected catalog archive version.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, EncodeError> {
    let mut input_records = 0_u64;
    for count in input.ir.census().values() {
        let count = cadmpeg_core::decode::u64_from_index(*count);
        if count > input.limits.max_entities - input_records {
            return Err(cadmpeg_core::decode::ResourceLimit {
                scope: cadmpeg_core::decode::LimitScope::Session,
                dimension: cadmpeg_core::decode::ResourceDimension::Entities,
                reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
                limit: input.limits.max_entities,
                used: input.limits.max_entities,
                additional: 1,
                operation: "inspect Rhino input records",
            }
            .into());
        }
        input_records += count;
    }
    let synthesis = target.for_synthesis()?;
    let target = synthesis.resolution();
    let index = synthesis.index();
    debug_assert!(
        index < RhinoArchiveVersion::ALL.len(),
        "Rhino catalog rows match archive grammars"
    );
    let version = match RhinoArchiveVersion::ALL.get(index) {
        Some(version) => *version,
        None => RhinoArchiveVersion::ALL[0],
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        mode: DecodeMode::Salvage,
        limits: input.limits,
    };
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(&ctx, input.ir)?;
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx)?;
    let (bytes, outcome) = super::write(&ctx, &recovered.document, version, &session)?;
    let mut losses = outcome.losses;
    if let Ok(document) = serde_json::to_value(input.ir) {
        if let Some(model) = document.get("model").and_then(serde_json::Value::as_object) {
            let mut duplicates = std::collections::BTreeSet::new();
            for rows in model.values() {
                let mut seen = std::collections::BTreeSet::new();
                if let Some(rows) = rows.as_array() {
                    for row in rows {
                        if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                            if !seen.insert(id.to_owned()) {
                                duplicates.insert(id.to_owned());
                            }
                        }
                    }
                }
            }
            for id in duplicates {
                losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "duplicate neutral record {id} is omitted"
                )));
            }
        }
    }
    super::tables::document::uncarried_attribute_losses(input.ir, &mut losses);
    for binding in &input.ir.model.appearance_bindings {
        if !binding.channels.is_empty() {
            losses.push(RhinoLossCode::WriterAppearanceChannelsOmitted.note(format!(
                    "appearance binding {} change-version channels are omitted: {}",
                    binding.id,
                    binding
                        .channels
                        .keys()
                        .map(cadmpeg_core::text::NonBlankString::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
        }
        let target = binding.target.typed_record_target(&input.ir.model);
        if let cadmpeg_ir::appearance::AppearanceTarget::Face(face) =
            target.as_ref().unwrap_or(&binding.target)
        {
            let face_color = input
                .ir
                .model
                .faces
                .iter()
                .find(|row| row.id == *face)
                .and_then(|row| row.color)
                .or_else(|| {
                    super::tables::materials::bound_appearance(input.ir, face.as_str())
                        .and_then(|appearance| appearance.base_color)
                });
            let binding_color = input
                .ir
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.base_color);
            let detail = if outcome.face_colors.contains(face.as_str())
                && face_color.is_some()
                && face_color == binding_color
            {
                "is reduced to a face color; its appearance asset link is omitted"
            } else {
                "is omitted because its color is not represented by the target face"
            };
            losses.push(
                RhinoLossCode::WriterFaceAppearanceReduced
                    .note(format!("appearance binding {} {detail}", binding.id)),
            );
        }
        if matches!(
            target.as_ref().unwrap_or(&binding.target),
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
                | cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
        ) {
            losses.push(RhinoLossCode::WriterSubobjectAppearanceReduced.note(format!(
                "appearance binding {} cannot retain its edge or vertex target in 3DM object attributes",
                binding.id
            )));
        }
    }
    for layer in &input.ir.model.presentation_layers {
        if layer.visible.is_none() {
            losses.push(RhinoLossCode::WriterLayerVisibilityDefaulted.note(format!(
                "layer {} unspecified visibility is written as visible",
                layer.id
            )));
        }
        if let Some(description) = &layer.description {
            if super::tables::normalized_layer_description(description)
                != Some(description.as_str())
            {
                losses.push(
                    RhinoLossCode::WriterLayerDescriptionNormalized.note(format!(
                        "layer {} description is normalized by the 3DM layer format",
                        layer.id
                    )),
                );
            }
        }
        let omitted = layer
            .items
            .iter()
            .filter_map(|item| {
                use cadmpeg_ir::presentation::PresentationItem;
                match item {
                    PresentationItem::Face { face } => Some(face.as_str()),
                    PresentationItem::Edge { edge } => Some(edge.as_str()),
                    PresentationItem::Vertex { vertex } => Some(vertex.as_str()),
                    PresentationItem::Product { product } => Some(product.as_str()),
                    PresentationItem::Pmi { annotation } => Some(annotation.as_str()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if !omitted.is_empty() {
            losses.push(RhinoLossCode::WriterLayerMembershipOmitted.note(format!(
                "layer {} cannot assign individual non-object members: {}",
                layer.id,
                omitted.join(", ")
            )));
        }
    }
    let model = &input.ir.model;
    for (name, ids, code) in [
        (
            "features",
            model
                .features
                .iter()
                .filter(|row| !outcome.history_features.contains(row.id.as_str()))
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "configurations",
            model
                .configurations
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "parameters",
            model
                .parameters
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketches",
            model
                .sketches
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketch_entities",
            model
                .sketch_entities
                .iter()
                .map(|row| row.id().as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketch_constraints",
            model
                .sketch_constraints
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            RhinoLossCode::WriterDesignOmitted,
        ),
    ] {
        if !ids.is_empty() {
            losses.push(code.note(format!(
                "model arena {name}: {} record(s) reduced or omitted: {}",
                ids.len(),
                ids.join(", ")
            )));
        }
    }
    let colors =
        model
            .bodies
            .iter()
            .filter_map(|body| body.color)
            .chain(model.points.iter().filter_map(|point| {
                input
                    .ir
                    .effective_display_color(&cadmpeg_ir::appearance::AppearanceTarget::Point(
                        point.id.clone(),
                    ))
            }))
            .chain(model.curves.iter().filter_map(|curve| {
                input
                    .ir
                    .effective_display_color(&cadmpeg_ir::appearance::AppearanceTarget::Curve(
                        curve.id.clone(),
                    ))
            }))
            .chain(model.surfaces.iter().filter_map(|surface| {
                input.ir.effective_display_color(
                    &cadmpeg_ir::appearance::AppearanceTarget::Surface(surface.id.clone()),
                )
            }))
            .chain(model.tessellations.iter().filter_map(|mesh| {
                input.ir.effective_display_color(
                    &cadmpeg_ir::appearance::AppearanceTarget::Tessellation(mesh.id.to_string()),
                )
            }))
            .chain(
                model
                    .appearances
                    .iter()
                    .filter_map(|value| value.base_color),
            )
            .chain(model.faces.iter().filter_map(|face| {
                outcome
                    .face_colors
                    .contains(face.id.as_str())
                    .then_some(face.color)
                    .flatten()
            }));
    if colors.into_iter().any(super::presentation::color_quantized) {
        losses.push(RhinoLossCode::WriterColorPrecisionReduced.note(
            "object display colors use eight-bit RGBA channels; non-byte colors were rounded",
        ));
    }
    let face_metadata = model
        .faces
        .iter()
        .filter(|face| {
            face.name.is_some()
                || (face.color.is_some() && !outcome.face_colors.contains(face.id.as_str()))
        })
        .map(|face| face.id.as_str())
        .collect::<Vec<_>>();
    if !face_metadata.is_empty() {
        losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
            "face-level name or color overrides were omitted for {}; body display remains",
            face_metadata.join(", ")
        )));
    }
    if outcome.geometry_objects == 0 {
        losses.push(
            RhinoLossCode::NoExportableSolids.note("the document holds no exportable geometry"),
        );
    }
    if !recovered.sampled_agreements.is_empty() {
        losses.push(RhinoLossCode::EdgePcurveAgreementSampled.note(format!(
            "{} recovered edge(s) have coedge curves compared at five parameters; agreement between samples is not established",
            recovered.sampled_agreements.len()
        )));
    }
    if !recovered.sampled_fits.is_empty() {
        losses.push(RhinoLossCode::EdgePcurveSampledToBspline.note(format!(
            "{} recovered edge(s) were fitted as B-splines with sampled residual at most {} model length units",
            recovered.sampled_fits.len(), recovered.residual_limit
        )));
    }
    let owned_curves = input
        .ir
        .model
        .edges
        .iter()
        .filter_map(cadmpeg_ir::topology::Edge::curve)
        .collect::<std::collections::BTreeSet<_>>();
    let curves = input
        .ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            !owned_curves.contains(&curve.id)
                && !outcome.written_curves.contains(curve.id.as_str())
                && super::is_support(curve.source_object.as_ref())
        })
        .collect::<Vec<_>>();
    if !curves.is_empty() {
        let mut withheld = Vec::new();
        withheld.extend(curves.iter().map(|curve| curve.id.as_str()));
        for curve in &curves {
            if let Some(construction) = curve.geometry.procedural_construction() {
                withheld.push(construction.as_str());
            }
        }
        losses.push(RhinoLossCode::WriterSupportGeometryWithheld.note(format!(
            "{} support curve record(s) were withheld; Rhino cannot encode their standalone form: {}",
            curves.len(),
            withheld.join(", ")
        )));
    }
    if let Some(message) = target.displacement_message() {
        losses.push(RhinoLossCode::SourceDialectDisplaced.note(message));
    }
    let (verify_ctx, verify_root) =
        DecodeContext::for_encode_session_root(&arena, &session, bytes.as_bytes())?;
    let coverage = ArenaCoverage::Declared(coverage::verified(
        &verify_ctx,
        input.ir,
        verify_root.window(),
        outcome.subds,
        !outcome.history_features.is_empty(),
    ));
    session.finish()?;
    Ok(ExportBody {
        native_dispositions: super::native_dispositions(input.ir, version).map_err(|error| {
            match error {
                super::WriteError::Omit(message) => {
                    transitional_from_codec(CodecError::UnsupportedContent(message))
                }
                super::WriteError::Fail(error) => transitional_from_codec(error),
                super::WriteError::Encode(error) => error,
            }
        })?,
        synthesis_cause: None,
        bytes: bytes.into_vec(),
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::Synthesized {
            consumption: Consumption::NotConsumed,
        },
        coverage,
        losses,
        notes: vec![format!("3DM archive version {}", version.value())],
    })
}

/// Reduced storage rows (#232): neutral JSON paths the 3DM write stores in a
/// reduced form, with the storage that bounds them. Comparison on these paths
/// applies the row's storage rule before it compares.
///
/// Mesh normals are binary32 on every target; standalone mesh vertices are
/// binary32 below archive 60 and binary64 at and above. Object and appearance
/// colors use eight-bit RGBA channels. A quantized write charges the row's
/// local precision code naming the paths.
pub(crate) const REDUCED_FIELDS: &[(&str, ReducedStorage)] = &[
    (
        "model.tessellations[].vertex_normals[]",
        ReducedStorage::F32,
    ),
    (
        "model.tessellations[].vertices[]",
        ReducedStorage::F32BelowArchive60,
    ),
    ("model.bodies[].color", ReducedStorage::U8Channel),
    ("model.appearances[].base_color", ReducedStorage::U8Channel),
    ("model.faces[].color", ReducedStorage::U8Channel),
    ("tolerances.angular", ReducedStorage::AngularTolerance),
];

pub(crate) const PUBLIC_REDUCED_FIELDS: &[cadmpeg_ir::codec::write::ReducedField] = &[
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.tessellations[].vertex_normals[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::F32,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.appearances[].base_color",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.faces[].color",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "tolerances.angular",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "rhino/writer.angular-tolerance-reduced",
        },
    },
];

/// The reduced storage of one [`REDUCED_FIELDS`] row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReducedStorage {
    /// Eight-bit RGBA channels on every target.
    U8Channel,
    /// IEEE-754 binary32 on every target.
    F32,
    /// IEEE-754 binary32 below archive 60; binary64 at and above.
    F32BelowArchive60,
    /// Angular tolerances above pi are stored as pi.
    AngularTolerance,
}

/// Format capacity rows (#217 §5.0): the stored fields whose width bounds the
/// write, with the limit, its source, and the helper that enforces it.
///
/// The writer emits only archive words at and above 50, whose chunk framing
/// stores an eight-byte declared length (`docs/formats/rhino_3dm.md` §4).
/// `archive_body_len` enforces the declared field width.
pub(crate) const CAPACITY_FIELDS: &[CapacityField] = &[CapacityField {
    field: "chunk.declared_length",
    limit: 9_223_372_036_854_775_807,
    source: "docs/formats/rhino_3dm.md §4: archive version >= 50 stores an i64 declared_length",
    helper: "archive_body_len",
}];

const _: () = {
    assert!(REDUCED_FIELDS.len() == 6);
    assert!(CAPACITY_FIELDS.len() == 1);
};

/// Fallback claims are promoted only after full-record verification under a
/// shared identity map.
const COVERAGE: ArenaDispositions = {
    use ArenaDisposition::{Dropped, Omitted, Reported};
    ArenaDispositions {
        bodies: Reported,
        regions: Reported,
        shells: Reported,
        faces: Reported,
        loops: Reported,
        coedges: Reported,
        edges: Reported,
        vertices: Reported,
        points: Reported,
        surfaces: Reported,
        curves: Reported,
        subds: Dropped,
        pcurves: Reported,
        procedural_surfaces: Reported,
        procedural_curves: Reported,
        assets: Reported,
        features: Dropped,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Dropped,
        parameters: Dropped,
        sketches: Dropped,
        sketch_entities: Dropped,
        sketch_constraints: Dropped,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: Reported,
        occurrences: Reported,
        assembly_joints: Omitted,
        drawings: Omitted,
        semantic_annotations: Reported,
        presentation_documents: Reported,
        view_presentations: Reported,
        tessellations: Reported,
        appearances: Reported,
        appearance_bindings: Reported,
        attributes: Reported,
        pmi: Omitted,
        presentation_layers: Reported,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
    }
};
