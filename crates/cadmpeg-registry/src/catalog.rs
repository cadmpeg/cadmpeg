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
#[derive(Debug, Clone, Copy)]
pub enum ForcedInput {
    /// Force the registered native codec witnessed by this descriptor.
    Codec(&'static crate::descriptors::NativeDescriptor),
    /// Force CADIR JSON parsing.
    Cadir,
}

impl PartialEq for ForcedInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Codec(left), Self::Codec(right)) => std::ptr::eq(*left, *right),
            (Self::Cadir, Self::Cadir) => true,
            (Self::Codec(_), Self::Cadir) | (Self::Cadir, Self::Codec(_)) => false,
        }
    }
}

impl Eq for ForcedInput {}

/// A built input descriptor's neutral or native capability.
enum InputKind {
    Neutral {
        descriptor: &'static crate::descriptors::FormatDescriptor,
    },
    Native {
        native: &'static crate::descriptors::NativeDescriptor,
        codec: Box<dyn Codec>,
    },
}

/// One registered input format.
pub struct InputDescriptor {
    kind: InputKind,
}

impl InputDescriptor {
    /// Stable format identifier derived from the native codec or neutral
    /// descriptor.
    pub fn format_id(&self) -> &'static str {
        match &self.kind {
            InputKind::Neutral { descriptor } => descriptor.id(),
            InputKind::Native { codec, .. } => codec.id(),
        }
    }

    /// Recognized lowercase filename extensions.
    pub fn extensions(&self) -> &'static [&'static str] {
        match &self.kind {
            InputKind::Neutral { descriptor } => descriptor.input_extensions(),
            InputKind::Native { native, .. } => native.input_extensions(),
        }
    }

    /// Decoder and inspector implementation for a native format.
    pub fn codec(&self) -> Option<&dyn Codec> {
        match &self.kind {
            InputKind::Neutral { .. } => None,
            InputKind::Native { codec, .. } => Some(codec.as_ref()),
        }
    }
}

/// Result of content-based detection.
///
/// A detected candidate carries its codec directly: only descriptors that
/// have one take part in detection, so a detection without a codec cannot
/// be expressed.
pub enum DetectionOutcome<'a> {
    /// No decoder recognized the prefix.
    None,
    /// One codec won by confidence.
    Detected {
        /// Winning codec.
        codec: &'a dyn Codec,
        /// Winning confidence.
        confidence: Confidence,
    },
    /// Multiple codecs tied at the strongest confidence.
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Candidate format ids in catalog order.
        candidates: Vec<&'static str>,
    },
}

/// How a native codec was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Content detection named the codec at this confidence.
    Detected {
        /// Detection confidence of the byte prefix.
        confidence: Confidence,
    },
    /// The caller forced the codec; no detection ran.
    Forced,
}

impl Selection {
    /// Detection confidence, or `None` when the codec was forced.
    #[must_use]
    pub const fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Detected { confidence } => Some(confidence),
            Self::Forced => None,
        }
    }
}

/// Resolved input after forced selection or content detection.
pub enum ResolvedSource<'a> {
    /// A native codec will decode or inspect the file.
    Native {
        /// Selected codec.
        codec: &'a dyn Codec,
        /// How the codec was selected.
        selection: Selection,
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
    /// Multiple codecs tied at the strongest confidence.
    #[error("ambiguous {confidence}-confidence input format: {names}", names = .candidates.join(", "))]
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Candidate format ids in detection order.
        candidates: Vec<&'static str>,
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
                    kind: match &descriptor.kind {
                        crate::descriptors::FormatKind::Neutral { .. } => {
                            InputKind::Neutral { descriptor }
                        }
                        crate::descriptors::FormatKind::Native(native) => InputKind::Native {
                            native,
                            codec: (native.decoder)(),
                        },
                    },
                })
                .collect(),
        };
        debug_assert!(catalog
            .descriptors
            .iter()
            .all(|descriptor| !descriptor.extensions().is_empty()));
        catalog
    }

    /// Every descriptor whose codec gives `prefix` more than
    /// [`Confidence::No`], strongest first and in catalog order within a tier.
    ///
    /// The whole candidate set, not the winner. `detect` keeps only the
    /// strongest candidate or tied candidates because loading and inspection
    /// need one resolution tier; [`crate::identify`] exposes that detected
    /// outcome without discarding a strongest-tier ambiguity.
    pub fn candidates(&self, prefix: &[u8]) -> Vec<(&dyn Codec, Confidence)> {
        let mut matches = self
            .descriptors
            .iter()
            .filter_map(|descriptor| {
                let codec = descriptor.codec()?;
                let confidence = codec.detect(prefix);
                (confidence > Confidence::No).then_some((codec, confidence))
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
            let codec = matches[0].0;
            DetectionOutcome::Detected {
                codec,
                confidence: best_confidence,
            }
        } else {
            DetectionOutcome::Ambiguous {
                confidence: best_confidence,
                candidates: matches.into_iter().map(|(codec, _)| codec.id()).collect(),
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
        self.descriptor(id)?.codec()
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
            Some(ForcedInput::Codec(native)) => {
                let codec = self
                    .descriptors
                    .iter()
                    .find_map(|input| match &input.kind {
                        InputKind::Native {
                            native: candidate,
                            codec,
                        } if std::ptr::eq(*candidate, native) => Some(codec.as_ref()),
                        InputKind::Neutral { .. } | InputKind::Native { .. } => None,
                    })
                    .expect("forced native descriptors come from this built-in catalog");
                Ok(ResolvedSource::Native {
                    codec,
                    selection: Selection::Forced,
                })
            }
            Some(ForcedInput::Cadir) => Ok(ResolvedSource::Cadir),
            None => match self.detect(prefix) {
                DetectionOutcome::None if is_cadir_prefix(prefix) => Ok(ResolvedSource::Cadir),
                DetectionOutcome::None => Ok(ResolvedSource::Unrecognized),
                DetectionOutcome::Detected { codec, confidence } => Ok(ResolvedSource::Native {
                    codec,
                    selection: Selection::Detected { confidence },
                }),
                DetectionOutcome::Ambiguous {
                    confidence,
                    candidates,
                } => Err(ResolveSourceError::Ambiguous {
                    confidence,
                    candidates,
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
        let rows = crate::views::format_rows(&catalog);
        assert_eq!(rows.len(), catalog.descriptors().count());
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| !row.extensions.is_empty()));
        for row in rows {
            let input = catalog
                .descriptor(row.id)
                .expect("each format row comes from an input descriptor");
            assert_eq!(row.extensions, input.extensions());
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
        assert_eq!(candidates, expected);
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
        assert_eq!(descriptor.extensions(), ["ipt", "iam"]);
        assert!(descriptor.codec().is_some());
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
            let ResolvedSource::Native { codec, selection } = catalog
                .resolve_source(
                    b"",
                    Some(crate::forced_input("step").expect("step is registered")),
                )
                .unwrap()
            else {
                panic!("forced step must resolve to native");
            };
            assert_eq!(codec.id(), "step");
            assert_eq!(selection, Selection::Forced);
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
