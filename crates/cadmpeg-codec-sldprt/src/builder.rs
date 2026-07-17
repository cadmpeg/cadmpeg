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
//! This module is the construction path for those notes. A decoder that wants
//! to record a loss reaches [`omit`] or [`census`] here, each of which resolves
//! the note through the platform [`Builder`](cadmpeg_ir::transfer::Builder) into
//! the report's loss channel. The guarantee is that this module is the one path
//! the decode uses, not a compiler ban: a bare `losses.push(note)` elsewhere
//! would still compile (`clippy.toml` disallows only the capacity-taking `Vec`
//! methods, not `push`), so the no-silent-drop property is held by keeping
//! construction here and by review, not by the type system.
//!
//! [`omit`] models the omission boundary: the value the transfer would have
//! carried does not exist, so [`Transfer::omitted`] records the note and yields
//! nothing. [`census`] records an accountable aggregate note for content that is
//! already present in the IR as an opaque carrier or a derived hierarchy (the
//! value entered the arenas earlier, so there is nothing left to gate); its note
//! is a standalone census threaded through the same sink.

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
    fn census_records_the_aggregate_note() {
        let mut losses = Vec::new();
        census(&mut losses, note(LossCode::GeometryNotTransferred));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::GeometryNotTransferred);
    }
}
