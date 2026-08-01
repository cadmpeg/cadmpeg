// SPDX-License-Identifier: Apache-2.0
//! A CAD document together with its decode origin.

use crate::{CadIr, DecodeReport, SourceFidelity};

/// A neutral document and the source information available for later export.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentArtifact {
    /// The format-neutral document.
    pub ir: CadIr,
    /// Whether the document came from neutral JSON or a native decoder.
    pub origin: DocumentOrigin,
}

/// Source information attached to a loaded document.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum DocumentOrigin {
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

impl DocumentArtifact {
    /// Creates an artifact from a neutral document.
    pub const fn neutral(ir: CadIr) -> Self {
        Self {
            ir,
            origin: DocumentOrigin::Neutral,
        }
    }

    /// Creates an artifact from a native decode result.
    pub fn decoded(result: crate::DecodeResult) -> Self {
        Self {
            ir: result.ir,
            origin: DocumentOrigin::Decoded {
                report: result.report,
                fidelity: result.source_fidelity,
            },
        }
    }
}
