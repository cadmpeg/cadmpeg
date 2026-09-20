#![allow(dead_code,unused_imports,unused_variables)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{nurbs::{NurbsCurve,NurbsSurface,NurbsSurfaceAxis,NurbsSurfaceLanes}};
use cadmpeg_core::decode::alloc_filled;
use std::borrow::Cow;
fn curve(knots:Vec<f64>,points:Vec<Point3>,weights:Option<Vec<f64>>,degree:u32)->NurbsCurve{NurbsCurve::from_lanes(degree,knots,points,weights,false).unwrap()}
fn knots_nondecreasing(k:&[f64])->bool { k.windows(2).all(|x|x[0]<=x[1]) }
#[derive(Clone)] struct HomogeneousBezierSpan {domain:[f64;2],controls:Vec<[f64;4]>}
fn midpoint(p:&[[f64;4]])->[f64;4]{let mut p=p.to_vec();for n in (1..p.len()).rev(){for i in 0..n{for j in 0..4{p[i][j]=0.5*p[i][j]+0.5*p[i+1][j]}}}p[0]}
mod ir_spans {use super::*;fn insert_homogeneous_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = alloc_filled(
        count.checked_add(1)?,
        [0.0; 4],
        "IR homogeneous knot insertion",
    )
    .ok()?;
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}
fn homogeneous_bezier_spans(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; 4]>,
) -> Option<Vec<HomogeneousBezierSpan>> {
    if degree == 0 {
        let mut spans = Vec::new();
        for (index, window) in knots.windows(2).enumerate() {
            if window[0] < window[1] {
                spans.push(HomogeneousBezierSpan {
                    domain: [window[0], window[1]],
                    controls: vec![*controls.get(index)?],
                });
            }
        }
        return (!spans.is_empty()).then_some(spans);
    }

    let mut knots = knots.to_vec();
    let domain = [*knots.get(degree)?, *knots.get(controls.len())?];
    let mut internal = knots[degree + 1..controls.len()]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(HomogeneousBezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

#[test]fn unclamped(){let poles=vec![Point3::new(0.,0.,0.),Point3::new(1.,1.,0.),Point3::new(2.,0.,0.)];let k=vec![-1.,-1.,0.,1.,2.,2.];let c=curve(k.clone(),poles.clone(),None,2);let actual=cadmpeg_ir::eval::nurbs_curve_point(2,&k,&poles,None,0.5).unwrap();let p=homogeneous_bezier_spans(2,&k,poles.iter().map(|p|[p.x,p.y,p.z,1.]).collect()).unwrap();let got=midpoint(&p[0].controls);println!("IR unclamped: decomposed_y={} actual_y={}",got[1],actual.y);assert_eq!(got[1],0.5);assert_eq!(actual.y,0.75);}
#[test]fn discontinuous(){let k=vec![0.,0.,0.,1.,1.,1.,2.,2.,2.];let poles=(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect::<Vec<_>>();let c=curve(k.clone(),poles.clone(),None,2);let p=homogeneous_bezier_spans(2,&k,poles.iter().map(|p|[p.x,0.,0.,1.]).collect()).unwrap();let got=midpoint(&p[1].controls)[0];let actual=cadmpeg_ir::eval::nurbs_curve_point(2,&k,&poles,None,1.5).unwrap().x;println!("IR discontinuous: decomposed_x={got} actual_x={actual}");assert_eq!((got,actual),(3.,4.));}
}
mod iges_spans {use super::*;fn insert_homogeneous_curve_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = alloc_filled(
        count.checked_add(1)?,
        [0.0; 4],
        "iges surface knot insertion",
    )
    .ok()?;
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}
fn homogeneous_bezier_spans(curve: &NurbsCurve) -> Option<Vec<HomogeneousBezierSpan>> {
    let degree = usize::try_from(curve.degree()).ok()?;
    let count = curve.control_points().len();
    if !knots_nondecreasing(curve.knots()) {
        return None;
    }
    let weights = curve.weights().map_or_else(
        || cadmpeg_core::decode::alloc_filled(count, 1.0, "iges_surface_closure_weights").ok(),
        Some,
    )?;
    if curve.control_points().iter().any(|point| {
        [point.x, point.y, point.z]
            .into_iter()
            .any(|value| !value.is_finite())
    }) || weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }
    let mut controls = curve
        .control_points()
        .iter()
        .zip(weights)
        .map(|(point, weight)| [weight * point.x, weight * point.y, weight * point.z, weight])
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }

    if degree == 0 {
        let mut spans = Vec::new();
        for (index, window) in curve.knots().windows(2).enumerate() {
            if window[0] < window[1] {
                spans.push(HomogeneousBezierSpan {
                    domain: [window[0], window[1]],
                    controls: vec![*controls.get(index)?],
                });
            }
        }
        return (!spans.is_empty()).then_some(spans);
    }

    let mut knots = curve.knots().to_vec();
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    let mut internal = knots[degree + 1..count]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_curve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(HomogeneousBezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

#[test]fn unclamped(){let c=curve(vec![-1.,-1.,0.,1.,2.,2.],vec![Point3::new(0.,0.,0.),Point3::new(1.,1.,0.),Point3::new(2.,0.,0.)],None,2);let p=homogeneous_bezier_spans(&c).unwrap();let got=midpoint(&p[0].controls)[1];println!("IGES unclamped decomposed_y={got} expected_y=0.75");assert_eq!(got,0.5);}
#[test]fn discontinuous(){let c=curve(vec![0.,0.,0.,1.,1.,1.,2.,2.,2.],(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect(),None,2);let p=homogeneous_bezier_spans(&c).unwrap();let got=midpoint(&p[1].controls)[0];println!("IGES discontinuous decomposed_x={got} expected_x=4");assert_eq!(got,3.);}}
mod ir_speed {use super::*;fn nurbs_curve_speed_bound_about(
    curve: &NurbsCurve,
    weights: &[f64],
    origin: Point3,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree()).ok()?;
    let count = curve.control_points().len();
    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let radius = |control: &Point3| {
        ((control.x - origin.x).powi(2)
            + (control.y - origin.y).powi(2)
            + (control.z - origin.z).powi(2))
        .sqrt()
    };
    let maximum_weighted_radius = curve
        .control_points()
        .iter()
        .zip(weights)
        .map(|(control, weight)| weight * radius(control))
        .fold(0.0_f64, f64::max);
    let mut maximum_numerator_speed = 0.0_f64;
    let mut maximum_weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = curve.knots()[index + degree + 1] - curve.knots()[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(curve.degree()) / denominator;
        let first = curve.control_points()[index];
        let second = curve.control_points()[index + 1];
        let numerator_delta = Vector3::new(
            weights[index + 1] * (second.x - origin.x) - weights[index] * (first.x - origin.x),
            weights[index + 1] * (second.y - origin.y) - weights[index] * (first.y - origin.y),
            weights[index + 1] * (second.z - origin.z) - weights[index] * (first.z - origin.z),
        );
        maximum_numerator_speed = maximum_numerator_speed.max(factor * numerator_delta.norm());
        maximum_weight_speed =
            maximum_weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let speed_bound = maximum_numerator_speed / minimum_weight
        + maximum_weighted_radius * maximum_weight_speed / minimum_weight.powi(2);
    speed_bound.is_finite().then_some(speed_bound)
}

#[test]fn common_weights(){let c=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);for w in [1.,1e-200]{let got=nurbs_curve_speed_bound_about(&c,&[w,w],Point3::new(0.,0.,0.));println!("IR speed weight={w:e} bound={got:?}");if w==1.{assert_eq!(got,Some(1.))}else{assert_eq!(got,None)}}}}
mod ir_laws {use super::*;#[derive(Clone)]enum LawExpression{Null{},Integer{value:i64},Double{value:f64},Text{value:String},Algebraic{operator:String,operands:Vec<Self>},Point{},Vector{},Transform{},TransformVec{},Edge{},Spline{}}#[derive(Clone,Copy,Debug)]struct ScalarSweepDifferential{value:f64,derivative:f64}
fn finite_sweep_differential(value: f64, derivative: f64) -> Option<ScalarSweepDifferential> {
    (value.is_finite() && derivative.is_finite())
        .then_some(ScalarSweepDifferential { value, derivative })
}
fn scalar_sweep_law_differential(
    expression: &LawExpression,
    parameter: f64,
) -> Option<ScalarSweepDifferential> {
    if !parameter.is_finite() {
        return None;
    }
    match expression {
        LawExpression::Null {} => finite_sweep_differential(0.0, 0.0),
        LawExpression::Integer { value } => finite_sweep_differential(*value as f64, 0.0),
        LawExpression::Double { value } => finite_sweep_differential(*value, 0.0),
        LawExpression::Text { value } => {
            let value = value.as_str().trim();
            if value == "X" {
                return finite_sweep_differential(parameter, 1.0);
            }
            if let Ok(constant) = value.parse::<f64>() {
                return finite_sweep_differential(constant, 0.0);
            }
            let (left, right) = value.split_once('*')?;
            if right.trim() == "X" {
                let coefficient = left.trim().parse::<f64>().ok()?;
                return finite_sweep_differential(coefficient * parameter, coefficient);
            }
            if left.trim() == "X" {
                let coefficient = right.trim().parse::<f64>().ok()?;
                return finite_sweep_differential(coefficient * parameter, coefficient);
            }
            None
        }
        LawExpression::Algebraic { operator, operands } => {
            if let [operand] = operands.as_slice() {
                let operand = scalar_sweep_law_differential(operand, parameter)?;
                return scalar_unary_sweep_law_differential(operator, operand);
            }
            if operator == "O" {
                let [outer, inner] = operands.as_slice() else {
                    return None;
                };
                let inner = scalar_sweep_law_differential(inner, parameter)?;
                let outer = scalar_sweep_law_differential(outer, inner.value)?;
                return finite_sweep_differential(outer.value, outer.derivative * inner.derivative);
            }
            let [left, right] = operands.as_slice() else {
                return None;
            };
            let left = scalar_sweep_law_differential(left, parameter)?;
            let right = scalar_sweep_law_differential(right, parameter)?;
            match operator.as_str() {
                "ADD" => finite_sweep_differential(
                    left.value + right.value,
                    left.derivative + right.derivative,
                ),
                "SUB" => finite_sweep_differential(
                    left.value - right.value,
                    left.derivative - right.derivative,
                ),
                "MUL" => finite_sweep_differential(
                    left.value * right.value,
                    left.derivative * right.value + left.value * right.derivative,
                ),
                "DIV" if right.value != 0.0 => {
                    let denominator = right.value * right.value;
                    finite_sweep_differential(
                        left.value / right.value,
                        (left.derivative * right.value - left.value * right.derivative)
                            / denominator,
                    )
                }
                _ => None,
            }
        }
        LawExpression::Point { .. }
        | LawExpression::Vector { .. }
        | LawExpression::Transform { .. }
        | LawExpression::TransformVec { .. }
        | LawExpression::Edge { .. }
        | LawExpression::Spline { .. } => None,
    }
}
fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Option<ScalarSweepDifferential> {
    let x = operand.value;
    let derivative = match operator {
        "SIN" => x.cos(),
        "COS" => -x.sin(),
        "TAN" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / (cosine * cosine))?
        }
        "COT" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-1.0 / (sine * sine))?
        }
        "SEC" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / cosine * x.tan())?
        }
        "CSC" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-(1.0 / sine) * (x.cos() / sine))?
        }
        "COSH" => x.sinh(),
        "SINH" => x.cosh(),
        "TANH" => 1.0 - x.tanh() * x.tanh(),
        "COTH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-1.0 / (sinh * sinh))?
        }
        "SECH" => {
            let value = 1.0 / x.cosh();
            -value * x.tanh()
        }
        "CSCH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-(1.0 / sinh) * (x.cosh() / sinh))?
        }
        "ARCCOS" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(-1.0 / denominator)?
        }
        "ARCSIN" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCTAN" => 1.0 / (1.0 + x * x),
        "ARCOT" => -1.0 / (1.0 + x * x),
        "ARCSEC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(1.0 / (x.abs() * denominator))?
        }
        "ARCCSC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(-1.0 / (x.abs() * denominator))?
        }
        "ARCCOSH" => {
            let denominator = (x * x - 1.0).sqrt();
            (x > 1.0 && denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCSINH" => 1.0 / (1.0 + x * x).sqrt(),
        "ARCTANH" => (x.abs() < 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCOTH" => (x.abs() > 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCSECH" => {
            let denominator = (1.0 - x * x).sqrt();
            (x > 0.0 && x < 1.0 && denominator > 0.0).then_some(-1.0 / (x * denominator))?
        }
        "ARCCSCH" => (x != 0.0).then_some(-1.0 / (x.abs() * (1.0 + x * x).sqrt()))?,
        "ABS" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                return None;
            }
        }
        "EXP" => x.exp(),
        "LN" => (x > 0.0).then_some(1.0 / x)?,
        "SIGN" => (x != 0.0).then_some(0.0)?,
        "SQRT" => (x > 0.0).then_some(0.5 / x.sqrt())?,
        _ => return None,
    };
    finite_sweep_differential(
        match operator {
            "SIN" => x.sin(),
            "COS" => x.cos(),
            "TAN" => x.tan(),
            "COT" => 1.0 / x.tan(),
            "SEC" => 1.0 / x.cos(),
            "CSC" => 1.0 / x.sin(),
            "COSH" => x.cosh(),
            "SINH" => x.sinh(),
            "TANH" => x.tanh(),
            "COTH" => 1.0 / x.tanh(),
            "SECH" => 1.0 / x.cosh(),
            "CSCH" => 1.0 / x.sinh(),
            "ARCCOS" => x.acos(),
            "ARCSIN" => x.asin(),
            "ARCTAN" => x.atan(),
            "ARCOT" => std::f64::consts::FRAC_PI_2 - x.atan(),
            "ARCSEC" => (1.0 / x).acos(),
            "ARCCSC" => (1.0 / x).asin(),
            "ARCCOSH" => x.acosh(),
            "ARCSINH" => x.asinh(),
            "ARCTANH" => x.atanh(),
            "ARCOTH" => 0.5 * ((x + 1.0) / (x - 1.0)).ln(),
            "ARCSECH" => (1.0 / x).acosh(),
            "ARCCSCH" => (1.0 / x).asinh(),
            "ABS" => x.abs(),
            "EXP" => x.exp(),
            "LN" => x.ln(),
            "SIGN" => x.signum(),
            "SQRT" => x.sqrt(),
            _ => return None,
        },
        derivative * operand.derivative,
    )
}

#[test]fn quotient(){let x=LawExpression::Text{value:"X".to_owned()};for d in [1e200,1e-200]{let expr=LawExpression::Algebraic{operator:"DIV".into(),operands:vec![x.clone(),LawExpression::Double{value:d}]};let got=scalar_sweep_law_differential(&expr,1.);println!("IR DIV denominator={d:e} got={got:?} expected_derivative={:e}",1./d);if d>1.{assert_eq!(got.unwrap().derivative,0.)}else{assert!(got.is_none())}}}
#[test]fn hyperbolic(){let d=scalar_unary_sweep_law_differential("TANH",ScalarSweepDifferential{value:20.,derivative:1e20}).unwrap();let expected=4.*(-40.0f64).exp()/(1.+(-40.0f64).exp()).powi(2)*1e20;println!("IR TANH(20) chain derivative={} expected={expected}",d.derivative);assert_eq!(d.derivative,0.);assert!(expected>1000.);let a=scalar_unary_sweep_law_differential("ARCOTH",ScalarSweepDifferential{value:1e20,derivative:1.}).unwrap();println!("IR ARCOTH(1e20) value={} expected~1e-20",a.value);assert_eq!(a.value,0.);let a=scalar_unary_sweep_law_differential("ARCSINH",ScalarSweepDifferential{value:1e200,derivative:1e200}).unwrap();println!("IR ARCSINH large chain derivative={} expected~1",a.derivative);assert_eq!(a.derivative,0.);}}
mod f3d {use super::*;enum ProfileBoundarySegment {Line{start:Point2,end:Point2},Arc{center:Point2,radius:f64,start_angle:f64,end_angle:f64}}
fn point_distance(a: Point2, b: Point2) -> f64 {
    ((a.u - b.u).powi(2) + (a.v - b.v).powi(2)).sqrt()
}
fn directed_angle_parameter(angle: f64, start: f64, end: f64) -> Option<f64> {
    let sweep = end - start;
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU {
        return None;
    }
    let displacement = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        -(start - angle).rem_euclid(std::f64::consts::TAU)
    };
    (displacement.abs() <= sweep.abs()).then_some(displacement / sweep)
}
fn line_arc_intersects(
    (start, end): (Point2, Point2),
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> bool {
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    let quadratic = direction.u * direction.u + direction.v * direction.v;
    if quadratic == 0.0 {
        return point_distance(start, center) == radius
            && directed_angle_parameter(
                (start.v - center.v).atan2(start.u - center.u),
                start_angle,
                end_angle,
            )
            .is_some();
    }
    let linear = 2.0 * (offset.u * direction.u + offset.v * direction.v);
    let constant = offset.u * offset.u + offset.v * offset.v - radius * radius;
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    let error =
        64.0 * f64::EPSILON * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
    if discriminant < -error {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-linear + signed_root) / (2.0 * quadratic);
        if !(0.0..=1.0).contains(&parameter) {
            return false;
        }
        let point = Point2::new(
            start.u + parameter * direction.u,
            start.v + parameter * direction.v,
        );
        directed_angle_parameter(
            (point.v - center.v).atan2(point.u - center.u),
            start_angle,
            end_angle,
        )
        .is_some()
    })
}
fn line_arc_intersection_points(
    (start, end): (Point2, Point2),
    arc: &ProfileBoundarySegment,
) -> Option<Vec<Point2>> {
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = arc
    else {
        return None;
    };
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    let scale = direction
        .u
        .abs()
        .max(direction.v.abs())
        .max(offset.u.abs())
        .max(offset.v.abs())
        .max(radius.abs());
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    let d = Point2::new(direction.u / scale, direction.v / scale);
    let o = Point2::new(offset.u / scale, offset.v / scale);
    let radius = radius / scale;
    let quadratic = d.u * d.u + d.v * d.v;
    if quadratic == 0.0 {
        return Some(Vec::new());
    }
    let linear = 2.0 * (o.u * d.u + o.v * d.v);
    let constant = o.u * o.u + o.v * o.v - radius * radius;
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    let error = 64.0 * f64::EPSILON * (linear * linear + (4.0 * quadratic * constant).abs());
    if discriminant < -error {
        return Some(Vec::new());
    }
    let root = discriminant.max(0.0).sqrt();
    let mut points = Vec::new();
    let q = -0.5 * (linear + root.copysign(linear));
    let parameters = if root == 0.0 {
        [-linear / (2.0 * quadratic); 2]
    } else {
        [q / quadratic, constant / q]
    };
    for parameter in parameters {
        if (0.0..=1.0).contains(&parameter) {
            let point = Point2::new(
                start.u + parameter * direction.u,
                start.v + parameter * direction.v,
            );
            if directed_angle_parameter(
                (point.v - center.v).atan2(point.u - center.u),
                *start_angle,
                *end_angle,
            )
            .is_some()
                && !points.contains(&point)
            {
                points.push(point);
            }
        }
    }
    Some(points)
}

#[test]fn copies_disagree(){let scale=1e-4;let start=Point2::new(-scale,2.*scale);let end=Point2::new(scale,2.*scale);let center=Point2::new(0.,0.);let got=line_arc_intersects((start,end),center,scale,0.,std::f64::consts::TAU);let points=line_arc_intersection_points((start,end),&ProfileBoundarySegment::Arc{center,radius:scale,start_angle:0.,end_angle:std::f64::consts::TAU}).unwrap();println!("F3D disjoint line/circle: bool={got} point_count={}",points.len());assert!(got);assert!(points.is_empty());}}
mod iges_closure {use super::*;fn insert_homogeneous_curve_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = alloc_filled(
        count.checked_add(1)?,
        [0.0; 4],
        "iges surface knot insertion",
    )
    .ok()?;
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}
fn homogeneous_bezier_spans(curve: &NurbsCurve) -> Option<Vec<HomogeneousBezierSpan>> {
    let degree = usize::try_from(curve.degree()).ok()?;
    let count = curve.control_points().len();
    if !knots_nondecreasing(curve.knots()) {
        return None;
    }
    let weights = curve.weights().map_or_else(
        || cadmpeg_core::decode::alloc_filled(count, 1.0, "iges_surface_closure_weights").ok(),
        Some,
    )?;
    if curve.control_points().iter().any(|point| {
        [point.x, point.y, point.z]
            .into_iter()
            .any(|value| !value.is_finite())
    }) || weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }
    let mut controls = curve
        .control_points()
        .iter()
        .zip(weights)
        .map(|(point, weight)| [weight * point.x, weight * point.y, weight * point.z, weight])
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }

    if degree == 0 {
        let mut spans = Vec::new();
        for (index, window) in curve.knots().windows(2).enumerate() {
            if window[0] < window[1] {
                spans.push(HomogeneousBezierSpan {
                    domain: [window[0], window[1]],
                    controls: vec![*controls.get(index)?],
                });
            }
        }
        return (!spans.is_empty()).then_some(spans);
    }

    let mut knots = curve.knots().to_vec();
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    let mut internal = knots[degree + 1..count]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_curve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(HomogeneousBezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}
fn homogeneous_curve_boundary_matches(
    first: &NurbsCurve,
    second: &NurbsCurve,
    range: [f64; 2],
    resolution: f64,
) -> Option<bool> {
    if !resolution.is_finite()
        || resolution < 0.0
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
    {
        return None;
    }
    let first_spans = homogeneous_bezier_spans(first)?;
    let second_spans = homogeneous_bezier_spans(second)?;
    if first.degree() != second.degree()
        || first.knots() != second.knots()
        || first_spans.len() != second_spans.len()
    {
        return None;
    }
    let degree = usize::try_from(first.degree()).ok()?;
    let product_degree = degree.checked_mul(2)?;
    let binomial = |n: usize, k: usize| {
        let k = k.min(n - k);
        (1..=k).fold(1.0, |value, factor| {
            value * (n - k + factor) as f64 / factor as f64
        })
    };
    for (first_span, second_span) in first_spans.iter().zip(second_spans) {
        if first_span.domain[1] <= range[0] || first_span.domain[0] >= range[1] {
            continue;
        }
        if first_span.domain != second_span.domain {
            return None;
        }
        let first_weight = first_span
            .controls
            .iter()
            .map(|control| control[3])
            .fold(f64::INFINITY, f64::min);
        let second_weight = second_span
            .controls
            .iter()
            .map(|control| control[3])
            .fold(f64::INFINITY, f64::min);
        let threshold = resolution * first_weight * second_weight / 3.0_f64.sqrt();
        if !threshold.is_finite() {
            return None;
        }
        for product_index in 0..=product_degree {
            let mut cross = [0.0; 3];
            let lower = product_index.saturating_sub(degree);
            let upper = product_index.min(degree);
            for first_index in lower..=upper {
                let second_index = product_index - first_index;
                let coefficient = binomial(degree, first_index) * binomial(degree, second_index)
                    / binomial(product_degree, product_index);
                for (axis, component) in cross.iter_mut().enumerate() {
                    *component += coefficient
                        * (first_span.controls[first_index][axis]
                            * second_span.controls[second_index][3]
                            - second_span.controls[second_index][axis]
                                * first_span.controls[first_index][3]);
                }
            }
            if cross
                .into_iter()
                .any(|component| !component.is_finite() || component.abs() > threshold)
            {
                return Some(false);
            }
        }
    }
    Some(true)
}

#[test]fn omitted_control(){let k=vec![0.,0.,0.,1.,1.,1.,2.,2.,2.];let p=(0..6).map(|i|Point3::new(i as f64,0.,0.)).collect::<Vec<_>>();let a=curve(k.clone(),p.clone(),None,2);let mut q=p;q[5].y=4.;let b=curve(k,q,None,2);let got=homogeneous_curve_boundary_matches(&a,&b,[0.,2.],0.);println!("IGES discontinuous boundary equality={got:?}; actual difference at t=1.5 is 1");assert_eq!(got,Some(true));}
#[test]fn small_weights(){let k=vec![0.,0.,1.,1.];let a=curve(k.clone(),vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],Some(vec![1e-200;2]),1);let b=curve(k,vec![Point3::new(0.,2.,0.),Point3::new(1.,2.,0.)],Some(vec![1e-200;2]),1);let got=homogeneous_curve_boundary_matches(&a,&b,[0.,1.],1e-6);println!("IGES offset parallel boundaries equality={got:?}; true separation=2 tolerance=1e-6");assert_eq!(got,Some(true));}}
mod freecad {use super::*;use cadmpeg_ir::transform::Transform;use cadmpeg_core::CodecError;const EPS_TOPOLOGY_TRANSFER_DEGENERATE:f64=1e-10;fn uniform_scale(transform: Transform) -> Result<f64, CodecError> {
    let columns = [
        Vector3::new(
            transform.rows()[0][0],
            transform.rows()[1][0],
            transform.rows()[2][0],
        ),
        Vector3::new(
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1],
        ),
        Vector3::new(
            transform.rows()[0][2],
            transform.rows()[1][2],
            transform.rows()[2][2],
        ),
    ];
    let scale = columns[0].norm();
    let tolerance = EPS_TOPOLOGY_TRANSFER_DEGENERATE * scale.max(1.0);
    if !scale.is_finite()
        || scale <= 0.0
        || columns
            .iter()
            .any(|column| (column.norm() - scale).abs() > tolerance)
        || columns[0].dot(columns[1]).abs() > tolerance
        || columns[0].dot(columns[2]).abs() > tolerance
        || columns[1].dot(columns[2]).abs() > tolerance
    {
        return Err(CodecError::Malformed(
            "B-rep location is not a finite similarity transform".into(),
        ));
    }
    Ok(scale)
}
fn transform_normalized_vector(transform: Transform, vector: Vector3) -> Option<Vector3> {
    let transformed = transform.apply_vector(vector)?;
    let magnitude = (transformed.x * transformed.x
        + transformed.y * transformed.y
        + transformed.z * transformed.z)
        .sqrt();
    if magnitude > 0.0 && magnitude.is_finite() {
        Some(Vector3::new(
            transformed.x / magnitude,
            transformed.y / magnitude,
            transformed.z / magnitude,
        ))
    } else {
        Some(transformed)
    }
}

#[test]fn small_shear(){let a=1e-6;let t=Transform::affine([[a,0.5*a,0.,0.],[0.,0.75f64.sqrt()*a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=uniform_scale(t);println!("FreeCAD angle-60-degree columns accepted as similarity: {got:?}");assert_eq!(got.unwrap(),a);}
#[test]fn transformed_normal(){for a in [1e200,1e-200]{let t=Transform::affine([[a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=transform_normalized_vector(t,Vector3::new(1.,0.,0.)).unwrap();println!("FreeCAD scale={a:e} transformed normal length={:e} expected=1",got.norm());assert_eq!(got.x,a);}}}
mod ir_surface {use super::*;use cadmpeg_ir::eval::{nurbs_surface_point,nurbs_surface_partials};pub fn nurbs_surface_parameter_near_point(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    const COARSE_GRID: usize = 8;
    const MAX_ITERATIONS: usize = 24;
    const MAX_LINE_SEARCH_STEPS: usize = 12;

    if !point.is_finite() {
        return None;
    }
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = [
        *surface.u_knots().get(u_degree)?,
        *surface.u_knots().get(u_count)?,
    ];
    let v_domain = [
        *surface.v_knots().get(v_degree)?,
        *surface.v_knots().get(v_count)?,
    ];
    if !u_domain[0].is_finite()
        || !u_domain[1].is_finite()
        || !v_domain[0].is_finite()
        || !v_domain[1].is_finite()
        || u_domain[0] >= u_domain[1]
        || v_domain[0] >= v_domain[1]
    {
        return None;
    }
    let mut parameters = match seed.filter(Point2::is_finite) {
        Some(seed) => Point2::new(
            seed.u.clamp(u_domain[0], u_domain[1]),
            seed.v.clamp(v_domain[0], v_domain[1]),
        ),
        None => {
            let mut best = None;
            for u_index in 0..=COARSE_GRID {
                let u = u_domain[0]
                    + (u_index as f64 / COARSE_GRID as f64) * (u_domain[1] - u_domain[0]);
                for v_index in 0..=COARSE_GRID {
                    let v = v_domain[0]
                        + (v_index as f64 / COARSE_GRID as f64) * (v_domain[1] - v_domain[0]);
                    let candidate = nurbs_surface_point(surface, u, v)?;
                    let distance = candidate.distance(point);
                    if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                        best = Some((Point2::new(u, v), distance));
                    }
                }
            }
            best?.0
        }
    };
    let squared_distance = |left: Point3| left.distance(point).powi(2);
    for _ in 0..MAX_ITERATIONS {
        let partials = nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let current_distance = squared_distance(partials.point);
        if current_distance <= f64::EPSILON {
            return Some(parameters);
        }
        let residual = Vector3::new(
            partials.point.x - point.x,
            partials.point.y - point.y,
            partials.point.z - point.z,
        );
        let du_squared = partials.du.dot(partials.du);
        let mixed = partials.du.dot(partials.dv);
        let dv_squared = partials.dv.dot(partials.dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
            break;
        }
        let du_residual = partials.du.dot(residual);
        let dv_residual = partials.dv.dot(residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let mut scale = 1.0;
        let mut accepted = false;
        for _ in 0..MAX_LINE_SEARCH_STEPS {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(u_domain[0], u_domain[1]),
                (parameters.v - scale * step.v).clamp(v_domain[0], v_domain[1]),
            );
            let candidate_point = nurbs_surface_point(surface, candidate.u, candidate.v)?;
            if squared_distance(candidate_point) < current_distance {
                parameters = candidate;
                accepted = true;
                break;
            }
            scale *= 0.5;
        }
        if !accepted {
            break;
        }
    }
    parameters.u.is_finite().then_some(parameters)
}

#[test]fn small_plane(){let a=1e-5;let axis=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);let sf=NurbsSurface::from_lanes(axis.clone(),axis,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,a,0.)],vec![Point3::new(a,0.,0.),Point3::new(a,a,0.)]],None),false).unwrap();let target=Point3::new(0.3*a,0.4*a,0.);let got=nurbs_surface_parameter_near_point(&sf,target,Some(Point2::new(0.,0.))).unwrap();println!("IR small plane UV=({},{}) expected=(0.3,0.4)",got.u,got.v);assert_eq!(got,Point2::new(0.,0.));}
#[test]fn cached_public_curve_residual(){let c=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);let got=cadmpeg_ir::eval::nurbs_curve_parameter_near_point(&c,Point3::new(0.,1e-200,0.),0.,0.);println!("IR cached public API: zero-tolerance witness for point off line={got:?}");assert_eq!(got,Some(0.));}}
mod step {use super::*;use std::collections::BTreeMap;
struct MeshProperties{area:f64,volume:f64,centroid:Point3}
struct Body{id:String} struct Model{bodies:Vec<Body>,tessellations:Vec<Mesh>} struct CadIr{model:Model}
struct Mesh{body:Option<String>,points:Vec<Point3>,faces:Vec<[u32;3]>}
impl Mesh{fn vertices(&self)->&[Point3]{&self.points}fn triangles(&self)->&[[u32;3]]{&self.faces}}
fn mesh_properties(ir: &CadIr) -> Option<MeshProperties> {
    let body = (ir.model.bodies.len() == 1).then(|| ir.model.bodies[0].id.clone())?;
    let meshes = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| mesh.body.as_ref() == Some(&body));
    let mut area = 0.0;
    let mut area_centroid = [0.0; 3];
    let mut signed_volume = 0.0;
    let mut volume_centroid = [0.0; 3];
    let mut triangles = 0usize;
    let mut watertight = true;
    let mut coordinate_scale = 0.0_f64;
    for mesh in meshes {
        let mut edge_uses = BTreeMap::<(u32, u32), usize>::new();
        for triangle in mesh.triangles() {
            let [a, b, c] = triangle.map(|index| mesh.vertices().get(index as usize).copied());
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                return None;
            };
            for [first, second] in [
                [triangle[0], triangle[1]],
                [triangle[1], triangle[2]],
                [triangle[2], triangle[0]],
            ] {
                *edge_uses
                    .entry((first.min(second), first.max(second)))
                    .or_default() += 1;
            }
            coordinate_scale = coordinate_scale
                .max(a.x.abs())
                .max(a.y.abs())
                .max(a.z.abs())
                .max(b.x.abs())
                .max(b.y.abs())
                .max(b.z.abs())
                .max(c.x.abs())
                .max(c.y.abs())
                .max(c.z.abs());
            let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
            let ac = [c.x - a.x, c.y - a.y, c.z - a.z];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let triangle_area = 0.5 * cross[0].hypot(cross[1]).hypot(cross[2]);
            area += triangle_area;
            for axis in 0..3 {
                area_centroid[axis] +=
                    triangle_area * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 3.0;
            }
            let tetra_volume = (a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0;
            signed_volume += tetra_volume;
            for axis in 0..3 {
                volume_centroid[axis] +=
                    tetra_volume * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 4.0;
            }
            triangles += 1;
        }
        watertight &= !edge_uses.is_empty() && edge_uses.values().all(|uses| *uses == 2);
    }
    if triangles == 0 || area == 0.0 {
        return None;
    }
    let volume_epsilon =
        f64::EPSILON * coordinate_scale.max(1.0).powi(3) * (triangles as f64).max(1.0);
    let centroid = if watertight && signed_volume.abs() > volume_epsilon {
        Point3::new(
            volume_centroid[0] / signed_volume,
            volume_centroid[1] / signed_volume,
            volume_centroid[2] / signed_volume,
        )
    } else {
        Point3::new(
            area_centroid[0] / area,
            area_centroid[1] / area,
            area_centroid[2] / area,
        )
    };
    Some(MeshProperties {
        area,
        volume: signed_volume.abs(),
        centroid,
    })
}

#[test]fn translation(){for d in [0.,1e6,1e9]{let points=[[0.,0.,0.],[2.,0.,0.],[0.,1.,0.],[0.,0.,1.]].map(|p|Point3::new(p[0]+d,p[1]+d,p[2]+d)).to_vec();let ir=CadIr{model:Model{bodies:vec![Body{id:"body".into()}],tessellations:vec![Mesh{body:Some("body".into()),points,faces:vec![[0,2,1],[0,1,3],[0,3,2],[1,2,3]]}]}};let got=mesh_properties(&ir).unwrap();println!("STEP tetra translation={d}: volume={} (expected 1/3); relative centroid=({},{},{}) expected=(0.5,0.25,0.25)",got.volume,got.centroid.x-d,got.centroid.y-d,got.centroid.z-d);if d==0.{assert!((got.volume-1./3.).abs()<1e-15)}else{assert!((got.centroid.x-d-0.5).abs()>1e-3)}}}}
mod nx {use super::*;use cadmpeg_core::decode::View;fn parse_jt9_geometric_transform_body(body: &[u8]) -> Option<(u8, u32, u16, [[f32; 4]; 4])> {
    let mut view = View::over_retained(body);
    let base_version = view.u16_le()?;
    let state_flags = view.u8()?;
    let field_inhibit_flags = view.u32_le()?;
    let version = view.u16_le()?;
    let stored_values_mask = view.u16_le()?;
    if base_version != 1 || version != 1 || state_flags & !0x0f != 0 || field_inhibit_flags != 0 {
        return None;
    }
    let mut matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for index in 0..16 {
        if stored_values_mask & (0x8000 >> index) == 0 {
            continue;
        }
        let value = view.f32_le()?;
        if !value.is_finite() {
            return None;
        }
        matrix[index / 4][index % 4] = value;
    }
    if !view.is_empty()
        || matrix[0][3] != 0.0
        || matrix[1][3] != 0.0
        || matrix[2][3] != 0.0
        || matrix[3][3] != 1.0
    {
        return None;
    }
    let rows = [&matrix[0][..3], &matrix[1][..3], &matrix[2][..3]];
    let lengths = rows.map(|row| row.iter().map(|value| value * value).sum::<f32>().sqrt());
    if lengths
        .iter()
        .any(|length| !length.is_finite() || *length == 0.0)
    {
        return None;
    }
    for first in 0..3 {
        for second in first + 1..3 {
            let dot = rows[first]
                .iter()
                .zip(rows[second])
                .map(|(left, right)| left * right)
                .sum::<f32>();
            if dot.abs() > 1.0e-5 * lengths[first] * lengths[second] {
                return None;
            }
        }
    }
    Some((state_flags, field_inhibit_flags, stored_values_mask, matrix))
}

#[test]fn f32_scale(){for a in [1f32,1e20,1e-30]{let mut b=Vec::new();b.extend(1u16.to_le_bytes());b.push(0);b.extend(0u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(0x8420u16.to_le_bytes());for _ in 0..3{b.extend(a.to_le_bytes())}let got=parse_jt9_geometric_transform_body(&b);println!("NX JT finite scale={a:e} accepted={}",got.is_some());assert_eq!(got.is_some(),a==1.);}}}
mod sld {use super::*;const EPS_CYLINDER_ANGLE:f64=1e-12;const MAX_PLANAR_TRIM_ARC_SEGMENTS:usize=4096;fn planar_arc_segments(span: f64, radius: f64, tolerance: f64) -> (usize, f64) {
    let cosine = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
    let maximum_span = 2.0 * cosine.acos();
    let requested = if maximum_span.is_finite() && maximum_span > EPS_CYLINDER_ANGLE {
        (span.abs() / maximum_span).ceil() as usize
    } else {
        MAX_PLANAR_TRIM_ARC_SEGMENTS
    };
    let segments = requested.clamp(1, MAX_PLANAR_TRIM_ARC_SEGMENTS);
    let actual_span = span.abs() / f64::from(segments as u32);
    (segments, radius * (1.0 - (actual_span / 2.0).cos()))
}

#[test]fn sagitta(){let span=1e-5;let radius=1e12;let tolerance=1e-9;let (n,claimed)=planar_arc_segments(span,radius,tolerance);let actual=2.*radius*(span/(4.*n as f64)).sin().powi(2);println!("SLD arc segments={n} claimed_error={claimed:e} stable_error={actual:e} tolerance={tolerance:e}");assert_eq!(claimed,0.);assert!(actual>tolerance);}}
mod creo {use super::*;const EPS_NEAR_ZERO:f64=1e-12;const EPS_SOLVER_SCALE:f64=1e-12;const EPS_DISCRIMINANT_SCALE:f64=1e-12;const EPS_SOLUTION_AGREEMENT:f64=1e-9;const EPS_DISTANCE_AGREEMENT:f64=1e-9;const TRIM_INTERSECTION_EPS:f64=1e-12;const TRIM_COORDINATE_EPS:f64=1e-9;fn quadratic_real_roots(
    quadratic: f64,
    linear: f64,
    constant: f64,
) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    if quadratic.abs() <= 1e-14 * scale {
        return if linear.abs() > 1e-14 * scale {
            vec![-constant / linear]
        } else {
            Vec::new()
        };
    }
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    if discriminant < -EPS_NEAR_ZERO * scale * scale {
        return Vec::new();
    }
    let root = if discriminant.abs() <= EPS_NEAR_ZERO * scale * scale {
        0.0
    } else {
        discriminant.sqrt()
    };
    let mut roots = vec![(-linear - root) / (2.0 * quadratic)];
    if root > EPS_NEAR_ZERO * scale {
        roots.push((-linear + root) / (2.0 * quadratic));
    }
    roots
}
fn quadratic_roots((quadratic, linear, constant): (f64, f64, f64)) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    let tolerance = EPS_SOLVER_SCALE * scale;
    let mut roots = if quadratic.abs() <= tolerance {
        if linear.abs() <= tolerance {
            Vec::new()
        } else {
            vec![-constant / linear]
        }
    } else {
        let discriminant = linear * linear - 4.0 * quadratic * constant;
        let discriminant_tolerance = EPS_DISCRIMINANT_SCALE
            * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
        if discriminant < -discriminant_tolerance {
            Vec::new()
        } else if discriminant.abs() <= discriminant_tolerance {
            vec![-linear / (2.0 * quadratic)]
        } else {
            let root = discriminant.sqrt();
            vec![
                (-linear - root) / (2.0 * quadratic),
                (-linear + root) / (2.0 * quadratic),
            ]
        }
    };
    roots.retain(|root| {
        root.is_finite()
            && (quadratic * root * root + linear * root + constant).abs()
                <= EPS_SOLUTION_AGREEMENT
                    * (quadratic * root * root)
                        .abs()
                        .max((linear * root).abs())
                        .max(constant.abs())
                        .max(1.0)
    });
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| approximately_equal(*first, *second));
    roots
}
fn approximately_equal(first: f64, second: f64) -> bool {
    let scale = first.abs().max(second.abs()).max(1.0);
    (first - second).abs() <= EPS_DISTANCE_AGREEMENT * scale
}
fn trim_circle_circle_intersection(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
) -> Option<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius).max(second_radius).max(1.0);
    if !distance.is_finite() || distance <= TRIM_INTERSECTION_EPS * scale {
        return None;
    }
    let external = first_radius + second_radius;
    let internal = (first_radius - second_radius).abs();
    if distance < internal - TRIM_COORDINATE_EPS * scale
        || distance > external + TRIM_COORDINATE_EPS * scale
    {
        return None;
    }
    let axial = (first_radius * first_radius - second_radius * second_radius + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius.mul_add(first_radius, -(axial * axial));
    let tolerance = TRIM_INTERSECTION_EPS * scale * scale;
    if !height_squared.is_finite() || height_squared.abs() > tolerance {
        return None;
    }
    let direction = [delta[0] / distance, delta[1] / distance];
    let coordinate = [
        first_center[0] + axial * direction[0],
        first_center[1] + axial * direction[1],
    ];
    coordinate
        .into_iter()
        .all(f64::is_finite)
        .then_some(coordinate)
}

#[test]fn coefficient_scale(){for scale in [1.,1e-10,1e-20]{let analytic=quadratic_real_roots(scale,0.,-scale);let sketch=quadratic_roots((scale,0.,-scale));println!("Creo {scale:e}*(x*x-1): analytic={analytic:?}, sketch={sketch:?}, expected=[-1,1]");if scale==1.{assert_eq!(analytic,vec![-1.,1.]);assert_eq!(sketch,vec![-1.,1.]);}else{assert_ne!(analytic,vec![-1.,1.]);assert_ne!(sketch,vec![-1.,1.]);}}}
#[test]fn two_circle_intersections(){let r=1e-6;let got=trim_circle_circle_intersection([0.,0.],r,[r,0.],r);println!("Creo two intersections: returned unique point={got:?}; true points=(0.5e-6,+/-sqrt(3)*0.5e-6)");assert_eq!(got,Some([0.5e-6,0.]));}}
mod asm {use super::*;const LEN_TO_MM:f64=10.;fn ellipse_to_nurbs(
    center: [f64; 3],
    normal: [f64; 3],
    major: [f64; 3],
    ratio: f64,
) -> Option<NurbsCurve> {
    let length = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt();
    (length.is_finite() && length > 0.0).then_some(())?;
    let minor_direction = [
        normal[1] * major[2] - normal[2] * major[1],
        normal[2] * major[0] - normal[0] * major[2],
        normal[0] * major[1] - normal[1] * major[0],
    ];
    let minor_length = (minor_direction[0] * minor_direction[0]
        + minor_direction[1] * minor_direction[1]
        + minor_direction[2] * minor_direction[2])
        .sqrt();
    (minor_length.is_finite() && minor_length > 0.0).then_some(())?;
    let minor_scale = ratio * length / minor_length;
    let minor = [
        minor_direction[0] * minor_scale,
        minor_direction[1] * minor_scale,
        minor_direction[2] * minor_scale,
    ];
    let at = |mj: f64, mn: f64| {
        Point3::new(
            (center[0] + mj * major[0] + mn * minor[0]) * LEN_TO_MM,
            (center[1] + mj * major[1] + mn * minor[1]) * LEN_TO_MM,
            (center[2] + mj * major[2] + mn * minor[2]) * LEN_TO_MM,
        )
    };
    // The rational quadratic circle states each pole with its own weight, so
    // the carrier reads rows and there is no pole lane and weight lane to pair.
    let corner = cadmpeg_ir::scalar::NonZeroReal::new(std::f64::consts::FRAC_1_SQRT_2)?;
    let full = cadmpeg_ir::scalar::NonZeroReal::new(1.0)?;
    let pole = |point, weight| cadmpeg_ir::geometry::nurbs::WeightedPole3 { point, weight };
    NurbsCurve::new(
        2,
        vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational {
            points: vec![
                pole(at(1.0, 0.0), full),
                pole(at(1.0, 1.0), corner),
                pole(at(0.0, 1.0), full),
                pole(at(-1.0, 1.0), corner),
                pole(at(-1.0, 0.0), full),
                pole(at(-1.0, -1.0), corner),
                pole(at(0.0, -1.0), full),
                pole(at(1.0, -1.0), corner),
                pole(at(1.0, 0.0), full),
            ],
        },
        false,
    )
    .ok()
}

#[test]fn radii_scale(){for radius in [1.,1e200,1e-200]{let got=ellipse_to_nurbs([0.,0.,0.],[0.,0.,1.],[radius,0.,0.],0.5);println!("ASM finite ellipse radius={radius:e} accepted={}",got.is_some());assert_eq!(got.is_some(),radius==1.);}}}
mod f3d_speed {use super::*;use cadmpeg_ir::geometry::pcurve::PcurveNurbs;fn nurbs_speed_bound(curve: &PcurveNurbs) -> Option<f64> {
    let degree = curve.degree();
    let degree_usize = degree as usize;
    let knots = curve.knots();
    let control_points = curve.control_points();
    let weights = curve.weights();
    let count = control_points.len();
    let weights = match weights {
        Some(weights) => weights,
        None => alloc_filled(count, 1.0, "f3d_nurbs_weights").ok()?,
    };
    if knots.iter().any(|value| !value.is_finite())
        || !knots_nondecreasing(knots)
        || control_points
            .iter()
            .zip(&weights)
            .any(|(point, weight)| !point.is_finite() || !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }
    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum_numerator = control_points
        .iter()
        .zip(&weights)
        .map(|(point, weight)| weight * point.u.hypot(point.v))
        .fold(0.0_f64, f64::max);
    let mut numerator_speed = 0.0_f64;
    let mut weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = knots[index + degree_usize + 1] - knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(degree) / denominator;
        let first = Point2::new(
            weights[index] * control_points[index].u,
            weights[index] * control_points[index].v,
        );
        let second = Point2::new(
            weights[index + 1] * control_points[index + 1].u,
            weights[index + 1] * control_points[index + 1].v,
        );
        numerator_speed =
            numerator_speed.max(factor * (second.u - first.u).hypot(second.v - first.v));
        weight_speed = weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let bound = numerator_speed / minimum_weight
        + maximum_numerator * weight_speed / minimum_weight.powi(2);
    bound.is_finite().then_some(bound)
}

#[test]fn weights(){for w in [1.,1e-200]{let c=PcurveNurbs::from_lanes(1,vec![0.,0.,1.,1.],vec![Point2::new(0.,0.),Point2::new(1.,0.)],Some(vec![w;2]),false).unwrap();let got=nurbs_speed_bound(&c);println!("F3D identical pcurve weights={w:e}, speed bound={got:?}");assert_eq!(got.is_some(),w==1.);}}}
mod ir_phase {use super::*;use cadmpeg_ir::eval::nurbs_curve_parameter_domain;pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: f64) -> Option<f64> {
    let [lower, upper] = nurbs_curve_parameter_domain(curve)?;
    if !parameter.is_finite() {
        return None;
    }
    if curve.periodic() {
        let period = upper - lower;
        Some(lower + (parameter - lower).rem_euclid(period))
    } else {
        (lower..=upper).contains(&parameter).then_some(parameter)
    }
}

#[test]fn finite_phase(){let c=NurbsCurve::from_lanes(1,vec![-1e308,-1e308,-9e307,-9e307],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,true).unwrap();let got=map_nurbs_curve_parameter(&c,1e308).unwrap();println!("IR public periodic map finite parameter yields {got}");assert!(got.is_nan());}}
mod ir_witness {use super::*;use cadmpeg_ir::eval::nurbs_curve_point;#[derive(Debug,PartialEq)]enum BoundaryWitness{Invalid,NoMatch,Found(f64)}fn nearest_boundary_witness<F>(
    boundaries: &[f64],
    seed: f64,
    tolerance: f64,
    mut distance: F,
) -> BoundaryWitness
where
    F: FnMut(f64) -> Option<f64>,
{
    let mut previous_boundary = None;
    let mut nearest = None;
    let mut nearest_seed_distance = f64::INFINITY;
    for &parameter in boundaries {
        if previous_boundary == Some(parameter) {
            continue;
        }
        previous_boundary = Some(parameter);
        let seed_distance = (parameter - seed).abs();
        if seed_distance >= nearest_seed_distance {
            continue;
        }
        let Some(candidate_distance) = distance(parameter) else {
            return BoundaryWitness::Invalid;
        };
        if candidate_distance <= tolerance {
            nearest = Some(parameter);
            nearest_seed_distance = seed_distance;
        }
    }
    nearest.map_or(BoundaryWitness::NoMatch, BoundaryWitness::Found)
}

#[test]fn false_zero_residual(){let curve=curve(vec![0.,0.,1.,1.],vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],None,1);let weights=vec![1.,1.];let point=Point3::new(0.,1e-200,0.);
let distance = |parameter| {
        let position = nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.control_points(),
            Some(weights.as_ref()),
            parameter,
        )?;
        Some(
            ((position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2))
            .sqrt(),
        )
    };

let got=nearest_boundary_witness(&[0.,1.],0.,0.,distance);println!("IR current boundary witness with off-curve distance=1e-200 and tolerance=0: {got:?}");assert_eq!(got,BoundaryWitness::Found(0.));}}
mod rhino_mean {#[derive(Debug,Clone,Copy)]struct Point3([f64;3]);struct Vertex{point:Point3}struct Accum{point_sum:[f64;3],point_count:usize,vertex:Vertex}
#[test]fn finite_endpoint_average(){let mut v=Accum{point_sum:[0.;3],point_count:0,vertex:Vertex{point:Point3([0.;3])}};for point in [Point3([1e308,0.,0.]);2]{let vertex=&mut v;
vertex.point_sum[0] += point.0[0];
            vertex.point_sum[1] += point.0[1];
            vertex.point_sum[2] += point.0[2];
            vertex.point_count += 1;
}let accumulated=&mut v;let count=accumulated.point_count as f64;
accumulated.vertex.point = Point3([
                    accumulated.point_sum[0] / count,
                    accumulated.point_sum[1] / count,
                    accumulated.point_sum[2] / count,
                ]);
println!("Rhino two identical finite endpoints: mean={:?} expected=[1e308,0,0]",v.vertex.point);assert!(v.vertex.point.0[0].is_infinite());}}
mod ir_helix {use super::*;const EPS_HELIX_SURFACE_RADIUS_RELATIVE:f64=1e-9;fn admit(major:Vector3,minor:Vector3)->Result<(),&'static str>{let major_length = (major.x.powi(2) + major.y.powi(2) + major.z.powi(2)).sqrt();
        let minor_length = (minor.x.powi(2) + minor.y.powi(2) + minor.z.powi(2)).sqrt();
        if !(major_length > 0.0
            && (major_length - minor_length).abs()
                <= EPS_HELIX_SURFACE_RADIUS_RELATIVE * major_length.max(1.0))
        {
            return Err("helix surface path major and minor must define a circular path");
        }

        Ok(())}
#[test]fn finite_circular_frame(){for r in [1.,1e200,1e-200]{let got=admit(Vector3::new(r,0.,0.),Vector3::new(0.,r,0.));println!("IR circular helix radius={r:e} accepted={}",got.is_ok());assert_eq!(got.is_ok(),r==1.);}}}
