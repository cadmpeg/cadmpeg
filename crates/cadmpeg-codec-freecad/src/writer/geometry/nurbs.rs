// SPDX-License-Identifier: Apache-2.0
//! Reverse the OCCT periodic-knot and cyclic-pole normalization.

use cadmpeg_ir::geometry::nurbs::{KnotVector, NurbsCurve, NurbsSurface};
use std::fmt::Write as _;

pub(in crate::writer) struct Axis {
    pub(in crate::writer) rows: Vec<(f64, usize)>,
    pub(in crate::writer) count: usize,
    pub(in crate::writer) padding: usize,
}

/// Recover one OCCT axis without changing its parameterization.
pub(in crate::writer) fn axis(
    knots: &KnotVector,
    degree: u32,
    count: usize,
    periodic: bool,
) -> Option<Axis> {
    let full = knots.as_slice();
    let degree = usize::try_from(degree).ok()?;
    let padding = if periodic {
        let first = *full.get(degree)?;
        let last = *full.get(count)?;
        if first >= last {
            return None;
        }
        let first_multiplicity = full.iter().filter(|knot| **knot == first).count();
        let last_multiplicity = full.iter().filter(|knot| **knot == last).count();
        if first_multiplicity == 0
            || first_multiplicity > degree
            || last_multiplicity != first_multiplicity
        {
            return None;
        }
        degree.checked_add(1)?.checked_sub(first_multiplicity)?
    } else {
        0
    };
    let count = count.checked_sub(padding)?;
    if count == 0 || count < padding || (!periodic && count <= degree) {
        return None;
    }
    let source = full.get(padding..full.len().checked_sub(padding)?)?;
    if periodic {
        let first = *source.first()?;
        let last = *source.last()?;
        let multiplicity = degree + 1 - padding;
        let before_last = source.len().checked_sub(multiplicity)?;
        let lower = source.get(before_last.checked_sub(padding)?..before_last)?;
        let upper = source.get(multiplicity..multiplicity.checked_add(padding)?)?;
        for (actual, knot) in full[..padding].iter().zip(lower) {
            if *actual != first - (last - *knot) {
                return None;
            }
        }
        for (actual, knot) in full[full.len() - padding..].iter().zip(upper) {
            if *actual != last + (*knot - first) {
                return None;
            }
        }
    }
    let mut rows: Vec<(f64, usize)> = Vec::new();
    for knot in source {
        if let Some((_, multiplicity)) = rows.last_mut().filter(|(last, _)| *last == *knot) {
            *multiplicity += 1;
        } else {
            rows.push((*knot, 1));
        }
    }
    Some(Axis {
        rows,
        count,
        padding,
    })
}

/// Removing periodic padding is safe only when it repeats the same poles.
pub(in crate::writer) fn cyclic_tail<T: PartialEq>(lane: &[T], axis: &Axis) -> bool {
    lane.len() == axis.count + axis.padding && lane[axis.count..] == lane[..axis.padding]
}

pub(super) fn curve(value: &NurbsCurve) -> Option<String> {
    let axis = axis(
        value.knots(),
        value.degree(),
        value.pole_count(),
        value.periodic(),
    )?;
    let poles = value.control_points();
    let weights = value.weights();
    if !cyclic_tail(&poles, &axis)
        || weights.as_ref().is_some_and(|weights| {
            !cyclic_tail(weights, &axis) || weights.iter().any(|weight| weight.get() <= 0.0)
        })
    {
        return None;
    }
    let mut output = format!(
        "7 {} {} {} {} {}",
        u8::from(weights.is_some()),
        u8::from(value.periodic()),
        value.degree(),
        axis.count,
        axis.rows.len()
    );
    for (index, pole) in poles.iter().take(axis.count).enumerate() {
        write!(output, " {}", super::point(pole.get())).expect("writing to String cannot fail");
        if let Some(weights) = &weights {
            write!(output, " {}", weights[index].get()).expect("writing to String cannot fail");
        }
    }
    append_knots(&mut output, axis.rows);
    Some(output)
}

pub(super) fn surface(value: &NurbsSurface) -> Option<String> {
    let u = axis(
        value.u_knots(),
        value.u_degree(),
        value.u_count(),
        value.u_periodic(),
    )?;
    let v = axis(
        value.v_knots(),
        value.v_degree(),
        value.v_count(),
        value.v_periodic(),
    )?;
    let poles = value.control_grid();
    let weights = value.weights();
    if !cyclic_tail(&poles, &u)
        || poles.iter().any(|row| !cyclic_tail(row, &v))
        || weights.as_ref().is_some_and(|weights| {
            !cyclic_tail(weights, &u)
                || weights.iter().any(|row| {
                    !cyclic_tail(row, &v) || row.iter().any(|weight| weight.get() <= 0.0)
                })
        })
    {
        return None;
    }
    let mut output = format!(
        "9 {} {} {} {} {} {} {} {} {} {}",
        u8::from(weights.is_some()),
        u8::from(weights.is_some()),
        u8::from(value.u_periodic()),
        u8::from(value.v_periodic()),
        value.u_degree(),
        value.v_degree(),
        u.count,
        v.count,
        u.rows.len(),
        v.rows.len()
    );
    for (ui, row) in poles.iter().take(u.count).enumerate() {
        for (vi, pole) in row.iter().take(v.count).enumerate() {
            write!(output, " {}", super::point(pole.get())).expect("writing to String cannot fail");
            if let Some(weights) = &weights {
                write!(output, " {}", weights[ui][vi].get())
                    .expect("writing to String cannot fail");
            }
        }
    }
    append_knots(&mut output, u.rows);
    append_knots(&mut output, v.rows);
    Some(output)
}

fn append_knots(output: &mut String, rows: Vec<(f64, usize)>) {
    for (knot, multiplicity) in rows {
        write!(output, " {knot} {multiplicity}").expect("writing to String cannot fail");
    }
}

#[cfg(test)]
mod tests;
