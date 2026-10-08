// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, WritePath,
};

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

/// Why this writer cannot reproduce a source archive version outside
/// [`RhinoArchiveVersion::TARGETS`].
///
/// Archives 1, 2, 3, 4, 5 and 90 decode without a writer, unknown words decode
/// as residual, and 3DM has no retained-image path that could write any of
/// them back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the source archive version is one this writer cannot synthesize, and 3DM has no byte-replay \
     path that could preserve it";

/// Synthesize the resolved archive version.
///
/// Every catalog resolution names its `RhinoArchiveVersion::ALL` row by
/// position; the preserved resolution has no row and is refused here because
/// 3DM has no replay path.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let Some(index) = target.index() else {
        return Err(target.unavailable(OFF_CATALOG_SOURCE_REASON));
    };
    let version = RhinoArchiveVersion::ALL[index];
    let mut bytes = Vec::new();
    super::write(input.ir, version, &mut bytes)?;
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
    let mut losses = Vec::new();
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
    Ok(ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::Synthesized {
            consumption: Consumption::NotConsumed,
        },
        coverage: ArenaCoverage::Declared(COVERAGE),
        losses,
        notes: vec![format!("3DM archive version {}", version.value())],
    })
}

/// What `prepare_write` does with each model arena.
///
/// Topology is written as Brep and point objects or refused as orphaned;
/// unowned pcurves are refused. The refused arenas are the ones
/// `prepare_write` names in its unsupported-arena refusal.
const COVERAGE: ArenaDispositions = {
    use ArenaDisposition::{Omitted, Reported, Written};
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
        curves: Written,
        subds: Reported,
        pcurves: Written,
        procedural_surfaces: Reported,
        procedural_curves: Reported,
        assets: Omitted,
        features: Reported,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Reported,
        parameters: Reported,
        sketches: Reported,
        sketch_entities: Reported,
        sketch_constraints: Reported,
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
        tessellations: Written,
        appearances: Reported,
        appearance_bindings: Reported,
        attributes: Reported,
        pmi: Omitted,
        presentation_layers: Omitted,
    }
};
