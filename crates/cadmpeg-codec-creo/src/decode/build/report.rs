// SPDX-License-Identifier: Apache-2.0
//! Decode-report assembly from coverage counters and container census.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::sketches::SketchGeometry;

use crate::container::{self, role, ContainerScan};
use crate::loss::CreoLossCode;

use super::super::analytic::is_axis_aligned;
use super::report_coverage::push_coverage_drop_losses;
use super::report_losses::{
    push_carrier_transfer_notes, push_legacy_value_losses, push_structural_layer_notes,
};
use cadmpeg_ir::report::DecodeReport;

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

/// Build diagnostics for data that cannot be represented in the emitted IR.
pub(in super::super) fn build_report(
    scan: &ContainerScan,
    ir: &CadIr,
    coverage: BTreeMap<String, usize>,
    container_only: bool,
) -> DecodeReport {
    let summary = container::summarize(scan);
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

    if container_only {
        losses.push(
            CreoLossCode::ContainerOnlyDecode
                .note("Container-only decode requested; entity transfer was skipped."),
        );
    }

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

    // The core prototype-vs-instance limitation.
    losses.push(CreoLossCode::BrepTransferIncomplete.note(format!(
        "General model B-rep transfer remains incomplete. Native face components transfer \
         when every boundary edge has solved vertex orbits, face orientation is unique, and \
         every loop is complete; a multi-loop face additionally requires strict parameter-space \
         containment or a complete common-center, distinct-radius circular-loop proof on a plane. Selected \
         cylinders transfer when an exact `fc 05` record and placed cap outline binds a row, \
         a four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
         square cap outline establishes the complete axis placement and radius, or a compact \
         class-911 table owns a complete positional cylinder carrier, a class-911 \
         counterbore dimension replay agrees with its generated larger-cylinder carrier, or two same-feature \
         patches have complementary square outline bounds on one axis-normal plane. Later positional \
         instances do not inherit prototype placement or scalar \
         defaults; they require their per-instance parameter bodies \
         ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
         records."
    )));

    push_carrier_transfer_notes(
        &mut losses,
        scan,
        &coverage,
        container_only,
        placed_plane_count,
    );
    push_structural_layer_notes(&mut losses, scan);
    push_coverage_drop_losses(&mut losses, &coverage);

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: has_transferred_geometry(ir),
        coverage,
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary.notes,
    }
}
