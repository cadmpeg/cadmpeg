// SPDX-License-Identifier: Apache-2.0
//! Body-relative exact-read spans for the opt-in read ledger.
//!
//! [`collect`] re-runs the same admission predicates the decoder runs over
//! one Parasolid stream body and records the byte range each admitted record
//! interprets. Topology spans name admitted records. Analytic spans name
//! every carrier the carrier scan admits. Spline spans name the wrapper,
//! descriptor, and semantic array values of every admitted NURBS carrier; the
//! wrapper, descriptor, and array spans of one carrier are separate entries.
//! Array value spans cover the semantic prefix only: physical float slots past
//! the descriptor knot counts are not recorded. The decode path never calls
//! this module, so collection cannot change decode results, admission, or
//! selection.

/// One interpreted byte range: body-relative offset and length, record tag,
/// and source record id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawSpan {
    pub(crate) offset: usize,
    pub(crate) length: usize,
    pub(crate) tag: u8,
    pub(crate) record: u32,
}

impl RawSpan {
    pub(crate) fn at(offset: usize, length: usize, tag: u8, record: u32) -> Option<Self> {
        if length == 0 {
            return None;
        }
        Some(Self {
            offset,
            length,
            tag,
            record,
        })
    }
}

/// Every exact-read span of one Parasolid stream body, ordered by offset.
/// `plain` reads a plain stream (spec §3.3).
pub(crate) fn collect(stream_body: &[u8], plain: bool) -> Vec<RawSpan> {
    let mut spans = Vec::new();
    spans.extend(super::analytic_spans(stream_body));
    spans.extend(super::topology::topology_spans(stream_body, plain));
    spans.extend(super::spline::spline_spans(stream_body));
    spans.sort_by(|left, right| {
        (left.offset, left.length, left.tag, left.record).cmp(&(
            right.offset,
            right.length,
            right.tag,
            right.record,
        ))
    });
    spans
}
