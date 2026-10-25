// SPDX-License-Identifier: Apache-2.0
//! The compatible pairs of the bodies of two documents, without the rows of
//! every pair.
//!
//! Only pairs that can be compatible are evaluated, and each evaluation is
//! charged to the comparison's pair allowance; its rows are dropped.
//!
//! - A signature body is compatible only with a signature body of the same
//!   kind, counts and reason codes: every other body has a measured numeric
//!   quantity, which a signature body lacks or has unavailable for an
//!   `input` or `undefined` reason, and that row disagrees.
//! - Two bodies whose boxes are measured under no reading and bound their
//!   tight boxes from below agree on their box rows only when the boxes
//!   overlap along every axis within the largest box tolerance `T` of the
//!   comparison. A sweep along one axis evaluates exactly those pairs.
//! - Every other body is evaluated against every numeric body.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, ResourceDimension, ResourceFailure, ResourceLimit};

use super::rows::{body_rows, kind_name, measurable, signature_only, Context};
use crate::aggregate::{corners, lower_bound};
use crate::report::BodyMeasures;

/// The pair evaluations a comparison may use, and has used.
#[derive(Debug, Clone, Copy)]
pub(super) struct PairWork {
    pub(super) allowance: usize,
    pub(super) used: usize,
}

impl PairWork {
    /// Charge one pair evaluation, or refuse when the allowance is spent.
    fn charge(&mut self) -> Result<(), ResourceLimit> {
        if self.used >= self.allowance {
            return Err(ResourceLimit {
                scope: cadmpeg_core::decode::LimitScope::Session,
                dimension: ResourceDimension::Entities,
                reason: ResourceFailure::BudgetExceeded,
                limit: u64_from_index(self.allowance),
                used: u64_from_index(self.used),
                additional: 1,
                operation: "geometry comparison pair allowance",
            });
        }
        self.used += 1;
        Ok(())
    }
}

/// A body box that bounds the sweep: its corners and its error terms.
struct Swept {
    index: usize,
    min: [f64; 3],
    max: [f64; 3],
    errors: f64,
}

/// The sweep box of a body whose box is measured under no reading and has
/// a lower bound.
fn swept(index: usize, body: &BodyMeasures) -> Option<Swept> {
    let measured = body.bbox().measured()?;
    if measured.reading().is_some() || !measured.candidates().is_empty() {
        return None;
    }
    let value = measured.value();
    lower_bound(value)?;
    let aabb = corners(value);
    Some(Swept {
        index,
        min: aabb.min,
        max: aabb.max,
        errors: measured.numeric_error().get() + measured.model_uncertainty().get(),
    })
}

/// The exact signature facts of a signature body.
type SignatureKey = (&'static str, usize, usize, usize, Vec<&'static str>);

fn signature_key(body: &BodyMeasures) -> SignatureKey {
    (
        kind_name(*body.kind()),
        *body.faces(),
        *body.edges(),
        *body.vertices(),
        body.unavailable_codes(),
    )
}

/// The compatible pairs `(first, second)`, `first < second`, of `bodies`.
pub(super) fn compatible_pairs(
    bodies: &[&BodyMeasures],
    context: &Context,
    work: &mut PairWork,
) -> Result<Vec<(usize, usize)>, ResourceLimit> {
    let mut found = Vec::new();
    let mut evaluate = |first: usize, second: usize| -> Result<(), ResourceLimit> {
        work.charge()?;
        if body_rows(bodies[first], bodies[second], context).compatible {
            found.push((first.min(second), first.max(second)));
        }
        Ok(())
    };
    let mut signatures: BTreeMap<SignatureKey, Vec<usize>> = BTreeMap::new();
    let mut sweeps = Vec::new();
    let mut others = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        if !measurable(body) {
            continue;
        }
        if signature_only(body) {
            signatures
                .entry(signature_key(body))
                .or_default()
                .push(index);
        } else if let Some(sweep) = swept(index, body) {
            sweeps.push(sweep);
        } else {
            others.push(index);
        }
    }
    for members in signatures.values() {
        for (position, first) in members.iter().enumerate() {
            for second in &members[position + 1..] {
                evaluate(*first, *second)?;
            }
        }
    }
    for (position, first) in others.iter().enumerate() {
        for second in &others[position + 1..] {
            evaluate(*first, *second)?;
        }
        for second in &sweeps {
            evaluate(*first, second.index)?;
        }
    }
    let Some(axis) = widest_axis(&sweeps) else {
        return Ok(found);
    };
    let reach = sweep_tolerance(&sweeps, context);
    sweeps.sort_by(|left, right| left.min[axis].total_cmp(&right.min[axis]));
    for (position, first) in sweeps.iter().enumerate() {
        for second in &sweeps[position + 1..] {
            if second.min[axis] > first.max[axis] + reach {
                break;
            }
            evaluate(first.index, second.index)?;
        }
    }
    Ok(found)
}

/// The axis along which the box centres spread the most.
fn widest_axis(sweeps: &[Swept]) -> Option<usize> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for sweep in sweeps {
        for axis in 0..3 {
            let centre = sweep.min[axis].midpoint(sweep.max[axis]);
            low[axis] = low[axis].min(centre);
            high[axis] = high[axis].max(centre);
        }
    }
    (0..3)
        .filter(|axis| low[*axis] <= high[*axis])
        .max_by(|left, right| (high[*left] - low[*left]).total_cmp(&(high[*right] - low[*right])))
}

/// An upper bound of the tolerance of every box row between two swept
/// bodies: both bodies' largest error terms and the relative floor of the
/// largest coordinate or the document scale.
fn sweep_tolerance(sweeps: &[Swept], context: &Context) -> f64 {
    let errors = sweeps.iter().map(|sweep| sweep.errors).fold(0.0, f64::max);
    let magnitude = sweeps
        .iter()
        .flat_map(|sweep| sweep.min.iter().chain(&sweep.max))
        .map(|coordinate| coordinate.abs())
        .fold(0.0, f64::max);
    context.length_bound(magnitude, errors)
}

#[cfg(test)]
mod tests {
    use super::{compatible_pairs, Context, PairWork};
    use crate::select::Selection;
    use crate::test_support::builders::measure;

    #[test]
    fn a_spent_pair_allowance_refuses_the_comparison() {
        let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
        let report = measure(&cube, &Selection::default());
        let body = &report.bodies()[0];
        let context = Context {
            floor: 1.0e-9,
            scale: 20.0,
        };
        let mut work = PairWork {
            allowance: 1,
            used: 0,
        };
        let pairs = compatible_pairs(&[body, body], &context, &mut work).expect("one pair");
        assert_eq!(pairs, [(0, 1)]);
        assert_eq!(work.used, 1);
        let mut spent = PairWork {
            allowance: 0,
            used: 0,
        };
        let refusal = compatible_pairs(&[body, body], &context, &mut spent)
            .expect_err("no pair evaluation is left");
        assert_eq!(refusal.limit, 0);
        assert_eq!(refusal.operation, "geometry comparison pair allowance");
    }
}
