// SPDX-License-Identifier: Apache-2.0
//! Hem gap length forms and parallel directions.

use super::body_closures::same_axis_line;
use super::edge_operand_history::historical_edge_axis;
use super::history_states::{effective_scope_previous_history_state_id, unique_history_state_pair};
use super::{EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7, EPS_HISTORY_PARALLEL_DIRECTIONS_E7};
use crate::history_records::{AsmHistoricalCylinder, AsmHistory};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HemGapLengthForm {
    Flat,
    Open,
}

/// Distinguish Flat and Open Hem forms from the inserted coaxial bend carriers.
pub(crate) fn hem_gap_length_form_for_scope(
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> Option<HemGapLengthForm> {
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, histories),
    ) else {
        return None;
    };
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    let (Some(previous_topology), Some(transition)) =
        (history.topology(previous), state.transition.as_ref())
    else {
        return None;
    };
    let current_topology = history.topology(state)?;
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !current_topology
        .surface_cylinders
        .iter()
        .any(|cylinder| inserted_surfaces.contains(&cylinder.surface))
    {
        return None;
    }
    let edge_direction =
        historical_edge_axis(edge_slot, previous_topology).map(|(_, direction)| direction);
    let cylinders = || {
        current_topology
            .surface_cylinders
            .iter()
            .filter(|cylinder| {
                inserted_surfaces.contains(&cylinder.surface)
                    && edge_direction
                        .is_none_or(|direction| parallel_directions(direction, cylinder.axis))
            })
    };
    cylinders().next()?;
    hem_gap_length_form(cylinders())
}

pub(super) fn hem_gap_length_form<'a>(
    mut cylinders: impl Iterator<Item = &'a AsmHistoricalCylinder>,
) -> Option<HemGapLengthForm> {
    let first = cylinders.next()?;
    let second = cylinders.next()?;
    if cylinders.next().is_some() {
        return None;
    }
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return None;
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return None;
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Some(HemGapLengthForm::Open)
    } else if 2.0 * inner < thickness - tolerance {
        Some(HemGapLengthForm::Flat)
    } else {
        None
    }
}

pub(super) fn parallel_directions(
    left: cadmpeg_ir::math::Vector3,
    right: cadmpeg_ir::math::Vector3,
) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    if !(left_length.is_finite()
        && left_length > 0.0
        && right_length.is_finite()
        && right_length > 0.0)
    {
        return false;
    }
    let dot = left
        .scale(1.0 / left_length)
        .dot(right.scale(1.0 / right_length));
    dot.is_finite() && (dot.abs() - 1.0).abs() <= EPS_HISTORY_PARALLEL_DIRECTIONS_E7
}
