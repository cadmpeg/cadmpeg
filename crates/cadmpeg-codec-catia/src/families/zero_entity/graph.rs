// SPDX-License-Identifier: Apache-2.0
//! Counted topology records in the zero-entity `a9 03` stream family.

use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry,
};
use cadmpeg_ir::le::u32_at;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{BTreeMap, HashMap};

/// Resolved zero-entity `a9 03` stream: records, faces, loops, carrier runs,
/// and the edge/vertex tables recovered from them ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntityTopology {
    /// Every `a9 03` record found by the stream walk, in stream order.
    /// Indexed by `ordinal`, and by extension by every `*_ordinal` field
    /// below.
    pub records: Vec<ZeroEntityRecord>,
    /// `5f 0c` face records.
    pub faces: Vec<ZeroEntityFace>,
    /// `62 xx` loop records.
    pub loops: Vec<ZeroEntityLoop>,
    /// Carrier-then-supports runs: each surface carrier (`27 6a`/`28
    /// 8a`/`29 b8`/`2b c8`/`34 xx`) followed by its maximal run of `21 xx`
    /// support occurrences, one run per face ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
    pub carrier_runs: Vec<ZeroCarrierRun>,
    /// `21 xx` curve-support-on-surface records, across all carrier runs.
    pub supports: Vec<ZeroSupport>,
    /// `5e 1a` edge-stride records.
    pub physical_edges: Vec<ZeroPhysicalEdge>,
    /// `06 38` coedge records, two per physical edge (one per side).
    pub coedge_twins: Vec<ZeroCoedgeTwin>,
    /// `25 69` side-pair header records, each identifying its two `06 38`
    /// twin coedges.
    pub side_pairs: Vec<ZeroSidePair>,
    /// `05 0b`/`05 10`/`05 15` vertex-incidence records paired with their
    /// following `5d 06` marker.
    pub vertices: Vec<ZeroVertex>,
}

/// A resolved vertex-incidence pair: a `05 0b`/`05 10`/`05 15` incidence
/// record immediately followed by its `5d 06` vertex marker ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroVertex {
    /// `ordinal` of the following `5d 06` marker record.
    pub marker_ordinal: usize,
    /// `ordinal` of this `05 0x` incidence record.
    pub incidence_record_ordinal: usize,
    /// Referenced record ordinals from the incidence record's counted
    /// reference lane: 2 items for tag `0x0b`, 3 for `0x10`, 4 for `0x15`.
    pub incidence_items: Vec<u32>,
}

/// A resolved `5e 1a` edge-stride record (38 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroPhysicalEdge {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Six `0x10`-tagged `u32` reference tokens at fixed offsets `7, 12,
    /// 17, 22, 27, 32`; meaning not decoded further.
    pub references: [u32; 6],
}

/// A resolved `06 38` coedge record: one of the two per-side halves of a
/// physical edge (68 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroCoedgeTwin {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Side number, `1` or `2`, read from the byte following the `0x10`
    /// marker at the record's `0x83` position.
    pub side: u8,
    /// `0x10`-tagged `u32` reference tokens following the side byte, in
    /// serialized order.
    pub references: Vec<u32>,
}

/// A resolved `25 69` side-pair header record, linking two [`ZeroCoedgeTwin`]
/// records by side number ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroSidePair {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// The header's two base columns `[B0, B1]`.
    pub bases: [u32; 2],
    /// `record_ordinal`s of the two following `06 38` records: side `1`
    /// first, side `2` second.
    pub coedge_ordinals: [usize; 2],
    /// `[bases[i] + side]` for `side` in `1, 2`; each side's composite key
    /// must equal the first two references of its paired coedge.
    pub composite_keys: [[u32; 2]; 2],
}

/// One surface carrier and its maximal run of `21 xx` support occurrences,
/// aligned 1:1 with a face ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant), "Carrier run = per-face surface").
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroCarrierRun {
    /// `ordinal` of the carrier record (`27 6a`/`28 8a`/`29 b8`/`2b
    /// c8`/`34 xx`).
    pub carrier_ordinal: usize,
    /// `ordinal`s of the carrier's `21 xx` support records, in stream
    /// order.
    pub support_ordinals: Vec<usize>,
    /// Complete decoded carrier geometry.
    pub geometry: Option<SurfaceGeometry>,
}

/// A resolved `21 xx` curve-support-on-surface record, with its UV
/// endpoints lifted through the owning carrier where possible ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroSupport {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// `ordinal` of the owning carrier record.
    pub owner_carrier_ordinal: usize,
    /// Local slot index at `+12`, used with a loop's `terminal_id` to
    /// address this support from a `62xx` loop member (`A = T - s`).
    pub slot: u32,
    /// `(u0,v0)`/`(u1,v1)` endpoint pairs read from the record's f64 tail
    /// at the family-specific offsets in [`support_uv_endpoints`], or
    /// `None` for an unrecognized support-record tag.
    pub uv_endpoints: Option<[[f64; 2]; 2]>,
    /// Complete inline pcurve geometry in the neutral parameterization of
    /// the owning carrier, when the support family stores its poles inline.
    pub pcurve: Option<PcurveGeometry>,
    /// `uv_endpoints` lifted to world-frame 3D points through the owning
    /// carrier's analytic parameterization, or `None` when `uv_endpoints`
    /// is `None` or the carrier's tag is not one of the four supported
    /// analytic kinds ([`lift_geometry`]).
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

/// One length-framed `a9 03` record as found by the stream walk: framing
/// `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12` ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityRecord {
    /// This record's position in the stream walk order. Records reference
    /// each other by this ordinal, not by byte offset ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
    pub ordinal: usize,
    /// Byte offset of the `a9 03` marker in the source stream.
    pub offset: usize,
    /// The two tag bytes (`XX`, `YY`) identifying the record family.
    pub tag: [u8; 2],
    /// The full record, including its `a9 03 XX YY` header.
    pub bytes: Vec<u8>,
}

/// A resolved `5f 0c` face record (24 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityFace {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// The record's counted reference lane `[R0, R1, ..., Rm]`: `R0` is
    /// the face's terminal base, `R1..` name loop terminals.
    pub references: Vec<u32>,
    /// Ordered loop terminals `T[j] = R0 - R[j+1]`, one per loop owned by
    /// this face.
    pub loop_terminals: Vec<u32>,
    /// Indices into the topology's `loops` vector, one per
    /// `loop_terminals` entry in the same order, resolved by
    /// [`bind_face_runs`]. Empty until binding runs.
    pub loop_indices: Vec<usize>,
    /// Index into the topology's `carrier_runs` vector for this face's
    /// surface carrier, resolved by [`bind_face_runs`]. `None` until
    /// binding runs or when no carrier run aligns with this face.
    pub carrier_run: Option<usize>,
}

/// A resolved `62 xx` loop record: an alternating even/odd reference lane
/// plus a packed 3-bit-per-member sense stream ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityLoop {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Even-lane reference ids `A[j]`, one per loop member, satisfying
    /// `A[j] = T - g - j` for this loop's `terminal_id` (`T`) and `gap`
    /// (`g`).
    pub member_ids: Vec<u32>,
    /// Odd-lane reference ids interleaved with `member_ids`; meaning not
    /// decoded further.
    pub secondary_refs: Vec<u32>,
    /// The loop's terminal id `T`: the last entry of the record's counted
    /// reference lane.
    pub terminal_id: u32,
    /// `T - member_ids[0]`: the offset between the terminal id and the
    /// first even-lane member.
    pub gap: u32,
    /// Loop-class byte from the record header: `0x50` marks an inner
    /// (hole) loop, `0x41`/`0xc1` mark a non-inner loop.
    pub loop_class: u8,
    /// `true` when `loop_class == 0x50` (an inner/hole loop).
    pub inner: bool,
    /// Per-member coedge sense decoded from the packed 3-bit stream: code
    /// `7` (`.T.`, forward) decodes to `false`, code `2` (`.F.`, reversed)
    /// decodes to `true`. Index-aligned with `member_ids`.
    pub reversed: Vec<bool>,
    /// Per-member index into the topology's `supports` vector, resolved by
    /// [`bind_face_runs`] from each member's local slot `A = T - s`.
    /// `None` for a member whose slot resolves to no support in the
    /// owning carrier run, or before binding runs.
    pub support_indices: Vec<Option<usize>>,
}

/// One loop-member occurrence participating in a geometrically closed radial
/// edge pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub struct ZeroResolvedOccurrence {
    /// Index into [`ZeroEntityTopology::loops`].
    pub loop_index: usize,
    /// Member index within the loop.
    pub member_index: usize,
    /// Index into [`ZeroEntityTopology::supports`].
    pub support_index: usize,
}

/// One physical edge resolved from two surface-side occurrences with equal
/// unordered world-space endpoint pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroResolvedEdge {
    /// Canonical endpoint order inherited from the first occurrence.
    pub endpoints: [[f64; 3]; 2],
    /// The two radial surface-side occurrences.
    pub occurrences: [ZeroResolvedOccurrence; 2],
    /// Endpoint order after applying each occurrence's packed loop sense.
    pub occurrence_endpoints: [[[f64; 3]; 2]; 2],
}

/// Exact two-surface support construction and its tolerance-bounded solved
/// carrier cache.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroIntersectionCurve {
    /// Support occurrence indices in radial order.
    pub supports: [usize; 2],
    /// Shared native curve parameter interval.
    pub parameter_range: [f64; 2],
    /// Piecewise-linear NURBS cache fitted to the first support lift.
    pub cache: NurbsCurve,
    /// Maximum admitted cache deviation in model length units.
    pub fit_tolerance: f64,
}

/// Direct support lift, including an exact construction when the model-space
/// curve is not an elementary carrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZeroDirectCurve {
    /// Elementary carrier or tolerance-bounded construction cache.
    pub geometry: CurveGeometry,
    /// Increasing interval on `geometry` and on `construction`, when retained.
    pub parameter_range: Option<[f64; 2]>,
    /// Exact non-elementary construction represented by `geometry`.
    pub construction: Option<ProceduralCurveDefinition>,
    /// Maximum cache deviation in model length units.
    pub cache_fit_tolerance: Option<f64>,
}

/// Lift one support pcurve and retain its trim interval when the lifted
/// carrier has a direct, branch-free parameter mapping.
#[must_use]
pub(crate) fn direct_support_curve(
    topology: &ZeroEntityTopology,
    occurrence: ZeroResolvedOccurrence,
    edge_endpoints: [[f64; 3]; 2],
) -> Option<ZeroDirectCurve> {
    let support = topology.supports.get(occurrence.support_index)?;
    let pcurve = support.pcurve.as_ref()?;
    let run = topology
        .carrier_runs
        .iter()
        .find(|run| run.carrier_ordinal == support.owner_carrier_ordinal)?;
    let surface = run.geometry.as_ref()?;
    let source_range = pcurve_parameter_range(pcurve, support.uv_endpoints)?;
    let reversed = *topology
        .loops
        .get(occurrence.loop_index)?
        .reversed
        .get(occurrence.member_index)?;
    if let Some(curve) = lift_cylinder_helix(pcurve, surface, source_range, reversed) {
        return Some(curve);
    }
    let curve = lift_pcurve(pcurve, surface)?;
    let (mut geometry, mut parameter_range) =
        orient_direct_support_curve(pcurve, surface, curve, source_range, reversed)?;
    if let (CurveGeometry::Nurbs(curve), Some(range)) = (&mut geometry, parameter_range) {
        parameter_range = Some(orient_nurbs_to_endpoints(curve, range, edge_endpoints)?);
    }
    Some(ZeroDirectCurve {
        geometry,
        parameter_range,
        construction: None,
        cache_fit_tolerance: None,
    })
}

fn lift_cylinder_helix(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    source_range: [f64; 2],
    reversed: bool,
) -> Option<ZeroDirectCurve> {
    const FIT_TOLERANCE: f64 = 1e-4;

    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface
    else {
        return None;
    };
    let PcurveGeometry::Nurbs {
        degree,
        control_points,
        ..
    } = pcurve
    else {
        return None;
    };
    if *degree != 1 || control_points.len() != 2 || radius.abs() <= f64::EPSILON {
        return None;
    }
    let uv = source_range.map(|parameter| cadmpeg_ir::eval::pcurve_uv(pcurve, parameter));
    let [Some(start), Some(end)] = uv else {
        return None;
    };
    let mut uv = [start, end];
    if reversed {
        uv.swap(0, 1);
    }
    let delta_u = uv[1].u - uv[0].u;
    let delta_v = uv[1].v - uv[0].v;
    if !delta_u.is_finite()
        || !delta_v.is_finite()
        || delta_u.abs() <= 1e-12
        || delta_v.abs() <= 1e-12
    {
        return None;
    }
    let tangent = (*axis).cross(*ref_direction);
    let radial = Vector3::new(
        ref_direction.x * uv[0].u.cos() + tangent.x * uv[0].u.sin(),
        ref_direction.y * uv[0].u.cos() + tangent.y * uv[0].u.sin(),
        ref_direction.z * uv[0].u.cos() + tangent.z * uv[0].u.sin(),
    );
    let radial_tangent = (*axis).cross(radial);
    let direction = delta_u.signum();
    let sweep = delta_u.abs();
    let construction = ProceduralCurveDefinition::Helix {
        angle_range: [0.0, sweep],
        center: (*origin).translated(*axis, uv[0].v),
        major: radial.scale(*radius),
        minor: radial_tangent.scale(radius * direction),
        pitch: (*axis).scale(delta_v / sweep * 2.0 * std::f64::consts::PI),
        apex_factor: 0.0,
        axis: *axis,
    };
    let cache = crate::nurbs::circular_helix_cache(&construction, FIT_TOLERANCE)?;
    Some(ZeroDirectCurve {
        geometry: CurveGeometry::Nurbs(cache.curve),
        parameter_range: Some([0.0, sweep]),
        construction: Some(construction),
        cache_fit_tolerance: Some(cache.fit_tolerance),
    })
}

fn orient_direct_support_curve(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    mut curve: CurveGeometry,
    source_range: [f64; 2],
    reversed: bool,
) -> Option<(CurveGeometry, Option<[f64; 2]>)> {
    let uv = source_range.map(|parameter| cadmpeg_ir::eval::pcurve_uv(pcurve, parameter));
    let [Some(start_uv), Some(end_uv)] = uv else {
        return None;
    };
    let mut uv = [start_uv, end_uv];
    if reversed {
        uv.swap(0, 1);
    }
    let range = match (surface, &mut curve) {
        (SurfaceGeometry::Plane { .. }, CurveGeometry::Nurbs(curve)) => {
            if reversed {
                reverse_nurbs_curve(curve)?;
            }
            canonical_nurbs_interval(curve, source_range)
        }
        (SurfaceGeometry::Nurbs(_), CurveGeometry::Nurbs(curve)) => {
            let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
                return None;
            };
            if constant_coordinate(control_points, |point| point.u).is_some() {
                orient_nurbs_interval(curve, [uv[0].v, uv[1].v])
            } else if constant_coordinate(control_points, |point| point.v).is_some() {
                orient_nurbs_interval(curve, [uv[0].u, uv[1].u])
            } else {
                None
            }
        }
        (
            SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. },
            CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. },
        ) => orient_conic_interval(pcurve, surface, &uv, &mut curve),
        (_, CurveGeometry::Line { origin, direction }) => {
            let points = uv.map(|uv| cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v));
            let [Some(start), Some(end)] = points else {
                return None;
            };
            let delta = end.vector_from(start);
            let length = delta.norm();
            if !length.is_finite() || length <= f64::EPSILON {
                return None;
            }
            *origin = start;
            *direction = delta.scale(length.recip());
            Some([0.0, length])
        }
        _ => None,
    };
    Some((curve, range))
}

fn orient_conic_interval(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    uv: &[Point2; 2],
    curve: &mut CurveGeometry,
) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
        return None;
    };
    let span = match surface {
        SurfaceGeometry::Cylinder { .. } => {
            constant_coordinate(control_points, |point| point.v)?;
            monotone_coordinate(control_points, |point| point.u)?;
            uv[1].u - uv[0].u
        }
        SurfaceGeometry::Cone { ratio, .. } => {
            constant_coordinate(control_points, |point| point.v)?;
            monotone_coordinate(control_points, |point| point.u)?;
            (uv[1].u - uv[0].u) * ratio.signum()
        }
        SurfaceGeometry::Sphere { .. } => {
            if constant_coordinate(control_points, |point| point.u).is_some() {
                monotone_coordinate(control_points, |point| point.v)?;
                uv[1].v - uv[0].v
            } else {
                constant_coordinate(control_points, |point| point.v)?;
                monotone_coordinate(control_points, |point| point.u)?;
                uv[1].u - uv[0].u
            }
        }
        SurfaceGeometry::Torus { .. } => {
            if constant_coordinate(control_points, |point| point.u).is_some() {
                monotone_coordinate(control_points, |point| point.v)?;
                uv[1].v - uv[0].v
            } else {
                constant_coordinate(control_points, |point| point.v)?;
                monotone_coordinate(control_points, |point| point.u)?;
                uv[1].u - uv[0].u
            }
        }
        _ => return None,
    };
    if !span.is_finite() || span.abs() <= 1e-12 || span.abs() > std::f64::consts::TAU + 1e-9 {
        return None;
    }
    let start_point = cadmpeg_ir::eval::surface_point(surface, uv[0].u, uv[0].v)?;
    let end_point = cadmpeg_ir::eval::surface_point(surface, uv[1].u, uv[1].v)?;
    let start = conic_parameter(curve, start_point)?;
    if span < 0.0 {
        reverse_conic_axis(curve)?;
    }
    let start = if span < 0.0 { -start } else { start };
    let sweep = span.abs();
    let range = crate::nurbs::canonical_periodic_range([start, start + sweep])?;
    let expected_end = conic_point(curve, range[1])?;
    (point_distance(
        [expected_end.x, expected_end.y, expected_end.z],
        [end_point.x, end_point.y, end_point.z],
    ) <= 1e-9 * (1.0 + conic_scale(curve)?))
    .then_some(range)
}

fn monotone_coordinate(points: &[Point2], coordinate: impl Fn(&Point2) -> f64) -> Option<()> {
    let mut direction = 0.0;
    for pair in points.windows(2) {
        let delta = coordinate(&pair[1]) - coordinate(&pair[0]);
        if delta.abs() <= 1e-12 {
            continue;
        }
        if direction != 0.0 && delta.signum() != direction {
            return None;
        }
        direction = delta.signum();
    }
    (direction != 0.0).then_some(())
}

fn conic_parameter(curve: &CurveGeometry, point: Point3) -> Option<f64> {
    let (center, axis, reference, major, minor) = match curve {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (*center, *axis, *ref_direction, *radius, *radius),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            *center,
            *axis,
            *major_direction,
            *major_radius,
            *minor_radius,
        ),
        _ => return None,
    };
    let offset = point.vector_from(center);
    let tangent = axis.cross(reference);
    Some((offset.dot(tangent) / minor).atan2(offset.dot(reference) / major))
}

fn reverse_conic_axis(curve: &mut CurveGeometry) -> Option<()> {
    match curve {
        CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => {
            *axis = (*axis).scale(-1.0);
            Some(())
        }
        _ => None,
    }
}

fn conic_point(curve: &CurveGeometry, parameter: f64) -> Option<Point3> {
    let (center, axis, reference, major, minor) = match curve {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (*center, *axis, *ref_direction, *radius, *radius),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            *center,
            *axis,
            *major_direction,
            *major_radius,
            *minor_radius,
        ),
        _ => return None,
    };
    Some(offset(
        center,
        reference,
        major * parameter.cos(),
        axis.cross(reference),
        minor * parameter.sin(),
    ))
}

fn conic_scale(curve: &CurveGeometry) -> Option<f64> {
    match curve {
        CurveGeometry::Circle { radius, .. } => Some(*radius),
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => Some((*major_radius).max(*minor_radius)),
        _ => None,
    }
}

fn orient_nurbs_interval(curve: &mut NurbsCurve, range: [f64; 2]) -> Option<[f64; 2]> {
    if range[0] < range[1] {
        return canonical_nurbs_interval(curve, range);
    }
    if range[0] <= range[1] {
        return None;
    }
    let domain = nurbs_curve_domain(curve)?;
    reverse_nurbs_curve(curve)?;
    canonical_nurbs_interval(
        curve,
        [
            domain[0] + domain[1] - range[0],
            domain[0] + domain[1] - range[1],
        ],
    )
}

fn canonical_nurbs_interval(curve: &NurbsCurve, mut range: [f64; 2]) -> Option<[f64; 2]> {
    let domain = nurbs_curve_domain(curve)?;
    for (value, boundary) in range.iter_mut().zip(domain) {
        if (*value - boundary).abs() <= 1e-12 * (1.0 + boundary.abs()) {
            *value = boundary;
        }
    }
    (range[0].is_finite()
        && range[1].is_finite()
        && domain[0] <= range[0]
        && range[0] < range[1]
        && range[1] <= domain[1])
        .then_some(range)
}

fn orient_nurbs_to_endpoints(
    curve: &mut NurbsCurve,
    range: [f64; 2],
    endpoints: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    const TOLERANCE: f64 = 2e-3;

    let expected = endpoints.map(|point| Point3::new(point[0], point[1], point[2]));
    let geometry = CurveGeometry::Nurbs(curve.clone());
    let evaluated = range.map(|parameter| cadmpeg_ir::eval::curve_point(&geometry, parameter));
    let [Some(start), Some(end)] = evaluated else {
        return None;
    };
    let forward = start
        .vector_from(expected[0])
        .norm()
        .max(end.vector_from(expected[1]).norm());
    if forward <= TOLERANCE {
        return Some(range);
    }
    let reverse = start
        .vector_from(expected[1])
        .norm()
        .max(end.vector_from(expected[0]).norm());
    if reverse > TOLERANCE {
        return None;
    }
    orient_nurbs_interval(curve, [range[1], range[0]])
}

fn nurbs_curve_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree).ok()?;
    Some([
        *curve.knots.get(degree)?,
        *curve
            .knots
            .get(curve.knots.len().checked_sub(degree + 1)?)?,
    ])
}

fn reverse_nurbs_curve(curve: &mut NurbsCurve) -> Option<()> {
    let [start, end] = nurbs_curve_domain(curve)?;
    curve.control_points.reverse();
    if let Some(weights) = &mut curve.weights {
        weights.reverse();
    }
    curve.knots = curve
        .knots
        .iter()
        .rev()
        .map(|knot| start + end - knot)
        .collect();
    for knot in &mut curve.knots {
        if (*knot - start).abs() <= 1e-12 * (1.0 + start.abs()) {
            *knot = start;
        } else if (*knot - end).abs() <= 1e-12 * (1.0 + end.abs()) {
            *knot = end;
        }
    }
    Some(())
}

/// Reconstruct a physical curve carried by two complete radial pcurves.
///
/// Both pcurves must share one increasing parameter interval. The returned
/// NURBS follows the midpoint of the two stored support traces; its fit contract
/// includes half their measured separation. The paired surface construction
/// remains authoritative.
#[must_use]
pub fn intersection_curve(
    topology: &ZeroEntityTopology,
    edge: &ZeroResolvedEdge,
) -> Option<ZeroIntersectionCurve> {
    const FIT_TOLERANCE: f64 = 1e-4;
    const MAX_DEPTH: u8 = 20;

    let supports = edge.occurrences.map(|occurrence| occurrence.support_index);
    let ranges = supports.map(|index| support_parameter_range(&topology.supports[index]));
    let [Some(first_range), Some(second_range)] = ranges else {
        return degenerate_plane_torus_intersection(topology, edge, supports)
            .or_else(|| degenerate_parallel_plane_cylinder_intersection(topology, edge, supports));
    };
    if first_range[0] >= first_range[1]
        || first_range
            .into_iter()
            .zip(second_range)
            .any(|(left, right)| (left - right).abs() > 1e-12 * (1.0 + left.abs()))
    {
        return None;
    }
    let (first, _) = radial_midpoint(topology, supports, first_range[0])?;
    let (last, _) = radial_midpoint(topology, supports, first_range[1])?;
    let mut samples = vec![(first_range[0], first)];
    subdivide_support_curve(
        topology,
        supports,
        first_range[0],
        first,
        first_range[1],
        last,
        FIT_TOLERANCE,
        MAX_DEPTH,
        &mut samples,
    )?;
    let radial_error = samples.iter().try_fold(0.0f64, |maximum, (parameter, _)| {
        let (_, separation) = radial_midpoint(topology, supports, *parameter)?;
        Some(maximum.max(separation * 0.5))
    })?;
    let mut knots = Vec::with_capacity(samples.len() + 2);
    knots.push(first_range[0]);
    knots.extend(samples.iter().map(|(parameter, _)| *parameter));
    knots.push(first_range[1]);
    Some(ZeroIntersectionCurve {
        supports,
        parameter_range: first_range,
        cache: NurbsCurve {
            degree: 1,
            knots,
            control_points: samples.into_iter().map(|(_, point)| point).collect(),
            weights: None,
            periodic: false,
        },
        fit_tolerance: FIT_TOLERANCE + radial_error,
    })
}

fn degenerate_parallel_plane_cylinder_intersection(
    topology: &ZeroEntityTopology,
    edge: &ZeroResolvedEdge,
    supports: [usize; 2],
) -> Option<ZeroIntersectionCurve> {
    const INCIDENCE_TOLERANCE: f64 = 2e-3;

    if supports
        .iter()
        .any(|index| topology.records[topology.supports[*index].record_ordinal].tag != [0x21, 0x18])
    {
        return None;
    }
    let surfaces = supports.map(|index| support_surface(topology, index));
    let [Some(first), Some(second)] = surfaces else {
        return None;
    };
    let (plane, cylinder) = match (first, second) {
        (SurfaceGeometry::Plane { .. }, SurfaceGeometry::Cylinder { .. }) => (first, second),
        (SurfaceGeometry::Cylinder { .. }, SurfaceGeometry::Plane { .. }) => (second, first),
        _ => return None,
    };
    let SurfaceGeometry::Plane { origin, normal, .. } = plane else {
        unreachable!();
    };
    let SurfaceGeometry::Cylinder {
        origin: axis_origin,
        axis,
        radius,
        ..
    } = cylinder
    else {
        unreachable!();
    };
    if (*normal).dot(*axis).abs() > 1e-10 || *radius <= 0.0 {
        return None;
    }
    let signed_axis_offset = (*axis_origin).vector_from(*origin).dot(*normal);
    if signed_axis_offset.abs() > *radius {
        return None;
    }
    let transverse = normalize((*axis).cross(*normal))?;
    let transverse_offset = (radius.powi(2) - signed_axis_offset.powi(2))
        .max(0.0)
        .sqrt();
    let base = (*axis_origin).translated(*normal, -signed_axis_offset);
    let signs = if transverse_offset <= 1e-12 {
        &[1.0][..]
    } else {
        &[-1.0, 1.0][..]
    };
    let endpoint_points = edge
        .endpoints
        .map(|point| Point3::new(point[0], point[1], point[2]));
    let matching = signs
        .iter()
        .filter_map(|sign| {
            let line_origin = base.translated(transverse, sign * transverse_offset);
            let residuals = endpoint_points.map(|point| {
                let offset = point.vector_from(line_origin);
                let axial = offset.dot(*axis);
                let projected = line_origin.translated(*axis, axial);
                (
                    point_distance(
                        [point.x, point.y, point.z],
                        [projected.x, projected.y, projected.z],
                    ),
                    projected,
                )
            });
            residuals
                .iter()
                .all(|(residual, _)| *residual <= INCIDENCE_TOLERANCE)
                .then_some(residuals)
        })
        .collect::<Vec<_>>();
    let [residuals] = matching.as_slice() else {
        return None;
    };
    let control_points = residuals.map(|(_, projected)| projected);
    let length = point_distance(
        [
            control_points[0].x,
            control_points[0].y,
            control_points[0].z,
        ],
        [
            control_points[1].x,
            control_points[1].y,
            control_points[1].z,
        ],
    );
    if length <= 1e-12 {
        return None;
    }
    let parameter_range = [0.0, length];
    Some(ZeroIntersectionCurve {
        supports,
        parameter_range,
        cache: NurbsCurve {
            degree: 1,
            knots: vec![
                parameter_range[0],
                parameter_range[0],
                parameter_range[1],
                parameter_range[1],
            ],
            control_points: control_points.to_vec(),
            weights: None,
            periodic: false,
        },
        fit_tolerance: residuals
            .iter()
            .map(|(residual, _)| *residual)
            .fold(0.0, f64::max),
    })
}

fn degenerate_plane_torus_intersection(
    topology: &ZeroEntityTopology,
    edge: &ZeroResolvedEdge,
    supports: [usize; 2],
) -> Option<ZeroIntersectionCurve> {
    const FIT_TOLERANCE: f64 = 1e-4;
    const MAX_DEPTH: u8 = 20;

    if supports
        .iter()
        .any(|index| topology.records[topology.supports[*index].record_ordinal].tag != [0x21, 0x18])
    {
        return None;
    }
    let surfaces = supports.map(|index| support_surface(topology, index));
    let [Some(first), Some(second)] = surfaces else {
        return None;
    };
    let (plane, torus) = match (first, second) {
        (SurfaceGeometry::Plane { .. }, SurfaceGeometry::Torus { .. }) => (first, second),
        (SurfaceGeometry::Torus { .. }, SurfaceGeometry::Plane { .. }) => (second, first),
        _ => return None,
    };
    let SurfaceGeometry::Plane { origin, normal, .. } = plane else {
        unreachable!();
    };
    let SurfaceGeometry::Torus {
        center,
        axis,
        major_radius,
        minor_radius,
        ..
    } = torus
    else {
        unreachable!();
    };
    if (*normal).dot(*axis).abs() > 1e-10 || *major_radius <= 0.0 || *minor_radius <= 0.0 {
        return None;
    }
    let transverse = normalize((*axis).cross(*normal))?;
    let plane_offset = (*origin).vector_from(*center).dot(*normal);
    let endpoint_data = edge.endpoints.map(|point| {
        let point = Point3::new(point[0], point[1], point[2]);
        let relative = point.vector_from(*center);
        let transverse_coordinate = relative.dot(transverse);
        let radial = (transverse_coordinate.powi(2) + plane_offset.powi(2)).sqrt();
        (
            (relative.dot(*axis)).atan2(radial - major_radius),
            transverse_coordinate,
        )
    });
    let branch_sign = endpoint_data[0].1.signum();
    if branch_sign == 0.0 || endpoint_data[1].1.signum() != branch_sign {
        return None;
    }
    let mut delta = endpoint_data[1].0 - endpoint_data[0].0;
    if delta > std::f64::consts::PI {
        delta -= std::f64::consts::TAU;
    } else if delta < -std::f64::consts::PI {
        delta += std::f64::consts::TAU;
    }
    let extent = delta.abs();
    if extent <= 1e-12 {
        return None;
    }
    let direction = delta.signum();
    let evaluate = |parameter| {
        plane_torus_section_point(
            *center,
            *axis,
            *normal,
            transverse,
            *major_radius,
            *minor_radius,
            plane_offset,
            branch_sign,
            endpoint_data[0].0 + direction * parameter,
        )
    };
    let first = evaluate(0.0)?;
    let last = evaluate(extent)?;
    if point_distance([first.x, first.y, first.z], edge.endpoints[0]) > 2e-3
        || point_distance([last.x, last.y, last.z], edge.endpoints[1]) > 2e-3
    {
        return None;
    }
    let mut samples = vec![(0.0, first)];
    subdivide_parametric_curve(
        &evaluate,
        0.0,
        first,
        extent,
        last,
        FIT_TOLERANCE,
        MAX_DEPTH,
        &mut samples,
    )?;
    let mut knots = Vec::with_capacity(samples.len() + 2);
    knots.push(0.0);
    knots.extend(samples.iter().map(|(parameter, _)| *parameter));
    knots.push(extent);
    Some(ZeroIntersectionCurve {
        supports,
        parameter_range: [0.0, extent],
        cache: NurbsCurve {
            degree: 1,
            knots,
            control_points: samples.into_iter().map(|(_, point)| point).collect(),
            weights: None,
            periodic: false,
        },
        fit_tolerance: FIT_TOLERANCE,
    })
}

fn support_surface(
    topology: &ZeroEntityTopology,
    support_index: usize,
) -> Option<&SurfaceGeometry> {
    let support = topology.supports.get(support_index)?;
    topology
        .carrier_runs
        .iter()
        .find(|run| run.carrier_ordinal == support.owner_carrier_ordinal)?
        .geometry
        .as_ref()
}

#[allow(clippy::too_many_arguments)]
fn plane_torus_section_point(
    center: Point3,
    axis: Vector3,
    normal: Vector3,
    transverse: Vector3,
    major_radius: f64,
    minor_radius: f64,
    plane_offset: f64,
    branch_sign: f64,
    minor_angle: f64,
) -> Option<Point3> {
    let radial = major_radius + minor_radius * minor_angle.cos();
    let transverse_coordinate = branch_sign * (radial.powi(2) - plane_offset.powi(2)).sqrt();
    transverse_coordinate.is_finite().then(|| {
        let point = center.translated(normal, plane_offset);
        let point = point.translated(transverse, transverse_coordinate);
        point.translated(axis, minor_radius * minor_angle.sin())
    })
}

#[allow(clippy::too_many_arguments)]
fn subdivide_parametric_curve(
    evaluate: &impl Fn(f64) -> Option<Point3>,
    start_parameter: f64,
    start: Point3,
    end_parameter: f64,
    end: Point3,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<(f64, Point3)>,
) -> Option<()> {
    let fractions = [0.25, 0.5, 0.75];
    let probes = fractions.map(|fraction| {
        let parameter = start_parameter + fraction * (end_parameter - start_parameter);
        Some((parameter, evaluate(parameter)?))
    });
    let probes = probes.into_iter().collect::<Option<Vec<_>>>()?;
    if probes.iter().zip(fractions).all(|((_, point), fraction)| {
        point_lerp_distance(*point, start, end, fraction) <= tolerance
    }) {
        output.push((end_parameter, end));
        return Some(());
    }
    if depth == 0 {
        return None;
    }
    let (middle_parameter, middle) = probes[1];
    subdivide_parametric_curve(
        evaluate,
        start_parameter,
        start,
        middle_parameter,
        middle,
        tolerance,
        depth - 1,
        output,
    )?;
    subdivide_parametric_curve(
        evaluate,
        middle_parameter,
        middle,
        end_parameter,
        end,
        tolerance,
        depth - 1,
        output,
    )
}

fn support_parameter_range(support: &ZeroSupport) -> Option<[f64; 2]> {
    pcurve_parameter_range(support.pcurve.as_ref()?, support.uv_endpoints)
}

pub(crate) fn pcurve_parameter_range(
    pcurve: &PcurveGeometry,
    uv_endpoints: Option<[[f64; 2]; 2]>,
) -> Option<[f64; 2]> {
    match pcurve {
        PcurveGeometry::Nurbs { degree, knots, .. } => {
            let degree = usize::try_from(*degree).ok()?;
            Some([
                *knots.get(degree)?,
                *knots.get(knots.len().checked_sub(degree + 1)?)?,
            ])
        }
        PcurveGeometry::Line { origin, direction } => {
            let endpoints = uv_endpoints?;
            let denominator = direction.u.mul_add(direction.u, direction.v * direction.v);
            if !denominator.is_finite() || denominator == 0.0 {
                return None;
            }
            let mut range = [0.0; 2];
            for (parameter, endpoint) in range.iter_mut().zip(endpoints) {
                let delta = [endpoint[0] - origin.u, endpoint[1] - origin.v];
                *parameter = delta[0].mul_add(direction.u, delta[1] * direction.v) / denominator;
                let residual = (origin.u + *parameter * direction.u - endpoint[0])
                    .hypot(origin.v + *parameter * direction.v - endpoint[1]);
                let scale = 1.0f64
                    .max(endpoint[0].abs())
                    .max(endpoint[1].abs())
                    .max(origin.u.abs())
                    .max(origin.v.abs());
                if !parameter.is_finite() || residual > 1e-10 * scale {
                    return None;
                }
            }
            Some(range)
        }
        _ => None,
    }
}

fn support_point(
    topology: &ZeroEntityTopology,
    support_index: usize,
    parameter: f64,
) -> Option<Point3> {
    let support = topology.supports.get(support_index)?;
    let uv = cadmpeg_ir::eval::pcurve_uv(support.pcurve.as_ref()?, parameter)?;
    let run = topology
        .carrier_runs
        .iter()
        .find(|run| run.carrier_ordinal == support.owner_carrier_ordinal)?;
    cadmpeg_ir::eval::surface_point(run.geometry.as_ref()?, uv.u, uv.v)
}

fn radial_midpoint(
    topology: &ZeroEntityTopology,
    supports: [usize; 2],
    parameter: f64,
) -> Option<(Point3, f64)> {
    let points = supports.map(|support| support_point(topology, support, parameter));
    let [Some(first), Some(second)] = points else {
        return None;
    };
    let separation = point_distance([first.x, first.y, first.z], [second.x, second.y, second.z]);
    Some((
        Point3::new(
            (first.x + second.x) * 0.5,
            (first.y + second.y) * 0.5,
            (first.z + second.z) * 0.5,
        ),
        separation,
    ))
}

#[allow(clippy::too_many_arguments)]
fn subdivide_support_curve(
    topology: &ZeroEntityTopology,
    supports: [usize; 2],
    start_parameter: f64,
    start: Point3,
    end_parameter: f64,
    end: Point3,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<(f64, Point3)>,
) -> Option<()> {
    let fractions = [0.25, 0.5, 0.75];
    let probes = fractions.map(|fraction| {
        let parameter = start_parameter + fraction * (end_parameter - start_parameter);
        Some((parameter, radial_midpoint(topology, supports, parameter)?.0))
    });
    let probes = probes.into_iter().collect::<Option<Vec<_>>>()?;
    let within_tolerance = probes.iter().zip(fractions).all(|((_, point), fraction)| {
        point_lerp_distance(*point, start, end, fraction) <= tolerance
    });
    if within_tolerance {
        output.push((end_parameter, end));
        return Some(());
    }
    if depth == 0 {
        return None;
    }
    let (middle_parameter, middle) = probes[1];
    subdivide_support_curve(
        topology,
        supports,
        start_parameter,
        start,
        middle_parameter,
        middle,
        tolerance,
        depth - 1,
        output,
    )?;
    subdivide_support_curve(
        topology,
        supports,
        middle_parameter,
        middle,
        end_parameter,
        end,
        tolerance,
        depth - 1,
        output,
    )
}

fn point_lerp_distance(point: Point3, start: Point3, end: Point3, fraction: f64) -> f64 {
    let expected = Point3::new(
        start.x + fraction * (end.x - start.x),
        start.y + fraction * (end.y - start.y),
        start.z + fraction * (end.z - start.z),
    );
    point_distance(
        [point.x, point.y, point.z],
        [expected.x, expected.y, expected.z],
    )
}

fn lift_pcurve(pcurve: &PcurveGeometry, surface: &SurfaceGeometry) -> Option<CurveGeometry> {
    let controls: &[Point2] = match pcurve {
        PcurveGeometry::Line { origin, direction } => {
            return lift_parameter_line(*origin, *direction, surface);
        }
        PcurveGeometry::Nurbs { control_points, .. } => control_points,
        _ => return None,
    };
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let v_axis = (*normal).cross(*u_axis);
            let PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } = pcurve
            else {
                unreachable!();
            };
            Some(CurveGeometry::Nurbs(NurbsCurve {
                degree: *degree,
                knots: knots.clone(),
                control_points: control_points
                    .iter()
                    .map(|point| offset(*origin, *u_axis, point.u, v_axis, point.v))
                    .collect(),
                weights: weights.clone(),
                periodic: *periodic,
            }))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            if let Some(u) = constant_coordinate(controls, |point| point.u) {
                let point = cadmpeg_ir::eval::surface_point(surface, u, 0.0)?;
                Some(CurveGeometry::Line {
                    origin: point,
                    direction: *axis,
                })
            } else {
                let v = constant_coordinate(controls, |point| point.v)?;
                Some(CurveGeometry::Circle {
                    center: (*origin).translated(*axis, v),
                    axis: *axis,
                    ref_direction: *ref_direction,
                    radius: *radius,
                })
            }
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            if let Some(u) = constant_coordinate(controls, |point| point.u) {
                let tangent = (*axis).cross(*ref_direction);
                let radial = Vector3::new(
                    ref_direction.x * u.cos() + tangent.x * ratio * u.sin(),
                    ref_direction.y * u.cos() + tangent.y * ratio * u.sin(),
                    ref_direction.z * u.cos() + tangent.z * ratio * u.sin(),
                );
                let direction = normalize(Vector3::new(
                    axis.x + radial.x * half_angle.tan(),
                    axis.y + radial.y * half_angle.tan(),
                    axis.z + radial.z * half_angle.tan(),
                ))?;
                Some(CurveGeometry::Line {
                    origin: cadmpeg_ir::eval::surface_point(surface, u, 0.0)?,
                    direction,
                })
            } else {
                let v = constant_coordinate(controls, |point| point.v)?;
                let local_radius = radius + v * half_angle.tan();
                let sign = local_radius.signum();
                let major_radius = local_radius.abs();
                (major_radius > 0.0).then_some(())?;
                let reference = (*ref_direction).scale(sign);
                if (*ratio - 1.0).abs() <= 1e-12 {
                    Some(CurveGeometry::Circle {
                        center: (*origin).translated(*axis, v),
                        axis: *axis,
                        ref_direction: reference,
                        radius: major_radius,
                    })
                } else {
                    Some(CurveGeometry::Ellipse {
                        center: (*origin).translated(*axis, v),
                        axis: *axis,
                        major_direction: reference,
                        major_radius,
                        minor_radius: major_radius * ratio.abs(),
                    })
                }
            }
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            if let Some(u) = constant_coordinate(controls, |point| point.u) {
                let tangent = (*axis).cross(*ref_direction);
                let radial = Vector3::new(
                    ref_direction.x * u.cos() + tangent.x * u.sin(),
                    ref_direction.y * u.cos() + tangent.y * u.sin(),
                    ref_direction.z * u.cos() + tangent.z * u.sin(),
                );
                Some(CurveGeometry::Circle {
                    center: (*center).translated(radial, *major_radius),
                    axis: radial.cross(*axis),
                    ref_direction: radial,
                    radius: *minor_radius,
                })
            } else {
                let v = constant_coordinate(controls, |point| point.v)?;
                let ring_radius = major_radius + minor_radius * v.cos();
                (ring_radius.abs() > 0.0).then_some(())?;
                Some(CurveGeometry::Circle {
                    center: (*center).translated(*axis, minor_radius * v.sin()),
                    axis: *axis,
                    ref_direction: (*ref_direction).scale(ring_radius.signum()),
                    radius: ring_radius.abs(),
                })
            }
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if let Some(u) = constant_coordinate(controls, |point| point.u) {
                let tangent = (*axis).cross(*ref_direction);
                let radial = Vector3::new(
                    ref_direction.x * u.cos() + tangent.x * u.sin(),
                    ref_direction.y * u.cos() + tangent.y * u.sin(),
                    ref_direction.z * u.cos() + tangent.z * u.sin(),
                );
                let sign = radius.signum();
                (sign != 0.0).then_some(())?;
                Some(CurveGeometry::Circle {
                    center: *center,
                    axis: radial.cross(*axis),
                    ref_direction: radial.scale(sign),
                    radius: radius.abs(),
                })
            } else {
                let v = constant_coordinate(controls, |point| point.v)?;
                let latitude_radius = radius * v.cos();
                (latitude_radius.abs() > 1e-12 * (1.0 + radius.abs())).then_some(())?;
                Some(CurveGeometry::Circle {
                    center: (*center).translated(*axis, radius * v.sin()),
                    axis: *axis,
                    ref_direction: (*ref_direction).scale(latitude_radius.signum()),
                    radius: latitude_radius.abs(),
                })
            }
        }
        SurfaceGeometry::Nurbs(surface) => {
            if let Some(u) = constant_coordinate(controls, |point| point.u) {
                crate::nurbs::nurbs_surface_isocurve(surface, u, true).map(CurveGeometry::Nurbs)
            } else {
                let v = constant_coordinate(controls, |point| point.v)?;
                crate::nurbs::nurbs_surface_isocurve(surface, v, false).map(CurveGeometry::Nurbs)
            }
        }
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn lift_parameter_line(
    origin: Point2,
    direction: Point2,
    surface: &SurfaceGeometry,
) -> Option<CurveGeometry> {
    if let SurfaceGeometry::Plane {
        origin: plane_origin,
        normal,
        u_axis,
    } = surface
    {
        let v_axis = (*normal).cross(*u_axis);
        let model_direction = Vector3::new(
            u_axis.x * direction.u + v_axis.x * direction.v,
            u_axis.y * direction.u + v_axis.y * direction.v,
            u_axis.z * direction.u + v_axis.z * direction.v,
        );
        return Some(CurveGeometry::Line {
            origin: offset(*plane_origin, *u_axis, origin.u, v_axis, origin.v),
            direction: normalize(model_direction)?,
        });
    }
    let controls = [
        origin,
        Point2::new(origin.u + direction.u, origin.v + direction.v),
    ];
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: controls.to_vec(),
        weights: None,
        periodic: false,
    };
    lift_pcurve(&pcurve, surface)
}

fn constant_coordinate(points: &[Point2], coordinate: impl Fn(&Point2) -> f64) -> Option<f64> {
    let first = coordinate(points.first()?);
    points
        .iter()
        .all(|point| (coordinate(point) - first).abs() <= 1e-12)
        .then_some(first)
}

fn offset(origin: Point3, u: Vector3, a: f64, v: Vector3, b: f64) -> Point3 {
    Point3::new(
        origin.x + u.x * a + v.x * b,
        origin.y + u.y * a + v.y * b,
        origin.z + u.z * a + v.z * b,
    )
}

fn normalize(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| vector.scale(norm.recip()))
}

/// Resolve the reference-closed subset of zero-entity edge occurrences.
///
/// Stored support endpoints are oriented by each loop's packed sense lane.
/// An occurrence without a lifted carrier is completed only when it is isolated
/// between two lifted occurrences in the same closed loop. Radial twins are the
/// unique pairs with equal unordered endpoints within single-precision storage
/// tolerance. Ambiguous and unpaired occurrences remain unresolved.
#[must_use]
pub fn resolve_occurrence_edges(topology: &ZeroEntityTopology) -> Vec<ZeroResolvedEdge> {
    const TOLERANCE: f64 = 2e-3;
    let mut occurrences = Vec::<(ZeroResolvedOccurrence, Option<[[f64; 3]; 2]>)>::new();
    for (loop_index, loop_) in topology.loops.iter().enumerate() {
        let mut endpoints: Vec<Option<[[f64; 3]; 2]>> = loop_
            .support_indices
            .iter()
            .zip(&loop_.reversed)
            .map(|(support, reversed)| {
                let mut endpoints = topology.supports.get((*support)?)?.lifted_endpoints?;
                if *reversed {
                    endpoints.swap(0, 1);
                }
                Some(endpoints)
            })
            .collect();
        if endpoints.is_empty() {
            continue;
        }
        for index in 0..endpoints.len() {
            let next = (index + 1) % endpoints.len();
            if let (Some(current), Some(next_endpoints)) = (endpoints[index], endpoints[next]) {
                if point_distance(current[1], next_endpoints[0]) > TOLERANCE {
                    endpoints.fill(None);
                    break;
                }
            }
        }
        let stored = endpoints.clone();
        for index in 0..endpoints.len() {
            if endpoints[index].is_some() {
                continue;
            }
            let previous = (index + endpoints.len() - 1) % endpoints.len();
            let next = (index + 1) % endpoints.len();
            if let (Some(previous), Some(next)) = (stored[previous], stored[next]) {
                if point_distance(previous[1], next[0]) > TOLERANCE {
                    endpoints[index] = Some([previous[1], next[0]]);
                }
            }
        }
        for (member_index, (support_index, endpoints)) in loop_
            .support_indices
            .iter()
            .copied()
            .zip(endpoints)
            .enumerate()
        {
            if let Some(support_index) = support_index {
                occurrences.push((
                    ZeroResolvedOccurrence {
                        loop_index,
                        member_index,
                        support_index,
                    },
                    endpoints,
                ));
            }
        }
    }

    let loop_owners: HashMap<usize, usize> = topology
        .faces
        .iter()
        .enumerate()
        .flat_map(|(face_index, face)| {
            face.loop_indices
                .iter()
                .map(move |loop_index| (*loop_index, face_index))
        })
        .collect();
    let mut endpoint_groups = Vec::<Vec<usize>>::new();
    for (index, (_, endpoints)) in occurrences.iter().enumerate() {
        let Some(endpoints) = endpoints else { continue };
        if let Some(group) = endpoint_groups.iter_mut().find(|group| {
            occurrences[group[0]]
                .1
                .is_some_and(|other| same_endpoint_pair(*endpoints, other, TOLERANCE))
        }) {
            group.push(index);
        } else {
            endpoint_groups.push(vec![index]);
        }
    }

    let mut face_components: Vec<usize> = (0..topology.faces.len()).collect();
    for group in endpoint_groups.iter().filter(|group| group.len() == 2) {
        let Some(left_face) = loop_owners.get(&occurrences[group[0]].0.loop_index) else {
            continue;
        };
        let Some(right_face) = loop_owners.get(&occurrences[group[1]].0.loop_index) else {
            continue;
        };
        union_components(&mut face_components, *left_face, *right_face);
    }

    let mut pairs = Vec::<[usize; 2]>::new();
    for group in endpoint_groups {
        if group.len() == 2 {
            pairs.push([group[0], group[1]]);
            continue;
        }
        let mut by_component = BTreeMap::<usize, Vec<usize>>::new();
        for index in group {
            let Some(face) = loop_owners.get(&occurrences[index].0.loop_index) else {
                continue;
            };
            let component = component_root(&mut face_components, *face);
            by_component.entry(component).or_default().push(index);
        }
        if by_component.values().any(|members| members.len() != 2) {
            continue;
        }
        pairs.extend(
            by_component
                .into_values()
                .map(|members| [members[0], members[1]]),
        );
    }

    let mut edges = Vec::with_capacity(pairs.len());
    for [left, right] in pairs {
        let (Some(endpoints), Some(right_endpoints)) = (occurrences[left].1, occurrences[right].1)
        else {
            continue;
        };
        edges.push(ZeroResolvedEdge {
            endpoints,
            occurrences: [occurrences[left].0, occurrences[right].0],
            occurrence_endpoints: [endpoints, right_endpoints],
        });
    }
    edges
}

// Retained as a private copy rather than migrated onto `solve::UnionFind`.
// The consumer in `resolve_occurrence_edges` keys a `BTreeMap` directly by the
// component root and emits pairs via `into_values()` in root-sorted order, so
// the output edge order depends on WHICH element is the representative. This
// copy makes `right` the root; `solve::UnionFind::union` makes `left` the root.
// Swapping would reorder pairs within a multi-member endpoint group, changing
// `edges` order and the `edge_index` identities the decoder derives from it.
fn component_root(components: &mut [usize], mut index: usize) -> usize {
    while components[index] != index {
        components[index] = components[components[index]];
        index = components[index];
    }
    index
}

fn union_components(components: &mut [usize], left: usize, right: usize) {
    let left = component_root(components, left);
    let right = component_root(components, right);
    components[left] = right;
}

fn same_endpoint_pair(left: [[f64; 3]; 2], right: [[f64; 3]; 2], tolerance: f64) -> bool {
    (point_distance(left[0], right[0]).max(point_distance(left[1], right[1])) <= tolerance)
        || (point_distance(left[0], right[1]).max(point_distance(left[1], right[0])) <= tolerance)
}

fn point_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    ((left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2))
        .sqrt()
}

/// Walk native zero-entity records by `YY + 12`, then decode face counted
/// references and `62xx` alternating loop lanes with packed 3-bit senses.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<ZeroEntityTopology> {
    let records = walk_records(bytes);
    if records.is_empty() {
        return None;
    }
    let mut faces = records
        .iter()
        .filter(|record| record.tag[0] == 0x5f)
        .map(parse_face)
        .collect::<Option<Vec<_>>>()?;
    let mut loops = records
        .iter()
        .filter(|record| record.tag[0] == 0x62)
        .map(parse_loop)
        .collect::<Option<Vec<_>>>()?;
    if faces.is_empty() || loops.is_empty() {
        return None;
    }
    let (carrier_runs, supports) = parse_carrier_runs(&records, bytes)?;
    let physical_edges = records
        .iter()
        .filter(|record| record.tag == [0x5e, 0x1a])
        .map(parse_physical_edge)
        .collect::<Option<Vec<_>>>()?;
    let coedge_twins = records
        .iter()
        .filter(|record| record.tag == [0x06, 0x38])
        .map(parse_coedge_twin)
        .collect::<Option<Vec<_>>>()?;
    let side_pairs = parse_side_pairs(&records, &coedge_twins)?;
    let vertices = parse_vertices(&records)?;
    bind_face_runs(&mut faces, &mut loops, &carrier_runs, &supports);
    Some(ZeroEntityTopology {
        records,
        faces,
        loops,
        carrier_runs,
        supports,
        physical_edges,
        coedge_twins,
        side_pairs,
        vertices,
    })
}

/// Read unframed `05 08 01` coordinate rows outside every declared or extended
/// `a9 03` record extent. These rows support partial geometry fallback only;
/// connected topology derives its logical vertices from lifted incidences.
#[must_use]
pub fn unframed_vertices(bytes: &[u8]) -> Vec<Point3> {
    let records = walk_records(bytes);
    if records.is_empty() {
        return Vec::new();
    }
    let mut vertices = Vec::new();
    let mut region_start = 0usize;
    for record in &records {
        if region_start <= record.offset {
            vertices.extend(crate::wire::records::scan_vertex_records(
                &bytes[region_start..record.offset],
            ));
        }
        let logical_len = support_logical_len(record.tag).unwrap_or(record.bytes.len());
        region_start = record.offset.saturating_add(logical_len).min(bytes.len());
    }
    vertices.extend(crate::wire::records::scan_vertex_records(
        &bytes[region_start..],
    ));
    vertices
}

fn parse_vertices(records: &[ZeroEntityRecord]) -> Option<Vec<ZeroVertex>> {
    let mut vertices = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if !matches!(record.tag, [0x05, 0x0b | 0x10 | 0x15]) {
            continue;
        }
        let marker = records.get(index + 1)?;
        if marker.tag != [0x5d, 0x06] {
            return None;
        }
        let (incidence_items, end) = counted_references(&record.bytes, 12)?;
        if end != record.bytes.len()
            || incidence_items.len()
                != match record.tag[1] {
                    0x0b => 2,
                    0x10 => 3,
                    0x15 => 4,
                    _ => unreachable!(),
                }
        {
            return None;
        }
        vertices.push(ZeroVertex {
            marker_ordinal: marker.ordinal,
            incidence_record_ordinal: record.ordinal,
            incidence_items,
        });
    }
    Some(vertices)
}

fn parse_physical_edge(record: &ZeroEntityRecord) -> Option<ZeroPhysicalEdge> {
    let mut references = [0; 6];
    for (target, offset) in references.iter_mut().zip([7usize, 12, 17, 22, 27, 32]) {
        *target = token_u32(&record.bytes, offset)?;
    }
    Some(ZeroPhysicalEdge {
        record_ordinal: record.ordinal,
        references,
    })
}

fn parse_coedge_twin(record: &ZeroEntityRecord) -> Option<ZeroCoedgeTwin> {
    let marker = record
        .bytes
        .get(7..)?
        .windows(1)
        .position(|value| value == [0x83])?
        + 7;
    if record.bytes.get(marker + 1) != Some(&0x10) {
        return None;
    }
    let side = *record.bytes.get(marker + 2)?;
    if !matches!(side, 1 | 2) {
        return None;
    }
    let mut references = Vec::new();
    let mut position = marker + 3;
    while position + 5 <= record.bytes.len() {
        if record.bytes[position] == 0x10 {
            references.push(token_u32(&record.bytes, position)?);
            position += 5;
        } else {
            position += 1;
        }
    }
    Some(ZeroCoedgeTwin {
        record_ordinal: record.ordinal,
        side,
        references,
    })
}

fn parse_side_pairs(
    records: &[ZeroEntityRecord],
    coedges: &[ZeroCoedgeTwin],
) -> Option<Vec<ZeroSidePair>> {
    let mut pairs = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if record.tag != [0x25, 0x69] {
            continue;
        }
        let (references, _) = counted_references(&record.bytes, 12)?;
        let bases: [u32; 2] = references.try_into().ok()?;
        let first = records.get(index + 1)?;
        let second = records.get(index + 2)?;
        let coedge0 = coedges
            .iter()
            .find(|coedge| coedge.record_ordinal == first.ordinal)?;
        let coedge1 = coedges
            .iter()
            .find(|coedge| coedge.record_ordinal == second.ordinal)?;
        if coedge0.side != 1 || coedge1.side != 2 {
            return None;
        }
        let composite_keys = [
            [bases[0].checked_add(1)?, bases[1].checked_add(1)?],
            [bases[0].checked_add(2)?, bases[1].checked_add(2)?],
        ];
        if coedge0.references.get(..2) != Some(&composite_keys[0])
            || coedge1.references.get(..2) != Some(&composite_keys[1])
        {
            return None;
        }
        pairs.push(ZeroSidePair {
            record_ordinal: record.ordinal,
            bases,
            coedge_ordinals: [coedge0.record_ordinal, coedge1.record_ordinal],
            composite_keys,
        });
    }
    Some(pairs)
}

fn bind_face_runs(
    faces: &mut [ZeroEntityFace],
    loops: &mut [ZeroEntityLoop],
    carrier_runs: &[ZeroCarrierRun],
    supports: &[ZeroSupport],
) {
    let mut loop_cursor = 0;
    for (face_index, face) in faces.iter_mut().enumerate() {
        let run = carrier_runs.get(face_index);
        face.carrier_run = run.map(|_| face_index);
        for terminal in &face.loop_terminals {
            let Some(relative) = loops[loop_cursor..]
                .iter()
                .position(|loop_| loop_.terminal_id == *terminal)
            else {
                continue;
            };
            let loop_index = loop_cursor + relative;
            face.loop_indices.push(loop_index);
            loop_cursor = loop_index + 1;
        }
        let Some(run) = run else {
            continue;
        };
        let slot_to_support: std::collections::HashMap<u32, usize> = run
            .support_ordinals
            .iter()
            .filter_map(|ordinal| {
                supports
                    .iter()
                    .position(|support| support.record_ordinal == *ordinal)
                    .map(|index| (supports[index].slot, index))
            })
            .collect();
        for &loop_index in &face.loop_indices {
            let loop_ = &mut loops[loop_index];
            loop_.support_indices = loop_
                .member_ids
                .iter()
                .map(|member| {
                    loop_
                        .terminal_id
                        .checked_sub(*member)
                        .and_then(|slot| slot_to_support.get(&slot).copied())
                })
                .collect();
        }
    }
}

fn parse_carrier_runs(
    records: &[ZeroEntityRecord],
    bytes: &[u8],
) -> Option<(Vec<ZeroCarrierRun>, Vec<ZeroSupport>)> {
    let mut runs = Vec::new();
    let mut supports = Vec::new();
    let mut position = 0;
    while position < records.len() {
        if !matches!(records[position].tag[0], 0x27 | 0x28 | 0x29 | 0x2b | 0x34) {
            position += 1;
            continue;
        }
        let carrier = position;
        let geometry = crate::families::zero_entity::records::zero_entity_surface_at(
            bytes,
            records[carrier].offset,
        );
        position += 1;
        let mut support_ordinals = Vec::new();
        while position < records.len() && records[position].tag[0] == 0x21 {
            let record = &records[position];
            let slot = token_u32(&record.bytes, 12)?;
            let uv_endpoints = support_uv_endpoints(record);
            let expanded;
            let pcurve_record = if let Some(expected_len) = support_logical_len(record.tag) {
                let end = records
                    .get(position + 1)
                    .map_or(bytes.len(), |next| next.offset);
                if end.checked_sub(record.offset) != Some(expected_len) {
                    position += 1;
                    continue;
                }
                expanded = ZeroEntityRecord {
                    ordinal: record.ordinal,
                    offset: record.offset,
                    tag: record.tag,
                    bytes: bytes.get(record.offset..end)?.to_vec(),
                };
                &expanded
            } else {
                record
            };
            let pcurve = geometry
                .as_ref()
                .and_then(|geometry| support_pcurve(pcurve_record, geometry));
            let lifted_endpoints = pcurve
                .as_ref()
                .and_then(|pcurve| {
                    geometry
                        .as_ref()
                        .and_then(|surface| lift_pcurve_endpoints(pcurve, surface))
                })
                .or_else(|| {
                    uv_endpoints
                        .and_then(|uv| geometry.as_ref().and_then(|value| lift_geometry(value, uv)))
                });
            supports.push(ZeroSupport {
                record_ordinal: record.ordinal,
                owner_carrier_ordinal: records[carrier].ordinal,
                slot,
                uv_endpoints,
                pcurve,
                lifted_endpoints,
            });
            support_ordinals.push(record.ordinal);
            position += 1;
        }
        if !support_ordinals.is_empty() {
            runs.push(ZeroCarrierRun {
                carrier_ordinal: records[carrier].ordinal,
                support_ordinals,
                geometry,
            });
        }
    }
    Some((runs, supports))
}

fn lift_geometry(geometry: &SurfaceGeometry, uv: [[f64; 2]; 2]) -> Option<[[f64; 3]; 2]> {
    uv.map(|[u, v]| {
        let neutral = match geometry {
            SurfaceGeometry::Cylinder { radius, .. } => [u / radius, v],
            SurfaceGeometry::Cone { half_angle, .. } => [u, v * half_angle.cos()],
            SurfaceGeometry::Torus {
                major_radius,
                minor_radius,
                ..
            } => [u / major_radius, v / minor_radius],
            SurfaceGeometry::Plane { .. } | SurfaceGeometry::Nurbs(_) => [u, v],
            SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Transformed { .. }
            | SurfaceGeometry::Unknown { .. } => return None,
        };
        let point = cadmpeg_ir::eval::surface_point(geometry, neutral[0], neutral[1])?;
        Some([point.x, point.y, point.z])
    })
    .into_iter()
    .collect::<Option<Vec<_>>>()?
    .try_into()
    .ok()
}

fn support_uv_endpoints(record: &ZeroEntityRecord) -> Option<[[f64; 2]; 2]> {
    let offsets = match record.tag {
        [0x21, 0x71] => [93, 101, 109, 117],
        [0x21, 0x91] => [93, 101, 141, 149],
        [0x21, 0x99] => [93, 101, 125, 133],
        [0x21, 0xd6] => [106, 114, 170, 178],
        [0x21, 0xe8] => [132, 140, 228, 236],
        _ => return None,
    };
    let values = offsets.map(|offset| {
        f64::from_le_bytes(
            record.bytes[offset..offset + 8]
                .try_into()
                .expect("validated record-family offset"),
        )
    });
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some([[values[0], values[1]], [values[2], values[3]]])
}

fn support_pcurve(record: &ZeroEntityRecord, carrier: &SurfaceGeometry) -> Option<PcurveGeometry> {
    let (knot_offsets, multiplicity_offsets, pole_offset, rational) = match record.tag {
        [0x21, 0x45] => (
            &[67, 75, 83, 91, 99, 107][..],
            &[115, 120, 125, 130, 135, 140][..],
            145,
            false,
        ),
        [0x21, 0x71] => (&[67, 75][..], &[83, 88][..], 93, false),
        [0x21, 0x72] => (
            &[67, 75, 83, 91, 99, 107, 115][..],
            &[123, 128, 133, 138, 143, 148, 153][..],
            158,
            false,
        ),
        [0x21, 0x91] => (&[67, 75][..], &[83, 88][..], 93, false),
        [0x21, 0x99] => (&[67, 75][..], &[83, 88][..], 93, true),
        [0x21, 0x9f] => (
            &[67, 75, 83, 91, 99, 107, 115, 123][..],
            &[131, 136, 141, 146, 151, 156, 161, 166][..],
            171,
            false,
        ),
        [0x21, 0xd6] => (&[67, 75, 83][..], &[91, 96, 101][..], 106, true),
        [0x21, 0xe8] => (
            &[67, 75, 83, 91, 99][..],
            &[107, 112, 117, 122, 127][..],
            132,
            false,
        ),
        _ => return None,
    };
    let distinct_knots: Vec<f64> = knot_offsets
        .iter()
        .map(|offset| f64_at(&record.bytes, *offset))
        .collect::<Option<_>>()?;
    if distinct_knots.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    let multiplicities: Vec<u32> = multiplicity_offsets
        .iter()
        .map(|offset| token_u32(&record.bytes, *offset))
        .collect::<Option<_>>()?;
    let degree = multiplicities.first()?.checked_sub(1)?;
    let knot_count = multiplicities.iter().try_fold(0usize, |sum, value| {
        sum.checked_add(usize::try_from(*value).ok()?)
    })?;
    let control_count = knot_count.checked_sub(usize::try_from(degree).ok()? + 1)?;
    if control_count < usize::try_from(degree).ok()? + 1 {
        return None;
    }
    // Each control point consumes 16 bytes (two f64) from `pole_offset` onward;
    // the summed-multiplicity count cannot exceed what the record can hold.
    cadmpeg_ir::cursor::bounded_len(
        control_count as u64,
        16,
        record.bytes.len().saturating_sub(pole_offset),
    )?;
    let mut control_points = Vec::with_capacity(control_count);
    for index in 0..control_count {
        let offset = pole_offset + 16 * index;
        let native = [
            f64_at(&record.bytes, offset)?,
            f64_at(&record.bytes, offset + 8)?,
        ];
        let [u, v] = neutral_uv(native, carrier)?;
        control_points.push(Point2::new(u, v));
    }
    if let Some(endpoints) = support_uv_endpoints(record) {
        let endpoints = endpoints
            .map(|point| neutral_uv(point, carrier))
            .into_iter()
            .collect::<Option<Vec<_>>>()?;
        let first = control_points.first()?;
        let last = control_points.last()?;
        if (first.u - endpoints[0][0])
            .abs()
            .max((first.v - endpoints[0][1]).abs())
            > 1e-9
            || (last.u - endpoints[1][0])
                .abs()
                .max((last.v - endpoints[1][1]).abs())
                > 1e-9
        {
            return None;
        }
    } else if support_logical_len(record.tag).is_none() {
        return None;
    }
    let weights = if rational {
        Some(
            (0..control_count)
                .map(|index| f64_at(&record.bytes, pole_offset + 16 * control_count + 8 * index))
                .collect::<Option<Vec<_>>>()?,
        )
    } else {
        None
    };
    if weights.as_ref().is_some_and(|values| {
        values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    }) {
        return None;
    }
    let knots = distinct_knots
        .into_iter()
        .zip(multiplicities)
        .flat_map(|(knot, count)| std::iter::repeat_n(knot, count as usize))
        .collect();
    Some(PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic: false,
    })
}

fn lift_pcurve_endpoints(
    pcurve: &PcurveGeometry,
    carrier: &SurfaceGeometry,
) -> Option<[[f64; 3]; 2]> {
    let PcurveGeometry::Nurbs { degree, knots, .. } = pcurve else {
        return None;
    };
    let degree = usize::try_from(*degree).ok()?;
    let range = [
        *knots.get(degree)?,
        *knots.get(knots.len().checked_sub(degree + 1)?)?,
    ];
    range
        .map(|parameter| {
            let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter)?;
            let point = cadmpeg_ir::eval::surface_point(carrier, uv.u, uv.v)?;
            Some([point.x, point.y, point.z])
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn neutral_uv(native: [f64; 2], carrier: &SurfaceGeometry) -> Option<[f64; 2]> {
    Some(match carrier {
        SurfaceGeometry::Cylinder { radius, .. } => [native[0] / radius, native[1]],
        SurfaceGeometry::Cone { half_angle, .. } => [native[0], native[1] * half_angle.cos()],
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => [native[0] / major_radius, native[1] / minor_radius],
        SurfaceGeometry::Plane { .. } | SurfaceGeometry::Nurbs(_) => native,
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => return None,
    })
}

fn f64_at(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

fn token_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    if bytes.get(offset) != Some(&0x10) {
        return None;
    }
    u32_at(bytes, offset + 1)
}

fn support_logical_len(tag: [u8; 2]) -> Option<usize> {
    match tag {
        [0x21, 0x45] => Some(337),
        [0x21, 0x72] => Some(382),
        [0x21, 0x9f] => Some(427),
        _ => None,
    }
}

fn walk_records(bytes: &[u8]) -> Vec<ZeroEntityRecord> {
    let mut records = Vec::new();
    let mut position = 0;
    while position + 4 <= bytes.len() {
        if bytes.get(position..position + 2) != Some(&[0xa9, 0x03]) {
            position += 1;
            continue;
        }
        let length = usize::from(bytes[position + 3]) + 12;
        let Some(end) = position.checked_add(length) else {
            break;
        };
        if end > bytes.len() {
            break;
        }
        let tag = [bytes[position + 2], bytes[position + 3]];
        let logical_end = support_logical_len(tag)
            .and_then(|length| position.checked_add(length))
            .unwrap_or(end);
        if logical_end > bytes.len() {
            break;
        }
        records.push(ZeroEntityRecord {
            ordinal: records.len(),
            offset: position,
            tag,
            bytes: bytes[position..end].to_vec(),
        });
        position = logical_end;
    }
    records
}

fn parse_face(record: &ZeroEntityRecord) -> Option<ZeroEntityFace> {
    let (references, _) = counted_references(&record.bytes, 12)?;
    if references.len() < 2 {
        return None;
    }
    let base = references[0];
    let loop_terminals = references[1..]
        .iter()
        .map(|reference| base.checked_sub(*reference))
        .collect::<Option<Vec<_>>>()?;
    Some(ZeroEntityFace {
        record_ordinal: record.ordinal,
        references,
        loop_terminals,
        loop_indices: Vec::new(),
        carrier_run: None,
    })
}

fn parse_loop(record: &ZeroEntityRecord) -> Option<ZeroEntityLoop> {
    let (references, mut position) = counted_references(&record.bytes, 12)?;
    if references.len() < 3 || references.len() % 2 == 0 {
        return None;
    }
    let segment_count = (references.len() - 1) / 2;
    let member_ids: Vec<u32> = references[..references.len() - 1]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    let secondary_refs: Vec<u32> = references[1..references.len() - 1]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    let terminal_id = *references.last()?;
    let gap = terminal_id.checked_sub(*member_ids.first()?)?;
    for (index, member) in member_ids.iter().enumerate() {
        if *member != terminal_id - gap - u32::try_from(index).ok()? {
            return None;
        }
    }
    if record.bytes.get(position) != Some(&(0x80u8.checked_add(u8::try_from(segment_count).ok()?)?))
    {
        return None;
    }
    let loop_class = *record.bytes.get(position + 1)?;
    position += 2;
    let packed_length = (3 * segment_count).div_ceil(8);
    let packed = record.bytes.get(position..position + packed_length)?;
    position += packed_length;
    if record.bytes.get(position) != Some(&0x01) {
        return None;
    }
    let mut reversed = Vec::with_capacity(segment_count);
    for member in 0..segment_count {
        let mut code = 0u8;
        for bit in 0..3 {
            let bit_position = member * 3 + bit;
            code |= ((packed[bit_position / 8] >> (bit_position % 8)) & 1) << bit;
        }
        reversed.push(match code {
            7 => false,
            2 => true,
            _ => return None,
        });
    }
    if matches!(loop_class, 0x41 | 0xc1) && !matches!(gap, 1 | 2) {
        return None;
    }
    Some(ZeroEntityLoop {
        record_ordinal: record.ordinal,
        member_ids,
        secondary_refs,
        terminal_id,
        gap,
        loop_class,
        inner: loop_class == 0x50,
        reversed,
        support_indices: Vec::new(),
    })
}

fn counted_references(bytes: &[u8], position: usize) -> Option<(Vec<u32>, usize)> {
    let count = usize::from(bytes.get(position)?.checked_sub(0x80)?);
    let mut cursor = position + 1;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(cursor) != Some(&0x10) {
            return None;
        }
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        cursor += 5;
    }
    Some((references, cursor))
}

#[cfg(test)]
mod occurrence_tests {
    use super::*;

    #[test]
    fn malformed_extended_support_does_not_discard_other_topology() {
        let carrier = ZeroEntityRecord {
            ordinal: 0,
            offset: 0,
            tag: [0x27, 0x00],
            bytes: vec![0; 12],
        };
        let mut support_bytes = vec![0; 20];
        support_bytes[12] = 0x10;
        let support = ZeroEntityRecord {
            ordinal: 1,
            offset: 12,
            tag: [0x21, 0x45],
            bytes: support_bytes,
        };
        let next = ZeroEntityRecord {
            ordinal: 2,
            offset: 100,
            tag: [0x5f, 0x00],
            bytes: vec![0; 12],
        };
        let (runs, supports) =
            parse_carrier_runs(&[carrier, support, next], &[0; 200]).expect("partial carrier scan");
        assert!(runs.is_empty());
        assert!(supports.is_empty());
    }

    fn support(index: usize, endpoints: Option<[[f64; 3]; 2]>) -> ZeroSupport {
        ZeroSupport {
            record_ordinal: index,
            owner_carrier_ordinal: 0,
            slot: index as u32,
            uv_endpoints: None,
            pcurve: None,
            lifted_endpoints: endpoints,
        }
    }

    fn loop_(support_indices: [usize; 3]) -> ZeroEntityLoop {
        ZeroEntityLoop {
            record_ordinal: 0,
            member_ids: vec![0; 3],
            secondary_refs: vec![0; 3],
            terminal_id: 0,
            gap: 0,
            loop_class: 0x41,
            inner: false,
            reversed: vec![false; 3],
            support_indices: support_indices.into_iter().map(Some).collect(),
        }
    }

    fn face(loop_index: usize) -> ZeroEntityFace {
        ZeroEntityFace {
            record_ordinal: 0,
            references: Vec::new(),
            loop_terminals: Vec::new(),
            loop_indices: vec![loop_index],
            carrier_run: None,
        }
    }

    #[test]
    fn unresolved_face_loop_does_not_discard_later_carrier_bindings() {
        let mut faces = (0..3)
            .map(|index| ZeroEntityFace {
                record_ordinal: index,
                references: Vec::new(),
                loop_terminals: vec![10 + index as u32],
                loop_indices: Vec::new(),
                carrier_run: None,
            })
            .collect::<Vec<_>>();
        let mut loops = [10u32, 12]
            .into_iter()
            .enumerate()
            .map(|(index, terminal_id)| ZeroEntityLoop {
                record_ordinal: index,
                member_ids: Vec::new(),
                secondary_refs: Vec::new(),
                terminal_id,
                gap: 1,
                loop_class: 0x41,
                inner: false,
                reversed: Vec::new(),
                support_indices: Vec::new(),
            })
            .collect::<Vec<_>>();
        let carrier_runs = (0..3)
            .map(|carrier_ordinal| ZeroCarrierRun {
                carrier_ordinal,
                support_ordinals: Vec::new(),
                geometry: None,
            })
            .collect::<Vec<_>>();

        bind_face_runs(&mut faces, &mut loops, &carrier_runs, &[]);

        assert_eq!(faces[0].loop_indices, vec![0]);
        assert!(faces[1].loop_indices.is_empty());
        assert_eq!(faces[2].loop_indices, vec![1]);
        assert_eq!(
            faces
                .iter()
                .map(|face| face.carrier_run)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn rational_inline_support_expands_knots_poles_and_weights() {
        let mut bytes = vec![0; 165];
        bytes[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x99]);
        for (offset, value) in [(67, 0.0f64), (75, 1.0)] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        for offset in [83, 88] {
            bytes[offset] = 0x10;
            bytes[offset + 1..offset + 5].copy_from_slice(&3u32.to_le_bytes());
        }
        for (index, value) in [0.0f64, 0.0, 0.5, 1.0, 1.0, 0.0, 1.0, 0.5, 1.0]
            .into_iter()
            .enumerate()
        {
            let offset = 93 + 8 * index;
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = ZeroEntityRecord {
            ordinal: 0,
            offset: 0,
            tag: [0x21, 0x99],
            bytes,
        };
        let carrier = SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        };
        let Some(PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        }) = support_pcurve(&record, &carrier)
        else {
            panic!("expected rational support pcurve");
        };
        assert_eq!(degree, 2);
        assert_eq!(knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(control_points.len(), 3);
        assert_eq!(weights, Some(vec![1.0, 0.5, 1.0]));
        assert!(!periodic);
    }

    #[test]
    fn extended_support_owns_continuation_and_decodes_inline_poles() {
        let mut bytes = vec![0; 341];
        bytes[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x45]);
        for (index, value) in [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0].into_iter().enumerate() {
            let offset = 67 + 8 * index;
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        for (index, multiplicity) in [4u32, 2, 2, 2, 2, 4].into_iter().enumerate() {
            let offset = 115 + 5 * index;
            bytes[offset] = 0x10;
            bytes[offset + 1..offset + 5].copy_from_slice(&multiplicity.to_le_bytes());
        }
        for index in 0..12 {
            let offset = 145 + 16 * index;
            bytes[offset..offset + 8].copy_from_slice(&(index as f64).to_le_bytes());
            bytes[offset + 8..offset + 16].copy_from_slice(&(2.0 * index as f64).to_le_bytes());
        }
        bytes[337..341].copy_from_slice(&[0xa9, 0x03, 0x5d, 0x06]);

        let records = walk_records(&bytes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].bytes.len(), 81);

        let record = ZeroEntityRecord {
            bytes: bytes[..337].to_vec(),
            ..records[0].clone()
        };
        let carrier = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let Some(PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        }) = support_pcurve(&record, &carrier)
        else {
            panic!("expected extended support pcurve");
        };
        assert_eq!(degree, 3);
        assert_eq!(knots.len(), 16);
        assert_eq!(control_points.len(), 12);
        assert_eq!(control_points[0], Point2::new(0.0, 0.0));
        assert_eq!(control_points[11], Point2::new(11.0, 22.0));
        assert_eq!(weights, None);
        assert!(!periodic);
    }

    #[test]
    fn plane_support_lift_preserves_rational_nurbs_carrier() {
        let pcurve = PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 4.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        };
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(10.0, 20.0, 30.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };

        let geometry = lift_pcurve(&pcurve, &plane).expect("expected lifted NURBS curve");
        let CurveGeometry::Nurbs(curve) = &geometry else {
            panic!("expected lifted NURBS curve");
        };
        assert_eq!(curve.control_points[1], Point3::new(11.0, 22.0, 30.0));
        assert_eq!(curve.weights, Some(vec![1.0, 0.5, 1.0]));
        assert_eq!(
            orient_direct_support_curve(&pcurve, &plane, geometry, [0.0, 1.0], false)
                .expect("oriented plane lift")
                .1,
            Some([0.0, 1.0]),
        );
    }

    #[test]
    fn direct_nurbs_range_snaps_to_domain_and_follows_physical_edge_order() {
        let mut curve = NurbsCurve {
            degree: 1,
            knots: vec![0.02, 0.02, 1.79, 1.79],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        };
        let range = canonical_nurbs_interval(&curve, [0.020000000000000018, 1.7899999999999998])
            .expect("roundoff-equivalent native interval");
        assert_eq!(range, [0.02, 1.79]);

        let range =
            orient_nurbs_to_endpoints(&mut curve, range, [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]])
                .expect("physical edge orientation");
        assert_eq!(range, [0.02, 1.79]);
        assert_eq!(curve.control_points[0], Point3::new(1.0, 0.0, 0.0));
        assert_eq!(curve.control_points[1], Point3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn plane_line_support_uses_its_complete_native_interval() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(10.0, 20.0, 30.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let pcurve = PcurveGeometry::Line {
            origin: Point2::new(2.0, -1.0),
            direction: Point2::new(3.0, 4.0),
        };
        let curve = lift_pcurve(&pcurve, &plane).expect("lifted plane line");
        let (curve, range) =
            orient_direct_support_curve(&pcurve, &plane, curve, [1.0, -2.0], false)
                .expect("oriented complete plane line");
        assert_eq!(range, Some([0.0, 15.0]));
        let CurveGeometry::Line { origin, direction } = curve else {
            panic!("expected exact line carrier");
        };
        assert_eq!(origin, Point3::new(15.0, 23.0, 30.0));
        assert_eq!(direction, Vector3::new(-0.6, -0.8, 0.0));
    }

    #[test]
    fn cylinder_isoparametric_support_lifts_to_circle() {
        let pcurve = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 4.0), Point2::new(1.0, 4.0)],
            weights: None,
            periodic: false,
        };
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };

        let geometry = lift_pcurve(&pcurve, &cylinder).expect("expected lifted circle");
        let CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } = &geometry
        else {
            panic!("expected lifted circle");
        };
        assert_eq!(*center, Point3::new(1.0, 2.0, 7.0));
        assert_eq!(*axis, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(*radius, 2.0);
        let (oriented, range) =
            orient_direct_support_curve(&pcurve, &cylinder, geometry, [0.0, 1.0], false)
                .expect("oriented circle lift");
        assert_eq!(range, Some([0.0, 1.0]));
        let (reversed, range) =
            orient_direct_support_curve(&pcurve, &cylinder, oriented, [0.0, 1.0], true)
                .expect("reversed circle lift");
        assert_eq!(
            range,
            Some([std::f64::consts::TAU - 1.0, std::f64::consts::TAU])
        );
        assert!(matches!(
            reversed,
            CurveGeometry::Circle { axis, .. } if axis == Vector3::new(0.0, 0.0, -1.0)
        ));

        let generator = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.5, -2.0), Point2::new(0.5, 3.0)],
            weights: None,
            periodic: false,
        };
        let line = lift_pcurve(&generator, &cylinder).expect("lifted generator");
        let (line, range) =
            orient_direct_support_curve(&generator, &cylinder, line, [0.0, 1.0], false)
                .expect("oriented generator");
        assert_eq!(range, Some([0.0, 5.0]));
        let CurveGeometry::Line { origin, direction } = line else {
            panic!("expected oriented generator line");
        };
        assert!(
            point_distance(
                [origin.x, origin.y, origin.z],
                [1.0 + 2.0 * 0.5_f64.cos(), 2.0 + 2.0 * 0.5_f64.sin(), 1.0,]
            ) < 1e-12
        );
        assert!(direction.x.abs() < 1e-12 && direction.y.abs() < 1e-12);
        assert!((direction.z - 1.0).abs() < 1e-12);

        let line = lift_pcurve(&generator, &cylinder).expect("lifted generator");
        let (line, range) =
            orient_direct_support_curve(&generator, &cylinder, line, [0.0, 1.0], true)
                .expect("reversed generator");
        assert_eq!(range, Some([0.0, 5.0]));
        let CurveGeometry::Line { origin, direction } = line else {
            panic!("expected reversed generator line");
        };
        assert!(
            point_distance(
                [origin.x, origin.y, origin.z],
                [1.0 + 2.0 * 0.5_f64.cos(), 2.0 + 2.0 * 0.5_f64.sin(), 6.0,]
            ) < 1e-12
        );
        assert!(direction.x.abs() < 1e-12 && direction.y.abs() < 1e-12);
        assert!((direction.z + 1.0).abs() < 1e-12);
    }

    #[test]
    fn conic_support_range_tracks_signed_chart_direction_and_rejects_turnbacks() {
        let cone = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
            ratio: -0.5,
            half_angle: 0.25,
        };
        let latitude = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 2.0), Point2::new(1.0, 2.0)],
            weights: None,
            periodic: false,
        };
        let ellipse = lift_pcurve(&latitude, &cone).expect("lifted cone latitude");
        let (ellipse, range) =
            orient_direct_support_curve(&latitude, &cone, ellipse, [0.0, 1.0], false)
                .expect("oriented cone latitude");
        assert_eq!(range, Some([0.0, 1.0]));
        assert!(matches!(
            ellipse,
            CurveGeometry::Ellipse { axis, .. } if axis == Vector3::new(0.0, 0.0, -1.0)
        ));

        let turning = PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(0.0, 2.0),
                Point2::new(1.0, 2.0),
                Point2::new(0.5, 2.0),
            ],
            weights: None,
            periodic: false,
        };
        let ellipse = lift_pcurve(&turning, &cone).expect("lifted turning latitude");
        assert_eq!(
            orient_direct_support_curve(&turning, &cone, ellipse, [0.0, 1.0], false)
                .expect("untrimmed turning latitude")
                .1,
            None,
        );
    }

    #[test]
    fn sphere_isoparametric_supports_lift_to_oriented_circles() {
        let sphere = SurfaceGeometry::Sphere {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let latitude = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 2.0), Point2::new(1.0, 2.0)],
            weights: None,
            periodic: false,
        };
        let circle = lift_pcurve(&latitude, &sphere).expect("southern latitude");
        let (circle, range) =
            orient_direct_support_curve(&latitude, &sphere, circle, [0.0, 1.0], false)
                .expect("oriented southern latitude");
        assert_eq!(range, Some([0.0, 1.0]));
        assert!(matches!(
            circle,
            CurveGeometry::Circle { ref_direction, radius, .. }
                if ref_direction == Vector3::new(-1.0, 0.0, 0.0)
                    && (radius - 4.0 * 2.0_f64.cos().abs()).abs() < 1e-12
        ));

        let meridian = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.4, -1.0), Point2::new(0.4, 1.5)],
            weights: None,
            periodic: false,
        };
        let circle = lift_pcurve(&meridian, &sphere).expect("sphere meridian");
        let (circle, range) =
            orient_direct_support_curve(&meridian, &sphere, circle, [0.0, 1.0], true)
                .expect("reversed sphere meridian");
        assert!(range.is_some_and(|range| (range[1] - range[0] - 2.5).abs() < 1e-12));
        assert!(matches!(circle, CurveGeometry::Circle { radius, .. } if radius == 4.0));

        let negative_sphere = SurfaceGeometry::Sphere {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: -4.0,
        };
        let circle = lift_pcurve(&meridian, &negative_sphere).expect("signed sphere meridian");
        let (circle, range) =
            orient_direct_support_curve(&meridian, &negative_sphere, circle, [0.0, 1.0], false)
                .expect("oriented signed sphere meridian");
        assert!(range.is_some_and(|range| (range[1] - range[0] - 2.5).abs() < 1e-12));
        assert!(matches!(circle, CurveGeometry::Circle { radius, .. } if radius == 4.0));

        let pole = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(0.0, std::f64::consts::FRAC_PI_2),
                Point2::new(1.0, std::f64::consts::FRAC_PI_2),
            ],
            weights: None,
            periodic: false,
        };
        assert_eq!(lift_pcurve(&pole, &sphere), None);
    }

    #[test]
    fn affine_cylinder_support_retains_oriented_helix_and_bounded_cache() {
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let pcurve = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(0.0, 1.0),
                Point2::new(std::f64::consts::PI, 4.0),
            ],
            weights: None,
            periodic: false,
        };
        let helix = lift_cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], false)
            .expect("affine cylinder helix");
        assert_eq!(helix.parameter_range, Some([0.0, std::f64::consts::PI]));
        assert!(helix
            .cache_fit_tolerance
            .is_some_and(|tolerance| tolerance <= 1e-4));
        let Some(ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            ..
        }) = &helix.construction
        else {
            panic!("expected exact helix construction");
        };
        assert_eq!(*angle_range, [0.0, std::f64::consts::PI]);
        assert_eq!(*center, Point3::new(1.0, 2.0, 4.0));
        assert!((pitch.z - 6.0).abs() < 1e-12);
        let CurveGeometry::Nurbs(cache) = &helix.geometry else {
            panic!("expected bounded helix cache");
        };
        assert_eq!(
            cache.control_points.first(),
            Some(&Point3::new(3.0, 2.0, 4.0))
        );
        assert!(
            point_distance(
                cache
                    .control_points
                    .last()
                    .map(|point| [point.x, point.y, point.z])
                    .expect("cache endpoint"),
                [-1.0, 2.0, 7.0],
            ) < 1e-12
        );

        let reversed = lift_cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], true)
            .expect("reversed affine cylinder helix");
        let Some(ProceduralCurveDefinition::Helix { center, pitch, .. }) = reversed.construction
        else {
            panic!("expected reversed helix construction");
        };
        assert_eq!(center, Point3::new(1.0, 2.0, 7.0));
        assert!((pitch.z + 6.0).abs() < 1e-12);

        let isoparametric = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 1.0), Point2::new(0.0, 4.0)],
            weights: None,
            periodic: false,
        };
        assert_eq!(
            lift_cylinder_helix(&isoparametric, &cylinder, [0.0, 1.0], false),
            None,
        );

        let unbounded = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(0.0, 1.0),
                Point2::new(std::f64::consts::TAU * 5_000.0, 4.0),
            ],
            weights: None,
            periodic: false,
        };
        assert_eq!(
            lift_cylinder_helix(&unbounded, &cylinder, [0.0, 1.0], false),
            None,
        );
    }

    #[test]
    fn paired_support_traces_produce_bounded_intersection_cache() {
        let pcurve = |v| PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, v), Point2::new(1.0, v)],
            weights: None,
            periodic: false,
        };
        let mut first = support(0, Some([[0.0; 3]; 2]));
        first.owner_carrier_ordinal = 10;
        first.pcurve = Some(pcurve(0.0));
        let mut second = support(1, Some([[0.0; 3]; 2]));
        second.owner_carrier_ordinal = 11;
        second.pcurve = Some(pcurve(0.008));
        let plane = || SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let topology = ZeroEntityTopology {
            records: Vec::new(),
            faces: Vec::new(),
            loops: Vec::new(),
            carrier_runs: vec![
                ZeroCarrierRun {
                    carrier_ordinal: 10,
                    support_ordinals: vec![0],
                    geometry: Some(plane()),
                },
                ZeroCarrierRun {
                    carrier_ordinal: 11,
                    support_ordinals: vec![1],
                    geometry: Some(plane()),
                },
            ],
            supports: vec![first, second],
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let edge = ZeroResolvedEdge {
            endpoints: [[0.0; 3]; 2],
            occurrences: [
                ZeroResolvedOccurrence {
                    loop_index: 0,
                    member_index: 0,
                    support_index: 0,
                },
                ZeroResolvedOccurrence {
                    loop_index: 1,
                    member_index: 0,
                    support_index: 1,
                },
            ],
            occurrence_endpoints: [[[0.0; 3]; 2]; 2],
        };

        let intersection = intersection_curve(&topology, &edge).expect("intersection cache");
        assert_eq!(intersection.parameter_range, [0.0, 1.0]);
        assert_eq!(
            intersection.cache.control_points[0],
            Point3::new(0.0, 0.004, 0.0)
        );
        assert!((intersection.fit_tolerance - 0.0041).abs() < 1e-12);
    }

    #[test]
    fn degenerate_support_pair_retains_plane_torus_intersection_branch() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 7.7, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let torus = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 8.2,
            minor_radius: 2.0,
        };
        let start = [6.689_544_080_129_829, 7.7, 0.0];
        let end = [2.819_574_435_974_341, 7.7, -2.0];
        let mut supports = [
            support(0, Some([start, end])),
            support(1, Some([start, end])),
        ];
        supports[0].owner_carrier_ordinal = 10;
        supports[1].owner_carrier_ordinal = 11;
        let topology = ZeroEntityTopology {
            records: vec![
                ZeroEntityRecord {
                    ordinal: 0,
                    offset: 0,
                    tag: [0x21, 0x18],
                    bytes: Vec::new(),
                },
                ZeroEntityRecord {
                    ordinal: 1,
                    offset: 0,
                    tag: [0x21, 0x18],
                    bytes: Vec::new(),
                },
            ],
            faces: Vec::new(),
            loops: Vec::new(),
            carrier_runs: vec![
                ZeroCarrierRun {
                    carrier_ordinal: 10,
                    support_ordinals: vec![0],
                    geometry: Some(plane),
                },
                ZeroCarrierRun {
                    carrier_ordinal: 11,
                    support_ordinals: vec![1],
                    geometry: Some(torus),
                },
            ],
            supports: supports.into(),
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let edge = ZeroResolvedEdge {
            endpoints: [start, end],
            occurrences: [
                ZeroResolvedOccurrence {
                    loop_index: 0,
                    member_index: 0,
                    support_index: 0,
                },
                ZeroResolvedOccurrence {
                    loop_index: 1,
                    member_index: 0,
                    support_index: 1,
                },
            ],
            occurrence_endpoints: [[start, end], [start, end]],
        };

        let intersection = intersection_curve(&topology, &edge).expect("plane-torus branch");
        assert!((intersection.parameter_range[1] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let first = intersection
            .cache
            .control_points
            .first()
            .expect("cache start");
        let last = intersection.cache.control_points.last().expect("cache end");
        assert!(point_distance([first.x, first.y, first.z], start) < 1e-12);
        assert!(point_distance([last.x, last.y, last.z], end) < 1e-12);
        assert!(intersection.cache.control_points.len() > 2);
        assert_eq!(intersection.fit_tolerance, 1e-4);
    }

    #[test]
    fn degenerate_support_pair_retains_parallel_plane_cylinder_line() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 3.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        };
        let start = [4.0, 3.0, 2.0];
        let end = [4.0, 3.0, 7.0];
        let mut supports = [
            support(0, Some([start, end])),
            support(1, Some([start, end])),
        ];
        supports[0].owner_carrier_ordinal = 10;
        supports[1].owner_carrier_ordinal = 11;
        let mut topology = ZeroEntityTopology {
            records: vec![
                ZeroEntityRecord {
                    ordinal: 0,
                    offset: 0,
                    tag: [0x21, 0x18],
                    bytes: Vec::new(),
                },
                ZeroEntityRecord {
                    ordinal: 1,
                    offset: 0,
                    tag: [0x21, 0x18],
                    bytes: Vec::new(),
                },
            ],
            faces: Vec::new(),
            loops: Vec::new(),
            carrier_runs: vec![
                ZeroCarrierRun {
                    carrier_ordinal: 10,
                    support_ordinals: vec![0],
                    geometry: Some(plane),
                },
                ZeroCarrierRun {
                    carrier_ordinal: 11,
                    support_ordinals: vec![1],
                    geometry: Some(cylinder),
                },
            ],
            supports: supports.into(),
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let mut edge = ZeroResolvedEdge {
            endpoints: [start, end],
            occurrences: [
                ZeroResolvedOccurrence {
                    loop_index: 0,
                    member_index: 0,
                    support_index: 0,
                },
                ZeroResolvedOccurrence {
                    loop_index: 1,
                    member_index: 0,
                    support_index: 1,
                },
            ],
            occurrence_endpoints: [[start, end], [start, end]],
        };

        let intersection =
            intersection_curve(&topology, &edge).expect("parallel plane-cylinder line");
        assert_eq!(intersection.parameter_range, [0.0, 5.0]);
        assert_eq!(
            intersection.cache.control_points,
            [Point3::new(4.0, 3.0, 2.0), Point3::new(4.0, 3.0, 7.0)]
        );
        assert_eq!(intersection.fit_tolerance, 0.0);

        edge.endpoints = [end, start];
        let reversed = intersection_curve(&topology, &edge).expect("reversed plane-cylinder line");
        assert_eq!(reversed.parameter_range, [0.0, 5.0]);
        assert_eq!(
            reversed.cache.control_points,
            [Point3::new(4.0, 3.0, 7.0), Point3::new(4.0, 3.0, 2.0)]
        );

        topology.carrier_runs[0].geometry = Some(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 5.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        });
        edge.endpoints = [[0.0, 5.0, 2.0], [0.0, 5.0, 7.0]];
        let tangent = intersection_curve(&topology, &edge).expect("tangent plane-cylinder line");
        assert_eq!(
            tangent.cache.control_points,
            [Point3::new(0.0, 5.0, 2.0), Point3::new(0.0, 5.0, 7.0)]
        );
    }

    #[test]
    fn isolated_unlifted_occurrence_closes_and_pairs_from_neighbors() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [0.0, 1.0, 0.0];
        let topology = ZeroEntityTopology {
            records: Vec::new(),
            faces: Vec::new(),
            loops: vec![loop_([0, 1, 2]), loop_([3, 4, 5])],
            carrier_runs: Vec::new(),
            supports: vec![
                support(0, Some([a, b])),
                support(1, None),
                support(2, Some([c, a])),
                support(3, Some([b, a])),
                support(4, Some([a, c])),
                support(5, Some([c, b])),
            ],
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let edges = resolve_occurrence_edges(&topology);
        assert_eq!(edges.len(), 3);
        assert!(edges
            .iter()
            .any(|edge| same_endpoint_pair(edge.endpoints, [b, c], 1e-12)));
    }

    #[test]
    fn coincident_edges_partition_by_established_face_components() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [0.0, 1.0, 0.0];
        let d = [0.0, -1.0, 0.0];
        let triangles = [[a, b, c], [b, a, c], [a, b, d], [b, a, d]];
        let mut supports = Vec::new();
        let mut loops = Vec::new();
        for (face_index, triangle) in triangles.into_iter().enumerate() {
            let base = supports.len();
            supports.extend([
                support(base, Some([triangle[0], triangle[1]])),
                support(base + 1, Some([triangle[1], triangle[2]])),
                support(base + 2, Some([triangle[2], triangle[0]])),
            ]);
            let mut loop_ = loop_([base, base + 1, base + 2]);
            loop_.record_ordinal = face_index;
            loops.push(loop_);
        }
        let topology = ZeroEntityTopology {
            records: Vec::new(),
            faces: (0..4).map(face).collect(),
            loops,
            carrier_runs: Vec::new(),
            supports,
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let edges = resolve_occurrence_edges(&topology);
        assert_eq!(edges.len(), 6);
        let coincident: Vec<_> = edges
            .iter()
            .filter(|edge| same_endpoint_pair(edge.endpoints, [a, b], 1e-12))
            .collect();
        assert_eq!(coincident.len(), 2);
        let face_pairs: Vec<Vec<usize>> = coincident
            .iter()
            .map(|edge| {
                let mut faces: Vec<_> = edge
                    .occurrences
                    .iter()
                    .map(|occurrence| occurrence.loop_index)
                    .collect();
                faces.sort_unstable();
                faces
            })
            .collect();
        assert!(face_pairs.contains(&vec![0, 1]));
        assert!(face_pairs.contains(&vec![2, 3]));
    }
}
