// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Phase-4B typed lossy construction for the `.f3d` codec (doc §6.2, §10).
//!
//! Every incomplete or approximate transfer f3d reports crosses one of the
//! named Phase-4B boundaries: a concept that did not reach typed IR
//! (unsupported-concept-to-omission), or a form that survived only
//! approximately (a spline/procedural definition reduced to a cached carrier).
//! This module is the single construction path for those notes. A decoder that
//! wants to record a loss reaches [`omit`] or [`reduce`] here, both of which
//! resolve the note through the platform [`Builder`](cadmpeg_ir::transfer::Builder)
//! into the report's loss channel. The guarantee is that this module is the one
//! construction path the geometry path uses, not a compiler ban: a bare
//! `losses.push(note)` elsewhere would still compile (`clippy.toml` disallows
//! only the capacity-taking `Vec` methods, not `push`), so the no-silent-drop
//! property is held by keeping construction here and by review, not by the type
//! system.
//!
//! [`omit`] models the omission boundary: the value the transfer would have
//! carried does not exist, so [`Transfer::omitted`] records the note and yields
//! nothing. [`reduce`] models an approximation that is still transferred (the
//! carrier is present in the IR); its note is an accountable census that
//! [`Builder::record_loss`] threads into the same sink.

use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::transfer::{Builder, Transfer};

/// Record an omission: a source concept that produced no typed IR entity.
///
/// The note drains through [`Transfer::omitted`], which records it and yields
/// [`None`] — the omission cannot be reached without surrendering the note, so
/// it is never silent.
pub(crate) fn omit(losses: &mut Vec<LossNote>, note: LossNote) {
    let omitted: Option<()> = Builder::new(losses).take(Transfer::omitted(note));
    debug_assert!(omitted.is_none(), "an omission transfer yields no value");
}

/// Record an accountable reduction or informational census: the form was
/// transferred, but only approximately (a solved carrier), or the note reports
/// a count with no content lost. The value already lives in the IR, so the note
/// is a standalone census threaded through the shared sink rather than a value
/// crossing the boundary.
pub(crate) fn reduce(losses: &mut Vec<LossNote>, note: LossNote) {
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
    fn reduce_records_the_census_note() {
        let mut losses = Vec::new();
        reduce(&mut losses, note(LossCode::ProceduralReduced));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::ProceduralReduced);
    }
}
