// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodeMode, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, CapacityField,
    Consumption, EncodeInput, ExportBody, WritePath,
};

mod coverage;

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

/// Synthesize the selected catalog archive version.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
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
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(input.ir)?;
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        mode: DecodeMode::Salvage,
        limits: input.limits,
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx)?;
    let (bytes, outcome) = super::write(&ctx, &recovered.document, version, &session)?;
    let mut losses = outcome.losses;
    super::tables::document::uncarried_attribute_losses(input.ir, &mut losses);
    let vertex_quantization = !version.stores_mesh_vertices_as_f64()
        && input
            .ir
            .model
            .tessellations
            .iter()
            .flat_map(cadmpeg_ir::tessellation::Tessellation::vertices)
            .any(|point| {
                cadmpeg_core::convert::f32_from_f64(point.x).map(f64::from) != Some(point.x)
                    || cadmpeg_core::convert::f32_from_f64(point.y).map(f64::from) != Some(point.y)
                    || cadmpeg_core::convert::f32_from_f64(point.z).map(f64::from) != Some(point.z)
            });
    let normal_quantization = input
        .ir
        .model
        .tessellations
        .iter()
        .flat_map(cadmpeg_ir::tessellation::Tessellation::vertex_normals)
        .any(|normal| {
            cadmpeg_core::convert::f32_from_f64(normal.x).map(f64::from) != Some(normal.x)
                || cadmpeg_core::convert::f32_from_f64(normal.y).map(f64::from) != Some(normal.y)
                || cadmpeg_core::convert::f32_from_f64(normal.z).map(f64::from) != Some(normal.z)
        });
    let model = &input.ir.model;
    for (name, ids, code) in [
        (
            "features",
            model
                .features
                .iter()
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
            );
    if colors.into_iter().any(super::presentation::color_quantized) {
        losses.push(RhinoLossCode::WriterColorPrecisionReduced.note(
            "object display colors use eight-bit RGBA channels; non-byte colors were rounded",
        ));
    }
    let face_metadata = model
        .faces
        .iter()
        .filter(|face| face.name.is_some() || face.color.is_some())
        .map(|face| face.id.as_str())
        .collect::<Vec<_>>();
    if !face_metadata.is_empty() {
        losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
            "face-level name or color overrides were omitted for {}; body display remains",
            face_metadata.join(", ")
        )));
    }
    for mesh in &model.tessellations {
        let metadata_dropped = mesh.body.is_some()
            || !mesh.feature_edges().is_empty()
            || !mesh.per_corner_normals().is_empty()
            || !mesh.triangle_groups().is_empty()
            || !mesh.texture_assignments().is_empty()
            || mesh.channels().iter().any(|channel| {
                !matches!(
                    channel.kind(),
                    super::CHANNEL_UV
                        | super::CHANNEL_COLOR
                        | super::CHANNEL_SURFACE_PARAMETERS
                        | super::CHANNEL_CURVATURE
                )
            });
        if metadata_dropped {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "mesh {} auxiliary metadata omitted; triangle geometry is retained",
                mesh.id
            )));
        }
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
    let owned_points = input
        .ir
        .model
        .vertices
        .iter()
        .map(|vertex| &vertex.point)
        .collect::<std::collections::BTreeSet<_>>();
    let owned_curves = input
        .ir
        .model
        .edges
        .iter()
        .filter_map(cadmpeg_ir::topology::Edge::curve)
        .collect::<std::collections::BTreeSet<_>>();
    let points = input
        .ir
        .model
        .points
        .iter()
        .filter(|point| {
            !owned_points.contains(&point.id) && super::is_support(point.source_object.as_ref())
        })
        .map(|point| point.id.as_str())
        .collect::<Vec<_>>();
    let curves = input
        .ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            !owned_curves.contains(&curve.id) && super::is_support(curve.source_object.as_ref())
        })
        .collect::<Vec<_>>();
    if !points.is_empty() || !curves.is_empty() {
        let mut withheld: Vec<&str> = points.clone();
        withheld.extend(curves.iter().map(|curve| curve.id.as_str()));
        for curve in &curves {
            if let Some(construction) = curve.geometry.procedural_construction() {
                withheld.push(construction.as_str());
            }
        }
        losses.push(RhinoLossCode::WriterSupportGeometryWithheld.note(format!(
            "{} support point record(s) and {} support curve record(s) were withheld; Rhino cannot encode dependent standalone geometry: {}",
            points.len(),
            curves.len(),
            withheld.join(", ")
        )));
    }
    if let Some(message) = target.displacement_message() {
        losses.push(RhinoLossCode::SourceDialectDisplaced.note(message));
    }
    if vertex_quantization {
        losses.push(RhinoLossCode::MeshVertexPrecisionReduced.note(
            "archive version 50 stores standalone mesh vertices as f32; \
             rhino:archive-60, rhino:archive-70, and rhino:archive-80 store them as f64 \
             and would not charge this",
        ));
    }
    if normal_quantization {
        losses.push(RhinoLossCode::MeshNormalPrecisionReduced.note(
            "3DM mesh normals are stored as f32; every rhino write target charges this, \
             so no other target avoids it",
        ));
    }
    // Re-admit the written bytes for the verification decode so the
    // re-decode carries the digest of the bytes it reads.
    let (verify_ctx, verify_root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy)?;
    let coverage = ArenaCoverage::Declared(coverage::verified(
        &verify_ctx,
        input.ir,
        verify_root.window(),
        outcome.subds,
    ));
    verify_ctx.finish_session()?;
    ctx.finish_session()?;
    session.finish()?;
    Ok(ExportBody {
        native_dispositions: super::native_dispositions(input.ir, version).map_err(|error| {
            match error {
                // The plan boundary converts control flow that no record boundary
                // caught. #239 switches this site to `EncodeError`.
                super::WriteError::Omit(message) => CodecError::UnsupportedContent(message),
                super::WriteError::Fail(error) => error,
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
}

/// Format capacity rows (#217 §5.0): the stored fields whose width bounds the
/// write, with the limit, its source, and the helper that enforces it.
///
/// The writer emits only archive words at and above 50, whose chunk framing
/// stores an eight-byte declared length (`docs/formats/rhino_3dm.md` §4).
/// `archive_body_len` is the helper that refuses a body the field cannot
/// store; it constructs `TargetCapacity` once #239 switches the encode path
/// to `EncodeError`.
pub(crate) const CAPACITY_FIELDS: &[CapacityField] = &[CapacityField {
    field: "chunk.declared_length",
    limit: 9_223_372_036_854_775_807,
    source: "docs/formats/rhino_3dm.md §4: archive version >= 50 stores an i64 declared_length",
    helper: "archive_body_len",
}];

const _: () = {
    assert!(REDUCED_FIELDS.len() == 4);
    assert!(CAPACITY_FIELDS.len() == 1);
};

/// Conservative fallback when synthesized-output verification does not prove
/// complete preservation of an arena. The verifier promotes populated arenas
/// only after comparing their complete records under one global identity map.
///
/// Procedural definitions are reduced to solved carriers with a per-record
/// loss; features, configurations, parameters, sketches, sketch entities
/// and constraints, spatial sketches and entities, spreadsheets,
/// assembly joints, drawings, pmi, physical materials, mass properties,
/// rigid bodies, colliders, articulations, physics scenes, solver settings
/// and declared measures are `Dropped` or `Omitted` with a per-arena loss. Topology is written as Brep and point objects. Unsupported bodies and
/// free entities are omitted with Error losses; unowned pcurves are also
/// reported. `points` and `curves` are `Reported` because
/// `WriterSupportGeometryWithheld` accounts for withheld support records.
/// Every other carrying arena holds records a `writer/tables/*` module emits,
/// so the base claim is `Reported`: the verifier promotes an arena to
/// `Written` only after the re-decode agrees with the held records.
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
