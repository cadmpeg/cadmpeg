// SPDX-License-Identifier: Apache-2.0
//! The codec registry: which input formats this build carries, and which of
//! them a byte prefix names.
//!
//! Prefix detection is the cheap candidate stage. It is legitimately
//! ambiguous — a ZIP with no format marker is `Low` for every ZIP-based
//! format at once — and it settles nothing about a dialect. [`crate::identify`]
//! is the stage that opens the container.

use cadmpeg_ir::codec::{Codec, Confidence, FormatId};

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
    pub fn format_id(&self) -> FormatId {
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

/// A strongest-confidence tie between at least two candidate formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousDetection {
    confidence: Confidence,
    candidates: Vec<FormatId>,
}

impl AmbiguousDetection {
    /// Admits a tie with at least two candidate formats.
    pub fn new(
        confidence: Confidence,
        candidates: Vec<FormatId>,
    ) -> Result<Self, ResolveSourceError> {
        if candidates.len() < 2 {
            return Err(ResolveSourceError::InsufficientCandidates(candidates.len()));
        }
        Ok(Self {
            confidence,
            candidates,
        })
    }

    fn from_tie(
        confidence: Confidence,
        first: FormatId,
        second: FormatId,
        rest: impl Iterator<Item = FormatId>,
    ) -> Self {
        Self {
            confidence,
            candidates: [first, second].into_iter().chain(rest).collect(),
        }
    }

    /// Returns the confidence shared by the candidates.
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Returns candidate format ids in catalog order.
    pub fn candidates(&self) -> &[FormatId] {
        &self.candidates
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
    Ambiguous(AmbiguousDetection),
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
    /// The forced native descriptor is absent from this catalog.
    #[error("forced input format {0} is not in this catalog")]
    Unregistered(FormatId),
    /// An ambiguity request contains fewer than two candidates.
    #[error("candidates: ambiguity requires at least two formats, received {0}")]
    InsufficientCandidates(usize),
    /// Multiple codecs tied at the strongest confidence.
    #[error(
        "ambiguous {confidence}-confidence input format: {names}",
        confidence = .0.confidence(),
        names = .0.candidates().iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "),
    )]
    Ambiguous(AmbiguousDetection),
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
        match matches.as_slice() {
            [] => DetectionOutcome::None,
            [(codec, _)] => DetectionOutcome::Detected {
                codec: *codec,
                confidence: best_confidence,
            },
            [(first, _), (second, _), rest @ ..] => {
                DetectionOutcome::Ambiguous(AmbiguousDetection::from_tie(
                    best_confidence,
                    first.id(),
                    second.id(),
                    rest.iter().map(|(codec, _)| codec.id()),
                ))
            }
        }
    }

    /// Every registered input format, in catalog order.
    pub fn descriptors(&self) -> impl Iterator<Item = &InputDescriptor> {
        self.descriptors.iter()
    }

    /// Returns the descriptor whose format id is spelled `id`.
    pub fn descriptor(&self, id: &str) -> Option<&InputDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.format_id().as_str() == id)
    }

    /// Returns the decoder whose format id is spelled `id`.
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
                    .ok_or(ResolveSourceError::Unregistered(native.id()))?;
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
                DetectionOutcome::Ambiguous(tie) => Err(ResolveSourceError::Ambiguous(tie)),
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

    #[test]
    fn ambiguity_requires_at_least_two_candidates() {
        for candidates in [Vec::new(), vec![FormatId::new("step")]] {
            assert!(matches!(
                AmbiguousDetection::new(Confidence::Low, candidates),
                Err(ResolveSourceError::InsufficientCandidates(_))
            ));
        }
        let tie = AmbiguousDetection::new(
            Confidence::Low,
            vec![FormatId::new("step"), FormatId::new("f3d")],
        )
        .unwrap();
        assert_eq!(
            tie.candidates(),
            &[FormatId::new("step"), FormatId::new("f3d")]
        );
        assert_eq!(tie.confidence(), Confidence::Low);
    }

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
                .descriptor(row.id.as_str())
                .expect("each format row comes from an input descriptor");
            assert_eq!(row.extensions, input.extensions());
        }
    }

    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn markerless_zip_is_explicitly_ambiguous() {
        let catalog = InputCatalog::with_builtins();
        let DetectionOutcome::Ambiguous(tie) = catalog.detect(b"PK\x03\x04 markerless") else {
            panic!("markerless ZIP must remain ambiguous");
        };
        assert_eq!(tie.confidence(), Confidence::Low);
        let expected = if cfg!(feature = "step") {
            vec!["fcstd", "f3d", "step"]
        } else {
            vec!["fcstd", "f3d"]
        };
        assert_eq!(
            tie.candidates()
                .iter()
                .map(|id| id.as_str())
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
        assert_eq!(descriptor.extensions(), ["ipt", "iam"]);
        assert!(descriptor.codec().is_some());
        assert_eq!(descriptor.format_id(), FormatId::new("inventor"));
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
            assert_eq!(codec.id(), FormatId::new("step"));
            assert_eq!(selection, Selection::Forced);
        }
    }

    #[cfg(feature = "step")]
    #[test]
    fn forced_descriptor_absent_from_catalog_returns_an_error() {
        let catalog = InputCatalog {
            descriptors: Vec::new(),
        };
        let forced = crate::forced_input("step").expect("step is registered");
        assert!(matches!(catalog.resolve_source(b"", Some(forced)),
            Err(ResolveSourceError::Unregistered(id)) if id == FormatId::new("step")));
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
