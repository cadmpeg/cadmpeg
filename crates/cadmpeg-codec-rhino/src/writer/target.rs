// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan, ResolvedWrite};
use cadmpeg_ir::{ExportReport, FidelityResolution, WritePath};

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

/// Why this writer cannot reproduce a source archive version outside
/// [`RhinoArchiveVersion::TARGETS`].
///
/// Archives 1, 2, 3, 4, 5 and 90 decode without a writer, unknown words decode
/// as admitted-unverified, and 3DM has no retained-image path that could write
/// any of them back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the source archive version is one this writer cannot synthesize, and 3DM has no byte-replay \
     path that could preserve it";

/// Resolve the request against the source and synthesize the selected archive.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite,
) -> Result<ExportPlan, CodecError> {
    let Some(entry) = resolved.catalog_entry() else {
        return Err(resolved.unavailable(OFF_CATALOG_SOURCE_REASON));
    };
    let version = RhinoArchiveVersion::from_catalog_entry(entry);
    let mut bytes = Vec::new();
    super::write(input.ir, version, &mut bytes)?;
    let vertex_quantization = !version.stores_mesh_vertices_as_f64()
        && input
            .ir
            .model
            .tessellations
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .any(|point| {
                f64::from(point.x as f32) != point.x
                    || f64::from(point.y as f32) != point.y
                    || f64::from(point.z as f32) != point.z
            });
    let normal_quantization = input
        .ir
        .model
        .tessellations
        .iter()
        .flat_map(|mesh| &mesh.normals)
        .any(|normal| {
            f64::from(normal.x as f32) != normal.x
                || f64::from(normal.y as f32) != normal.y
                || f64::from(normal.z as f32) != normal.z
        });
    let mut losses = Vec::new();
    let target = version.descriptor().id;
    if let Some(message) = resolved.displacement_message() {
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
    let report = ExportReport::native(
        target,
        cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        },
        WritePath::Synthesized,
        losses,
        vec![format!("3DM archive version {}", version.value())],
    );
    Ok(ExportPlan::buffered(report, bytes))
}
