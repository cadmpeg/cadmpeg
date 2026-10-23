// SPDX-License-Identifier: Apache-2.0
//! Extremum search of a smooth function along an interval.
//!
//! The extremes of `f` on `[a, b]` lie at the ends or at roots of `f'`. The
//! search samples `f'` at the Gauss–Kronrod nodes of each panel, brackets the
//! sign changes, and refines each bracket by bisection. Two roots between
//! adjacent samples can be missed; this is a stated sampling fact of the
//! measurement contract.

use super::gauss_kronrod::NODES;
use crate::failure::Measure;
use crate::tolerance::EXTREMUM_PARAMETER_RESOLUTION;

/// Bisection steps of one root refinement.
const MAX_EXTREMUM_REFINEMENT_STEPS: usize = 64;

/// Equal panels each span between breaks is split into before sampling.
const PANELS_PER_SPAN: u32 = 8;

/// The extremes of a function on an interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Extrema {
    pub(crate) min: f64,
    pub(crate) max: f64,
    /// `|f'|` times the final bracket width at each refined root.
    pub(crate) error: f64,
}

/// Sample abscissae of one panel: its ends and the Kronrod nodes.
fn samples(a: f64, b: f64) -> impl Iterator<Item = f64> {
    const XGK: [f64; 7] = [
        0.991_455_371_120_812_6,
        0.949_107_912_342_758_5,
        0.864_864_423_359_769_1,
        0.741_531_185_599_394_5,
        0.586_087_235_467_691_1,
        0.405_845_151_377_397_2,
        0.207_784_955_007_898_48,
    ];
    let center = a.midpoint(b);
    let half = (b - a) / 2.0;
    std::iter::once(a)
        .chain(XGK.into_iter().map(move |x| center - half * x))
        .chain(std::iter::once(center))
        .chain(XGK.into_iter().rev().map(move |x| center + half * x))
        .chain(std::iter::once(b))
}

/// The extremes of `f` on the interval between `start` and `end`, with `f`
/// returning the value and the derivative. `charge` pays one unit per
/// evaluation.
pub(crate) fn extrema(
    charge: &mut dyn FnMut(usize) -> Measure<()>,
    f: &mut dyn FnMut(f64) -> Measure<(f64, f64)>,
    start: f64,
    end: f64,
    breaks: &[f64],
) -> Measure<Extrema> {
    let (low, high) = if end < start {
        (end, start)
    } else {
        (start, end)
    };
    let mut cuts: Vec<f64> = std::iter::once(low)
        .chain(breaks.iter().copied().filter(|t| *t > low && *t < high))
        .chain(std::iter::once(high))
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    for span in cuts.windows(2) {
        let width = (span[1] - span[0]) / f64::from(PANELS_PER_SPAN);
        for panel in 0..PANELS_PER_SPAN {
            let a = span[0] + width * f64::from(panel);
            let b = if panel + 1 == PANELS_PER_SPAN {
                span[1]
            } else {
                a + width
            };
            charge(NODES + 2)?;
            for t in samples(a, b) {
                let (value, slope) = f(t)?;
                points.push((t, value, slope));
            }
        }
    }
    if points.is_empty() {
        charge(1)?;
        let (value, _) = f(low)?;
        return Ok(Extrema {
            min: value,
            max: value,
            error: 0.0,
        });
    }
    points.sort_by(|left, right| left.0.total_cmp(&right.0));
    points.dedup_by(|right, left| right.0 == left.0);
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &(_, value, _) in &points {
        min = min.min(value);
        max = max.max(value);
    }
    let resolution = EXTREMUM_PARAMETER_RESOLUTION * (high - low);
    let mut error: f64 = 0.0;
    for pair in points.windows(2) {
        let (mut a, _, mut slope_a) = pair[0];
        let (mut b, _, slope_b) = pair[1];
        if slope_a == 0.0 || slope_b == 0.0 || slope_a.signum() == slope_b.signum() {
            continue;
        }
        let mut steps = 0;
        while b - a > resolution && steps < MAX_EXTREMUM_REFINEMENT_STEPS {
            let middle = a.midpoint(b);
            charge(1)?;
            let (_, slope) = f(middle)?;
            if slope == 0.0 {
                a = middle;
                b = middle;
                break;
            }
            if slope.signum() == slope_a.signum() {
                a = middle;
                slope_a = slope;
            } else {
                b = middle;
            }
            steps += 1;
        }
        let root = a.midpoint(b);
        charge(1)?;
        let (value, slope) = f(root)?;
        min = min.min(value);
        max = max.max(value);
        error = error.max(slope.abs().max(slope_a.abs()) * (b - a));
    }
    Ok(Extrema { min, max, error })
}

/// A root of `g` between `a` and `b`, where `g(a)` and `g(b)` differ in
/// sign, refined by bisection until the bracket is below the parameter
/// resolution of `span`.
pub(crate) fn root(
    charge: &mut dyn FnMut(usize) -> Measure<()>,
    g: &mut dyn FnMut(f64) -> Measure<f64>,
    [mut a, mut b]: [f64; 2],
    span: f64,
) -> Measure<f64> {
    charge(1)?;
    let low = g(a)?;
    let resolution = EXTREMUM_PARAMETER_RESOLUTION * span.abs();
    let mut steps = 0;
    while (b - a).abs() > resolution && steps < MAX_EXTREMUM_REFINEMENT_STEPS {
        let middle = a.midpoint(b);
        charge(1)?;
        let value = g(middle)?;
        if value == 0.0 {
            return Ok(middle);
        }
        if value.signum() == low.signum() {
            a = middle;
        } else {
            b = middle;
        }
        steps += 1;
    }
    Ok(a.midpoint(b))
}

#[cfg(test)]
mod tests {
    use super::extrema;
    use crate::failure::Measure;

    fn free() -> impl FnMut(usize) -> Measure<()> {
        |_| Ok(())
    }

    #[test]
    fn a_cubic_with_two_close_interior_extrema_is_found_within_resolution() {
        // f(t) = (t − 0.5)³ − 1e-4·(t − 0.5) has extremes at 0.5 ± 0.00577…
        let c = 1.0e-4_f64;
        let mut f = |t: f64| -> Measure<(f64, f64)> {
            let x = t - 0.5;
            Ok((x * x * x - c * x, 3.0 * x * x - c))
        };
        let found = extrema(&mut free(), &mut f, 0.45, 0.55, &[]).unwrap();
        let root = (c / 3.0).sqrt();
        let local_max = -root * root * root + c * root;
        let at_end = 0.05_f64.powi(3) - c * 0.05;
        assert!((found.max - at_end.max(local_max)).abs() < 1.0e-15);
        let mut g = |t: f64| -> Measure<(f64, f64)> {
            let x = t - 0.5;
            Ok((-(x * x * x - c * x), -(3.0 * x * x - c)))
        };
        // On [0.49, 0.51] both ends give g = 0 and the interior maximum at
        // x = +root is positive.
        let interior = extrema(&mut free(), &mut g, 0.49, 0.51, &[]).unwrap();
        assert!((interior.max - local_max.abs()).abs() <= 1.0e-15 + interior.error);
    }

    #[test]
    fn a_cosine_extreme_inside_the_interval_is_refined() {
        let mut f = |t: f64| -> Measure<(f64, f64)> { Ok((t.cos(), -t.sin())) };
        let found = extrema(&mut free(), &mut f, -1.0, 4.0, &[]).unwrap();
        assert!((found.max - 1.0).abs() < 1.0e-15);
        assert!((found.min + 1.0).abs() < 1.0e-15);
    }
}
