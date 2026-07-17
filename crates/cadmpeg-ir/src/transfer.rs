// SPDX-License-Identifier: Apache-2.0
//! Typed lossy construction at the named Phase-4B boundaries (§6.2, §10).
//!
//! Byte conservation proves no bytes vanished; per-record dispositions prove
//! every committed record is accounted for. Neither makes a *value*
//! substitution honest: a decoder that fills a missing axis with `Vector3::z()`
//! or a writer that drops a hidden body can leave both ledgers balanced while
//! silently losing meaning. [`Transfer`] closes that gap at the five boundaries
//! §10 Phase 4B names — decoder record to entity, defaulted field, entity to
//! writer, unsupported concept to omission, and resolver to fallback
//! carrier/axis — by making the substituted or omitted value *unreadable*
//! without surrendering the loss note that explains it.
//!
//! The mechanism is a resolution barrier, not a wrapper on every field
//! (§12 rejects `Transfer<T>` on every IR slot). A [`Transfer`] is
//! `#[must_use]` and its payload can only be extracted through
//! [`Transfer::resolve`], which takes a [`LossSink`] and pushes the note for a
//! [`Fallback`](Transfer::Fallback) or [`Dropped`](Transfer::Dropped) outcome
//! before returning the value. There is no `unwrap_or_default`: a graduated
//! module that reaches for the value at all reaches through the sink, so a
//! silent fallback or drop is not a discipline failure but a construction that
//! does not typecheck. [`Builder`] threads one sink through a module so every
//! boundary in it resolves into the same loss channel.

use crate::provenance::Exactness;
use crate::report::LossNote;

/// A channel that collects the loss notes emitted when lossy [`Transfer`]s
/// resolve.
///
/// Both decode and export already accumulate `Vec<LossNote>` before attaching
/// it to a [`DecodeReport`](crate::report::DecodeReport) or
/// [`ExportReport`](crate::report::ExportReport); implementing this trait on
/// that vector lets the same buffer back a [`Builder`], so typed construction
/// and the report share one loss channel with no second bookkeeping pass.
pub trait LossSink {
    /// Records one loss note raised by a resolving [`Transfer`].
    fn record_loss(&mut self, note: LossNote);
}

impl LossSink for Vec<LossNote> {
    fn record_loss(&mut self, note: LossNote) {
        self.push(note);
    }
}

impl<S: LossSink + ?Sized> LossSink for &mut S {
    fn record_loss(&mut self, note: LossNote) {
        (**self).record_loss(note);
    }
}

/// The value-level transfer outcome for one field or entity crossing a Phase-4B
/// boundary (§6.2).
///
/// The payload is reachable only through [`resolve`](Transfer::resolve), which
/// forwards a [`Fallback`](Transfer::Fallback) or [`Dropped`](Transfer::Dropped)
/// note to a [`LossSink`] as it hands the value back (or, for `Dropped`,
/// records the note and yields nothing). The four variants map onto the named
/// boundaries: [`Exact`](Transfer::Exact) and [`Derived`](Transfer::Derived)
/// carry a faithful record-to-entity or computed value, `Fallback` carries a
/// defaulted field or a resolver's substituted carrier/axis, and `Dropped`
/// carries an unsupported concept omitted by a decoder or a writer. `Exact`,
/// `fallback`, and `omitted` have ergonomic constructors; `Derived` is built by
/// struct literal until a graduated module first computes one.
#[must_use = "a Transfer carries a possible loss; resolve it through a LossSink"]
pub enum Transfer<T> {
    /// Read verbatim from the source with no substitution.
    Exact(T),
    /// Computed deterministically; carries the resulting exactness.
    Derived {
        /// The computed value.
        value: T,
        /// How the value relates to its byte-exact inputs.
        exactness: Exactness,
    },
    /// A value was supplied in place of an absent, unresolved, or
    /// unrepresentable source field; the note explains the substitution.
    Fallback {
        /// The substituted value.
        value: T,
        /// The accountable loss the substitution incurred.
        note: LossNote,
    },
    /// No value could be produced; the note records the omission.
    Dropped(LossNote),
}

impl<T> Transfer<T> {
    /// A byte-exact value (decoder record to entity, faithful case).
    pub fn exact(value: T) -> Self {
        Transfer::Exact(value)
    }

    /// A resolver's substituted carrier or axis (resolver to fallback
    /// carrier/axis boundary), and the defaulted-field boundary — both map to
    /// [`Fallback`](Transfer::Fallback). The note is what strict mode weighs.
    pub fn fallback(value: T, note: LossNote) -> Self {
        Transfer::Fallback { value, note }
    }

    /// An unsupported concept omitted by a decoder or writer (unsupported
    /// concept to omission boundary).
    pub fn omitted(note: LossNote) -> Self {
        Transfer::Dropped(note)
    }

    /// Extracts the value, recording any loss into `sink`.
    ///
    /// This is the only accessor: an [`Exact`](Transfer::Exact) or
    /// [`Derived`](Transfer::Derived) value passes through untouched, a
    /// [`Fallback`](Transfer::Fallback) value passes through *after* its note is
    /// recorded, and a [`Dropped`](Transfer::Dropped) outcome records its note
    /// and yields [`None`]. A silent fallback or drop is therefore unwritable —
    /// the value cannot be reached without surrendering the note.
    pub fn resolve(self, sink: &mut impl LossSink) -> Option<T> {
        match self {
            Transfer::Exact(value) => Some(value),
            Transfer::Derived { value, .. } => Some(value),
            Transfer::Fallback { value, note } => {
                sink.record_loss(note);
                Some(value)
            }
            Transfer::Dropped(note) => {
                sink.record_loss(note);
                None
            }
        }
    }
}

/// The single construction path for a graduated module: one [`LossSink`]
/// threaded through every Phase-4B boundary the module crosses.
///
/// Holding a `Builder` gives a decoder or writer exactly one way to read a
/// [`Transfer`] — [`take`](Builder::take) — so every substitution and omission
/// in the module drains into the same loss channel. A module that constructs
/// entities only through a `Builder` cannot express a silent fallback, which is
/// the property §10 Phase 4B requires before fanout.
pub struct Builder<'s, S: LossSink> {
    sink: &'s mut S,
}

impl<'s, S: LossSink> Builder<'s, S> {
    /// Wraps a loss sink for the duration of a module's construction.
    pub fn new(sink: &'s mut S) -> Self {
        Builder { sink }
    }

    /// Resolves one transfer, recording any loss into the shared sink.
    pub fn take<T>(&mut self, transfer: Transfer<T>) -> Option<T> {
        transfer.resolve(self.sink)
    }

    /// Records a standalone loss that is not tied to a value crossing the
    /// boundary (for example an aggregate census note).
    pub fn record_loss(&mut self, note: LossNote) {
        self.sink.record_loss(note);
    }
}

/// Record an omission: a source concept that produced no typed IR entity.
///
/// The note drains through [`Transfer::omitted`], which records it and yields
/// [`None`] — the omission cannot be reached without surrendering the note, so
/// it is never silent.
pub fn omit<S: LossSink>(sink: &mut S, note: LossNote) {
    let omitted: Option<()> = Builder::new(sink).take(Transfer::omitted(note));
    debug_assert!(omitted.is_none(), "an omission transfer yields no value");
}

/// Record an accountable reduction or informational census: the form was
/// transferred, but only approximately (a solved carrier), or the note reports
/// a count with no content lost. The value already lives in the IR, so the note
/// is a standalone census threaded through the shared sink.
pub fn reduce<S: LossSink>(sink: &mut S, note: LossNote) {
    Builder::new(sink).record_loss(note);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{LossCategory, LossCode, Severity};

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
    fn exact_and_derived_record_no_loss() {
        let mut sink: Vec<LossNote> = Vec::new();
        assert_eq!(Transfer::exact(7).resolve(&mut sink), Some(7));
        assert_eq!(
            Transfer::Derived {
                value: 9,
                exactness: Exactness::Derived,
            }
            .resolve(&mut sink),
            Some(9)
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn fallback_yields_value_and_records_note() {
        let mut sink: Vec<LossNote> = Vec::new();
        let value = Transfer::fallback(3, note(LossCode::CarrierAxisInferred)).resolve(&mut sink);
        assert_eq!(value, Some(3));
        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].code, LossCode::CarrierAxisInferred);
    }

    #[test]
    fn dropped_yields_nothing_and_records_note() {
        let mut sink: Vec<LossNote> = Vec::new();
        let value: Option<i32> =
            Transfer::omitted(note(LossCode::UnsupportedObjectFamily)).resolve(&mut sink);
        assert_eq!(value, None);
        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].code, LossCode::UnsupportedObjectFamily);
    }

    #[test]
    fn omit_records_the_note_through_the_typed_builder() {
        let mut losses: Vec<LossNote> = Vec::new();
        omit(&mut losses, note(LossCode::GeometryNotTransferred));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::GeometryNotTransferred);
    }

    #[test]
    fn reduce_records_the_census_note() {
        let mut losses: Vec<LossNote> = Vec::new();
        reduce(&mut losses, note(LossCode::ProceduralReduced));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, LossCode::ProceduralReduced);
    }

    #[test]
    fn builder_threads_one_sink() {
        let mut sink: Vec<LossNote> = Vec::new();
        let mut builder = Builder::new(&mut sink);
        assert_eq!(builder.take(Transfer::exact(1)), Some(1));
        assert_eq!(
            builder.take(Transfer::fallback(2, note(LossCode::CarrierAxisInferred))),
            Some(2)
        );
        let dropped: Option<i32> = builder.take(Transfer::omitted(note(LossCode::PcurveOmitted)));
        assert_eq!(dropped, None);
        assert_eq!(sink.len(), 2);
    }
}
