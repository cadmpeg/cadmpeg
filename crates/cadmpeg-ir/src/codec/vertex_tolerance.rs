// SPDX-License-Identifier: Apache-2.0
//! Vertex tolerances widened to the positions part 10 compares a vertex with
//! (§2.3).
//!
//! The pass reads the comparisons the validator makes: an edge's curve ends,
//! a coedge's use-curve ends, and a coedge's pcurve image on its face's
//! surface. A vertex that a compared position misses beyond the coincidence
//! allowance states its `tolerance` as its largest distance from the
//! positions compared with it. Only records the decoder read are measured: a
//! record whose annotations mark it computed (any exactness but byte-exact,
//! other than its `tolerance`) is not. A vertex is left as stored where a
//! missing position lies more than twice as far from it as the carrier
//! itself does, or where that carrier distance cannot be measured: there a
//! range is off, not the vertex.
//!
//! The pass measures the current geometry, then the resolved state of each
//! evaluation and process state with content: its base with its delta
//! applied, as validation resolves it. A vertex that only a resolved state
//! compares with a moved position states its widened tolerance in that
//! state's delta: the delta's own record, or a copy of the base's record,
//! annotated as the base's record is, that the delta then restates. Each
//! state is measured after the states its chain of bases passes, with the
//! tolerances they widen, and over the records connected through references
//! with a record its delta changes or with a vertex its base widens: every
//! other vertex is compared as in the base, where it was measured. A state
//! whose base is the current state or empty is measured whole. A record
//! inside a history body or a witness body has no annotation key of its own,
//! so its widened tolerance could not be marked derived; it keeps its stored
//! tolerance, and validation reports its gaps.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit, WorkBudget};
use cadmpeg_core::CodecError;

use crate::annotations::{AnnotationBuilder, AnnotationKey, Annotations, FieldName};
use crate::codec::loss::DecodeLossCode;
use crate::eval::admission::ModelEvaluationPass;
use crate::eval::decode::{curve_point, surface_point_solved};
use crate::eval::finite_or_refusal;
use crate::geometry::{Curve, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::index::ModelIndex;
use crate::math::Point3;
use crate::provenance::Exactness;
use crate::report::loss::LossNote;
use crate::scalar::PositiveLength;
use crate::topology::{IncreasingParameterInterval, Vertex};
use crate::validate::design::resolved::visit::StateView;
use crate::validate::geometry_consistency::{
    pcurve_image_comparisons, ComparedCarrier, CurveEndComparisons, VertexComparison,
};
use crate::validate::process::evaluations::visit_resolved_states;
use crate::CadIr;

/// A compared position farther from the vertex than this many times the
/// vertex's distance from the carrier is off on the carrier.
const CARRIER_DISTANCE_FACTOR: f64 = 2.0;

/// Parameters sampled across a carrier's domain to find its nearest point.
const CARRIER_SAMPLES: u32 = 64;

/// Golden-section steps that refine the nearest sampled parameter.
const CARRIER_REFINEMENTS: usize = 48;

/// The work one NURBS surface projection may spend.
const SURFACE_PROJECTION_WORK: usize = 1_000_000;

/// Widens the tolerance of each vertex of the current geometry of `ir`, and
/// of each resolved state of its evaluations and process states, that a
/// compared position misses; marks each widened tolerance derived in
/// `annotations`; and returns the `geometry.vertex-tolerance-widened` loss
/// where it widens one. The pass measures in a session of its own under
/// `policy`. Where it meets a resource ceiling of that session, it widens no
/// tolerance of the geometry it has not measured and returns the
/// `geometry.vertex-tolerance-unmeasured` loss.
pub(super) fn widen(
    ir: &mut CadIr,
    annotations: &mut Annotations,
    policy: &DecodePolicy,
) -> Vec<LossNote> {
    widen_measuring(ir, annotations, policy, Measured::Region)
}

/// What the pass measures of the resolved state of an evaluation or a
/// process state.
#[derive(Clone, Copy)]
enum Measured {
    /// The records connected with a record its delta changes or with a
    /// vertex its base widens, over a base the pass measured.
    Region,
    /// Every record, which widens as `Region` does.
    #[cfg(test)]
    Whole,
}

fn widen_measuring(
    ir: &mut CadIr,
    annotations: &mut Annotations,
    policy: &DecodePolicy,
    measured: Measured,
) -> Vec<LossNote> {
    let arena = DecodeArena::new();
    let read = |key: &str| !computed(annotations, key);
    let measured = DecodeContext::from_root_bytes_limit(&[], &arena, policy)
        .map_err(CodecError::from)
        .and_then(|(ctx, _)| {
            let current = measure(&ctx, ir, &read)?;
            let mut states = StateWidening::new(&current, measured);
            let stopped = visit_resolved_states(&ctx, ir, &mut |state| {
                states.measure(&ctx, ir, &read, &state)
            })
            .err();
            let deltas = states.widened;
            Ok((current, deltas, stopped))
        });
    let (current, deltas, stopped) = match measured {
        Ok(measured) => measured,
        Err(error) => {
            return vec![DecodeLossCode::VertexToleranceUnmeasured.note(format!(
                "the decode wrapper's vertex tolerance pass stopped ({error}), so it widened no \
                 vertex tolerance"
            ))]
        }
    };
    let mut written = Vec::new();
    for delta in deltas {
        let Some((key, inserted)) = write_into_delta(ir, &delta.holder, delta.vertex) else {
            continue;
        };
        if inserted {
            annotations.copy_entity(&delta.source, key.clone());
        }
        written.push((key, delta.increase));
    }
    let linear = ir.tolerances.linear;
    let mut largest = 0.0_f64;
    let mut widened = Vec::new();
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    for vertex in &mut ir.geometry.vertices {
        let Some(tolerance) = current
            .get(vertex.id.as_str())
            .copied()
            .and_then(PositiveLength::new)
        else {
            continue;
        };
        builder.derived_field(vertex.id.as_str(), TOLERANCE);
        let before = vertex.tolerance.unwrap_or(linear).get();
        vertex.tolerance = Some(tolerance);
        largest = largest.max(tolerance.get() - before);
        widened.push(vertex.id.as_str().to_owned());
    }
    for (key, increase) in written {
        builder.derived_field(&key, TOLERANCE);
        largest = largest.max(increase);
        widened.push(key.as_str().to_owned());
    }
    *annotations = builder.build();
    let mut losses = Vec::new();
    if !widened.is_empty() {
        widened.sort_unstable();
        losses.push(DecodeLossCode::VertexToleranceWidened.note(format!(
            "widened {} vertex tolerance(s) to each vertex's largest distance from the \
             positions compared with it; the largest widening is {largest} mm: {}",
            widened.len(),
            widened.join(", ")
        )));
    }
    if let Some(stopped) = stopped {
        let stopped = CodecError::from(stopped);
        losses.push(DecodeLossCode::VertexToleranceUnmeasured.note(format!(
            "the decode wrapper's vertex tolerance pass stopped ({stopped}), so it widened no \
             vertex tolerance of the resolved states of evaluations and process states it had not \
             measured"
        )));
    }
    losses
}

/// A vertex tolerance widened in the resolved state of an evaluation or a
/// process state, which that state's delta states.
struct DeltaWidening {
    /// The id of the evaluation or process state.
    holder: String,
    /// The vertex as the resolved state holds it, with its widened tolerance.
    vertex: Vertex,
    /// The annotation key of the resolved state's record of the vertex.
    source: String,
    /// The widened tolerance less the tolerance in force before.
    increase: f64,
}

/// The tolerances the pass widens in resolved states, in the order it
/// measures them.
struct StateWidening<'w> {
    /// The tolerances widened in the current geometry.
    current: &'w BTreeMap<String, f64>,
    measured: Measured,
    /// For each vertex, each evaluation or process state that widens it,
    /// with the tolerance it states there.
    by_state: HashMap<String, Vec<(String, PositiveLength)>>,
    /// For each evaluation or process state, the vertices it widens.
    by_holder: HashMap<String, Vec<String>>,
    /// The evaluations and process states the pass measured.
    measured_states: HashSet<String>,
    widened: Vec<DeltaWidening>,
}

impl<'w> StateWidening<'w> {
    fn new(current: &'w BTreeMap<String, f64>, measured: Measured) -> Self {
        Self {
            current,
            measured,
            by_state: HashMap::new(),
            by_holder: HashMap::new(),
            measured_states: HashSet::new(),
            widened: Vec::new(),
        }
    }

    /// The tolerance that the vertex `id`, which the delta of `source` holds
    /// (the current geometry where `None`), states in `state` after the
    /// widenings of the states its chain of bases passes.
    fn widened_below(
        &self,
        state: &StateView<'_, '_>,
        id: &str,
        source: Option<&str>,
    ) -> Option<PositiveLength> {
        if source == Some(state.holder) {
            return None;
        }
        if let Some(widened) = self.by_state.get(id) {
            for &base in &state.bases {
                if let Some((_, tolerance)) = widened.iter().find(|(holder, _)| holder == base) {
                    return Some(*tolerance);
                }
                if source == Some(base) {
                    return None;
                }
            }
        }
        if source.is_some() {
            return None;
        }
        self.current.get(id).copied().and_then(PositiveLength::new)
    }

    /// Measures one resolved state, each vertex at the tolerance the states
    /// below it widen, between the records `read` admits by annotation key.
    fn measure(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        read: &dyn Fn(&str) -> bool,
        state: &StateView<'_, '_>,
    ) -> Result<(), ResourceLimit> {
        let base = state
            .bases
            .first()
            .filter(|base| self.measured_states.contains(**base));
        let mut topology = match (self.measured, base) {
            (Measured::Region, Some(base)) => state.region(
                ctx,
                self.by_holder
                    .get(*base)
                    .into_iter()
                    .flatten()
                    .map(String::as_str),
            )?,
            _ => state.whole(),
        };
        self.measured_states.insert(state.holder.to_owned());
        if topology.vertices.is_empty() {
            return Ok(());
        }
        for vertex in &mut topology.vertices {
            let source = state.source(vertex.id.as_str());
            if let Some(tolerance) = self.widened_below(state, vertex.id.as_str(), source) {
                vertex.tolerance = Some(tolerance);
            }
        }
        let mut alone = CadIr::empty();
        alone.fragment = ir.fragment;
        alone.tolerances = ir.tolerances;
        alone.geometry = topology;
        let key = |id: &str| match state.source(id) {
            Some(holder) => format!("{holder} {id}"),
            None => id.to_owned(),
        };
        let tolerances = measure(ctx, &alone, &|id| read(&key(id)))?;
        for vertex in alone.geometry.vertices {
            let Some(tolerance) = tolerances
                .get(vertex.id.as_str())
                .copied()
                .and_then(PositiveLength::new)
            else {
                continue;
            };
            let before = vertex.tolerance.unwrap_or(ir.tolerances.linear).get();
            self.by_state
                .entry(vertex.id.as_str().to_owned())
                .or_default()
                .push((state.holder.to_owned(), tolerance));
            self.by_holder
                .entry(state.holder.to_owned())
                .or_default()
                .push(vertex.id.as_str().to_owned());
            self.widened.push(DeltaWidening {
                holder: state.holder.to_owned(),
                source: key(vertex.id.as_str()),
                vertex: Vertex {
                    tolerance: Some(tolerance),
                    ..vertex
                },
                increase: tolerance.get() - before,
            });
        }
        Ok(())
    }
}

/// States `vertex` in the delta of the evaluation or process state `holder`:
/// replaces the delta's record of its id, or adds it in id order. Returns the
/// vertex's annotation key in that delta and whether the delta gained the
/// record; `None` where no evaluation or process state of that id has
/// content.
fn write_into_delta(ir: &mut CadIr, holder: &str, vertex: Vertex) -> Option<(AnnotationKey, bool)> {
    let (key, delta) = if let Some(evaluation) = ir
        .design
        .evaluations
        .iter_mut()
        .find(|evaluation| evaluation.id.as_str() == holder)
    {
        let key = AnnotationKey::in_delta(&evaluation.id, vertex.id.as_str());
        (key, &mut evaluation.content.as_mut()?.delta)
    } else {
        let state = ir
            .process
            .states
            .iter_mut()
            .find(|state| state.id.as_str() == holder)?;
        let key = AnnotationKey::in_delta(&state.id, vertex.id.as_str());
        (key, &mut state.content.as_mut()?.delta)
    };
    let vertices = &mut delta.records.vertices;
    let inserted =
        match vertices.binary_search_by(|stated| stated.id.as_str().cmp(vertex.id.as_str())) {
            Ok(at) => {
                vertices[at] = vertex;
                false
            }
            Err(at) => {
                vertices.insert(at, vertex);
                true
            }
        };
    Some((key, inserted))
}

/// The annotated field of a vertex's tolerance.
const TOLERANCE: FieldName = FieldName::from_static("tolerance");

/// Whether the annotations mark the record computed rather than read.
fn computed(annotations: &Annotations, id: &str) -> bool {
    annotations.exactness().get(id).is_some_and(|note| {
        note.entity() != Exactness::ByteExact
            || note
                .fields()
                .iter()
                .any(|(field, exactness)| *field != TOLERANCE && *exactness != Exactness::ByteExact)
    })
}

/// The tolerance each vertex of `ir`'s geometry states after the pass, for
/// the vertices it widens, measuring only between the records `read` admits.
fn measure(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    read: &dyn Fn(&str) -> bool,
) -> Result<BTreeMap<String, f64>, ResourceLimit> {
    let index = ModelIndex::new_model_only(ir);
    // Each vertex read from the source, with its position.
    let vertices: HashMap<&str, Point3> = ir
        .geometry
        .vertices
        .iter()
        .filter(|vertex| read(vertex.id.as_str()) && read(vertex.point.as_str()))
        .filter_map(|vertex| {
            let (position, _) = index.vertex_position(vertex.id.as_str())?;
            Some((vertex.id.as_str(), *position))
        })
        .collect();
    let mut comparisons: Vec<VertexComparison<'_>> = Vec::new();
    let curve_ends = CurveEndComparisons::new(&index);
    for edge in &ir.geometry.edges {
        comparisons.extend(curve_ends.edge(ctx, edge)?);
    }
    for coedge in &ir.geometry.coedges {
        comparisons.extend(curve_ends.coedge(ctx, coedge)?);
    }
    pcurve_image_comparisons(ctx, &index, &ModelEvaluationPass::new(), |comparison| {
        comparisons.push(comparison);
    })?;
    let mut largest: BTreeMap<&str, f64> = BTreeMap::new();
    let mut misses: BTreeMap<&str, Vec<(f64, ComparedCarrier<'_>)>> = BTreeMap::new();
    for comparison in &comparisons {
        if !read(comparison.subject) || !comparison.records.iter().all(|record| read(record)) {
            continue;
        }
        for end in &comparison.ends {
            if !vertices.contains_key(end.vertex) || !end.distance.is_finite() {
                continue;
            }
            let distance = largest.entry(end.vertex).or_default();
            *distance = distance.max(end.distance);
            if end.distance > end.allowance {
                misses
                    .entry(end.vertex)
                    .or_default()
                    .push((end.distance, comparison.carrier));
            }
        }
    }
    let mut tolerances = BTreeMap::new();
    'vertices: for (vertex, misses) in misses {
        let Some(&position) = vertices.get(vertex) else {
            continue;
        };
        for (distance, carrier) in misses {
            let carrier = match carrier {
                ComparedCarrier::Curve(curve) => curve_distance(ctx, curve, position)?,
                ComparedCarrier::Surface(surface) => surface_distance(ctx, surface, position)?,
            };
            let Some(carrier) = carrier else {
                continue 'vertices;
            };
            if distance > CARRIER_DISTANCE_FACTOR * carrier {
                continue 'vertices;
            }
        }
        if let Some(&tolerance) = largest.get(vertex) {
            tolerances.insert(vertex.to_owned(), tolerance);
        }
    }
    Ok(tolerances)
}

/// The distance of `point` from `surface`, or `None` where the surface has
/// no solved geometry this pass projects onto or the projection has no
/// finite point.
fn surface_distance(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    point: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let Some(solved) = surface.solved() else {
        return Ok(None);
    };
    let parameters = match solved {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            crate::eval::nurbs_surface_closest_parameter_with_budget(
                ctx,
                nurbs,
                point,
                None,
                &WorkBudget::new(SURFACE_PROJECTION_WORK),
            )?
        }
        _ => crate::eval::analytic_surface_parameters_solved(solved, point),
    };
    let Some(parameters) = parameters else {
        return Ok(None);
    };
    let parameters = parameters.get();
    Ok(finite_or_refusal(surface_point_solved(
        ctx,
        solved,
        parameters.u,
        parameters.v,
    ))?
    .map(|at| Point3::distance(at.get(), point))
    .filter(|distance| distance.is_finite()))
}

/// The distance of `point` from `curve` over the curve's own domain, or
/// `None` where the curve has no domain this pass searches or no finite
/// point in it.
fn curve_distance(
    ctx: &DecodeContext<'_>,
    curve: &Curve,
    point: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let range = curve
        .parameter_range
        .map(IncreasingParameterInterval::endpoints);
    let solved = curve.geometry.solved();
    if let Some(SolvedCurveGeometry::Line(_)) = solved {
        return line_distance(ctx, curve, range, point);
    }
    let domain = range.or_else(|| match solved {
        Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            Some([0.0, std::f64::consts::TAU])
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => crate::eval::nurbs_curve_parameter_domain(nurbs)
            .map(IncreasingParameterInterval::endpoints),
        _ => None,
    });
    let Some([low, high]) = domain else {
        return Ok(None);
    };
    let distance_at = |parameter: f64| -> Result<Option<f64>, ResourceLimit> {
        Ok(
            finite_or_refusal(curve_point(ctx, &curve.geometry, parameter))?
                .map(|at| Point3::distance(at.get(), point))
                .filter(|distance| distance.is_finite()),
        )
    };
    let step = (high - low) / f64::from(CARRIER_SAMPLES);
    let mut nearest: Option<(f64, f64)> = None;
    for sample in 0..=CARRIER_SAMPLES {
        let parameter = low + step * f64::from(sample);
        if let Some(distance) = distance_at(parameter)? {
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((parameter, distance));
            }
        }
    }
    let Some((parameter, mut best)) = nearest else {
        return Ok(None);
    };
    let (mut a, mut b) = ((parameter - step).max(low), (parameter + step).min(high));
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..CARRIER_REFINEMENTS {
        let (left, right) = (b - ratio * (b - a), a + ratio * (b - a));
        let (Some(at_left), Some(at_right)) = (distance_at(left)?, distance_at(right)?) else {
            break;
        };
        best = best.min(at_left).min(at_right);
        if at_left < at_right {
            b = right;
        } else {
            a = left;
        }
    }
    Ok(Some(best))
}

/// The distance of `point` from a line, over its `range` where it has one.
fn line_distance(
    ctx: &DecodeContext<'_>,
    curve: &Curve,
    range: Option<[f64; 2]>,
    point: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let (Some(origin), Some(unit)) = (
        finite_or_refusal(curve_point(ctx, &curve.geometry, 0.0))?,
        finite_or_refusal(curve_point(ctx, &curve.geometry, 1.0))?,
    ) else {
        return Ok(None);
    };
    let (origin, unit) = (origin.get(), unit.get());
    let direction = [unit.x - origin.x, unit.y - origin.y, unit.z - origin.z];
    let offset = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    let length = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if !(length.is_finite() && length > 0.0) {
        return Ok(None);
    }
    let along = direction
        .iter()
        .zip(offset)
        .map(|(direction, offset)| direction * offset)
        .sum::<f64>()
        / length;
    let parameter = match range {
        Some([low, high]) => along.clamp(low, high),
        None => along,
    };
    Ok(
        finite_or_refusal(curve_point(ctx, &curve.geometry, parameter))?
            .map(|at| Point3::distance(at.get(), point))
            .filter(|distance| distance.is_finite()),
    )
}

#[cfg(test)]
mod tests;
