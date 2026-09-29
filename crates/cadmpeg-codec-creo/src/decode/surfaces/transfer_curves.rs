// SPDX-License-Identifier: Apache-2.0
//! Transfer of carrier intersection curves and NURBS boundary curves.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    nurbs::NurbsSurface, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::native::annotate;
use super::super::uniqueness::exactly_one;
use crate::decode::analytic::carriers::placed_carriers;
use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation};
use crate::decode::analytic::pcurves::pcurve_edge_endpoint_evidence;
use crate::decode::analytic::vertices::solved_topological_vertices;
use crate::decode::source_carriers::SourceUnitCarriers;

use super::intersection_resolve::{
    fc14_held_coordinate, multi_component_intersection_candidates, resolve_curve_candidates,
    select_fc14_axis_coordinate_candidate,
};
use super::intersection_candidates::FixedCandidates;
use super::intersections::carrier_intersection_curve;
use super::intersection_resolve::curve_contains_points;
use super::nurbs_boundaries::{
    cubic_extrusion_plane_generator_curve, nurbs_plane_boundary_curve,
    shared_extrusion_generator_curve,
};

pub(in super::super) fn analytic_curve_branches(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    tag: &'static str,
) -> Result<FixedCandidates<(CurveGeometry, &'static str)>, CodecError> {
    let opposite = if let CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) = geometry {
        Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                hyperbola_curve.opposite_branch(),
            )),
            tag,
        ))
    } else {
        None
    };
    Ok(std::iter::once((geometry.copy_admitted(ctx, "creo analytic curve branch geometry")?, tag))
        .chain(opposite)
        .collect())
}

fn resolve_carrier_intersection_curve(
    ctx: &DecodeContext<'_>,
    first: CarrierEquation,
    second: CarrierEquation,
    points: Option<[[f64; 3]; 2]>,
    allow_unresolved_endpoint_witness: bool,
) -> Result<Option<(CurveGeometry, &'static str)>, CodecError> {
    let Some((geometry, tag)) = carrier_intersection_curve(first, second) else { return Ok(None) };
    let candidates = analytic_curve_branches(ctx, &geometry, tag)?;
    let selected = match points {
        Some(points) => exactly_one(candidates.iter().enumerate()
            .filter(|(_, (geometry, _))| curve_contains_points(geometry, points))
            .map(|(index, _)| index)),
        None => (candidates.len() == 1).then_some(0),
    };
    let selected = selected.or_else(|| {
        // A one-sided pcurve on an unresolved adjacent face supplies only a
        // finite-edge witness. It does not veto the exact infinite plane line.
        (tag == "plane_intersection_line"
            && (points.is_none() || allow_unresolved_endpoint_witness))
            .then_some(())
            .filter(|_| candidates.len() == 1)
            .map(|_| 0)
    });
    Ok(selected.and_then(|index| candidates.into_iter().nth(index)))
}

pub(in super::super) fn transfer_carrier_intersection_curves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<BTreeSet<CurveId>, cadmpeg_core::CodecError> {
    let mut transferred = BTreeSet::new();
    let carriers = placed_carriers(ctx, scan, ir, source_carriers)?;
    let solved_vertices = solved_topological_vertices(
        ctx,
        scan,
        ir,
        &carriers,
        nurbs_endpoint_witnesses,
        source_carriers,
    )?;
    let endpoint_evidence = pcurve_edge_endpoint_evidence(ctx, scan, ir, source_carriers)?;
    let edge_vertices =
        crate::topology::edge_vertex_pairs(ctx, &scan.topology.half_edge_vertex_incidence)?;
    for row in crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| row.id)? {
        let [Some(first_face), Some(second_face)] = row.faces else {
            continue;
        };
        let (Some(first), Some(second)) = (
            carriers.get(&first_face.get()).copied(),
            carriers.get(&second_face.get()).copied(),
        ) else {
            continue;
        };
        let points = (|| {
            let vertices = edge_vertices.get(&row.id)?;
            let points = [
                *solved_vertices.get(&vertices[0])?,
                *solved_vertices.get(&vertices[1])?,
            ];
            Some(points)
        })();
        let curve_id = crate::identity::compose_checked::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            row.id,
            "creo carrier intersection curve identity",
        )?;
        let allow_unresolved_endpoint_witness = endpoint_evidence
            .get(&row.id)
            .is_some_and(|evidence| !evidence.complete)
            && !nurbs_endpoint_witnesses.contains(&curve_id);
        let resolved = resolve_carrier_intersection_curve(
            ctx,
            first,
            second,
            points,
            allow_unresolved_endpoint_witness,
        )?
        .or_else(|| {
            let candidates = multi_component_intersection_candidates(first, second);
            if points.is_some() {
                resolve_curve_candidates(candidates, points)
            } else {
                let held = fc14_held_coordinate(&scan.curves.fc_coordinates, row.id)?;
                select_fc14_axis_coordinate_candidate(candidates, held)
            }
        });
        let Some((geometry, tag)) = resolved else {
            continue;
        };
        let id = curve_id;
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            tag,
            Exactness::Derived,
        );
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id: crate::identity::copy_checked_id(
                    ctx,
                    id.as_str(),
                    "creo carrier intersection IR curve ID copy",
                )?,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("VisibGeom:{}", row.id),
                        "creo carrier intersection source object ID",
                    )?)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        ctx.charge_collection_items(1, "creo transferred carrier curve nodes")?;
        transferred.insert(id);
    }
    Ok(transferred)
}

pub(in super::super) struct TransferredNurbsBoundaryCurves {
    pub(in super::super) ids: BTreeSet<CurveId>,
    pub(in super::super) endpoint_witnesses: BTreeSet<CurveId>,
    pub(in super::super) extrusion_plane_count: usize,
    pub(in super::super) extrusion_plane_section_generator_count: usize,
    pub(in super::super) shared_extrusion_generator_count: usize,
}

#[derive(Clone, Copy)]
enum NurbsBoundaryKind {
    ExtrusionPlane,
    ExtrusionPlaneSectionGenerator,
    SharedExtrusionGenerator,
}

/// Boundary curve of an extrusion surface against a plane, with the
/// section-generator fallback.
///
/// The sink is drained before the fallback runs, so the fallback's own `?`
/// never sits between a refusal and the report that names it. A refused lane
/// that the fallback covers costs the model nothing and states no loss. A
/// refused lane that leaves the curve-topology row without a carrier is a loss
/// note naming the row and the surface that stated the lane.
fn extrusion_plane_boundary_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    surface_id: u32,
    curve_row_id: u32,
    plane: PlaneEquation,
    refusal: &mut crate::lane_refusal::LaneRefusals,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<(CurveGeometry, NurbsBoundaryKind)>, CodecError> {
    if let Some(geometry) = nurbs_plane_boundary_curve(ctx, nurbs, surface_id, plane, refusal)? {
        return Ok(Some((geometry, NurbsBoundaryKind::ExtrusionPlane)));
    }
    let refused = refusal.take_records_checked()?;
    let mut fallback_losses = Vec::new();
    let fallback =
        cubic_extrusion_plane_generator_curve(ctx, nurbs, surface_id, plane, &mut fallback_losses)?
            .map(|geometry| (geometry, NurbsBoundaryKind::ExtrusionPlaneSectionGenerator));
    if fallback.is_none() {
        append_boundary_fallback_losses(ctx, losses, fallback_losses)?;
        note_boundary_lane_records(ctx, curve_row_id, &refused, losses)?;
    }
    Ok(fallback)
}

fn append_boundary_fallback_losses(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    fallback_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    ctx.try_reserve_items(losses, fallback_losses.len(), "creo boundary fallback losses")?;
    losses.extend(fallback_losses);
    Ok(())
}

/// Drain every refused boundary lane into one loss note per record.
fn note_refused_boundary_lanes(
    ctx: &DecodeContext<'_>,
    curve_row_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let records = refusal.take_records_checked()?;
    note_boundary_lane_records(ctx, curve_row_id, &records, losses)
}

/// One loss note per refused boundary-lane record, each naming the row.
fn note_boundary_lane_records(
    ctx: &DecodeContext<'_>,
    curve_row_id: u32,
    records: &[String],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    for record in records {
        let message = ctx.format_retained(
            format_args!(
                "VisibGeom curve-topology row {curve_row_id} states no NURBS boundary carrier: \
                 {record}"
            ),
            "creo boundary loss message",
        )?;
        ctx.try_reserve_items(losses, 1, "creo boundary loss notes")?;
        losses.push(
            crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.note(message),
        );
    }
    Ok(())
}

pub(in super::super) fn transfer_nurbs_boundary_curves(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<TransferredNurbsBoundaryCurves, CodecError> {
    let mut result = TransferredNurbsBoundaryCurves {
        ids: BTreeSet::new(),
        endpoint_witnesses: BTreeSet::new(),
        extrusion_plane_count: 0,
        extrusion_plane_section_generator_count: 0,
        shared_extrusion_generator_count: 0,
    };
    for row in crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| row.id)? {
        let [Some(first_face), Some(second_face)] = row.faces else {
            continue;
        };
        let Some(first) = crate::surface::unique_surface_row(&scan.surfaces.rows, first_face.get())
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_row(&scan.surfaces.rows, second_face.get())
        else {
            continue;
        };
        let geometry = |surface_id| {
            exactly_one(ir.model.surfaces.iter().filter(|surface| {
                crate::identity::matches_numbered_identity(
                    surface.id.as_str(),
                    "creo:visibgeom:surface#",
                    surface_id,
                )
            }))
                .map(|surface| source_carriers.surface_geometry(surface))
        };
        let Some(first_geometry) = geometry(first.id) else {
            continue;
        };
        let Some(second_geometry) = geometry(second.id) else {
            continue;
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let refusal = &mut refusal;
        let resolved = match (first.kind, second.kind, first_geometry, second_geometry) {
            (
                crate::surface::SurfaceKind::Extrusion(_),
                crate::surface::SurfaceKind::Plane,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            ) => {
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                extrusion_plane_boundary_curve(
                    ctx, nurbs, first.id, row.id, plane, refusal, losses,
                )?
            }
            (
                crate::surface::SurfaceKind::Plane,
                crate::surface::SurfaceKind::Extrusion(_),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ) => {
                let origin = plane_surface_2.origin();
                let normal = plane_surface_2.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                extrusion_plane_boundary_curve(
                    ctx, nurbs, second.id, row.id, plane, refusal, losses,
                )?
            }
            (
                crate::surface::SurfaceKind::Extrusion(_),
                crate::surface::SurfaceKind::Extrusion(_),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(first_nurbs)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(second_nurbs)),
            ) => shared_extrusion_generator_curve(
                ctx,
                first_nurbs,
                first.id,
                second_nurbs,
                second.id,
                refusal,
            )?
            .map(|geometry| (geometry, NurbsBoundaryKind::SharedExtrusionGenerator)),
            _ => None,
        };
        let Some((geometry, kind)) = resolved else {
            note_refused_boundary_lanes(ctx, row.id, refusal, losses)?;
            continue;
        };
        let id = crate::identity::compose_checked::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            row.id,
            "creo NURBS boundary curve identity",
        )?;
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            match kind {
                NurbsBoundaryKind::ExtrusionPlane => "extrusion_plane_nurbs_boundary",
                NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                    "extrusion_plane_nurbs_section_generator"
                }
                NurbsBoundaryKind::SharedExtrusionGenerator => "shared_extrusion_nurbs_generator",
            },
            Exactness::Derived,
        );
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id: crate::identity::copy_checked_id(
                    ctx,
                    id.as_str(),
                    "creo NURBS boundary IR curve ID copy",
                )?,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("VisibGeom:{}", row.id),
                        "creo NURBS boundary source object ID",
                    )?)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        ctx.charge_collection_items(1, "creo NURBS boundary curve ID nodes")?;
        result.ids.insert(crate::identity::copy_checked_id(
            ctx,
            id.as_str(),
            "creo NURBS boundary result curve ID copy",
        )?);
        ctx.charge_collection_items(1, "creo NURBS boundary endpoint nodes")?;
        result.endpoint_witnesses.insert(id);
        match kind {
            NurbsBoundaryKind::ExtrusionPlane => result.extrusion_plane_count += 1,
            NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                result.extrusion_plane_section_generator_count += 1;
            }
            NurbsBoundaryKind::SharedExtrusionGenerator => {
                result.shared_extrusion_generator_count += 1;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::resolve_carrier_intersection_curve as resolve_carrier_intersection_curve_admitted;
    use std::collections::BTreeSet;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        nurbs::NurbsSurface, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::AnnotationBuilder;

    use super::transfer_nurbs_boundary_curves;
    use super::transfer_carrier_intersection_curves;

    fn resolve_carrier_intersection_curve(
        first: CarrierEquation,
        second: CarrierEquation,
        points: Option<[[f64; 3]; 2]>,
        allow_unresolved_endpoint_witness: bool,
    ) -> Option<(CurveGeometry, &'static str)> {
        crate::decode::with_test_decode_ctx(|ctx| {
            resolve_carrier_intersection_curve_admitted(
                ctx, first, second, points, allow_unresolved_endpoint_witness,
            )
        })
        .expect("carrier intersection admission")
    }
    use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation};
    use crate::topology::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, TopologicalVertex};
    use crate::{container, curve, surface};

    #[test]
    fn analytic_curve_branch_copy_refuses_below_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_ir::ids::ProceduralCurveId;

        let identity = "test:model:procedural#curve";
        let geometry = CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(identity).expect("valid construction ID"),
            cache: None,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = identity.len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = super::analytic_curve_branches(&ctx, &geometry, "procedural")
            .expect_err("copy exceeds retained limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo analytic curve branch geometry"));
        policy.limits.max_retained_bytes = identity.len() as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let branches = super::analytic_curve_branches(&ctx, &geometry, "procedural")
            .expect("exact cap admits branch");
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].0, geometry);
    }

    fn carrier_transfer_with_limits(
        policy: DecodePolicy,
    ) -> Result<BTreeSet<CurveId>, cadmpeg_core::CodecError> {
        let mut scan = container::scan_bytes_ok(Vec::new());
        scan.surfaces.rows = [1_u32, 2]
            .into_iter()
            .map(|id| surface::SurfaceRow {
                id,
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            })
            .collect();
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [1_u32, 2].map(std::num::NonZeroU32::new),
            next_edges: [10, 10],
            offset: 0,
        }];
        let mut ir = CadIr::empty();
        for (id, origin, normal) in [
            (1, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
            (2, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        ] {
            ir.model.surfaces.push(Surface {
                id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
                    .expect("valid source surface ID"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        origin,
                        normal,
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid plane fixture"),
                )),
                source_object: None,
            });
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        transfer_carrier_intersection_curves(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &BTreeSet::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    }

    #[test]
    fn carrier_transfer_retained_boundaries_refuse_before_identity_and_source_copies() {
        use cadmpeg_core::decode::ResourceDimension;

        assert_eq!(
            carrier_transfer_with_limits(DecodePolicy::service()).expect("service transfer"),
            BTreeSet::from([
                CurveId::mint("creo:visibgeom:curve#10".to_owned()).expect("valid curve ID")
            ])
        );
        for operation in [
            "creo carrier intersection curve identity",
            "creo carrier intersection IR curve ID copy",
            "creo carrier intersection source object ID",
        ] {
            let refusal = (0..512).find_map(|limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                match carrier_transfer_with_limits(policy) {
                    Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                        if refusal.operation == operation => Some(refusal),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_)) => None,
                    other => panic!("{operation} was not reached before {other:?}"),
                }
            }).expect("named retained boundary reached");
            assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
            assert!(refusal.limit < refusal.used + refusal.additional);
        }
    }

    #[test]
    fn carrier_transfer_result_node_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        assert!(carrier_transfer_with_limits(DecodePolicy::service()).is_ok());
        let refusal = (0..64).find_map(|limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            match carrier_transfer_with_limits(policy) {
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.operation == "creo transferred carrier curve nodes" => Some(refusal),
                Err(cadmpeg_core::CodecError::ResourceLimit(_)) => None,
                other => panic!("carrier result node was not reached before {other:?}"),
            }
        }).expect("carrier result node reached");
        assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
        assert_eq!(refusal.limit, refusal.used);
    }

    #[test]
    fn carrier_intersection_rejects_solved_endpoints_off_candidate() {
        let first = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        });
        let second = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        let off_candidate = [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]];

        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), false).is_none()
        );
        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), true).is_some()
        );
        assert!(resolve_carrier_intersection_curve(first, second, None, false).is_some());
    }

    #[test]
    fn plane_intersection_survives_inconsistent_endpoint_witness() {
        let mut scan = container::scan_bytes_ok(Vec::new());
        scan.surfaces.rows = [1_u32, 2, 3]
            .into_iter()
            .map(|id| surface::SurfaceRow {
                id,
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            })
            .collect();
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [10, 10],
            offset: 0,
        }];
        scan.curves.pcurves = vec![curve::PcurveEndpoints {
            curve_id: 10,
            faces: [1, 3].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[0.0, 1.0], [1.0, 1.0]],
            face_1_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            offset: 0,
        }];
        scan.topology.half_edges = vec![
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                },
                face_id: std::num::NonZeroU32::new(1),
                next: None,
            },
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                },
                face_id: std::num::NonZeroU32::new(2),
                next: None,
            },
        ];
        scan.topology.vertices = vec![
            TopologicalVertex {
                id: 1,
                half_edges: vec![HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                }],
            },
            TopologicalVertex {
                id: 2,
                half_edges: vec![HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                }],
            },
        ];
        scan.topology.half_edge_vertex_incidence = vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id: 1,
                end_vertex_id: Some(2),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                },
                start_vertex_id: 2,
                end_vertex_id: Some(1),
            },
        ];

        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#1".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 2.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#2".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#3".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            },
        ]);

        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_carrier_intersection_curves(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &BTreeSet::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert_eq!(
            transferred,
            BTreeSet::from([
                CurveId::mint("creo:visibgeom:curve#10".to_string()).expect("identity grammar")
            ])
        );
        assert!(matches!(ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id
            == CurveId::mint("creo:visibgeom:curve#10".to_string()).expect("identity grammar"))
        .map(|curve| &curve.geometry), Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)))
            if {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                origin.x == 0.0
                    && origin.y == 2.0
                    && origin.z == 0.0
                    && direction.x == 1.0
                    && direction.y == 0.0
                    && direction.z == 0.0
            }));
    }

    fn nurbs_boundary_fixture(
        duplicate_extrusion: bool,
    ) -> (container::ContainerScan<'static>, CadIr) {
        let mut scan = container::scan_bytes_ok(Vec::new());
        scan.surfaces.rows = vec![
            surface::SurfaceRow {
                id: 1,
                kind: surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
            surface::SurfaceRow {
                id: 2,
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
        ];
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [10, 10],
            offset: 0,
        }];

        let extrusion = Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#1".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                NurbsSurface::from_lanes(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                        vec![
                            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                            vec![Point3::new(1.0, 0.0, 1.0), Point3::new(1.0, 1.0, 1.0)],
                        ],
                        None,
                    ),
                    false,
                )
                .expect("valid test surface"),
            )),
            source_object: None,
        };
        let plane = Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#2".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(extrusion.clone());
        if duplicate_extrusion {
            ir.model.surfaces.push(extrusion);
        }
        ir.model.surfaces.push(plane);
        (scan, ir)
    }

    #[test]
    fn nurbs_boundary_rejects_duplicate_model_surface_ids() {
        let (scan, mut ir) = nurbs_boundary_fixture(true);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");

        let result = transfer_nurbs_boundary_curves(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("transfer should not fail");

        assert!(result.ids.is_empty());
        assert!(ir.model.curves.is_empty());
    }

    fn nurbs_boundary_transfer_with_limits(
        policy: DecodePolicy,
    ) -> Result<super::TransferredNurbsBoundaryCurves, cadmpeg_core::CodecError> {
        let (scan, mut ir) = nurbs_boundary_fixture(false);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        transfer_nurbs_boundary_curves(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    }

    #[test]
    fn nurbs_boundary_result_nodes_refuse_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let service = nurbs_boundary_transfer_with_limits(DecodePolicy::service())
            .expect("service boundary transfer");
        assert_eq!(service.ids.len(), 1);
        assert_eq!(service.endpoint_witnesses.len(), 1);
        for operation in [
            "creo NURBS boundary curve ID nodes",
            "creo NURBS boundary endpoint nodes",
        ] {
            let refusal = (0..512).find_map(|limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                match nurbs_boundary_transfer_with_limits(policy) {
                    Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                        if refusal.operation == operation => Some(refusal),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_)) => None,
                    Ok(_) => panic!("{operation} was not reached before transfer completed"),
                    Err(error) => panic!("{operation} was not reached before {error:?}"),
                }
            }).expect("named boundary node reached");
            assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
            assert_eq!(refusal.limit, refusal.used);
        }
    }

    #[test]
    fn nurbs_boundary_retained_text_refuses_before_identity_and_source_copies() {
        use cadmpeg_core::decode::ResourceDimension;

        assert_eq!(
            nurbs_boundary_transfer_with_limits(DecodePolicy::service())
                .expect("service boundary transfer")
                .ids
                .len(),
            1
        );
        for operation in [
            "creo NURBS boundary curve identity",
            "creo NURBS boundary IR curve ID copy",
            "creo NURBS boundary source object ID",
            "creo NURBS boundary result curve ID copy",
        ] {
            let refusal = (0..1024).find_map(|limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                match nurbs_boundary_transfer_with_limits(policy) {
                    Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                        if refusal.operation == operation => Some(refusal),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_)) => None,
                    Ok(_) => panic!("{operation} was not reached before transfer completed"),
                    Err(error) => panic!("{operation} was not reached before {error:?}"),
                }
            }).expect("named retained boundary reached");
            assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
            assert!(refusal.limit < refusal.used + refusal.additional);
        }
    }

    #[test]
    fn refused_boundary_lanes_name_the_curve_topology_row_and_continue() {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        refusal.note(
            "creo VisibGeom surface row 7 boundary curve record",
            &cadmpeg_ir::geometry::nurbs::NurbsError::Structure("knot vector".to_owned()),
        );
        let mut losses = Vec::new();

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root admitted");

        super::note_refused_boundary_lanes(&ctx, 41, &mut refusal, &mut losses)
            .expect("refusal report is admitted");

        let [note] = losses.as_slice() else {
            panic!("one loss note per refused record");
        };
        assert_eq!(
            note.code,
            crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.kind()
        );
        assert!(note.message.contains("row 41"), "{}", note.message);
        assert!(note.message.contains("surface row 7"), "{}", note.message);
        assert!(refusal.take_records().is_empty());
    }

    fn boundary_loss_with_limits(
        policy: DecodePolicy,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut losses = Vec::new();
        super::note_boundary_lane_records(&ctx, 41, &["surface row 7".to_owned()], &mut losses)?;
        Ok(losses)
    }

    #[test]
    fn boundary_loss_message_refuses_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let service = boundary_loss_with_limits(DecodePolicy::service())
            .expect("service profile admits the loss note");
        assert_eq!(service.len(), 1);
        assert!(service[0].message.contains("row 41"));
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = service[0].message.len() as u64 - 1;
        let error = boundary_loss_with_limits(policy).expect_err("message exceeds retained cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "creo boundary loss message"));
    }

    #[test]
    fn boundary_loss_note_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        assert_eq!(boundary_loss_with_limits(DecodePolicy::service()).expect("service note").len(), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let error = boundary_loss_with_limits(policy).expect_err("one loss exceeds item cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo boundary loss notes"));
    }

    #[test]
    fn boundary_fallback_losses_refuse_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let run = |policy: DecodePolicy| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root admitted");
            let mut losses = Vec::new();
            let fallback = crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved
                .note("fallback failure");
            super::append_boundary_fallback_losses(&ctx, &mut losses, vec![fallback])?;
            Ok::<_, cadmpeg_core::CodecError>(losses)
        };
        assert_eq!(run(DecodePolicy::service()).expect("service fallback").len(), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let error = run(policy).expect_err("one fallback exceeds item cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo boundary fallback losses"));
    }
}
