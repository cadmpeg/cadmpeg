// SPDX-License-Identifier: Apache-2.0
//! Face regions, vertices, and face sides of one exact stream (§8.18 to
//! §8.20).
//!
//! The stream stores no face side. [`resolve`] chooses, per face, whether
//! the neutral loops run with or against the stored order, so that the two
//! uses of every shared edge, and of every pair of coincident unshared
//! edges, run opposite ways. It then turns each closed component outward by
//! a vote of its extreme points along the six axis directions. The winding
//! of a face's outer loop in its pcurve domain relates the loop direction to
//! the domain normal.

use std::collections::BTreeSet;

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::CodecError;

use super::eval::{cross, distance, dot, norm, scale, sub};
use super::model::{CurveGeometry, ExactModel, Vec2, Vec3};
use super::resolve::{
    closure_gap, coedge_geometry, domain_periods, domain_point, edges_along, encloses,
    face_regions, face_slot, loop_polygon, pcurve_directions, signed_area, CoedgeGeometry,
    LoopPolygon, PcurveSlot, RegionLoops, ENDPOINT_TOLERANCE,
};

/// Largest distance between two coedge ends that a loop joins into one
/// vertex, in metres.
const VERTEX_JOIN_TOLERANCE: f64 = 1.0e-3;
/// Parameter step of the finite-difference surface normal, relative to the
/// parameter magnitude.
const NORMAL_STEP: f64 = 1.0e-6;
/// Interior samples per region axis used to find an extreme point.
const INTERIOR_SAMPLES: usize = 6;
/// Samples that test whether a singular-use curve collapses.
const COLLAPSE_SAMPLES: usize = 8;
/// A component takes the outward test when at most one in this many of its
/// edge classes is open: it then encloses a volume up to small gaps.
const OPEN_CLASS_DIVISOR: usize = 4;
/// Smallest `|n·x̂|` of a unit normal that decides the side of a component.
const SIDE_DECISION: f64 = 0.1;
/// Floor of the derived-search work of one stream: containment tests in
/// face region nesting, charged per polygon sample, and coincident-edge
/// candidate tests.
pub(crate) const MAX_TOPOLOGY_WORK: usize = 1 << 24;
/// Derived-search work allowed per face, loop, coedge, and edge record.
const TOPOLOGY_WORK_PER_RECORD: usize = 1 << 10;
/// Natural-interval fractions of the interior samples that a coincident
/// edge must pass through.
const PARTNER_FRACTIONS: [f64; 3] = [0.25, 0.5, 0.75];
/// Uniform samples of the nearest-point search on a partner curve.
const PARTNER_CURVE_SAMPLES: usize = 16;
/// Golden-section steps that refine the nearest-point search.
const PARTNER_REFINEMENT_STEPS: usize = 24;
/// Work of one coincidence test of two curves: the curve evaluations of the
/// nearest-point searches for both sample sets.
const PARTNER_COINCIDENCE_WORK: usize =
    2 * PARTNER_FRACTIONS.len() * (PARTNER_CURVE_SAMPLES + 1 + 2 * PARTNER_REFINEMENT_STEPS);

/// One loop with its resolved coedges.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopTopology {
    /// Global loop index.
    pub(crate) index: usize,
    /// Resolved coedges in stored order.
    pub(crate) coedges: Vec<CoedgeGeometry>,
    /// Pcurve direction of each coedge along the stored order.
    pub(crate) directions: Vec<bool>,
    /// Edge-curve direction of each coedge along the stored order.
    pub(crate) along: Vec<bool>,
    /// Largest gap between consecutive coedge ends, in metres.
    pub(crate) gap: Option<f64>,
    /// Domain polygon of the loop.
    pub(crate) polygon: Option<LoopPolygon>,
}

/// One face with its loops and regions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceTopology {
    /// Pcurve domain of the face.
    pub(crate) slot: Option<PcurveSlot>,
    /// Periods of the pcurve domain.
    pub(crate) periods: [Option<f64>; 2],
    /// Loops in the face's record order.
    pub(crate) loops: Vec<LoopTopology>,
    /// Regions, as positions in `loops`.
    pub(crate) regions: Vec<RegionLoops>,
    /// `+1` when outer loops run counter-clockwise in the domain, else `-1`.
    pub(crate) winding: f64,
    /// Neutral loops run against the stored order.
    pub(crate) reversed: bool,
    /// The side came from an outward test of a closed component.
    pub(crate) side_derived: bool,
    /// [`MAX_TOPOLOGY_WORK`] ran out before the loops were nested; the
    /// face has one unclassified region.
    pub(crate) nesting_exhausted: bool,
}

/// Start and end slots of a coedge end in stored order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndSlots {
    /// Slot at the stored-order start.
    pub(crate) start: usize,
    /// Slot at the stored-order end.
    pub(crate) end: usize,
}

/// Resolved topology of one exact stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Topology {
    /// Faces in record order.
    pub(crate) faces: Vec<FaceTopology>,
    /// Vertex index of each end slot. Slots `2e` and `2e + 1` are the start
    /// and end of edge `e`; edgeless coedges own the slots after them.
    pub(crate) slot_vertex: Vec<usize>,
    /// Vertex positions, in metres.
    pub(crate) vertices: Vec<Vec3>,
    /// Largest distance from a vertex to a joined end, in metres.
    pub(crate) vertex_spread: Vec<f64>,
    /// First end slot of each edgeless coedge, keyed by loop and position.
    pub(crate) free_slots: std::collections::BTreeMap<(usize, usize), usize>,
    /// Edges of singular uses whose curve collapses to one point.
    pub(crate) singular_edges: BTreeSet<usize>,
    /// Loop junctions whose ends lie farther apart than the join tolerance.
    pub(crate) split_junctions: usize,
    /// Two-use edges whose uses cannot run opposite ways.
    pub(crate) side_conflicts: usize,
    /// Derived searches that [`MAX_TOPOLOGY_WORK`] stopped: face region
    /// nesting and coincident-edge pairing.
    pub(crate) work_exhausted: usize,
    /// Coincident partner of each single-use edge that one model edge stores
    /// once per use (§8.20): the edge with the smaller index that the class
    /// is written as, and whether both edges run the same way.
    pub(crate) partners: std::collections::BTreeMap<usize, (usize, bool)>,
}

impl Topology {
    /// The edge that edge record `edge` is written as, and whether that edge
    /// runs the same way as the record.
    pub(crate) fn written_edge(&self, edge: usize) -> (usize, bool) {
        self.partners.get(&edge).copied().unwrap_or((edge, true))
    }

    /// End slots of a coedge in stored order.
    pub(crate) fn end_slots(
        &self,
        model: &ExactModel,
        loop_index: usize,
        position: usize,
        along: bool,
    ) -> Option<EndSlots> {
        let coedge = model.loops.get(loop_index)?.coedges.get(position)?;
        let (first, second) = match coedge.edge {
            Some(edge) => (edge.checked_mul(2)?, edge.checked_mul(2)?.checked_add(1)?),
            None => {
                let base = *self.free_slots.get(&(loop_index, position))?;
                (base, base.checked_add(1)?)
            }
        };
        Some(if along {
            EndSlots {
                start: first,
                end: second,
            }
        } else {
            EndSlots {
                start: second,
                end: first,
            }
        })
    }
}

/// Resolves loops, regions, vertices, and face sides with the derived-search
/// work limit `work`.
pub(crate) fn resolve(model: &ExactModel, work: usize) -> Result<Topology, CodecError> {
    let budget = WorkBudget::new(work);
    let mut faces: Vec<FaceTopology> = (0..model.faces.len())
        .map(|face| face_topology(model, face, &budget))
        .collect();
    let regions_exhausted = faces.iter().filter(|face| face.nesting_exhausted).count();
    let mut free_slots = std::collections::BTreeMap::new();
    let slot_overflow = || CodecError::malformed("exact stream has too many coedge ends");
    let mut next_slot = model.edges.len().checked_mul(2).ok_or_else(slot_overflow)?;
    for (face, record) in faces.iter().zip(&model.faces) {
        for (topology, index) in face.loops.iter().zip(&record.loops) {
            for (position, coedge) in model.loops[*index].coedges.iter().enumerate() {
                if coedge.edge.is_none() {
                    free_slots.insert((topology.index, position), next_slot);
                    next_slot = next_slot.checked_add(2).ok_or_else(slot_overflow)?;
                }
            }
        }
    }
    let singular_edges: BTreeSet<usize> = model
        .faces
        .iter()
        .flat_map(|face| face.singular_uses.iter().map(|singular| singular.edge))
        .filter(|edge| {
            model
                .edges
                .get(*edge)
                .is_some_and(|record| collapsed(&record.geometry))
        })
        .collect();
    let mut topology = Topology {
        faces: Vec::new(),
        slot_vertex: Vec::new(),
        vertices: Vec::new(),
        vertex_spread: Vec::new(),
        free_slots,
        singular_edges,
        split_junctions: 0,
        side_conflicts: 0,
        work_exhausted: regions_exhausted,
        partners: std::collections::BTreeMap::new(),
    };
    let (side_conflicts, partners) = orient(model, &mut faces, &topology.singular_edges, &budget);
    topology.side_conflicts = side_conflicts;
    topology.work_exhausted += usize::from(budget.exhausted());
    topology.partners = partners;
    topology.faces = faces;
    join_vertices(model, &mut topology, next_slot);
    Ok(topology)
}

/// Work limit of the derived searches of `model`: the larger of
/// [`MAX_TOPOLOGY_WORK`] and [`TOPOLOGY_WORK_PER_RECORD`] per record.
pub(crate) fn work_limit(model: &ExactModel) -> usize {
    let coedges: usize = model.loops.iter().map(|record| record.coedges.len()).sum();
    let records = model.faces.len() + model.loops.len() + coedges + model.edges.len();
    records
        .checked_mul(TOPOLOGY_WORK_PER_RECORD)
        .map_or(MAX_TOPOLOGY_WORK, |work| work.max(MAX_TOPOLOGY_WORK))
}

/// Whether a curve stays within the endpoint tolerance of its start.
fn collapsed(curve: &CurveGeometry<3>) -> bool {
    let Some([low, high]) = super::eval::curve_interval(curve) else {
        return false;
    };
    let Some(start) = super::eval::curve_point(curve, low) else {
        return false;
    };
    (1..=COLLAPSE_SAMPLES).all(|step| {
        let fraction = cadmpeg_core::convert::f64_from_index(step).unwrap_or(0.0)
            / cadmpeg_core::convert::f64_from_index(COLLAPSE_SAMPLES).unwrap_or(1.0);
        super::eval::curve_point(curve, low + (high - low) * fraction)
            .is_some_and(|point| distance(point, start) <= ENDPOINT_TOLERANCE)
    })
}

fn face_topology(model: &ExactModel, face: usize, budget: &WorkBudget) -> FaceTopology {
    let record = &model.faces[face];
    let slot = face_slot(model, face);
    let periods = slot.map_or([None, None], |slot| {
        domain_periods(&record.surface.geometry, slot)
    });
    let loops: Vec<LoopTopology> = record
        .loops
        .iter()
        .map(|index| {
            let coedges = coedge_geometry(model, *index, slot);
            let orientations: Vec<u8> = model.loops[*index]
                .coedges
                .iter()
                .map(|coedge| coedge.orientation)
                .collect();
            let directions = pcurve_directions(&orientations, &coedges);
            let along = edges_along(&orientations, &coedges, &directions);
            let gap = closure_gap(&coedges, &directions);
            let polygon = loop_polygon(model, &coedges, &directions, periods);
            LoopTopology {
                index: *index,
                coedges,
                directions,
                along,
                gap,
                polygon,
            }
        })
        .collect();
    let inner: Vec<bool> = record
        .loops
        .iter()
        .map(|index| model.loops[*index].inner)
        .collect();
    let polygons: Vec<Option<LoopPolygon>> = loops
        .iter()
        .map(|topology| topology.polygon.clone())
        .collect();
    let nested = face_regions(&inner, &polygons, periods, budget);
    let nesting_exhausted = nested.is_none();
    let regions = nested.unwrap_or_else(|| {
        vec![RegionLoops {
            loops: (0..inner.len()).collect(),
            classified: false,
        }]
    });
    let winding = regions
        .iter()
        .find(|region| region.classified)
        .and_then(|region| loops.get(*region.loops.first()?)?.polygon.as_ref())
        .map_or(1.0, |polygon| {
            if signed_area(&polygon.points) < 0.0 {
                -1.0
            } else {
                1.0
            }
        });
    FaceTopology {
        slot,
        periods,
        loops,
        regions,
        winding,
        reversed: false,
        side_derived: false,
        nesting_exhausted,
    }
}

/// Coedge uses of each edge: face and edge direction along the stored order.
fn edge_uses(model: &ExactModel, faces: &[FaceTopology]) -> Vec<Vec<(usize, bool)>> {
    let mut uses = vec![Vec::new(); model.edges.len()];
    for (face, topology) in faces.iter().enumerate() {
        for loop_topology in &topology.loops {
            let record = &model.loops[loop_topology.index];
            for (coedge, along) in record.coedges.iter().zip(&loop_topology.along) {
                if let Some(list) = coedge.edge.and_then(|edge| uses.get_mut(edge)) {
                    list.push((face, *along));
                }
            }
        }
    }
    uses
}

/// Uses of each edge class: an edge with one use joins the class of its
/// coincident partner, with its direction restated against the partner.
fn class_uses(
    uses: &[Vec<(usize, bool)>],
    partners: &std::collections::BTreeMap<usize, (usize, bool)>,
) -> Vec<Vec<(usize, bool)>> {
    let mut classes = uses.to_vec();
    for (edge, (class, same)) in partners {
        let moved = std::mem::take(&mut classes[*edge]);
        classes[*class].extend(
            moved
                .into_iter()
                .map(|(face, along)| (face, along == *same)),
        );
    }
    classes
}

/// Components of `count` nodes under parity constraints `(a, b, differ)`:
/// `flip[a] != flip[b]` exactly when `differ`. Seeds keep their flip; a
/// constraint that contradicts an earlier one is counted and skipped.
fn propagate(
    count: usize,
    constraints: &[(usize, usize, bool)],
    flip: &mut [bool],
) -> (Vec<usize>, Vec<Vec<usize>>, usize) {
    let mut neighbours: Vec<Vec<(usize, bool)>> = vec![Vec::new(); flip.len()];
    for (a, b, differ) in constraints {
        neighbours[*a].push((*b, *differ));
        neighbours[*b].push((*a, *differ));
    }
    let mut component_of: Vec<Option<usize>> = vec![None; flip.len()];
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut conflicts = 0;
    for seed in 0..count {
        if component_of[seed].is_some() {
            continue;
        }
        let id = components.len();
        component_of[seed] = Some(id);
        let mut members = vec![seed];
        let mut stack = vec![seed];
        while let Some(node) = stack.pop() {
            for (other, differ) in &neighbours[node] {
                let wanted = flip[node] != *differ;
                if component_of[*other].is_some() {
                    if flip[*other] != wanted {
                        conflicts += 1;
                    }
                    continue;
                }
                component_of[*other] = Some(id);
                flip[*other] = wanted;
                members.push(*other);
                stack.push(*other);
            }
        }
        components.push(members);
    }
    let component_of = component_of.into_iter().map(|id| id.unwrap_or(0)).collect();
    (component_of, components, conflicts / 2)
}

/// Sample points of a single-use edge for the coincident-edge search: the
/// ends, the interior samples at [`PARTNER_FRACTIONS`], and the natural
/// interval.
struct EdgeShape {
    ends: [Vec3; 2],
    interior: Vec<Vec3>,
    interval: Vec2,
}

impl EdgeShape {
    fn of(curve: &CurveGeometry<3>) -> Option<Self> {
        let [low, high] = super::eval::curve_interval(curve)?;
        let at = |fraction: f64| super::eval::curve_point(curve, low + (high - low) * fraction);
        let interior = PARTNER_FRACTIONS
            .iter()
            .map(|fraction| at(*fraction))
            .collect::<Option<Vec<Vec3>>>()?;
        let ends = [at(0.0)?, at(1.0)?];
        let finite = ends
            .iter()
            .chain(&interior)
            .all(|point| point.iter().all(|value| value.is_finite()));
        finite.then_some(Self {
            ends,
            interior,
            interval: [low, high],
        })
    }
}

/// Cell of a point in the grid of [`ENDPOINT_TOLERANCE`] cells.
fn end_cell(point: Vec3) -> Option<[i64; 3]> {
    let cell = |value: f64| {
        cadmpeg_core::convert::truncate_f64_to_i64((value / ENDPOINT_TOLERANCE).floor())
    };
    Some([cell(point[0])?, cell(point[1])?, cell(point[2])?])
}

/// Cells within one step of `cell` on every axis: every point within
/// [`ENDPOINT_TOLERANCE`] of a point of `cell` lies in one of them.
fn neighbour_cells(cell: [i64; 3]) -> impl Iterator<Item = [i64; 3]> {
    let steps = [-1_i64, 0, 1];
    steps.into_iter().flat_map(move |x| {
        steps.into_iter().flat_map(move |y| {
            steps.into_iter().filter_map(move |z| {
                Some([
                    cell[0].checked_add(x)?,
                    cell[1].checked_add(y)?,
                    cell[2].checked_add(z)?,
                ])
            })
        })
    })
}

/// Distance from `point` to `curve` over `interval`: the nearest of
/// [`PARTNER_CURVE_SAMPLES`] samples, refined by a golden-section search in
/// the sample intervals next to it.
fn distance_to_curve(curve: &CurveGeometry<3>, interval: Vec2, point: Vec3) -> Option<f64> {
    let [low, high] = interval;
    let samples = cadmpeg_core::convert::f64_from_index(PARTNER_CURVE_SAMPLES)?;
    let parameter = |step: usize| {
        Some(low + (high - low) * cadmpeg_core::convert::f64_from_index(step)? / samples)
    };
    let gap = |t: f64| Some(distance(super::eval::curve_point(curve, t)?, point));
    let mut nearest = (0, f64::INFINITY);
    for step in 0..=PARTNER_CURVE_SAMPLES {
        let value = gap(parameter(step)?)?;
        if value < nearest.1 {
            nearest = (step, value);
        }
    }
    let (step, mut best) = nearest;
    let mut a = parameter(step.max(1) - 1)?;
    let mut b = parameter((step + 1).min(PARTNER_CURVE_SAMPLES))?;
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..PARTNER_REFINEMENT_STEPS {
        let c = b - (b - a) * ratio;
        let d = a + (b - a) * ratio;
        let (gap_c, gap_d) = (gap(c)?, gap(d)?);
        best = best.min(gap_c).min(gap_d);
        if gap_c < gap_d {
            b = d;
        } else {
            a = c;
        }
    }
    Some(best)
}

/// Largest distance from an interior sample of either edge to the other
/// curve, or `None` when it exceeds the endpoint tolerance.
fn coincidence_gap(
    model: &ExactModel,
    (edge, shape): (usize, &EdgeShape),
    (other, other_shape): (usize, &EdgeShape),
) -> Option<f64> {
    let gaps = |points: &[Vec3], curve: usize, interval: Vec2| {
        points.iter().try_fold(0.0_f64, |worst, point| {
            let gap = distance_to_curve(&model.edges[curve].geometry, interval, *point)?;
            (gap <= ENDPOINT_TOLERANCE).then_some(worst.max(gap))
        })
    };
    Some(
        gaps(&shape.interior, other, other_shape.interval)?.max(gaps(
            &other_shape.interior,
            edge,
            shape.interval,
        )?),
    )
}

/// Pairs each edge that has one coedge use, is not a point, and is not a
/// collapsed singular edge with a coincident edge of the same kind (§8.20): both ends meet within the endpoint tolerance and
/// the interior samples of each lie on the other. Pairs are taken in order
/// of their largest sample distance, so an edge pairs with the nearest of
/// several coincident edges. The value is the partner with the smaller index
/// and whether both edges run the same way. The candidate tests are charged
/// to `budget`; the search stops when it runs out.
fn coincident_partners(
    model: &ExactModel,
    uses: &[Vec<(usize, bool)>],
    singular: &BTreeSet<usize>,
    budget: &WorkBudget,
) -> std::collections::BTreeMap<usize, (usize, bool)> {
    let mut shapes: std::collections::BTreeMap<usize, EdgeShape> =
        std::collections::BTreeMap::new();
    let mut cells: std::collections::BTreeMap<[i64; 3], Vec<usize>> =
        std::collections::BTreeMap::new();
    for (edge, list) in uses.iter().enumerate() {
        let geometry = &model.edges[edge].geometry;
        if list.len() != 1
            || singular.contains(&edge)
            || matches!(geometry, CurveGeometry::Point(_))
        {
            continue;
        }
        let Some(shape) = EdgeShape::of(geometry) else {
            continue;
        };
        let Some(cell) = end_cell(shape.ends[0]) else {
            continue;
        };
        cells.entry(cell).or_default().push(edge);
        shapes.insert(edge, shape);
    }
    let mut candidates: Vec<(f64, usize, usize, bool)> = Vec::new();
    'edges: for (edge, shape) in &shapes {
        let [start, end] = shape.ends;
        let mut near: BTreeSet<usize> = BTreeSet::new();
        for point in [start, end] {
            let Some(cell) = end_cell(point) else {
                continue;
            };
            for neighbour in neighbour_cells(cell) {
                near.extend(cells.get(&neighbour).into_iter().flatten().copied());
            }
        }
        for other in near.range(edge + 1..) {
            if !budget.charge() {
                break 'edges;
            }
            let other_shape = &shapes[other];
            let [other_start, other_end] = other_shape.ends;
            let forward = distance(start, other_start).max(distance(end, other_end));
            let backward = distance(start, other_end).max(distance(end, other_start));
            if forward.min(backward) > ENDPOINT_TOLERANCE {
                continue;
            }
            let same = if forward.max(backward) <= ENDPOINT_TOLERANCE {
                let [early, .., late] = other_shape.interior.as_slice() else {
                    continue;
                };
                let probe = shape.interior[0];
                distance(probe, *early) <= distance(probe, *late)
            } else {
                forward <= backward
            };
            let matched = shape
                .interior
                .iter()
                .zip(if same {
                    other_shape.interior.clone()
                } else {
                    other_shape.interior.iter().rev().copied().collect()
                })
                .map(|(a, b)| distance(*a, b))
                .fold(0.0_f64, f64::max);
            let gap = if matched <= ENDPOINT_TOLERANCE {
                matched
            } else {
                if !budget.charge_by(PARTNER_COINCIDENCE_WORK) {
                    break 'edges;
                }
                let Some(gap) = coincidence_gap(model, (*edge, shape), (*other, other_shape))
                else {
                    continue;
                };
                gap
            };
            let ends = if same { forward } else { backward };
            candidates.push((gap.max(ends), *edge, *other, same));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    let mut paired = BTreeSet::new();
    let mut partners = std::collections::BTreeMap::new();
    for (_, edge, other, same) in candidates {
        if paired.contains(&edge) || paired.contains(&other) {
            continue;
        }
        paired.insert(edge);
        paired.insert(other);
        partners.insert(other, (edge, same));
    }
    partners
}

/// Chooses `reversed` for every face so that the uses of each two-use edge
/// run opposite ways, then turns closed components outward. Returns the
/// number of two-use edges whose constraint cannot hold and the coincident
/// partners of single-use edges.
fn orient(
    model: &ExactModel,
    faces: &mut [FaceTopology],
    singular: &BTreeSet<usize>,
    budget: &WorkBudget,
) -> (usize, std::collections::BTreeMap<usize, (usize, bool)>) {
    let uses = edge_uses(model, faces);
    let mut seam_conflicts = 0;
    let mut shared = Vec::new();
    for list in &uses {
        let [(a, along_a), (b, along_b)] = list.as_slice() else {
            continue;
        };
        if a == b {
            if along_a == along_b {
                seam_conflicts += 1;
            }
            continue;
        }
        shared.push((*a, *b, along_a == along_b));
    }
    let mut reversed: Vec<bool> = faces.iter().map(|face| face.reversed).collect();
    let (component_of, components, shared_conflicts) =
        propagate(faces.len(), &shared, &mut reversed);
    let partners = coincident_partners(model, &uses, singular, budget);
    let mut linked = Vec::new();
    for (edge, (class, same)) in &partners {
        let ([(a, along_a)], [(b, along_b)]) = (uses[*class].as_slice(), uses[*edge].as_slice())
        else {
            continue;
        };
        let differ = *along_a == (*along_b == *same);
        let (left, right) = (component_of[*a], component_of[*b]);
        if left != right {
            linked.push((left, right, differ != (reversed[*a] != reversed[*b])));
        }
    }
    let mut component_flip = vec![false; components.len()];
    let (group_of, groups, _) = propagate(components.len(), &linked, &mut component_flip);
    for (face, flag) in reversed.iter_mut().enumerate() {
        *flag = *flag != component_flip[component_of[face]];
    }
    for (face, flag) in faces.iter_mut().zip(&reversed) {
        face.reversed = *flag;
    }
    let classes = class_uses(&uses, &partners);
    let mut closed_classes: Vec<usize> = groups.iter().map(|_| 0).collect();
    let mut open_classes = closed_classes.clone();
    for (edge, list) in classes.iter().enumerate() {
        if singular.contains(&edge) {
            continue;
        }
        let owners: BTreeSet<usize> = list
            .iter()
            .map(|(face, _)| group_of[component_of[*face]])
            .collect();
        for owner in &owners {
            if list.len() == 2 && owners.len() == 1 {
                closed_classes[*owner] += 1;
            } else {
                open_classes[*owner] += 1;
            }
        }
    }
    for (group, members) in groups.iter().enumerate() {
        let encloses = closed_classes[group] > 0
            && open_classes[group]
                .checked_mul(OPEN_CLASS_DIVISOR)
                .is_some_and(|open| open <= closed_classes[group] + open_classes[group]);
        if !encloses {
            continue;
        }
        let group_faces: Vec<usize> = members
            .iter()
            .flat_map(|component| components[*component].iter().copied())
            .collect();
        if let Some(outward) = outward(model, faces, &group_faces) {
            for face in &group_faces {
                if !outward {
                    faces[*face].reversed = !faces[*face].reversed;
                }
                faces[*face].side_derived = true;
            }
        }
    }
    (shared_conflicts + seam_conflicts, partners)
}

/// Unit normal of the domain surface of `face` at `uv`, from central
/// differences.
pub(crate) fn domain_normal(
    model: &ExactModel,
    face: usize,
    slot: PcurveSlot,
    uv: Vec2,
) -> Option<Vec3> {
    let record = model.faces.get(face)?;
    let step = |value: f64| NORMAL_STEP * value.abs().max(1.0);
    let [u, v] = uv;
    let du = sub(
        domain_point(record, slot, [u + step(u), v])?,
        domain_point(record, slot, [u - step(u), v])?,
    );
    let dv = sub(
        domain_point(record, slot, [u, v + step(v)])?,
        domain_point(record, slot, [u, v - step(v)])?,
    );
    let normal = cross(du, dv);
    let length = norm(normal);
    (length > 0.0 && length.is_finite()).then(|| scale(normal, length.recip()))
}

/// Domain samples of a face: loop polygon points and an interior grid of
/// each classified region.
fn face_samples(face: &FaceTopology) -> Vec<Vec2> {
    let mut samples = Vec::new();
    for topology in &face.loops {
        if let Some(polygon) = &topology.polygon {
            samples.extend(polygon.points.iter().copied());
        }
    }
    for region in face.regions.iter().filter(|region| region.classified) {
        let Some(outer) = region
            .loops
            .first()
            .and_then(|position| face.loops.get(*position)?.polygon.as_ref())
        else {
            continue;
        };
        let Some(first) = outer.points.first() else {
            continue;
        };
        let [low, high] = outer
            .points
            .iter()
            .fold([*first, *first], |[low, high], point| {
                [
                    [low[0].min(point[0]), low[1].min(point[1])],
                    [high[0].max(point[0]), high[1].max(point[1])],
                ]
            });
        for i in 1..INTERIOR_SAMPLES {
            for j in 1..INTERIOR_SAMPLES {
                let fraction = |k: usize| {
                    cadmpeg_core::convert::f64_from_index(k).unwrap_or(0.0)
                        / cadmpeg_core::convert::f64_from_index(INTERIOR_SAMPLES).unwrap_or(1.0)
                };
                let point = [
                    low[0] + (high[0] - low[0]) * fraction(i),
                    low[1] + (high[1] - low[1]) * fraction(j),
                ];
                let inside = region.loops.iter().enumerate().all(|(rank, position)| {
                    let Some(polygon) = face
                        .loops
                        .get(*position)
                        .and_then(|loop_| loop_.polygon.as_ref())
                    else {
                        return true;
                    };
                    encloses(&polygon.points, point, face.periods) == (rank == 0)
                });
                if inside {
                    samples.push(point);
                }
            }
        }
    }
    samples
}

/// Directions whose extreme points vote on the side of a closed component.
const SIDE_DIRECTIONS: [Vec3; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Whether the current sides of `component` face outward: the majority of
/// the decided votes of [`side_votes`].
fn outward(model: &ExactModel, faces: &[FaceTopology], component: &[usize]) -> Option<bool> {
    let votes = side_votes(model, faces, component);
    let outward = votes.iter().filter(|vote| **vote == Some(true)).count();
    let inward = votes.iter().filter(|vote| **vote == Some(false)).count();
    match outward.cmp(&inward) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    }
}

/// One vote per direction `d` of [`SIDE_DIRECTIONS`]: at the sample that
/// lies farthest along `d`, an outward face normal `n` has `n·d ≥ 0`. A
/// vote is absent when no sample there has `|n·d|` of at least
/// [`SIDE_DECISION`].
fn side_votes(
    model: &ExactModel,
    faces: &[FaceTopology],
    component: &[usize],
) -> [Option<bool>; 6] {
    let mut samples: Vec<(usize, PcurveSlot, Vec2, Vec3)> = Vec::new();
    for face in component {
        let topology = &faces[*face];
        let Some(slot) = topology.slot else {
            continue;
        };
        let record = &model.faces[*face];
        for uv in face_samples(topology) {
            if let Some(point) = domain_point(record, slot, uv) {
                samples.push((*face, slot, uv, point));
            }
        }
    }
    SIDE_DIRECTIONS.map(|direction| {
        let top = samples
            .iter()
            .map(|(_, _, _, point)| dot(*point, direction))
            .fold(f64::NEG_INFINITY, f64::max);
        let mut decision: Option<(f64, bool)> = None;
        for (face, slot, uv, point) in &samples {
            if top - dot(*point, direction) > ENDPOINT_TOLERANCE {
                continue;
            }
            let Some(normal) = domain_normal(model, *face, *slot, *uv) else {
                continue;
            };
            let topology = &faces[*face];
            let side = if topology.reversed { -1.0 } else { 1.0 } * topology.winding;
            let along = dot(scale(normal, side), direction);
            if decision.is_none_or(|(magnitude, _)| along.abs() > magnitude) {
                decision = Some((along.abs(), along > 0.0));
            }
        }
        decision
            .filter(|(magnitude, _)| *magnitude >= SIDE_DECISION)
            .map(|(_, outward)| outward)
    })
}

/// Joins coedge ends into vertices (§8.20).
fn join_vertices(model: &ExactModel, topology: &mut Topology, slots: usize) {
    let mut parent: Vec<usize> = (0..slots).collect();
    let mut points: Vec<Option<Vec3>> = vec![None; parent.len()];
    for (edge, record) in model.edges.iter().enumerate() {
        if let Some([start, end]) = super::eval::curve_ends(&record.geometry) {
            points[2 * edge] = Some(start);
            points[2 * edge + 1] = Some(end);
        }
    }
    let loops: std::collections::BTreeMap<usize, &LoopTopology> = topology
        .faces
        .iter()
        .flat_map(|face| face.loops.iter())
        .map(|loop_| (loop_.index, loop_))
        .collect();
    for ((loop_index, position), base) in &topology.free_slots {
        let ends = loops
            .get(loop_index)
            .and_then(|loop_| loop_.coedges.get(*position)?.ends);
        if let Some([start, end]) = ends {
            points[*base] = Some(start);
            points[*base + 1] = Some(end);
        }
    }
    for edge in &topology.singular_edges {
        union(&mut parent, 2 * edge, 2 * edge + 1);
    }
    for (edge, (class, same)) in &topology.partners {
        let (start, end) = if *same {
            (2 * class, 2 * class + 1)
        } else {
            (2 * class + 1, 2 * class)
        };
        union(&mut parent, 2 * edge, start);
        union(&mut parent, 2 * edge + 1, end);
    }
    let mut split = 0;
    for face in &topology.faces {
        for loop_ in &face.loops {
            let count = loop_.along.len();
            for position in 0..count {
                let next = (position + 1) % count;
                let (Some(here), Some(there)) = (
                    topology.end_slots(model, loop_.index, position, loop_.along[position]),
                    topology.end_slots(model, loop_.index, next, loop_.along[next]),
                ) else {
                    continue;
                };
                let joined = match (points[here.end], points[there.start]) {
                    (Some(a), Some(b)) => distance(a, b) <= VERTEX_JOIN_TOLERANCE,
                    _ => true,
                };
                if joined {
                    union(&mut parent, here.end, there.start);
                } else {
                    split += 1;
                }
            }
        }
    }
    let mut vertex_of_root: Vec<Option<usize>> = vec![None; parent.len()];
    let mut slot_vertex = vec![0; parent.len()];
    let mut members: Vec<Vec<Vec3>> = Vec::new();
    for slot in 0..parent.len() {
        let root = find(&mut parent, slot);
        let vertex = *vertex_of_root[root].get_or_insert_with(|| {
            members.push(Vec::new());
            members.len() - 1
        });
        slot_vertex[slot] = vertex;
        if let (Some(point), Some(list)) = (points[slot], members.get_mut(vertex)) {
            list.push(point);
        }
    }
    let mut positions = Vec::new();
    let mut spread = Vec::new();
    for list in &members {
        let count = cadmpeg_core::convert::f64_from_index(list.len())
            .unwrap_or(1.0)
            .max(1.0);
        let mean = scale(
            list.iter()
                .fold([0.0; 3], |sum, point| super::eval::add(sum, *point)),
            count.recip(),
        );
        spread.push(
            list.iter()
                .map(|point| distance(*point, mean))
                .fold(0.0, f64::max),
        );
        positions.push(mean);
    }
    topology.slot_vertex = slot_vertex;
    topology.vertices = positions;
    topology.vertex_spread = spread;
    topology.split_junctions = split;
}

fn find(parent: &mut [usize], slot: usize) -> usize {
    let mut root = slot;
    while parent[root] != root {
        root = parent[root];
    }
    let mut current = slot;
    while parent[current] != root {
        let next = parent[current];
        parent[current] = root;
        current = next;
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}

#[cfg(test)]
mod tests;
