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
const IDENTITY_AFFINE_SCALE_TOLERANCE: f64 = 1.0e-12;
const MIN_REVOLUTION_SPAN_RAD: f64 = 1.0e-9;
const REVOLUTION_FULL_TURN_TOLERANCE_RAD: f64 = 1.0e-6;
const MIN_RULED_V_SPAN: f64 = 1.0e-6;
const MAX_ALIAS_DEPTH: usize = 8;
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
) -> HashMap<u32, PlaneSurface> {
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

    let mut journal_planes = HashMap::new();
    for (index, record) in surface_records.iter().enumerate() {
        let StandardSurfaceRecord::Analytic(prefix) = record else {
            continue;
        };
        if prefix.kind != AnalyticSurfaceKind::Plane {
            continue;
        }
        let bounds = parsed_bounds[index];
        if let Some(Some((_, carrier))) = face_carriers.get(&u64::from(prefix.target)) {
            if let Some(plane) =
                resolve_b2_plane(data, sources, &journal, carrier, &b2_planes, &b2_offsets, 0)
            {
                let axis = *plane.frame().axis().as_raw();
                let bounds_ok = bounds.is_none_or(|b| {
                    let center = Point3::new(
                        b.sphere_center[0].get(),
                        b.sphere_center[1].get(),
                        b.sphere_center[2].get(),
                    );
                    center.vector_from(plane.origin().get()).dot(axis).abs()
                        <= PLANE_BOUNDS_TOLERANCE_MM
                });
                if bounds_ok {
                    journal_planes.insert(prefix.target, plane);
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        normals.entry(prefix.target)
                    {
                        if let Some(vector) = FiniteVector::new([axis.x, axis.y, axis.z]) {
                            entry.insert(vector);
                            continue;
                        }
                    }
                }
            }
        }
        if normals.contains_key(&prefix.target) {
            continue;
        }
        let Some(bounds) = bounds else {
            continue;
        };
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
    journal_planes
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
    let mut b2_cones: HashMap<_, _> = b2_cones_from_records(data, records)
        .map(|cone| (cone.pos, cone))
        .collect();
    for (cone, _) in crate::families::b2::records::owned_cone_charts(data, records) {
        b2_cones.entry(cone.pos).or_insert(cone);
    }
    let b2_spheres: HashMap<_, _> = b2_spheres_from_records(data, records)
        .map(|sphere| (sphere.pos, sphere))
        .collect();
    let mut b2_tori: HashMap<_, _> = b2_tori_from_records(data, records)
        .map(|torus| (torus.pos, torus))
        .collect();
    for (torus, _) in crate::families::b2::records::owned_torus_charts(data, records) {
        b2_tori.entry(torus.pos).or_insert(torus);
    }
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
    #[inline(never)]
    fn resolve_surface(
        &self,
        _origin_index: usize,
        mut carrier: &'a ConsolidatedRecord,
        refusal: &mut crate::nurbs::LaneRefusals,
        mut depth: usize,
    ) -> Option<SolvedSurfaceGeometry> {
        let mut carrier_index = self
            .journal
            .index(carrier.source_index(), carrier.source_range().start)?;
        loop {
            if depth > MAX_ALIAS_DEPTH {
                return None;
            }
            match (carrier.family(), carrier.class()) {
                (ConsolidatedFamily::B, 0x2e) => {
                    let payload = carrier.payload_bytes(self.data, self.sources)?;
                    let mut at = usize::from(payload.first() == Some(&0x81));
                    let inner_ref = allocation_reference(&payload, &mut at)?;
                    carrier = resolve_ref(self.journal, carrier_index, inner_ref)?;
                    carrier_index = self
                        .journal
                        .index(carrier.source_index(), carrier.source_range().start)?;
                    depth += 1;
                }
                (ConsolidatedFamily::B, 0x37 | 0x3b) => {
                    let payload = carrier.payload_bytes(self.data, self.sources)?;
                    if payload.first() != Some(&0x85) {
                        return None;
                    }
                    let mut at = 1;
                    let inner_ref = allocation_reference(&payload, &mut at)?;
                    carrier = resolve_ref(self.journal, carrier_index, inner_ref)?;
                    carrier_index = self
                        .journal
                        .index(carrier.source_index(), carrier.source_range().start)?;
                    depth += 1;
                }
                (ConsolidatedFamily::B, 0x38) => {
                    let payload = carrier.payload_bytes(self.data, self.sources)?;
                    if payload.first() != Some(&0x81) {
                        return None;
                    }
                    let mut at = 1;
                    let inner_ref = allocation_reference(&payload, &mut at)?;
                    carrier = resolve_ref(self.journal, carrier_index, inner_ref)?;
                    carrier_index = self
                        .journal
                        .index(carrier.source_index(), carrier.source_range().start)?;
                    depth += 1;
                }
                _ => break,
            }
        }
        match (carrier.family(), carrier.class()) {
            (ConsolidatedFamily::A, 0x32) => {
                decode_rolling_ball_surface(self.data, self.sources, carrier)
                    .map(SolvedSurfaceGeometry::Nurbs)
            }
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
            (ConsolidatedFamily::A, 0x39) => {
                decode_ruled_guide_surface(self.data, self.sources, carrier)
                    .map(SolvedSurfaceGeometry::Nurbs)
            }
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
                })
                .or_else(|| {
                    let payload = carrier.payload_bytes(self.data, self.sources)?;
                    decode_axis_mask_cylinder(&payload).map(SolvedSurfaceGeometry::Cylinder)
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
            (ConsolidatedFamily::B, 0x2d) => self
                .decode_revolution(carrier_index, carrier, refusal, depth + 1)
                .map(SolvedSurfaceGeometry::Nurbs),
            (ConsolidatedFamily::B, 0x30 | 0x31) => {
                let payload = carrier.payload_bytes(self.data, self.sources)?;
                if payload.first() == Some(&0x82) {
                    let mut at = 1;
                    if let Some(target_ref) = allocation_reference(&payload, &mut at) {
                        if let Some(target) = resolve_ref(self.journal, carrier_index, target_ref) {
                            if let Some(solved) =
                                self.resolve_surface(carrier_index, target, refusal, depth + 1)
                            {
                                return Some(solved);
                            }
                        }
                    }
                }
                self.decode_offset(carrier_index, carrier, refusal, depth + 1)
            }
            (ConsolidatedFamily::B, 0x39) => {
                let payload = carrier.payload_bytes(self.data, self.sources)?;
                decode_bitmask_plane(&payload)
                    .or_else(|| decode_uncompressed_axis_mask_plane(&payload))
                    .map(SolvedSurfaceGeometry::Plane)
            }
            _ => None,
        }
    }

    #[inline(never)]
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
                parse_offset_tail(&payload, at)?
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

    #[inline(never)]
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
        let tail_valid = if tail.len() == 8 {
            true
        } else {
            let mut tail_at = 0;
            crate::wire::bytes::compact_int(tail, &mut tail_at).is_some()
                && crate::wire::bytes::compact_int(tail, &mut tail_at).is_some()
                && tail_at == tail.len()
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

    #[inline(never)]
    fn decode_revolution(
        &self,
        carrier_index: usize,
        carrier: &'a ConsolidatedRecord,
        refusal: &mut crate::nurbs::LaneRefusals,
        depth: usize,
    ) -> Option<NurbsSurface> {
        let payload = carrier.payload_bytes(self.data, self.sources)?;
        let mut at = 0;
        let profile_ref = allocation_reference(&payload, &mut at)?;
        let mut view = View::over_retained(payload.get(at..)?);
        let origin = Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?);
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            return None;
        }
        let dir_x = ExactUnitVector3::new([view.f64_le()?, view.f64_le()?, view.f64_le()?])?;
        let dir_y = ExactUnitVector3::new([view.f64_le()?, view.f64_le()?, view.f64_le()?])?;
        let axis = ExactUnitVector3::new([view.f64_le()?, view.f64_le()?, view.f64_le()?])?;
        crate::checked::UnitFrame3::right_handed(dir_x, dir_y, axis)?;
        let ang_lo = view.f64_le()?;
        let ang_hi = view.f64_le()?;
        let prof_lo = view.f64_le()?;
        let prof_hi = view.f64_le()?;
        let ang_bounds = IncreasingParameterInterval::new([ang_lo, ang_hi])?.endpoints();
        let prof_bounds = IncreasingParameterInterval::new([prof_lo, prof_hi])?.endpoints();
        // Compact form uses [0x05, 0x05] + 4 f64 + 0x01 + f64 (43 bytes);
        // uncompressed form uses [1u32, 1u32] + 4 f64 + u32 + f64 (52 bytes).
        let (angular_scale, mean_angle) = if view.remaining() == 43 {
            if view.take(2)? != [0x05, 0x05] {
                return None;
            }
            let scale = view.f64_le()?;
            let s1 = view.f64_le()?;
            let s2 = view.f64_le()?;
            let off = view.f64_le()?;
            if s1.to_bits() != 1.0_f64.to_bits()
                || s2.to_bits() != 1.0_f64.to_bits()
                || off.to_bits() != 0.0_f64.to_bits()
                || view.u8()? != 0x01
            {
                return None;
            }
            (scale, view.f64_le()?)
        } else if view.remaining() == 52 {
            let c0 = view.u32_le()?;
            let c1 = view.u32_le()?;
            if c0 != 1 || c1 != 1 {
                return None;
            }
            let scale = view.f64_le()?;
            let s1 = view.f64_le()?;
            let s2 = view.f64_le()?;
            let off = view.f64_le()?;
            let flag = view.u32_le()?;
            if s1.to_bits() != 1.0_f64.to_bits()
                || s2.to_bits() != 1.0_f64.to_bits()
                || off.to_bits() != 0.0_f64.to_bits()
                || flag > 1
            {
                return None;
            }
            (scale, view.f64_le()?)
        } else {
            return None;
        };
        if !angular_scale.is_finite() || angular_scale <= 0.0 || !mean_angle.is_finite() {
            return None;
        }
        let angle_lo = ang_bounds[0] / angular_scale;
        let angle_hi = ang_bounds[1] / angular_scale;
        let angle_span = angle_hi - angle_lo;
        if !angle_span.is_finite()
            || angle_span <= MIN_REVOLUTION_SPAN_RAD
            || angle_span > std::f64::consts::TAU + REVOLUTION_FULL_TURN_TOLERANCE_RAD
        {
            return None;
        }
        let profile_record = resolve_ref(self.journal, carrier_index, profile_ref)?;
        let profile_index = self.journal.index(
            profile_record.source_index(),
            profile_record.source_range().start,
        )?;
        let profile = self.decode_3d_curve(
            profile_index,
            profile_record,
            Some(prof_bounds),
            refusal,
            depth,
        )?;
        revolve_nurbs_curve(&profile, origin, UnitVector3::from(axis), [0.0, angle_span])
    }

    #[inline(never)]
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
                                if dview.remaining() == 9 {
                                    if dview.u8()? != 0x85 {
                                        return None;
                                    }
                                    let hi = dview.f64_le()?;
                                    Some(IncreasingParameterInterval::new([0.0, hi])?.endpoints())
                                } else {
                                    if dview.remaining() == 17 && dview.u8()? != 0x84 {
                                        return None;
                                    }
                                    let lo = dview.f64_le()?;
                                    let hi = dview.f64_le()?;
                                    if !dview.is_empty() {
                                        return None;
                                    }
                                    Some(IncreasingParameterInterval::new([lo, hi])?.endpoints())
                                }
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
                        const DUAL_ENDPOINT_TOL_MM: f64 = 0.05;
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
                                .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
                        };
                        let p1a = eval_end(&c1, range1[0])?;
                        let p1b = eval_end(&c1, range1[1])?;
                        let p2a = eval_end(&c2, range2[0])?;
                        let p2b = eval_end(&c2, range2[1])?;
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
                .or_else(|| decode_compact_nurbs_curve(self.data, self.sources, record, refusal))
                .or_else(|| decode_u32_nurbs_curve(self.data, self.sources, record, refusal)),
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x21) => {
                let payload = record.payload_bytes(self.data, self.sources)?;
                let (support_ref, pcurve_nurbs, range) =
                    decode_u32_bspline_pcurve_payload(&payload)
                        .or_else(|| decode_compact_bspline_pcurve_payload(&payload))?;
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

/// Revolve a 3D `NurbsCurve` around `(origin, axis)` over `[a0, a1]` (radians)
/// to produce an exact rational tensor-product `NurbsSurface` of degree
/// `(curve.degree(), 2)`.
pub(in crate::families) fn revolve_nurbs_curve(
    curve: &NurbsCurve,
    origin: Point3,
    axis: UnitVector3,
    [a0, a1]: [f64; 2],
) -> Option<NurbsSurface> {
    let span = a1 - a0;
    if !a0.is_finite()
        || !a1.is_finite()
        || !span.is_finite()
        || span <= MIN_REVOLUTION_SPAN_RAD
        || span > std::f64::consts::TAU + REVOLUTION_FULL_TURN_TOLERANCE_RAD
    {
        return None;
    }
    let segments = if span <= std::f64::consts::FRAC_PI_2 {
        1usize
    } else if span <= std::f64::consts::PI {
        2usize
    } else if span <= 1.5 * std::f64::consts::PI {
        3usize
    } else {
        4usize
    };
    let seg_count_f = match segments {
        1 => 1.0,
        2 => 2.0,
        3 => 3.0,
        _ => 4.0,
    };
    let seg_angle = span / seg_count_f;
    let half = 0.5 * seg_angle;
    let cos_half = half.cos();
    if !cos_half.is_finite() || cos_half <= 0.1 {
        return None;
    }
    let inv_cos = 1.0 / cos_half;

    let mut v_knots = Vec::with_capacity(2 * segments + 4);
    v_knots.extend_from_slice(&[a0, a0, a0]);
    for seg in 1..segments {
        let seg_f = match seg {
            1 => 1.0,
            2 => 2.0,
            _ => 3.0,
        };
        let knot = a0 + seg_f * seg_angle;
        v_knots.push(knot);
        v_knots.push(knot);
    }
    v_knots.extend_from_slice(&[a1, a1, a1]);

    let axis_vec = *axis.as_raw();
    let cols = 2 * segments + 1;
    let mut rows = Vec::with_capacity(curve.control_points().len());
    for (idx, pole) in curve.control_points().iter().enumerate() {
        let p = pole.get();
        let base_w = curve.weights().map_or(1.0, |weights| weights[idx].get());
        let rel = p.vector_from(origin);
        let axial = axis_vec.scale(rel.dot(axis_vec));
        let radial = rel - axial;
        let perp = axis_vec.cross(radial);
        let mut row = Vec::with_capacity(cols);
        for col in 0..cols {
            let half_steps = cadmpeg_core::convert::f64_from_index(col)?;
            let theta = a0 + half_steps * half;
            let (s, c) = theta.sin_cos();
            let rotated = radial.scale(c) + perp.scale(s);
            let is_mid = col % 2 == 1;
            let disp = if is_mid {
                axial + rotated.scale(inv_cos)
            } else {
                axial + rotated
            };
            let pt = Point3::new(origin.x + disp.x, origin.y + disp.y, origin.z + disp.z);
            let w_val = if is_mid { base_w * cos_half } else { base_w };
            let weight = cadmpeg_ir::scalar::NonZeroReal::new(w_val)?;
            row.push(cadmpeg_ir::geometry::nurbs::WeightedPole3 { point: pt, weight });
        }
        rows.push(row);
    }
    NurbsSurface::new(
        NurbsSurfaceAxis::new(curve.degree(), curve.knots().clone(), false),
        NurbsSurfaceAxis::new(2, v_knots, false),
        NurbsPoleGrid::Rational { rows },
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

/// Decode a compact-header class-`0x21` 2D B-spline pcurve (`A_21` or `B_21`)
/// storing `<support_ref> <0x1a|0x3a> <span_count> <chord_len:f64> <degree> <nknots> <marker> <knots> <2D poles> [tail]`.
fn decode_compact_bspline_pcurve_payload(
    payload: &[u8],
) -> Option<(AllocationReference, PcurveNurbs, [f64; 2])> {
    let mut at = 0;
    let support_ref = allocation_reference(payload, &mut at)?;
    let form = *payload.get(at)?;
    if !matches!(form, 0x1a | 0x3a) {
        return None;
    }
    at += 1;
    let span_count = usize::try_from(crate::wire::bytes::compact_int(payload, &mut at)?).ok()?;
    let chord_len = crate::wire::bytes::f64_le(payload, at)?.get();
    if !chord_len.is_finite() || chord_len < 0.0 {
        return None;
    }
    at = at.checked_add(8)?;
    let degree = crate::wire::bytes::compact_int(payload, &mut at)?;
    let nknots = usize::try_from(crate::wire::bytes::compact_int(payload, &mut at)?).ok()?;
    if !matches!(degree, 1 | 3 | 5) || !(2..=MAX_BSPLINE_PCURVE_KNOTS).contains(&nknots) {
        return None;
    }
    let marker = *payload.get(at)?;
    at += 1;
    let implicit_mults = match form {
        0x1a => {
            if span_count != nknots - 1 || marker != 0x0c {
                return None;
            }
            true
        }
        0x3a => {
            if span_count == 0 || span_count >= nknots {
                return None;
            }
            match marker {
                0x0c => true,
                0x08 => {
                    let _extra = *payload.get(at)?;
                    at += 1;
                    true
                }
                0x04 => false,
                0x00 => {
                    let _extra = *payload.get(at)?;
                    at += 1;
                    false
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let expect_trailing_byte = form == 0x3a;
    let deg_usize = usize::try_from(degree).ok()?;
    let knot_bytes = nknots.checked_mul(8)?;
    if at.checked_add(knot_bytes)? > payload.len() {
        return None;
    }
    let mut distinct = Vec::with_capacity(nknots);
    for _ in 0..nknots {
        distinct.push(crate::wire::bytes::f64_le(payload, at)?.get());
        at += 8;
    }
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }
    let multiplicities: Vec<usize> = if implicit_mults {
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
    } else {
        let mut mults = Vec::with_capacity(nknots);
        for _ in 0..nknots {
            let mult = usize::try_from(crate::wire::bytes::compact_int(payload, &mut at)?).ok()?;
            if mult == 0 || mult > deg_usize + 1 {
                return None;
            }
            mults.push(mult);
        }
        mults
    };
    let total_knots = multiplicities
        .iter()
        .try_fold(0usize, |acc, &m| acc.checked_add(m))?;
    let pole_count = total_knots.checked_sub(deg_usize.checked_add(1)?)?;
    if pole_count <= deg_usize {
        return None;
    }
    let expected_end = at.checked_add(pole_count.checked_mul(16)?)?;
    if expect_trailing_byte {
        if expected_end.checked_add(1)? != payload.len()
            || payload.get(expected_end).is_none_or(|b| b % 4 != 1)
        {
            return None;
        }
    } else if expected_end != payload.len() {
        return None;
    }
    let mut poles = Vec::with_capacity(pole_count);
    for _ in 0..pole_count {
        let u = crate::wire::bytes::f64_le(payload, at)?.get();
        let v = crate::wire::bytes::f64_le(payload, at + 8)?.get();
        poles.push(Point2::new(u, v));
        at += 16;
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
    if view.u8()? != 0x01 || usize::from(view.u16_le()?) != nknots {
        return None;
    }
    let _lane_flag = view.u16_le()?;
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

#[inline(never)]
fn resolve_b2_plane<'a>(
    data: &'a [u8],
    sources: &[Vec<SourceExtent>],
    journal: &AllocationJournal<'a>,
    mut carrier: &'a ConsolidatedRecord,
    b2_planes: &HashMap<usize, crate::families::b2::records::B2PlaneCarrier>,
    b2_offsets: &HashMap<usize, crate::families::b2::records::B2OffsetSupport>,
    mut depth: usize,
) -> Option<PlaneSurface> {
    loop {
        if depth > MAX_ALIAS_DEPTH {
            return None;
        }
        match (carrier.family(), carrier.class()) {
            (ConsolidatedFamily::B, 0x2e) => {
                let carrier_index =
                    journal.index(carrier.source_index(), carrier.source_range().start)?;
                let payload = carrier.payload_bytes(data, sources)?;
                let mut at = usize::from(payload.first() == Some(&0x81));
                let inner_ref = allocation_reference(&payload, &mut at)?;
                carrier = resolve_ref(journal, carrier_index, inner_ref)?;
                depth += 1;
            }
            (ConsolidatedFamily::B, 0x37 | 0x3b) => {
                let carrier_index =
                    journal.index(carrier.source_index(), carrier.source_range().start)?;
                let payload = carrier.payload_bytes(data, sources)?;
                if payload.first() != Some(&0x85) {
                    return None;
                }
                let mut at = 1;
                let inner_ref = allocation_reference(&payload, &mut at)?;
                carrier = resolve_ref(journal, carrier_index, inner_ref)?;
                depth += 1;
            }
            (ConsolidatedFamily::B, 0x38) => {
                let carrier_index =
                    journal.index(carrier.source_index(), carrier.source_range().start)?;
                let payload = carrier.payload_bytes(data, sources)?;
                if payload.first() != Some(&0x81) {
                    return None;
                }
                let mut at = 1;
                let inner_ref = allocation_reference(&payload, &mut at)?;
                carrier = resolve_ref(journal, carrier_index, inner_ref)?;
                depth += 1;
            }
            _ => break,
        }
    }
    match (carrier.family(), carrier.class()) {
        (ConsolidatedFamily::B, 0x27) => {
            decode_b2_plane_in_sources(data, sources, carrier, b2_planes)
        }
        (ConsolidatedFamily::B, 0x30 | 0x31) => {
            let carrier_index =
                journal.index(carrier.source_index(), carrier.source_range().start)?;
            let payload = carrier.payload_bytes(data, sources)?;
            if payload.first() == Some(&0x82) {
                let mut at = 1;
                if let Some(target_ref) = allocation_reference(&payload, &mut at) {
                    if let Some(target) = resolve_ref(journal, carrier_index, target_ref) {
                        if let Some(plane) = resolve_b2_plane(
                            data,
                            sources,
                            journal,
                            target,
                            b2_planes,
                            b2_offsets,
                            depth + 1,
                        ) {
                            return Some(plane);
                        }
                    }
                }
            }
            let mut at = 0;
            let base_ref = allocation_reference(&payload, &mut at)?;
            let base = resolve_ref(journal, carrier_index, base_ref)?;
            let distance = if let Some(parsed) = b2_offsets.get(&carrier.byte_offset()) {
                parsed.distance.get()
            } else {
                parse_offset_tail(&payload, at)?.0
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
        (ConsolidatedFamily::B, 0x39) => {
            let payload = carrier.payload_bytes(data, sources)?;
            decode_bitmask_plane(&payload).or_else(|| decode_uncompressed_axis_mask_plane(&payload))
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

/// Decode an uncompressed `u32le`-header class-`0x16` 3D NURBS curve (`A_16` or `B_16`).
fn decode_u32_nurbs_curve(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    if record.class() != 0x16 {
        return None;
    }
    let payload = record.payload_bytes(data, sources)?;
    let mut view = View::over_retained(&payload);
    let (degree, knots, pole_count) = read_u32_nurbs_axis(&mut view)?;
    let mode = [view.u8()?, view.u8()?];
    let rational = match mode {
        [0x00, 0x00] => false,
        [0x01, 0x00] => true,
        _ => return None,
    };
    let pole_bytes = pole_count.checked_mul(24)?;
    let weight_bytes = if rational {
        pole_count.checked_mul(8)?
    } else {
        0
    };
    if pole_bytes.checked_add(weight_bytes)?.checked_add(39)? > view.remaining() {
        return None;
    }
    let mut control_points = Vec::with_capacity(pole_count);
    for _ in 0..pole_count {
        let x = view.f64_le()?;
        let y = view.f64_le()?;
        let z = view.f64_le()?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return None;
        }
        control_points.push(Point3::new(x, y, z));
    }
    let weights = if rational {
        let mut raw = Vec::with_capacity(pole_count);
        for _ in 0..pole_count {
            let w = view.f64_le()?;
            if !w.is_finite() || w <= 0.0 {
                return None;
            }
            raw.push(w);
        }
        Some(raw)
    } else {
        None
    };
    let start_span = view.u16_le()?;
    let end_span = view.u16_le()?;
    if start_span != 1 || end_span == 0 {
        return None;
    }
    let dom0 = view.f64_le()?;
    let dom1 = view.f64_le()?;
    let scale = view.f64_le()?;
    let offset = view.f64_le()?;
    if !dom0.is_finite()
        || !dom1.is_finite()
        || scale.to_bits() != 1.0_f64.to_bits()
        || offset.to_bits() != 0.0_f64.to_bits()
    {
        return None;
    }
    let tail = view.take(view.remaining())?;
    if tail.len() < 3 || tail.get(tail.len() - 2..) != Some(&[0xfe, 0xff]) {
        return None;
    }
    crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(degree, knots, control_points, weights, false),
        refusal,
        format_args!(
            "u32le 0x16 NURBS curve record at byte {}",
            record.byte_offset()
        ),
    )
}

/// Decode a consolidated `a5 03 32` rolling-ball fillet surface (compact or
/// `u32le` header) and fit it as a bounded polynomial NURBS patch.
#[inline(never)]
fn decode_rolling_ball_surface(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
) -> Option<NurbsSurface> {
    if record.family() != ConsolidatedFamily::A || record.class() != 0x32 {
        return None;
    }
    let definition = crate::families::a5a8::records::a5_freeform_curves_from_records(
        data,
        std::slice::from_ref(record),
    )
    .into_iter()
    .next()
    .and_then(|jet| jet.definition())
    .or_else(|| {
        let payload = record.payload_bytes(data, sources)?;
        decode_u32_rolling_ball_definition(&payload)
    })?;
    let cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::RollingBallJet(
        ref jet,
    ) = definition
    else {
        return None;
    };
    let u_bounds = [
        jet.stations().first()?.knot.get(),
        jet.stations().last()?.knot.get(),
    ];
    if !u_bounds[0].is_finite() || !u_bounds[1].is_finite() || u_bounds[0] >= u_bounds[1] {
        return None;
    }
    super::owned_patch::surface_fit::rolling(&definition, [u_bounds, [0.0, 1.0]]).or_else(|| {
        super::rolling_ball_patch::fit_rectangular_patch(&definition, u_bounds)
            .map(|fit| fit.surface)
    })
}

fn decode_u32_rolling_ball_definition(
    payload: &[u8],
) -> Option<cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition> {
    let mut view = View::over_retained(payload);
    let count = usize::try_from(view.u32_le()?).ok()?;
    let degree = view.u32_le()?;
    let count_repeat = usize::try_from(view.u32_le()?).ok()?;
    if count < 2 || count != count_repeat || degree != 5 || count > MAX_BSPLINE_PCURVE_KNOTS {
        return None;
    }
    match view.u8()? {
        0x04 => {}
        0x00 => {
            let _flag = view.u32_le()?;
        }
        _ => return None,
    }
    let mut knots = Vec::with_capacity(count);
    let mut prev: Option<f64> = None;
    for _ in 0..count {
        let knot = cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?;
        if prev.is_some_and(|p| knot.get() <= p) {
            return None;
        }
        prev = Some(knot.get());
        knots.push(knot);
    }
    if view.u8()? != 0x01 {
        return None;
    }
    let block_bytes = count.checked_mul(80)?;
    if block_bytes.checked_mul(3)? > view.remaining() {
        return None;
    }
    let mut read_block = || -> Option<Vec<[cadmpeg_ir::scalar::FiniteReal; 10]>> {
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let mut row = [cadmpeg_ir::scalar::FiniteReal::ZERO; 10];
            for slot in &mut row {
                *slot = cadmpeg_ir::scalar::FiniteReal::new(view.f64_le()?)?;
            }
            rows.push(row);
        }
        Some(rows)
    };
    let positions = read_block()?;
    let first_derivs = read_block()?;
    let second_derivs = read_block()?;
    let mut stations = Vec::with_capacity(count);
    for i in 0..count {
        let site = crate::families::a5a8::records::rolling_ball_site(positions[i], true)?;
        stations.push(cadmpeg_ir::geometry::blends::RollingBallJetStation {
            knot: knots[i],
            multiplicity: 6,
            site: crate::families::a5a8::records::rolling_ball_jet_site(
                &site,
                first_derivs[i],
                second_derivs[i],
            ),
        });
    }
    cadmpeg_ir::geometry::blends::RollingBallJetStations::from_parts(5, stations)
        .ok()
        .map(cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::RollingBallJet)
}

/// Decode a consolidated `a5 03 39` ruled guide-curve surface (typically a
/// chamfer carrier) into a degree-`(5, 1)` polynomial NURBS surface.
#[inline(never)]
fn decode_ruled_guide_surface(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
) -> Option<NurbsSurface> {
    if record.family() != ConsolidatedFamily::A || record.class() != 0x39 {
        return None;
    }
    let guide = crate::families::a5a8::records::a5_ruled_guides_from_records(
        data,
        std::slice::from_ref(record),
    )
    .into_iter()
    .next()?;
    let knots = guide.knots();
    let u_bounds = [*knots.first()?, *knots.last()?];
    if let Some(payload) = record.payload_bytes(data, sources) {
        if let Some(tail_start) = payload.len().checked_sub(48) {
            if let Some(tail) = crate::wire::bytes::read_f64_array::<4>(&payload, tail_start) {
                let [u0, u1, v0, v1] = tail.map(cadmpeg_ir::scalar::FiniteReal::get);
                if v0 < v1
                    && (v1 - v0) > MIN_RULED_V_SPAN
                    && u0 <= u_bounds[1]
                    && u1 >= u_bounds[0]
                    && u0 < u1
                {
                    if let Some(ruled) = super::owned_patch::ruled::Ruled::new(&guide, [v0, v1]) {
                        if let Some(surface) = ruled.fit([[u0, u1], [v0, v1]]) {
                            return Some(surface);
                        }
                    }
                }
            }
        }
    }
    let points: Vec<[f64; 6]> = guide
        .sites
        .iter()
        .map(|s| {
            let [x, y, z] = s.point.get();
            let [rx, ry, rz] = s.reference_point.get();
            [x, y, z, rx, ry, rz]
        })
        .collect();
    let first: Vec<[f64; 6]> = guide
        .sites
        .iter()
        .map(|s| s.first_derivative.get())
        .collect();
    let second: Vec<[f64; 6]> = guide
        .sites
        .iter()
        .map(|s| s.second_derivative.get())
        .collect();
    let (bspline_knots, poles) =
        crate::nurbs::quintic_jet_bspline(guide.degree, &knots, &points, &first, &second)?;
    let rows: Vec<Vec<Point3>> = poles
        .into_iter()
        .map(|p| {
            let v = p.get();
            vec![Point3::new(v[0], v[1], v[2]), Point3::new(v[3], v[4], v[5])]
        })
        .collect();
    NurbsSurface::new(
        NurbsSurfaceAxis::new(guide.degree, bspline_knots, false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsPoleGrid::Polynomial { rows },
        false,
    )
    .ok()
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

/// Parse the scalar tail of a `B_30` or `B_31` offset carrier after its leading
/// `base_ref`:
/// - `B_31` compact form: `<distance:f64> <u0:f64> <v0:f64> <u1:f64> <v1:f64>` (40 bytes)
/// - `B_30` two-ref form: `<aux_ref:allocation_reference> <distance:f64> <0x01|0x15> <u0:f64> <u1:f64> <v0:f64> <v1:f64>`
fn parse_offset_tail(payload: &[u8], at: usize) -> Option<(f64, [f64; 2], [f64; 2])> {
    let tail = payload.get(at..)?;
    if tail.len() == 40 {
        let mut view = View::over_retained(tail);
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
        return Some((distance, u_bounds, v_bounds));
    }
    let mut aux_at = at;
    let _aux_ref = allocation_reference(payload, &mut aux_at)?;
    let mut view = View::over_retained(payload.get(aux_at..)?);
    if view.remaining() != 41 {
        return None;
    }
    let distance = view.f64_le()?;
    let tag = view.u8()?;
    if !matches!(tag, 0x01 | 0x15) || !distance.is_finite() {
        return None;
    }
    let u0 = view.f64_le()?;
    let u1 = view.f64_le()?;
    let v0 = view.f64_le()?;
    let v1 = view.f64_le()?;
    if !view.is_empty() {
        return None;
    }
    let u_bounds = IncreasingParameterInterval::new([u0, u1])?.endpoints();
    let v_bounds = IncreasingParameterInterval::new([v0, v1])?.endpoints();
    Some((distance, u_bounds, v_bounds))
}

/// Decode a consolidated `b2 03 28` cylinder carrier storing a 24-byte `f64le`
/// origin, a 6-bit zero-component axis mask for `(ref_x, ref_y)`, the non-zero
/// `f64le` frame components, `radius`, `u_range`, `v_range`, and either `0x07`
/// (standard) or `0x03 <range_origin:f64le>`.
fn decode_axis_mask_cylinder(
    bytes: &[u8],
) -> Option<cadmpeg_ir::geometry::analytic::CylinderSurface> {
    if bytes.len() < 66 {
        return None;
    }
    let mask = bytes[24];
    if (mask & 0xc0) != 0 {
        return None;
    }
    let zero_mask = mask & 0x3f;
    let stored_components = 6usize.checked_sub(usize::try_from(zero_mask.count_ones()).ok()?)?;
    let body_end = 25usize
        .checked_add(stored_components.checked_mul(8)?)?
        .checked_add(40)?;
    let tail_len = bytes.len().checked_sub(body_end)?;
    if !matches!(tail_len, 1 | 9) {
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
    let radius_raw = view.f64_le()?;
    let u0 = view.f64_le()?;
    let u1 = view.f64_le()?;
    let v0 = view.f64_le()?;
    let v1 = view.f64_le()?;
    let tail_tag = view.u8()?;
    match (tail_len, tail_tag) {
        (1, 0x07) => {}
        (9, 0x03) => {
            if !view.f64_le()?.is_finite() {
                return None;
            }
        }
        _ => return None,
    }
    if !view.is_empty() {
        return None;
    }
    let radius = cadmpeg_ir::scalar::PositiveLength::new(radius_raw)?;
    IncreasingParameterInterval::new([u0, u1])?;
    IncreasingParameterInterval::new([v0, v1])?;
    if !crate::families::b2::records::circle_range_is_within_full_turn(radius_raw, [u0, u1]) {
        return None;
    }
    let u = UnitVector3::new(Vector3::new(components[0], components[1], components[2]))?;
    let v = UnitVector3::new(Vector3::new(components[3], components[4], components[5]))?;
    Some(cadmpeg_ir::geometry::analytic::CylinderSurface::new(
        origin,
        OrthonormalFrame3::completing(u, v)?,
        radius,
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
    if (parameters[4] - 1.0).abs() > IDENTITY_AFFINE_SCALE_TOLERANCE
        || parameters[5] != 0.0
        || (parameters[6] - 1.0).abs() > IDENTITY_AFFINE_SCALE_TOLERANCE
        || parameters[7] != 0.0
    {
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
    decode_bitmask_plane_with_bounds(bytes).map(|(plane, _)| plane)
}

pub(in crate::families::freeform) fn decode_bitmask_plane_with_bounds(
    bytes: &[u8],
) -> Option<(PlaneSurface, Option<[[f64; 2]; 2]>)> {
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
    let bounds = if has_bounds {
        let u0 = view.f64_le()?;
        let u1 = view.f64_le()?;
        let v0 = view.f64_le()?;
        let v1 = view.f64_le()?;
        let u_iv = IncreasingParameterInterval::new([u0, u1])?.endpoints();
        let v_iv = IncreasingParameterInterval::new([v0, v1])?.endpoints();
        Some([u_iv, v_iv])
    } else {
        None
    };
    if !view.is_empty() {
        return None;
    }
    Some((
        PlaneSurface::new(origin, OrthonormalFrame3::completing(u, v)?),
        bounds,
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

    #[test]
    fn revolve_nurbs_curve_produces_exact_rational_quarter_and_full_turns() {
        use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
        const TOL: f64 = 1.0e-9;
        let line = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 10.0)],
            None,
            false,
        )
        .expect("line profile");
        let quarter = super::revolve_nurbs_curve(
            &line,
            Point3::new(0.0, 0.0, 0.0),
            UnitVector3::Z_AXIS,
            [0.0, std::f64::consts::FRAC_PI_2],
        )
        .expect("quarter revolution");
        assert_eq!(quarter.v_degree(), 2);
        assert_eq!(quarter.v_count(), 3);
        let mid = eval::decode::nurbs_surface_point(
            EvaluationAdmission::Standard,
            &quarter,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("midpoint")
        .get();
        let expected_xy = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((mid.x - expected_xy).abs() < TOL);
        assert!((mid.y - expected_xy).abs() < TOL);
        assert!((mid.z - 5.0).abs() < TOL);
    }

    #[test]
    fn decode_axis_mask_cylinder_and_offset_tails() {
        const TOL: f64 = 1.0e-12;
        // Axis-mask cylinder: mask 0x2e (stored bits 0, 4 => u=(1,0,0), v=(0,1,0), axis=+Z)
        let mut cyl = Vec::new();
        for v in [10.0_f64, 20.0, 30.0] {
            cyl.extend_from_slice(&v.to_le_bytes());
        }
        cyl.push(0x2e);
        for v in [
            1.0_f64,
            1.0,
            5.0,
            0.0,
            std::f64::consts::TAU * 5.0,
            -2.0,
            2.0,
        ] {
            cyl.extend_from_slice(&v.to_le_bytes());
        }
        cyl.push(0x07);
        let decoded = super::decode_axis_mask_cylinder(&cyl).expect("axis-mask cylinder");
        assert!((decoded.radius().get() - 5.0).abs() < TOL);
        assert!((decoded.frame().axis().as_raw().z - 1.0).abs() < TOL);

        // Two-ref B_30 offset tail: aux_ref=0x05, dist=-2.5, tag=0x15, u=[0,10], v=[-5,5]
        let mut off = vec![0x09, 0x05];
        off.extend_from_slice(&(-2.5_f64).to_le_bytes());
        off.push(0x15);
        for v in [0.0_f64, 10.0, -5.0, 5.0] {
            off.extend_from_slice(&v.to_le_bytes());
        }
        let (dist, u, v) = super::parse_offset_tail(&off, 1).expect("two-ref offset tail");
        assert!((dist + 2.5).abs() < TOL);
        assert!((u[0] - 0.0).abs() < TOL && (u[1] - 10.0).abs() < TOL);
        assert!((v[0] + 5.0).abs() < TOL && (v[1] - 5.0).abs() < TOL);
    }

    #[test]
    fn decode_compact_bspline_pcurve_handles_1a_and_3a_explicit_multiplicities() {
        // 0x1a form: 1 span, degree 1, 2 knots, 2 poles
        let mut p1a = vec![0x05, 0x1a, 0x05];
        p1a.extend_from_slice(&1.0_f64.to_le_bytes());
        p1a.extend_from_slice(&[0x05, 0x09, 0x0c]);
        for v in [0.0_f64, 1.0, 0.0, 0.0, 1.0, 2.0] {
            p1a.extend_from_slice(&v.to_le_bytes());
        }
        let (sref, nurbs, range) =
            super::decode_compact_bspline_pcurve_payload(&p1a).expect("0x1a pcurve");
        assert_eq!(sref.value, 1);
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(nurbs.control_points().len(), 2);
        assert!((range[1] - 1.0).abs() < 1.0e-12);

        // 0x3a form with explicit multiplicities (0x04): 3 knots, mults [2, 1, 2] -> 3 poles
        let mut p3a = vec![0x05, 0x3a, 0x05];
        p3a.extend_from_slice(&2.0_f64.to_le_bytes());
        p3a.extend_from_slice(&[0x05, 0x0d, 0x04]);
        for k in [0.0_f64, 1.0, 2.0] {
            p3a.extend_from_slice(&k.to_le_bytes());
        }
        // Explicit compact-int multiplicities: 2 (0x09), 1 (0x05), 2 (0x09)
        p3a.extend_from_slice(&[0x09, 0x05, 0x09]);
        for v in [0.0_f64, 0.0, 1.0, 1.0, 2.0, 0.0] {
            p3a.extend_from_slice(&v.to_le_bytes());
        }
        p3a.push(0x01); // trailing flag
        let (_, nurbs3a, range3a) =
            super::decode_compact_bspline_pcurve_payload(&p3a).expect("0x3a explicit pcurve");
        assert_eq!(nurbs3a.degree(), 1);
        assert_eq!(nurbs3a.control_points().len(), 3);
        assert!((range3a[1] - 2.0).abs() < 1.0e-12);
    }
}
