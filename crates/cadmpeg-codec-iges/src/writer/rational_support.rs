// SPDX-License-Identifier: Apache-2.0
//! Analytic face supports restated as exact rational B-spline surfaces, which
//! a trimmed surface (Type 144 over Type 128) can bound.

use super::covered_uses::{over, referenced_pcurves, visit_pole_use_lists, visit_pole_uses};
use super::degrade::{naming, Unwritten};
use super::exact::nurbs_form;
use super::same_range;
use super::{edge_span, flatten_curve, solved_curve, TopologyIndex};
use crate::entities::curve_conversion::{elliptical_arc_nurbs, ANGULAR_TOLERANCE};
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::topology::{Face, LoopBoundary, LoopRing, PcurveUse, Sense};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// The largest degree `|p| + |q|` of the exact image of a parameter line of
/// slope `p / q` on a sphere or torus that the writer states.
const RATIONAL_SLOPE_DEGREE: i32 = 6;

/// The number of times the writer halves a piece of a rational image whose
/// Bernstein weights are not all positive.
const RATIONAL_PIECE_SPLITS: u32 = 8;

/// The document with each cylinder, cone, sphere or torus face support
/// restated as the rational B-spline surface that equals it over a domain
/// that holds its faces, and the parameter curves of those faces restated
/// on it. A face whose parameter curves are not all exact on the restated
/// support keeps its model-space boundary: its parameter curves are removed
/// and each is charged `writer.pcurve-omitted`. Only the supports that
/// `admit` names are restated. Fails naming a face that no restated domain
/// holds. The seam cuts name where a loop of a face passes the end of the
/// turn that holds the face.
pub(super) fn rational_analytic_supports<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    admit: impl Fn(&str) -> bool,
) -> Result<(Cow<'ir, CadIr>, Vec<LossNote>, SeamCuts), Unwritten> {
    let forms = ir
        .geometry
        .surfaces
        .iter()
        .filter(|surface| admit(surface.id.as_str()))
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Solved(geometry) => {
                Some((surface.id.as_str(), Revolved::of(geometry)?))
            }
            SurfaceGeometry::Procedural { .. } => None,
        })
        .collect::<Vec<_>>();
    if forms.is_empty() {
        return Ok((Cow::Borrowed(ir), Vec::new(), SeamCuts::default()));
    }
    let ends = pcurve_ends(ctx, ir)?;
    let index = TopologyIndex::new(ir);
    let mut surfaces = Vec::new();
    let mut pcurves = BTreeMap::new();
    let mut model_space = Vec::new();
    let mut copies = Vec::new();
    let mut splits = Vec::new();
    let mut jumps = Vec::new();
    for (surface, form) in forms {
        let Some(restated) = restated_support(ctx, &ends, &index, surface, &form) else {
            continue;
        };
        let restated = restated?;
        if restated
            .pcurves
            .keys()
            .any(|pcurve| pcurves.contains_key(pcurve))
        {
            continue;
        }
        surfaces.push((surface, restated.surface));
        pcurves.extend(restated.pcurves);
        model_space.extend(restated.model_space);
        copies.extend(restated.copies);
        splits.extend(restated.splits);
        jumps.extend(restated.jumps);
    }
    if surfaces.is_empty() {
        return Ok((Cow::Borrowed(ir), Vec::new(), SeamCuts::default()));
    }
    let mut restated = ir.clone();
    for (id, nurbs) in surfaces {
        if let Some(surface) = restated
            .geometry
            .surfaces
            .iter_mut()
            .find(|surface| surface.id.as_str() == id)
        {
            surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
        }
    }
    for copy in &copies {
        face_copy(&mut restated, copy);
    }
    for split in &splits {
        face_split(&mut restated, split);
    }
    let referenced = referenced_pcurves(&restated.geometry);
    restated.geometry.pcurves.retain(|pcurve| {
        let id = pcurve.id.as_str();
        referenced.contains(id) || !splits.iter().any(|split| split.source == id)
    });
    for pcurve in &mut restated.geometry.pcurves {
        if let Some(nurbs) = pcurves.remove(pcurve.id.as_str()) {
            pcurve.geometry = PcurveGeometry::Nurbs { nurbs };
        }
    }
    let losses = model_space_boundaries(&mut restated, &model_space);
    let mut cuts = SeamCuts::default();
    let pairs = splits
        .iter()
        .filter_map(|split| match split.pieces.as_slice() {
            [(before, _), (after, _)] => Some([
                (before.as_str().to_owned(), CurveEnd::End),
                (after.as_str().to_owned(), CurveEnd::Start),
            ]),
            _ => None,
        })
        .chain(jumps);
    for mut pair in pairs {
        pair.sort();
        cuts.0.insert(pair);
    }
    Ok((Cow::Owned(restated), losses, cuts))
}

/// An end of a parameter curve in the direction of its parameter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CurveEnd {
    Start,
    End,
}

/// The ends of parameter curves where a loop of a face on a restated support
/// passes the end of the turn that holds the face: the end and the start of
/// the two pieces of a curve cut there, and the ends of two curves that meet
/// there. Each pair is in sorted order.
#[derive(Default)]
pub(super) struct SeamCuts(BTreeSet<[(String, CurveEnd); 2]>);

impl SeamCuts {
    /// Whether a loop passes the end of its turn where the end `first` of
    /// one parameter curve meets the end `second` of the next.
    pub(super) fn cut_between(&self, first: (&str, CurveEnd), second: (&str, CurveEnd)) -> bool {
        let mut pair = [first, second].map(|(id, end)| (id.to_owned(), end));
        pair.sort();
        self.0.contains(&pair)
    }
}

/// Removes the parameter curve uses of each face in `faces`, and each
/// parameter curve record that no use then references, with one
/// `writer.pcurve-omitted` note per removed record.
fn model_space_boundaries(ir: &mut CadIr, faces: &[ModelSpaceFace]) -> Vec<LossNote> {
    let owners = faces
        .iter()
        .flat_map(|face| face.loops.iter().map(move |loop_| (loop_.as_str(), face)))
        .collect::<BTreeMap<_, _>>();
    let mut removed = BTreeMap::<String, &ModelSpaceFace>::new();
    let mut remove = |uses: &mut Vec<PcurveUse>, face| {
        for pcurve_use in uses.drain(..) {
            removed
                .entry(pcurve_use.pcurve.as_str().to_owned())
                .or_insert(face);
        }
    };
    for coedge in &mut ir.geometry.coedges {
        if let Some(&face) = owners.get(coedge.owner_loop.as_str()) {
            remove(&mut coedge.pcurves, face);
        }
    }
    for loop_ in &mut ir.geometry.loops {
        let Some(&face) = owners.get(loop_.id.as_str()) else {
            continue;
        };
        match &mut loop_.boundary {
            LoopBoundary::Vertex { pcurves, .. } => remove(pcurves, face),
            LoopBoundary::Ring(ring) => {
                let mut vertex_uses = ring.vertex_uses().to_vec();
                for vertex_use in &mut vertex_uses {
                    remove(&mut vertex_use.pcurves, face);
                }
                if let Ok(replaced) = LoopRing::new(ring.coedges().to_vec(), vertex_uses) {
                    *ring = replaced;
                }
            }
        }
    }
    let geometry = &ir.geometry;
    let referenced = geometry
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .chain(geometry.loops.iter().flat_map(|loop_| {
            match &loop_.boundary {
                LoopBoundary::Vertex { pcurves, .. } => pcurves.iter().collect::<Vec<_>>(),
                LoopBoundary::Ring(ring) => ring
                    .vertex_uses()
                    .iter()
                    .flat_map(|vertex_use| vertex_use.pcurves.iter())
                    .collect(),
            }
        }))
        .map(|pcurve_use| pcurve_use.pcurve.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let notes = removed
        .iter()
        .filter(|(pcurve, _)| !referenced.contains(pcurve.as_str()))
        .map(|(pcurve, face)| {
            IgesLossCode::WriterPcurveOmitted.note(format!(
                "parameter curve {pcurve} of face {} was not written: the face is written on \
                 the rational Type 128 of support {} with its model-space boundary, because {}",
                face.face, face.surface, face.reason
            ))
        })
        .collect();
    ir.geometry.pcurves.retain(|pcurve| {
        referenced.contains(pcurve.id.as_str()) || !removed.contains_key(pcurve.id.as_str())
    });
    notes
}

/// The finite start and end points of each bounded parameter curve.
fn pcurve_ends<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
) -> Result<BTreeMap<&'ir str, [Point2; 2]>, CodecError> {
    let mut ends = BTreeMap::new();
    for pcurve in &ir.geometry.pcurves {
        let Some(range) = pcurve.parameter_range() else {
            continue;
        };
        let [start, end] = range.get();
        let at = |parameter| {
            finite_or_refusal(pcurve_uv(
                EvaluationAdmission::Write(ctx),
                &pcurve.geometry,
                parameter,
            ))
        };
        if let (Some(start), Some(end)) = (at(start)?, at(end)?) {
            ends.insert(pcurve.id.as_str(), [start.get(), end.get()]);
        }
    }
    Ok(ends)
}

/// An analytic surface as `O + ρ(v)(cos u X + k sin u Y) + h(v) Z`.
pub(super) struct Revolved {
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    pub(super) z_axis: Vector3,
    profile: Profile,
}

enum Profile {
    /// `ρ(v) = radius + slope v`, `h(v) = v`, with `k = ratio`.
    Ruled { radius: f64, slope: f64, ratio: f64 },
    /// `ρ(v) = offset + radius cos v`, `h(v) = radius sin v`, with `k = 1`.
    Circular { offset: f64, radius: f64 },
}

impl Revolved {
    pub(super) fn of(geometry: &SolvedSurfaceGeometry) -> Option<Self> {
        let (origin, frame, profile) = match geometry {
            SolvedSurfaceGeometry::Cylinder(cylinder) => (
                cylinder.origin(),
                cylinder.frame(),
                Profile::Ruled {
                    radius: cylinder.radius().get(),
                    slope: 0.0,
                    ratio: 1.0,
                },
            ),
            SolvedSurfaceGeometry::Cone(cone) => (
                cone.origin(),
                cone.frame(),
                Profile::Ruled {
                    radius: cone.radius().get(),
                    slope: cone.half_angle().get().portable_tan(),
                    ratio: cone.ratio().get(),
                },
            ),
            SolvedSurfaceGeometry::Sphere(sphere) => (
                sphere.center(),
                sphere.frame(),
                Profile::Circular {
                    offset: 0.0,
                    radius: sphere.radius().get(),
                },
            ),
            SolvedSurfaceGeometry::Torus(torus) => (
                torus.center(),
                torus.frame(),
                Profile::Circular {
                    offset: torus.major_radius().get(),
                    radius: torus.minor_radius().get(),
                },
            ),
            _ => return None,
        };
        let z_axis = *frame.axis().as_raw();
        let x_axis = *frame.reference().as_raw();
        Some(Self {
            origin: origin.get(),
            x_axis,
            y_axis: z_axis.cross(x_axis),
            z_axis,
            profile,
        })
    }

    /// The parameters of `point` on the surface, with the azimuth in
    /// `(-π, π]`. Absent on the axis.
    pub(super) fn parameters(&self, point: Point3) -> Option<Point2> {
        let offset = Vector3::new(
            point.x - self.origin.x,
            point.y - self.origin.y,
            point.z - self.origin.z,
        );
        let (x, y, h) = (
            offset.dot(self.x_axis),
            offset.dot(self.y_axis),
            offset.dot(self.z_axis),
        );
        match self.profile {
            Profile::Ruled {
                radius,
                slope,
                ratio,
            } => {
                let rho = slope.mul_add(h, radius);
                (rho != 0.0).then(|| Point2::new((y / (ratio * rho)).atan2(x / rho), h))
            }
            Profile::Circular { offset, radius } => {
                let rho = x.hypot(y);
                (rho != 0.0)
                    .then(|| Point2::new(y.atan2(x), (h / radius).atan2((rho - offset) / radius)))
            }
        }
    }

    /// Whether the second parameter is an angle, which a torus states.
    pub(super) fn v_periodic(&self) -> bool {
        matches!(self.profile, Profile::Circular { offset, .. } if offset != 0.0)
    }

    /// The sign of `S_u × S_v` along the outward normal where `ρ > 0`.
    pub(super) fn orientation(&self) -> f64 {
        match self.profile {
            Profile::Ruled { ratio, .. } => ratio.signum(),
            Profile::Circular { .. } => 1.0,
        }
    }

    /// The face side of each loop in `loops` that winds once around the
    /// support, as the whole periods each loop moves by so that the upper
    /// loop on a periodic parameter lies above the lower one, and the
    /// parameter values the domain must reach past the loops: a pole or apex
    /// that bounds the face, or one period past a lone loop on a periodic
    /// parameter. None where the loops do not bound one face on the support.
    pub(super) fn face_sides(
        &self,
        sense: Sense,
        loops: &[LoopSpan],
    ) -> Option<(Vec<Point2>, [Vec<f64>; 2])> {
        let orientation = self.orientation() * if sense == Sense::Reversed { -1.0 } else { 1.0 };
        let mut sides: [(Vec<usize>, Vec<usize>); 2] = Default::default();
        for (position, span) in loops.iter().enumerate() {
            let (axis, side) = match span.winding {
                [0, 0] => continue,
                [turn, 0] => (1, f64::from(turn) * orientation),
                [0, turn] => (0, -f64::from(turn) * orientation),
                _ => return None,
            };
            let (lowers, uppers) = &mut sides[axis];
            if side > 0.0 {
                lowers.push(position);
            } else {
                uppers.push(position);
            }
        }
        let mut shifts = vec![Point2::new(0.0, 0.0); loops.len()];
        let mut reach: [Vec<f64>; 2] = Default::default();
        for (axis, (lowers, uppers)) in sides.into_iter().enumerate() {
            let periodic = axis == 0 || self.v_periodic();
            let poles = if periodic { Vec::new() } else { self.poles() };
            match (lowers.as_slice(), uppers.as_slice()) {
                ([], []) => {}
                ([lower], [upper]) => {
                    let [_, low] = loops[*lower].extent[axis];
                    let [high, _] = loops[*upper].extent[axis];
                    if periodic {
                        let gap = high - low;
                        let moved = gap - TAU * ((gap / TAU).ceil() - 1.0);
                        let shift = moved - gap;
                        let shifted = &mut shifts[*upper];
                        if axis == 0 {
                            shifted.u += shift;
                        } else {
                            shifted.v += shift;
                        }
                    } else if high <= low {
                        return None;
                    }
                }
                ([lower], []) => {
                    let [_, low] = loops[*lower].extent[axis];
                    reach[axis].push(if periodic {
                        low + TAU
                    } else {
                        poles
                            .into_iter()
                            .filter(|pole| *pole > low)
                            .min_by(f64::total_cmp)?
                    });
                }
                ([], [upper]) => {
                    let [high, _] = loops[*upper].extent[axis];
                    reach[axis].push(if periodic {
                        high - TAU
                    } else {
                        poles
                            .into_iter()
                            .filter(|pole| *pole < high)
                            .max_by(f64::total_cmp)?
                    });
                }
                _ => return None,
            }
        }
        Some((shifts, reach))
    }

    /// The second parameters of the poles of a sphere and the apex of a cone.
    pub(super) fn poles(&self) -> Vec<f64> {
        match self.profile {
            Profile::Ruled { radius, slope, .. } if slope != 0.0 => vec![-radius / slope],
            Profile::Circular { offset: 0.0, .. } => vec![-FRAC_PI_2, FRAC_PI_2],
            Profile::Ruled { .. } | Profile::Circular { .. } => Vec::new(),
        }
    }
}

/// One closed loop of a face in the parameter plane of a revolved support:
/// its winding about each parameter in whole turns, and its least and
/// greatest value of each parameter.
pub(super) struct LoopSpan {
    pub(super) winding: [i32; 2],
    pub(super) extent: [[f64; 2]; 2],
}

struct Restated {
    surface: NurbsSurface,
    pcurves: BTreeMap<String, PcurveNurbs>,
    model_space: Vec<ModelSpaceFace>,
    copies: Vec<FaceCopy>,
    splits: Vec<FaceSplit>,
    /// The ends of the parameter curves that meet at the end of the turn
    /// that holds their face.
    jumps: Vec<[(String, CurveEnd); 2]>,
}

/// A copy of a parameter curve for the uses on the loops of one face, which
/// place the curve at another period than the face that first uses it.
struct FaceCopy {
    source: String,
    id: PcurveId,
    loops: BTreeSet<String>,
}

/// Adds the copy of its source curve that `copy` names, and moves the uses of
/// the source on its loops to the copy.
fn face_copy(ir: &mut CadIr, copy: &FaceCopy) {
    let Some(mut pcurve) = ir
        .geometry
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == copy.source)
        .cloned()
    else {
        return;
    };
    pcurve.id = copy.id.clone();
    ir.geometry.pcurves.push(pcurve);
    let moved = |pcurve_use: &mut PcurveUse| {
        if pcurve_use.pcurve.as_str() == copy.source {
            pcurve_use.pcurve = copy.id.clone();
        }
    };
    for coedge in ir
        .geometry
        .coedges
        .iter_mut()
        .filter(|coedge| copy.loops.contains(coedge.owner_loop.as_str()))
    {
        coedge.pcurves.iter_mut().for_each(moved);
    }
    for loop_ in ir
        .geometry
        .loops
        .iter_mut()
        .filter(|loop_| copy.loops.contains(loop_.id.as_str()))
    {
        visit_pole_uses(loop_, moved);
    }
}

/// The pieces of a parameter curve that one face uses across the end of the
/// turn that holds the face, in the order of the curve's parameter, each with
/// its parameter range. A curve that meets another curve of its loop at that
/// end, across the turn, is one piece.
struct FaceSplit {
    source: String,
    loops: BTreeSet<String>,
    pieces: Vec<(PcurveId, [f64; 2])>,
}

/// Adds the pieces that `split` names, and replaces each use of the source
/// curve on its loops with uses of the pieces in the direction of the use.
fn face_split(ir: &mut CadIr, split: &FaceSplit) {
    let cut = ir
        .geometry
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == split.source)
        .and_then(|source| {
            let range = source.parameter_range()?.get();
            let pieces = split
                .pieces
                .iter()
                .map(|(id, range)| over(source, id.clone(), FiniteVector::new(*range)?))
                .collect::<Option<Vec<_>>>()?;
            Some((range[1] > range[0], pieces))
        });
    debug_assert!(
        cut.is_some(),
        "the source {} of a face split is a stored parameter curve with a general \
         range, as its exact image requires, and its pieces have finite ranges",
        split.source
    );
    let Some((ascending, pieces)) = cut else {
        return;
    };
    ir.geometry.pcurves.extend(pieces);
    let replace = |uses: &mut Vec<PcurveUse>| {
        *uses = std::mem::take(uses)
            .into_iter()
            .flat_map(|pcurve_use| {
                if pcurve_use.pcurve.as_str() != split.source {
                    return vec![pcurve_use];
                }
                let backward = pcurve_use.parameter_range.is_some_and(|range| {
                    let [start, end] = range.endpoints();
                    (end > start) != ascending
                });
                let mut pieces = split
                    .pieces
                    .iter()
                    .map(|(id, [start, end])| PcurveUse {
                        pcurve: id.clone(),
                        parameter_range: pcurve_use.parameter_range.and_then(|_| {
                            DirectedParameterRange::new(if backward {
                                [*end, *start]
                            } else {
                                [*start, *end]
                            })
                            .ok()
                        }),
                        ..pcurve_use.clone()
                    })
                    .collect::<Vec<_>>();
                if backward {
                    pieces.reverse();
                }
                pieces
            })
            .collect();
    };
    for coedge in ir
        .geometry
        .coedges
        .iter_mut()
        .filter(|coedge| split.loops.contains(coedge.owner_loop.as_str()))
    {
        replace(&mut coedge.pcurves);
    }
    for loop_ in ir
        .geometry
        .loops
        .iter_mut()
        .filter(|loop_| split.loops.contains(loop_.id.as_str()))
    {
        visit_pole_use_lists(loop_, replace);
    }
}

/// A face written with its model-space boundary only.
struct ModelSpaceFace {
    face: String,
    surface: String,
    loops: Vec<String>,
    reason: String,
}

/// One exact parameter curve: its parameter range, its end points, for a
/// line that is not isoparametric its slope `p / q` in whole numbers, whether
/// its use runs against the curve, whether it starts a turn after the curve
/// before it in its loop, and, for a piece of a curve cut at the end of a
/// turn or a curve that meets another one there, the place of the piece along
/// the curve.
struct Exact {
    id: String,
    range: [f64; 2],
    start: Point2,
    end: Point2,
    slope: Option<Slope>,
    backward: bool,
    turned: bool,
    piece: Option<usize>,
}

impl Exact {
    fn shift(&mut self, shift: Point2) {
        for point in [&mut self.start, &mut self.end] {
            *point = Point2::new(point.u + shift.u, point.v + shift.v);
        }
    }

    /// The end points in the direction of the use.
    fn traversed(&self) -> [Point2; 2] {
        if self.backward {
            [self.end, self.start]
        } else {
            [self.start, self.end]
        }
    }

    /// The curve ends in the direction of the use.
    fn traversed_ends(&self) -> [CurveEnd; 2] {
        if self.backward {
            [CurveEnd::End, CurveEnd::Start]
        } else {
            [CurveEnd::Start, CurveEnd::End]
        }
    }
}

/// The slope `p / q` of a parameter line in lowest whole numbers, `q > 0`.
#[derive(Clone, Copy)]
struct Slope {
    p: i32,
    q: i32,
}

impl Slope {
    /// The degree `|p| + q` of the exact image of the line.
    fn degree(self) -> Option<usize> {
        usize::try_from(self.p.unsigned_abs() + self.q.unsigned_abs()).ok()
    }
}

/// The exact parameter curves of one loop, and the loop's winding about
/// each parameter in whole turns. The first `chain` curves are those of the
/// coedges, or of the vertex, in the order of the loop.
struct LoopImages {
    images: Vec<Exact>,
    chain: usize,
    winding: [i32; 2],
}

impl LoopImages {
    fn shift(&mut self, shift: Point2) {
        for image in &mut self.images {
            image.shift(shift);
        }
    }

    /// The least and greatest value of the parameter `axis` on the loop,
    /// absent for a loop of no curve.
    fn extent(&self, axis: usize) -> Option<[f64; 2]> {
        let [low, high] = self.span().extent[axis];
        (low.is_finite() && high.is_finite()).then_some([low, high])
    }

    fn span(&self) -> LoopSpan {
        let extent = |coordinate: fn(&Point2) -> f64| {
            self.images
                .iter()
                .flat_map(|image| [&image.start, &image.end])
                .map(coordinate)
                .fold([f64::INFINITY, f64::NEG_INFINITY], |[low, high], value| {
                    [low.min(value), high.max(value)]
                })
        };
        LoopSpan {
            winding: self.winding,
            extent: [extent(|point| point.u), extent(|point| point.v)],
        }
    }
}

/// The parameter curves of each loop of one face on a revolved support, or
/// the reason the face keeps its model-space boundary. Each curve of the
/// chain of a loop moves by whole turns of the `periodic` parameters to start
/// where the curve before it ends.
fn face_images(
    ends: &BTreeMap<&str, [Point2; 2]>,
    index: &TopologyIndex<'_>,
    face: &Face,
    circular: bool,
    periodic: [bool; 2],
) -> Option<Result<Vec<LoopImages>, String>> {
    let mut loops = Vec::new();
    for loop_id in face.loops.stated() {
        let loop_ = index.loops.get(loop_id.as_str()).copied()?;
        let mut uses = Vec::<&PcurveUse>::new();
        let chain;
        match &loop_.boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                uses.extend(pcurves);
                chain = uses.len();
            }
            LoopBoundary::Ring(ring) => {
                for coedge_id in ring.coedges() {
                    let coedge = index.coedges.get(coedge_id.as_str()).copied()?;
                    if coedge.pcurves.is_empty() {
                        return Some(Err(format!("coedge {coedge_id} has no parameter curve")));
                    }
                    uses.extend(&coedge.pcurves);
                }
                chain = uses.len();
                for vertex_use in ring.vertex_uses() {
                    uses.extend(&vertex_use.pcurves);
                }
            }
        }
        let mut images = Vec::new();
        let mut travel = [0.0; 2];
        let mut exit: Option<Point2> = None;
        for (position, pcurve_use) in uses.into_iter().enumerate() {
            let mut image = match exact_image(ends, index, pcurve_use, circular)? {
                Ok(image) => image,
                Err(reason) => return Some(Err(reason)),
            };
            image.backward = pcurve_use.parameter_range.is_some_and(|range| {
                let [start, end] = range.endpoints();
                end < start
            });
            let sign = if image.backward { -1.0 } else { 1.0 };
            travel[0] += sign * (image.end.u - image.start.u);
            travel[1] += sign * (image.end.v - image.start.v);
            if position < chain {
                if let Some(exit) = exit {
                    let [entry, _] = image.traversed();
                    let turns = |periodic: bool, entry: f64, exit: f64| {
                        if periodic {
                            ((exit - entry) / TAU).round() * TAU
                        } else {
                            0.0
                        }
                    };
                    image.shift(Point2::new(
                        turns(periodic[0], entry.u, exit.u),
                        turns(periodic[1], entry.v, exit.v),
                    ));
                }
                exit = Some(image.traversed()[1]);
            }
            images.push(image);
        }
        let winding = travel.map(|travel| {
            let turns = (travel / TAU).round();
            if turns >= 1.0 {
                1
            } else if turns <= -1.0 {
                -1
            } else {
                0
            }
        });
        loops.push(LoopImages {
            images,
            chain,
            winding,
        });
    }
    Some(Ok(loops))
}

/// The exact image of one parameter curve use on the restated support, or
/// the reason it has none.
fn exact_image(
    ends: &BTreeMap<&str, [Point2; 2]>,
    index: &TopologyIndex<'_>,
    pcurve_use: &PcurveUse,
    circular: bool,
) -> Option<Result<Exact, String>> {
    let pcurve = index.pcurves.get(pcurve_use.pcurve.as_str()).copied()?;
    let id = pcurve.id.as_str();
    let Some(range) = pcurve
        .parameter_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
    else {
        return Some(Err(format!("parameter curve {id} states no range")));
    };
    if let Some(used) = pcurve_use.parameter_range {
        let used = used.endpoints();
        if !same_range(used, range) && !same_range(used, [range[1], range[0]]) {
            return Some(Err(format!(
                "a use of parameter curve {id} restricts its range"
            )));
        }
    }
    let Some(&[start, end]) = ends.get(id) else {
        return Some(Err(format!(
            "parameter curve {id} has no finite end points"
        )));
    };
    let points = match &pcurve.geometry {
        PcurveGeometry::Line(_) => vec![start, end],
        PcurveGeometry::Nurbs { nurbs } => nurbs
            .control_points()
            .into_iter()
            .map(cadmpeg_ir::units::FinitePoint2::get)
            .collect(),
        _ => {
            return Some(Err(format!(
                "parameter curve {id} is not a line or a B-spline"
            )))
        }
    };
    let exact = |slope| {
        Some(Ok(Exact {
            id: id.to_owned(),
            range,
            start,
            end,
            slope,
            backward: false,
            turned: false,
            piece: None,
        }))
    };
    let along = |coordinate: fn(&Point2) -> f64| {
        let first = coordinate(&points[0]);
        let constant = points
            .iter()
            .all(|point| (coordinate(point) - first).abs() <= ANGULAR_TOLERANCE);
        let steps = points
            .windows(2)
            .map(|pair| coordinate(&pair[1]) - coordinate(&pair[0]))
            .collect::<Vec<_>>();
        let monotone =
            steps.iter().all(|step| *step >= 0.0) || steps.iter().all(|step| *step <= 0.0);
        (constant, monotone)
    };
    let (constant_u, monotone_u) = along(|point| point.u);
    let (constant_v, monotone_v) = along(|point| point.v);
    if constant_u != constant_v && ((constant_u && monotone_v) || (constant_v && monotone_u)) {
        return exact(None);
    }
    let delta = Point2::new(end.u - start.u, end.v - start.v);
    let length = delta.u.hypot(delta.v);
    let straight = length > 0.0
        && points.iter().all(|point| {
            let offset = Point2::new(point.u - start.u, point.v - start.v);
            (offset.u * delta.v - offset.v * delta.u).abs() <= ANGULAR_TOLERANCE * length
        })
        && monotone_u
        && monotone_v;
    if circular && straight {
        if let Some(slope) = whole_slope(delta) {
            return exact(Some(slope));
        }
    }
    Some(Err(format!(
        "parameter curve {id} has no exact image on the rational parameters of its support"
    )))
}

/// The slope `p / q` with `|p| + q` at most [`RATIONAL_SLOPE_DEGREE`] of a
/// parameter line of extent `delta`.
fn whole_slope(delta: Point2) -> Option<Slope> {
    if delta.u == 0.0 || delta.v == 0.0 {
        return None;
    }
    (1..RATIONAL_SLOPE_DEGREE).find_map(|q| {
        (-RATIONAL_SLOPE_DEGREE..=RATIONAL_SLOPE_DEGREE)
            .filter(|p| *p != 0 && p.abs() + q <= RATIONAL_SLOPE_DEGREE)
            .find(|p| (delta.u - f64::from(*p) / f64::from(q) * delta.v).abs() <= ANGULAR_TOLERANCE)
            .map(|p| Slope { p, q })
    })
}

/// The rational form of the revolved support `surface` over a domain that
/// holds its faces, with the exact image of each parameter curve on it. A
/// face that states no loop on a sphere or torus covers the whole support.
/// The faces move by whole turns into the one window of a turn that holds
/// the most of them. An error names the first face that this window does not
/// hold.
fn restated_support(
    ctx: &DecodeContext<'_>,
    ends: &BTreeMap<&str, [Point2; 2]>,
    index: &TopologyIndex<'_>,
    surface: &str,
    form: &Revolved,
) -> Option<Result<Restated, Unwritten>> {
    let geometry = &index.ir.geometry;
    let faces = geometry
        .faces
        .iter()
        .filter(|face| face.surface.as_str() == surface)
        .collect::<Vec<_>>();
    if faces.is_empty() {
        return None;
    }
    let circular = matches!(form.profile, Profile::Circular { .. });
    let mut curves = Vec::new();
    let mut u_extent = Extent::default();
    let mut v_extent = Extent::default();
    let mut model_space = Vec::new();
    let mut heights = Extent::default();
    let mut placed = BTreeMap::<String, [Point2; 2]>::new();
    let mut copies = Vec::new();
    let mut splits = Vec::new();
    let mut jumps = Vec::new();
    let mut whole = false;
    let mut images_of_faces = Vec::new();
    let extent_of = |loops: &[LoopImages], reach: &[Vec<f64>; 2], axis: usize| {
        let mut extent = Extent::default();
        for value in loops
            .iter()
            .filter_map(|images| images.extent(axis))
            .flatten()
            .chain(reach[axis].iter().copied())
        {
            extent.include(value);
        }
        extent.bounds()
    };
    for face in faces {
        if face.loops.stated().next().is_none() {
            if !circular {
                return Some(Err(naming(face.id.as_str())(CodecError::NotImplemented(
                    format!(
                        "IGES restated support {surface}: a cylinder or cone face that states no loop has no finite domain"
                    ),
                ))));
            }
            whole = true;
            continue;
        }
        match face_images(ends, index, face, circular, [true, form.v_periodic()])? {
            Ok(mut loops) => {
                let spans = loops.iter().map(LoopImages::span).collect::<Vec<_>>();
                let (shifts, reach) = form.face_sides(face.sense, &spans).unwrap_or_else(|| {
                    (vec![Point2::new(0.0, 0.0); loops.len()], Default::default())
                });
                for (images, shift) in loops.iter_mut().zip(shifts) {
                    images.shift(shift);
                }
                for (axis, parameter) in [(0, "azimuth"), (1, "tube angle")] {
                    if axis == 1 && !form.v_periodic() {
                        continue;
                    }
                    windowed(&mut loops, axis, &reach[axis]);
                    if extent_of(&loops, &reach, axis)
                        .is_some_and(|[low, high]| high - low > TAU + ANGULAR_TOLERANCE)
                    {
                        return Some(Err(naming(face.id.as_str())(CodecError::NotImplemented(
                            format!(
                                "IGES restated support {surface}: no one turn of the {parameter} holds the loops of face {}",
                                face.id
                            ),
                        ))));
                    }
                }
                images_of_faces.push((face, loops, reach));
            }
            Err(reason) => {
                if !circular {
                    for height in edge_heights(ctx, index, face, form)? {
                        heights.include(height);
                    }
                }
                model_space.push(ModelSpaceFace {
                    face: face.id.as_str().to_owned(),
                    surface: surface.to_owned(),
                    loops: face
                        .loops
                        .stated()
                        .map(|loop_| loop_.as_str().to_owned())
                        .collect(),
                    reason,
                });
            }
        }
    }
    for (axis, parameter) in [(0, "azimuth"), (1, "tube angle")] {
        if axis == 1 && !form.v_periodic() {
            continue;
        }
        let extents = images_of_faces
            .iter()
            .map(|(_, loops, reach)| extent_of(loops, reach, axis))
            .collect::<Vec<_>>();
        let (window, placements) = shared_window(&extents);
        for ((face, loops, reach), turns) in images_of_faces.iter_mut().zip(placements) {
            let Some(turns) = turns else {
                return Some(Err(naming(face.id.as_str())(CodecError::NotImplemented(
                    format!(
                        "IGES restated support {surface}: the turn of the {parameter} from {} that holds the most faces of the support does not hold face {}",
                        window.unwrap_or(0.0),
                        face.id
                    ),
                ))));
            };
            for images in loops.iter_mut() {
                images.shift(along(axis, turns));
            }
            for value in &mut reach[axis] {
                *value += turns;
            }
        }
    }
    for (face, loops, reach) in images_of_faces {
        let face_key = face.id.as_str().replace('#', "/");
        let mut own = BTreeSet::new();
        let mut pieces = BTreeMap::<String, Vec<(usize, PcurveId, [f64; 2])>>::new();
        let mut renamed = BTreeMap::<String, String>::new();
        let mut chains = Vec::new();
        for loop_images in loops {
            let mut chain = Vec::new();
            for mut image in loop_images.images {
                let source = image.id.clone();
                if let Some(piece) = image.piece {
                    format!(
                        "iges:writer:pcurve#{}@{face_key}/{piece}",
                        source.replace('#', "/")
                    )
                    .clone_into(&mut image.id);
                }
                if chain.len() < loop_images.chain {
                    chain.push((image.id.clone(), image.traversed_ends(), image.turned));
                }
                if !own.insert(image.id.clone()) {
                    continue;
                }
                if let Some(piece) = image.piece {
                    pieces.entry(source).or_default().push((
                        piece,
                        PcurveId::try_from(image.id.as_str()).ok()?,
                        image.range,
                    ));
                }
                for point in [&image.start, &image.end] {
                    u_extent.include(point.u);
                    v_extent.include(point.v);
                }
                let ends = [image.start, image.end];
                match placed.get(&image.id) {
                    None => {
                        placed.insert(image.id.clone(), ends);
                        curves.push(image);
                    }
                    Some(first) if same_ends(*first, ends) => {}
                    Some(_) => {
                        let id = PcurveId::try_from(format!(
                            "iges:writer:pcurve#{}@{face_key}",
                            image.id.replace('#', "/"),
                        ))
                        .ok()?;
                        copies.push(FaceCopy {
                            source: image.id.clone(),
                            id: id.clone(),
                            loops: face
                                .loops
                                .stated()
                                .map(|loop_| loop_.as_str().to_owned())
                                .collect(),
                        });
                        renamed.insert(image.id.clone(), id.as_str().to_owned());
                        id.as_str().clone_into(&mut image.id);
                        curves.push(image);
                    }
                }
            }
            chains.push(chain);
        }
        for chain in chains {
            let mut before = None;
            for (id, [leading, trailing], turned) in chain {
                let id = renamed.get(&id).cloned().unwrap_or(id);
                if let Some(before) = before.filter(|_| turned) {
                    jumps.push([before, (id.clone(), leading)]);
                }
                before = Some((id, trailing));
            }
        }
        let face_loops = face
            .loops
            .stated()
            .map(|loop_| loop_.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        for (source, mut cut) in pieces {
            cut.sort_by_key(|(piece, ..)| *piece);
            splits.push(FaceSplit {
                source,
                loops: face_loops.clone(),
                pieces: cut.into_iter().map(|(_, id, range)| (id, range)).collect(),
            });
        }
        let [u_reach, v_reach] = reach;
        for u in u_reach {
            u_extent.include(u);
        }
        for v in v_reach {
            v_extent.include(v);
        }
    }
    let full = |extent: &Extent| {
        let low = extent.0.map_or(-PI, |[low, _]| low);
        [low, low + TAU]
    };
    let whole = whole || !model_space.is_empty();
    let [u_start, u_end] = if whole {
        full(&u_extent)
    } else {
        u_extent.bounds()?
    };
    let [v_start, v_end] = match form.profile {
        Profile::Circular { offset, .. } if whole => {
            if offset == 0.0 {
                [-FRAC_PI_2, FRAC_PI_2]
            } else {
                full(&v_extent)
            }
        }
        Profile::Circular { .. } => v_extent.bounds()?,
        Profile::Ruled { .. } => {
            if let Some([low, high]) = heights.0 {
                v_extent.include(low);
                v_extent.include(high);
                // The face reaches the apex where its edges lie on one side.
                for apex in form.poles() {
                    if low >= apex || high <= apex {
                        v_extent.include(apex);
                    }
                }
            }
            v_extent.bounds()?
        }
    };
    let u_arc = UnitArc::new([u_start, u_end])?;
    let (v_profile, v_arc) = match form.profile {
        Profile::Ruled {
            radius,
            slope,
            ratio,
        } => {
            let tolerance = ANGULAR_TOLERANCE * radius.max(1.0);
            let at = |v: f64| slope.mul_add(v, radius);
            if at(v_start) < -tolerance
                || at(v_end) < -tolerance
                || (at(v_start) <= 0.0 && at(v_end) <= 0.0)
            {
                return None;
            }
            (
                VProfile {
                    knots: vec![v_start, v_start, v_end, v_end],
                    points: vec![(at(v_start).max(0.0), v_start), (at(v_end).max(0.0), v_end)],
                    weights: vec![1.0, 1.0],
                    degree: 1,
                    ratio,
                },
                None,
            )
        }
        Profile::Circular { offset, radius } => {
            if radius <= 0.0 {
                return None;
            }
            let arc = UnitArc::new([v_start, v_end])?;
            (
                VProfile {
                    knots: arc.knots.clone(),
                    points: arc
                        .points
                        .iter()
                        .map(|point| (radius.mul_add(point.u, offset), radius * point.v))
                        .collect(),
                    weights: arc.weights.clone(),
                    degree: 2,
                    ratio: 1.0,
                },
                Some(arc),
            )
        }
    };
    let mut rows = Vec::new();
    let mut weights = Vec::new();
    for (direction, u_weight) in u_arc.points.iter().zip(&u_arc.weights) {
        let mut row = Vec::new();
        let mut row_weights = Vec::new();
        for ((rho, height), v_weight) in v_profile.points.iter().zip(&v_profile.weights) {
            let radial = form.x_axis.scale(rho * direction.u)
                + form.y_axis.scale(rho * v_profile.ratio * direction.v)
                + form.z_axis.scale(*height);
            row.push(Point3::new(
                form.origin.x + radial.x,
                form.origin.y + radial.y,
                form.origin.z + radial.z,
            ));
            row_weights.push(u_weight * v_weight);
        }
        rows.push(row);
        weights.push(row_weights);
    }
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(2, u_arc.knots.clone(), false),
        NurbsSurfaceAxis::new(v_profile.degree, v_profile.knots, false),
        NurbsPoleGrid::from_lanes(rows, Some(weights)).ok()?,
        false,
    )
    .ok()?;
    let map = |point: Point2| {
        Some(Point2::new(
            u_arc.parameter(point.u)?,
            v_arc
                .as_ref()
                .map_or(Some(point.v), |arc| arc.parameter(point.v))?,
        ))
    };
    let mut pcurves = BTreeMap::new();
    for curve in curves {
        let [start, end] = curve.range;
        let nurbs = match (curve.slope, &v_arc) {
            (Some(slope), Some(v_arc)) => {
                rational_line(&u_arc, v_arc, curve.start, curve.end, slope, curve.range)?
            }
            _ => PcurveNurbs::from_lanes(
                1,
                vec![start, start, end, end],
                vec![map(curve.start)?, map(curve.end)?],
                None,
                false,
            )
            .ok()?,
        };
        pcurves.insert(curve.id, nurbs);
    }
    Some(Ok(Restated {
        surface,
        pcurves,
        model_space,
        copies,
        splits,
        jumps,
    }))
}

/// The whole turns that move the value `low` to at least `start`.
fn window_turns(start: f64, low: f64) -> f64 {
    ((start - low - ANGULAR_TOLERANCE) / TAU).ceil() * TAU
}

/// The point `value` along the parameter `axis`.
fn along(axis: usize, value: f64) -> Point2 {
    if axis == 0 {
        Point2::new(value, 0.0)
    } else {
        Point2::new(0.0, value)
    }
}

/// The start of one window of a turn of a periodic parameter, and the whole
/// turns that move each face, of extent `extents` in that parameter, into
/// that window, or none for a face that the window does not hold. The window
/// starts at the least value of one face and holds the most faces. Of windows
/// that hold equally many faces, the window of the face that comes first in
/// the document applies. Without a face extent, no window applies and no
/// face moves.
fn shared_window(extents: &[Option<[f64; 2]>]) -> (Option<f64>, Vec<Option<f64>>) {
    let placed = |start: f64| {
        extents
            .iter()
            .map(|extent| match extent {
                None => Some(0.0),
                Some([low, high]) => {
                    let turns = window_turns(start, *low);
                    (high + turns <= start + TAU + ANGULAR_TOLERANCE).then_some(turns)
                }
            })
            .collect::<Vec<_>>()
    };
    let held = |turns: &[Option<f64>]| turns.iter().flatten().count();
    let mut window: Option<(f64, Vec<Option<f64>>)> = None;
    for [start, _] in extents.iter().flatten() {
        let candidate = placed(*start);
        if window
            .as_ref()
            .is_none_or(|(_, window)| held(&candidate) > held(window))
        {
            window = Some((*start, candidate));
        }
    }
    window.map_or_else(
        || (None, vec![Some(0.0); extents.len()]),
        |(start, turns)| (Some(start), turns),
    )
}

/// Moves each loop of a face that does not turn around the other parameter
/// by whole turns of the periodic parameter `axis` into one window of a
/// turn. The loops that turn around the other parameter bound the face in
/// `axis` and keep their places: the window starts at the least value of
/// these loops and of `reach`, the bounds their face sides set. Without such
/// loops, the window is the narrowest that holds the loops that turn around
/// neither parameter, and of these windows the narrowest that holds every
/// loop. The chain of a loop that turns around `axis` keeps each curve where
/// the curve before it ends, except where it passes an end of the window: a
/// curve that crosses that end is cut there, and its pieces lie at the two
/// ends of the window; a curve that starts at that end and leaves the window
/// moves by a turn to the other end.
fn windowed(loops: &mut [LoopImages], axis: usize, reach: &[f64]) {
    let other = 1 - axis;
    let extents = |turning: bool| {
        loops
            .iter()
            .filter(|images| images.winding[other] == 0 && (turning || images.winding[axis] == 0))
            .filter_map(|images| images.extent(axis))
            .collect::<Vec<_>>()
    };
    let (moved, whole) = (extents(true), extents(false));
    let width = |start: f64, extents: &[[f64; 2]]| {
        extents
            .iter()
            .map(|[low, high]| high + window_turns(start, *low) - start)
            .fold(0.0, f64::max)
    };
    let start = loops
        .iter()
        .filter(|images| images.winding[other] != 0)
        .filter_map(|images| images.extent(axis))
        .map(|[low, _]| low)
        .chain(reach.iter().copied())
        .min_by(f64::total_cmp)
        .or_else(|| {
            moved
                .iter()
                .map(|[start, _]| {
                    let misfit = width(*start, &whole) > TAU + ANGULAR_TOLERANCE;
                    (*start, misfit, width(*start, &moved))
                })
                .min_by(|first, second| first.1.cmp(&second.1).then(first.2.total_cmp(&second.2)))
                .map(|(start, ..)| start)
        });
    let Some(start) = start else {
        return;
    };
    for images in loops.iter_mut().filter(|images| images.winding[other] == 0) {
        if images.winding[axis] == 0 {
            if let Some([low, _]) = images.extent(axis) {
                images.shift(along(axis, window_turns(start, low)));
            }
        } else {
            let mut chain = std::mem::take(&mut images.images);
            let poles = chain.split_off(images.chain);
            let mut placed = Vec::<Exact>::new();
            let mut turns = None;
            for image in chain {
                let low = image_low(&image, axis);
                let current = *turns.get_or_insert_with(|| window_turns(start, low));
                let (pieces, next) = windowed_image(image, axis, start, current);
                turns = Some(next);
                if pieces.first().is_some_and(|piece| piece.turned) {
                    if let Some(before) = placed.last_mut() {
                        before.piece.get_or_insert(0);
                    }
                }
                placed.extend(pieces);
            }
            images.chain = placed.len();
            for image in poles {
                let turns = window_turns(start, image_low(&image, axis));
                placed.extend(windowed_image(image, axis, start, turns).0);
            }
            images.images = placed;
        }
    }
}

/// The least value of the parameter `axis` on `image`.
fn image_low(image: &Exact, axis: usize) -> f64 {
    if axis == 0 {
        image.start.u.min(image.end.u)
    } else {
        image.start.v.min(image.end.v)
    }
}

/// `image` moved by `turns` of the parameter `axis`, in the window of a turn
/// from `start`, and the turns of the curve after it in the chain. A curve
/// that crosses an end of the window is cut there into two pieces, and the
/// piece past that end moves by a turn to the other end. A curve that lies
/// past an end moves by a turn and starts a turn after the curve before it.
fn windowed_image(mut image: Exact, axis: usize, start: f64, turns: f64) -> (Vec<Exact>, f64) {
    let coordinate = |point: Point2| if axis == 0 { point.u } else { point.v };
    image.shift(along(axis, turns));
    let (from, to) = (coordinate(image.start), coordinate(image.end));
    let (low, high) = (from.min(to), from.max(to));
    let end = start + TAU;
    let (cut, back) = if high > end + ANGULAR_TOLERANCE {
        (end, -TAU)
    } else if low < start - ANGULAR_TOLERANCE {
        (start, TAU)
    } else {
        return (vec![image], turns);
    };
    if high - low > TAU + ANGULAR_TOLERANCE {
        return (vec![image], turns);
    }
    let past = if back < 0.0 {
        low >= cut - ANGULAR_TOLERANCE
    } else {
        high <= cut + ANGULAR_TOLERANCE
    };
    if past {
        image.shift(along(axis, back));
        image.turned = true;
        image.piece = Some(0);
        return (vec![image], turns + back);
    }
    let fraction = (cut - from) / (to - from);
    let [first, last] = image.range;
    let parameter = fraction.mul_add(last - first, first);
    let point = Point2::new(
        fraction.mul_add(image.end.u - image.start.u, image.start.u),
        fraction.mul_add(image.end.v - image.start.v, image.start.v),
    );
    let point = if axis == 0 {
        Point2::new(cut, point.v)
    } else {
        Point2::new(point.u, cut)
    };
    let mut head = Exact {
        id: image.id.clone(),
        range: [first, parameter],
        start: image.start,
        end: point,
        slope: image.slope,
        backward: image.backward,
        turned: false,
        piece: Some(0),
    };
    let mut tail = Exact {
        id: image.id,
        range: [parameter, last],
        start: point,
        end: image.end,
        slope: image.slope,
        backward: image.backward,
        turned: false,
        piece: Some(1),
    };
    let head_past = (from > to) == (back < 0.0);
    if head_past {
        head.shift(along(axis, back));
    } else {
        tail.shift(along(axis, back));
    }
    let next = if head_past == image.backward {
        turns + back
    } else {
        turns
    };
    (vec![head, tail], next)
}

/// Whether two parameter curve images start and end at the same points.
fn same_ends(first: [Point2; 2], second: [Point2; 2]) -> bool {
    first.iter().zip(&second).all(|(first, second)| {
        (first.u - second.u).abs() <= ANGULAR_TOLERANCE
            && (first.v - second.v).abs() <= ANGULAR_TOLERANCE
    })
}

/// The heights along the axis of the control points of the exact rational
/// forms of the edge curves of `face`, which bound the heights of the face.
fn edge_heights(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'_>,
    face: &Face,
    form: &Revolved,
) -> Option<Vec<f64>> {
    let mut heights = Vec::new();
    for loop_id in face.loops.stated() {
        let loop_ = index.loops.get(loop_id.as_str()).copied()?;
        for coedge_id in loop_.coedges() {
            let coedge = index.coedges.get(coedge_id.as_str()).copied()?;
            let edge = index.edges.get(coedge.edge.as_str()).copied()?;
            let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
            let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
            let span = edge_span(ctx, index, edge, &geometry).ok()?;
            let nurbs = nurbs_form(&geometry, span.range)?;
            heights.extend(nurbs.control_points().into_iter().map(|point| {
                let point = point.get();
                Vector3::new(
                    point.x - form.origin.x,
                    point.y - form.origin.y,
                    point.z - form.origin.z,
                )
                .dot(form.z_axis)
            }));
        }
    }
    Some(heights)
}

/// The exact image on the parameters of the restated sphere or torus of the
/// parameter line from `start` to `end` of slope `p / q`, over `range`.
///
/// On a span of each arc the restated parameter is an affine function of
/// `tan((θ - c) / 2)` for the span centre `c`. Along the line, `θ_u / 2 =
/// α_u + p x` and `θ_v / 2 = α_v + q x`, and `tan(α + n x)` is a rational
/// function of `w = tan x` of degree `|n|`: with `(1 + i w)^n = D + i N`, it is
/// `(N cos α + D sin α) / (D cos α - N sin α)`. Each piece between span
/// boundaries is therefore a rational Bézier curve of degree `|p| + q`. Its
/// knots are the parameters of the line at the piece ends, and its middle is
/// at the line parameter of the piece middle.
fn rational_line(
    u_arc: &UnitArc,
    v_arc: &UnitArc,
    start: Point2,
    end: Point2,
    slope: Slope,
    range: [f64; 2],
) -> Option<PcurveNurbs> {
    let delta = Point2::new(end.u - start.u, end.v - start.v);
    let mut breaks = vec![0.0, 1.0];
    for (boundaries, origin, extent) in [
        (&u_arc.boundaries, start.u, delta.u),
        (&v_arc.boundaries, start.v, delta.v),
    ] {
        breaks.extend(
            boundaries
                .iter()
                .map(|boundary| (boundary - origin) / extent)
                .filter(|s| *s > 0.0 && *s < 1.0),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|later, earlier| *later - *earlier <= ANGULAR_TOLERANCE);
    let line = Line {
        start,
        delta,
        k: delta.v / f64::from(slope.q),
        slope,
    };
    let mut pieces = Vec::new();
    for pair in breaks.windows(2) {
        line.pieces(
            u_arc,
            v_arc,
            [pair[0], pair[1]],
            RATIONAL_PIECE_SPLITS,
            &mut pieces,
        )?;
    }
    let degree = slope.degree()?;
    let last = pieces.len().checked_sub(1)?;
    let knot = |s: f64| (range[1] - range[0]).mul_add(s, range[0]);
    let mut knots = std::iter::repeat_n(range[0], degree + 1).collect::<Vec<_>>();
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for (position, ([_, s1], piece_points, piece_weights)) in pieces.into_iter().enumerate() {
        let scale = weights
            .last()
            .map_or(1.0, |previous: &f64| previous / piece_weights[0]);
        let skip = usize::from(position > 0);
        points.extend(piece_points.into_iter().skip(skip));
        weights.extend(
            piece_weights
                .into_iter()
                .skip(skip)
                .map(|weight| weight * scale),
        );
        if position == last {
            knots.extend(std::iter::repeat_n(range[1], degree + 1));
        } else {
            knots.extend(std::iter::repeat_n(knot(s1), degree));
        }
    }
    PcurveNurbs::from_lanes(
        u32::try_from(degree).ok()?,
        knots,
        points,
        Some(weights),
        false,
    )
    .ok()
}

/// One rational Bézier piece of the image of a line: its interval of the
/// line parameter, its poles and its weights.
type Piece = ([f64; 2], Vec<Point2>, Vec<f64>);

/// A parameter line `start + s delta` with `delta = k (p, q)`.
struct Line {
    start: Point2,
    delta: Point2,
    k: f64,
    slope: Slope,
}

impl Line {
    /// Appends the rational Bézier pieces of the image over `[s0, s1]`,
    /// halving a piece whose weights are not all positive at most `splits`
    /// times.
    fn pieces(
        &self,
        u_arc: &UnitArc,
        v_arc: &UnitArc,
        [s0, s1]: [f64; 2],
        splits: u32,
        pieces: &mut Vec<Piece>,
    ) -> Option<()> {
        let middle = 0.5 * (s0 + s1);
        if let Some((points, weights)) = self.piece(u_arc, v_arc, [s0, s1]) {
            pieces.push(([s0, s1], points, weights));
            return Some(());
        }
        let splits = splits.checked_sub(1)?;
        self.pieces(u_arc, v_arc, [s0, middle], splits, pieces)?;
        self.pieces(u_arc, v_arc, [middle, s1], splits, pieces)
    }

    /// The rational Bézier form of the image over `[s0, s1]`, absent when a
    /// weight is not positive.
    fn piece(
        &self,
        u_arc: &UnitArc,
        v_arc: &UnitArc,
        [s0, s1]: [f64; 2],
    ) -> Option<(Vec<Point2>, Vec<f64>)> {
        let middle = 0.5 * (s0 + s1);
        // `x = k (s - middle) / 2`, and `w = tan x` runs over `[-t, t]`.
        let half = (0.25 * self.k * (s1 - s0)).portable_tan();
        let tangent = [-half, 2.0 * half];
        let axis = |arc: &UnitArc, origin: f64, extent: f64, power: i32| {
            let angle = extent.mul_add(middle, origin);
            let span = arc.span(angle)?;
            let sweep = span[1] - span[0];
            let alpha = 0.5 * (angle - 0.5 * (span[0] + span[1]));
            let (sin, cos) = alpha.sin_cos();
            let (real, imaginary) = Polynomial::tangent_multiple(tangent, power);
            let numerator = real.scaled(sin).plus(&imaginary.scaled(cos));
            let denominator = real.scaled(cos).plus(&imaginary.scaled(-sin));
            Some((
                span[0] + 0.5 * sweep,
                sweep / (2.0 * (0.25 * sweep).portable_tan()),
                numerator,
                denominator,
            ))
        };
        let (u_base, u_gain, u_x, u_y) = axis(u_arc, self.start.u, self.delta.u, self.slope.p)?;
        let (v_base, v_gain, v_x, v_y) = axis(v_arc, self.start.v, self.delta.v, self.slope.q)?;
        let weight = u_y.times(&v_y);
        let u = weight.scaled(u_base).plus(&u_x.times(&v_y).scaled(u_gain));
        let v = weight.scaled(v_base).plus(&v_x.times(&u_y).scaled(v_gain));
        let degree = self.slope.degree()?;
        let [weight, u, v] = [weight, u, v].map(|polynomial| polynomial.bernstein(degree));
        let weight = weight?;
        if weight.iter().any(|weight| *weight <= 0.0) {
            return None;
        }
        let points = u?
            .into_iter()
            .zip(v?)
            .zip(&weight)
            .map(|((u, v), weight)| Point2::new(u / weight, v / weight))
            .collect();
        Some((points, weight))
    }
}

/// A real polynomial in `t` by its coefficients in increasing degree.
#[derive(Clone)]
struct Polynomial(Vec<f64>);

impl Polynomial {
    /// The real and imaginary parts `D`, `N` of `(1 + i w)^n` for `n ≥ 0`, or
    /// of `(1 - i w)^|n|` for `n < 0`, for the linear `w = w[0] + w[1] t`.
    fn tangent_multiple(w: [f64; 2], n: i32) -> (Self, Self) {
        let sign = if n < 0 { -1.0 } else { 1.0 };
        let step = Self(vec![sign * w[0], sign * w[1]]);
        let (mut real, mut imaginary) = (Self(vec![1.0]), Self(vec![0.0]));
        for _ in 0..n.unsigned_abs() {
            // (a + i b)(1 + i c) = (a - b c) + i (b + a c)
            let next_real = real.plus(&imaginary.times(&step).scaled(-1.0));
            imaginary = imaginary.plus(&real.times(&step));
            real = next_real;
        }
        (real, imaginary)
    }

    fn scaled(&self, factor: f64) -> Self {
        Self(self.0.iter().map(|value| value * factor).collect())
    }

    fn plus(&self, other: &Self) -> Self {
        let length = self.0.len().max(other.0.len());
        Self(
            (0..length)
                .map(|index| {
                    self.0.get(index).copied().unwrap_or(0.0)
                        + other.0.get(index).copied().unwrap_or(0.0)
                })
                .collect(),
        )
    }

    fn times(&self, other: &Self) -> Self {
        Self(
            (1..self.0.len() + other.0.len())
                .map(|terms| {
                    let power = terms - 1;
                    self.0
                        .iter()
                        .enumerate()
                        .filter_map(|(i, left)| Some(left * other.0.get(power.checked_sub(i)?)?))
                        .sum::<f64>()
                })
                .collect(),
        )
    }

    /// The Bernstein coefficients on `[0, 1]` of degree `degree`, absent when
    /// the polynomial has a higher degree.
    fn bernstein(&self, degree: usize) -> Option<Vec<f64>> {
        if self.0.len() > degree + 1 {
            return None;
        }
        let binomial = |n: usize, k: usize| {
            (0..k).fold(1.0, |value, index| {
                let numerator =
                    cadmpeg_core::convert::f64_from_index(n - index).unwrap_or(f64::NAN);
                let denominator =
                    cadmpeg_core::convert::f64_from_index(index + 1).unwrap_or(f64::NAN);
                value * numerator / denominator
            })
        };
        Some(
            (0..=degree)
                .map(|i| {
                    (0..=i)
                        .map(|j| {
                            self.0.get(j).copied().unwrap_or(0.0) * binomial(i, j)
                                / binomial(degree, j)
                        })
                        .sum::<f64>()
                })
                .collect(),
        )
    }
}

struct VProfile {
    knots: Vec<f64>,
    /// `(ρ, h)` of each pole.
    points: Vec<(f64, f64)>,
    weights: Vec<f64>,
    degree: u32,
    ratio: f64,
}

#[derive(Default)]
struct Extent(Option<[f64; 2]>);

impl Extent {
    fn include(&mut self, value: f64) {
        let [low, high] = self.0.get_or_insert([value, value]);
        *low = low.min(value);
        *high = high.max(value);
    }

    /// The extent, when it has positive width.
    fn bounds(&self) -> Option<[f64; 2]> {
        self.0.filter(|[low, high]| high > low)
    }
}

/// The rational quadratic unit circular arc over an angle interval, with
/// knots at its segment boundaries' angles.
struct UnitArc {
    knots: Vec<f64>,
    points: Vec<Point2>,
    weights: Vec<f64>,
    boundaries: Vec<f64>,
}

impl UnitArc {
    fn new(interval: [f64; 2]) -> Option<Self> {
        let arc = elliptical_arc_nurbs(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            PositiveLength::new(1.0)?,
            PositiveLength::new(1.0)?,
            interval,
        )
        .ok()??;
        let knots = arc
            .knots()
            .finite_knots()
            .map(FiniteReal::get)
            .collect::<Vec<_>>();
        let mut boundaries = knots.clone();
        boundaries.dedup();
        Some(Self {
            points: arc
                .control_points()
                .into_iter()
                .map(|point| {
                    let point = point.get();
                    Point2::new(point.x, point.y)
                })
                .collect(),
            weights: arc
                .weights()?
                .into_iter()
                .map(cadmpeg_ir::scalar::NonZeroReal::get)
                .collect(),
            knots,
            boundaries,
        })
    }

    /// The span boundaries around angle `angle`.
    fn span(&self, angle: f64) -> Option<[f64; 2]> {
        let first = *self.boundaries.first()?;
        let last = *self.boundaries.last()?;
        if angle < first - ANGULAR_TOLERANCE || angle > last + ANGULAR_TOLERANCE {
            return None;
        }
        let angle = angle.clamp(first, last);
        let segment = self.boundaries.windows(2).find(|pair| angle <= pair[1])?;
        Some([segment[0], segment[1]])
    }

    /// The arc parameter at the point of angle `angle`.
    fn parameter(&self, angle: f64) -> Option<f64> {
        let [start, end] = self.span(angle)?;
        let angle = angle.clamp(start, end);
        let sweep = end - start;
        let offset = angle - start - sweep * 0.5;
        let fraction = (1.0 + (offset * 0.5).portable_tan() / (sweep * 0.25).portable_tan()) * 0.5;
        Some(fraction.mul_add(sweep, start))
    }
}

#[cfg(test)]
mod tests;
