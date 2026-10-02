// SPDX-License-Identifier: Apache-2.0
//! The codec registry: which input formats this build carries, and which of
//! them a byte prefix names.
//!
//! Prefix detection is the cheap candidate stage. It is legitimately
//! ambiguous — a ZIP with no format marker is `Low` for every ZIP-based
//! format at once — and it settles nothing about a dialect.
//! [`crate::resolve_and_inspect_with`] opens the resolved container.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
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
    fn from_tie(
        ctx: &DecodeContext<'_>,
        confidence: Confidence,
        first: FormatId,
        second: FormatId,
        rest: impl Iterator<Item = FormatId>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            confidence,
            candidates: ctx.collect_retained_vec(
                [first, second].into_iter().chain(rest),
                "detection tie candidates",
            )?,
        })
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
    /// Detection resource or codec refusal.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// The forced native descriptor is absent from this catalog.
    #[error("forced input format {0} is not in this catalog")]
    Unregistered(FormatId),
    /// Multiple codecs tied at the strongest confidence.
    #[error(
        "ambiguous {confidence}-confidence input format: {names}",
        confidence = .0.confidence(),
        names = .0.candidates().iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "),
    )]
    Ambiguous(AmbiguousDetection),
}

/// States that every registered format descriptor declares one input extension
/// or more. `InputDescriptor::extensions` reads the same list, so the built-in
/// catalog carries no descriptor without an extension.
const fn every_descriptor_states_an_extension() -> bool {
    let descriptors = crate::descriptors::FORMAT_DESCRIPTORS;
    let mut index = 0;
    while index < descriptors.len() {
        if descriptors[index].input_extensions().is_empty() {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(every_descriptor_states_an_extension());

/// Source detection and codec lookup.
pub struct InputCatalog {
    descriptors: Vec<InputDescriptor>,
}

impl InputCatalog {
    /// Creates a catalog containing every input format shipped with the CLI.
    pub fn with_builtins() -> Self {
        Self {
            descriptors: crate::descriptors::FORMAT_DESCRIPTORS
                .iter()
                .map(|descriptor| InputDescriptor {
                    kind: match &descriptor.kind {
                        crate::descriptors::FormatKind::Neutral { .. } => {
                            InputKind::Neutral { descriptor }
                        }
                        #[cfg(any(
                            feature = "fcstd",
                            feature = "f3d",
                            feature = "inventor",
                            feature = "sldprt",
                            feature = "catia",
                            feature = "creo",
                            feature = "nx",
                            feature = "rhino",
                            feature = "step",
                            feature = "iges",
                            feature = "sat"
                        ))]
                        crate::descriptors::FormatKind::Native(native) => InputKind::Native {
                            native,
                            codec: (native.decoder)(),
                        },
                    },
                })
                .collect(),
        }
    }

    /// Every descriptor whose codec gives `prefix` more than
    /// [`Confidence::No`], strongest first and in catalog order within a tier.
    ///
    /// The whole candidate set, not the winner. `detect` keeps only the
    /// strongest candidate or tied candidates because loading and inspection
    /// need one resolution tier. The detected outcome retains a strongest-tier
    /// ambiguity.
    /// Replaces `output`; the caller holds the returned reservation while the
    /// candidate vector is live.
    pub fn candidates<'catalog, 'ctx>(
        &'catalog self,
        ctx: &'ctx DecodeContext<'_>,
        prefix: View<'_>,
        output: &mut Vec<(&'catalog dyn Codec, Confidence)>,
    ) -> Result<ScopedReservation<'ctx>, CodecError> {
        let (matches, storage) = ctx.with_scoped_storage("detection candidates", || {
            let mut matches = Vec::new();
            for descriptor in &self.descriptors {
                ctx.charge_work(1, "detect catalog entry")?;
                let Some(codec) = descriptor.codec() else {
                    continue;
                };
                let confidence = codec.detect(ctx, prefix)?;
                if confidence > Confidence::No {
                    ctx.push_retained_vec(
                        &mut matches,
                        (codec, confidence),
                        "detection candidates",
                    )?;
                }
            }
            ctx.stable_sort_by(
                &mut matches,
                |(_, left), (_, right)| right.cmp(left),
                |_| 0,
                "sort detection candidates",
            )?;
            Ok::<_, CodecError>(matches)
        })?;
        *output = matches;
        Ok(storage)
    }

    /// Detects a format without hiding equal-confidence ambiguity.
    pub fn detect(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<DetectionOutcome<'_>, CodecError> {
        let mut matches = Vec::new();
        let _storage = self.candidates(ctx, prefix, &mut matches)?;
        ctx.charge_work(
            u64_from_index(matches.len()) * 2,
            "select strongest detection",
        )?;
        let Some(best_confidence) = matches.iter().map(|(_, confidence)| *confidence).max() else {
            return Ok(DetectionOutcome::None);
        };
        matches.retain(|(_, confidence)| *confidence == best_confidence);
        Ok(match matches.as_slice() {
            [] => DetectionOutcome::None,
            [(codec, _)] => DetectionOutcome::Detected {
                codec: *codec,
                confidence: best_confidence,
            },
            [(first, _), (second, _), rest @ ..] => {
                DetectionOutcome::Ambiguous(AmbiguousDetection::from_tie(
                    ctx,
                    best_confidence,
                    first.id(),
                    second.id(),
                    rest.iter().map(|(codec, _)| codec.id()),
                )?)
            }
        })
    }

    /// Every registered input format, in catalog order.
    pub fn descriptors(&self) -> impl Iterator<Item = &InputDescriptor> {
        self.descriptors.iter()
    }

    /// Resolves a forced format or content detection into a source selection.
    ///
    /// Resolves the shared inspect/load source selection. CADIR is selected only
    /// when the prefix begins as a JSON object; unmatched non-JSON stays unrecognized.
    pub fn resolve_source<'a>(
        &'a self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
        forced: Option<ForcedInput>,
    ) -> Result<ResolvedSource<'a>, ResolveSourceError> {
        ctx.charge_work(
            u64_from_index(self.descriptors.len()),
            "resolve input source",
        )?;
        ctx.charge_work(u64_from_index(prefix.window().len()), "detect CADIR prefix")?;
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
            None => match self.detect(ctx, prefix)? {
                DetectionOutcome::None if is_cadir_prefix(prefix.window()) => {
                    Ok(ResolvedSource::Cadir)
                }
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
    use super::{ForcedInput, InputCatalog, ResolvedSource};

    #[test]
    fn detection_candidates_keep_workspace_and_slot_refusals_typed() {
        for dimension in [
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            if dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes {
                policy.limits.max_materialized_bytes = 0;
            } else {
                policy.limits.max_collection_items = 0;
            }
            let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                b"PK\x03\x04",
                &arena,
                &policy,
            )
            .expect("root");
            let catalog = InputCatalog::with_builtins();
            let mut candidates = Vec::new();
            assert!(matches!(catalog.candidates(&ctx, root, &mut candidates),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
        }
    }

    #[test]
    fn detection_propagates_work_refusal_before_catalog_scan() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(b"PK\x03\x04", &arena, &policy)
                .expect("root");
        let catalog = InputCatalog::with_builtins();
        assert!(
            matches!(catalog.detect(&ctx, root), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    fn resolve<'a>(
        catalog: &'a InputCatalog,
        prefix: &[u8],
        forced: Option<ForcedInput>,
    ) -> Result<ResolvedSource<'a>, super::ResolveSourceError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            prefix,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("root");
        let result = catalog.resolve_source(&ctx, root, forced);
        ctx.finish_session().expect("session");
        result
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
                .descriptors()
                .find(|descriptor| descriptor.format_id() == row.id)
                .expect("each format row comes from an input descriptor");
            assert_eq!(row.extensions, input.extensions());
        }
    }

    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn markerless_zip_is_explicitly_ambiguous() {
        use super::DetectionOutcome;
        use cadmpeg_ir::codec::Confidence;

        let catalog = InputCatalog::with_builtins();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"PK\x03\x04 markerless",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("root");
        let DetectionOutcome::Ambiguous(tie) = catalog.detect(&ctx, root).expect("detect") else {
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
        assert!(InputCatalog::with_builtins()
            .descriptors()
            .any(|descriptor| descriptor.format_id().as_str() == "step"
                && descriptor.codec().is_some()));
    }

    #[cfg(feature = "inventor")]
    #[test]
    fn inventor_is_registered_as_a_read_only_family_codec() {
        use cadmpeg_ir::codec::FormatId;

        let catalog = InputCatalog::with_builtins();
        let descriptor = catalog
            .descriptors()
            .find(|descriptor| descriptor.format_id().as_str() == "inventor")
            .expect("Inventor descriptor exists");
        assert_eq!(descriptor.extensions(), ["ipt", "iam"]);
        assert!(descriptor.codec().is_some());
        assert_eq!(descriptor.format_id(), FormatId::new("inventor"));
    }

    #[cfg(feature = "iges")]
    #[test]
    fn iges_is_registered_as_a_reader() {
        assert!(InputCatalog::with_builtins()
            .descriptors()
            .any(|descriptor| descriptor.format_id().as_str() == "iges"
                && descriptor.codec().is_some()));
    }

    #[test]
    fn resolve_source_shares_forced_and_detected_paths() {
        let catalog = InputCatalog::with_builtins();
        assert!(matches!(
            resolve(&catalog, b"", Some(ForcedInput::Cadir)).unwrap(),
            ResolvedSource::Cadir
        ));
        #[cfg(feature = "step")]
        {
            use super::Selection;
            use cadmpeg_ir::codec::FormatId;

            let ResolvedSource::Native { codec, selection } = resolve(
                &catalog,
                b"",
                Some(
                    crate::forced_input("step")
                        .expect("embedded registry loads")
                        .expect("step is registered"),
                ),
            )
            .unwrap() else {
                panic!("forced step must resolve to native");
            };
            assert_eq!(codec.id(), FormatId::new("step"));
            assert_eq!(selection, Selection::Forced);
        }
    }

    #[cfg(feature = "step")]
    #[test]
    fn forced_descriptor_absent_from_catalog_returns_an_error() {
        use super::ResolveSourceError;
        use cadmpeg_ir::codec::FormatId;

        let catalog = InputCatalog {
            descriptors: Vec::new(),
        };
        let forced = crate::forced_input("step")
            .expect("embedded registry loads")
            .expect("step is registered");
        assert!(matches!(resolve(&catalog, b"", Some(forced)),
            Err(ResolveSourceError::Unregistered(id)) if id == FormatId::new("step")));
    }

    #[test]
    fn resolve_source_distinguishes_cadir_from_unrecognized_bytes() {
        let catalog = InputCatalog::with_builtins();
        assert!(matches!(
            resolve(&catalog, b" \n{\"ir_version\": 1}", None).unwrap(),
            ResolvedSource::Cadir
        ));
        assert!(matches!(
            resolve(&catalog, b"\xef\xbb\xbf\t{\"ir_version\": 1}", None).unwrap(),
            ResolvedSource::Cadir
        ));
        assert!(matches!(
            resolve(&catalog, b"not CAD or JSON", None).unwrap(),
            ResolvedSource::Unrecognized
        ));
    }
}
