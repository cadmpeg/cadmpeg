// SPDX-License-Identifier: Apache-2.0
//! The codec registry: which input formats this build carries, and which of
//! them a byte prefix names.
//!
//! Prefix detection is the cheap candidate stage. It is legitimately
//! ambiguous — a ZIP with no format marker is `Low` for every ZIP-based
//! format at once — and it settles nothing about a dialect. [`crate::identify`]
//! is the stage that opens the container.

use cadmpeg_ir::codec::{Codec, Confidence};

/// Explicit input selection that bypasses content detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedInput {
    /// Force a registered native codec by stable id.
    Codec(&'static str),
    /// Force CADIR JSON parsing.
    Cadir,
}

/// One registered input format: extensions plus optional decoder.
pub struct InputDescriptor {
    /// Stable format identifier when no codec is present (CADIR).
    id: &'static str,
    /// Recognized lowercase filename extensions.
    pub extensions: &'static [&'static str],
    /// Decoder and inspector implementation.
    pub codec: Option<Box<dyn Codec>>,
}

impl InputDescriptor {
    /// Stable format identifier derived from the codec when present.
    pub fn format_id(&self) -> &'static str {
        self.codec.as_ref().map_or(self.id, |codec| codec.id())
    }
}

/// Result of content-based detection.
pub enum DetectionOutcome<'a> {
    /// No decoder recognized the prefix.
    None,
    /// One descriptor won by confidence.
    Detected {
        /// Winning descriptor.
        descriptor: &'a InputDescriptor,
        /// Winning confidence.
        confidence: Confidence,
    },
    /// Multiple descriptors tied at the strongest confidence.
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Candidate descriptors in catalog order.
        candidates: Vec<&'a InputDescriptor>,
    },
}

/// Resolved input after forced selection or content detection.
pub enum ResolvedSource<'a> {
    /// A native codec will decode or inspect the file.
    Native {
        /// Selected codec.
        codec: &'a dyn Codec,
        /// Stable format id.
        format_id: &'static str,
        /// Detection confidence when not forced.
        confidence: Option<Confidence>,
    },
    /// No native codec selected; the caller may parse CADIR JSON.
    Cadir,
    /// No native codec recognized the bytes, and they do not begin as CADIR.
    Unrecognized,
}

/// Failure resolving an input source.
///
/// Each message states the fact and nothing else. The remedy is the caller's:
/// a CLI names its own override flag, and an embedder has no flag to name.
#[derive(Debug, thiserror::Error)]
pub enum ResolveSourceError {
    /// Forced format id is not registered.
    #[error("unsupported input format {0}")]
    UnsupportedFormat(&'static str),
    /// Multiple codecs tied at the strongest confidence.
    #[error("ambiguous {confidence}-confidence input format: {candidates}")]
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Comma-separated candidate format ids.
        candidates: String,
    },
}

/// Source detection and codec lookup.
pub struct InputCatalog {
    descriptors: Vec<InputDescriptor>,
}

impl InputCatalog {
    /// Creates a catalog containing every input format shipped with the CLI.
    pub fn with_builtins() -> Self {
        let catalog = Self {
            descriptors: crate::descriptors::FORMAT_DESCRIPTORS
                .iter()
                .map(|descriptor| InputDescriptor {
                    id: descriptor.id,
                    extensions: descriptor.input_extensions,
                    codec: descriptor.decoder.map(|constructor| constructor()),
                })
                .collect(),
        };
        debug_assert!(catalog
            .descriptors
            .iter()
            .all(|descriptor| !descriptor.extensions.is_empty()));
        catalog
    }

    /// Every descriptor whose codec gives `prefix` more than
    /// [`Confidence::No`], strongest first and in catalog order within a tier.
    ///
    /// The whole candidate set, not the winner. `detect` collapses it to a
    /// selection because loading one file needs one codec; [`crate::identify`]
    /// keeps every candidate because reporting what a file might be is the
    /// question it answers.
    pub fn candidates(&self, prefix: &[u8]) -> Vec<(&InputDescriptor, Confidence)> {
        let mut matches = self
            .descriptors
            .iter()
            .filter_map(|descriptor| {
                let confidence = descriptor.codec.as_deref()?.detect(prefix);
                (confidence > Confidence::No).then_some((descriptor, confidence))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(_, left), (_, right)| right.cmp(left));
        matches
    }

    /// Detects a format without hiding equal-confidence ambiguity.
    pub fn detect(&self, prefix: &[u8]) -> DetectionOutcome<'_> {
        let mut matches = self.candidates(prefix);
        let Some(best_confidence) = matches.iter().map(|(_, confidence)| *confidence).max() else {
            return DetectionOutcome::None;
        };
        matches.retain(|(_, confidence)| *confidence == best_confidence);
        if matches.len() == 1 {
            DetectionOutcome::Detected {
                descriptor: matches[0].0,
                confidence: best_confidence,
            }
        } else {
            DetectionOutcome::Ambiguous {
                confidence: best_confidence,
                candidates: matches
                    .into_iter()
                    .map(|(descriptor, _)| descriptor)
                    .collect(),
            }
        }
    }

    /// Every registered input format, in catalog order.
    pub fn descriptors(&self) -> impl Iterator<Item = &InputDescriptor> {
        self.descriptors.iter()
    }

    /// Returns the descriptor with the stable format identifier.
    pub fn descriptor(&self, id: &str) -> Option<&InputDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.format_id() == id)
    }

    /// Returns the decoder with the stable format identifier.
    pub fn by_id(&self, id: &str) -> Option<&dyn Codec> {
        self.descriptor(id)?.codec.as_deref()
    }

    /// Resolves a forced format or content detection into a source selection.
    ///
    /// Resolves the shared inspect/load source selection. CADIR is selected only
    /// when the prefix begins as a JSON object; unmatched non-JSON stays unrecognized.
    pub fn resolve_source<'a>(
        &'a self,
        prefix: &[u8],
        forced: Option<ForcedInput>,
    ) -> Result<ResolvedSource<'a>, ResolveSourceError> {
        match forced {
            Some(ForcedInput::Codec(id)) => {
                let codec = self
                    .by_id(id)
                    .ok_or(ResolveSourceError::UnsupportedFormat(id))?;
                Ok(ResolvedSource::Native {
                    codec,
                    format_id: id,
                    confidence: None,
                })
            }
            Some(ForcedInput::Cadir) => Ok(ResolvedSource::Cadir),
            None => match self.detect(prefix) {
                DetectionOutcome::None if is_cadir_prefix(prefix) => Ok(ResolvedSource::Cadir),
                DetectionOutcome::None => Ok(ResolvedSource::Unrecognized),
                DetectionOutcome::Detected {
                    descriptor,
                    confidence,
                } => {
                    let codec = descriptor
                        .codec
                        .as_deref()
                        .expect("detected descriptor has codec");
                    Ok(ResolvedSource::Native {
                        codec,
                        format_id: descriptor.format_id(),
                        confidence: Some(confidence),
                    })
                }
                DetectionOutcome::Ambiguous {
                    confidence,
                    candidates,
                } => Err(ResolveSourceError::Ambiguous {
                    confidence,
                    candidates: candidates
                        .iter()
                        .map(|candidate| candidate.format_id())
                        .collect::<Vec<_>>()
                        .join(", "),
                }),
            },
        }
    }
}

/// Whether a byte prefix begins as a CADIR JSON object.
///
/// Accepts UTF-8 BOM and ASCII whitespace before the opening object delimiter.
pub(crate) fn is_cadir_prefix(prefix: &[u8]) -> bool {
    let prefix = prefix.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(prefix);
    prefix.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered format rows retain the input catalog's readable formats
    /// and extension data while adding write capability.
    #[test]
    fn format_rows_preserve_the_readable_input_catalog() {
        let catalog = InputCatalog::with_builtins();
        let rows = crate::support::format_rows(&catalog);
        assert_eq!(rows.len(), catalog.descriptors().count());
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| !row.extensions.is_empty()));
        for row in rows {
            let input = catalog
                .descriptor(row.id)
                .expect("each format row comes from an input descriptor");
            assert_eq!(row.extensions, input.extensions);
        }
    }

    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn markerless_zip_is_explicitly_ambiguous() {
        let catalog = InputCatalog::with_builtins();
        let DetectionOutcome::Ambiguous {
            confidence,
            candidates,
        } = catalog.detect(b"PK\x03\x04 markerless")
        else {
            panic!("markerless ZIP must remain ambiguous");
        };
        assert_eq!(confidence, Confidence::Low);
        let expected = if cfg!(feature = "step") {
            vec!["fcstd", "f3d", "step"]
        } else {
            vec!["fcstd", "f3d"]
        };
        assert_eq!(
            candidates
                .iter()
                .map(|value| value.format_id())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[cfg(feature = "step")]
    #[test]
    fn step_is_registered_as_a_reader() {
        assert!(InputCatalog::with_builtins().by_id("step").is_some());
    }

    #[cfg(feature = "inventor")]
    #[test]
    fn inventor_is_registered_as_a_read_only_family_codec() {
        let catalog = InputCatalog::with_builtins();
        let descriptor = catalog
            .descriptor("inventor")
            .expect("Inventor descriptor exists");
        assert_eq!(descriptor.extensions, ["ipt", "iam"]);
        assert!(descriptor.codec.is_some());
        assert_eq!(descriptor.format_id(), "inventor");
    }

    #[cfg(feature = "iges")]
    #[test]
    fn iges_is_registered_as_a_reader() {
        assert!(InputCatalog::with_builtins().by_id("iges").is_some());
    }

    #[test]
    fn resolve_source_shares_forced_and_detected_paths() {
        let catalog = InputCatalog::with_builtins();
        assert!(matches!(
            catalog
                .resolve_source(b"", Some(ForcedInput::Cadir))
                .unwrap(),
            ResolvedSource::Cadir
        ));
        #[cfg(feature = "step")]
        {
            let ResolvedSource::Native {
                format_id,
                confidence,
                ..
            } = catalog
                .resolve_source(b"", Some(ForcedInput::Codec("step")))
                .unwrap()
            else {
                panic!("forced step must resolve to native");
            };
            assert_eq!(format_id, "step");
            assert!(confidence.is_none());
        }
    }

    #[test]
    fn resolve_source_distinguishes_cadir_from_unrecognized_bytes() {
        let catalog = InputCatalog::with_builtins();
        assert!(matches!(
            catalog
                .resolve_source(b" \n{\"ir_version\": 1}", None)
                .unwrap(),
            ResolvedSource::Cadir
        ));
        assert!(matches!(
            catalog
                .resolve_source(b"\xef\xbb\xbf\t{\"ir_version\": 1}", None)
                .unwrap(),
            ResolvedSource::Cadir
        ));
        assert!(matches!(
            catalog.resolve_source(b"not CAD or JSON", None).unwrap(),
            ResolvedSource::Unrecognized
        ));
    }
}
