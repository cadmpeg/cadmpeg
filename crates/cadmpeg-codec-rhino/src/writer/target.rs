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
/// `subds`, procedural surfaces and curves, features, configurations, parameters,
/// sketches, sketch entities and constraints, appearances, appearance bindings
/// and attributes are `Dropped` because `prepare_write` refuses non-empty arenas.
/// Topology is written as Brep and point objects or refused as orphaned;
/// unowned pcurves are refused.
const COVERAGE: ArenaDispositions = {
    use ArenaDisposition::{Dropped, Omitted, Written};
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
        subds: Dropped,
        pcurves: Written,
        procedural_surfaces: Dropped,
        procedural_curves: Dropped,
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
        tessellations: Written,
        appearances: Dropped,
        appearance_bindings: Dropped,
        attributes: Dropped,
        pmi: Omitted,
        presentation_layers: Omitted,
    }
};
