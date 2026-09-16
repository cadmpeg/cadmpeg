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

/// The source record and refusal sink shared by one carrier conversion.
pub(crate) struct LaneRefusalContext<'record, 'sink> {
    pub(crate) record: &'record dyn std::fmt::Display,
    pub(crate) refusals: &'sink mut LaneRefusals,
}

impl<'record, 'sink> LaneRefusalContext<'record, 'sink> {
    /// Pair a source record with the sink that owns its refusal report.
    pub(crate) fn new(
        record: &'record dyn std::fmt::Display,
        refusals: &'sink mut LaneRefusals,
    ) -> Self {
        Self { record, refusals }
    }
}

impl LaneRefusals {
    /// An empty sink.
    pub(crate) fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Record one refusal against the record that stated it, with the reason
    /// the reader refused it.
    pub(crate) fn note(&mut self, record: impl std::fmt::Display, reason: &dyn std::fmt::Display) {
        self.records.push(format!("{record}: {reason}"));
    }

    /// Take every refusal recorded so far, in reader order, and leave the sink
    /// empty.
    pub(crate) fn take_records(&mut self) -> Vec<String> {
        std::mem::take(&mut self.records)
    }
}
