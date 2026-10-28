// SPDX-License-Identifier: Apache-2.0
//! Opt-in exact-read ledger for `.sldprt` Parasolid stream bodies.
//!
//! The ledger records exact read spans (body-relative offset and length, tag,
//! source record id, SHA-256 of the exact bytes) for successfully interpreted
//! byte ranges: topology records, compact analytic carriers, and NURBS spline
//! carriers. NURBS wrapper, descriptor, and array spans are separate entries;
//! array value spans cover the semantic prefix only, so physical float slots
//! past the descriptor knot counts are not recorded. Compact array records
//! carry no tag byte; their spans use tag zero.
//!
//! [`ExactReadLedger::collect`] re-runs the decode admission predicates over
//! one decompressed Parasolid stream body without changing them. The normal
//! decode path never collects, so an unused ledger changes no decode result,
//! admission, or stream selection. [`EXACT_READ_LEDGER_VERSION`] versions the
//! sidecar contract independently of the IR version.

/// Version of the exact-read sidecar contract this codec writes.
pub const EXACT_READ_LEDGER_VERSION: u32 = 1;

/// One interpreted byte range of a Parasolid stream body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactReadSpan {
    /// Body-relative offset of the first interpreted byte.
    pub offset: usize,
    /// Length of the interpreted range in bytes.
    pub length: usize,
    /// Record tag byte. Compact array records carry no tag byte; their spans
    /// use zero.
    pub tag: u8,
    /// Source record id (stream-local attribute id).
    pub record: u16,
    /// SHA-256 of the exact span bytes.
    pub sha256: [u8; 32],
}

impl ExactReadSpan {
    /// Lowercase hexadecimal form of the span digest.
    pub fn sha256_hex(&self) -> String {
        self.sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .concat()
    }
}

/// Versioned sidecar naming the exact-read spans of one Parasolid stream
/// body, in offset order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactReadLedger {
    /// Sidecar contract version ([`EXACT_READ_LEDGER_VERSION`]).
    pub version: u32,
    /// Name of the Parasolid stream the body came from, empty when the
    /// caller collects a bare body.
    pub stream: String,
    /// Interpreted spans in offset order.
    pub spans: Vec<ExactReadSpan>,
}

impl ExactReadLedger {
    /// Collect the exact-read spans of one decompressed Parasolid stream
    /// body. The body is read-only input: collection admits the same records
    /// the decoder admits and changes no decode state.
    pub fn collect(stream: &str, body: &[u8]) -> Self {
        let mut spans = Vec::new();
        for raw in crate::brep::read_ledger::collect(body) {
            let Some(end) = raw.offset.checked_add(raw.length) else {
                continue;
            };
            let Some(bytes) = body.get(raw.offset..end) else {
                continue;
            };
            spans.push(ExactReadSpan {
                offset: raw.offset,
                length: raw.length,
                tag: raw.tag,
                record: raw.record,
                sha256: cadmpeg_ir::hash::sha256(bytes),
            });
        }
        Self {
            version: EXACT_READ_LEDGER_VERSION,
            stream: stream.to_owned(),
            spans,
        }
    }

    /// Whether the body admitted no ledger span.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The count of ledger spans.
    pub fn len(&self) -> usize {
        self.spans.len()
    }
}

#[cfg(test)]
mod tests;
