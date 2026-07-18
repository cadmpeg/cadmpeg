// SPDX-License-Identifier: Apache-2.0
//! Loss-recording value resolution.
//!
//! [`Transfer`] is used at selected construction boundaries where a fallback or
//! omission must add its [`LossNote`] before yielding a value. It does not
//! account for construction paths that do not use this type.

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

/// The value-level transfer outcome for one field or entity.
///
/// The payload is reachable only through [`resolve`](Transfer::resolve), which
/// forwards a [`Fallback`](Transfer::Fallback) or [`Dropped`](Transfer::Dropped)
/// note to a [`LossSink`] as it hands the value back (or, for `Dropped`,
/// records the note and yields nothing). [`Exact`](Transfer::Exact) carries a
/// faithful record-to-entity value, `Fallback` carries a
/// defaulted field or a resolver's substituted carrier/axis, and `Dropped`
/// carries an unsupported concept omitted by a decoder or a writer. `Exact`,
/// `fallback`, and `omitted` have ergonomic constructors.
#[must_use = "a Transfer carries a possible loss; resolve it through a LossSink"]
pub enum Transfer<T> {
    /// Read verbatim from the source with no substitution.
    Exact(T),
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
    /// This is the only accessor: an [`Exact`](Transfer::Exact) value passes
    /// through untouched, a
    /// [`Fallback`](Transfer::Fallback) value passes through *after* its note is
    /// recorded, and a [`Dropped`](Transfer::Dropped) outcome records its note
    /// and yields [`None`]. A silent fallback or drop is therefore unwritable —
    /// the value cannot be reached without surrendering the note.
    pub fn resolve(self, sink: &mut impl LossSink) -> Option<T> {
        match self {
            Transfer::Exact(value) => Some(value),
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

/// Resolves several [`Transfer`] values into one loss sink.
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
    fn exact_records_no_loss() {
        let mut sink: Vec<LossNote> = Vec::new();
        assert_eq!(Transfer::exact(7).resolve(&mut sink), Some(7));
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
