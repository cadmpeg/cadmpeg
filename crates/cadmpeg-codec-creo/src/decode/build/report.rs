// SPDX-License-Identifier: Apache-2.0
//! Decode-report assembly from coverage counters and container census.

use crate::container::SectionRole;

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry,
};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

use crate::container::{self, ContainerScan};
use crate::loss::CreoLossCode;

use super::super::surfaces::brep::BrepTransferDiagnostics;
use super::report_coverage::push_coverage_drop_losses;
use super::report_losses::{
    push_brep_transfer_note, push_carrier_transfer_notes, push_legacy_value_losses,
    push_report_loss, push_structural_layer_notes,
};
use crate::decode::analytic::planes::is_axis_aligned;
use cadmpeg_ir::codec::DecodeBody;

pub(in super::super) fn has_transferred_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<bool, cadmpeg_core::CodecError> {
    let model = &ir.model;
    Ok(!model.points.is_empty()
        || !model.vertices.is_empty()
        || !model.edges.is_empty()
        || !model.coedges.is_empty()
        || !model.loops.is_empty()
        || !model.faces.is_empty()
        || !model.shells.is_empty()
        || !model.regions.is_empty()
        || !model.bodies.is_empty()
        || ctx.any_by(
            &model.surfaces,
            |surface| {
                Ok(!matches!(
                    surface.geometry.solved(),
                    Some(SolvedSurfaceGeometry::Unknown { .. })
                ))
            },
            "creo transferred surfaces search",
        )?
        || ctx.any_by(
            &model.curves,
            |curve| {
                Ok(!matches!(
                    curve.geometry.solved(),
                    Some(SolvedCurveGeometry::Unknown { .. })
                ))
            },
            "creo transferred curves search",
        )?
        || !model.subds.is_empty()
        || !model.pcurves.is_empty()
        || ctx.any_by(
            &model.procedural_surfaces,
            |surface| {
                Ok(!matches!(
                    surface.definition(),
                    ProceduralSurfaceDefinition::Unknown { .. }
                ))
            },
            "creo transferred procedural_surfaces search",
        )?
        || ctx.any_by(
            &model.procedural_curves,
            |curve| {
                Ok(!matches!(
                    curve.definition(),
                    ProceduralCurveDefinition::Unknown { .. }
                ))
            },
            "creo transferred procedural_curves search",
        )?
        || ctx.any_by(
            &model.sketch_entities,
            |entity| {
                Ok(!matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Native { .. }
                ))
            },
            "creo transferred sketch_entities search",
        )?
        || !model.tessellations.is_empty())
}

/// Build the decode body from the entry point's one dialect classification.
pub(in super::super) fn build_report(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
    ir: &CadIr,
    coverage: cadmpeg_ir::report::decode::Coverage,
    brep_diagnostics: &BrepTransferDiagnostics,
    container_only: bool,
) -> Result<DecodeBody, cadmpeg_core::CodecError> {
    struct OptionalCount<T>(Option<T>);
    impl<T: std::fmt::Display> std::fmt::Display for OptionalCount<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.0 {
                Some(value) => write!(f, "{value}"),
                None => f.write_str("n/a"),
            }
        }
    }

    let geom_sections = ctx
        .admit_iter(&scan.framing.sections, "creo geometry section census")?
        .filter(|s| s.role() == SectionRole::PsbGeometry)
        .count();
    let placed_frames = ctx
        .admit_iter(&scan.planes.local_systems, "creo placed local frame census")?
        .map(|source| {
            let frame = source.frame();
            let placed = if frame.origin.is_some() && frame.u_axis.is_some() {
                match frame.normal() {
                    Some(normal) => !is_axis_aligned(ctx, normal)?,
                    None => false,
                }
            } else {
                false
            };
            Ok::<_, cadmpeg_core::CodecError>(placed.then_some(source.surface_id))
        });
    let mut lookup_storage = ctx.reserve_scoped(0, "creo report placed plane lookup storage")?;
    let mut placed_plane_ids = BTreeSet::new();
    for id in placed_frames
        .chain(
            ctx.admit_iter(&scan.planes.outlines, "creo placed outline census")?
                .map(|plane| Ok(Some(plane.surface_id))),
        )
        .chain(
            ctx.admit_iter(
                &scan.planes.positional_frames,
                "creo placed positional frame census",
            )?
            .map(|plane| Ok(Some(plane.surface_id))),
        )
    {
        let Some(id) = id? else {
            continue;
        };
        lookup_storage.with_storage(|| ctx.insert_btree_set(
            &mut placed_plane_ids,
            id,
            "creo report placed plane ID nodes",
        ))?;
    }
    let placed_plane_count = placed_plane_ids.len();
    drop(placed_plane_ids);
    drop(lookup_storage);
    let mut losses = Vec::new();

    // The admission charge, first: it describes how the whole document was
    // read, not what any one record cost. Identity itself is authored once, in
    // `ir.source`; the report body carries only the charge.
    if let Some(loss) = classification.loss(ctx)? {
        ctx.reserve_vec(&mut losses, 1, "creo dialect report losses")?;
        losses.push(loss);
    }

    // The namespace census: what is byte-backed and readable.

    let srf = OptionalCount(scan.framing.census.srf_array_count);
    let crv = OptionalCount(scan.framing.census.crv_array_count);
    push_report_loss(ctx, &mut losses, CreoLossCode::ContainerCensus, format_args!(
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
    ))?;

    push_legacy_value_losses(ctx, &mut losses, &coverage)?;

    push_brep_transfer_note(ctx, &mut losses, brep_diagnostics, geom_sections)?;

    push_carrier_transfer_notes(
        ctx,
        &mut losses,
        scan,
        &coverage,
        container_only,
        placed_plane_count,
    )?;
    push_structural_layer_notes(ctx, &mut losses, scan)?;
    push_coverage_drop_losses(ctx, &mut losses, &coverage)?;

    Ok(DecodeBody {
        transfer: if container_only {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(has_transferred_geometry(ctx, ir)?)
        },
        coverage,
        losses,
        notes: container::notes(ctx, scan)?,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::build_report;
    use crate::decode::surfaces::brep::BrepTransferDiagnostics;
    use crate::surface::OutlinePlane;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::units::UnitVector3;

    #[test]
    fn geometry_signal_refuses_before_surface_search() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface".to_string())
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
            ),
            source_object: None,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error =
            super::has_transferred_geometry(&ctx, &ir).expect_err("surface traversal refused");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo transferred surfaces search"));
    }

    #[test]
    fn placed_plane_id_nodes_refuse_collection_limit() {
        let mut scan =
            crate::container::scan_bytes_ok(crate::test_support::build_prt("report", &[]));
        scan.planes.outlines.push(OutlinePlane {
            surface_id: 17,
            origin: [0.0; 3],
            normal: UnitVector3::Z_AXIS,
            u_axis: UnitVector3::X_AXIS,
            offset: 0,
        });
        let classification = crate::decode::with_test_decode_ctx(|ctx| {
            crate::dialect::classify(ctx, &scan).expect("service classification admitted")
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = build_report(
            &ctx,
            &scan,
            &classification,
            &CadIr::empty(),
            cadmpeg_ir::report::decode::Coverage::default(),
            &BrepTransferDiagnostics::default(),
            false,
        )
        .expect_err("placed plane node refused");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo report placed plane ID nodes"));
    }
}
