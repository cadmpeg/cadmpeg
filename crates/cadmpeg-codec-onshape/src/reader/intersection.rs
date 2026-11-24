// SPDX-License-Identifier: Apache-2.0
//! Closed-form real roots for analytic planar curve intersections. Coefficients
//! come from stored carriers, never a tessellation or a collection of samples.
use super::Mapping;

/// Arithmetic roundoff units admitted before a root or discriminant is ambiguous.
const ROUNDOFF_FACTOR: f64 = 64.0;

pub(super) fn roots(coefficients: &[f64]) -> Mapping<Vec<f64>> {
    if coefficients.iter().any(|v| !v.is_finite()) {
        return Err("the analytic intersection exceeds the finite range".into());
    }
    let Some((&leading, rest)) = coefficients.split_last() else {
        return Err("the analytic intersection carriers coincide".into());
    };
    if leading == 0.0 {
        return roots(rest);
    }
    let normalized: Vec<_> = rest.iter().map(|v| v / leading).collect();
    if normalized.iter().any(|n| !n.is_finite()) {
        return Err("the analytic intersection normalization exceeds the finite range".into());
    }
    let result = match normalized.as_slice() {
        [] => Vec::new(),
        [c0] => vec![-c0],
        [c0, c1] => quadratic(*c1, *c0)?,
        [c0, c1, c2] => cubic(*c2, *c1, *c0)?,
        [c0, c1, c2, c3] => {
            let depressed_p = c2 - 3.0 * c3 * c3 / 8.0;
            let depressed_q = c3 * c3 * c3 / 8.0 - c3 * c2 / 2.0 + c1;
            let depressed_r = -3.0 * c3.powi(4) / 256.0 + c3 * c3 * c2 / 16.0 - c3 * c1 / 4.0 + c0;
            let mut result = Vec::new();
            if depressed_q == 0.0 {
                for z in quadratic(depressed_p, depressed_r)? {
                    if z != 0.0
                        && z.abs()
                            <= ROUNDOFF_FACTOR
                                * f64::EPSILON
                                * (depressed_p.abs() + depressed_r.abs()).max(1.0)
                    {
                        return Err(
                            "the analytic intersection is tangent within arithmetic roundoff"
                                .into(),
                        );
                    }
                    if z >= 0.0 {
                        result.extend([z.sqrt(), -z.sqrt()]);
                    }
                }
            } else {
                // Ferrari factorization into two real quadratics. The largest
                // positive resolvent root avoids cancellation in sqrt(z).
                let z = cubic(
                    2.0 * depressed_p,
                    depressed_p * depressed_p - 4.0 * depressed_r,
                    -depressed_q * depressed_q,
                )?
                .into_iter()
                .filter(|z| *z > 0.0)
                .max_by(f64::total_cmp)
                .ok_or("the analytic intersection resolvent is unresolved")?;
                let u = z.sqrt();
                result.extend(quadratic(u, (depressed_p + z - depressed_q / u) / 2.0)?);
                result.extend(quadratic(-u, (depressed_p + z + depressed_q / u) / 2.0)?);
            }
            result.into_iter().map(|x| x - c3 / 4.0).collect()
        }
        _ => return Err("the analytic intersection degree is not supported yet".into()),
    };
    if result.iter().any(|v| !v.is_finite()) {
        return Err("the analytic intersection roots are unresolved".into());
    }
    for root in &result {
        let residual = coefficients.iter().rev().fold(0.0, |sum, c| sum * root + c);
        let magnitude = coefficients
            .iter()
            .rev()
            .fold(0.0, |sum, c| sum * root.abs() + c.abs());
        if !residual.is_finite()
            || !magnitude.is_finite()
            || residual.abs() > ROUNDOFF_FACTOR * f64::EPSILON * magnitude
        {
            return Err("the analytic intersection root residual is unresolved".into());
        }
    }
    Ok(result)
}
fn quadratic(b: f64, c: f64) -> Mapping<Vec<f64>> {
    let discriminant = b * b - 4.0 * c;
    let uncertainty = ROUNDOFF_FACTOR * f64::EPSILON * (b * b + 4.0 * c.abs());
    if discriminant != 0.0 && discriminant.abs() <= uncertainty {
        return Err("the analytic intersection is tangent within arithmetic roundoff".into());
    }
    if discriminant < 0.0 {
        return Ok(Vec::new());
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0.0 {
        Ok(vec![0.0])
    } else {
        Ok(vec![q, c / q])
    }
}
fn cubic(c2: f64, c1: f64, c0: f64) -> Mapping<Vec<f64>> {
    let depressed_p = c1 - c2 * c2 / 3.0;
    let depressed_q = 2.0 * c2 * c2 * c2 / 27.0 - c2 * c1 / 3.0 + c0;
    let discriminant =
        depressed_q * depressed_q / 4.0 + depressed_p * depressed_p * depressed_p / 27.0;
    let uncertainty = ROUNDOFF_FACTOR
        * f64::EPSILON
        * (depressed_q * depressed_q / 4.0
            + (depressed_p * depressed_p * depressed_p / 27.0).abs());
    if discriminant != 0.0 && discriminant.abs() <= uncertainty {
        return Err(
            "the analytic intersection resolvent is ambiguous within arithmetic roundoff".into(),
        );
    }
    if discriminant >= 0.0 {
        let u = (-depressed_q / 2.0 + discriminant.sqrt()).cbrt();
        let v = (-depressed_q / 2.0 - discriminant.sqrt()).cbrt();
        let mut result = vec![u + v - c2 / 3.0];
        if discriminant == 0.0 {
            result.push(-(u + v) / 2.0 - c2 / 3.0);
        }
        Ok(result)
    } else {
        let radius = 2.0 * (-depressed_p / 3.0).sqrt();
        let phase = ((3.0 * depressed_q / (2.0 * depressed_p)) * (-3.0 / depressed_p).sqrt())
            .clamp(-1.0, 1.0)
            .acos()
            / 3.0;
        Ok((0..3)
            .map(|i| radius * (phase - std::f64::consts::TAU * f64::from(i) / 3.0).cos() - c2 / 3.0)
            .collect())
    }
}
