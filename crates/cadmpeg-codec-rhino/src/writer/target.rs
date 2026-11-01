// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodeMode, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, WritePath,
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
    for (name, count, code) in [
        (
            "procedural_surfaces",
            model.procedural_surfaces.len(),
            RhinoLossCode::WriterProceduralReduced,
        ),
        (
            "procedural_curves",
            model
                .procedural_curves
                .iter()
                .filter(|value| {
                    !matches!(
                        value.definition(),
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(_)
                    )
                })
                .count(),
            RhinoLossCode::WriterProceduralReduced,
        ),
        (
            "features",
            model.features.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "configurations",
            model.configurations.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "parameters",
            model.parameters.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketches",
            model.sketches.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketch_entities",
            model.sketch_entities.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "sketch_constraints",
            model.sketch_constraints.len(),
            RhinoLossCode::WriterDesignOmitted,
        ),
        (
            "appearances",
            model.appearances.len(),
            RhinoLossCode::WriterMaterialOmitted,
        ),
        (
            "appearance_bindings",
            model.appearance_bindings.len(),
            RhinoLossCode::WriterMaterialOmitted,
        ),
        (
            "attributes",
            model.attributes.len(),
            RhinoLossCode::WriterAttributesOmitted,
        ),
    ] {
        if count != 0 {
            losses.push(code.note(format!(
                "model arena {name}: {count} record(s) reduced or omitted"
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
        .count();
    if face_metadata != 0 {
        losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
            "{face_metadata} face-level name or color override(s) were omitted; body display remains")));
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
        .count();
    let curves = input
        .ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            !owned_curves.contains(&curve.id) && super::is_support(curve.source_object.as_ref())
        })
        .count();
    if points != 0 || curves != 0 {
        losses.push(RhinoLossCode::WriterSupportGeometryWithheld.note(format!(
            "{points} support point record(s) and {curves} support curve record(s) were withheld; Rhino cannot encode dependent standalone geometry"
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
        native_dispositions: super::native_dispositions(input.ir)?,
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

/// Conservative fallback when synthesized-output verification does not prove
/// complete preservation of an arena. The verifier promotes populated arenas
/// only after comparing their complete records under one global identity map.
///
/// Procedural surfaces and curves, features, configurations, parameters,
/// sketches, sketch entities and constraints, appearances, appearance bindings
/// and attributes are `Dropped` with a per-arena loss.
/// Topology is written as Brep and point objects. Unsupported bodies and free
/// entities are omitted with Error losses; unowned pcurves are also reported.
/// `points` and `curves` are `Reported` because
/// `WriterSupportGeometryWithheld` accounts for withheld support records.
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
        procedural_surfaces: Dropped,
        procedural_curves: Reported,
        assets: Omitted,
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
        product_definitions: Omitted,
        occurrences: Omitted,
        assembly_joints: Omitted,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Reported,
        appearances: Reported,
        appearance_bindings: Reported,
        attributes: Dropped,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
    }
};
