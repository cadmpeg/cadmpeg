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

pub(super) enum SourceSurfaceGeometry<'a> {
    Missing,
    Present(&'a SurfaceGeometry),
}

pub(super) type SourceSurfaceIndex<'a> = std::collections::HashMap<u32, Option<&'a SurfaceGeometry>>;

pub(super) fn source_surface_geometries<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    source_carriers: &'a crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<SourceSurfaceIndex<'a>, CodecError> {
    const PREFIX: &str = "creo:visibgeom:surface#";
    let mut geometries = std::collections::HashMap::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo source surface index scan")? {
        let Some(suffix) = surface.id.as_str().strip_prefix(PREFIX) else { continue; };
        if suffix.len() > 10 { continue; }
        let Ok(id) = suffix.parse::<u32>() else { continue; };
        if !crate::identity::matches_numbered_identity(surface.id.as_str(), PREFIX, id) { continue; }
        ctx.entry_hash_map(&mut geometries, id, "creo source surface index")?
            .and_modify(|geometry| *geometry = None)
            .or_insert(Some(source_carriers.surface_geometry(surface)));
    }
    Ok(geometries)
}

pub(super) fn unique_source_surface_geometry<'a>(
    geometries: &SourceSurfaceIndex<'a>,
    surface_id: u32,
) -> Option<SourceSurfaceGeometry<'a>> {
    match geometries.get(&surface_id) {
        Some(Some(geometry)) => Some(SourceSurfaceGeometry::Present(geometry)),
        Some(None) => None,
        None => Some(SourceSurfaceGeometry::Missing),
    }
}

fn blind_extrusion_from_carriers(
    ctx: &DecodeContext<'_>,
    carriers: &[ExtrusionCarrierSpan],
    planes: &[([f64; 3], [f64; 3])],
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some(first) = carriers.first() else { return Ok(None); };
    let Some(&first_start) = first.starts.first() else { return Ok(None); };
    let Some(direction) = normalize(first.vector) else { return Ok(None); };
    let length = first.vector.into_iter().fold(0.0_f64, f64::hypot);
    if !length.is_finite() || length <= 0.0 { return Ok(None); }
    let mut coordinate_scale = length.max(1.0);
    for carrier in ctx.admit_iter(carriers, "creo carrier coordinate scale")? {
        for start in ctx.admit_iter(&carrier.starts, "creo carrier start coordinate scale")? {
            for coordinate in start { coordinate_scale = coordinate_scale.max(coordinate.abs()); }
        }
    }
    for (origin, _) in ctx.admit_iter(planes, "creo carrier plane coordinate scale")? {
        for coordinate in origin { coordinate_scale = coordinate_scale.max(coordinate.abs()); }
    }
    if let Some(transform) = transform { for coordinate in transform.origin() { coordinate_scale = coordinate_scale.max(coordinate.abs()); } }
    if !coordinate_scale.is_finite() {
        return Ok(None);
    }
    let tolerance = EPS_SWEEP_EXTENT_GEOMETRY * coordinate_scale;
    let vector_tolerance = EPS_SWEEP_EXTENT_GEOMETRY * length.max(1.0);
    let start_station = dot(first_start, direction);
    let end_station = start_station + length;
    let mut has_opposed_carrier = false;
    if !ctx.all_by(carriers, |carrier| {
            if carrier.starts.is_empty() {
                return Ok(false);
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
            Ok((same_direction && ctx.all_by(&carrier.starts, |start| Ok((dot(*start, direction) - start_station).abs() <= tolerance), "creo carrier start station agreement")?)
                || (opposite_direction && ctx.all_by(&carrier.starts, |start| Ok((dot(*start, direction) - end_station).abs() <= tolerance), "creo carrier end station agreement")?))
        }, "creo carrier direction agreement")? { return Ok(None); }
    let mut unique_stations = [0.0; 2];
    let mut station_count = 0;
    let mut planes = planes.iter();
    while let Some((origin, normal)) = ctx.next_charged(&mut planes, "creo carrier plane station scan")? {
        let Some(normal) = normalize(*normal) else { return Ok(None); };
        let alignment = dot(normal, direction).abs();
        if alignment >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE {
            let station = dot(*origin, direction);
            if unique_stations[..station_count]
                .iter()
                .all(|existing| (station - existing).abs() > tolerance)
            {
                if station_count == unique_stations.len() {
                    return Ok(None);
                }
                unique_stations[station_count] = station;
                station_count += 1;
            }
        } else if alignment > EPS_SWEEP_EXTENT_DEGENERATE || !alignment.is_finite() {
            return Ok(None);
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
                return Ok(None);
            }
        } else {
            let [terminal_station] = &unique_stations[..station_count] else {
                return Ok(None);
            };
            if (*terminal_station - end_station).abs() <= tolerance {
                false
            } else if (*terminal_station - start_station).abs() <= tolerance {
                true
            } else {
                return Ok(None);
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
        if !((dot(direction, normal).abs() - 1.0).abs() <= EPS_SWEEP_EXTENT_DEGENERATE
            && (dot(transform.origin(), direction) - start_station).abs() <= tolerance) { return Ok(None); }
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
        _ => return Ok(None),
    }
    Ok(Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: match cadmpeg_ir::scalar::NonZeroLength::new(length) { Some(length) => length, None => return Ok(None) },
                },
                draft: None,
            },
        },
        direction,
    )))
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
    fn source_surface_index_borrows_canonical_ids_and_keeps_ambiguity() {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        for id in ["creo:visibgeom:surface#7", "creo:visibgeom:surface#07", "creo:visibgeom:surface#8", "creo:visibgeom:surface#8", "test:foreign:surface#7"] {
            ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                id: cadmpeg_ir::ids::SurfaceId::mint(id).expect("identity grammar"),
                geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            });
        }
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let index = crate::test_support::assert_work_boundaries(&["creo source surface index scan", "creo source surface index"], |ctx| super::source_surface_geometries(ctx, &ir, &carriers));
        assert_eq!(index.len(), 2);
        let geometry = index.get(&7).copied().flatten().expect("unique geometry");
        assert!(std::ptr::eq(geometry, &ir.model.surfaces[0].geometry));
        assert!(index.get(&8).expect("ambiguous geometry").is_none());
        assert!(super::unique_source_surface_geometry(&index, 8).is_none());
        assert!(matches!(super::unique_source_surface_geometry(&index, 99), Some(super::SourceSurfaceGeometry::Missing)));
    }

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
        assert!(crate::decode::with_test_decode_ctx(|ctx| blind_extrusion_from_carriers(ctx, &carriers, &[], None)).expect("service resources").is_none());
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
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo bounded cylinder starts"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        bounded_cylinder_span(&ctx, frame, &[])
    });
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
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo NURBS translation starts"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        nurbs_translation_span(&ctx, &surface)
    });
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
            matches!(bounded_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo bounded cylinder cap planes"), bounded_extent_at_limit)), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder cap planes")
        );
    }

    #[test]
    fn bounded_cylinder_frame_limit_refuses() {
        assert!(
            matches!(bounded_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo bounded cylinder frames"), bounded_extent_at_limit)), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder frames")
        );
    }

    #[test]
    fn bounded_cylinder_carrier_limit_refuses() {
        assert!(
            matches!(bounded_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo bounded cylinder carriers"), bounded_extent_at_limit)), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo bounded cylinder carriers")
        );
        assert!(bounded_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, None, bounded_extent_at_limit))
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
            matches!(nurbs_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo NURBS translation carriers"), nurbs_extent_at_limit)), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo NURBS translation carriers")
        );
    }

    #[test]
    fn nurbs_translation_cap_plane_limit_refuses() {
        assert!(
            matches!(nurbs_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo NURBS translation cap planes"), nurbs_extent_at_limit)), Err(CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo NURBS translation cap planes")
        );
        assert!(nurbs_extent_at_limit(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, None, nurbs_extent_at_limit)).expect("admitted extent").is_some());
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
    let mut scratch = ctx.reserve_scoped(0, "creo generated extent scratch")?;
    let mut source_geometries = None;
    enum CylinderExtentSurface {
        Plane,
        Carrier,
    }
    let (local_planes, _local_plane_storage) = ctx.with_scoped_storage("creo extent local plane scratch", || placed_planes(ctx, scan))?;
    let mut frames = Vec::new();
    let mut planes = Vec::new();
    let mut saw_row = false;
    let mut rows = scan.surfaces.rows.iter();
    while let Some(row) = ctx.next_charged(&mut rows, "creo generated extent surface row scan")? {
        if row.feature_id != feature_id { continue; }
        saw_row = true;
        let kind = match row.kind {
            crate::surface::SurfaceKind::Plane => CylinderExtentSurface::Plane,
            crate::surface::SurfaceKind::Cylinder => CylinderExtentSurface::Carrier,
            _ => return Ok(None),
        };
        if !scan.surfaces.rows.unique(row.id).is_some_and(|unique| std::ptr::eq(unique, row)) {
            return Ok(None);
        }
        if source_geometries.is_none() {
            source_geometries = Some(scratch.with_storage(|| source_surface_geometries(ctx, ir, source_carriers))?);
        }
        let Some(geometries) = &source_geometries else { return Ok(None); };
        let Some(source_geometry) = unique_source_surface_geometry(geometries, row.id)
        else {
            return Ok(None);
        };
        match kind {
            CylinderExtentSurface::Plane => match source_geometry {
                SourceSurfaceGeometry::Missing => {
                    if let Some(plane) = ctx.get_btree_map(&local_planes, &row.id, "creo generated extent local plane lookup")? {
                        scratch.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes"))?;
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
                    scratch.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes"))?;
                    planes.push((plane.origin, plane.normal));
                }
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { .. },
                )) => {
                    if let Some(plane) = ctx.get_btree_map(&local_planes, &row.id, "creo generated extent local plane lookup")? {
                        scratch.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo bounded cylinder cap planes"))?;
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
                        crate::decode::uniqueness::exactly_one_by(ctx, &scan.surfaces.parameters, |record| Ok(record.surface_id == row.id), "creo generated extent cylinder parameter search")?
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
                    scratch.with_storage(|| ctx.reserve_vec(&mut frames, 1, "creo bounded cylinder frames"))?;
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
    let mut frames = frames.into_iter();
    while let Some(frame) = ctx.next_charged(&mut frames, "creo bounded cylinder frame scan")? {
        let Some(span) = scratch.with_storage(|| bounded_cylinder_span(ctx, frame, &planes))? else {
            return Ok(None);
        };
        scratch.with_storage(|| ctx.reserve_vec(&mut carriers, 1, "creo bounded cylinder carriers"))?;
        carriers.push(span);
    }
    blind_extrusion_from_carriers(ctx, &carriers, &planes, transform)
}

pub(in super::super) fn bounded_cylinder_span(
    ctx: &DecodeContext<'_>,
    frame: crate::surface::PositionalCylinderFrame,
    planes: &[([f64; 3], [f64; 3])],
) -> Result<Option<ExtrusionCarrierSpan>, CodecError> {
    let axis = unit_length(*frame.frame().orthonormal_frame().axis());
    let vector_for = || -> Result<Option<[f64; 3]>, CodecError> {
        let vector = match frame.length() {
            Some(length) => axis.map(|component| component * length.get()),
            None => {
                let scale = ctx.admit_iter(planes, "creo bounded cylinder plane scale")?
                    .flat_map(|(origin, _)| *origin)
                    .chain(frame.frame().origin())
                    .map(f64::abs)
                    .fold(1.0, f64::max);
                let tolerance = EPS_SWEEP_EXTENT_GEOMETRY * scale;
                let start_station = dot(frame.frame().origin(), axis);
                let mut terminal_offset: Option<f64> = None;
                let mut planes = planes.iter();
                while let Some((origin, normal)) = ctx.next_charged(&mut planes, "creo bounded cylinder terminal plane scan")? {
                    let Some(normal) = normalize(*normal) else { return Ok(None); };
                    let alignment = dot(normal, axis).abs();
                    if alignment >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE {
                        let offset = dot(*origin, axis) - start_station;
                        if offset.abs() > tolerance {
                            if let Some(existing) = terminal_offset {
                                if (offset - existing).abs() > tolerance {
                                    return Ok(None);
                                }
                            } else {
                                terminal_offset = Some(offset);
                            }
                        }
                    } else if alignment > EPS_SWEEP_EXTENT_DEGENERATE {
                        return Ok(None);
                    }
                }
                let Some(offset) = terminal_offset else { return Ok(None); };
                axis.map(|component| component * offset)
            }
        };
        Ok(Some(vector))
    };
    let vector = vector_for()?;
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
    let mut pairs = 0..pair_count;
    while let Some(index) = ctx.next_charged(&mut pairs, "creo NURBS translation pole pair scan")? {
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
    let mut scratch = ctx.reserve_scoped(0, "creo generated extent scratch")?;
    let mut source_geometries = None;
    enum TranslationExtentSurface {
        Plane,
        Carrier,
    }
    let mut carriers = Vec::new();
    let mut planes = Vec::new();
    let (local_planes, _local_plane_storage) = ctx.with_scoped_storage("creo extent local plane scratch", || placed_planes(ctx, scan))?;
    let mut saw_row = false;
    let mut rows = scan.surfaces.rows.iter();
    while let Some(row) = ctx.next_charged(&mut rows, "creo generated extent surface row scan")? {
        if row.feature_id != feature_id { continue; }
        saw_row = true;
        let kind = match row.kind {
            crate::surface::SurfaceKind::Plane => TranslationExtentSurface::Plane,
            crate::surface::SurfaceKind::Extrusion(_) => TranslationExtentSurface::Carrier,
            _ => return Ok(None),
        };
        if !scan.surfaces.rows.unique(row.id).is_some_and(|unique| std::ptr::eq(unique, row)) {
            return Ok(None);
        }
        if source_geometries.is_none() {
            source_geometries = Some(scratch.with_storage(|| source_surface_geometries(ctx, ir, source_carriers))?);
        }
        let Some(geometries) = &source_geometries else { return Ok(None); };
        let Some(source_geometry) = unique_source_surface_geometry(geometries, row.id)
        else {
            return Ok(None);
        };
        match kind {
            TranslationExtentSurface::Plane => {
                let plane = match source_geometry {
                    SourceSurfaceGeometry::Missing
                    | SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                        SolvedSurfaceGeometry::Unknown { .. },
                    )) => ctx.get_btree_map(&local_planes, &row.id, "creo generated extent local plane lookup")?.copied(),
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
                    scratch.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo NURBS translation cap planes"))?;
                    planes.push((plane.origin, plane.normal));
                }
            }
            TranslationExtentSurface::Carrier => match source_geometry {
                SourceSurfaceGeometry::Missing => {}
                SourceSurfaceGeometry::Present(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Nurbs(nurbs),
                )) => {
                    let Some(span) = scratch.with_storage(|| nurbs_translation_span(ctx, nurbs))? else {
                        return Ok(None);
                    };
                    scratch.with_storage(|| ctx.reserve_vec(&mut carriers, 1, "creo NURBS translation carriers"))?;
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
    blind_extrusion_from_carriers(ctx, &carriers, &planes, transform)
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

fn section_plane_evidence(ctx: &DecodeContext<'_>, scan: &ContainerScan, id: u32) -> Result<SectionPlaneEvidence, CodecError> {
    let datum_index = ctx.position_by(&scan.planes.datums, |datum| Ok(datum.id == id), "creo section datum plane search")?;
    let datum = datum_index.map(|index| &scan.planes.datums[index]);
    let duplicate_datums = match datum_index { Some(index) => ctx.any_by(&scan.planes.datums[index + 1..], |datum| Ok(datum.id == id), "creo section datum plane search")?, None => false };
    let model_index = ctx.position_by(&scan.planes.local_systems, |plane| Ok(plane.surface_id == id), "creo section local plane search")?;
    let model_plane = model_index.map(|index| &scan.planes.local_systems[index]);
    let duplicate_model_planes = match model_index { Some(index) => ctx.any_by(&scan.planes.local_systems[index + 1..], |plane| Ok(plane.surface_id == id), "creo section local plane search")?, None => false };
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
    let outline_index = ctx.position_by(&scan.planes.outlines, |plane| Ok(plane.surface_id == id), "creo section outline plane search")?;
    let (outline_equation, duplicate_outline_planes) = if let Some(index) = outline_index {
        let duplicate = ctx.any_by(&scan.planes.outlines[index + 1..], |plane| Ok(plane.surface_id == id), "creo section outline plane search")?;
        let plane = &scan.planes.outlines[index];
        ((!duplicate).then(|| unit_plane(plane.normal, plane.origin)).flatten(), duplicate)
    } else {
        let index = ctx.position_by(&scan.planes.positional_frames, |plane| Ok(plane.surface_id == id), "creo section positional plane search")?;
        let duplicate = match index { Some(index) => ctx.any_by(&scan.planes.positional_frames[index + 1..], |plane| Ok(plane.surface_id == id), "creo section positional plane search")?, None => false };
        (index.filter(|_| !duplicate).and_then(|index| { let plane = &scan.planes.positional_frames[index]; unit_plane(plane.normal, plane.origin) }), duplicate)
    };

    if duplicate_datums
        || (datum.is_some() && (model_equation.is_some() || outline_equation.is_some()))
    {
        return Ok(SectionPlaneEvidence::Ambiguous);
    }
    if let Some(datum) = datum {
        return Ok(normalized_plane(datum.plane().normal(), datum.plane().offset()).map_or(
            SectionPlaneEvidence::Ambiguous,
            SectionPlaneEvidence::Resolved,
        ));
    }
    if let Some(equation) = model_equation {
        return Ok(SectionPlaneEvidence::Resolved(equation));
    }
    if duplicate_model_planes || duplicate_outline_planes {
        return Ok(SectionPlaneEvidence::Ambiguous);
    }
    Ok(outline_equation.map_or(
        SectionPlaneEvidence::Missing,
        SectionPlaneEvidence::Resolved,
    ))
}

fn rectilinear_family_extent(
    ctx: &DecodeContext<'_>,
    family: &RectilinearPlaneFamily,
    start_reversed: bool,
    station_tolerance: f64,
) -> Result<Option<([f64; 3], f64)>, CodecError> {
    let Some((initial, rest)) = family.stations.split_first() else { return Ok(None); };
    let mut first = initial;
    let mut last = initial;
    for station in ctx.admit_iter(rest, "creo rectilinear extreme station scan")? {
        if station.coordinate.get().total_cmp(&first.coordinate.get()).is_lt() { first = station; }
        if !station.coordinate.get().total_cmp(&last.coordinate.get()).is_lt() { last = station; }
    }
    if (last.coordinate.get() - first.coordinate.get()).abs() <= station_tolerance { return Ok(None); }
    let (start, end) = if first.reversed == start_reversed && last.reversed != start_reversed {
        (first.coordinate.get(), last.coordinate.get())
    } else if last.reversed == start_reversed && first.reversed != start_reversed {
        (last.coordinate.get(), first.coordinate.get())
    } else {
        return Ok(None);
    };
    let signed_length = end - start;
    let direction = if first.reversed == start_reversed {
        family.normal
    } else {
        family.normal.map(|component| -component)
    };
    Ok((signed_length.abs() > station_tolerance).then_some((direction, signed_length.abs())))
}

fn rectilinear_extent_from_section_plane(
    ctx: &DecodeContext<'_>,
    family: &RectilinearPlaneFamily,
    section_origin: [f64; 3],
    section_normal: [f64; 3],
    start_reversed: bool,
    station_tolerance: f64,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some((cap_direction, _)) = rectilinear_family_extent(ctx, family, start_reversed, station_tolerance)? else { return Ok(None); };
    let Some(section_normal) = normalize(section_normal) else { return Ok(None); };
    if dot(section_normal, family.normal).abs() < 1.0 - EPS_SWEEP_EXTENT_DEGENERATE { return Ok(None); }
    let planes = family.stations.iter().map(|station| {
        (
            family
                .normal
                .map(|component| component * station.coordinate.get()),
            family.normal,
        )
    });
    let Some((extent, direction)) = extrusion_extent_and_direction(ctx, section_origin, section_normal, planes)? else { return Ok(None); };
    if matches!(extent, ExtrudeExtent::OneSided { .. })
        && dot(cap_direction, direction) < 1.0 - EPS_SWEEP_EXTENT_DEGENERATE
    {
        return Ok(None);
    }
    Ok(Some((extent, direction)))
}

pub(in super::super) fn generated_rectilinear_plane_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    section: Option<&crate::feature::definitions::FeatureSection3d>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo generated extent scratch")?;
    let mut source_geometries = None;
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
    let (mut rows, mut row_storage) = ctx.temporary_vec(0, "creo rectilinear source rows")?;
    let mut source_rows = scan.surfaces.rows.iter();
    while let Some(row) = ctx.next_charged(&mut source_rows, "creo rectilinear source row scan")? {
        if row.feature_id != feature_id { continue; }
        if row.kind != crate::surface::SurfaceKind::Plane { return Ok(None); }
        ctx.push_scoped_vec(&mut row_storage, &mut rows, row, "creo rectilinear source rows")?;
    }
    if rows.len() < 4 { return Ok(None); }

    let (local_planes, _local_plane_storage) = ctx.with_scoped_storage("creo extent local plane scratch", || placed_planes(ctx, scan))?;
    let mut planes = Vec::new();
    let mut source_rows = rows.into_iter();
    while let Some(row) = ctx.next_charged(&mut source_rows, "creo rectilinear source plane scan")? {
        if !scan.surfaces.rows.unique(row.id).is_some_and(|unique| std::ptr::eq(unique, row)) {
            return Ok(None);
        }
        if source_geometries.is_none() {
            source_geometries = Some(scratch.with_storage(|| source_surface_geometries(ctx, ir, source_carriers))?);
        }
        let Some(geometries) = &source_geometries else { return Ok(None); };
        let Some(SourceSurfaceGeometry::Present(source_geometry)) = unique_source_surface_geometry(geometries, row.id)
        else {
            return Ok(None);
        };
        let plane = match source_geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
                ctx.get_btree_map(&local_planes, &row.id, "creo generated extent local plane lookup")?.copied()
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
        scratch.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo rectilinear cap planes"))?;
        planes.push((plane, row.reversed));
    }

    let coordinate_scale = ctx.admit_iter(&planes, "creo rectilinear plane coordinate scale")?
        .flat_map(|(plane, _)| plane.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let station_tolerance = EPS_SWEEP_EXTENT_GEOMETRY * coordinate_scale;
    let mut families: Vec<RectilinearPlaneFamily> = Vec::new();
    let mut plane_rows = planes.into_iter();
    while let Some((plane, reversed)) = ctx.next_charged(&mut plane_rows, "creo rectilinear family plane scan")? {
        let Some(station) = FiniteReal::new(dot(plane.origin, plane.normal)) else {
            return Ok(None);
        };
        if let Some(family) = ctx.find_by(families.iter_mut(), |family| {
            Ok(
            family
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= EPS_SWEEP_EXTENT_DEGENERATE))
        }, "creo rectilinear matching family search")? {
            if let Some(known) = ctx.find_by(&family.stations, |known| Ok((station.get() - known.coordinate.get()).abs() <= station_tolerance), "creo rectilinear matching station search")?
            {
                if known.reversed != reversed {
                    return Ok(None);
                }
            } else {
                scratch.with_storage(|| ctx.reserve_vec(&mut family.stations, 1, "creo rectilinear stations"))?;
                family.stations.push(RectilinearPlaneStation {
                    coordinate: station,
                    reversed,
                });
            }
        } else {
            if !ctx.all_by(&families, |family| Ok(dot(family.normal, plane.normal).abs() <= EPS_SWEEP_EXTENT_DEGENERATE), "creo rectilinear family orthogonality")?
            {
                return Ok(None);
            }
            let mut stations = Vec::new();
            scratch.with_storage(|| ctx.reserve_vec(&mut stations, 1, "creo rectilinear stations"))?;
            stations.push(RectilinearPlaneStation {
                coordinate: station,
                reversed,
            });
            scratch.with_storage(|| ctx.reserve_vec(&mut families, 1, "creo rectilinear families"))?;
            families.push(RectilinearPlaneFamily {
                normal: plane.normal,
                stations,
            });
        }
    }
    let mut family_count = 0;
    let mut candidates = families.iter();
    while family_count < 2 {
        let Some(family) = ctx.next_charged(&mut candidates, "creo rectilinear family count")? else { return Ok(None); };
        if family.stations.len() >= 2 { family_count += 1; }
    }

    match section_plane_evidence(ctx, scan, section_plane_id)? {
        SectionPlaneEvidence::Ambiguous => return Ok(None),
        SectionPlaneEvidence::Resolved(section_plane) => {
            let mut section_normal = section_plane.normal;
            if plane_flip {
                section_normal = section_normal.map(|component| -component);
            }
            if section_flip {
                section_normal = section_normal.map(|component| -component);
            }
            let Some(family) = crate::decode::uniqueness::exactly_one_by(ctx, &families,
                |family| Ok(dot(section_normal, family.normal).abs() >= 1.0 - EPS_SWEEP_EXTENT_DEGENERATE), "creo rectilinear axial family search")? else { return Ok(None); };
            return rectilinear_extent_from_section_plane(ctx, family, section_plane.origin, section_normal, start_reversed, station_tolerance);
        }
        SectionPlaneEvidence::Missing => {}
    }

    let mut families = families.iter();
    let candidate = |family: &RectilinearPlaneFamily| -> Result<Option<([f64; 3], f64)>, CodecError> {
        Ok(rectilinear_family_extent(ctx, family, start_reversed, station_tolerance)?.map(|(direction, length)| (direction.map(|component| component * length), length)))
    };
    let Some((vector, length)) = ctx.find_map(&mut families, candidate, "creo rectilinear extent candidate search")? else { return Ok(None); };
    if ctx.find_map(&mut families, candidate, "creo rectilinear extent candidate search")?.is_some() { return Ok(None); }
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
        if let Some((planes, _plane_storage)) = feature_plane_equations(ctx, scan, ir, source_carriers, feature_id)? {
            span = extrusion_span(ctx, transform.origin(), transform.normal(), planes.into_iter().map(|plane| (plane.origin, plane.normal)))?;
        }
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
