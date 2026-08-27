// SPDX-License-Identifier: Apache-2.0
//! Encoder construction at the CLI boundary.
//!
//! One function, total over the output formats this build carries, and no
//! per-format request type. What an encoder writes is a target, and
//! `TargetRequest` carries it; the only thing left for construction to decide
//! is what to do about content the writer cannot represent, which is a policy
//! every format shares.

#[cfg(test)]
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::codec::TargetRequest;
use cadmpeg_ir::codec::{CadirEncoder, Encoder};

use crate::Format;

/// What an encoder does with content it cannot represent exactly.
///
/// The typed form of `--reject-lossy=export` at the construction boundary.
/// Format-independent by construction: a policy that named one codec would be
/// a target flag wearing a policy's name, which is the confusion
/// `--reject-step-losses` embodied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LossPolicy {
    /// Write the representable subset and report the losses.
    #[default]
    Report,
    /// Refuse before writing any byte.
    Reject,
}

/// Builds the encoder for an export format.
///
/// Total and infallible: `Format` is the set of formats this build can write,
/// and nothing an encoder needs at construction can be wrong by then. What can
/// be wrong is the dialect, and that is `plan`'s question, not this one's.
#[cfg_attr(not(feature = "step"), allow(clippy::needless_pass_by_value))]
pub fn build_encoder(format: Format, losses: LossPolicy) -> Box<dyn Encoder> {
    let _ = losses;
    match format {
        Format::Cadir => Box::new(CadirEncoder),
        #[cfg(feature = "step")]
        Format::Step => Box::new(cadmpeg_codec_step::StepCodec {
            options: cadmpeg_codec_step::StepWriteOptions {
                unsupported: match losses {
                    LossPolicy::Report => cadmpeg_codec_step::StepUnsupportedPolicy::Report,
                    LossPolicy::Reject => cadmpeg_codec_step::StepUnsupportedPolicy::Reject,
                },
                ..Default::default()
            },
        }),
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
        Format::Rhino => Box::new(cadmpeg_codec_rhino::RhinoEncoder::default()),
        #[cfg(feature = "iges")]
        Format::Iges => Box::new(cadmpeg_codec_iges::IgesEncoder::default()),
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
        for format in Format::ALL {
            let encoder = build_encoder(*format, LossPolicy::Report);
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
        for format in Format::ALL {
            let encoder = build_encoder(*format, LossPolicy::Report);
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
            let encoder = build_encoder(*format, LossPolicy::Report);
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
            let encoder = build_encoder(*format, LossPolicy::Report);
            assert_eq!(encoder.id(), format.name());
        }
    }
}
