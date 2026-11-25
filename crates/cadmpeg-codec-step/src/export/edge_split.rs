// SPDX-License-Identifier: Apache-2.0
//! Preserve open edge topology at the representation uncertainty.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{
    curve_endpoints_coincident, finite_or_refusal,
    model_curve_parameter_near_point_in_index_with_tolerance, model_curve_point_by_id,
};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Edge;

/// How an open edge is written.
pub(super) enum Split {
    /// As one `EDGE_CURVE`: its ends do not coincide.
    Whole,
    /// As two `EDGE_CURVE`s meeting at this point of its curve: its ends
    /// coincide within the linear tolerance, and one `EDGE_CURVE` between
    /// them would read as closed.
    At(Point3),
    /// As one `EDGE_CURVE` although its ends coincide: no point of its curve
    /// strictly between them was found.
    Unsplittable,
}

/// How `edge` is written. An error is a decode-context or resource failure
/// of the parameter search.
pub(super) fn midpoint(
    index: &ModelIndex<'_>,
    edge: &Edge,
    ctx: &DecodeContext<'_>,
) -> Result<Split, CodecError> {
    let Some(ends) = edge.ends.as_ref().filter(|ends| ends.start != ends.end) else {
        return Ok(Split::Whole);
    };
    let position = |id: &cadmpeg_ir::ids::VertexId| {
        let vertex = index.vertices(id.as_str(), StandardIndex)?;
        Some(
            index
                .points(vertex.point.as_str(), StandardIndex)?
                .position()
                .get(),
        )
    };
    let (Some(start), Some(end)) = (position(&ends.start), position(&ends.end)) else {
        return Ok(Split::Whole);
    };
    if !curve_endpoints_coincident([start, end], index.ir().tolerances.linear.get()) {
        return Ok(Split::Whole);
    }
    let Some(curve_id) = edge.curve() else {
        return Ok(Split::Whole);
    };
    let Some(curve) = index.curves(curve_id.as_str(), StandardIndex) else {
        return Ok(Split::Whole);
    };
    let domain = parameter_domain(index, &curve.id);
    let range = edge
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        .or(match domain {
            Some(ParameterDomain::Bounded(range)) => Some(range),
            _ => None,
        });
    let [first, last] = match range {
        Some(range) => range,
        None => {
            let parameter = |point, seed| {
                model_curve_parameter_near_point_in_index_with_tolerance(
                    ctx,
                    index,
                    curve_id,
                    point,
                    seed,
                    index.ir().tolerances.linear.get(),
                )
            };
            let seeds = match domain {
                Some(ParameterDomain::Bounded(range) | ParameterDomain::Search(range)) => range,
                _ => [0.0, 1.0],
            };
            let first = parameter(start, seeds[0])?;
            let last = parameter(end, seeds[1])?;
            if let Some(limit) = ctx.resource_refusal() {
                return Err(limit.into());
            }
            let (Some(first), Some(last)) = (first, last) else {
                return Ok(Split::Unsplittable);
            };
            let (first, mut last) = (first.get(), last.get());
            if let Some(ParameterDomain::Periodic(period)) = domain {
                let span = (last - first).rem_euclid(period);
                last = first + if span == 0.0 { period } else { span };
            } else if last <= first {
                return Ok(Split::Unsplittable);
            }
            [first, last]
        }
    };
    let middle = first * 0.5 + last * 0.5;
    if middle == first || middle == last {
        return Ok(Split::Unsplittable);
    }
    let point = finite_or_refusal(model_curve_point_by_id(
        EvaluationAdmission::Decode(ctx),
        index,
        curve_id,
        middle,
    ))?;
    Ok(point.map_or(Split::Unsplittable, |point| Split::At(point.get())))
}

#[derive(Clone, Copy)]
enum ParameterDomain {
    Bounded([f64; 2]),
    Periodic(f64),
    Search([f64; 2]),
}

fn parameter_domain<'a>(
    index: &'a ModelIndex<'_>,
    mut id: &'a cadmpeg_ir::ids::CurveId,
) -> Option<ParameterDomain> {
    use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
    use cadmpeg_ir::geometry::surface_curves::SurfaceCurveFamily;
    let mut visited = std::collections::BTreeSet::new();
    loop {
        if !visited.insert(id.as_str()) {
            return None;
        }
        let curve = index.curves(id.as_str(), StandardIndex)?;
        if let Some(range) = curve.parameter_range {
            return Some(ParameterDomain::Bounded(range.endpoints()));
        }
        if let Some(definition) = index
            .procedural_curves_for_curve(id.as_str(), StandardIndex)
            .and_then(|items| items.first())
            .map(|item| item.definition())
        {
            match definition {
                ProceduralCurveDefinition::Replica { source, .. } => {
                    id = source;
                    continue;
                }
                ProceduralCurveDefinition::SpatialOffset(offset) => {
                    id = offset.source();
                    continue;
                }
                ProceduralCurveDefinition::Subset(subset) => {
                    let [first, last] = subset.parameter_range().endpoints();
                    let span = last - first;
                    return (span.is_finite() && span > 0.0)
                        .then_some(ParameterDomain::Search([0.0, span]));
                }
                ProceduralCurveDefinition::Helix(helix) => {
                    return Some(ParameterDomain::Search(helix.angle_range().get()))
                }
                ProceduralCurveDefinition::SurfaceCurve {
                    family: SurfaceCurveFamily::Parametric { context, .. },
                } => {
                    return Some(ParameterDomain::Search(
                        context.parameter_range().endpoints(),
                    ))
                }
                _ => {}
            }
        }
        let geometry = curve.geometry.solved()?;
        return geometry_domain(geometry);
    }
}

fn geometry_domain(
    geometry: &cadmpeg_ir::geometry::SolvedCurveGeometry,
) -> Option<ParameterDomain> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some(ParameterDomain::Periodic(std::f64::consts::TAU))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let range = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
            Some(if nurbs.periodic() {
                ParameterDomain::Periodic(range[1] - range[0])
            } else {
                ParameterDomain::Bounded(range)
            })
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let range = if let Some(mut parameters) = polyline.parameters() {
                let first = parameters.next()?.get();
                [
                    first,
                    parameters
                        .last()
                        .map_or(first, cadmpeg_ir::scalar::FiniteReal::get),
                ]
            } else {
                [
                    0.0,
                    cadmpeg_core::convert::f64_from_index(polyline.point_count().checked_sub(1)?)?,
                ]
            };
            Some(ParameterDomain::Search(range))
        }
        SolvedCurveGeometry::Transformed(placed) => geometry_domain(placed.basis()),
        _ => None,
    }
}
