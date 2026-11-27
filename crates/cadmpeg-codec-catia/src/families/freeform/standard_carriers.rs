// SPDX-License-Identifier: Apache-2.0
//! Direct standard-face carrier decoding through the consolidated allocation journal.

use std::collections::HashMap;

use cadmpeg_core::decode::View;
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
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord, SourceExtent};

use super::rolling_ball_patch::fit_edge_with_error;
use super::weighted_plane::lift_plane_pcurve;
use super::{consolidated_jet_pcurve, ConsolidatedCarrierChart};

const PLANE_BOUNDS_TOLERANCE_MM: f64 = 0.05;
const ZERO_EXTENT_TOLERANCE_MM: f64 = 1.0e-5;
const NONZERO_EXTENT_MIN_MM: f64 = 1.0e-3;
const DIRECTRIX_FIT_TOLERANCE_MM: f64 = 0.0005;
const TERMINAL_SPAN_DOMAIN_TOLERANCE: f64 = 1.0e-6;
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
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
    brep: &[u8],
    surface_records: &[StandardSurfaceRecord],
    normals: &mut HashMap<u32, FiniteVector<3>>,
) {
    let journal = AllocationJournal::with_sources(data, sources, records);
    let face_carriers = unique_5f_carriers(data, sources, records, &journal);
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
                resolve_b2_plane(data, sources, &journal, carrier, &b2_planes, &b2_offsets, 0)
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
        let extents = bounds
            .aabb_half_extents
            .map(cadmpeg_ir::scalar::NonNegativeLength::get);
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
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
    a5_carriers: &[FreeformSurface],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<usize, CodecError> {
    let journal = AllocationJournal::with_sources(data, sources, records);
    let face_carriers = unique_5f_carriers(data, sources, records, &journal);
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
        sources,
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
    sources: &'a [Vec<SourceExtent>],
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
                    crate::families::a5a8::records::a5_owned_surface_from_record_in_sources(
                        self.data,
                        self.sources,
                        carrier,
                        refusal,
                    )
                    .ok()
                    .flatten()
                    .map(|surface| SolvedSurfaceGeometry::Nurbs(surface.geometry))
                })
                .or_else(|| {
                    decode_u32_nurbs_surface(self.data, self.sources, carrier, refusal)
                        .map(SolvedSurfaceGeometry::Nurbs)
                }),
            (ConsolidatedFamily::B, 0x27) => {
                decode_b2_plane_in_sources(self.data, self.sources, carrier, self.b2_planes)
                    .map(SolvedSurfaceGeometry::Plane)
            }
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
                let payload = carrier.payload_bytes(self.data, self.sources)?;
                let mut at = 0;
                if payload.first() == Some(&0x81) {
                    at = 1;
                }
                let inner_ref = allocation_reference(&payload, &mut at)?;
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
        let payload = carrier.payload_bytes(self.data, self.sources)?;
        let mut at = 0;
        let base_ref = allocation_reference(&payload, &mut at)?;
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
        let payload = carrier.payload_bytes(self.data, self.sources)?;
        let mut at = 0;
        let directrix_ref = allocation_reference(&payload, &mut at)?;
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
        let curve_3d =
            self.decode_3d_curve(directrix_index, directrix_record, None, refusal, depth)?;
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
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x24 | 0x25) => {
                let dpl = record.payload_bytes(self.data, self.sources)?;
                match dpl.first().copied()? {
                    0x81 => {
                        let mut dat = 1;
                        let cref = allocation_reference(&dpl, &mut dat)?;
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
                        let target = resolve_ref(self.journal, curve_index, cref)?;
                        let target_index = self
                            .journal
                            .index(target.source_index(), target.source_range().start)?;
                        self.decode_3d_curve(
                            target_index,
                            target,
                            explicit_range.or(range),
                            refusal,
                            depth + 1,
                        )
                    }
                    0x82 => {
                        let mut dat = 1;
                        let ref1 = allocation_reference(&dpl, &mut dat)?;
                        let ref2 = allocation_reference(&dpl, &mut dat)?;
                        let mut dview = View::over_retained(dpl.get(dat..)?);
                        if dview.take(3)? != [0x0a, 0x05, 0x00] {
                            return None;
                        }
                        let r1_lo = dview.f64_le()?;
                        let r1_hi = dview.f64_le()?;
                        let _gap = dview.f64_le()?;
                        let r2_lo = dview.f64_le()?;
                        let r2_hi = dview.f64_le()?;
                        let range1 = IncreasingParameterInterval::new([r1_lo, r1_hi])?.endpoints();
                        let range2 = IncreasingParameterInterval::new([r2_lo, r2_hi])?.endpoints();
                        let t1 = resolve_ref(self.journal, curve_index, ref1)?;
                        let t1_idx = self
                            .journal
                            .index(t1.source_index(), t1.source_range().start)?;
                        let t2 = resolve_ref(self.journal, curve_index, ref2)?;
                        let t2_idx = self
                            .journal
                            .index(t2.source_index(), t2.source_range().start)?;
                        let c1 =
                            self.decode_3d_curve(t1_idx, t1, Some(range1), refusal, depth + 1)?;
                        let c2 =
                            self.decode_3d_curve(t2_idx, t2, Some(range2), refusal, depth + 1)?;
                        // Cross-check both dual-carrier representations agree at their endpoints.
                        let eval_end = |curve: &NurbsCurve, t: f64| -> Option<Point3> {
                            let cg = cadmpeg_ir::geometry::CurveGeometry::Solved(
                                cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve.clone()),
                            );
                            eval::decode::curve_point(EvaluationAdmission::Standard, &cg, t)
                                .ok()
                                .map(|p| p.get())
                        };
                        let p1a = eval_end(&c1, range1[0])?;
                        let p1b = eval_end(&c1, range1[1])?;
                        let p2a = eval_end(&c2, range2[0])?;
                        let p2b = eval_end(&c2, range2[1])?;
                        const DUAL_ENDPOINT_TOL_MM: f64 = 0.05;
                        let same_dir = p1a.distance(p2a) <= DUAL_ENDPOINT_TOL_MM
                            && p1b.distance(p2b) <= DUAL_ENDPOINT_TOL_MM;
                        let opp_dir = p1a.distance(p2b) <= DUAL_ENDPOINT_TOL_MM
                            && p1b.distance(p2a) <= DUAL_ENDPOINT_TOL_MM;
                        (same_dir || opp_dir).then_some(c1)
                    }
                    _ => None,
                }
            }
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x16) => self
                .b2_nurbs_curves
                .get(&record.byte_offset())
                .cloned()
                .or_else(|| decode_compact_nurbs_curve(self.data, self.sources, record, refusal)),
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x21) => {
                let (support_ref, pcurve_nurbs, range) = decode_u32_bspline_pcurve_payload(
                    &record.payload_bytes(self.data, self.sources)?,
                )?;
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
                    let payload = record.payload_bytes(self.data, self.sources)?;
                    let (support_ref, pcurve, range) =
                        decode_u32_jet_pcurve_payload(&payload, refusal)?;
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
pub(super) fn decode_u32_bspline_pcurve_in_sources(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
) -> Option<(AllocationReference, PcurveNurbs, [f64; 2])> {
    if record.class() != 0x21 {
        return None;
    }
    let payload = record.payload_bytes(data, sources)?;
    decode_u32_bspline_pcurve_payload(&payload)
}

fn decode_u32_bspline_pcurve_payload(
    payload: &[u8],
) -> Option<(AllocationReference, PcurveNurbs, [f64; 2])> {
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
    match view.u8()? {
        0x04 => {}
        0x00 => {
            let _flag = view.u32_le()?;
        }
        _ => return None,
    }
    let mut distinct = Vec::with_capacity(nknots);
    for _ in 0..nknots {
        distinct.push(view.f64_le()?);
    }
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }
    let deg_usize = usize::try_from(degree).ok()?;
    let multiplicities: Vec<usize> = match view.u8()? {
        0x01 => {
            let interior_mult = match degree {
                _ if nknots == 2 => 1usize,
                5 => 3usize,
                1..=3 => 1usize,
                _ => return None,
            };
            (0..nknots)
                .map(|index| {
                    if index == 0 || index + 1 == nknots {
                        deg_usize + 1
                    } else {
                        interior_mult
                    }
                })
                .collect()
        }
        0x00 => {
            if nknots.checked_mul(4)? > view.remaining() {
                return None;
            }
            let mut mults = Vec::with_capacity(nknots);
            for _ in 0..nknots {
                let mult = usize::try_from(view.u32_le()?).ok()?;
                if mult == 0 || mult > deg_usize + 1 {
                    return None;
                }
                mults.push(mult);
            }
            mults
        }
        _ => return None,
    };
    let total_knots = multiplicities
        .iter()
        .try_fold(0usize, |acc, &m| acc.checked_add(m))?;
    let pole_count = total_knots.checked_sub(deg_usize.checked_add(1)?)?;
    if pole_count <= deg_usize || pole_count.checked_mul(16)? > view.remaining() {
        return None;
    }
    let mut poles = Vec::with_capacity(pole_count);
    for _ in 0..pole_count {
        let u = view.f64_le()?;
        let v = view.f64_le()?;
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        poles.push(Point2::new(u, v));
    }
    let mut knots = Vec::with_capacity(total_knots);
    for (&knot, &mult) in distinct.iter().zip(&multiplicities) {
        knots.extend(std::iter::repeat_n(knot, mult));
    }
    let range = [*distinct.first()?, *distinct.last()?];
    let nurbs = PcurveNurbs::from_lanes(degree, knots, poles, None, false).ok()?;
    Some((support_ref, nurbs, range))
}

/// Decode a `u32le`-header class-`0x20` quintic UV jet pcurve (`A_20` or `B_20`).
pub(super) fn decode_u32_jet_pcurve_in_sources(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(AllocationReference, PcurveGeometry, [f64; 2])> {
    if record.class() != 0x20 {
        return None;
    }
    let payload = record.payload_bytes(data, sources)?;
    decode_u32_jet_pcurve_payload(&payload, refusal)
}

fn decode_u32_jet_pcurve_payload(
    payload: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(AllocationReference, PcurveGeometry, [f64; 2])> {
    let mut at = 0;
    let support_ref = allocation_reference(payload, &mut at)?;
    let mut view = View::over_retained(payload.get(at..)?);
    let degree = view.u32_le()?;
    let nknots = usize::try_from(view.u32_le()?).ok()?;
    if degree != 5 || !(2..=MAX_BSPLINE_PCURVE_KNOTS).contains(&nknots) {
        return None;
    }
    match view.u8()? {
        0x04 => {}
        0x00 => {
            let _flag = view.u32_le()?;
        }
        _ => return None,
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
    for lane in &mut lanes[..4] {
        for _ in 0..nknots {
            lane.push(cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?);
        }
    }
    if [view.u8()?, view.u8()?] != [0x01, 0x00] {
        return None;
    }
    for lane in &mut lanes[4..] {
        for _ in 0..nknots {
            lane.push(cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?);
        }
    }
    let range = if view.remaining() == 18 {
        let t0 = view.f64_le()?;
        let t1 = view.f64_le()?;
        if [view.u8()?, view.u8()?] != [0xfe, 0xff] {
            return None;
        }
        IncreasingParameterInterval::new([t0, t1])?.endpoints()
    } else if view.is_empty() {
        [*raw_knots.first()?, *raw_knots.last()?]
    } else {
        return None;
    };
    let [u, v, du, dv, d2u, d2v] = lanes;
    let points: Vec<[f64; 2]> = u
        .into_iter()
        .zip(v)
        .map(|(x, y)| [x.get(), y.get()])
        .collect();
    let first: Vec<[f64; 2]> = du
        .into_iter()
        .zip(dv)
        .map(|(x, y)| [x.get(), y.get()])
        .collect();
    let second: Vec<[f64; 2]> = d2u
        .into_iter()
        .zip(d2v)
        .map(|(x, y)| [x.get(), y.get()])
        .collect();
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
    sources: &[Vec<SourceExtent>],
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
        (ConsolidatedFamily::B, 0x27) => {
            decode_b2_plane_in_sources(data, sources, carrier, b2_planes)
        }
        (ConsolidatedFamily::B, 0x2e) => {
            let carrier_index =
                journal.index(carrier.source_index(), carrier.source_range().start)?;
            let payload = carrier.payload_bytes(data, sources)?;
            let mut at = usize::from(payload.first() == Some(&0x81));
            let inner_ref = allocation_reference(&payload, &mut at)?;
            let inner = resolve_ref(journal, carrier_index, inner_ref)?;
            resolve_b2_plane(
                data,
                sources,
                journal,
                inner,
                b2_planes,
                b2_offsets,
                depth + 1,
            )
        }
        (ConsolidatedFamily::B, 0x30 | 0x31) => {
            let carrier_index =
                journal.index(carrier.source_index(), carrier.source_range().start)?;
            let payload = carrier.payload_bytes(data, sources)?;
            let mut at = 0;
            let base_ref = allocation_reference(&payload, &mut at)?;
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
            let plane = resolve_b2_plane(
                data,
                sources,
                journal,
                base,
                b2_planes,
                b2_offsets,
                depth + 1,
            )?;
            let axis = *plane.frame().axis().as_raw();
            let origin = FinitePoint3::new(plane.origin().get().translated(axis, distance))?;
            Some(PlaneSurface::new(origin, *plane.frame()))
        }
        _ => None,
    }
}

pub(super) fn decode_b2_plane_in_sources(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
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
    let bytes = record.payload_bytes(data, sources)?;
    decode_bitmask_plane(&bytes).or_else(|| decode_uncompressed_axis_mask_plane(&bytes))
}

/// Decode a compact-header class-`0x16` 3D NURBS curve (`A_16` or `B_16`).
///
/// Handles single-span curves of degrees `1..=64` and multi-span curves of
/// degrees `1`, `3`, and `5` (with interior knot multiplicity `3` for degree 5
/// and `1` for degrees `1` and `3`), polynomial (`0x01`) or rational weights,
/// and tails carrying either the full knot domain, zero-origin full span, or
/// terminal-span local domain alongside the identity affine map `[1.0, 0.0]`.
fn decode_compact_nurbs_curve(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    if record.class() != 0x16 {
        return None;
    }
    let payload_cow = record.payload_bytes(data, sources)?;
    let payload: &[u8] = &payload_cow;
    let mut at = 0usize;
    let degree = crate::wire::bytes::compact_int(payload, &mut at)?;
    let knot_count = usize::try_from(crate::wire::bytes::compact_int(payload, &mut at)?).ok()?;
    if !(1..=64).contains(&degree) || knot_count < 2 || payload.get(at) != Some(&0x0c) {
        return None;
    }
    at += 1;
    let interior_mult = match degree {
        _ if knot_count == 2 => 1usize,
        1 | 3 => 1usize,
        5 => 3usize,
        _ => return None,
    };
    let deg_usize = usize::try_from(degree).ok()?;
    let control_count = deg_usize
        .checked_add(1)?
        .checked_add(interior_mult.checked_mul(knot_count.checked_sub(2)?)?)?;
    if knot_count
        .checked_mul(8)?
        .checked_add(control_count.checked_mul(24)?)?
        .checked_add(at)?
        > payload.len()
    {
        return None;
    }
    let mut distinct = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        distinct.push(crate::wire::bytes::f64_le(payload, at)?.get());
        at += 8;
    }
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }
    let mode = *payload.get(at)?;
    if mode != 0x01 {
        return None;
    }
    at += 1;
    let mut control_points = Vec::with_capacity(control_count);
    for _ in 0..control_count {
        control_points.push(crate::wire::bytes::f64_point(payload, at)?.get());
        at += 24;
    }
    let start_span = crate::wire::bytes::compact_int(payload, &mut at)?;
    let end_span = crate::wire::bytes::compact_int(payload, &mut at)?;
    let span_count = u32::try_from(knot_count.checked_sub(1)?).ok()?;
    if start_span != 1 || end_span != span_count {
        return None;
    }
    let dom0 = crate::wire::bytes::f64_le(payload, at)?.get();
    let dom1 = crate::wire::bytes::f64_le(payload, at.checked_add(8)?)?.get();
    let scale = crate::wire::bytes::f64_le(payload, at.checked_add(16)?)?.get();
    let offset = crate::wire::bytes::f64_le(payload, at.checked_add(24)?)?.get();
    at = at.checked_add(32)?;
    if scale.to_bits() != 1.0_f64.to_bits()
        || offset.to_bits() != 0.0_f64.to_bits()
        || payload.get(at..) != Some(&[0x00, 0x07])
    {
        return None;
    }
    let k_first = distinct[0];
    let k_last = distinct[knot_count - 1];
    let k_prev = distinct[knot_count - 2];
    let last_span = k_last - k_prev;
    let k_scale = 1.0_f64.max(k_last.abs()).max(k_prev.abs());
    let domain_ok = (dom0.to_bits() == k_first.to_bits() && dom1.to_bits() == k_last.to_bits())
        || (dom0.to_bits() == 0.0_f64.to_bits() && dom1.to_bits() == k_last.to_bits())
        || (dom0.to_bits() == 0.0_f64.to_bits()
            && (dom1 - last_span).abs() <= TERMINAL_SPAN_DOMAIN_TOLERANCE * k_scale);
    if !domain_ok {
        return None;
    }
    let mut knots = Vec::with_capacity(control_count + deg_usize + 1);
    for (index, &knot) in distinct.iter().enumerate() {
        let mult = if index == 0 || index + 1 == knot_count {
            deg_usize + 1
        } else {
            interior_mult
        };
        knots.extend(std::iter::repeat_n(knot, mult));
    }
    crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(degree, knots, control_points, None, false),
        refusal,
        format_args!(
            "compact 0x16 NURBS curve record at byte {}",
            record.byte_offset()
        ),
    )
}

/// Decode an uncompressed `b2 03 27` plane carrier storing a 24-byte `f64le`
/// origin, a 1-byte axis bitmask (`0xc0 | zero_mask` unbounded, or `0x40 |
/// zero_mask` followed by 4 trailing `f64le` UV bounds), and only the non-zero
/// `f64le` axis components (`ux, uy, uz, vx, vy, vz` in bits `0..6`).
fn decode_uncompressed_axis_mask_plane(bytes: &[u8]) -> Option<PlaneSurface> {
    if bytes.len() < 25 {
        return None;
    }
    let mask = bytes[24];
    let has_bounds = match mask & 0xc0 {
        0xc0 => false,
        0x40 => true,
        _ => return None,
    };
    let zero_mask = mask & 0x3f;
    let stored_components = 6usize.checked_sub(usize::try_from(zero_mask.count_ones()).ok()?)?;
    let bounds_bytes = if has_bounds { 32usize } else { 0usize };
    let expected_len = 25usize
        .checked_add(stored_components.checked_mul(8)?)?
        .checked_add(bounds_bytes)?;
    if bytes.len() != expected_len {
        return None;
    }
    let mut view = View::over_retained(bytes);
    let origin = FinitePoint3::new(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))?;
    let _mask = view.u8()?;
    let mut components = [0.0_f64; 6];
    for (bit, slot) in components.iter_mut().enumerate() {
        if (zero_mask & (1u8 << bit)) == 0 {
            let value = view.f64_le()?;
            if !value.is_finite() {
                return None;
            }
            *slot = value;
        }
    }
    if has_bounds {
        for _ in 0..4 {
            if !view.f64_le()?.is_finite() {
                return None;
            }
        }
    }
    if !view.is_empty() {
        return None;
    }
    let u = UnitVector3::new(Vector3::new(components[0], components[1], components[2]))?;
    let v = UnitVector3::new(Vector3::new(components[3], components[4], components[5]))?;
    Some(PlaneSurface::new(
        origin,
        OrthonormalFrame3::completing(u, v)?,
    ))
}

/// Decode an uncompressed `u32le`-header class-`0x34` NURBS surface (`A_34`).
///
/// Each axis carries `(degree: u32le, distinct_count: u32le)`, a knot marker
/// (`0x04`, or `0x00` + `u32le` flag), strictly increasing `f64le` knots, and
/// a multiplicity mode (`0x01` standard, or `0x00` explicit `u32le` vector).
/// Follows with `[mode: u16le]` (`0` polynomial, `1` rational with inline
/// `f64le` weights), `u_count * v_count` poles, and a `>= 152`-byte tail
/// carrying `[u_lo, u_hi, v_lo, v_hi]` (`u16le`), the 8-scalar chart block with
/// identity affine map `[1.0, 0.0, 1.0, 0.0]`, and terminal `fe ff fe ff`.
pub(super) fn decode_u32_nurbs_surface(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsSurface> {
    if record.family() != ConsolidatedFamily::A || record.class() != 0x34 {
        return None;
    }
    let payload = record.payload_bytes(data, sources)?;
    let mut view = View::over_retained(&payload);
    let (u_degree, u_knots, u_count) = read_u32_nurbs_axis(&mut view)?;
    let (v_degree, v_knots, v_count) = read_u32_nurbs_axis(&mut view)?;
    let mode = [view.u8()?, view.u8()?];
    let rational = match mode {
        [0x00, 0x00] => false,
        [0x01, 0x00] => true,
        _ => return None,
    };
    let poles = crate::nurbs_surface_control_count(u_count, v_count)?;
    let pole_bytes = poles.checked_mul(24)?;
    let weight_bytes = if rational { poles.checked_mul(8)? } else { 0 };
    if pole_bytes.checked_add(weight_bytes)?.checked_add(152)? > view.remaining() {
        return None;
    }
    let mut raw_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        let x = view.f64_le()?;
        let y = view.f64_le()?;
        let z = view.f64_le()?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return None;
        }
        raw_points.push(Point3::new(x, y, z));
    }
    let raw_weights = if rational {
        let mut weights = Vec::with_capacity(poles);
        for _ in 0..poles {
            let weight = view.f64_le()?;
            if !weight.is_finite() || weight <= 0.0 {
                return None;
            }
            weights.push(weight);
        }
        Some(weights)
    } else {
        None
    };
    let tail = view.take(view.remaining())?;
    if tail.len() < 152 || tail.get(tail.len() - 4..) != Some(&[0xfe, 0xff, 0xfe, 0xff]) {
        return None;
    }
    let mut tail_view = View::over_retained(tail);
    let u_lo = tail_view.u16_le()?;
    let u_hi = tail_view.u16_le()?;
    let v_lo = tail_view.u16_le()?;
    let v_hi = tail_view.u16_le()?;
    if u_lo != 1 || v_lo != 1 || u_hi == 0 || v_hi == 0 {
        return None;
    }
    let mut parameters = [0.0_f64; 8];
    for slot in &mut parameters {
        let value = tail_view.f64_le()?;
        if !value.is_finite() {
            return None;
        }
        *slot = value;
    }
    if parameters[4..] != [1.0, 0.0, 1.0, 0.0] {
        return None;
    }
    let grid = if let Some(weights) = raw_weights {
        let mut rows = Vec::with_capacity(u_count);
        for u in 0..u_count {
            let mut row = Vec::with_capacity(v_count);
            for v in 0..v_count {
                let idx = u * v_count + v;
                row.push(cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                    point: raw_points[idx],
                    weight: cadmpeg_ir::scalar::NonZeroReal::new(weights[idx])?,
                });
            }
            rows.push(row);
        }
        NurbsPoleGrid::Rational { rows }
    } else {
        let mut rows = Vec::with_capacity(u_count);
        for u in 0..u_count {
            let start = u * v_count;
            rows.push(raw_points[start..start + v_count].to_vec());
        }
        NurbsPoleGrid::Polynomial { rows }
    };
    crate::nurbs::note_refusal(
        NurbsSurface::new(
            NurbsSurfaceAxis::new(u_degree, u_knots, false),
            NurbsSurfaceAxis::new(v_degree, v_knots, false),
            grid,
            false,
        ),
        refusal,
        format_args!(
            "u32le A_34 NURBS surface record at byte {}",
            record.byte_offset()
        ),
    )
}

fn read_u32_nurbs_axis(view: &mut View<'_>) -> Option<(u32, Vec<f64>, usize)> {
    let degree = view.u32_le()?;
    let distinct_count = usize::try_from(view.u32_le()?).ok()?;
    if !(1..=64).contains(&degree)
        || distinct_count < 2
        || distinct_count.checked_mul(8)? > view.remaining()
    {
        return None;
    }
    match view.u8()? {
        0x04 => {}
        0x00 => {
            let _flag = view.u32_le()?;
        }
        _ => return None,
    }
    let mut distinct = Vec::with_capacity(distinct_count);
    for _ in 0..distinct_count {
        let knot = view.f64_le()?;
        if !knot.is_finite() {
            return None;
        }
        distinct.push(knot);
    }
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }
    let deg_usize = usize::try_from(degree).ok()?;
    let multiplicities: Vec<usize> = match view.u8()? {
        0x01 => {
            let interior = match degree {
                _ if distinct_count == 2 => 1usize,
                1..=3 => 1usize,
                5 => 3usize,
                _ => return None,
            };
            (0..distinct_count)
                .map(|index| {
                    if index == 0 || index + 1 == distinct_count {
                        deg_usize + 1
                    } else {
                        interior
                    }
                })
                .collect()
        }
        0x00 => {
            if distinct_count.checked_mul(4)? > view.remaining() {
                return None;
            }
            let mut mults = Vec::with_capacity(distinct_count);
            for _ in 0..distinct_count {
                let mult = usize::try_from(view.u32_le()?).ok()?;
                if mult == 0 || mult > deg_usize + 1 {
                    return None;
                }
                mults.push(mult);
            }
            mults
        }
        _ => return None,
    };
    let total_knots = multiplicities
        .iter()
        .try_fold(0usize, |acc, &m| acc.checked_add(m))?;
    let pole_count = total_knots.checked_sub(deg_usize.checked_add(1)?)?;
    if pole_count <= deg_usize {
        return None;
    }
    let mut knots = Vec::with_capacity(total_knots);
    for (&knot, &mult) in distinct.iter().zip(&multiplicities) {
        knots.extend(std::iter::repeat_n(knot, mult));
    }
    Some((degree, knots, pole_count))
}

/// General bitmask decoder for `b2 03 27` plane payloads.
///
/// Bounded planes use `(layout & 0x8f) == 0x84` with `mask`:
/// - `mask & 0x07`: omitted zero coordinates of origin `(x, y, z)`
/// - `mask & 0x78`: U-axis zero-component mask (`0x10/0x20/0x40`) + unit flag (`0x08`)
/// - `(layout & 0x70) | ((mask & 0x80) >> 4)`: V-axis zero-component mask + unit flag
///
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
    sources: &[Vec<SourceExtent>],
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
        let Some(payload) = record.payload_bytes(data, sources) else {
            continue;
        };
        let Some(count) = payload.first().and_then(|byte| byte.checked_sub(0x80)) else {
            continue;
        };
        if count < 2 {
            continue;
        }
        let mut at = 1;
        let Some(carrier_ref) = allocation_reference(&payload, &mut at) else {
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
        AllocationReferenceEncoding::OwnedChild
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16
        | AllocationReferenceEncoding::TaggedU24 => journal.resolve_address(
            origin,
            journal
                .address(origin)?
                .checked_add(u64::from(reference.value))?
                .checked_add(1)?,
        ),
        AllocationReferenceEncoding::Selector2 => None,
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
        let mut frame = vec![
            0xb2,
            0x03,
            0x21,
            u8::try_from(payload.len()).expect("payload length fits in u8"),
            0x05,
        ];
        frame.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&frame);
        assert_eq!(records.len(), 1);
        let (support_ref, nurbs, range) =
            super::decode_u32_bspline_pcurve_in_sources(&frame, &[], &records[0])
                .expect("decoded 0x21 pcurve");
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

    #[test]
    fn compact_0x16_decodes_shifted_single_span_and_multispan_polynomial_curves() {
        // Single-span degree-5 B_16 curve with shifted knots [10.0, 25.0] and terminal-span tail [0.0, 15.0].
        let mut payload = vec![0x15, 0x09, 0x0c];
        for k in [10.0_f64, 25.0] {
            payload.extend_from_slice(&k.to_le_bytes());
        }
        payload.push(0x01);
        for i in 0..6 {
            payload.extend_from_slice(&(f64::from(i) * 3.0).to_le_bytes());
            payload.extend_from_slice(&0.0_f64.to_le_bytes());
            payload.extend_from_slice(&5.0_f64.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for v in [0.0_f64, 15.0, 1.0, 0.0] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        payload.extend_from_slice(&[0x00, 0x07]);
        let mut frame = vec![
            0xb2,
            0x03,
            0x16,
            u8::try_from(payload.len()).expect("payload length fits in u8"),
            0x05,
        ];
        frame.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&frame);
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let curve = super::decode_compact_nurbs_curve(&frame, &[], &records[0], &mut refusal)
            .expect("decoded shifted B_16 curve");
        assert_eq!(curve.degree(), 5);
        assert_eq!(curve.control_points().len(), 6);
        assert!(refusal.take_notes().is_empty());

        // Multi-span degree-5 A_16 curve with 4 knots [0, 10, 20, 22] and terminal-span tail [0.0, 2.0].
        let mut a_payload = vec![0x15, 0x11, 0x0c];
        for k in [0.0_f64, 10.0, 20.0, 22.0] {
            a_payload.extend_from_slice(&k.to_le_bytes());
        }
        a_payload.push(0x01);
        for i in 0..12 {
            a_payload.extend_from_slice(&(f64::from(i)).to_le_bytes());
            a_payload.extend_from_slice(&(f64::from(i) * 0.5).to_le_bytes());
            a_payload.extend_from_slice(&0.0_f64.to_le_bytes());
        }
        a_payload.extend_from_slice(&[0x05, 0x0d]);
        for v in [0.0_f64, 2.0, 1.0, 0.0] {
            a_payload.extend_from_slice(&v.to_le_bytes());
        }
        a_payload.extend_from_slice(&[0x00, 0x07]);
        let mut a_frame = vec![0xa5, 0x03, 0x16];
        a_frame.extend_from_slice(
            &u32::try_from(a_payload.len())
                .expect("payload length fits in u32")
                .to_le_bytes(),
        );
        a_frame.push(0x05);
        a_frame.extend_from_slice(&a_payload);
        let a_records = crate::wire::records::consolidated_records(&a_frame);
        let a_curve = super::decode_compact_nurbs_curve(&a_frame, &[], &a_records[0], &mut refusal)
            .expect("decoded multi-span A_16 curve");
        assert_eq!(a_curve.degree(), 5);
        assert_eq!(a_curve.control_points().len(), 12);
        assert!(refusal.take_notes().is_empty());
    }

    #[test]
    fn uncompressed_axis_mask_plane_decodes_full_and_elided_zero_axes() {
        const TOL: f64 = 1.0e-12;
        // Full 73-byte plane (mask 0xc0): u=(1,0,0), v=(0,1,0) -> normal +Z.
        let mut full = Vec::new();
        for v in [10.0_f64, 20.0, 30.0] {
            full.extend_from_slice(&v.to_le_bytes());
        }
        full.push(0xc0);
        for v in [1.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0] {
            full.extend_from_slice(&v.to_le_bytes());
        }
        let plane = super::decode_uncompressed_axis_mask_plane(&full).expect("73-byte plane");
        assert!(plane.origin().get().distance(Point3::new(10.0, 20.0, 30.0)) < TOL);
        assert!((plane.frame().axis().as_raw().z - 1.0).abs() < TOL);

        // Elided 49-byte plane (mask 0xf1: ux, vy, vz omitted): u=(0, 0.6, -0.8), v=(-1, 0, 0).
        let mut elided = Vec::new();
        for v in [1.0_f64, 2.0, 3.0] {
            elided.extend_from_slice(&v.to_le_bytes());
        }
        elided.push(0xf1);
        for v in [0.6_f64, -0.8, -1.0] {
            elided.extend_from_slice(&v.to_le_bytes());
        }
        let plane2 = super::decode_uncompressed_axis_mask_plane(&elided).expect("49-byte plane");
        let n = plane2.frame().axis().as_raw();
        assert!(n.x.abs() < TOL);
        assert!((n.y - 0.8).abs() < TOL);
        assert!((n.z - 0.6).abs() < TOL);

        // Bounded 105-byte plane (mask 0x40: all 6 axis components + 4 UV bounds).
        let mut bounded = full.clone();
        bounded[24] = 0x40;
        for b in [0.0_f64, 100.0, 0.0, 50.0] {
            bounded.extend_from_slice(&b.to_le_bytes());
        }
        assert_eq!(bounded.len(), 105);
        let plane3 = super::decode_uncompressed_axis_mask_plane(&bounded).expect("105-byte plane");
        assert!(
            plane3
                .origin()
                .get()
                .distance(Point3::new(10.0, 20.0, 30.0))
                < TOL
        );
        assert!((plane3.frame().axis().as_raw().z - 1.0).abs() < TOL);
    }

    #[test]
    fn u32_a34_decodes_polynomial_rational_and_explicit_multiplicity_surfaces() {
        // Build a degree (1, 1) rational 2x2 u32le A_34 surface with explicit V multiplicities.
        let mut payload = Vec::new();
        // U axis: deg=1, distinct=2, marker=0x04, knots=[0.0, 10.0], mult_mode=0x01
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.push(0x04);
        for k in [0.0_f64, 10.0] {
            payload.extend_from_slice(&k.to_le_bytes());
        }
        payload.push(0x01);
        // V axis: deg=1, distinct=2, marker=0x00 + flag 2, knots=[0.0, 5.0], mult_mode=0x00, mults=[2, 2]
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.push(0x00);
        payload.extend_from_slice(&2_u32.to_le_bytes());
        for k in [0.0_f64, 5.0] {
            payload.extend_from_slice(&k.to_le_bytes());
        }
        payload.push(0x00);
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        // Mode: rational [1, 0]
        payload.extend_from_slice(&[0x01, 0x00]);
        for p in [
            [0.0_f64, 0.0, 0.0],
            [0.0, 5.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 5.0, 1.0],
        ] {
            for c in p {
                payload.extend_from_slice(&c.to_le_bytes());
            }
        }
        for w in [1.0_f64, 1.0, 1.0, 1.0] {
            payload.extend_from_slice(&w.to_le_bytes());
        }
        // 152-byte tail: [1,1,1,1] u16 + [0,10,0,5,1,0,1,0] f64 + 76 zero bytes + fe ff fe ff
        for h in [1_u16, 1, 1, 1] {
            payload.extend_from_slice(&h.to_le_bytes());
        }
        for v in [0.0_f64, 10.0, 0.0, 5.0, 1.0, 0.0, 1.0, 0.0] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        payload.extend_from_slice(&[0x00; 76]);
        payload.extend_from_slice(&[0xfe, 0xff, 0xfe, 0xff]);

        let mut frame = vec![0xa5, 0x03, 0x34];
        frame.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("payload length fits in u32")
                .to_le_bytes(),
        );
        frame.push(0x05);
        frame.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&frame);
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let surf = super::decode_u32_nurbs_surface(&frame, &[], &records[0], &mut refusal)
            .expect("decoded u32le A_34 surface");
        assert_eq!(surf.u_degree(), 1);
        assert_eq!(surf.v_degree(), 1);
        assert_eq!(surf.u_count(), 2);
        assert_eq!(surf.v_count(), 2);
        assert!(refusal.take_notes().is_empty());
    }

    #[test]
    fn u32_jet_pcurve_decodes_quintic_lanes_with_second_derivative_marker() {
        const TOL: f64 = 1.0e-12;
        let mut payload = vec![0x05]; // support_ref = BackwardDistance(1)
        payload.extend_from_slice(&5_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.push(0x04);
        for k in [0.0_f64, 10.0] {
            payload.extend_from_slice(&k.to_le_bytes());
        }
        payload.push(0x01);
        payload.extend_from_slice(&2_u32.to_le_bytes());
        // 4 lanes: u, v, du, dv
        for v in [0.0_f64, 10.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        // Second-derivative flag [0x01, 0x00] + 2 lanes: d2u, d2v
        payload.extend_from_slice(&[0x01, 0x00]);
        for v in [0.0_f64, 0.0, 0.0, 0.0] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        // Active range [0.0, 10.0] + fe ff
        for v in [0.0_f64, 10.0] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        payload.extend_from_slice(&[0xfe, 0xff]);
        let mut frame = vec![
            0xb2,
            0x03,
            0x20,
            u8::try_from(payload.len()).expect("payload length fits in u8"),
            0x05,
        ];
        frame.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&frame);
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let (support_ref, _pcurve, range) =
            super::decode_u32_jet_pcurve_in_sources(&frame, &[], &records[0], &mut refusal)
                .expect("decoded u32le 0x20 quintic jet pcurve");
        assert_eq!(support_ref.value, 1);
        assert!((range[0] - 0.0).abs() < TOL);
        assert!((range[1] - 10.0).abs() < TOL);
        assert!(refusal.take_notes().is_empty());
    }
}
