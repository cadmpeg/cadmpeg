// SPDX-License-Identifier: Apache-2.0
//! `SP_CURVE` `00 89`: a curve given by a parameter-space B-curve on a support
//! surface, and the bounded `00 85` uses of one that carry a tolerant edge's
//! geometry (spec §7.6).

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::units::FinitePoint2;

use super::sweep::SweepKind;
use super::LEN_TO_MM;
use cadmpeg_parasolid::pointer::read_at as read_ref;

const TAG: u8 = 0x89;

/// One `00 89` record whose `b_curve` decodes.
#[derive(Debug, Clone)]
pub(super) struct SpCurve {
    pub(super) attr: u32,
    pub(super) offset: usize,
    /// The support surface's carrier `attr`.
    pub(super) surface: u32,
    /// The `b_curve`, in the support's stored parameters.
    pub(super) pcurve: PcurveNurbs,
}

/// A `00 85` bounded curve on an `SP_CURVE` (spec §7.6).
#[derive(Debug, Clone)]
pub(super) struct SurfaceCurveCarrier {
    pub(super) attr: u32,
    pub(super) offset: usize,
    /// The support surface's carrier `attr`.
    pub(super) surface: u32,
    pub(super) pcurve: PcurveGeometry,
    pub(super) parameter_range: [f64; 2],
    /// Start and end points, millimetres.
    pub(super) points: [Point3; 2],
    /// Set when the support is a swept or spun construction; `pcurve` is then
    /// in its stored parameters.
    pub(super) sweep: Option<SweepSupport>,
    /// Set when the support is an offset; `pcurve` is in the base carrier's
    /// parameters.
    pub(super) offset_support: Option<OffsetSupport>,
}

/// An offset of a surface carrier as an `SP_CURVE` support: the carrier, and
/// the signed distance along its natural unit normal (spec §7.5).
#[derive(Debug, Clone)]
pub(super) struct OffsetSupport {
    pub(super) base: SurfaceGeometry,
    pub(super) distance: f64,
}

impl OffsetSupport {
    /// The model-space point at `uv`, the carrier's IR parameters, millimetres.
    pub(super) fn point(
        &self,
        ctx: &DecodeContext<'_>,
        uv: Point2,
    ) -> Result<Option<Point3>, CodecError> {
        let Some(partials) = super::evaluation::surface_partials(ctx, &self.base, uv.u, uv.v)?
        else {
            return Ok(None);
        };
        let partials = partials.into_raw();
        let normal = partials.du.cross(partials.dv);
        let length = normal.dot(normal).sqrt();
        if !(length.is_finite() && length > 0.0) {
            return Ok(None);
        }
        let along = self.distance / length;
        let point = Point3::new(
            partials.point.x + normal.x * along,
            partials.point.y + normal.y * along,
            partials.point.z + normal.z * along,
        );
        let finite = point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
        Ok(finite.then_some(point))
    }
}

/// A swept or spun support, evaluated in its stored parameters: `u` is the
/// profile's own parameter, and `v` the spin angle in radians, right-handed
/// about the axis, or the travel along the sweep direction in metres.
#[derive(Debug, Clone)]
pub(super) struct SweepSupport {
    pub(super) profile: CurveGeometry,
    pub(super) kind: SweepKind,
}

impl SweepSupport {
    /// The model-space point at stored parameters `uv`, millimetres.
    pub(super) fn point(
        &self,
        ctx: &DecodeContext<'_>,
        uv: Point2,
    ) -> Result<Option<Point3>, CodecError> {
        let Some(on_profile) = super::subset::point_at(ctx, &self.profile, uv.u)? else {
            return Ok(None);
        };
        let point = match &self.kind {
            SweepKind::Swept { direction } => {
                let direction = direction.as_raw();
                let travel = uv.v * LEN_TO_MM;
                Point3::new(
                    on_profile.x + direction.x * travel,
                    on_profile.y + direction.y * travel,
                    on_profile.z + direction.z * travel,
                )
            }
            SweepKind::Spun { base, axis, .. } => {
                // Rodrigues' rotation of the profile point about the axis.
                let axis = axis.as_raw();
                let offset = Vector3::new(
                    on_profile.x - base.x,
                    on_profile.y - base.y,
                    on_profile.z - base.z,
                );
                let (sine, cosine) = uv.v.sin_cos();
                let across = axis.cross(offset);
                let along = axis.dot(offset) * (1.0 - cosine);
                Point3::new(
                    base.x + offset.x * cosine + across.x * sine + axis.x * along,
                    base.y + offset.y * cosine + across.y * sine + axis.y * along,
                    base.z + offset.z * cosine + across.z * sine + axis.z * along,
                )
            }
        };
        let finite = point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
        Ok(finite.then_some(point))
    }

    /// Whether `pcurve` over `[low, high]` spans at most two pieces of a
    /// B-curve profile.
    fn crosses_one_knot(
        &self,
        ctx: &DecodeContext<'_>,
        pcurve: &PcurveGeometry,
        low: f64,
        high: f64,
    ) -> Result<bool, CodecError> {
        let CurveGeometry::Solved(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(profile)) =
            &self.profile
        else {
            return Ok(true);
        };
        let mut at = [0.0; 3];
        for (slot, t) in at.iter_mut().zip([low, 0.5 * (low + high), high]) {
            let Some(uv) = super::evaluation::pcurve_uv(ctx, pcurve, t)? else {
                return Ok(true);
            };
            *slot = uv.get().u;
        }
        let pieces = at.map(|value| distinct_knots_at_or_below(profile.knots(), value));
        let most = pieces.iter().max().copied().map_or(0, |most| most);
        let least = pieces.iter().min().copied().map_or(0, |least| least);
        Ok(most - least <= 1)
    }
}

/// An `SP_CURVE` support as the carrier index finds it: a surface carrier,
/// or a swept, spun or offset construction built from the index.
pub(super) enum ResolvedSupport<'a> {
    Surface(&'a SurfaceGeometry),
    Sweep(SweepSupport),
    Offset(OffsetSupport),
}

impl ResolvedSupport<'_> {
    pub(super) fn support(&self) -> Support<'_> {
        match self {
            Self::Surface(surface) => Support::Surface(surface),
            Self::Sweep(sweep) => Support::Sweep(sweep),
            Self::Offset(offset) => Support::Offset(offset),
        }
    }
}

/// What an `SP_CURVE`'s pcurve is evaluated on.
#[derive(Clone, Copy)]
pub(super) enum Support<'a> {
    /// A surface carrier; the pcurve is in the IR's parameters of it.
    Surface(&'a SurfaceGeometry),
    /// A swept or spun construction; the pcurve is in its stored parameters.
    Sweep(&'a SweepSupport),
    /// An offset of a surface carrier; the pcurve is in the IR's parameters
    /// of that carrier.
    Offset(&'a OffsetSupport),
}

impl<'a> Support<'a> {
    /// `pcurve`, a record's B-curve in the support's stored parameters, in
    /// the parameters this support is evaluated in.
    pub(super) fn pcurve(self, pcurve: &PcurveNurbs) -> Option<PcurveGeometry> {
        match self {
            Self::Surface(surface) => normalized(pcurve, surface),
            Self::Sweep(_) => Some(PcurveGeometry::Nurbs {
                nurbs: pcurve.clone(),
            }),
            Self::Offset(offset) => normalized(pcurve, &offset.base),
        }
    }

    /// The model-space point of `pcurve` at `t`.
    pub(super) fn point(
        self,
        ctx: &DecodeContext<'_>,
        pcurve: &PcurveGeometry,
        t: f64,
    ) -> Result<Option<Point3>, CodecError> {
        match self {
            Self::Surface(surface) => point_at(ctx, surface, pcurve, t),
            Self::Sweep(sweep) => {
                let Some(uv) = super::evaluation::pcurve_uv(ctx, pcurve, t)? else {
                    return Ok(None);
                };
                sweep.point(ctx, uv.get())
            }
            Self::Offset(offset) => {
                let Some(uv) = super::evaluation::pcurve_uv(ctx, pcurve, t)? else {
                    return Ok(None);
                };
                offset.point(ctx, uv.get())
            }
        }
    }

    /// Whether a fit's span from `low` to `high` is short enough for its
    /// samples to judge it.
    pub(super) fn resolves(
        self,
        ctx: &DecodeContext<'_>,
        pcurve: &PcurveGeometry,
        low: f64,
        high: f64,
    ) -> Result<bool, CodecError> {
        match self {
            Self::Surface(surface) => crosses_one_knot_line(ctx, surface, pcurve, low, high),
            Self::Sweep(sweep) => sweep.crosses_one_knot(ctx, pcurve, low, high),
            Self::Offset(offset) => crosses_one_knot_line(ctx, &offset.base, pcurve, low, high),
        }
    }

    pub(super) fn sweep(self) -> Option<&'a SweepSupport> {
        match self {
            Self::Sweep(sweep) => Some(sweep),
            Self::Surface(_) | Self::Offset(_) => None,
        }
    }

    pub(super) fn offset(self) -> Option<&'a OffsetSupport> {
        match self {
            Self::Offset(offset) => Some(offset),
            Self::Surface(_) | Self::Sweep(_) => None,
        }
    }
}

/// The count of distinct knots at or below `value`: the piece it is in.
fn distinct_knots_at_or_below(knots: &[f64], value: f64) -> usize {
    let mut pieces = 0_usize;
    let mut previous = None;
    for knot in knots {
        if previous != Some(*knot) && *knot <= value {
            pieces += 1;
        }
        previous = Some(*knot);
    }
    pieces
}

/// The `surface` and `b_curve` references after the sense marker at
/// `marker` (spec §7.6).
fn support_refs(bytes: &[u8], marker: usize) -> Option<(u32, u32)> {
    let mut at = marker.checked_add(1)?;
    let surface = read_ref(bytes, &mut at)?;
    let b_curve = read_ref(bytes, &mut at)?;
    Some((surface, b_curve))
}

struct Header {
    attr: u32,
    surface: u32,
    b_curve: u32,
}

fn native_header(bytes: &[u8], start: usize) -> Option<(Header, usize)> {
    let mut at = start;
    let attr = read_ref(bytes, &mut at)?;
    (attr > 1).then_some(())?;
    View::u32_be_at(bytes, at)?;
    at += 4;
    super::native_refs::<5>(bytes, &mut at)?;
    matches!(bytes.get(at), Some(b'+' | b'-')).then_some(())?;
    let (surface, b_curve) = support_refs(bytes, at)?;
    at += 1;
    super::native_refs::<3>(bytes, &mut at)?;
    View::f64_be_at(bytes, at)?.is_finite().then_some(())?;
    Some((
        Header {
            attr,
            surface,
            b_curve,
        },
        at + 8,
    ))
}

pub(super) fn record_end(bytes: &[u8], off: usize) -> Option<usize> {
    super::record_starts(bytes, off, TAG)
        .filter_map(|start| native_header(bytes, start).map(|(_, end)| end))
        .min()
}

/// `00 89` records whose `b_curve` is in `pcurves`.
pub(super) fn scan(bytes: &[u8], pcurves: &HashMap<u32, PcurveNurbs>) -> HashMap<u32, SpCurve> {
    let mut out = HashMap::new();
    if pcurves.is_empty() {
        return out;
    }
    for off in 0..bytes.len().checked_sub(2).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, TAG]) {
            continue;
        }
        for start in super::record_starts(bytes, off, TAG) {
            let native = native_header(bytes, start)
                .map(|(header, _)| (header.attr, header.surface, header.b_curve));
            let legacy = super::word_at(bytes, start)
                .filter(|attr| (2..=0x7fff).contains(attr))
                .and_then(|attr| {
                    super::analytic_marker_candidates(bytes, start)?
                        .into_iter()
                        .filter(|marker| {
                            *marker != start + crate::layout::compact_analytic_header::MARKER
                        })
                        .filter_map(|marker| support_refs(bytes, marker))
                        .find_map(|(surface, b_curve)| {
                            pcurves
                                .contains_key(&b_curve)
                                .then_some((attr, surface, b_curve))
                        })
                });
            for (attr, surface, b_curve) in native.into_iter().chain(legacy) {
                let Some(pcurve) = pcurves.get(&b_curve).filter(|_| surface > 1) else {
                    continue;
                };
                out.entry(attr).or_insert_with(|| SpCurve {
                    attr,
                    offset: off,
                    surface,
                    pcurve: pcurve.clone(),
                });
            }
        }
    }
    out
}

/// A support's stored parameters as the IR's (spec §7.1).
fn surface_parameters(surface: &SolvedSurfaceGeometry, uv: Point2) -> Option<FinitePoint2> {
    let point = match surface {
        SolvedSurfaceGeometry::Plane(_) => Point2::new(uv.u * LEN_TO_MM, uv.v * LEN_TO_MM),
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => {
            Point2::new(uv.u, uv.v * LEN_TO_MM)
        }
        SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_)
        | SolvedSurfaceGeometry::Nurbs(_) => uv,
        SolvedSurfaceGeometry::Transformed(placed) => {
            return surface_parameters(placed.basis(), uv)
        }
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => return None,
    };
    FinitePoint2::new(point)
}

/// `pcurve` in the IR's parameters of `surface`.
pub(super) fn normalized(
    pcurve: &PcurveNurbs,
    surface: &SurfaceGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Solved(surface) = surface else {
        return None;
    };
    let mut nurbs = pcurve.clone();
    let mapped = nurbs.try_map_control_points_in_place(|point| {
        surface_parameters(surface, point.get()).ok_or(())
    });
    mapped.is_ok().then_some(PcurveGeometry::Nurbs { nurbs })
}

/// The model-space point of `pcurve` at `t` on `surface`.
pub(super) fn point_at(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    t: f64,
) -> Result<Option<Point3>, CodecError> {
    let Some(uv) = super::evaluation::pcurve_uv(ctx, pcurve, t)? else {
        return Ok(None);
    };
    let uv = uv.get();
    let point = match surface {
        // A B-surface support is evaluated directly, for its point alone.
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            super::evaluation::nurbs_surface_point(ctx, nurbs, uv.u, uv.v)?
        }
        _ => super::evaluation::surface_point(ctx, surface, uv.u, uv.v)?,
    };
    Ok(point.map(cadmpeg_ir::features::FinitePoint3::get))
}

/// A bare `SP_CURVE` over its whole knot domain (spec §7.6).
pub(super) fn whole(
    ctx: &DecodeContext<'_>,
    curve: &SpCurve,
    support: Support<'_>,
) -> Result<Option<SurfaceCurveCarrier>, CodecError> {
    let Some(pcurve) = support.pcurve(&curve.pcurve) else {
        return Ok(None);
    };
    let knots = curve.pcurve.knots();
    let Some((low, high)) = usize::try_from(curve.pcurve.degree())
        .ok()
        .and_then(|degree| {
            let low = *knots.get(degree)?;
            let high = *knots.get(knots.len().checked_sub(degree + 1)?)?;
            Some((low, high))
        })
    else {
        return Ok(None);
    };
    if !(low.is_finite() && high.is_finite() && low < high) {
        return Ok(None);
    }
    let (Some(start), Some(end)) = (
        support.point(ctx, &pcurve, low)?,
        support.point(ctx, &pcurve, high)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(SurfaceCurveCarrier {
        attr: curve.attr,
        offset: curve.offset,
        surface: curve.surface,
        pcurve,
        parameter_range: [low, high],
        points: [start, end],
        sweep: support.sweep().cloned(),
        offset_support: support.offset().cloned(),
    }))
}

/// Whether `carrier`'s ends match `ends` in exactly one order within
/// `tolerance_mm`.
pub(super) fn reaches(carrier: &SurfaceCurveCarrier, ends: [Point3; 2], tolerance_mm: f64) -> bool {
    let [low, high] = carrier.points;
    let within = |a: Point3, b: Point3| a.distance(b) <= tolerance_mm;
    let forward = within(low, ends[0]) && within(high, ends[1]);
    let backward = within(low, ends[1]) && within(high, ends[0]);
    forward != backward
}

/// Whether `pcurve` over `[low, high]` crosses at most one knot line of a
/// B-surface in each direction (spec §7.6).
pub(super) fn crosses_one_knot_line(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    low: f64,
    high: f64,
) -> Result<bool, CodecError> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) = surface else {
        return Ok(true);
    };
    let mut at = [Point2::new(0.0, 0.0); 3];
    for (slot, t) in at.iter_mut().zip([low, 0.5 * (low + high), high]) {
        let Some(uv) = super::evaluation::pcurve_uv(ctx, pcurve, t)? else {
            return Ok(true);
        };
        *slot = uv.get();
    }
    // The count of distinct knots at or below a parameter is its piece.
    let piece = distinct_knots_at_or_below;
    let within_two = |pieces: [usize; 3]| {
        let most = pieces.iter().max().copied().map_or(0, |most| most);
        let least = pieces.iter().min().copied().map_or(0, |least| least);
        most - least <= 1
    };
    Ok(
        within_two([0, 1, 2].map(|i| piece(nurbs.u_knots(), at[i].u)))
            && within_two([0, 1, 2].map(|i| piece(nurbs.v_knots(), at[i].v))),
    )
}

#[cfg(test)]
mod tests {
    mod wide_references;

    use super::{crosses_one_knot_line, point_at};
    use crate::test_support::surfaces::ridged;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    fn line(origin: Point2, direction: Point2) -> PcurveGeometry {
        PcurveGeometry::Line(LinePcurve::try_new(origin, direction).expect("a line pcurve"))
    }

    /// A B-surface support is evaluated directly, and at the point
    /// `surface_point` gives: `u` across the first index, `v` the second.
    #[test]
    fn a_b_surface_support_evaluates_where_surface_point_does() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let rows = [
            [[0.0, 0.0, 0.0], [0.0, 30.0, 5.0]],
            [[10.0, 0.0, 2.0], [10.0, 30.0, 9.0]],
            [[20.0, 0.0, 1.0], [20.0, 30.0, -4.0]],
        ]
        .map(|row| row.map(|[x, y, z]| Point3::new(x, y, z)).to_vec())
        .to_vec();
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new(rows, None),
                false,
            )
            .unwrap(),
        ));
        let pcurve = line(Point2::new(0.1, 0.2), Point2::new(0.7, 0.5));
        let on_surface = |u: f64, v: f64| {
            crate::brep::evaluation::surface_point(&ctx, &surface, u, v)
                .unwrap()
                .unwrap()
                .get()
        };
        for t in [0.0, 0.37, 1.0] {
            let uv = crate::brep::evaluation::pcurve_uv(&ctx, &pcurve, t)
                .unwrap()
                .unwrap()
                .get();
            let expected = on_surface(uv.u, uv.v);
            let point = point_at(&ctx, &surface, &pcurve, t).unwrap().unwrap();
            assert!(
                point.distance(expected) < 1e-12,
                "{t}: {point:?} {expected:?}"
            );
        }
        // Swapped parameters give another point.
        let swapped = on_surface(0.7, 0.45);
        let middle = point_at(&ctx, &surface, &pcurve, 0.5).unwrap().unwrap();
        assert!(middle.distance(swapped) > 1.0);
    }

    /// An offset support is its carrier's point at the pcurve's parameters
    /// plus the signed distance along the carrier's natural unit normal: the
    /// plane `z = 0` with `u` along `x` has `du x dv` along `+z`.
    #[test]
    fn an_offset_support_moves_its_carrier_along_the_natural_normal() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let base = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("a plane"),
        ));
        let pcurve = line(Point2::new(3.0, 4.0), Point2::new(1.0, 0.0));
        for distance in [2.0, -2.0] {
            let offset = super::OffsetSupport {
                base: base.clone(),
                distance,
            };
            let point = super::Support::Offset(&offset)
                .point(&ctx, &pcurve, 1.0)
                .unwrap()
                .unwrap();
            assert!(
                point.distance(Point3::new(4.0, 4.0, distance)) < 1e-12,
                "{distance}: {point:?}"
            );
        }
    }

    /// A span is resolved when it crosses at most one knot line of a
    /// B-surface support in each direction; any span is on another support.
    #[test]
    fn a_span_over_several_b_surface_pieces_is_not_resolved() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let surface = ridged();
        let crosses = |surface: &SurfaceGeometry, pcurve: &PcurveGeometry, low: f64, high: f64| {
            crosses_one_knot_line(&ctx, surface, pcurve, low, high).unwrap()
        };
        let along_u = line(Point2::new(0.0, 0.5), Point2::new(1.0, 0.0));
        assert!(crosses(&surface, &along_u, 0.301, 0.319));
        assert!(crosses(&surface, &along_u, 0.301, 0.339));
        assert!(!crosses(&surface, &along_u, 0.301, 0.341));
        assert!(!crosses(&surface, &along_u, 0.0, 1.0));
        let along_v = line(Point2::new(0.31, 0.0), Point2::new(0.0, 1.0));
        assert!(crosses(&surface, &along_v, 0.0, 1.0));
        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("a plane"),
        ));
        assert!(crosses(&plane, &along_u, 0.0, 1.0));
    }

    /// The fit that resolves knot lines is within the bound; the fit judged by
    /// samples alone misses the ridge by more than 1e-3 mm.
    #[test]
    fn a_fit_resolves_a_ridge_narrower_than_its_samples() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let surface = ridged();
        let pcurve = line(Point2::new(0.0, 0.5), Point2::new(1.0, 0.0));
        let on_support = |t: f64| point_at(&ctx, &surface, &pcurve, t);
        let worst = |nurbs: NurbsCurve| {
            let fitted = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
            (0..=20_011)
                .map(|i| f64::from(i) / 20_011.0)
                .map(|t| {
                    crate::brep::evaluation::curve_point(&ctx, &fitted, t)
                        .unwrap()
                        .unwrap()
                        .get()
                        .distance(on_support(t).unwrap().unwrap())
                })
                .fold(0.0, f64::max)
        };
        let resolved = crate::brep::fit::fit_curve(on_support, [0.0, 1.0], &[], |low, high| {
            crosses_one_knot_line(&ctx, &surface, &pcurve, low, high)
        })
        .unwrap()
        .unwrap();
        let departure = worst(resolved);
        assert!(
            departure <= crate::brep::fit::FIT_TOLERANCE_MM,
            "{departure}"
        );
        let sampled = crate::brep::fit::fit_curve(
            on_support,
            [0.0, 1.0],
            &[],
            |_, _| -> Result<bool, CodecError> { Ok(true) },
        )
        .unwrap()
        .unwrap();
        assert!(worst(sampled) > 1.0e-3);
    }
}
