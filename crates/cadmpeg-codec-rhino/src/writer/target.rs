// SPDX-License-Identifier: Apache-2.0
//! Rhino target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    output_empty, target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions,
    Consumption, EncodeInput, ExportBody, WritePath,
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

/// The regions that state no complete shell list and hold no partial face.
/// Their stated faces can close, and 3DM reads a closed Brep as a solid, so
/// the writer writes each of their faces as its own Brep.
fn unjoined_regions(ir: &cadmpeg_ir::CadIr) -> std::collections::BTreeSet<&str> {
    let geometry = &ir.geometry;
    let lists = geometry.child_lists();
    geometry
        .regions
        .iter()
        .filter(|region| {
            lists.complete_shells(region).is_none()
                && !region.stated_shells().any(|shell| {
                    lists
                        .stated_shell_faces(shell)
                        .iter()
                        .any(|face| face.loops.is_partial())
                })
        })
        .map(|region| region.id.as_str())
        .collect()
}

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
    // A cage the archive cannot hold is left out with the body that holds
    // only it (§5.6).
    let mut losses = Vec::new();
    let mut cageless = None;
    let unwritten = super::subd::unwritten_cages(input.ir, version);
    if let Some((reason, cages)) = &unwritten {
        let mut ir = input.ir.clone();
        let bodies =
            cadmpeg_ir::codec::write::remove_cage_bodies_of(&mut ir, |id| cages.contains(&id));
        losses.push(RhinoLossCode::WriterRecordOmitted.note(format!(
            "{} subdivision cage(s) are not written: {reason}: {}{}",
            cages.len(),
            cages
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            cadmpeg_ir::codec::write::cage_bodies_clause(&bodies)
        )));
        cageless = (!bodies.is_empty()).then_some(ir);
    }
    let unwritten = unwritten.map(|(_, cages)| cages).unwrap_or_default();
    let source = cageless.as_ref().unwrap_or(input.ir);
    let unjoined = unjoined_regions(source);
    let (complete, partial_losses) = match cadmpeg_ir::codec::write::without_partial_topology(
        source,
        "Rhino",
        "its faces are written as an open Brep",
    )? {
        Some((complete, losses)) => (Some(complete), losses),
        None => (None, Vec::new()),
    };
    let ir = complete.as_ref().unwrap_or(source);
    let mut bytes = Vec::new();
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(ir)?;
    let written = super::write(&recovered.document, version, &unjoined, &mut bytes)?;
    let withheld_meshes = super::withheld_body_meshes(ir);
    let written_meshes = || {
        ir.model
            .tessellations
            .iter()
            .filter(|mesh| !withheld_meshes.contains(mesh.id.as_str()))
    };
    let vertex_quantization = !version.stores_mesh_vertices_as_f64()
        && written_meshes()
            .flat_map(cadmpeg_ir::tessellation::Tessellation::vertices)
            .any(|point| {
                cadmpeg_core::convert::f32_from_f64(point.x).map(f64::from) != Some(point.x)
                    || cadmpeg_core::convert::f32_from_f64(point.y).map(f64::from) != Some(point.y)
                    || cadmpeg_core::convert::f32_from_f64(point.z).map(f64::from) != Some(point.z)
            });
    let normal_quantization = written_meshes()
        .flat_map(cadmpeg_ir::tessellation::Tessellation::vertex_normals)
        .any(|normal| {
            cadmpeg_core::convert::f32_from_f64(normal.x).map(f64::from) != Some(normal.x)
                || cadmpeg_core::convert::f32_from_f64(normal.y).map(f64::from) != Some(normal.y)
                || cadmpeg_core::convert::f32_from_f64(normal.z).map(f64::from) != Some(normal.z)
        });
    losses.extend(partial_losses);
    losses.extend(written.losses);
    if !written.wrote_object {
        losses.push(output_empty(
            "the written 3DM archive holds no object: the document states no geometry or mesh \
             this writer encodes",
        ));
    }
    if !withheld_meshes.is_empty() {
        losses.push(RhinoLossCode::WriterBodyMeshWithheld.note(format!(
            "{} tessellation record(s) that display a body written as a Brep were withheld; the \
             writer writes no Brep render mesh: {}",
            withheld_meshes.len(),
            withheld_meshes.iter().copied().collect::<Vec<_>>().join(", ")
        )));
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
        .geometry
        .vertices
        .iter()
        .map(|vertex| &vertex.point)
        .collect::<std::collections::BTreeSet<_>>();
    let owned_curves = input
        .ir
        .geometry
        .edges
        .iter()
        .filter_map(cadmpeg_ir::topology::Edge::curve)
        .collect::<std::collections::BTreeSet<_>>();
    let points = input
        .ir
        .geometry
        .points
        .iter()
        .filter(|point| {
            !owned_points.contains(&point.id) && super::is_support(point.source_object.as_ref())
        })
        .count();
    let curves = input
        .ir
        .geometry
        .curves
        .iter()
        .filter(|curve| {
            !owned_curves.contains(&curve.id) && super::is_support(curve.source_object.as_ref())
        })
        .count();
    // A withheld support record is charged above; every other record is
    // written without its source-object association.
    let associations = input
        .ir
        .geometry
        .points
        .iter()
        .filter(|point| {
            owned_points.contains(&point.id) || !super::is_support(point.source_object.as_ref())
        })
        .filter_map(|point| point.source_object.as_ref())
        .chain(
            input
                .ir
                .geometry
                .curves
                .iter()
                .filter(|curve| {
                    owned_curves.contains(&curve.id)
                        || !super::is_support(curve.source_object.as_ref())
                })
                .filter_map(|curve| curve.source_object.as_ref()),
        )
        .chain(
            input
                .ir
                .geometry
                .surfaces
                .iter()
                .filter_map(|surface| surface.source_object.as_ref()),
        )
        .chain(written_meshes().filter_map(|mesh| mesh.source_object.as_ref()))
        .chain(
            input
                .ir
                .geometry
                .subds
                .iter()
                .filter(|subd| !unwritten.contains(&&subd.id))
                .filter_map(|subd| subd.source_object.as_ref()),
        )
        .count();
    if associations != 0 {
        losses.push(RhinoLossCode::WriterSourceAssociationOmitted.note(format!(
            "{associations} source-object association(s) were not written; a 3DM object holds \
             no source-format object identity"
        )));
    }
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
            counts: source.census(),
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
/// Arenas 3DM is not written with are `Omitted`: the archive is written
/// without them, and the export wrapper charges each non-empty one. Procedural
/// surfaces and curves are definitions beside the solved carriers the archive
/// holds. Topology and pcurves are written as Brep and point objects;
/// `WriterRecordOmitted` names each record the writer leaves out.
/// `points` and `curves` are `Reported` because
/// `WriterSupportGeometryWithheld` accounts for withheld support records,
/// `tessellations` because `WriterBodyMeshWithheld` accounts for the display
/// meshes of bodies written as Breps, and `subds` because
/// `WriterRecordOmitted` names each cage the archive version cannot hold.
const COVERAGE: ArenaDispositions = {
    use ArenaDisposition::{Ignored, Omitted, Reported, Written};
    ArenaDispositions {
        bodies: Written,
        regions: Written,
        shells: Written,
        faces: Written,
        loops: Written,
        coedges: Written,
        edges: Written,
        vertices: Written,
        points: Reported,
        surfaces: Written,
        curves: Reported,
        subds: Reported,
        pcurves: Written,
        procedural_surfaces: Omitted,
        procedural_curves: Omitted,
        assets: Omitted,
        features: Omitted,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Omitted,
        parameters: Omitted,
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
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
        point_clouds: Omitted,
        history_bodies: Ignored,
        appearances: Omitted,
        appearance_bindings: Omitted,
        attributes: Omitted,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
        design: cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted),
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
    }
};
