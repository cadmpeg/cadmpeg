// SPDX-License-Identifier: Apache-2.0
//! Encoder construction at the CLI boundary.
//!
//! One function, total over the output formats this build carries, and no
//! per-format request type. What an encoder writes is a target, and
//! `TargetRequest` carries it. Export-loss rejection is an application decision
//! over the completed plan, not an encoder-construction option.

#[cfg(test)]
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::codec::default_target;
#[cfg(test)]
use cadmpeg_ir::codec::TargetRequest;
use cadmpeg_ir::codec::{CadirEncoder, Encoder, TargetDescriptor};

use crate::Format;

/// Builds the encoder for an export format.
///
/// Total and infallible: `Format` is the set of formats this build can write,
/// and nothing an encoder needs at construction can be wrong by then. What can
/// be wrong is the dialect, and that is `plan`'s question, not this one's.
pub fn build_encoder(format: Format) -> Box<dyn Encoder> {
    match format {
        Format::Cadir => Box::new(CadirEncoder),
        #[cfg(feature = "step")]
        Format::Step => Box::new(cadmpeg_codec_step::StepCodec::default()),
        #[cfg(feature = "fcstd")]
        Format::Fcstd => Box::new(cadmpeg_codec_freecad::FcstdCodec),
        #[cfg(feature = "f3d")]
        Format::F3d => Box::new(cadmpeg_codec_f3d::F3dCodec),
        #[cfg(feature = "sldprt")]
        Format::Sldprt => Box::new(cadmpeg_codec_sldprt::SldprtCodec),
        // Neither the Rhino nor the IGES encoder takes a constructed version.
        // The archive version and the specification version are targets, and
        // `TargetRequest` carries them; deciding one here is what silently
        // rewrote a Rhino 5 file as archive 80.
        #[cfg(feature = "rhino")]
        Format::Rhino => Box::new(cadmpeg_codec_rhino::RhinoEncoder),
        #[cfg(feature = "iges")]
        Format::Iges => Box::new(cadmpeg_codec_iges::IgesEncoder),
    }
}

/// The synthesis catalog of an export format's encoder in this build.
///
/// The catalog is static per format and independent of every constructor
/// knob: per-codec options configure how a target is written, never which
/// ones exist. The unit and default values below exist only to reach the
/// instance method required by [`Encoder`].
#[must_use]
pub fn write_targets(format: Format) -> &'static [TargetDescriptor] {
    match format {
        Format::Cadir => Encoder::targets(&CadirEncoder),
        #[cfg(feature = "step")]
        Format::Step => Encoder::targets(&cadmpeg_codec_step::StepCodec::default()),
        #[cfg(feature = "fcstd")]
        Format::Fcstd => Encoder::targets(&cadmpeg_codec_freecad::FcstdCodec),
        #[cfg(feature = "f3d")]
        Format::F3d => Encoder::targets(&cadmpeg_codec_f3d::F3dCodec),
        #[cfg(feature = "sldprt")]
        Format::Sldprt => Encoder::targets(&cadmpeg_codec_sldprt::SldprtCodec),
        #[cfg(feature = "rhino")]
        Format::Rhino => Encoder::targets(&cadmpeg_codec_rhino::RhinoEncoder),
        #[cfg(feature = "iges")]
        Format::Iges => Encoder::targets(&cadmpeg_codec_iges::IgesEncoder),
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
        for format in Format::ALL {
            let encoder = build_encoder(*format);
            let targets = encoder.targets();
            let default = default_target(targets);
            if targets.is_empty() {
                assert_eq!(
                    encoder.id(),
                    "cadir",
                    "{}: only the neutral encoder may have no synthesis catalog",
                    encoder.id()
                );
                continue;
            }
            assert!(
                default.is_some(),
                "{}: a catalog has exactly one cross-format default",
                encoder.id()
            );
            for target in targets {
                assert!(
                    target.id.starts_with(&format!("{}:", encoder.id())),
                    "{}: target {} is outside this encoder's own namespace",
                    encoder.id(),
                    target.id
                );
            }
        }
    }

    /// An explicit id the catalog does not carry is refused, and the refusal
    /// names the catalog so the caller can correct the request.
    ///
    /// Asked of `plan`, not of a request-level helper: catalog membership is
    /// the first step of every encoder's resolution, so an empty document is
    /// enough to reach it and the assertion covers the resolution each encoder
    /// actually runs.
    #[test]
    fn an_unknown_explicit_target_is_refused_with_the_catalog() {
        let ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        for format in Format::ALL {
            let encoder = build_encoder(*format);
            let error = encoder
                .plan(
                    cadmpeg_ir::codec::EncodeInput::new(&ir, None),
                    TargetRequest::Explicit("nonesuch:dialect"),
                )
                .err()
                .expect("an id outside the catalog is refused");
            let CodecError::UnsupportedTarget {
                requested,
                available,
                ..
            } = &error
            else {
                panic!("{}: expected a target refusal, got {error}", encoder.id());
            };
            assert_eq!(requested.as_deref(), Some("nonesuch:dialect"));
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

    /// No target alias collides with an output-format name.
    ///
    /// The Rust half of the checker rule that keeps `--to VALUE` unambiguous:
    /// a bare value is read as a format first and as a dialect alias second,
    /// so an alias that is also a format name would be unreachable.
    /// `scripts/check-dialect-support.py` proves the same thing across every
    /// catalog in the tree, including the ones this build does not compile.
    #[test]
    fn no_target_alias_is_an_output_format_name() {
        for format in Format::ALL {
            let encoder = build_encoder(*format);
            for target in encoder.targets() {
                for alias in target.aliases {
                    assert!(
                        Format::from_name(alias).is_none(),
                        "{}: alias {alias} of {} is also an output format name",
                        encoder.id(),
                        target.id
                    );
                }
            }
        }
    }

    #[test]
    fn every_exportable_format_builds_an_encoder() {
        for format in Format::ALL {
            let encoder = build_encoder(*format);
            assert_eq!(encoder.id(), format.name());
        }
    }
}
