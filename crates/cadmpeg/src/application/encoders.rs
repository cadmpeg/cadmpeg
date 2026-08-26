// SPDX-License-Identifier: Apache-2.0
//! Encoder construction at the CLI boundary.
//!
//! Format-specific write options are chosen before calling these helpers, so a
//! Cadir encoder cannot carry STEP options.

use cadmpeg_core::CodecError;
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
    /// Rhino archive version.
    #[cfg(feature = "rhino")]
    Rhino(cadmpeg_codec_rhino::RhinoArchiveVersion),
    /// IGES specification version.
    #[cfg(feature = "iges")]
    Iges(cadmpeg_codec_iges::IgesWriteOptions),
}

/// Builds the encoder for an export format from an already-selected request.
///
/// Cadir and STEP options are distinct request variants, so they are not
/// representable together.
#[cfg_attr(
    not(any(feature = "step", feature = "rhino", feature = "iges")),
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
        #[cfg(feature = "rhino")]
        Format::Rhino => match request {
            EncoderRequest::Rhino(version) => {
                Ok(Box::new(cadmpeg_codec_rhino::RhinoEncoder::new(version)))
            }
            _ => Err(CodecError::Malformed(
                "Rhino encoder requires Rhino target options".into(),
            )),
        },
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
    not(any(feature = "step", feature = "rhino", feature = "iges")),
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
        #[cfg(any(feature = "step", feature = "rhino", feature = "iges"))]
        _ => Err(CodecError::malformed(format_args!(
            "target options do not belong to the {id} encoder"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_exportable_format_builds_an_encoder() {
        for (format, request) in [
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
            (
                Format::Rhino,
                EncoderRequest::Rhino(cadmpeg_codec_rhino::RhinoArchiveVersion::V8),
            ),
            #[cfg(feature = "iges")]
            (
                Format::Iges,
                EncoderRequest::Iges(cadmpeg_codec_iges::IgesWriteOptions::default()),
            ),
        ] {
            build_encoder(format, request).expect("encoder builds");
        }
    }
}
