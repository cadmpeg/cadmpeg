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
use cadmpeg_ir::codec::assert_valid_target_catalog;
use cadmpeg_ir::codec::Encoder;
#[cfg(test)]
use cadmpeg_ir::codec::TargetRequest;

use crate::Format;

/// Builds the encoder for an export format.
///
/// Total and infallible: `Format` is the set of formats this build can write,
/// and nothing an encoder needs at construction can be wrong by then. What can
/// be wrong is the dialect, and that is `plan`'s question, not this one's.
pub fn build_encoder(format: Format) -> Box<dyn Encoder> {
    let descriptor = crate::descriptors::by_output(format);
    descriptor
        .encoder
        .expect("every output format has an encoder")()
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
    fn every_catalog_names_declared_dialects_with_at_most_one_default() {
        for format in Format::all() {
            let encoder = build_encoder(format);
            let targets = encoder.targets();
            assert_valid_target_catalog(targets);
            if targets.is_empty() {
                assert_eq!(
                    encoder.id(),
                    "cadir",
                    "{}: only the neutral encoder may have no synthesis catalog",
                    encoder.id()
                );
                continue;
            }
            for target in targets {
                assert!(
                    target
                        .id
                        .as_str()
                        .starts_with(&format!("{}:", encoder.id())),
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
        for format in Format::all() {
            let encoder = build_encoder(format);
            let error = encoder
                .plan(
                    cadmpeg_ir::codec::EncodeInput::new(&ir, None),
                    TargetRequest::Explicit("nonesuch:dialect"),
                )
                .err()
                .expect("an id outside the catalog is refused");
            let CodecError::UnsupportedTarget(refusal) = &error else {
                panic!("{}: expected a target refusal, got {error}", encoder.id());
            };
            assert_eq!(refusal.requested(), Some("nonesuch:dialect"));
            for target in encoder.targets() {
                assert!(
                    refusal
                        .available()
                        .iter()
                        .any(|available| available == target),
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
    /// `registry::tests::compiled_write_catalogs_match_registry_policy` applies
    /// the same rule to every catalog compiled into the current build.
    #[test]
    fn no_target_alias_is_an_output_format_name() {
        for format in Format::all() {
            let encoder = build_encoder(format);
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
        for format in Format::all() {
            let encoder = build_encoder(format);
            assert_eq!(encoder.id(), format.name());
        }
    }
}
