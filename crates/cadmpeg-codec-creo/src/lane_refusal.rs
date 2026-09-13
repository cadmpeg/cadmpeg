// SPDX-License-Identifier: Apache-2.0
//! Sink for carrier records whose lanes the IR carrier refuses.

/// Every carrier record a reader refused, each named by the record that stated
/// it.
///
/// The IR carrier states which lanes disagree; the codec states which source
/// record stated them. The sink is owned by the reader that reports, so no
/// `return None`, `?` or `.ok()?` between a refusal and the report can drop it,
/// and N refusals in one document stay N records.
#[derive(Debug, Default)]
pub(crate) struct LaneRefusals {
    records: Vec<String>,
}

impl LaneRefusals {
    /// An empty sink.
    pub(crate) fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Record one refusal against the record that stated it.
    pub(crate) fn note(
        &mut self,
        record: impl std::fmt::Display,
        error: &cadmpeg_ir::geometry::NurbsError,
    ) {
        self.records.push(format!("{record}: {error}"));
    }

    /// Take every refusal recorded so far, in reader order, and leave the sink
    /// empty.
    pub(crate) fn take_records(&mut self) -> Vec<String> {
        std::mem::take(&mut self.records)
    }

    /// Take every refusal as one error naming each record it read, or `None`
    /// when no record was refused.
    pub(crate) fn take_error(&mut self) -> Option<cadmpeg_core::CodecError> {
        let records = self.take_records();
        (!records.is_empty())
            .then(|| cadmpeg_core::CodecError::malformed(records.join("; ")))
    }
}
