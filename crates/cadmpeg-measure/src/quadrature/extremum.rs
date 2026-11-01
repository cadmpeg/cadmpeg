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

/// The extremes of each of the `N` coordinates of `f` between `start` and
/// `end`; `f` returns the coordinates and their derivatives. A bracket is
/// bisected until it is below the parameter resolution or `|f'|` times its
/// width is within `precision`. `charge` pays one unit per evaluation.
pub(crate) fn extrema<const N: usize>(
    charge: &mut dyn FnMut(usize) -> Measure<()>,
    f: &mut dyn FnMut(f64) -> Measure<([f64; N], [f64; N])>,
    start: f64,
    end: f64,
    breaks: &[f64],
    precision: [f64; N],
) -> Measure<[Extrema; N]> {
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
    let mut points: Vec<(f64, [f64; N], [f64; N])> = Vec::new();
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
        return Ok(value.map(|value| Extrema {
            min: value,
            max: value,
            error: 0.0,
        }));
    }
    points.sort_by(|left, right| left.0.total_cmp(&right.0));
    points.dedup_by(|right, left| right.0 == left.0);
    let resolution = EXTREMUM_PARAMETER_RESOLUTION * (high - low);
    let mut found = [Extrema {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        error: 0.0,
    }; N];
    for (axis, extrema) in found.iter_mut().enumerate() {
        for (_, value, _) in &points {
            extrema.min = extrema.min.min(value[axis]);
            extrema.max = extrema.max.max(value[axis]);
        }
        for pair in points.windows(2) {
            let (mut a, _, slope_a) = pair[0];
            let (mut b, _, slope_b) = pair[1];
            let (mut slope_a, mut slope_b) = (slope_a[axis], slope_b[axis]);
            if slope_a == 0.0 || slope_b == 0.0 || slope_a.signum() == slope_b.signum() {
                continue;
            }
            let bound = |a: f64, b: f64, slope_a: f64, slope_b: f64| {
                slope_a.abs().max(slope_b.abs()) * (b - a)
            };
            if bound(a, b, slope_a, slope_b) <= precision[axis] {
                extrema.error = extrema.error.max(bound(a, b, slope_a, slope_b));
                continue;
            }
            let mut steps = 0;
            while b - a > resolution
                && bound(a, b, slope_a, slope_b) > precision[axis]
                && steps < MAX_EXTREMUM_REFINEMENT_STEPS
            {
                let middle = a.midpoint(b);
                charge(1)?;
                let slope = f(middle)?.1[axis];
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
                    slope_b = slope;
                }
                steps += 1;
            }
            let root = a.midpoint(b);
            charge(1)?;
            let (value, slope) = f(root)?;
            extrema.min = extrema.min.min(value[axis]);
            extrema.max = extrema.max.max(value[axis]);
            extrema.error = extrema
                .error
                .max(slope[axis].abs().max(slope_a.abs()).max(slope_b.abs()) * (b - a));
        }
    }
    Ok(found)
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
        let mut f = |t: f64| -> Measure<([f64; 1], [f64; 1])> {
            let x = t - 0.5;
            Ok(([x * x * x - c * x], [3.0 * x * x - c]))
        };
        let [found] = extrema(&mut free(), &mut f, 0.45, 0.55, &[], [0.0]).unwrap();
        let root = (c / 3.0).sqrt();
        let local_max = -root * root * root + c * root;
        let at_end = 0.05_f64.powi(3) - c * 0.05;
        assert!((found.max - at_end.max(local_max)).abs() < 1.0e-15);
        let mut g = |t: f64| -> Measure<([f64; 1], [f64; 1])> {
            let x = t - 0.5;
            Ok(([-(x * x * x - c * x)], [-(3.0 * x * x - c)]))
        };
        // On [0.49, 0.51] both ends give g = 0 and the interior maximum at
        // x = +root is positive.
        let [interior] = extrema(&mut free(), &mut g, 0.49, 0.51, &[], [0.0]).unwrap();
        assert!((interior.max - local_max.abs()).abs() <= 1.0e-15 + interior.error);
    }

    #[test]
    fn a_bracket_within_the_values_precision_is_not_refined() {
        // A value known to 1e-12 with slope noise of 1e-11 changes slope sign
        // between most samples, about 0.007 apart, so no bracket can move its
        // extremes by more than the precision. A second coordinate shares the
        // samples.
        let mut f = |t: f64| -> Measure<([f64; 2], [f64; 2])> {
            let noise = (1.0e4 * t).sin();
            Ok(([1.0e-15 * noise, t], [1.0e-11 * noise, 1.0]))
        };
        let mut charged = 0;
        let mut count = |units: usize| -> Measure<()> {
            charged += units;
            Ok(())
        };
        let [noisy, rising] = extrema(&mut count, &mut f, 0.0, 1.0, &[], [1.0e-12, 0.0]).unwrap();
        let sampled = usize::try_from(super::PANELS_PER_SPAN).expect("panels") * (super::NODES + 2);
        assert_eq!(charged, sampled);
        assert!(noisy.max - noisy.min <= 2.0e-15 && noisy.error <= 1.0e-12);
        assert_eq!([rising.min, rising.max], [0.0, 1.0]);
        let mut refined = 0;
        let mut count = |units: usize| -> Measure<()> {
            refined += units;
            Ok(())
        };
        extrema(&mut count, &mut f, 0.0, 1.0, &[], [0.0, 0.0]).unwrap();
        assert!(refined > sampled);
    }

    #[test]
    fn a_cosine_extreme_inside_the_interval_is_refined() {
        let mut f = |t: f64| -> Measure<([f64; 1], [f64; 1])> { Ok(([t.cos()], [-t.sin()])) };
        let [found] = extrema(&mut free(), &mut f, -1.0, 4.0, &[], [0.0]).unwrap();
        assert!((found.max - 1.0).abs() < 1.0e-15);
        assert!((found.min + 1.0).abs() < 1.0e-15);
    }
}
