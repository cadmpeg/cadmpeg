// SPDX-License-Identifier: Apache-2.0
//! A CAD document together with its load origin.

use cadmpeg_ir::{report::decode::DecodeReport, CadIr, DecodeResult, SourceFidelity};
use cadmpeg_registry::Selection;

/// A neutral document and the source information available for later export.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadedDocument {
    /// The format-neutral document.
    pub(crate) ir: CadIr,
    /// Whether the document came from neutral JSON or a native decoder.
    pub(crate) origin: LoadOrigin,
    /// Physical byte length measured while loading the input artifact.
    pub(crate) input_bytes: u64,
}

/// Source information attached to a loaded document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoadOrigin {
    /// The document was loaded without native decode metadata.
    Neutral,
    /// Decode metadata restored from a neutral document sidecar.
    Restored {
        /// What the decoder transferred and omitted.
        report: DecodeReport,
        /// Decode-time annotations and retained native records.
        fidelity: Box<SourceFidelity>,
    },
    /// The document was produced by a native decoder.
    Decoded {
        /// How the native decoder was selected.
        selection: Selection,
        /// What the decoder transferred and omitted.
        report: DecodeReport,
        /// Decode-time annotations and retained native records.
        fidelity: Box<SourceFidelity>,
    },
}

impl LoadedDocument {
    /// Creates a document from a neutral CADIR payload.
    pub(crate) const fn neutral(ir: CadIr, input_bytes: u64) -> Self {
        Self {
            ir,
            origin: LoadOrigin::Neutral,
            input_bytes,
        }
    }

    /// Creates a document from a native decode result.
    pub(crate) fn decoded(result: DecodeResult, selection: Selection, input_bytes: u64) -> Self {
        let (ir, report, fidelity) = result.into_parts();
        Self {
            ir,
            origin: LoadOrigin::Decoded {
                report,
                fidelity: Box::new(fidelity),
                selection,
            },
            input_bytes,
        }
    }

    /// Creates a neutral load whose matching sidecar restores decode origin.
    pub(crate) fn restored(
        ir: CadIr,
        report: DecodeReport,
        fidelity: SourceFidelity,
        input_bytes: u64,
    ) -> Self {
        Self {
            ir,
            origin: LoadOrigin::Restored {
                report,
                fidelity: Box::new(fidelity),
            },
            input_bytes,
        }
    }

    /// Returns the native decode report, when this document has decoded origin.
    pub(crate) const fn decode_report(&self) -> Option<&DecodeReport> {
        match &self.origin {
            LoadOrigin::Neutral => None,
            LoadOrigin::Decoded { report, .. } | LoadOrigin::Restored { report, .. } => {
                Some(report)
            }
        }
    }

    /// Returns source fidelity, when this document has decoded origin.
    pub(crate) const fn fidelity(&self) -> Option<&SourceFidelity> {
        match &self.origin {
            LoadOrigin::Neutral => None,
            LoadOrigin::Decoded { fidelity, .. } | LoadOrigin::Restored { fidelity, .. } => {
                Some(fidelity)
            }
        }
    }
}
