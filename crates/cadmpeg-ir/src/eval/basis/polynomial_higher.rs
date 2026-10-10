// SPDX-License-Identifier: Apache-2.0
//! Polynomial higher orders from one Cox triangle and its preceding row.

use std::borrow::Cow;
use cadmpeg_core::decode::{ResourceLimit, ScopedReservation};
use super::super::{decode, EvaluationFailure};
use super::{EvaluationAdmission, PositiveReal};

pub(super) struct PreviousBasis<'ctx> {
    backing: PreviousBacking<'ctx>,
    state: PreviousState,
}

#[derive(Clone, Copy)]
enum PreviousState { Pending, Known, Unavailable }

enum PreviousBacking<'ctx> {
    Inline { values: [f64; 2], len: usize },
    Heap(PreviousHeap<'ctx>),
}

struct PreviousHeap<'ctx> {
    values: Vec<f64>,
    // This actual preceding-row backing dies before its lease.
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<'ctx> PreviousBasis<'ctx> {
    fn new(scratch: &decode::Scratch<'ctx, '_>, count: usize) -> Option<Self> {
        if count <= 2 {
            return Some(Self { backing: PreviousBacking::Inline { values: [0.0; 2], len: count }, state: PreviousState::Pending });
        }
        let (values, storage) = scratch.temporary_vec(count, "IR polynomial preceding basis storage")?;
        Some(Self { backing: PreviousBacking::Heap(PreviousHeap { values, _storage: storage }), state: PreviousState::Pending })
    }

    pub(super) fn capture(&mut self, admission: EvaluationAdmission<'_, '_>, source: &[f64])
        -> Result<(), ResourceLimit>
    {
        if matches!(self.state, PreviousState::Unavailable) { return Ok(()); }
        match &mut self.backing {
            PreviousBacking::Inline { values, len } => {
                if source.len() != *len { self.state = PreviousState::Unavailable; return Ok(()); }
                values[..*len].copy_from_slice(source);
            }
            PreviousBacking::Heap(heap) => {
                for value in source {
                    if admission.independent_cost::<()>(Some(1)).is_err() {
                        self.state = PreviousState::Unavailable; return Ok(());
                    }
                    admission.work(1, "IR polynomial preceding basis copy")?;
                    heap.values.push(*value);
                }
            }
        }
        self.state = PreviousState::Known;
        Ok(())
    }

    pub(super) fn lose_coefficients(&mut self) { self.state = PreviousState::Unavailable; }

    fn as_slice(&self) -> Option<&[f64]> {
        if !matches!(self.state, PreviousState::Known) { return None; }
        Some(match &self.backing {
            PreviousBacking::Inline { values, len } => &values[..*len],
            PreviousBacking::Heap(heap) => &heap.values,
        })
    }
}

pub(in crate::eval) struct PolynomialHigherBasis {
    pub(in crate::eval) third: Result<Cow<'static, [f64]>, EvaluationFailure<()>>,
    pub(in crate::eval) fourth: Result<Cow<'static, [f64]>, EvaluationFailure<()>>,
    pub(in crate::eval) fifth: Result<Cow<'static, [f64]>, EvaluationFailure<()>>,
}

/// Preceding rows copied from the point owner's actual Cox triangle.
/// Each heap row owns its backing and its lease through destruction.
pub(in crate::eval) struct CapturedBasis<'ctx> {
    degree: usize,
    max_order: usize,
    values: [Option<PreviousBasis<'ctx>>; 3],
}

impl<'ctx> CapturedBasis<'ctx> {
    fn new(scratch: &decode::Scratch<'ctx, '_>, degree: usize, max_order: usize) -> Option<Self> {
        let mut values = [None, None, None];
        for (at, value) in values.iter_mut().enumerate() {
            let order = at + 3;
            if order <= max_order && degree >= order {
                *value = Some(PreviousBasis::new(scratch, degree - order + 1)?);
            }
        }
        Some(Self { degree, max_order, values })
    }

    pub(super) fn capture(&mut self, admission: EvaluationAdmission<'_, '_>, degree: usize, values: &[f64])
        -> Result<(), ResourceLimit>
    {
        for (at, target) in self.values.iter_mut().enumerate() {
            if self.degree.checked_sub(at + 3) == Some(degree) {
                if let Some(target) = target { target.capture(admission, values)?; }
            }
        }
        Ok(())
    }

    pub(super) fn lose_coefficients(&mut self, degree: usize) {
        for (at, target) in self.values.iter_mut().enumerate() {
            if self.degree.checked_sub(at + 3).is_some_and(|target| degree <= target) {
                if let Some(target) = target { target.lose_coefficients(); }
            }
        }
    }

    pub(in crate::eval) fn rows(&self, scratch: &decode::Scratch<'_, '_>, knots: &[f64], span: usize,
        scale: PositiveReal) -> PolynomialHigherBasis
    {
        let derivative = |at: usize| {
            let order = at + 3;
            if order > self.max_order { return Err(EvaluationFailure::NoValue); }
            if self.degree < order {
                let zeros: &'static [f64] = match self.degree {
                    0 => &[0.0], 1 => &[0.0; 2], 2 => &[0.0; 3],
                    3 => &[0.0; 4], _ => &[0.0; 5],
                };
                return Ok(Cow::Borrowed(zeros));
            }
            let source = self.values[at].as_ref().and_then(PreviousBasis::as_slice)
                .ok_or(EvaluationFailure::NoValue)?;
            let mut lower = Cow::Borrowed(source);
            for row_degree in self.degree - order + 1..=self.degree {
                let values = super::bspline_basis_scaled_derivative_level(scratch, knots, row_degree, span,
                    match order {
                        3 => super::ScaledDerivativeOrder::Third(scale),
                        4 => super::ScaledDerivativeOrder::Fourth(scale),
                        _ => super::ScaledDerivativeOrder::Fifth(scale),
                    }, &lower).ok_or_else(|| scratch.failure(EvaluationFailure::NoValue))?;
                lower = Cow::Owned(values);
            }
            Ok(Cow::Owned(lower.into_owned()))
        };
        PolynomialHigherBasis { third: derivative(0), fourth: derivative(1), fifth: derivative(2) }
    }
}

/// Point basis and higher derivative seeds from one selected span triangle.
pub(in crate::eval) fn point_basis<'ctx>(scratch: &decode::Scratch<'ctx, '_>, knots: &[f64], degree: usize,
    span: usize, t: f64, max_order: usize) -> Option<(decode::SupportValues<f64>, CapturedBasis<'ctx>)>
{
    let support = degree.checked_add(1)?;
    let mut values = if support <= 2 {
        decode::SupportValues::Inline { values: [0.0; 2], len: support }
    } else {
        decode::SupportValues::Heap(scratch.filled(support, 0.0,
            "IR B-spline basis", "IR B-spline basis work")?)
    };
    let mut captured = CapturedBasis::new(scratch, degree, max_order)?;
    scratch.admit(super::fill_bspline_basis_rows(scratch.admission, knots, degree, span, t, &mut values,
        None, None, Some(&mut captured)))??;
    Some((values, captured))
}

/// Preserve the old Third row arithmetic. Fourth starts in the penultimate
/// degree of the same Cox triangle; it does not replay that triangle.
pub(in crate::eval) fn rows(
    scratch: &decode::Scratch<'_, '_>,
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    scale: Option<PositiveReal>,
    fifth_requested: bool,
) -> Result<PolynomialHigherBasis, EvaluationFailure<()>> {
    scratch.unless_refused()?;
    let no_value = EvaluationFailure::NoValue;
    let base_degree = degree.checked_sub(3).filter(|degree| *degree >= 1).ok_or(no_value)?;
    let support = base_degree.checked_add(1).ok_or(no_value)?;
    let base_work = if base_degree <= 1 { 0 } else {
        base_degree.checked_add(2).and_then(|cost| cost.checked_add(
            base_degree.checked_mul(base_degree.checked_add(1)?)?.checked_div(2)?))
            .and_then(|cost| cost.checked_add(base_degree)).ok_or(no_value)?
    };
    scratch.admission.independent_cost(Some(base_work))?;
    let mut base = if support <= 2 {
        decode::SupportValues::Inline { values: [0.0; 2], len: support }
    } else {
        decode::SupportValues::Heap(scratch.filled(support, 0.0,
            "IR B-spline basis", "IR B-spline basis work")
            .ok_or_else(|| scratch.failure(no_value))?)
    };
    let mut previous = PreviousBasis::new(scratch, base_degree)
        .ok_or_else(|| scratch.failure(no_value))?;
    let mut second_previous = if fifth_requested && degree >= 5 {
        Some(PreviousBasis::new(scratch, base_degree - 1)
            .ok_or_else(|| scratch.failure(no_value))?)
    } else { None };
    let filled = scratch.admit(super::fill_bspline_basis_rows(scratch.admission, knots,
        base_degree, span, t, &mut base, Some(&mut previous), second_previous.as_mut(), None));
    let derivative_rows = |base: &[f64], first_degree: usize, order| {
        let mut lower: Cow<'_, [f64]> = Cow::Borrowed(base);
        if scale.is_none() && order == 3 {
            scratch.admission.independent_cost(degree.checked_mul(3))?;
        }
        for row_degree in first_degree..=degree {
            let values = if let Some(scale) = scale {
                super::bspline_basis_scaled_derivative_level(scratch, knots, row_degree, span,
                    match order {
                        3 => super::ScaledDerivativeOrder::Third(scale),
                        4 => super::ScaledDerivativeOrder::Fourth(scale),
                        _ => super::ScaledDerivativeOrder::Fifth(scale),
                    }, &lower)
            } else {
                super::higher_basis_derivative_level(scratch, knots, row_degree, span, &lower,
                    match order {
                        3 => super::HigherBasisOrder::Third,
                        4 => super::HigherBasisOrder::RequestedFourth,
                        _ => super::HigherBasisOrder::RequestedFifth,
                    })
            };
            lower = Cow::Owned(values.ok_or_else(|| scratch.failure(
                if order >= 4 || scale.is_some() || scratch.admission.work_slice().is_some_and(|work| work.exhausted()) {
                    no_value
                } else { EvaluationFailure::NonFinite(()) }))?);
        }
        Ok(Cow::Owned(lower.into_owned()))
    };
    let third = if matches!(filled, Some(Some(()))) {
        derivative_rows(&base, base_degree + 1, 3)
    } else { Err(scratch.failure(if scale.is_some() { no_value } else { EvaluationFailure::NonFinite(()) })) };
    let fourth = previous.as_slice().ok_or(no_value)
        .and_then(|previous| derivative_rows(previous, base_degree, 4));
    let fifth = second_previous.as_ref().and_then(PreviousBasis::as_slice).ok_or(no_value)
        .and_then(|previous| derivative_rows(previous, base_degree - 1, 5));
    scratch.settle(Ok(PolynomialHigherBasis { third, fourth, fifth }))
}

#[cfg(test)]
mod tests;
