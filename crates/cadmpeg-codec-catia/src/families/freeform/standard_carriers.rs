// SPDX-License-Identifier: Apache-2.0
//! Direct standard-face carrier decoding through the consolidated allocation journal.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::AnnotationBuilder;

use crate::checked::ExactUnitVector3;
use crate::families::a5a8::records::FreeformSurface;
use crate::families::b2::records::{
    b2_cone_geometry, b2_cones_from_records, b2_cylinders_from_records,
    b2_nurbs_curves_from_records, b2_offset_supports_from_records,
    b2_parameter_points_from_records, b2_plane_carriers_from_records, b2_plane_geometry,
    b2_sphere_geometry, b2_spheres_from_records, b2_tori_from_records, b2_torus_geometry,
};
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::standard::records::{
    standard_face_bounds, AnalyticSurfaceKind, StandardSurfaceRecord,
};
use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

use super::rolling_ball_patch::fit_edge_with_error;
use super::weighted_plane::lift_plane_pcurve;
use super::{consolidated_jet_pcurve, ConsolidatedCarrierChart};

const PLANE_BOUNDS_TOLERANCE_MM: f64 = 0.05;
const ZERO_EXTENT_TOLERANCE_MM: f64 = 1.0e-5;
const NONZERO_EXTENT_MIN_MM: f64 = 1.0e-3;
const DIRECTRIX_FIT_TOLERANCE_MM: f64 = 0.0005;
const MAX_ALIAS_DEPTH: usize = 3;
const MAX_BSPLINE_PCURVE_KNOTS: usize = 4096;

/// Recover missing plane normals for standard `00 33 32` plane faces whose
/// trim packets omit explicit `0x08` normal frames.
///
/// Prefers the consolidated `b2 03 27` carrier directly addressed by the face's
/// `b2 03 5f` allocation-journal node (verified against the face's bounds
/// center), and falls back to strictly axis-aligned zero-thickness bounds when
/// no consolidated carrier exists.
#[inline(never)]
pub(in crate::families) fn recover_missing_plane_normals(
    data: &[u8],
    records: &[ConsolidatedRecord],
    brep: &[u8],
    surface_records: &[StandardSurfaceRecord],
    normals: &mut HashMap<u32, FiniteVector<3>>,
) {
    let journal = AllocationJournal::new(records);
    let face_carriers = unique_5f_carriers(data, records, &journal);
    let b2_planes: HashMap<_, _> = b2_plane_carriers_from_records(data, records)
        .into_iter()
        .map(|carrier| (carrier.pos, carrier))
        .collect();
    let b2_offsets: HashMap<_, _> = b2_offset_supports_from_records(data, records)
        .into_iter()
        .map(|offset| (offset.pos, offset))
        .collect();

    let mut all_bounds_min = [f64::INFINITY; 3];
    let mut all_bounds_max = [f64::NEG_INFINITY; 3];
    let mut parsed_bounds = Vec::with_capacity(surface_records.len());
    for record in surface_records {
        let bounds = standard_face_bounds(brep, record);
        if let Some(b) = bounds {
            for axis in 0..3 {
                let c = b.aabb_center[axis].get();
                let h = b.aabb_half_extents[axis].get();
                all_bounds_min[axis] = all_bounds_min[axis].min(c - h);
                all_bounds_max[axis] = all_bounds_max[axis].max(c + h);
            }
        }
        parsed_bounds.push(bounds);
    }
    let part_center = [
        0.5 * (all_bounds_min[0] + all_bounds_max[0]),
        0.5 * (all_bounds_min[1] + all_bounds_max[1]),
        0.5 * (all_bounds_min[2] + all_bounds_max[2]),
    ];

    for (index, record) in surface_records.iter().enumerate() {
        let StandardSurfaceRecord::Analytic(prefix) = record else {
            continue;
        };
        if prefix.kind != AnalyticSurfaceKind::Plane || normals.contains_key(&prefix.target) {
            continue;
        }
        let Some(bounds) = parsed_bounds[index] else {
            continue;
        };
        let center = Point3::new(
            bounds.sphere_center[0].get(),
            bounds.sphere_center[1].get(),
            bounds.sphere_center[2].get(),
        );
        if let Some(Some((_, carrier))) = face_carriers.get(&u64::from(prefix.target)) {
            if let Some(plane) =
                resolve_b2_plane(data, &journal, carrier, &b2_planes, &b2_offsets, 0)
            {
                let axis = *plane.frame().axis().as_raw();
                let offset = center.vector_from(plane.origin().get()).dot(axis).abs();
                if offset <= PLANE_BOUNDS_TOLERANCE_MM {
                    if let Some(vector) = FiniteVector::new([axis.x, axis.y, axis.z]) {
                        normals.insert(prefix.target, vector);
                        continue;
                    }
                }
            }
        }
        let extents = bounds.aabb_half_extents.map(|v| v.get());
        let zero_axes: Vec<usize> = (0..3)
            .filter(|&axis| extents[axis] <= ZERO_EXTENT_TOLERANCE_MM)
            .collect();
        if zero_axes.len() == 1 {
            let axis = zero_axes[0];
            let other_valid = (0..3)
                .filter(|&other| other != axis)
                .all(|other| extents[other] >= NONZERO_EXTENT_MIN_MM);
            if other_valid && part_center[axis].is_finite() {
                let sign = if bounds.aabb_center[axis].get() < part_center[axis] {
                    -1.0
                } else {
                    1.0
                };
                let mut normal = [0.0; 3];
                normal[axis] = sign;
                if let Some(vector) = FiniteVector::new(normal) {
                    normals.insert(prefix.target, vector);
                }
            }
        }
    }
}

/// Bind unresolved standard freeform faces directly from their `b2 03 5f`
/// allocation-journal carriers (`a5 03 34` NURBS, `b2 03 2c` extrusions,
/// `b2 03 30`/`31` offsets, and consolidated analytics).
#[inline(never)]
pub(in crate::families) fn bind_journal_standard_surfaces(
    _ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    a5_carriers: &[FreeformSurface],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<usize, CodecError> {
    let journal = AllocationJournal::new(records);
    let face_carriers = unique_5f_carriers(data, records, &journal);
    if face_carriers.is_empty() {
        return Ok(0);
    }

    let a5_by_pos: HashMap<usize, &NurbsSurface> = a5_carriers
        .iter()
        .map(|surface| (surface.pos, &surface.geometry))
        .collect();
    let b2_planes: HashMap<_, _> = b2_plane_carriers_from_records(data, records)
        .into_iter()
        .map(|carrier| (carrier.pos, carrier))
        .collect();
    let b2_cylinders: HashMap<_, _> = b2_cylinders_from_records(data, records)
        .map(|cylinder| (cylinder.pos, cylinder))
        .collect();
    let b2_cones: HashMap<_, _> = b2_cones_from_records(data, records)
        .map(|cone| (cone.pos, cone))
        .collect();
    let b2_spheres: HashMap<_, _> = b2_spheres_from_records(data, records)
        .map(|sphere| (sphere.pos, sphere))
        .collect();
    let b2_tori: HashMap<_, _> = b2_tori_from_records(data, records)
        .map(|torus| (torus.pos, torus))
        .collect();
    let b2_nurbs_curves: HashMap<_, _> = b2_nurbs_curves_from_records(data, records, refusal)
        .into_iter()
        .map(|curve| (curve.pos, curve.geometry))
        .collect();
    let b2_offsets: HashMap<_, _> = b2_offset_supports_from_records(data, records)
        .into_iter()
        .map(|offset| (offset.pos, offset))
        .collect();
    let mut compact_jets = HashMap::new();
    for family in [ConsolidatedFamily::A, ConsolidatedFamily::B] {
        for jet in crate::wire::records::family_pcurves_from_records(data, records, family) {
            compact_jets.insert(jet.pos, jet);
        }
    }

    let resolver = CarrierResolver {
        data,
        journal: &journal,
        a5_by_pos: &a5_by_pos,
        b2_planes: &b2_planes,
        b2_cylinders: &b2_cylinders,
        b2_cones: &b2_cones,
        b2_spheres: &b2_spheres,
        b2_tori: &b2_tori,
        b2_nurbs_curves: &b2_nurbs_curves,
        b2_offsets: &b2_offsets,
        compact_jets: &compact_jets,
    };

    let mut bound = 0usize;
    for surface in &mut ir.geometry.surfaces {
        // Only bind freeform standard faces (`record: None`). Analytic unknown
        // faces carry `record: Some(...)` and are accounted separately.
        if !matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        ) {
            continue;
        }
        let Some(tag) = source_carrier_tag(surface.source_object.as_ref()) else {
            continue;
        };
        let Some(Some((face_index, carrier))) = face_carriers.get(&tag) else {
            continue;
        };
        let Some(solved) = resolver.resolve_surface(*face_index, carrier, refusal, 0) else {
            continue;
        };
        surface.geometry = SurfaceGeometry::Solved(solved);
        annotations.derived(&surface.id, "geometry")?;
        bound += 1;
    }
    Ok(bound)
}

struct CarrierResolver<'a> {
    data: &'a [u8],
    journal: &'a AllocationJournal<'a>,
    a5_by_pos: &'a HashMap<usize, &'a NurbsSurface>,
    b2_planes: &'a HashMap<usize, crate::families::b2::records::B2PlaneCarrier>,
    b2_cylinders: &'a HashMap<usize, crate::families::b2::records::B2Cylinder>,
    b2_cones: &'a HashMap<usize, crate::families::b2::records::B2Cone>,
    b2_spheres: &'a HashMap<usize, crate::families::b2::records::B2Sphere>,
    b2_tori: &'a HashMap<usize, crate::families::b2::records::B2Torus>,
    b2_nurbs_curves: &'a HashMap<usize, NurbsCurve>,
    b2_offsets: &'a HashMap<usize, crate::families::b2::records::B2OffsetSupport>,
    compact_jets: &'a HashMap<usize, crate::wire::records::ConsolidatedPcurve>,
}

impl<'a> CarrierResolver<'a> {
    fn resolve_surface(
        &self,
        _origin_index: usize,
        carrier: &'a ConsolidatedRecord,
        refusal: &mut crate::nurbs::LaneRefusals,
        depth: usize,
    ) -> Option<SolvedSurfaceGeometry> {
        if depth > MAX_ALIAS_DEPTH {
            return None;
        }
        let carrier_index = self
            .journal
            .index(carrier.source_index(), carrier.source_range().start)?;
        match (carrier.family(), carrier.class()) {
            (ConsolidatedFamily::A, 0x34) => self
                .a5_by_pos
                .get(&carrier.byte_offset())
                .map(|surface| SolvedSurfaceGeometry::Nurbs((*surface).clone()))
                .or_else(|| {
                    crate::families::a5a8::records::a5_owned_surfaces_from_records(
                        self.data,
                        std::slice::from_ref(carrier),
                        refusal,
                    )
                    .ok()?
                    .into_iter()
                    .next()
                    .map(|surface| SolvedSurfaceGeometry::Nurbs(surface.geometry))
                }),
            (ConsolidatedFamily::B, 0x27) => decode_b2_plane(self.data, carrier, self.b2_planes)
                .map(SolvedSurfaceGeometry::Plane),
            (ConsolidatedFamily::B, 0x28) => self
                .b2_cylinders
                .get(&carrier.byte_offset())
                .and_then(|cylinder| match cylinder.surface_geometry() {
                    SurfaceGeometry::Solved(solved) => Some(solved),
                    SurfaceGeometry::Procedural { .. } => None,
                }),
            (ConsolidatedFamily::B, 0x29) => {
                self.b2_cones.get(&carrier.byte_offset()).and_then(|cone| {
                    match b2_cone_geometry(cone) {
                        SurfaceGeometry::Solved(solved) => Some(solved),
                        SurfaceGeometry::Procedural { .. } => None,
                    }
                })
            }
            (ConsolidatedFamily::B, 0x2a) => {
                self.b2_spheres
                    .get(&carrier.byte_offset())
                    .and_then(|sphere| match b2_sphere_geometry(sphere) {
                        SurfaceGeometry::Solved(solved) => Some(solved),
                        SurfaceGeometry::Procedural { .. } => None,
                    })
            }
            (ConsolidatedFamily::B, 0x2b) => {
                self.b2_tori.get(&carrier.byte_offset()).and_then(|torus| {
                    match b2_torus_geometry(torus) {
                        SurfaceGeometry::Solved(solved) => Some(solved),
                        SurfaceGeometry::Procedural { .. } => None,
                    }
                })
            }
            (ConsolidatedFamily::B, 0x2c) => self
                .decode_extrusion(carrier_index, carrier, refusal, depth + 1)
                .map(SolvedSurfaceGeometry::Nurbs),
            (ConsolidatedFamily::B, 0x2e) => {
                let payload = carrier.payload().and_then(|range| self.data.get(range))?;
                let mut at = 0;
                if payload.first() == Some(&0x81) {
                    at = 1;
                }
                let inner_ref = allocation_reference(payload, &mut at)?;
                let inner = resolve_ref(self.journal, carrier_index, inner_ref)?;
                self.resolve_surface(carrier_index, inner, refusal, depth + 1)
            }
            (ConsolidatedFamily::B, 0x30 | 0x31) => {
                self.decode_offset(carrier_index, carrier, refusal, depth + 1)
            }
            _ => None,
        }
    }

    fn decode_offset(
        &self,
        carrier_index: usize,
        carrier: &'a ConsolidatedRecord,
        refusal: &mut crate::nurbs::LaneRefusals,
        depth: usize,
    ) -> Option<SolvedSurfaceGeometry> {
        let payload = carrier.payload().and_then(|range| self.data.get(range))?;
        let mut at = 0;
        let base_ref = allocation_reference(payload, &mut at)?;
        let base_record = resolve_ref(self.journal, carrier_index, base_ref)?;
        let (distance, u_range, v_range) =
            if let Some(parsed) = self.b2_offsets.get(&carrier.byte_offset()) {
                (
                    parsed.distance.get(),
                    parsed.u_range.endpoints(),
                    parsed.v_range.endpoints(),
                )
            } else {
                let mut view = View::over_retained(payload.get(at..)?);
                let distance = view.f64_le()?;
                let u0 = view.f64_le()?;
                let v0 = view.f64_le()?;
                let u1 = view.f64_le()?;
                let v1 = view.f64_le()?;
                if !distance.is_finite() || !view.is_empty() {
                    return None;
                }
                let u_bounds = IncreasingParameterInterval::new([u0, u1])?.endpoints();
                let v_bounds = IncreasingParameterInterval::new([v0, v1])?.endpoints();
                (distance, u_bounds, v_bounds)
            };
        let base_geom = self.resolve_surface(carrier_index, base_record, refusal, depth)?;
        match base_geom {
            SolvedSurfaceGeometry::Nurbs(base_nurbs) => {
                let fitted = super::owned_patch::surface_fit::offset(
                    &base_nurbs,
                    distance,
                    [u_range, v_range],
                )?;
                Some(SolvedSurfaceGeometry::Nurbs(fitted))
            }
            other => offset_analytic_surface(&other, distance),
        }
    }

    fn decode_extrusion(
        &self,
        carrier_index: usize,
        carrier: &'a ConsolidatedRecord,
        refusal: &mut crate::nurbs::LaneRefusals,
        depth: usize,
    ) -> Option<NurbsSurface> {
        let payload = carrier.payload().and_then(|range| self.data.get(range))?;
        let mut at = 0;
        let directrix_ref = allocation_reference(payload, &mut at)?;
        let mut view = View::over_retained(payload.get(at..)?);
        let direction = ExactUnitVector3::new([view.f64_le()?, view.f64_le()?, view.f64_le()?])?;
        let direction = UnitVector3::from(direction);
        let u_bounds =
            IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?.endpoints();
        let scale = view.f64_le()?;
        let offset = view.f64_le()?;
        if scale.to_bits() != 1.0_f64.to_bits() || offset.to_bits() != 0.0_f64.to_bits() {
            return None;
        }
        let v_lim0 = view.f64_le()?;
        let v_lim1 = view.f64_le()?;
        if !v_lim0.is_finite() || !v_lim1.is_finite() {
            return None;
        }
        let tail = view.take(view.remaining())?;
        let tail_valid = match tail.len() {
            2 => tail.iter().all(|byte| *byte % 4 == 1),
            8 => true,
            _ => false,
        };
        if !tail_valid {
            return None;
        }
        let directrix_record = resolve_ref(self.journal, carrier_index, directrix_ref)?;
        let directrix_index = self.journal.index(
            directrix_record.source_index(),
            directrix_record.source_range().start,
        )?;
        let (curve_record, curve_index, v_range) = if directrix_record.family()
            == ConsolidatedFamily::B
            && directrix_record.class() == 0x24
        {
            let dpl = directrix_record
                .payload()
                .and_then(|range| self.data.get(range))?;
            if dpl.first() != Some(&0x81) {
                return None;
            }
            let mut dat = 1;
            let cref = allocation_reference(dpl, &mut dat)?;
            let mut dview = View::over_retained(dpl.get(dat..)?);
            let range = match dview.u8()? {
                0x0f => {
                    let lo = dview.f64_le()?;
                    let hi = dview.f64_le()?;
                    if !dview.is_empty() {
                        return None;
                    }
                    Some(IncreasingParameterInterval::new([lo, hi])?.endpoints())
                }
                _ => None,
            };
            let target = resolve_ref(self.journal, directrix_index, cref)?;
            let target_index = self
                .journal
                .index(target.source_index(), target.source_range().start)?;
            (target, target_index, range)
        } else {
            (directrix_record, directrix_index, None)
        };

        let curve_3d = self.decode_3d_curve(curve_index, curve_record, v_range, refusal, depth)?;
        extrude_nurbs_curve(&curve_3d, direction, u_bounds)
    }

    fn decode_3d_curve(
        &self,
        curve_index: usize,
        record: &'a ConsolidatedRecord,
        explicit_range: Option<[f64; 2]>,
        refusal: &mut crate::nurbs::LaneRefusals,
        depth: usize,
    ) -> Option<NurbsCurve> {
        if depth > MAX_ALIAS_DEPTH {
            return None;
        }
        match (record.family(), record.class()) {
            (ConsolidatedFamily::B, 0x16) => {
                self.b2_nurbs_curves.get(&record.byte_offset()).cloned()
            }
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x21) => {
                let (support_ref, pcurve_nurbs, range) =
                    decode_u32_bspline_pcurve(self.data, record)?;
                let support_record = resolve_ref(self.journal, curve_index, support_ref)?;
                let support_geom =
                    self.resolve_surface(curve_index, support_record, refusal, depth + 1)?;
                let range = explicit_range.unwrap_or(range);
                lift_pcurve_to_3d(
                    &PcurveGeometry::Nurbs {
                        nurbs: pcurve_nurbs,
                    },
                    &support_geom,
                    range,
                    refusal,
                )
            }
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x20) => {
                if let Some(jet) = self.compact_jets.get(&record.byte_offset()) {
                    let support_record = self
                        .journal
                        .resolve_weighted_distance(curve_index, jet.support_id)?;
                    let support_geom =
                        self.resolve_surface(curve_index, support_record, refusal, depth + 1)?;
                    let pcurve =
                        consolidated_jet_pcurve(jet, &ConsolidatedCarrierChart::Identity, refusal)?;
                    let range = explicit_range.unwrap_or(jet.range.endpoints());
                    lift_pcurve_to_3d(&pcurve, &support_geom, range, refusal)
                } else {
                    let (support_ref, pcurve, range) =
                        decode_u32_jet_pcurve(self.data, record, refusal)?;
                    let support_record = resolve_ref(self.journal, curve_index, support_ref)?;
                    let support_geom =
                        self.resolve_surface(curve_index, support_record, refusal, depth + 1)?;
                    let range = explicit_range.unwrap_or(range);
                    lift_pcurve_to_3d(&pcurve, &support_geom, range, refusal)
                }
            }
            (ConsolidatedFamily::B, 0x18) => {
                let payload = record.payload().and_then(|range| self.data.get(range))?;
                let mut at = 0;
                let support_ref = allocation_reference(payload, &mut at)?;
                let support_record = resolve_ref(self.journal, curve_index, support_ref)?;
                let support_geom =
                    self.resolve_surface(curve_index, support_record, refusal, depth + 1)?;
                let param_pt =
                    b2_parameter_points_from_records(self.data, std::slice::from_ref(record))
                        .next()?;
                let [uv0, uv1] = param_pt.axis_line()?;
                let line = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(uv0[0], uv0[1]),
                    Point2::new(uv1[0] - uv0[0], uv1[1] - uv0[1]),
                )
                .ok()?;
                lift_pcurve_to_3d(
                    &PcurveGeometry::Line(line),
                    &support_geom,
                    [0.0, 1.0],
                    refusal,
                )
            }
            _ => None,
        }
    }
}

/// Lift any 2D `PcurveGeometry` through a solved 3D surface into a 3D `NurbsCurve`.
pub(in crate::families) fn lift_pcurve_to_3d(
    pcurve: &PcurveGeometry,
    surface: &SolvedSurfaceGeometry,
    range: [f64; 2],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    if let SolvedSurfaceGeometry::Plane(plane) = surface {
        if let Some(exact) = lift_plane_pcurve(pcurve, plane, refusal) {
            return Some(exact);
        }
    }
    fit_edge_with_error(
        range,
        |t| {
            let uv = eval::decode::pcurve_uv(EvaluationAdmission::Standard, pcurve, t)
                .ok()?
                .get();
            let point = eval::decode::surface_point_solved(
                EvaluationAdmission::Standard,
                surface,
                uv.u,
                uv.v,
            )
            .ok()?
            .get();
            Some([point.x, point.y, point.z])
        },
        DIRECTRIX_FIT_TOLERANCE_MM,
    )
}

/// Extrude a 3D `NurbsCurve` along a unit direction over `[u_min, u_max]` to
/// produce an exact tensor-product `NurbsSurface` of degree `(curve.degree(), 1)`.
pub(in crate::families) fn extrude_nurbs_curve(
    curve: &NurbsCurve,
    direction: UnitVector3,
    [u_lo, u_hi]: [f64; 2],
) -> Option<NurbsSurface> {
    if !u_lo.is_finite() || !u_hi.is_finite() || u_lo >= u_hi {
        return None;
    }
    let axis = *direction.as_raw();
    let rows: Vec<Vec<Point3>> = curve
        .control_points()
        .iter()
        .map(|point| {
            let p = point.get();
            vec![p.translated(axis, u_lo), p.translated(axis, u_hi)]
        })
        .collect();
    let grid = if let Some(weights) = curve.weights() {
        let weighted_rows: Vec<Vec<cadmpeg_ir::geometry::nurbs::WeightedPole3>> = rows
            .into_iter()
            .zip(weights.iter().copied())
            .map(|(row, weight)| {
                row.into_iter()
                    .map(|point| cadmpeg_ir::geometry::nurbs::WeightedPole3 { point, weight })
                    .collect()
            })
            .collect();
        NurbsPoleGrid::Rational {
            rows: weighted_rows,
        }
    } else {
        NurbsPoleGrid::Polynomial { rows }
    };
    NurbsSurface::new(
        NurbsSurfaceAxis::new(curve.degree(), curve.knots().clone(), false),
        NurbsSurfaceAxis::new(1, vec![u_lo, u_lo, u_hi, u_hi], false),
        grid,
        false,
    )
    .ok()
}

/// Decode a `u32le`-header class-`0x21` 2D B-spline pcurve (`A_21` or `B_21`).
pub(in crate::families) fn decode_u32_bspline_pcurve(
    data: &[u8],
    record: &ConsolidatedRecord,
) -> Option<(AllocationReference, PcurveNurbs, [f64; 2])> {
    if record.class() != 0x21 {
        return None;
    }
    let payload = record.payload().and_then(|range| data.get(range))?;
    let mut at = 0;
    let support_ref = allocation_reference(payload, &mut at)?;
    let mut view = View::over_retained(payload.get(at..)?);
    if view.take(2)? != [0x00, 0x00] {
        return None;
    }
    let degree = view.u32_le()?;
    let nknots = usize::try_from(view.u32_le()?).ok()?;
    if !matches!(degree, 1 | 3 | 5) || !(2..=MAX_BSPLINE_PCURVE_KNOTS).contains(&nknots) {
        return None;
    }
    if view.u8()? != 0x04 {
        return None;
    }
    let mut distinct = Vec::with_capacity(nknots);
    for _ in 0..nknots {
        distinct.push(view.f64_le()?);
    }
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }
    if view.u8()? != 0x01 {
        return None;
    }
    let interior_mult = match degree {
        5 => 3usize,
        _ => 1usize,
    };
    let pole_count = usize::try_from(degree)
        .ok()?
        .checked_add(1)?
        .checked_add(interior_mult.checked_mul(nknots - 2)?)?;
    let mut poles = Vec::with_capacity(pole_count);
    for _ in 0..pole_count {
        let u = view.f64_le()?;
        let v = view.f64_le()?;
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        poles.push(Point2::new(u, v));
    }
    let mut knots = Vec::with_capacity(pole_count + usize::try_from(degree).ok()? + 1);
    for (index, &knot) in distinct.iter().enumerate() {
        let mult = if index == 0 || index + 1 == nknots {
            usize::try_from(degree).ok()? + 1
        } else {
            interior_mult
        };
        knots.extend(std::iter::repeat_n(knot, mult));
    }
    let range = [*distinct.first()?, *distinct.last()?];
    let nurbs = PcurveNurbs::from_lanes(degree, knots, poles, None, false).ok()?;
    Some((support_ref, nurbs, range))
}

/// Decode a `u32le`-header class-`0x20` quintic UV jet pcurve (`A_20` or `B_20`).
pub(in crate::families) fn decode_u32_jet_pcurve(
    data: &[u8],
    record: &ConsolidatedRecord,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(AllocationReference, PcurveGeometry, [f64; 2])> {
    if record.class() != 0x20 {
        return None;
    }
    let payload = record.payload().and_then(|range| data.get(range))?;
    let mut at = 0;
    let support_ref = allocation_reference(payload, &mut at)?;
    let mut view = View::over_retained(payload.get(at..)?);
    let degree = view.u32_le()?;
    let nknots = usize::try_from(view.u32_le()?).ok()?;
    if degree != 5 || !(2..=MAX_BSPLINE_PCURVE_KNOTS).contains(&nknots) {
        return None;
    }
    if view.u8()? != 0x04 {
        return None;
    }
    let mut knots = Vec::with_capacity(nknots);
    for _ in 0..nknots {
        knots.push(cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?);
    }
    let raw_knots: Vec<f64> = knots.iter().map(|k| k.get()).collect();
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&raw_knots) {
        return None;
    }
    if view.u8()? != 0x01 || usize::try_from(view.u32_le()?).ok()? != nknots {
        return None;
    }
    let mut lanes = [
        Vec::with_capacity(nknots),
        Vec::with_capacity(nknots),
        Vec::with_capacity(nknots),
        Vec::with_capacity(nknots),
        Vec::with_capacity(nknots),
        Vec::with_capacity(nknots),
    ];
    for lane in &mut lanes {
        for _ in 0..nknots {
            lane.push(cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?);
        }
    }
    let mut points = Vec::with_capacity(nknots);
    let mut first = Vec::with_capacity(nknots);
    let mut second = Vec::with_capacity(nknots);
    for i in 0..nknots {
        points.push([lanes[0][i].get(), lanes[1][i].get()]);
        first.push([lanes[2][i].get(), lanes[3][i].get()]);
        second.push([lanes[4][i].get(), lanes[5][i].get()]);
    }
    let range = [*raw_knots.first()?, *raw_knots.last()?];
    let pcurve = crate::assemble::quintic_jet_pcurve(
        5,
        &raw_knots,
        &points,
        (&first, &second),
        refusal,
        "u32le consolidated quintic UV jet",
    )?;
    Some((support_ref, pcurve, range))
}

/// Decode a consolidated `b2 03 27` plane carrier across all observed bitmask
/// layouts (both bounded `0x84..=0xf4` and unbounded `0x08` frames).
pub(in crate::families) fn offset_analytic_surface(
    base: &SolvedSurfaceGeometry,
    distance: f64,
) -> Option<SolvedSurfaceGeometry> {
    if !distance.is_finite() {
        return None;
    }
    match base {
        SolvedSurfaceGeometry::Plane(plane) => {
            let axis = *plane.frame().axis().as_raw();
            let origin = FinitePoint3::new(plane.origin().get().translated(axis, distance))?;
            Some(SolvedSurfaceGeometry::Plane(PlaneSurface::new(
                origin,
                *plane.frame(),
            )))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let radius =
                cadmpeg_ir::scalar::PositiveLength::new(cylinder.radius().get() + distance)?;
            Some(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                    cylinder.origin(),
                    *cylinder.frame(),
                    radius,
                ),
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            if sphere.radius().get() <= 0.0 {
                return None;
            }
            let radius = cadmpeg_ir::scalar::PositiveLength::new(sphere.radius().get() + distance)?;
            Some(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::new(
                    sphere.center(),
                    *sphere.frame(),
                    cadmpeg_ir::scalar::NonZeroLength::from(radius),
                ),
            ))
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            if torus.minor_radius().get() <= 0.0 {
                return None;
            }
            let minor =
                cadmpeg_ir::scalar::PositiveLength::new(torus.minor_radius().get() + distance)?;
            Some(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    torus.center(),
                    *torus.frame(),
                    torus.major_radius(),
                    cadmpeg_ir::scalar::NonZeroLength::from(minor),
                ),
            ))
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            const CONE_OFFSET_EXACT_TOLERANCE: f64 = 1.0e-12;
            if (cone.ratio().get() - 1.0).abs() > CONE_OFFSET_EXACT_TOLERANCE {
                return None;
            }
            let sin_half = cone.half_angle().get().sin();
            if !sin_half.is_finite() || sin_half.abs() <= CONE_OFFSET_EXACT_TOLERANCE {
                return None;
            }
            let shift = -distance / sin_half;
            let axis = *cone.frame().axis().as_raw();
            let origin = FinitePoint3::new(cone.origin().get().translated(axis, shift))?;
            Some(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::new(
                    origin,
                    *cone.frame(),
                    cone.radius(),
                    cone.ratio(),
                    cone.half_angle(),
                ),
            ))
        }
        _ => None,
    }
}

fn resolve_b2_plane<'a>(
    data: &'a [u8],
    journal: &AllocationJournal<'a>,
    carrier: &'a ConsolidatedRecord,
    b2_planes: &HashMap<usize, crate::families::b2::records::B2PlaneCarrier>,
    b2_offsets: &HashMap<usize, crate::families::b2::records::B2OffsetSupport>,
    depth: usize,
) -> Option<PlaneSurface> {
    if depth > MAX_ALIAS_DEPTH {
        return None;
    }
    match (carrier.family(), carrier.class()) {
        (ConsolidatedFamily::B, 0x27) => decode_b2_plane(data, carrier, b2_planes),
        (ConsolidatedFamily::B, 0x2e) => {
            let carrier_index =
                journal.index(carrier.source_index(), carrier.source_range().start)?;
            let payload = carrier.payload().and_then(|range| data.get(range))?;
            let mut at = usize::from(payload.first() == Some(&0x81));
            let inner_ref = allocation_reference(payload, &mut at)?;
            let inner = resolve_ref(journal, carrier_index, inner_ref)?;
            resolve_b2_plane(data, journal, inner, b2_planes, b2_offsets, depth + 1)
        }
        (ConsolidatedFamily::B, 0x30 | 0x31) => {
            let carrier_index =
                journal.index(carrier.source_index(), carrier.source_range().start)?;
            let payload = carrier.payload().and_then(|range| data.get(range))?;
            let mut at = 0;
            let base_ref = allocation_reference(payload, &mut at)?;
            let base = resolve_ref(journal, carrier_index, base_ref)?;
            let distance = if let Some(parsed) = b2_offsets.get(&carrier.byte_offset()) {
                parsed.distance.get()
            } else {
                let mut view = View::over_retained(payload.get(at..)?);
                let d = view.f64_le()?;
                let u0 = view.f64_le()?;
                let v0 = view.f64_le()?;
                let u1 = view.f64_le()?;
                let v1 = view.f64_le()?;
                if !d.is_finite() || !view.is_empty() {
                    return None;
                }
                IncreasingParameterInterval::new([u0, u1])?;
                IncreasingParameterInterval::new([v0, v1])?;
                d
            };
            let plane = resolve_b2_plane(data, journal, base, b2_planes, b2_offsets, depth + 1)?;
            let axis = *plane.frame().axis().as_raw();
            let origin = FinitePoint3::new(plane.origin().get().translated(axis, distance))?;
            Some(PlaneSurface::new(origin, *plane.frame()))
        }
        _ => None,
    }
}

pub(super) fn decode_b2_plane(
    data: &[u8],
    record: &ConsolidatedRecord,
    b2_planes: &HashMap<usize, crate::families::b2::records::B2PlaneCarrier>,
) -> Option<PlaneSurface> {
    if record.family() != ConsolidatedFamily::B || record.class() != 0x27 {
        return None;
    }
    if let Some(carrier) = b2_planes.get(&record.byte_offset()) {
        if let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))) =
            b2_plane_geometry(carrier)
        {
            return Some(plane);
        }
    }
    if let Some(plane) =
        crate::families::consolidated::packed_plane::packed_plane(View::over_retained(data), record)
    {
        return Some(plane);
    }
    let bytes = record.payload().and_then(|range| data.get(range))?;
    decode_bitmask_plane(bytes)
}

/// General bitmask decoder for `b2 03 27` plane payloads.
///
/// Bounded planes use `(layout & 0x8f) == 0x84` with `mask`:
/// - `mask & 0x07`: omitted zero coordinates of origin `(x, y, z)`
/// - `mask & 0x78`: U-axis zero-component mask (`0x10/0x20/0x40`) + unit flag (`0x08`)
/// - `(layout & 0x70) | ((mask & 0x80) >> 4)`: V-axis zero-component mask + unit flag
/// Unbounded planes use `layout == 0x08`, `mask`, and `frame` where
/// `(frame - 0x18) << 4` supplies the V-axis zero-component mask.
fn decode_bitmask_plane(bytes: &[u8]) -> Option<PlaneSurface> {
    let mut view = View::over_retained(bytes);
    let layout = view.u8()?;
    let mask = view.u8()?;
    let (origin_zero_mask, u_mask, v_mask, has_bounds) = if layout == 0x08 {
        let frame = view.u8()?;
        let v_omitted_nibble = frame.checked_sub(0x18)?;
        if v_omitted_nibble > 7 {
            return None;
        }
        let v_mask = (v_omitted_nibble << 4) | ((mask & 0x80) >> 4);
        (mask & 0x07, mask & 0x78, v_mask, false)
    } else if (layout & 0x8f) == 0x84 {
        let v_mask = (layout & 0x70) | ((mask & 0x80) >> 4);
        (mask & 0x07, mask & 0x78, v_mask, true)
    } else {
        return None;
    };

    let mut read_origin_component = |bit: u8| -> Option<f64> {
        if origin_zero_mask & bit != 0 {
            Some(0.0)
        } else {
            view.f64_le()
        }
    };
    let origin = FinitePoint3::new(Point3::new(
        read_origin_component(0x01)?,
        read_origin_component(0x02)?,
        read_origin_component(0x04)?,
    ))?;

    let mut read_axis = |axis_mask: u8| -> Option<UnitVector3> {
        let omitted = axis_mask & 0x70;
        let unit_component = axis_mask & 0x08 != 0;
        if unit_component && omitted.count_ones() != 2 {
            return None;
        }
        let mut comp = |bit: u8| -> Option<f64> {
            if omitted & bit != 0 {
                Some(0.0)
            } else if unit_component {
                Some(1.0)
            } else {
                view.f64_le()
            }
        };
        UnitVector3::new(Vector3::new(comp(0x10)?, comp(0x20)?, comp(0x40)?))
    };

    let u = read_axis(u_mask)?;
    let v = read_axis(v_mask)?;
    if has_bounds {
        let u0 = view.f64_le()?;
        let u1 = view.f64_le()?;
        let v0 = view.f64_le()?;
        let v1 = view.f64_le()?;
        IncreasingParameterInterval::new([u0, u1])?;
        IncreasingParameterInterval::new([v0, v1])?;
    }
    if !view.is_empty() {
        return None;
    }
    Some(PlaneSurface::new(
        origin,
        OrthonormalFrame3::completing(u, v)?,
    ))
}

fn unique_5f_carriers<'a>(
    data: &[u8],
    records: &'a [ConsolidatedRecord],
    journal: &AllocationJournal<'a>,
) -> HashMap<u64, Option<(usize, &'a ConsolidatedRecord)>> {
    let mut map = HashMap::<u64, Option<(usize, &'a ConsolidatedRecord)>>::new();
    for (face_index, record) in records.iter().enumerate() {
        if record.family() != ConsolidatedFamily::B || record.class() != 0x5f {
            continue;
        }
        let Some(face_tag) = journal.absolute_address(face_index) else {
            continue;
        };
        let Some(payload) = record.payload().and_then(|range| data.get(range)) else {
            continue;
        };
        let Some(count) = payload.first().and_then(|byte| byte.checked_sub(0x80)) else {
            continue;
        };
        if count < 2 {
            continue;
        }
        let mut at = 1;
        let Some(carrier_ref) = allocation_reference(payload, &mut at) else {
            continue;
        };
        let Some(carrier) = resolve_ref(journal, face_index, carrier_ref) else {
            continue;
        };
        map.entry(face_tag)
            .and_modify(|slot| *slot = None)
            .or_insert(Some((face_index, carrier)));
    }
    map
}

pub(in crate::families) fn resolve_ref<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    reference: AllocationReference,
) -> Option<&'a ConsolidatedRecord> {
    match reference.encoding {
        AllocationReferenceEncoding::BackwardDistance | AllocationReferenceEncoding::WidthCoded => {
            journal.resolve_weighted_distance(origin, reference.value)
        }
        AllocationReferenceEncoding::OwnedChild => journal.resolve_address(
            origin,
            journal
                .address(origin)?
                .checked_add(u64::from(reference.value))?
                .checked_add(1)?,
        ),
        AllocationReferenceEncoding::Selector2
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16
        | AllocationReferenceEncoding::TaggedU24 => None,
    }
}

fn source_carrier_tag(source: Option<&cadmpeg_ir::SourceObjectAssociation>) -> Option<u64> {
    let source = source?;
    u64::from_str_radix(source.object_id.as_str().strip_prefix("cgm-carrier:")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::{decode_bitmask_plane, extrude_nurbs_curve};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::units::UnitVector3;

    #[test]
    fn bitmask_plane_decodes_canonical_and_omitted_origin_layouts() {
        const TEST_TOLERANCE: f64 = 1.0e-12;
        // Canonical XY plane at origin: 08 ef 1d
        let xy = decode_bitmask_plane(&[0x08, 0xef, 0x1d]).expect("canonical XY plane");
        assert!((xy.origin().get().distance(Point3::new(0.0, 0.0, 0.0))) < TEST_TOLERANCE);
        assert!((xy.frame().axis().as_raw().z - 1.0).abs() < TEST_TOLERANCE);

        // Canonical YZ plane at origin: 08 df 1b
        let yz = decode_bitmask_plane(&[0x08, 0xdf, 0x1b]).expect("canonical YZ plane");
        assert!((yz.frame().axis().as_raw().x - 1.0).abs() < TEST_TOLERANCE);

        // Bounded plane with ox=0, u=(0,0,1), v=(1,0,0): layout=0xe4, mask=0x31
        let mut bounded = vec![0xe4, 0x31];
        for value in [10.0_f64, 20.0, 1.0, 1.0, -5.0, 5.0, -10.0, 10.0] {
            bounded.extend_from_slice(&value.to_le_bytes());
        }
        let plane = decode_bitmask_plane(&bounded).expect("bounded omitted-x plane");
        assert!((plane.origin().get().x).abs() < TEST_TOLERANCE);
        assert!((plane.origin().get().y - 10.0).abs() < TEST_TOLERANCE);
        assert!((plane.origin().get().z - 20.0).abs() < TEST_TOLERANCE);
        assert!((plane.frame().axis().as_raw().y - 1.0).abs() < TEST_TOLERANCE);
    }

    #[test]
    fn extrude_nurbs_curve_produces_exact_tensor_surface() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("line nurbs");
        let surface =
            extrude_nurbs_curve(&curve, UnitVector3::Z_AXIS, [0.0, 5.0]).expect("extruded surface");
        assert_eq!(surface.u_degree(), 1);
        assert_eq!(surface.v_degree(), 1);
        assert_eq!(surface.u_count(), 2);
        assert_eq!(surface.v_count(), 2);
    }

    #[test]
    fn decode_u32_bspline_pcurve_reads_quintic_two_knot_segment() {
        const TEST_TOLERANCE: f64 = 1.0e-12;
        let mut payload = vec![0x05, 0x00, 0x00];
        payload.extend_from_slice(&5_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.push(0x04);
        for knot in [0.0_f64, 2.0_f64] {
            payload.extend_from_slice(&knot.to_le_bytes());
        }
        payload.push(0x01);
        for i in 0..6 {
            payload.extend_from_slice(&(f64::from(i)).to_le_bytes());
            payload.extend_from_slice(&(f64::from(i) * 2.0).to_le_bytes());
        }
        let mut frame = vec![0xb2, 0x03, 0x21, u8::try_from(payload.len()).unwrap(), 0x05];
        frame.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&frame);
        assert_eq!(records.len(), 1);
        let (support_ref, nurbs, range) =
            super::decode_u32_bspline_pcurve(&frame, &records[0]).expect("decoded 0x21 pcurve");
        assert_eq!(support_ref.value, 1);
        assert_eq!(nurbs.degree(), 5);
        assert_eq!(nurbs.control_points().len(), 6);
        assert!((range[0] - 0.0).abs() < TEST_TOLERANCE);
        assert!((range[1] - 2.0).abs() < TEST_TOLERANCE);
    }

    #[test]
    fn offset_analytic_surface_shifts_along_exact_unit_normal() {
        use cadmpeg_ir::geometry::analytic::{
            ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
        };
        use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
        use cadmpeg_ir::math::Vector3;

        const TOL: f64 = 1.0e-9;
        let dist = 2.5;
        let origin = Point3::new(1.0, -2.0, 3.0);
        let z = Vector3::new(0.0, 0.0, 1.0);
        let x = Vector3::new(1.0, 0.0, 0.0);

        let plane =
            SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(origin, z, x).expect("plane"));
        let Some(SolvedSurfaceGeometry::Plane(off_plane)) =
            super::offset_analytic_surface(&plane, dist)
        else {
            panic!("plane offset");
        };
        assert!((off_plane.origin().get().z - (origin.z + dist)).abs() < TOL);

        let cyl = SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, z, x, 10.0).expect("cylinder"),
        );
        let Some(SolvedSurfaceGeometry::Cylinder(off_cyl)) =
            super::offset_analytic_surface(&cyl, dist)
        else {
            panic!("cylinder offset");
        };
        assert!((off_cyl.radius().get() - 12.5).abs() < TOL);

        let sph = SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(origin, z, x, 10.0).expect("sphere"),
        );
        let Some(SolvedSurfaceGeometry::Sphere(off_sph)) =
            super::offset_analytic_surface(&sph, -dist)
        else {
            panic!("sphere offset");
        };
        assert!((off_sph.radius().get() - 7.5).abs() < TOL);

        let tor = SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(origin, z, x, 20.0, 5.0).expect("torus"),
        );
        let Some(SolvedSurfaceGeometry::Torus(off_tor)) =
            super::offset_analytic_surface(&tor, dist)
        else {
            panic!("torus offset");
        };
        assert!((off_tor.major_radius().get() - 20.0).abs() < TOL);
        assert!((off_tor.minor_radius().get() - 7.5).abs() < TOL);

        let half = std::f64::consts::FRAC_PI_6;
        let cone = SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(origin, z, x, 4.0, 1.0, half).expect("cone"),
        );
        let Some(SolvedSurfaceGeometry::Cone(off_cone)) =
            super::offset_analytic_surface(&cone, dist)
        else {
            panic!("cone offset");
        };
        assert!((off_cone.origin().get().z - (origin.z - dist / half.sin())).abs() < TOL);

        let ell_cone = SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(origin, z, x, 4.0, 1.5, half).expect("elliptical cone"),
        );
        assert!(super::offset_analytic_surface(&ell_cone, dist).is_none());
    }
}
