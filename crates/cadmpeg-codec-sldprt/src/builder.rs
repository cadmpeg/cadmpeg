// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Phase-4B typed lossy construction for the `.sldprt` codec (doc §6.2, §10).
//!
//! Every incomplete or approximate transfer the `.sldprt` decoder reports
//! crosses one of the named Phase-4B boundaries: a mandatory concept that never
//! reached typed IR (unsupported-concept-to-omission — no partition/deltas
//! stream resolved into a topology graph, an untyped support surface or curve
//! carrier), or a field the source did not carry that a deterministic gauge
//! filled (defaulted-field / resolver-to-fallback — a derived body hierarchy).
//! This module is the single construction path for those notes. A decoder that
//! wants to record a loss reaches [`omit`], [`substitute`], or [`census`] here,
//! each of which resolves the note through the platform
//! [`Builder`](cadmpeg_ir::transfer::Builder) into the report's loss channel —
//! so the bare `losses.push` spelling that would let a drop or a silent
//! substitution go unrecorded is never written on the decode path.
//!
//! [`omit`] models the omission boundary: the value the transfer would have
//! carried does not exist, so [`Transfer::omitted`] records the note and yields
//! nothing. [`substitute`] models the fallback boundary: a deterministic value
//! stands in for an absent or unrepresentable source field, so
//! [`Transfer::fallback`] records the note and hands the substituted value back
//! — unreachable without surrendering its note. [`census`] records an
//! accountable aggregate note for content that is already present in the IR as
//! an opaque carrier (the value crossed the boundary earlier); its note is a
//! standalone census threaded through the same sink.

use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::transfer::{Builder, Transfer};

/// Record an omission: a mandatory source concept that produced no typed IR
/// entity.
///
/// The note drains through [`Transfer::omitted`], which records it and yields
/// [`None`] — the omission cannot be reached without surrendering the note, so
/// it is never silent.
pub(crate) fn omit(losses: &mut Vec<LossNote>, note: LossNote) {
    let omitted: Option<()> = Builder::new(losses).take(Transfer::omitted(note));
    debug_assert!(omitted.is_none(), "an omission transfer yields no value");
}

/// Resolve a deterministic substitution: `value` stands in for an absent or
/// unrepresentable source field, and `note` explains the fallback.
///
/// The value passes through [`Transfer::fallback`], which records the note
/// before returning the value, so the substitution cannot be reached without
/// surrendering its loss note.
pub(crate) fn substitute<T>(losses: &mut Vec<LossNote>, value: T, note: LossNote) -> T {
    Builder::new(losses)
        .take(Transfer::fallback(value, note))
        .expect("a fallback transfer always yields its substituted value")
}

/// Record an accountable census or aggregate reduction: the affected content is
/// already present in the IR (for example an opaque surface or curve carrier
/// that replaced an untyped support record), so the note is a standalone
/// aggregate threaded through the shared sink rather than a value crossing the
/// boundary.
pub(crate) fn census(losses: &mut Vec<LossNote>, note: LossNote) {
    Builder::new(losses).record_loss(note);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::report::{LossCategory, LossCode, Severity};

    fn note(code: LossCode) -> LossNote {
        LossNote {
            code,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: "test".to_owned(),
            provenance: None,
        }
    }

    #[test]
    fn omit_records_the_note_through_the_typed_builder() {
        let mut losses = Vec::new();
        omit(&mut losses, note(LossCode::GeometryNotTransferred));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::GeometryNotTransferred);
    }

    #[test]
    fn substitute_yields_the_value_and_records_the_note() {
        let mut losses = Vec::new();
        let kept = substitute(&mut losses, 42, note(LossCode::TopologyGaugeSubstituted));
        assert_eq!(kept, 42);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::TopologyGaugeSubstituted);
    }

    #[test]
    fn census_records_the_aggregate_note() {
        let mut losses = Vec::new();
        census(&mut losses, note(LossCode::GeometryNotTransferred));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::GeometryNotTransferred);
    }
}
