// SPDX-License-Identifier: Apache-2.0
//! Encoder construction at the CLI boundary.
//!
//! Format-specific write options are chosen before calling these helpers, so a
//! Cadir encoder cannot carry STEP options.

use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::codec::TargetRequest;
use cadmpeg_ir::codec::{CadirEncoder, Encoder};

use crate::Format;

/// Format-specific options required to construct an encoder.
#[derive(Debug, Clone)]
pub enum EncoderRequest {
    /// Neutral encoder with no format-specific options.
    Neutral,
    /// STEP header and schema options.
    #[cfg(feature = "step")]
    Step(cadmpeg_codec_step::StepWriteOptions),
    /// IGES specification version.
    #[cfg(feature = "iges")]
    Iges(cadmpeg_codec_iges::IgesWriteOptions),
}

/// Builds the encoder for an export format from an already-selected request.
///
/// Cadir and STEP options are distinct request variants, so they are not
/// representable together.
#[cfg_attr(
    not(any(feature = "step", feature = "iges")),
    allow(clippy::needless_pass_by_value)
)]
pub fn build_encoder(
    format: Format,
    request: EncoderRequest,
) -> Result<Box<dyn Encoder>, CodecError> {
    match format {
        Format::Cadir => {
            require_neutral(&request, "cadir")?;
            Ok(Box::new(CadirEncoder))
        }
        #[cfg(feature = "step")]
        Format::Step => match request {
            EncoderRequest::Step(options) => {
                Ok(Box::new(cadmpeg_codec_step::StepCodec { options }))
            }
            _ => Err(CodecError::Malformed(
                "STEP encoder requires STEP target options".into(),
            )),
        },
        #[cfg(feature = "fcstd")]
        Format::Fcstd => {
            require_neutral(&request, "fcstd")?;
            Ok(Box::new(cadmpeg_codec_freecad::FcstdCodec))
        }
        #[cfg(feature = "f3d")]
        Format::F3d => {
            require_neutral(&request, "f3d")?;
            Ok(Box::new(cadmpeg_codec_f3d::F3dCodec))
        }
        #[cfg(feature = "sldprt")]
        Format::Sldprt => {
            require_neutral(&request, "sldprt")?;
            Ok(Box::new(cadmpeg_codec_sldprt::SldprtCodec))
        }
        // The Rhino writer has no constructor-configured options: the archive
        // version is a target, and `TargetRequest` carries it. A request
        // variant here would be the CLI deciding the version again, which is
        // exactly the defect that turned a Rhino 5 file into archive 80.
        #[cfg(feature = "rhino")]
        Format::Rhino => {
            require_neutral(&request, "rhino")?;
            Ok(Box::new(cadmpeg_codec_rhino::RhinoEncoder::default()))
        }
        #[cfg(feature = "iges")]
        Format::Iges => match request {
            EncoderRequest::Iges(options) => {
                Ok(Box::new(cadmpeg_codec_iges::IgesEncoder::new(options)))
            }
            _ => Err(CodecError::Malformed(
                "IGES encoder requires IGES target options".into(),
            )),
        },
    }
}

// When no option-bearing codec feature is on, Neutral is the only
// EncoderRequest variant: the Err path is absent and Result is always Ok.
#[cfg_attr(
    not(any(feature = "step", feature = "iges")),
    allow(clippy::unnecessary_wraps)
)]
fn require_neutral(request: &EncoderRequest, id: &str) -> Result<(), CodecError> {
    match request {
        EncoderRequest::Neutral => {
            let _ = id;
            Ok(())
        }
        // Non-Neutral variants exist only when their codec features are on.
        // With `--features sldprt` alone, Neutral is the sole variant and this
        // arm must not compile, or `-D unreachable-patterns` fails the gate.
        #[cfg(any(feature = "step", feature = "iges"))]
        _ => Err(CodecError::malformed(format_args!(
            "target options do not belong to the {id} encoder"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every request an encoder catalog can be asked for, checked against the
    /// identity registry and against the catalog's own rules.
    ///
    /// The catalog is a claim about what a format's writer produces, so a typo
    /// in an id, a second default, or a row that names no declared dialect are
    /// all failures of the claim, not of style. CADIR is the one encoder with
    /// no catalog: it writes the neutral document, which has no dialect.
    #[test]
    fn every_catalog_names_declared_dialects_with_one_default() {
        let registry = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/dialects.toml"),
        )
        .expect("the dialect registry is readable");
        for (format, request) in every_exportable_target() {
            let encoder = build_encoder(format, request).expect("encoder builds");
            let targets = encoder.targets();
            let defaults = targets.iter().filter(|target| target.default).count();
            if targets.is_empty() {
                assert_eq!(
                    encoder.id(),
                    "cadir",
                    "{}: only the neutral encoder may have no synthesis catalog",
                    encoder.id()
                );
                continue;
            }
            assert_eq!(
                defaults,
                1,
                "{}: a catalog has exactly one cross-format default",
                encoder.id()
            );
            let mut seen = std::collections::BTreeSet::new();
            for target in targets {
                assert!(
                    target.id.starts_with(&format!("{}:", encoder.id())),
                    "{}: target {} is outside this encoder's own namespace",
                    encoder.id(),
                    target.id
                );
                assert!(
                    registry.contains(&format!("id = \"{}\"", target.id)),
                    "{}: target {} has no row in docs/dialects.toml",
                    encoder.id(),
                    target.id
                );
                assert!(
                    seen.insert(target.id),
                    "{}: target {} is listed twice",
                    encoder.id(),
                    target.id
                );
            }
        }
    }

    /// An explicit id the catalog does not carry is refused, and the refusal
    /// names the catalog so the caller can correct the request.
    #[test]
    fn an_unknown_explicit_target_is_refused_with_the_catalog() {
        for (format, request) in every_exportable_target() {
            let encoder = build_encoder(format, request).expect("encoder builds");
            let error = TargetRequest::Explicit("nonesuch:dialect")
                .check_explicit(encoder.id(), encoder.targets())
                .expect_err("an id outside the catalog is refused");
            let CodecError::UnsupportedTarget {
                requested,
                available,
                ..
            } = &error
            else {
                panic!("{}: expected a target refusal, got {error}", encoder.id());
            };
            assert_eq!(requested, "nonesuch:dialect");
            for target in encoder.targets() {
                assert!(
                    available.contains(target.id),
                    "{}: the refusal omits {}",
                    encoder.id(),
                    target.id
                );
            }
        }
    }

    fn every_exportable_target() -> Vec<(Format, EncoderRequest)> {
        vec![
            (Format::Cadir, EncoderRequest::Neutral),
            #[cfg(feature = "step")]
            (
                Format::Step,
                EncoderRequest::Step(cadmpeg_codec_step::StepWriteOptions::default()),
            ),
            #[cfg(feature = "fcstd")]
            (Format::Fcstd, EncoderRequest::Neutral),
            #[cfg(feature = "f3d")]
            (Format::F3d, EncoderRequest::Neutral),
            #[cfg(feature = "sldprt")]
            (Format::Sldprt, EncoderRequest::Neutral),
            #[cfg(feature = "rhino")]
            (Format::Rhino, EncoderRequest::Neutral),
            #[cfg(feature = "iges")]
            (
                Format::Iges,
                EncoderRequest::Iges(cadmpeg_codec_iges::IgesWriteOptions::default()),
            ),
        ]
    }

    #[test]
    fn every_exportable_format_builds_an_encoder() {
        for (format, request) in every_exportable_target() {
            build_encoder(format, request).expect("encoder builds");
        }
    }
}
