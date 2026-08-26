// SPDX-License-Identifier: Apache-2.0
//! Input detection and native-validator catalogs for the CLI.

use cadmpeg_ir::codec::{Codec, Confidence};
use cadmpeg_ir::{CadIr, Finding};

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
}

/// Failure resolving an input source.
#[derive(Debug, thiserror::Error)]
pub enum ResolveSourceError {
    /// Forced format id is not registered.
    #[error("unsupported input format {0}")]
    UnsupportedFormat(&'static str),
    /// Multiple codecs tied at the strongest confidence.
    #[error("ambiguous {confidence}-confidence input format: {candidates}; pass --input-format")]
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
            descriptors: vec![
                #[cfg(feature = "fcstd")]
                input(
                    "fcstd",
                    &["fcstd"],
                    Some(Box::new(cadmpeg_codec_freecad::FcstdCodec)),
                ),
                #[cfg(feature = "f3d")]
                input(
                    "f3d",
                    &["f3d", "f3z"],
                    Some(Box::new(cadmpeg_codec_f3d::F3dCodec)),
                ),
                #[cfg(feature = "inventor")]
                input(
                    "inventor",
                    &["ipt", "iam"],
                    Some(Box::new(cadmpeg_codec_inventor::InventorCodec)),
                ),
                #[cfg(feature = "sldprt")]
                input(
                    "sldprt",
                    &["sldprt"],
                    Some(Box::new(cadmpeg_codec_sldprt::SldprtCodec)),
                ),
                #[cfg(feature = "catia")]
                input(
                    "catia",
                    &["catpart"],
                    Some(Box::new(cadmpeg_codec_catia::CatiaCodec)),
                ),
                #[cfg(feature = "creo")]
                input(
                    "creo",
                    &["prt"],
                    Some(Box::new(cadmpeg_codec_creo::CreoCodec)),
                ),
                #[cfg(feature = "nx")]
                input("nx", &["prt"], Some(Box::new(cadmpeg_codec_nx::NxCodec))),
                #[cfg(feature = "rhino")]
                input(
                    "rhino",
                    &["3dm"],
                    Some(Box::new(cadmpeg_codec_rhino::RhinoCodec)),
                ),
                #[cfg(feature = "step")]
                input(
                    "step",
                    &["step", "stp"],
                    Some(Box::new(cadmpeg_codec_step::StepCodec::default())),
                ),
                #[cfg(feature = "iges")]
                input(
                    "iges",
                    &["iges", "igs"],
                    Some(Box::new(cadmpeg_codec_iges::IgesCodec)),
                ),
                #[cfg(feature = "sat")]
                input(
                    "sat",
                    &["sat", "sab", "smt", "smb"],
                    Some(Box::new(cadmpeg_codec_sat::SatCodec)),
                ),
                input("cadir", &["cadir", "json"], None),
            ],
        };
        debug_assert!(catalog
            .descriptors
            .iter()
            .all(|descriptor| !descriptor.extensions.is_empty()));
        catalog
    }

    /// Detects a format without hiding equal-confidence ambiguity.
    pub fn detect(&self, prefix: &[u8]) -> DetectionOutcome<'_> {
        let mut matches = self
            .descriptors
            .iter()
            .filter_map(|descriptor| {
                let confidence = descriptor.codec.as_deref()?.detect(prefix);
                (confidence > Confidence::No).then_some((descriptor, confidence))
            })
            .collect::<Vec<_>>();
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
    /// Shared by inspect and load. Cadir fallback means "no native codec"; the
    /// caller decides whether JSON parsing or a hard error is appropriate.
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
                DetectionOutcome::None => Ok(ResolvedSource::Cadir),
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

type NativeValidator = fn(&CadIr) -> Vec<Finding>;

/// Maps native namespace ids to codec-owned validator functions.
pub struct NativeValidatorCatalog {
    entries: Vec<(&'static str, NativeValidator)>,
}

impl NativeValidatorCatalog {
    /// Registers the four native validators shipped with the CLI.
    pub fn with_builtins() -> Self {
        Self {
            entries: vec![
                #[cfg(feature = "fcstd")]
                ("fcstd", cadmpeg_codec_freecad::validate_native),
                #[cfg(feature = "f3d")]
                ("f3d", cadmpeg_codec_f3d::validate_native),
                #[cfg(feature = "inventor")]
                ("inventor", cadmpeg_codec_inventor::validate_native),
                #[cfg(feature = "sldprt")]
                ("sldprt", cadmpeg_codec_sldprt::validate_native),
            ],
        }
    }

    /// Stable namespace ids that have a registered validator.
    #[cfg(test)]
    fn namespaces(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|(namespace, _)| *namespace)
    }

    /// Runs every validator whose namespace is present on the document.
    pub fn validate(&self, ir: &CadIr) -> Vec<Finding> {
        self.entries
            .iter()
            .filter(|(namespace, _)| ir.native.namespace(namespace).is_some())
            .flat_map(|(_, validator)| validator(ir))
            .collect()
    }
}

fn input(
    id: &'static str,
    extensions: &'static [&'static str],
    codec: Option<Box<dyn Codec>>,
) -> InputDescriptor {
    InputDescriptor {
        id,
        extensions,
        codec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;

    #[cfg(all(
        feature = "fcstd",
        feature = "f3d",
        feature = "inventor",
        feature = "sldprt"
    ))]
    #[test]
    fn native_validator_catalog_registers_the_four_shipped_validators() {
        let catalog = NativeValidatorCatalog::with_builtins();
        let mut namespaces = catalog.namespaces().collect::<Vec<_>>();
        namespaces.sort_unstable();
        assert_eq!(namespaces, ["f3d", "fcstd", "inventor", "sldprt"]);
    }

    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn native_validator_catalog_invokes_two_validators_for_two_namespaces() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_fcstd(ir: &CadIr) -> Vec<Finding> {
            let _ = ir;
            CALLS.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }
        fn counting_f3d(ir: &CadIr) -> Vec<Finding> {
            let _ = ir;
            CALLS.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }

        let catalog = NativeValidatorCatalog {
            entries: vec![("fcstd", counting_fcstd), ("f3d", counting_f3d)],
        };
        CALLS.store(0, Ordering::SeqCst);
        let mut ir = CadIr::empty(Units::default());
        let _ = ir.native.namespace_mut("fcstd");
        let _ = ir.native.namespace_mut("f3d");
        let _ = catalog.validate(&ir);
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);

        CALLS.store(0, Ordering::SeqCst);
        let mut none = CadIr::empty(Units::default());
        let _ = none.native.namespace_mut("absent");
        let _ = catalog.validate(&none);
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
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
}
