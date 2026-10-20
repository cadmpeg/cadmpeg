//! One-based arc parameters shared by E5 bounds and current limits.

use cadmpeg_ir::scalar::FiniteReal;

const EPS_ARC_LOCAL_BOUND: f64 = 1.0e-9;

/// Resolve a finite local parameter inside its selected knot span.
pub(super) fn arc_local_parameter(start: f64, end: f64, local: f64) -> Option<FiniteReal> {
    let width = end - start;
    let tolerance = EPS_ARC_LOCAL_BOUND * (1.0 + local.abs());
    if !start.is_finite()
        || !end.is_finite()
        || !local.is_finite()
        || width <= 0.0
        || local < -tolerance
        || local > width + tolerance
    {
        return None;
    }
    FiniteReal::new(start + local)
}

/// Resolve two current limits through their one-based distinct-knot arcs.
pub(super) fn current_limits(
    arcs: [u16; 2],
    locals: [f64; 2],
    mut span: impl FnMut(usize) -> Option<[f64; 2]>,
) -> Option<[FiniteReal; 2]> {
    let mut limits = [FiniteReal::ZERO; 2];
    for index in 0..2 {
        let arc = usize::from(arcs[index].checked_sub(1)?);
        let [start, end] = span(arc)?;
        limits[index] = arc_local_parameter(start, end, locals[index])?;
    }
    (limits[0] < limits[1]).then_some(limits)
}
