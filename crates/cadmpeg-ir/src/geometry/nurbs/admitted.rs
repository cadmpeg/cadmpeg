// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted raw NURBS construction.

use super::{KnotVector, NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, WeightedPole3};
use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::NonZeroReal;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

// Resource refusals stay outside the geometry refusal result.
enum ConstructionError {
    Resource(CodecError),
    Geometry(NurbsError),
}

impl From<CodecError> for ConstructionError {
    fn from(error: CodecError) -> Self { Self::Resource(error) }
}
impl From<NurbsError> for ConstructionError {
    fn from(error: NurbsError) -> Self { Self::Geometry(error) }
}

fn finish<T>(result: Result<T, ConstructionError>) -> Result<Result<T, NurbsError>, CodecError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(ConstructionError::Geometry(error)) => Ok(Err(error)),
        Err(ConstructionError::Resource(error)) => Err(error),
    }
}

fn structure(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> Result<ConstructionError, CodecError> {
    Ok(NurbsError::Structure(ctx.format_retained(message, "IR NURBS refusal text")?).into())
}

fn weight_lane(ctx: &DecodeContext<'_>, field: &str, poles: usize, weights: usize) -> Result<(), ConstructionError> {
    if poles != weights {
        return Err(NurbsError::WeightLaneLength { field: ctx.copy_retained_text(field, "IR NURBS refusal field")?, poles, weights }.into());
    }
    Ok(())
}

fn admitted_weight(ctx: &DecodeContext<'_>, field: &str, index: usize, weight: f64) -> Result<NonZeroReal, ConstructionError> {
    if let Some(weight) = NonZeroReal::new(weight) { return Ok(weight); }
    Err(NurbsError::UnusableWeight { field: ctx.copy_retained_text(field, "IR NURBS refusal field")?, index, weight }.into())
}

fn finite_point(ctx: &DecodeContext<'_>, point: Point3) -> Result<FinitePoint3, ConstructionError> {
    if let Some(point) = FinitePoint3::new(point) { return Ok(point); }
    Err(structure(ctx, format_args!("control_points contains a non-finite point"))?)
}

fn length(ctx: &DecodeContext<'_>, field: &str, actual: usize, expected: usize) -> Result<(), ConstructionError> {
    if actual != expected { return Err(structure(ctx, format_args!("{field} must contain {expected} values, found {actual}"))?); }
    Ok(())
}

fn knot_count(ctx: &DecodeContext<'_>, field: &str, poles: usize, degree: u32) -> Result<usize, ConstructionError> {
    let Ok(degree) = usize::try_from(degree) else { return Err(structure(ctx, format_args!("{field} knot count overflows usize"))?); };
    match poles.checked_add(degree).and_then(|count| count.checked_add(1)) {
        Some(count) => Ok(count),
        None => Err(structure(ctx, format_args!("{field} knot count overflows usize"))?),
    }
}

fn curve_cardinality(ctx: &DecodeContext<'_>, degree: u32, knots: usize, poles: usize) -> Result<(), ConstructionError> {
    if u64::try_from(poles).is_ok_and(|count| count <= u64::from(degree)) {
        return Err(structure(ctx, format_args!("control_points must contain more than degree {degree} poles, found {poles}"))?);
    }
    length(ctx, "knots", knots, knot_count(ctx, "curve", poles, degree)?)
}

fn surface_structure(ctx: &DecodeContext<'_>, u: &NurbsSurfaceAxis, v: &NurbsSurfaceAxis, poles: &NurbsPoleGrid) -> Result<(), ConstructionError> {
    let u_count = poles.u_count();
    let v_count = poles.v_count();
    if u64::try_from(u_count).is_ok_and(|count| count <= u64::from(u.degree)) {
        return Err(structure(ctx, format_args!("u_count must exceed u_degree {}, found {u_count}", u.degree))?);
    }
    if u64::try_from(v_count).is_ok_and(|count| count <= u64::from(v.degree)) {
        return Err(structure(ctx, format_args!("v_count must exceed v_degree {}, found {v_count}", v.degree))?);
    }
    length(ctx, "u_knots", u.knots.len(), knot_count(ctx, "u", u_count, u.degree)?)?;
    length(ctx, "v_knots", v.knots.len(), knot_count(ctx, "v", v_count, v.degree)?)?;
    let width = poles.v_count();
    let wrong = match poles {
        NurbsPoleGrid::Polynomial { rows } => rows.iter().find(|row| row.len() != width).map(Vec::len),
        NurbsPoleGrid::Rational { rows } => rows.iter().find(|row| row.len() != width).map(Vec::len),
    };
    if let Some(actual) = wrong {
        return Err(structure(ctx, format_args!("control_points row must contain {width} values, found {actual}"))?);
    }
    Ok(())
}

fn admitted_knots(ctx: &DecodeContext<'_>, knots: Vec<f64>, prefix: &str) -> Result<KnotVector, ConstructionError> {
    if !knots.iter().all(|value| value.is_finite()) {
        return Err(structure(ctx, format_args!("{prefix}knots contains a non-finite value"))?);
    }
    if !super::knots_nondecreasing(&knots) {
        return Err(structure(ctx, format_args!("{prefix}knots must be non-decreasing"))?);
    }
    Ok(KnotVector(knots))
}

fn collect<T, I>(ctx: &DecodeContext<'_>, values: I, operation: &'static str)
    -> Result<Vec<T>, ConstructionError>
where I: IntoIterator<Item = Result<T, ConstructionError>>,
{
    let mut output = Vec::new();
    for value in values {
        ctx.try_reserve_items(&mut output, 1, operation)?;
        output.push(value?);
    }
    Ok(output)
}

fn pair(ctx: &DecodeContext<'_>, points: Vec<Point3>, weights: Vec<f64>, field: &str)
    -> Result<Vec<WeightedPole3>, ConstructionError>
{
    weight_lane(ctx, field, points.len(), weights.len())?;
    let points = collect(ctx, points.into_iter().zip(weights).enumerate().map(|(index, (point, weight))|
        Ok(WeightedPole3 { point, weight: admitted_weight(ctx, field, index, weight)? })),
        "IR NURBS paired poles")?;
    Ok(points)
}

fn admit(ctx: &DecodeContext<'_>, poles: NurbsPoles3)
    -> Result<NurbsPoles3<FinitePoint3>, ConstructionError>
{
    Ok(match poles {
        NurbsPoles3::Polynomial { points } => NurbsPoles3::Polynomial {
            points: collect(ctx, points.into_iter().map(|point|
                finite_point(ctx, point)), "IR NURBS admitted poles")?,
        },
        NurbsPoles3::Rational { points } => NurbsPoles3::Rational {
            points: collect(ctx, points.into_iter().map(|pole| Ok(WeightedPole3 {
                point: finite_point(ctx, pole.point)?,
                weight: pole.weight,
            })), "IR NURBS admitted poles")?,
        },
    })
}

impl NurbsCurve {
    /// Construct raw lanes with caller admission before pole pairing and conversion.
    /// Resource refusal is separate from the geometry refusal, whose order is unchanged.
    pub fn from_lanes_admitted(ctx: &DecodeContext<'_>, degree: u32, knots: Vec<f64>,
        control_points: Vec<Point3>, weights: Option<Vec<f64>>, periodic: bool)
        -> Result<Result<Self, NurbsError>, CodecError>
    {
        finish((|| {
            let poles = match weights {
                Some(weights) => NurbsPoles3::Rational { points: pair(ctx, control_points, weights, "poles")? },
                None => NurbsPoles3::Polynomial { points: control_points },
            };
            curve_cardinality(ctx, degree, knots.len(), poles.count())?;
            let poles = admit(ctx, poles)?;
            let knots = admitted_knots(ctx, knots, "")?;
            Ok(Self { degree, knots, poles, periodic })
        })())
    }
}

impl NurbsSurface {
    /// Construct raw grids with caller admission before every outer and inner allocation.
    /// Resource refusal is separate from the geometry refusal, whose order is unchanged.
    pub fn from_lanes_admitted(ctx: &DecodeContext<'_>, u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis, lanes: NurbsSurfaceLanes, normal_reversed: bool)
        -> Result<Result<Self, NurbsError>, CodecError>
    {
        finish((|| {
            let NurbsSurfaceLanes { control_points, weights } = lanes;
            let poles = if let Some(weights) = weights {
                weight_lane(ctx, "pole grid", control_points.len(), weights.len())?;
                let mut rows = Vec::new();
                for (points, weights) in control_points.into_iter().zip(weights) {
                    ctx.try_reserve_items(&mut rows, 1, "IR NURBS paired grid rows")?;
                    let points = pair(ctx, points, weights, "pole grid row")?;
                    rows.push(points);
                }
                NurbsPoleGrid::Rational { rows }
            } else { NurbsPoleGrid::Polynomial { rows: control_points } };
            surface_structure(ctx, &u, &v, &poles)?;
            let poles = match poles {
                NurbsPoleGrid::Polynomial { rows } => {
                    let mut output = Vec::new();
                    for points in rows {
                        ctx.try_reserve_items(&mut output, 1, "IR NURBS admitted grid rows")?;
                        let points = collect(ctx, points.into_iter().map(|point|
                            finite_point(ctx, point)), "IR NURBS admitted poles")?;
                        output.push(points);
                    }
                    NurbsPoleGrid::Polynomial { rows: output }
                },
                NurbsPoleGrid::Rational { rows } => {
                    let mut output = Vec::new();
                    for points in rows {
                        ctx.try_reserve_items(&mut output, 1, "IR NURBS admitted grid rows")?;
                        let points = collect(ctx, points.into_iter().map(|pole| Ok(WeightedPole3 {
                            point: finite_point(ctx, pole.point)?,
                            weight: pole.weight,
                        })), "IR NURBS admitted poles")?;
                        output.push(points);
                    }
                    NurbsPoleGrid::Rational { rows: output }
                },
            };
            let u_knots = admitted_knots(ctx, u.knots, "u_")?;
            let v_knots = admitted_knots(ctx, v.knots, "v_")?;
            Ok(Self { u_degree: u.degree, v_degree: v.degree, u_knots, v_knots, poles,
                normal_reversed, u_periodic: u.periodic, v_periodic: v.periodic })
        })())
    }
}

#[cfg(test)]
mod tests;
