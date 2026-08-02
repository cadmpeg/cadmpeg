// SPDX-License-Identifier: Apache-2.0
//! Built-in format descriptors, detection, encoding, and native validation.

use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::codec::{CadirEncoder, Codec, Confidence, Encoder};
use cadmpeg_ir::{CadIr, Finding};

/// User-visible capabilities of one format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether neutral geometry can be exported to the format.
    pub geometry_export: bool,
    /// Whether decode-time source fidelity can be replayed.
    pub fidelity_replay: bool,
}

/// Format-specific options accepted by encoder factories.
#[derive(Debug, Clone)]
pub enum TargetOptions {
    /// No format-specific target options.
    Neutral,
    /// STEP header and schema options.
    Step(cadmpeg_step::StepWriteOptions),
    /// Rhino archive version.
    Rhino(cadmpeg_codec_rhino::RhinoArchiveVersion),
}

type EncoderFactory = fn(TargetOptions) -> Result<Box<dyn Encoder>, CodecError>;
type NativeValidator = fn(&CadIr) -> Vec<Finding>;

/// All runtime-owned behavior and metadata for one file format.
pub struct FormatDescriptor {
    /// Stable format identifier.
    pub id: &'static str,
    /// Human-readable format name.
    pub display_name: &'static str,
    /// Recognized lowercase filename extensions.
    pub extensions: &'static [&'static str],
    /// Tie-break priority among equal detection confidences.
    pub detection_priority: i32,
    /// User-visible format capabilities.
    pub capabilities: Capabilities,
    /// Decoder and inspector implementation.
    pub codec: Option<Box<dyn Codec>>,
    /// Encoder constructor.
    encoder: Option<EncoderFactory>,
    /// Format-native IR validator.
    native_validator: Option<NativeValidator>,
}

/// Result of content-based detection.
pub enum DetectionOutcome<'a> {
    /// No decoder recognized the prefix.
    None,
    /// One descriptor won by confidence and priority.
    Detected {
        /// Winning descriptor.
        descriptor: &'a FormatDescriptor,
        /// Winning confidence.
        confidence: Confidence,
    },
    /// Multiple descriptors tied at the strongest confidence and priority.
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Candidate descriptors in registry order.
        candidates: Vec<&'a FormatDescriptor>,
    },
}

/// Native formats available to the CLI.
pub struct Registry {
    descriptors: Vec<FormatDescriptor>,
}

impl Registry {
    /// Creates a registry containing every format shipped with the CLI.
    pub fn with_builtins() -> Self {
        let registry = Self {
            descriptors: vec![
                descriptor(
                    "fcstd",
                    "FreeCAD",
                    &["fcstd"],
                    Some(Box::new(cadmpeg_codec_freecad::FcstdCodec)),
                    Some(neutral_fcstd),
                    Some(cadmpeg_codec_freecad::validate_native),
                    true,
                ),
                descriptor(
                    "f3d",
                    "Autodesk Fusion",
                    &["f3d", "f3z"],
                    Some(Box::new(cadmpeg_codec_f3d::F3dCodec)),
                    Some(neutral_f3d),
                    Some(cadmpeg_codec_f3d::validate::validate_native),
                    true,
                ),
                descriptor(
                    "sldprt",
                    "SolidWorks Part",
                    &["sldprt"],
                    Some(Box::new(cadmpeg_codec_sldprt::SldprtCodec)),
                    Some(neutral_sldprt),
                    Some(cadmpeg_codec_sldprt::validate_native),
                    true,
                ),
                descriptor(
                    "catia",
                    "CATIA V5 Part",
                    &["catpart"],
                    Some(Box::new(cadmpeg_codec_catia::CatiaCodec)),
                    None,
                    None,
                    false,
                ),
                descriptor(
                    "creo",
                    "Creo Parametric Part",
                    &["prt"],
                    Some(Box::new(cadmpeg_codec_creo::CreoCodec)),
                    None,
                    None,
                    false,
                ),
                descriptor(
                    "nx",
                    "Siemens NX Part",
                    &["prt"],
                    Some(Box::new(cadmpeg_codec_nx::NxCodec)),
                    None,
                    None,
                    false,
                ),
                descriptor(
                    "rhino",
                    "Rhino 3DM",
                    &["3dm"],
                    Some(Box::new(cadmpeg_codec_rhino::RhinoCodec)),
                    Some(rhino),
                    None,
                    false,
                ),
                descriptor(
                    "step",
                    "STEP",
                    &["step", "stp"],
                    Some(Box::new(cadmpeg_step::StepCodec::default())),
                    Some(step),
                    None,
                    false,
                ),
                descriptor(
                    "iges",
                    "IGES",
                    &["iges", "igs"],
                    Some(Box::new(cadmpeg_codec_iges::IgesCodec)),
                    None,
                    None,
                    false,
                ),
                descriptor(
                    "cadir",
                    "CADIR JSON",
                    &["cadir", "json"],
                    None,
                    Some(neutral_cadir),
                    None,
                    false,
                ),
            ],
        };
        debug_assert!(registry.descriptors.iter().all(|descriptor| {
            !descriptor.display_name.is_empty()
                && !descriptor.extensions.is_empty()
                && (!descriptor.capabilities.fidelity_replay || descriptor.encoder.is_some())
                && (!descriptor.capabilities.geometry_export || descriptor.encoder.is_some())
        }));
        registry
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
        let best_priority = matches
            .iter()
            .map(|(descriptor, _)| descriptor.detection_priority)
            .max()
            .expect("nonempty detection candidates");
        matches.retain(|(descriptor, _)| descriptor.detection_priority == best_priority);
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
    pub fn descriptor(&self, id: &str) -> Option<&FormatDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }

    /// Returns the decoder with the stable format identifier.
    pub fn by_id(&self, id: &str) -> Option<&dyn Codec> {
        self.descriptor(id)?.codec.as_deref()
    }

    /// Constructs an encoder after validating its format-specific options.
    pub fn encoder(
        &self,
        id: &str,
        options: TargetOptions,
    ) -> Option<Result<Box<dyn Encoder>, CodecError>> {
        self.descriptor(id)?.encoder.map(|factory| factory(options))
    }

    /// Runs each registered native validator against its owning namespace.
    pub fn validate_native(&self, ir: &CadIr) -> Vec<Finding> {
        self.descriptors
            .iter()
            .filter(|descriptor| ir.native.namespace(descriptor.id).is_some())
            .filter_map(|descriptor| descriptor.native_validator)
            .flat_map(|validator| validator(ir))
            .collect()
    }
}

fn descriptor(
    id: &'static str,
    display_name: &'static str,
    extensions: &'static [&'static str],
    codec: Option<Box<dyn Codec>>,
    encoder: Option<EncoderFactory>,
    native_validator: Option<NativeValidator>,
    fidelity_replay: bool,
) -> FormatDescriptor {
    FormatDescriptor {
        id,
        display_name,
        extensions,
        detection_priority: 0,
        capabilities: Capabilities {
            geometry_export: matches!(id, "fcstd" | "f3d" | "sldprt" | "rhino" | "step"),
            fidelity_replay,
        },
        codec,
        encoder,
        native_validator,
    }
}

fn require_neutral(options: TargetOptions, id: &str) -> Result<(), CodecError> {
    let neutral = matches!(options, TargetOptions::Neutral);
    drop(options);
    if neutral {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "target options do not belong to the {id} encoder"
        )))
    }
}

fn neutral_fcstd(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    require_neutral(options, "fcstd")?;
    Ok(Box::new(cadmpeg_codec_freecad::FcstdCodec))
}

fn neutral_f3d(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    require_neutral(options, "f3d")?;
    Ok(Box::new(cadmpeg_codec_f3d::F3dCodec))
}

fn neutral_sldprt(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    require_neutral(options, "sldprt")?;
    Ok(Box::new(cadmpeg_codec_sldprt::SldprtCodec))
}

fn neutral_cadir(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    require_neutral(options, "cadir")?;
    Ok(Box::new(CadirEncoder))
}

fn step(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    match options {
        TargetOptions::Step(options) => Ok(Box::new(cadmpeg_step::StepCodec { options })),
        _ => Err(CodecError::Malformed(
            "STEP encoder requires STEP target options".into(),
        )),
    }
}

fn rhino(options: TargetOptions) -> Result<Box<dyn Encoder>, CodecError> {
    let version = match &options {
        TargetOptions::Rhino(version) => *version,
        _ => Err(CodecError::Malformed(
            "Rhino encoder requires Rhino target options".into(),
        ))?,
    };
    drop(options);
    Ok(Box::new(cadmpeg_codec_rhino::RhinoEncoder::new(version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_exportable_format_has_one_encoder_factory() {
        let registry = Registry::with_builtins();
        for id in ["cadir", "step", "fcstd", "f3d", "sldprt", "rhino"] {
            assert!(registry
                .descriptor(id)
                .is_some_and(|value| value.encoder.is_some()));
        }
    }

    #[test]
    fn markerless_zip_is_explicitly_ambiguous() {
        let registry = Registry::with_builtins();
        let DetectionOutcome::Ambiguous {
            confidence,
            candidates,
        } = registry.detect(b"PK\x03\x04 markerless")
        else {
            panic!("markerless ZIP must remain ambiguous");
        };
        assert_eq!(confidence, Confidence::Low);
        assert_eq!(
            candidates.iter().map(|value| value.id).collect::<Vec<_>>(),
            ["fcstd", "f3d"]
        );
    }

    #[test]
    fn step_is_registered_as_a_reader() {
        assert!(Registry::with_builtins().by_id("step").is_some());
    }
}
