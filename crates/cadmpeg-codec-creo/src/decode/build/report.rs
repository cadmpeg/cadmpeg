// SPDX-License-Identifier: Apache-2.0
//! Decode-report assembly from coverage counters and container census.

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::sketches::SketchGeometry;

use crate::container::{self, role, ContainerScan};
use crate::loss::CreoLossCode;

use super::super::analytic::is_axis_aligned;
use super::super::surfaces::BrepTransferDiagnostics;
use super::report_coverage::push_coverage_drop_losses;
use super::report_losses::{
    push_brep_transfer_note, push_carrier_transfer_notes, push_legacy_value_losses,
    push_structural_layer_notes,
};
use cadmpeg_ir::codec::DecodeBody;

pub(in super::super) fn has_transferred_geometry(ir: &CadIr) -> bool {
    let model = &ir.model;
    !model.points.is_empty()
        || !model.vertices.is_empty()
        || !model.edges.is_empty()
        || !model.coedges.is_empty()
        || !model.loops.is_empty()
        || !model.faces.is_empty()
        || !model.shells.is_empty()
        || !model.regions.is_empty()
        || !model.bodies.is_empty()
        || model
            .surfaces
            .iter()
            .any(|surface| !matches!(&surface.geometry, SurfaceGeometry::Unknown { .. }))
        || model
            .curves
            .iter()
            .any(|curve| !matches!(&curve.geometry, CurveGeometry::Unknown { .. }))
        || !model.subds.is_empty()
        || !model.pcurves.is_empty()
        || model.procedural_surfaces.iter().any(|surface| {
            !matches!(
                &surface.definition,
                ProceduralSurfaceDefinition::Unknown { .. }
            )
        })
        || model
            .procedural_curves
            .iter()
            .any(|curve| !matches!(&curve.definition, ProceduralCurveDefinition::Unknown { .. }))
        || model
            .sketch_entities
            .iter()
            .any(|entity| !matches!(&entity.geometry, SketchGeometry::Native { .. }))
        || !model.tessellations.is_empty()
}

/// Build the decode body from the entry point's one dialect classification.
pub(in super::super) fn build_report(
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
    ir: &CadIr,
    coverage: cadmpeg_ir::Coverage,
    brep_diagnostics: &BrepTransferDiagnostics,
    container_only: bool,
) -> DecodeBody {
    let geom_sections = scan
        .framing
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();
    let mut placed_plane_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| {
            frame.origin.is_some()
                && frame.u_axis.is_some()
                && frame.normal.is_some_and(|normal| !is_axis_aligned(normal))
        })
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    placed_plane_ids.extend(scan.planes.outlines.iter().map(|plane| plane.surface_id));
    placed_plane_ids.extend(
        scan.planes
            .positional_frames
            .iter()
            .map(|plane| plane.surface_id),
    );
    let placed_plane_count = placed_plane_ids.len();
    let mut losses = Vec::new();

    // The admission charge, first: it describes how the whole document was
    // read, not what any one record cost. Identity itself is authored once, in
    // `ir.source`; the report body carries only the charge.
    losses.extend(classification.loss());

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .framing
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .framing
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(CreoLossCode::ContainerCensus.note(format!(
        "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
         census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
         prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
         Outline-backed planes, guarded non-axis support frames, complete ND first-instance \
         plane, cylinder, cone, torus, and interpolation-spline prototypes, unbound straight positional \
         surface-of-extrusion planes, \
         topology-bound planes with analytic boundary carriers, `fc 05` cylinders with a \
         resolved axis-normal cap plane, four-entry two-cap and blind \
         circular-sweep cylinders, \
         four-entry simple-hole cylinders with complete cap outlines, radius-anchored \
         class-911 counterbore and bore patches, and compact simple-hole cylinders with \
         complete positional carriers, complementary split-outline cylinders \
         bound to an axis-normal plane, complete positional cylinder bodies, \
         complete support-apex and planar-envelope positional cones, and complete \
         local-system positional tori transfer as carriers; \
         other parameter bodies remain structural records.",
        scan.framing.sections.len(),
        scan.framing.layout.token(),
        scan.surfaces.rows.len(),
        scan.curves.prototypes.len(),
        scan.curves.topology_rows.len(),
        scan.topology.loops.len(),
    )));

    push_legacy_value_losses(&mut losses, &coverage);

    push_brep_transfer_note(&mut losses, brep_diagnostics, geom_sections);

    push_carrier_transfer_notes(
        &mut losses,
        scan,
        &coverage,
        container_only,
        placed_plane_count,
    );
    push_structural_layer_notes(&mut losses, scan);
    push_coverage_drop_losses(&mut losses, &coverage);

    DecodeBody {
        geometry_transferred: has_transferred_geometry(ir),
        coverage,
        losses,
        notes: container::notes(scan),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}
