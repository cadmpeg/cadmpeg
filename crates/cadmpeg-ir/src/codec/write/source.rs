// SPDX-License-Identifier: Apache-2.0
//! Omission accounting for the source inventory outside typed content.

use std::collections::BTreeMap;

use crate::report::loss::LossNote;

use super::{loss::ExportLossCode, EncodeInput};

pub(super) fn omission_losses(input: EncodeInput<'_>, format: &str) -> Vec<LossNote> {
    if format == "cadir" {
        return Vec::new();
    }
    let Some(fidelity) = input.fidelity else {
        return Vec::new();
    };
    let mut records = BTreeMap::<_, Vec<_>>::new();
    for (id, layout) in fidelity.record_layout() {
        if let Some(end) = layout.offset().checked_add(layout.byte_len()) {
            records
                .entry(layout.stream())
                .or_default()
                .push((layout.offset(), end, id.as_str()));
        }
    }
    let mut losses = Vec::new();
    for (stream, coverage) in fidelity.b1_spans() {
        let entries = records.entry(stream.as_str()).or_default();
        entries.sort_unstable();
        let mut next = 0;
        let mut active = BTreeMap::new();
        for span in coverage.spans() {
            let range = span.range();
            while let Some(&(start, end, id)) = entries.get(next) {
                if start >= range.end {
                    break;
                }
                if end > start && end > range.start {
                    active.insert(id, end);
                }
                next += 1;
            }
            active.retain(|_, end| *end > range.start);
            let ids = active.keys().copied().collect::<Vec<_>>();
            losses.push(ExportLossCode::SourceUndecodedOmitted.note(format!(
                "the {format} write omits undecoded source span stream={stream} offset={} length={} classes={} record(s): {}",
                range.start,
                range.end - range.start,
                span.classes(),
                ids.join(", "),
            )));
        }
    }
    losses
}
