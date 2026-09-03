// SPDX-License-Identifier: Apache-2.0
//! A CAD document together with its load origin.

use cadmpeg_ir::{CadIr, DecodeReport, DecodeResult, SourceFidelity};
use cadmpeg_registry::Selection;

/// A neutral document and the source information available for later export.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedDocument {
    /// The format-neutral document.
    pub ir: CadIr,
    /// Whether the document came from neutral JSON or a native decoder.
    pub origin: LoadOrigin,
    selection: LoadSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadSelection {
    Neutral,
    Native(Selection),
}

/// Source information attached to a loaded document.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum LoadOrigin {
    /// The document was loaded without native decode metadata.
    Neutral,
    /// The document was produced by a native decoder.
    Decoded {
        /// What the decoder transferred and omitted.
        report: DecodeReport,
        /// Decode-time annotations and retained native records.
        fidelity: SourceFidelity,
    },
}

impl LoadedDocument {
    /// Creates a document from a neutral CADIR payload.
    pub const fn neutral(ir: CadIr) -> Self {
        Self {
            ir,
            origin: LoadOrigin::Neutral,
            selection: LoadSelection::Neutral,
        }
    }

    /// Creates a document from a native decode result.
    pub fn decoded(result: DecodeResult, selection: Selection) -> Self {
        let (ir, report, fidelity) = result.into_parts();
        Self {
            ir,
            origin: LoadOrigin::Decoded { report, fidelity },
            selection: LoadSelection::Native(selection),
        }
    }

    /// Creates a neutral load whose matching sidecar restores decode origin.
    pub fn restored(ir: CadIr, report: DecodeReport, fidelity: SourceFidelity) -> Self {
        Self {
            ir,
            origin: LoadOrigin::Decoded { report, fidelity },
            selection: LoadSelection::Neutral,
        }
    }

    /// Returns how a native codec was selected for this load.
    pub const fn selection(&self) -> Option<Selection> {
        match self.selection {
            LoadSelection::Neutral => None,
            LoadSelection::Native(selection) => Some(selection),
        }
    }

    /// Returns the native decode report, when this document has decoded origin.
    pub const fn decode_report(&self) -> Option<&DecodeReport> {
        match &self.origin {
            LoadOrigin::Neutral => None,
            LoadOrigin::Decoded { report, .. } => Some(report),
        }
    }

    /// Returns source fidelity, when this document has decoded origin.
    pub const fn fidelity(&self) -> Option<&SourceFidelity> {
        match &self.origin {
            LoadOrigin::Neutral => None,
            LoadOrigin::Decoded { fidelity, .. } => Some(fidelity),
        }
    }
}
