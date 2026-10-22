// SPDX-License-Identifier: Apache-2.0
//! The Gauss–Kronrod 7/15 rule and global adaptive subdivision.
//!
//! The interval estimate is `|K15 − G7|` per component. The subdivision
//! bisects the interval with the largest ratio of estimate to target until
//! the summed estimate meets the target, the interval count reaches its cap,
//! or the work slice runs out. Values are summed left to right with Neumaier
//! compensation, so equal inputs give equal bits.

use crate::failure::Measure;
use crate::tolerance::{QUADRATURE_ACCEPTANCE_RELATIVE, QUADRATURE_RELATIVE_TARGET};

/// Kronrod abscissae of QUADPACK `qk15`, as the nearest doubles; the last is
/// the centre.
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_5,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_48,
    0.0,
];

/// Kronrod weights matching [`XGK`].
const WGK: [f64; 8] = [
    0.022_935_322_010_529_224,
    0.063_092_092_629_978_56,
    0.104_790_010_322_250_19,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_42,
    0.204_432_940_075_298_89,
    0.209_482_141_084_727_82,
];

/// Gauss weights at `XGK[1]`, `XGK[3]`, `XGK[5]` and the centre.
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_64,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

/// Integrand evaluations of one rule application.
pub(crate) const NODES: usize = 15;

/// Intervals of one integral.
const MAX_QUADRATURE_INTERVALS: usize = 1 << 12;

/// The target of one integral: per component, the scale `scale^d` the
/// relative target and the acceptance limit apply to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Target<const N: usize> {
    pub(crate) scale: [f64; N],
}

/// The result of one integral.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Integral<const N: usize> {
    pub(crate) value: [f64; N],
    pub(crate) error: [f64; N],
    /// Whether every component's estimate is within its acceptance limit.
    pub(crate) converged: bool,
}

#[derive(Debug, Clone, Copy)]
struct Panel<const N: usize> {
    a: f64,
    b: f64,
    value: [f64; N],
    error: [f64; N],
}

fn rule<const N: usize>(
    f: &mut dyn FnMut(f64) -> Measure<[f64; N]>,
    a: f64,
    b: f64,
) -> Measure<Panel<N>> {
    let center = a.midpoint(b);
    let half = (b - a) / 2.0;
    let mid = f(center)?;
    let mut kronrod: [f64; N] = std::array::from_fn(|k| WGK[7] * mid[k]);
    let mut gauss: [f64; N] = std::array::from_fn(|k| WG[3] * mid[k]);
    for (index, (&x, &weight)) in XGK.iter().zip(WGK.iter()).take(7).enumerate() {
        let left = f(center - half * x)?;
        let right = f(center + half * x)?;
        for k in 0..N {
            let sum = left[k] + right[k];
            kronrod[k] += weight * sum;
            if index % 2 == 1 {
                gauss[k] += WG[index / 2] * sum;
            }
        }
    }
    let value = kronrod.map(|value| value * half);
    let error = std::array::from_fn(|k| ((kronrod[k] - gauss[k]) * half).abs());
    Ok(Panel { a, b, value, error })
}

/// Neumaier-compensated sum in iteration order.
fn compensated(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for value in values {
        let next = sum + value;
        if sum.abs() >= value.abs() {
            compensation += (sum - next) + value;
        } else {
            compensation += (value - next) + sum;
        }
        sum = next;
    }
    sum + compensation
}

/// Integrate `f` from `start` to `end` over the panels between consecutive
/// `breaks` inside the interval. `charge` pays one unit per integrand node.
/// An integrand failure ends the integral with that failure.
pub(crate) fn integrate<const N: usize>(
    charge: &mut dyn FnMut(usize) -> Measure<()>,
    f: &mut dyn FnMut(f64) -> Measure<[f64; N]>,
    start: f64,
    end: f64,
    breaks: &[f64],
    target: Target<N>,
) -> Measure<Integral<N>> {
    let (low, high, sign) = if end < start {
        (end, start, -1.0)
    } else {
        (start, end, 1.0)
    };
    if low == high {
        return Ok(Integral {
            value: [0.0; N],
            error: [0.0; N],
            converged: true,
        });
    }
    let mut cuts: Vec<f64> = std::iter::once(low)
        .chain(breaks.iter().copied().filter(|t| *t > low && *t < high))
        .chain(std::iter::once(high))
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut panels = Vec::with_capacity(cuts.len());
    for pair in cuts.windows(2) {
        charge(NODES)?;
        panels.push(rule(f, pair[0], pair[1])?);
    }
    loop {
        let value: [f64; N] =
            std::array::from_fn(|k| compensated(panels.iter().map(|panel| panel.value[k])));
        let error: [f64; N] =
            std::array::from_fn(|k| panels.iter().map(|panel| panel.error[k]).sum());
        let goal: [f64; N] = std::array::from_fn(|k| {
            QUADRATURE_RELATIVE_TARGET * value[k].abs().max(target.scale[k])
        });
        let met = (0..N).all(|k| error[k] <= goal[k]);
        if met || panels.len() >= MAX_QUADRATURE_INTERVALS {
            return Ok(finish(&panels, target, sign));
        }
        let ratio = |panel: &Panel<N>| {
            (0..N)
                .map(|k| {
                    if goal[k] > 0.0 {
                        panel.error[k] / goal[k]
                    } else {
                        panel.error[k]
                    }
                })
                .fold(0.0, f64::max)
        };
        let Some(worst) = panels
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                ratio(left)
                    .total_cmp(&ratio(right))
                    .then(right.a.total_cmp(&left.a))
            })
            .map(|(index, _)| index)
        else {
            return Ok(finish(&panels, target, sign));
        };
        let panel = panels[worst];
        let middle = panel.a.midpoint(panel.b);
        if middle <= panel.a || middle >= panel.b {
            return Ok(finish(&panels, target, sign));
        }
        if charge(2 * NODES).is_err() {
            return Ok(finish(&panels, target, sign));
        }
        let left = rule(f, panel.a, middle)?;
        let right = rule(f, middle, panel.b)?;
        panels[worst] = left;
        panels.insert(worst + 1, right);
    }
}

fn finish<const N: usize>(panels: &[Panel<N>], target: Target<N>, sign: f64) -> Integral<N> {
    let value: [f64; N] =
        std::array::from_fn(|k| sign * compensated(panels.iter().map(|panel| panel.value[k])));
    let error: [f64; N] = std::array::from_fn(|k| panels.iter().map(|panel| panel.error[k]).sum());
    let converged = (0..N).all(|k| error[k] <= QUADRATURE_ACCEPTANCE_RELATIVE * target.scale[k]);
    Integral {
        value,
        error,
        converged,
    }
}

#[cfg(test)]
mod tests {
    use super::{integrate, rule, Target, MAX_QUADRATURE_INTERVALS};
    use crate::failure::Measure;

    fn free() -> impl FnMut(usize) -> Measure<()> {
        |_| Ok(())
    }

    fn one(scale: f64) -> Target<1> {
        Target { scale: [scale] }
    }

    #[test]
    fn kronrod_is_exact_to_degree_22_and_gauss_to_degree_13() {
        // ∫_0^1 x^d dx = 1/(d+1). The Kronrod sum is exact to degree 22 and
        // the Gauss sum to degree 13, so their difference vanishes up to
        // degree 13 and not beyond.
        for degree in 0..=22 {
            let mut f = |x: f64| -> Measure<[f64; 1]> { Ok([x.powi(degree)]) };
            let panel = rule(&mut f, 0.0, 1.0).unwrap();
            let exact = 1.0 / f64::from(degree + 1);
            assert!(
                (panel.value[0] - exact).abs() < 4.0 * f64::EPSILON,
                "degree {degree}"
            );
            if degree <= 13 {
                assert!(panel.error[0] < 4.0 * f64::EPSILON, "degree {degree}");
            }
        }
        let mut f = |x: f64| -> Measure<[f64; 1]> { Ok([x.powi(14)]) };
        assert!(rule(&mut f, 0.0, 1.0).unwrap().error[0] > 1.0e-10);
    }

    #[test]
    fn sine_over_half_a_period_integrates_to_two() {
        let mut f = |x: f64| -> Measure<[f64; 1]> { Ok([x.sin()]) };
        let integral = integrate(
            &mut free(),
            &mut f,
            0.0,
            std::f64::consts::PI,
            &[],
            one(1.0),
        )
        .unwrap();
        assert!((integral.value[0] - 2.0).abs() <= integral.error[0] + 1.0e-14);
        assert!(integral.converged);
        let reversed = integrate(
            &mut free(),
            &mut f,
            std::f64::consts::PI,
            0.0,
            &[],
            one(1.0),
        )
        .unwrap();
        assert_eq!(reversed.value[0], -integral.value[0]);
    }

    #[test]
    fn a_square_root_endpoint_singularity_reports_an_estimate_at_least_the_error() {
        let mut f = |x: f64| -> Measure<[f64; 1]> { Ok([x.sqrt()]) };
        let integral = integrate(&mut free(), &mut f, 0.0, 1.0, &[], one(1.0)).unwrap();
        let error = (integral.value[0] - 2.0 / 3.0).abs();
        assert!(integral.error[0] >= error);
        assert!(integral.converged);
    }

    #[test]
    fn the_interval_cap_reports_not_converged() {
        // A square wave of a million periods never meets its target, so the
        // subdivision stops at the interval cap.
        let mut f = |x: f64| -> Measure<[f64; 1]> {
            Ok([if (x * 1.0e6).fract() < 0.5 { 0.0 } else { 1.0 }])
        };
        let mut nodes = 0_usize;
        let mut count = |units: usize| -> Measure<()> {
            nodes += units;
            Ok(())
        };
        let integral = integrate(&mut count, &mut f, 0.0, 1.0, &[], one(1.0)).unwrap();
        assert!(!integral.converged);
        assert_eq!(nodes, (2 * MAX_QUADRATURE_INTERVALS - 1) * super::NODES);
    }

    #[test]
    fn equal_inputs_give_equal_bits() {
        let mut f = |x: f64| -> Measure<[f64; 2]> { Ok([(3.0 * x).cos().exp(), x.sqrt()]) };
        let first = integrate(
            &mut free(),
            &mut f,
            0.0,
            2.0,
            &[0.5],
            Target { scale: [1.0; 2] },
        )
        .unwrap();
        let second = integrate(
            &mut free(),
            &mut f,
            0.0,
            2.0,
            &[0.5],
            Target { scale: [1.0; 2] },
        )
        .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn vector_integrands_share_one_subdivision() {
        let mut evaluations = Vec::new();
        let mut f = |x: f64| -> Measure<[f64; 2]> {
            evaluations.push(x);
            Ok([x * x, (5.0 * x).sin()])
        };
        let integral = integrate(
            &mut free(),
            &mut f,
            0.0,
            1.0,
            &[],
            Target { scale: [1.0; 2] },
        )
        .unwrap();
        assert!((integral.value[0] - 1.0 / 3.0).abs() < 1.0e-14);
        let exact = (1.0 - 5.0_f64.cos()) / 5.0;
        assert!((integral.value[1] - exact).abs() <= integral.error[1] + 1.0e-14);
        assert_eq!(evaluations.len() % super::NODES, 0);
    }
}
