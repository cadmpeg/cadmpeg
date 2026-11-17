// SPDX-License-Identifier: Apache-2.0
//! Encode coincident full-multiplicity joins as shared homogeneous poles.

use crate::writer::Ref;

pub(super) fn continuous_curve_knots(
    points: &mut Vec<Ref>,
    weights: &mut Option<Vec<f64>>,
    knots: &[f64],
    degree: u32,
    periodic: bool,
) -> (Vec<f64>, Vec<usize>) {
    let (values, mut multiplicities) = super::compress_knots(knots);
    if periodic || degree == 0 {
        return (values, multiplicities);
    }
    let Some(full) = usize::try_from(degree).ok().and_then(|d| d.checked_add(1)) else {
        return (values, multiplicities);
    };
    let mut removed = Vec::new();
    let mut prefix = 0_usize;
    let Some(last) = multiplicities.len().checked_sub(1) else {
        return (values, multiplicities);
    };
    for (index, multiplicity) in multiplicities.iter_mut().enumerate() {
        let original = *multiplicity;
        if index > 0 && index < last && original == full && prefix > 0 {
            let equal_points = points
                .get(prefix - 1)
                .zip(points.get(prefix))
                .is_some_and(|(a, b)| a == b);
            let equal_weights = weights.as_ref().is_none_or(|weights| {
                weights
                    .get(prefix - 1)
                    .zip(weights.get(prefix))
                    .is_some_and(|(&a, &b)| a > 0.0 && a == b)
            });
            if equal_points && equal_weights {
                *multiplicity -= 1;
                removed.push(prefix);
            }
        }
        let Some(next) = prefix.checked_add(original) else {
            break;
        };
        prefix = next;
    }
    if !removed.is_empty() {
        remove_indices(points, &removed);
        if let Some(weights) = weights {
            remove_indices(weights, &removed);
        }
    }
    (values, multiplicities)
}

fn remove_indices<T>(values: &mut Vec<T>, removed: &[usize]) {
    let mut removed = removed.iter().copied().peekable();
    let mut index = 0;
    values.retain(|_| {
        let keep = removed.peek() != Some(&index);
        if !keep {
            removed.next();
        }
        index += 1;
        keep
    });
}

#[cfg(test)]
mod tests;
