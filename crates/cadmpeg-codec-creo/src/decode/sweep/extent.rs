// SPDX-License-Identifier: Apache-2.0
//! Extrusion span resolution from carriers, cylinders, NURBS translation, and rectilinear planes.

use super::super::holes::placement::ExtrusionSpan;
use super::super::holes::sweep::{extrusion_extent_and_direction, extrusion_span};
use super::planes::{
    feature_plane_equations, generated_arc_cylinder_extent, generated_cap_plane_extent,
};
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::analytic::planes::{canonical_plane, placed_planes, reconciled_model_plane};
use crate::vecmath::dot;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{nurbs::NurbsSurface, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::scalar::FiniteReal;

/// General reconstructed sweep-extent geometry tolerance.
const EPS_SWEEP_EXTENT_GEOMETRY: f64 = 1.0e-9;
/// Threshold for degenerate sweep-extent configurations.
const EPS_SWEEP_EXTENT_DEGENERATE: f64 = 1.0e-10;

pub(in super::super) struct ExtrusionCarrierSpan {
    pub(in super::super) starts: Vec<[f64; 3]>,
    pub(in super::super) vector: [f64; 3],
}

enum SourceSurfaceGeometry<'a> {
    Missing,
    Present(&'a SurfaceGeometry),
}

fn unique_source_surface_geometry<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &'a CadIr,
    source_carriers: &'a crate::decode::source_carriers::SourceUnitCarriers,
    surface_id: u32,
) -> Result<Option<SourceSurfaceGeometry<'a>>, cadmpeg_core::CodecError> {
    let mut found = None;
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo numbered identity candidate scan")? {
        if crate::identity::matches_numbered_identity(
            surface.id.as_str(),
            "creo:visibgeom:surface#",
            surface_id,
        ) {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(surface);
        }
    }
    Ok(Some(
        found.map_or(SourceSurfaceGeometry::Missing, |surface| {
            SourceSurfaceGeometry::Present(source_carriers.surface_geometry(surface))
        }),
    ))
}

fn blind_extrusion_from_carriers(
    carriers: &[ExtrusionCarrierSpan],
    planes: &[([f64; 3], [f64; 3])],
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let first = carriers.first()?;
    let first_start = *first.starts.first()?;
    let direction = normalize(first.vector)?;
    let length = first.vector.into_iter().fold(0.0_f64, f64::hypot);
    (length.is_finite() && length > 0.0).then_some(())?;
    let coordinate_scale = carriers
        .iter()
        .flat_map(|carrier| carrier.starts.iter().flatten().copied())
        .chain(planes.iter().flat_map(|(origin, _)| *origin))
        .chain(
            transform
                .into_iter()
                .flat_map(crate::placement::FeatureSectionTransform::origin),
        )
        .map(f64::abs)
        .fold(length.max(1.0), f64::max);
    if !coordinate_scale.is_finite() {
        return None;
    }
    let tolerance = EPS_SWEEP_EXTENT_GEOMETRY * coordinate_scale;
    let vector_tolerance = EPS_SWEEP_EXTENT_GEOMETRY * length.max(1.0);
    let start_station = dot(first_start, direction);
    let end_station = start_station + length;
    let mut has_opposed_carrier = false;
    carriers
        .iter()
        .all(|carrier| {
            if carrier.starts.is_empty() {
                return false;
            }
            let same_direction = carrier
                .vector
                .into_iter()
                .zip(first.vector)
                .all(|(candidate, reference)| (candidate - reference).abs() <= vector_tolerance);
            let opposite_direction = carrier
                .vector
                .into_iter()
                .zip(first.vector)
                .all(|(candidate, reference)| (candidate + reference).abs() <= vector_tolerance);
            has_opposed_carrier |= opposite_direction;
            (same_direction
                && carrier
                    .starts
                    .iter()
                    .all(|start| (dot(*start, direction) - start_station).abs() <= tolerance))
                || (opposite_direction
                    && carrier
                        .starts
                        .iter()
                        .all(|start| (dot(*start, direction) - end_station).abs() <= tolerance))
        })
        .then_some(())?;
    let mut unique_stations = [0.0; 2];
    let mut station_count = 0;
    for (origin, normal) in planes {
        let normal = normalize(*normal)?;
        let alignment = dot(normal, direction).abs();
        if alignment >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE {
            let station = dot(*origin, direction);
            if unique_stations[..station_count]
                .iter()
                .all(|existing| (station - existing).abs() > tolerance)
            {
                if station_count == unique_stations.len() {
                    return None;
                }
                unique_stations[station_count] = station;
                station_count += 1;
            }
        } else if alignment > EPS_SWEEP_EXTENT_DEGENERATE || !alignment.is_finite() {
            return None;
        }
    }
    let reverse = if has_opposed_carrier {
        if let Some(transform) = transform {
            let transform_station = dot(transform.origin(), direction);
            if (transform_station - start_station).abs() <= tolerance {
                false
            } else if (transform_station - end_station).abs() <= tolerance {
                true
            } else {
                return None;
            }
        } else {
            let [terminal_station] = &unique_stations[..station_count] else {
                return None;
            };
            if (*terminal_station - end_station).abs() <= tolerance {
                false
            } else if (*terminal_station - start_station).abs() <= tolerance {
                true
            } else {
                return None;
            }
        }
    } else {
        false
    };
    let (direction, start_station, end_station) = if reverse {
        (
            direction.map(|component| -component),
            -end_station,
            -start_station,
        )
    } else {
        (direction, start_station, end_station)
    };
    if let Some(transform) = transform {
        let normal = transform.normal();
        ((dot(direction, normal).abs() - 1.0).abs() <= EPS_SWEEP_EXTENT_DEGENERATE
            && (dot(transform.origin(), direction) - start_station).abs() <= tolerance)
            .then_some(())?;
    }
    if reverse {
        for station in &mut unique_stations[..station_count] {
            *station = -*station;
        }
    }
    let cap_matches = |cap: f64| {
        (cap - start_station).abs() <= tolerance || (cap - end_station).abs() <= tolerance
    };
    match &unique_stations[..station_count] {
        [] => {}
        [cap] if cap_matches(*cap) => {}
        [first_cap, second_cap]
            if cap_matches(*first_cap)
                && cap_matches(*second_cap)
                && ((first_cap - second_cap).abs() - length).abs() <= tolerance => {}
        _ => return None,
    }
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(length)?,
                },
                draft: None,
            },
        },
        direction,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        blind_extrusion_from_carriers, bounded_cylinder_span, nurbs_translation_span,
        ExtrusionCarrierSpan,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    #[test]
    fn infinite_carrier_origin_does_not_expand_station_tolerance() {
        let carriers = [
            ExtrusionCarrierSpan {
                starts: vec![[0.0, 0.0, 0.0]],
                vector: [0.0, 0.0, 1.0],
            },
            ExtrusionCarrierSpan {
                starts: vec![[0.0, 0.0, f64::INFINITY]],
                vector: [0.0, 0.0, 1.0],
            },
        ];
        assert!(blind_extrusion_from_carriers(&carriers, &[], None).is_none());
    }

    #[test]
    fn bounded_cylinder_start_limit_refuses_before_vector_creation() {
        let frame = crate::surface::PositionalCylinderFrame::new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            1.0,
            Some(2.0),
        )
        .expect("valid frame");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = bounded_cylinder_span(&ctx, frame, &[]);
        assert!(matches!(result, Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder starts"));
    }

    #[test]
    fn nurbs_translation_start_limit_refuses_before_grid_creation() {
        let surface = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![
                    vec![
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0),
                    ],
                    vec![
                        cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(1.0, 0.0, 2.0),
                    ],
                ],
                None,
            ),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid translation surface");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = nurbs_translation_span(&ctx, &surface);
        assert!(matches!(result, Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo NURBS translation starts"));
    }

    fn bounded_extent_at_limit(
        limit: u64,
    ) -> Result<Option<(cadmpeg_ir::features::ExtrudeExtent, [f64; 3])>, CodecError> {
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::SurfaceId;
        use cadmpeg_ir::math::{Point3, Vector3};
        let mut scan = crate::test_support::empty_container_scan();
        for (id, kind) in [
            (31, crate::surface::SurfaceKind::Plane),
            (32, crate::surface::SurfaceKind::Plane),
            (33, crate::surface::SurfaceKind::Cylinder),
        ] {
            scan.surfaces.rows.push(crate::surface::SurfaceRow {
                id,
                kind,
                feature_id: 7,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: usize::try_from(id).expect("fixture index fits usize"),
            });
        }
        scan.surfaces
            .parameters
            .push(crate::surface::SurfaceParameterRecord {
                surface_id: 33,
                body: Vec::new(),
                scalar_tokens: Vec::new(),
                opaque_spans: Vec::new(),
                scalar_frames: Vec::new(),
                carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                    crate::surface::InlineSurfaceCarrier::Cylinder {
                        frame: crate::surface::PositionalCylinderFrame::new(
                            [2.0, 4.0, 0.0],
                            [0.0, -1.0, 0.0],
                            [1.0, 0.0, 0.0],
                            1.0,
                            Some(8.0),
                        )
                        .expect("valid frame"),
                        split_bounds: None,
                    },
                ),
                boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                offset: 33,
                body_offset: 34,
            });
        let plane = |id, y, normal| Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, y, 0.0),
                    normal,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid plane"),
            )),
            source_object: None,
        };
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.model.surfaces.extend([
            plane(31, 4.0, Vector3::new(0.0, 1.0, 0.0)),
            plane(32, -4.0, Vector3::new(0.0, -1.0, 0.0)),
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#33").expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(2.0, 4.0, 0.0),
                        Vector3::new(0.0, -1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        1.0,
                    )
                    .expect("valid cylinder"),
                )),
                source_object: None,
            },
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        super::generated_bounded_cylinder_extent(
            &ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None,
        )
    }

    #[test]
    fn bounded_cylinder_cap_plane_limit_refuses() {
        assert!(
            matches!(bounded_extent_at_limit(0), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder cap planes")
        );
    }

    #[test]
    fn bounded_cylinder_frame_limit_refuses() {
        assert!(
            matches!(bounded_extent_at_limit(2), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder frames")
        );
    }

    #[test]
    fn bounded_cylinder_carrier_limit_refuses() {
        assert!(
            matches!(bounded_extent_at_limit(4), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder carriers")
        );
        assert!(bounded_extent_at_limit(5)
            .expect("admitted extent")
            .is_some());
    }

    fn nurbs_extent_at_limit(
        limit: u64,
    ) -> Result<Option<(cadmpeg_ir::features::ExtrudeExtent, [f64; 3])>, CodecError> {
        use cadmpeg_ir::geometry::{nurbs, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::SurfaceId;
        use cadmpeg_ir::math::{Point3, Vector3};
        let nurbs = nurbs::NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            nurbs::NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            nurbs::NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 2.0)],
                    vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 2.0)],
                ],
                None,
            ),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid translation surface");
        let mut scan = crate::test_support::empty_container_scan();
        for (id, kind) in [
            (
                31,
                crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
            ),
            (32, crate::surface::SurfaceKind::Plane),
        ] {
            scan.surfaces.rows.push(crate::surface::SurfaceRow {
                id,
                kind,
                feature_id: 7,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: usize::try_from(id).expect("fixture index fits usize"),
            });
        }
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#31").expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#32").expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid plane"),
                )),
                source_object: None,
            },
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        super::generated_nurbs_translation_extent(
            &ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None,
        )
    }

    #[test]
    fn nurbs_translation_carrier_limit_refuses() {
        assert!(
            matches!(nurbs_extent_at_limit(3), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo NURBS translation carriers")
        );
    }

    #[test]
    fn nurbs_translation_cap_plane_limit_refuses() {
        assert!(
            matches!(nurbs_extent_at_limit(4), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo NURBS translation cap planes")
        );
        assert!(nurbs_extent_at_limit(5).expect("admitted extent").is_some());
    }
}

pub(in super::super) fn generated_bounded_cylinder_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    enum CylinderExtentSurface {
        Plane,
        Carrier,
    }
    let local_planes = placed_planes(ctx, scan)?;
    let mut frames = Vec::new();
    let mut planes = Vec::new();
    let mut saw_row = false;
    for row in scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
    {
        saw_row = true;
        let kind = match row.kind {
            crate::surface::SurfaceKind::Plane => CylinderExtentSurface::Plane,
            crate::surface::SurfaceKind::Cylinder => CylinderExtentSurface::Carrier,
            _ => return Ok(None),
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) != Some(row) {
            return Ok(None);
        }
        let Some(source_geometry) =
            unique_source_surface_geometry(ctx, ir, source_carriers, row.id)?
        else {
            return Ok(None);
        };
        match kind {
            CylinderExtentSurface::Plane => match source_geometry {
                SourceSurfaceGeometry::Missing => {
                    if let Some(plane) = local_planes.get(&row.id) {
                        ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes")?;
                        planes.push((plane.origin, plane.normal));
                    }
                }
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Plane(_),
                )) => {
                    let Some(plane) =
                        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, row.id)?
                    else {
                        return Ok(None);
                    };
                    ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes")?;
                    planes.push((plane.origin, plane.normal));
                }
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { .. },
                )) => {
                    if let Some(plane) = local_planes.get(&row.id) {
                        ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes")?;
                        planes.push((plane.origin, plane.normal));
                    }
                }
                SourceSurfaceGeometry::Present(_) => return Ok(None),
            },
            CylinderExtentSurface::Carrier => match source_geometry {
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { .. },
                )) => {}
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Cylinder(cylinder_surface),
                )) => {
                    let origin = cylinder_surface.origin().get();
                    let axis = *cylinder_surface.frame().axis();
                    let Some(parameters) =
                        crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)
                    else {
                        return Ok(None);
                    };
                    let Some(frame) = parameters.positional_cylinder_frame() else {
                        return Ok(None);
                    };
                    let transferred_origin = [origin.x, origin.y, origin.z];
                    let transferred_axis = unit_length(axis);
                    let frame_axis = unit_length(*frame.frame().orthonormal_frame().axis());
                    let scale = transferred_origin
                        .into_iter()
                        .chain(frame.frame().origin())
                        .map(f64::abs)
                        .fold(1.0, f64::max);
                    if !(transferred_origin
                        .into_iter()
                        .zip(frame.frame().origin())
                        .all(|(left, right)| {
                            (left - right).abs() <= EPS_SWEEP_EXTENT_GEOMETRY * scale
                        })
                        && transferred_axis
                            .into_iter()
                            .zip(frame_axis)
                            .all(|(left, right)| {
                                (left - right).abs() <= EPS_SWEEP_EXTENT_DEGENERATE
                            }))
                    {
                        return Ok(None);
                    }
                    ctx.reserve_vec(&mut frames, 1, "creo bounded cylinder frames")?;
                    frames.push(frame);
                }
                _ => return Ok(None),
            },
        }
    }
    if !saw_row {
        return Ok(None);
    }
    let mut carriers = Vec::new();
    for frame in frames {
        let Some(span) = bounded_cylinder_span(ctx, frame, &planes)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut carriers, 1, "creo bounded cylinder carriers")?;
        carriers.push(span);
    }
    Ok(blind_extrusion_from_carriers(&carriers, &planes, transform))
}

pub(in super::super) fn bounded_cylinder_span(
    ctx: &DecodeContext<'_>,
    frame: crate::surface::PositionalCylinderFrame,
    planes: &[([f64; 3], [f64; 3])],
) -> Result<Option<ExtrusionCarrierSpan>, CodecError> {
    let axis = unit_length(*frame.frame().orthonormal_frame().axis());
    let vector = (|| -> Option<[f64; 3]> {
        let vector = match frame.length() {
            Some(length) => axis.map(|component| component * length.get()),
            None => {
                let scale = planes
                    .iter()
                    .flat_map(|(origin, _)| *origin)
                    .chain(frame.frame().origin())
                    .map(f64::abs)
                    .fold(1.0, f64::max);
                let tolerance = EPS_SWEEP_EXTENT_GEOMETRY * scale;
                let start_station = dot(frame.frame().origin(), axis);
                let mut terminal_offset: Option<f64> = None;
                for (origin, normal) in planes {
                    let normal = normalize(*normal)?;
                    let alignment = dot(normal, axis).abs();
                    if alignment >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE {
                        let offset = dot(*origin, axis) - start_station;
                        if offset.abs() > tolerance {
                            if let Some(existing) = terminal_offset {
                                if (offset - existing).abs() > tolerance {
                                    return None;
                                }
                            } else {
                                terminal_offset = Some(offset);
                            }
                        }
                    } else if alignment > EPS_SWEEP_EXTENT_DEGENERATE {
                        return None;
                    }
                }
                let offset = terminal_offset?;
                axis.map(|component| component * offset)
            }
        };
        Some(vector)
    })();
    let Some(vector) = vector else {
        return Ok(None);
    };
    let mut starts = Vec::new();
    ctx.reserve_vec(&mut starts, 1, "creo bounded cylinder starts")?;
    starts.push(frame.frame().origin());
    Ok(Some(ExtrusionCarrierSpan { starts, vector }))
}

fn nurbs_translation_candidate(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    along_v: bool,
) -> Result<Option<ExtrusionCarrierSpan>, CodecError> {
    let (degree, count, knots, periodic) = if along_v {
        (
            nurbs.v_degree(),
            nurbs.v_count(),
            nurbs.v_knots(),
            nurbs.v_periodic(),
        )
    } else {
        (
            nurbs.u_degree(),
            nurbs.u_count(),
            nurbs.u_knots(),
            nurbs.u_periodic(),
        )
    };
    let [first, second, third, fourth] = knots.as_slice() else {
        return Ok(None);
    };
    if !(degree == 1
        && count == 2
        && !periodic
        && first == second
        && third == fourth
        && first < third)
    {
        return Ok(None);
    }
    let u_count = nurbs.u_count();
    let v_count = nurbs.v_count();
    let pair_count = if along_v { u_count } else { v_count };
    let mut starts = Vec::new();
    ctx.reserve_vec(&mut starts, pair_count, "creo NURBS translation starts")?;
    let mut vector: Option<[f64; 3]> = None;
    for index in 0..pair_count {
        let (start_index, end_index) = if along_v {
            ((index, 0), (index, 1))
        } else {
            ((0, index), (1, index))
        };
        let Some(start) = nurbs.pole(start_index.0, start_index.1) else {
            return Ok(None);
        };
        let Some(end) = nurbs.pole(end_index.0, end_index.1) else {
            return Ok(None);
        };
        let start = [start.x, start.y, start.z];
        let end = [end.x, end.y, end.z];
        if let Some(start_weight) = nurbs.weight(start_index.0, start_index.1) {
            let start_weight = start_weight.get();
            let Some(end_weight) = nurbs.weight(end_index.0, end_index.1) else {
                return Ok(None);
            };
            let end_weight = end_weight.get();
            if (start_weight - end_weight).abs()
                > EPS_SWEEP_EXTENT_DEGENERATE * start_weight.abs().max(end_weight.abs()).max(1.0)
            {
                return Ok(None);
            }
        }
        let candidate = std::array::from_fn(|axis| end[axis] - start[axis]);
        if let Some(reference) = vector {
            let scale = reference
                .into_iter()
                .chain(candidate)
                .map(f64::abs)
                .fold(1.0, f64::max);
            if !candidate
                .into_iter()
                .zip(reference)
                .all(|(left, right)| (left - right).abs() <= EPS_SWEEP_EXTENT_GEOMETRY * scale)
            {
                return Ok(None);
            }
        } else {
            vector = Some(candidate);
        }
        starts.push(start);
    }
    Ok(vector.map(|vector| ExtrusionCarrierSpan { starts, vector }))
}

pub(in super::super) fn nurbs_translation_span(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
) -> Result<Option<ExtrusionCarrierSpan>, CodecError> {
    let along_v = nurbs_translation_candidate(ctx, nurbs, true)?;
    let along_u = nurbs_translation_candidate(ctx, nurbs, false)?;
    Ok(match (along_v, along_u) {
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        _ => None,
    })
}

pub(in super::super) fn generated_nurbs_translation_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    enum TranslationExtentSurface {
        Plane,
        Carrier,
    }
    let mut carriers = Vec::new();
    let mut planes = Vec::new();
    let local_planes = placed_planes(ctx, scan)?;
    let mut saw_row = false;
    for row in scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
    {
        saw_row = true;
        let kind = match row.kind {
            crate::surface::SurfaceKind::Plane => TranslationExtentSurface::Plane,
            crate::surface::SurfaceKind::Extrusion(_) => TranslationExtentSurface::Carrier,
            _ => return Ok(None),
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) != Some(row) {
            return Ok(None);
        }
        let Some(source_geometry) =
            unique_source_surface_geometry(ctx, ir, source_carriers, row.id)?
        else {
            return Ok(None);
        };
        match kind {
            TranslationExtentSurface::Plane => {
                let plane = match source_geometry {
                    SourceSurfaceGeometry::Missing
                    | SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                        SolvedSurfaceGeometry::Unknown { .. },
                    )) => local_planes.get(&row.id).copied(),
                    SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                        SolvedSurfaceGeometry::Plane(_),
                    )) => Some(
                        match reconciled_model_plane(
                            ctx,
                            &local_planes,
                            ir,
                            source_carriers,
                            row.id,
                        )? {
                            Some(plane) => plane,
                            None => return Ok(None),
                        },
                    ),
                    SourceSurfaceGeometry::Present(_) => return Ok(None),
                };
                if let Some(plane) = plane {
                    ctx.reserve_vec(&mut planes, 1, "creo NURBS translation cap planes")?;
                    planes.push((plane.origin, plane.normal));
                }
            }
            TranslationExtentSurface::Carrier => match source_geometry {
                SourceSurfaceGeometry::Missing => {}
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Nurbs(nurbs),
                )) => {
                    let Some(span) = nurbs_translation_span(ctx, nurbs)? else {
                        return Ok(None);
                    };
                    ctx.reserve_vec(&mut carriers, 1, "creo NURBS translation carriers")?;
                    carriers.push(span);
                }
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { .. },
                )) => {}
                SourceSurfaceGeometry::Present(_) => return Ok(None),
            },
        }
    }
    if !saw_row {
        return Ok(None);
    }
    Ok(blind_extrusion_from_carriers(&carriers, &planes, transform))
}

struct RectilinearPlaneStation {
    pub(in super::super) coordinate: FiniteReal,
    pub(in super::super) reversed: bool,
}

struct RectilinearPlaneFamily {
    pub(in super::super) normal: [f64; 3],
    pub(in super::super) stations: Vec<RectilinearPlaneStation>,
}

#[derive(Clone, Copy)]
enum SectionPlaneEvidence {
    Missing,
    Ambiguous,
    Resolved(PlaneEquation),
}

fn normalized_plane(normal: [f64; 3], distance: f64) -> Option<PlaneEquation> {
    let magnitude = dot(normal, normal).sqrt();
    (magnitude.is_finite() && magnitude > 0.0 && distance.is_finite()).then_some(())?;
    let normal = normal.map(|component| component / magnitude);
    let distance = distance / magnitude;
    Some(PlaneEquation {
        origin: normal.map(|component| component * distance),
        normal,
    })
}

fn unit_plane(normal: cadmpeg_ir::units::UnitVector3, origin: [f64; 3]) -> Option<PlaneEquation> {
    let normal: [f64; 3] = cadmpeg_ir::math::Vector3::from(normal).into();
    let distance = dot(normal, origin);
    distance.is_finite().then_some(PlaneEquation {
        origin: normal.map(|component| component * distance),
        normal,
    })
}

fn section_plane_evidence(scan: &ContainerScan, id: u32) -> SectionPlaneEvidence {
    let mut datums = scan.planes.datums.iter().filter(|datum| datum.id == id);
    let datum = datums.next();
    let duplicate_datums = datums.next().is_some();
    let mut model_planes = scan
        .planes
        .local_systems
        .iter()
        .filter(|plane| plane.surface_id == id);
    let model_plane = model_planes.next();
    let duplicate_model_planes = model_planes.next().is_some();
    let model_equation = if duplicate_model_planes {
        None
    } else {
        model_plane.and_then(|plane| {
            let frame = plane.frame();
            frame
                .normal
                .zip(frame.origin)
                .and_then(|(normal, origin)| unit_plane(normal, origin))
        })
    };
    let has_outline = scan
        .planes
        .outlines
        .iter()
        .any(|plane| plane.surface_id == id);
    let (outline_equation, duplicate_outline_planes) = if has_outline {
        let mut planes = scan
            .planes
            .outlines
            .iter()
            .filter(|plane| plane.surface_id == id);
        let first = planes.next();
        let duplicate = planes.next().is_some();
        (
            first
                .filter(|_| !duplicate)
                .and_then(|plane| unit_plane(plane.normal, plane.origin)),
            duplicate,
        )
    } else {
        let mut planes = scan
            .planes
            .positional_frames
            .iter()
            .filter(|plane| plane.surface_id == id);
        let first = planes.next();
        let duplicate = planes.next().is_some();
        (
            first
                .filter(|_| !duplicate)
                .and_then(|plane| unit_plane(plane.normal, plane.origin)),
            duplicate,
        )
    };

    if duplicate_datums
        || (datum.is_some() && (model_equation.is_some() || outline_equation.is_some()))
    {
        return SectionPlaneEvidence::Ambiguous;
    }
    if let Some(datum) = datum {
        return normalized_plane(datum.plane().normal(), datum.plane().offset()).map_or(
            SectionPlaneEvidence::Ambiguous,
            SectionPlaneEvidence::Resolved,
        );
    }
    if let Some(equation) = model_equation {
        return SectionPlaneEvidence::Resolved(equation);
    }
    if duplicate_model_planes || duplicate_outline_planes {
        return SectionPlaneEvidence::Ambiguous;
    }
    outline_equation.map_or(
        SectionPlaneEvidence::Missing,
        SectionPlaneEvidence::Resolved,
    )
}

fn rectilinear_family_extent(
    family: &RectilinearPlaneFamily,
    start_reversed: bool,
    station_tolerance: f64,
) -> Option<([f64; 3], f64)> {
    let first = family
        .stations
        .iter()
        .min_by(|left, right| left.coordinate.get().total_cmp(&right.coordinate.get()))?;
    let last = family
        .stations
        .iter()
        .max_by(|left, right| left.coordinate.get().total_cmp(&right.coordinate.get()))?;
    ((last.coordinate.get() - first.coordinate.get()).abs() > station_tolerance).then_some(())?;
    let (start, end) = if first.reversed == start_reversed && last.reversed != start_reversed {
        (first.coordinate.get(), last.coordinate.get())
    } else if last.reversed == start_reversed && first.reversed != start_reversed {
        (last.coordinate.get(), first.coordinate.get())
    } else {
        return None;
    };
    let signed_length = end - start;
    let direction = if first.reversed == start_reversed {
        family.normal
    } else {
        family.normal.map(|component| -component)
    };
    (signed_length.abs() > station_tolerance).then_some((direction, signed_length.abs()))
}

fn rectilinear_extent_from_section_plane(
    family: &RectilinearPlaneFamily,
    section_origin: [f64; 3],
    section_normal: [f64; 3],
    start_reversed: bool,
    station_tolerance: f64,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let (cap_direction, _) = rectilinear_family_extent(family, start_reversed, station_tolerance)?;
    let section_normal = normalize(section_normal)?;
    (dot(section_normal, family.normal).abs() >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE).then_some(())?;
    let planes = family.stations.iter().map(|station| {
        (
            family
                .normal
                .map(|component| component * station.coordinate.get()),
            family.normal,
        )
    });
    let (extent, direction) =
        extrusion_extent_and_direction(section_origin, section_normal, planes)?;
    if matches!(extent, ExtrudeExtent::OneSided { .. })
        && dot(cap_direction, direction) < 1.0 - EPS_SWEEP_EXTENT_DEGENERATE
    {
        return None;
    }
    Some((extent, direction))
}

pub(in super::super) fn generated_rectilinear_plane_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    section: Option<&crate::feature::definitions::FeatureSection3d>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some(section) = section else {
        return Ok(None);
    };
    let Some(section_plane_id) = section.sketch_plane_entity_id else {
        return Ok(None);
    };
    let Some(sketch_plane_flip) = section.sketch_plane_flip else {
        return Ok(None);
    };
    let plane_flip = match sketch_plane_flip {
        crate::feature::definitions::BinaryFlag::Clear => false,
        crate::feature::definitions::BinaryFlag::Set => true,
    };
    let Some(orientation_flip) = section.orientation.section_flip else {
        return Ok(None);
    };
    let section_flip = match orientation_flip {
        crate::feature::definitions::BinaryFlag::Clear => false,
        crate::feature::definitions::BinaryFlag::Set => true,
    };
    let start_reversed = plane_flip ^ section_flip;
    let rows = || {
        scan.surfaces
            .rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
    };
    if ctx
        .admit_iter(&scan.surfaces.rows, "creo rectilinear source row count")?
        .filter(|row| row.feature_id == feature_id)
        .count()
        < 4
        || !rows().all(|row| row.kind == crate::surface::SurfaceKind::Plane)
    {
        return Ok(None);
    }

    let local_planes = placed_planes(ctx, scan)?;
    let mut planes = Vec::new();
    for row in rows() {
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) != Some(row) {
            return Ok(None);
        }
        let Some(SourceSurfaceGeometry::Present(source_geometry)) =
            unique_source_surface_geometry(ctx, ir, source_carriers, row.id)?
        else {
            return Ok(None);
        };
        let plane = match source_geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
                local_planes.get(&row.id).copied()
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
                let Some(plane) =
                    reconciled_model_plane(ctx, &local_planes, ir, source_carriers, row.id)?
                else {
                    return Ok(None);
                };
                Some(plane)
            }
            _ => return Ok(None),
        };
        let Some(plane) = plane else {
            continue;
        };
        let Some(plane) = canonical_plane(PlaneEquation {
            origin: plane.origin,
            normal: plane.normal,
        }) else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut planes, 1, "creo rectilinear cap planes")?;
        planes.push((plane, row.reversed));
    }

    let coordinate_scale = planes
        .iter()
        .flat_map(|(plane, _)| plane.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let station_tolerance = EPS_SWEEP_EXTENT_GEOMETRY * coordinate_scale;
    let mut families: Vec<RectilinearPlaneFamily> = Vec::new();
    for (plane, reversed) in planes {
        let Some(station) = FiniteReal::new(dot(plane.origin, plane.normal)) else {
            return Ok(None);
        };
        if let Some(family) = families.iter_mut().find(|family| {
            family
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= EPS_SWEEP_EXTENT_DEGENERATE)
        }) {
            if let Some(known) = family
                .stations
                .iter()
                .find(|known| (station.get() - known.coordinate.get()).abs() <= station_tolerance)
            {
                if known.reversed != reversed {
                    return Ok(None);
                }
            } else {
                ctx.reserve_vec(&mut family.stations, 1, "creo rectilinear stations")?;
                family.stations.push(RectilinearPlaneStation {
                    coordinate: station,
                    reversed,
                });
            }
        } else {
            if !families
                .iter()
                .all(|family| dot(family.normal, plane.normal).abs() <= EPS_SWEEP_EXTENT_DEGENERATE)
            {
                return Ok(None);
            }
            let mut stations = Vec::new();
            ctx.reserve_vec(&mut stations, 1, "creo rectilinear stations")?;
            stations.push(RectilinearPlaneStation {
                coordinate: station,
                reversed,
            });
            ctx.reserve_vec(&mut families, 1, "creo rectilinear families")?;
            families.push(RectilinearPlaneFamily {
                normal: plane.normal,
                stations,
            });
        }
    }
    if !(families.len() >= 2
        && ctx
            .admit_iter(&families, "creo rectilinear family count")?
            .filter(|family| family.stations.len() >= 2)
            .count()
            >= 2)
    {
        return Ok(None);
    }

    match section_plane_evidence(scan, section_plane_id) {
        SectionPlaneEvidence::Ambiguous => return Ok(None),
        SectionPlaneEvidence::Resolved(section_plane) => {
            let mut section_normal = section_plane.normal;
            if plane_flip {
                section_normal = section_normal.map(|component| -component);
            }
            if section_flip {
                section_normal = section_normal.map(|component| -component);
            }
            let mut axial_families = families.iter().filter(|family| {
                dot(section_normal, family.normal).abs() >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE
            });
            let Some(family) = axial_families.next() else {
                return Ok(None);
            };
            if axial_families.next().is_some() {
                return Ok(None);
            }
            return Ok(rectilinear_extent_from_section_plane(
                family,
                section_plane.origin,
                section_normal,
                start_reversed,
                station_tolerance,
            ));
        }
        SectionPlaneEvidence::Missing => {}
    }

    let mut candidates = families.iter().filter_map(|family| {
        let (direction, length) =
            rectilinear_family_extent(family, start_reversed, station_tolerance)?;
        Some((direction.map(|component| component * length), length))
    });
    let Some((vector, length)) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    let Some(direction) = normalize(vector) else {
        return Ok(None);
    };
    let Some(length) = cadmpeg_ir::scalar::NonZeroLength::new(length) else {
        return Ok(None);
    };
    Ok(Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind { length },
                draft: None,
            },
        },
        direction,
    )))
}

pub(in super::super) fn directed_blind_extrusion_span(
    profile_direction: [f64; 3],
    extrusion_direction: [f64; 3],
    length: f64,
) -> Option<ExtrusionSpan> {
    let profile_direction = normalize(profile_direction)?;
    let extrusion_direction = normalize(extrusion_direction)?;
    let alignment = dot(profile_direction, extrusion_direction);
    (alignment.abs() >= 1.0 - EPS_SWEEP_EXTENT_GEOMETRY).then_some(())?;
    if alignment.is_sign_positive() {
        ExtrusionSpan::new(0.0, length)
    } else {
        ExtrusionSpan::new(-length, 0.0)
    }
}

fn feature_id_for_section_transform(
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<u32> {
    match (definition.identity.owner_feature_id(), transform.feature_id) {
        (Some(definition_feature_id), Some(transform_feature_id))
            if definition_feature_id != transform_feature_id =>
        {
            None
        }
        (Some(feature_id), _) | (_, Some(feature_id)) => Some(feature_id),
        (None, None) => None,
    }
}

fn derived_blind_extrusion_span(
    transform: &crate::placement::FeatureSectionTransform,
    extent: &ExtrudeExtent,
    direction: [f64; 3],
) -> Option<ExtrusionSpan> {
    let ExtrudeExtent::OneSided {
        side:
            ExtrudeSide {
                termination: LinearTermination::Blind { length },
                ..
            },
    } = extent
    else {
        return None;
    };
    directed_blind_extrusion_span(transform.normal(), direction, length.get())
}

pub(in super::super) fn resolved_feature_extrusion_span(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Result<Option<ExtrusionSpan>, CodecError> {
    let Some(feature_id) = feature_id_for_section_transform(definition, transform) else {
        return Ok(None);
    };
    let mut span =
        generated_arc_cylinder_extent(ctx, scan, ir, source_carriers, definition, transform)?
            .and_then(|(extent, direction)| {
                derived_blind_extrusion_span(transform, &extent, direction)
            });
    if span.is_none() {
        span = feature_plane_equations(ctx, scan, ir, source_carriers, feature_id)?.and_then(
            |planes| {
                extrusion_span(
                    transform.origin(),
                    transform.normal(),
                    planes.into_iter().map(|plane| (plane.origin, plane.normal)),
                )
            },
        );
    }
    if span.is_none() {
        span = generated_cap_plane_extent(ctx, scan, ir, source_carriers, feature_id)?.and_then(
            |(extent, direction)| derived_blind_extrusion_span(transform, &extent, direction),
        );
    }
    if span.is_none() {
        span = generated_bounded_cylinder_extent(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            Some(transform),
        )?
        .and_then(|(extent, direction)| {
            derived_blind_extrusion_span(transform, &extent, direction)
        });
    }
    if span.is_none() {
        span = generated_nurbs_translation_extent(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            Some(transform),
        )?
        .and_then(|(extent, direction)| {
            derived_blind_extrusion_span(transform, &extent, direction)
        });
    }
    Ok(span)
}

#[cfg(test)]
mod blind_tests;

#[cfg(test)]
mod rectilinear_tests;
