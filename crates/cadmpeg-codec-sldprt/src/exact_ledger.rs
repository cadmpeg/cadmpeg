// SPDX-License-Identifier: Apache-2.0
//! Opt-in exact-read ledger for `.sldprt` Parasolid stream bodies.
//!
//! The ledger records exact read spans (body-relative offset and length, tag,
//! source record id, SHA-256 of the exact bytes) for successfully interpreted
//! byte ranges: topology records, compact analytic carriers, and spatial NURBS
//! carriers.
//!
//! A stream that the decoder reads from its node sequence names one span over
//! the complete range of each node a node reader admits; the tag is the node
//! type and the record is the node index. A NURBS carrier names its wrapper,
//! descriptor, control-value, and knot nodes.
//!
//! A stream that the decoder scans names the records the scan admits; the tag
//! is the record tag byte and the record is its attribute id. NURBS wrapper,
//! descriptor, and array spans are separate entries; array value spans cover
//! the semantic prefix only, so physical float slots past the descriptor knot
//! counts are not recorded. Compact array records carry no tag byte; their
//! spans use tag zero.
//!
//! [`crate::SldprtCodec::decode_with_exact_read_ledger`] applies both rules as
//! the decoder does. [`ExactReadLedger::collect`] and
//! [`ExactReadLedger::collect_plain`] apply the scan rule to one body. The
//! normal decode path never collects, so an unused ledger changes no decode
//! result, admission, or stream selection. [`EXACT_READ_LEDGER_VERSION`] versions the
//! sidecar contract independently of the IR version.

/// Version of the exact-read sidecar contract this codec writes.
pub const EXACT_READ_LEDGER_VERSION: u32 = 2;

/// One interpreted byte range of a Parasolid stream body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactReadSpan {
    /// Body-relative offset of the first interpreted byte.
    pub offset: usize,
    /// Length of the interpreted range in bytes.
    pub length: usize,
    /// Node type of a node span, or record tag byte of a scanned span.
    /// Scanned compact array records carry no tag byte; their spans use
    /// zero.
    pub tag: u8,
    /// Source record id (stream-local attribute id).
    pub record: u32,
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
    /// Collect the scan-rule spans of one decompressed Parasolid stream body:
    /// the records the scan admits. The body is read-only input, and
    /// collection changes no decode state.
    pub fn collect(stream: &str, body: &[u8]) -> Self {
        Self::collect_stream(stream, body, false)
    }

    /// Collect the exact-read spans of one decompressed plain stream body
    /// (spec §3.3), as [`Self::collect`] does for a partition or deltas body.
    pub fn collect_plain(stream: &str, body: &[u8]) -> Self {
        Self::collect_stream(stream, body, true)
    }

    fn collect_stream(stream: &str, body: &[u8], plain: bool) -> Self {
        Self::from_raw(stream, body, crate::brep::read_ledger::collect(body, plain))
    }

    /// The ledger of `body` over the body-relative spans `raw`.
    pub(crate) fn from_raw(
        stream: &str,
        body: &[u8],
        raw: Vec<crate::brep::read_ledger::RawSpan>,
    ) -> Self {
        let mut spans = Vec::new();
        for raw in raw {
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
