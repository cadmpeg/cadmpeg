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
//! The pass widens the current geometry alone. A record inside a history
//! body or a witness body has no annotation key of its own, so its widened
//! tolerance could not be marked derived. A record inside an evaluation's or
//! a process state's delta keeps its stored tolerance. Validation reports the
//! gaps in each of them.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit, WorkBudget};
use cadmpeg_core::CodecError;

use crate::annotations::{AnnotationBuilder, Annotations, FieldName};
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
use crate::topology::IncreasingParameterInterval;
use crate::validate::geometry_consistency::{
    pcurve_image_comparisons, ComparedCarrier, CurveEndComparisons, VertexComparison,
};
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

/// Widens the tolerance of each vertex of the current geometry of `ir` that
/// a compared position misses, marks each widened tolerance derived in
/// `annotations`, and returns the `geometry.vertex-tolerance-widened` loss
/// where it widens one. The pass measures in a session of its own under
/// `policy`. Where it meets a resource ceiling of that session, it widens
/// nothing and returns the `geometry.vertex-tolerance-unmeasured` loss.
pub(super) fn widen(
    ir: &mut CadIr,
    annotations: &mut Annotations,
    policy: &DecodePolicy,
) -> Option<LossNote> {
    let arena = DecodeArena::new();
    let measured = DecodeContext::from_root_bytes_limit(&[], &arena, policy)
        .and_then(|(ctx, _)| measure(&ctx, ir, &|id: &str| !computed(annotations, id)));
    let tolerances = match measured {
        Ok(tolerances) => tolerances,
        Err(limit) => {
            return Some(DecodeLossCode::VertexToleranceUnmeasured.note(format!(
                "the decode wrapper's vertex tolerance pass stopped ({}), so it widened no \
                 vertex tolerance",
                CodecError::from(limit)
            )))
        }
    };
    let linear = ir.tolerances.linear;
    let mut largest = 0.0_f64;
    let mut widened = Vec::new();
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    for vertex in &mut ir.geometry.vertices {
        let Some(tolerance) = tolerances
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
    *annotations = builder.build();
    if widened.is_empty() {
        return None;
    }
    widened.sort_unstable();
    Some(DecodeLossCode::VertexToleranceWidened.note(format!(
        "widened {} vertex tolerance(s) to each vertex's largest distance from the positions \
         compared with it; the largest widening is {largest} mm: {}",
        widened.len(),
        widened.join(", ")
    )))
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
    let points: HashMap<&str, Point3> = ir
        .geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.position().get()))
        .collect();
    // Each vertex read from the source, with its position.
    let vertices: HashMap<&str, Point3> = ir
        .geometry
        .vertices
        .iter()
        .filter(|vertex| read(vertex.id.as_str()) && read(vertex.point.as_str()))
        .filter_map(|vertex| Some((vertex.id.as_str(), *points.get(vertex.point.as_str())?)))
        .collect();
    let mut comparisons: Vec<VertexComparison<'_>> = Vec::new();
    let curve_ends = CurveEndComparisons::new(ir);
    for edge in &ir.geometry.edges {
        comparisons.extend(curve_ends.edge(ctx, edge)?);
    }
    for coedge in &ir.geometry.coedges {
        comparisons.extend(curve_ends.coedge(ctx, coedge)?);
    }
    let index = ModelIndex::new_model_only(ir);
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
