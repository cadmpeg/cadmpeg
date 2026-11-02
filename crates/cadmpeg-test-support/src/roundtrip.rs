// SPDX-License-Identifier: Apache-2.0
//! Round-trip test entry points, one per write path.
//!
//! A codec that retains its source bytes answers "write this document" two ways:
//! it patches the retained bytes when the document is unchanged since the
//! decode, and it runs the writer otherwise. A test spelled decode, encode,
//! compare bytes reads the same at the call site in both cases. Over an
//! unedited document the patch reproduces the fixture byte for byte; the test
//! then proves the writer round-trips the document it was given.
//!
//! There are exactly three entry points here, one per way the question can be
//! asked:
//!
//! - [`patched_round_trip_holds`] covers the unchanged-document patch. It asserts
//!   the patch happened and that the patched bytes reproduce the fixture. It
//!   exercises the writer end to end on an unedited document.
//! - [`semantic_roundtrip`] covers the writer with the baseline taken away, so
//!   the retained-bytes shortcut is not available, and asserts synthesis.
//! - [`mutation_roundtrip`] covers the writer with the baseline left in place
//!   and a value edited, which is how a user reaches it. It asserts the write
//!   path the caller named.
//!
//! Each names the path it covers, so choosing wrong is an assertion-time event.
//! There is deliberately no path-agnostic entry point. One would be the trap
//! this module exists to close.
//!
//! ## Why the third one is not the trap
//!
//! Withholding fidelity and editing a value both deny the shortcut, but they are
//! not the same test and one does not subsume the other. Withholding fidelity
//! asks a codec what it does when it cannot establish whether its retained bytes
//! are current; a codec that needs the baseline to write at all answers by
//! refusing, and its writer is then unreachable from that helper. Editing a
//! value asks what the writer produces for a document that differs from the
//! retained bytes in one known place, which is the only question whose answer
//! can be checked against the edit. `Fusion` needs both: it refuses the first
//! and patches the second.
//!
//! This crate also owns the golden harness in [`crate::golden`]. Both helpers
//! are test-only and must not be depended on by production builds.

use cadmpeg_core::CodecError;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::export::{ExportReport, WritePath};
use cadmpeg_ir::SourceFidelity;

/// Encodes an unedited decode of `fixture` and asserts the encoder patched the
/// retained bytes, producing `fixture` again.
///
/// Proves container fidelity (retained image, integrity, matching baseline)
/// through the semantic writer; the verbatim-replay shortcut is unavailable
/// without a document baseline.
///
/// # Panics
///
/// Panics when either decode or the encode fails, when the encoder took any
/// path other than [`WritePath::Patched`], or when the patched bytes decode
/// to a document that disagrees with the original on a written arena.
pub fn patched_round_trip_holds<C>(codec: &C, label: &str, fixture: &[u8]) -> ExportReport
where
    C: Codec + Encoder,
{
    let decoded = crate::EditableDecodeResult::from(
        crate::golden::decode(
            codec,
            &mut std::io::Cursor::new(fixture.to_vec()),
            &DecodeOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{label}: decode failed: {error}")),
    );
    let plan = Encoder::plan(
        codec,
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Inherit,
    )
    .unwrap_or_else(|error| panic!("{label}: plan failed: {error}"));
    let path = plan.report().write_path().clone();
    let mut written = Vec::new();
    let report = plan
        .write_to(&mut written)
        .unwrap_or_else(|error| panic!("{label}: write failed: {error}"));
    assert!(
        matches!(path, WritePath::Patched { .. }),
        "{label}: this fixture was expected to patch its retained bytes, but the encoder took the {path} path"
    );
    let redecoded = crate::golden::decode(
        codec,
        &mut std::io::Cursor::new(written),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: patched output failed to decode: {error}"));
    crate::contract::assert_written_arenas_agree(&report, decoded.ir(), redecoded.ir());
    report
}

/// Encodes an unedited document with its fidelity, asserts the encoder took the
/// patched path, and asserts the patched bytes decode to a valid document
/// with the same model entity counts.
///
/// The patched bytes need not be byte-identical: the writer normalizes layout
/// (for example how a rewritten configuration is framed, which changes the
/// identity the next decode mints), so only content is compared.
///
/// # Panics
///
/// Panics when planning or writing fails, when the encoder took any path other
/// than [`WritePath::Patched`], when the patched bytes fail to decode or to
/// validate, or when a model arena count differs.
pub fn patched_write_preserves_model<C>(
    codec: &C,
    label: &str,
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> ExportReport
where
    C: Codec + Encoder,
{
    let plan = Encoder::plan(
        codec,
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Inherit,
    )
    .unwrap_or_else(|error| panic!("{label}: plan failed: {error}"));
    let path = plan.report().write_path().clone();
    assert!(
        matches!(path, WritePath::Patched { .. }),
        "{label}: expected the patched path, but the encoder took the {path} path"
    );
    let mut written = Vec::new();
    let report = plan
        .write_to(&mut written)
        .unwrap_or_else(|error| panic!("{label}: write failed: {error}"));
    let redecoded = codec
        .decode(
            &mut std::io::Cursor::new(written),
            &DecodeOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{label}: patched output failed to decode: {error}"));
    let validation = cadmpeg_ir::validate_neutral(redecoded.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "{label}: patched output failed validation: {:#?}",
        validation.findings
    );
    assert_eq!(
        model_counts(ir),
        model_counts(redecoded.ir()),
        "{label}: patched round trip changed model entity counts"
    );
    report
}

/// Entity counts per model arena, for patched round-trip comparison.
fn model_counts(ir: &CadIr) -> Vec<(&'static str, usize)> {
    let model = &ir.model;
    vec![
        ("bodies", model.bodies.len()),
        ("faces", model.faces.len()),
        ("edges", model.edges.len()),
        ("vertices", model.vertices.len()),
        ("coedges", model.coedges.len()),
        ("loops", model.loops.len()),
        ("points", model.points.len()),
        ("curves", model.curves.len()),
        ("surfaces", model.surfaces.len()),
        ("features", model.features.len()),
        ("occurrences", model.occurrences.len()),
        ("configurations", model.configurations.len()),
    ]
}

/// What the writer did when it was denied the retained-bytes shortcut.
#[derive(Debug)]
pub enum SemanticOutcome {
    /// The writer ran and produced bytes.
    Written {
        /// The encoder's report, whose `write_path` is not
        /// [`WritePath::VerbatimReplay`].
        report: Box<ExportReport>,
        /// The bytes the writer produced.
        bytes: Vec<u8>,
    },
    /// The encoder refused. Codecs that need a baseline refuse when it is gone.
    Refused {
        /// The refusal error.
        error: CodecError,
    },
}

/// Decodes `fixture`, withholds source fidelity, encodes it, and
/// hands the outcome to `check`.
///
/// Withholding fidelity selects synthesis without modifying the document.
///
/// # Panics
///
/// Panics when decoding or writing fails, or when the encoder consumes retained
/// bytes despite having no source fidelity.
pub fn semantic_roundtrip<C>(
    codec: &C,
    label: &str,
    fixture: &[u8],
    check: impl FnOnce(&SemanticOutcome),
) where
    C: Codec + Encoder,
{
    let decoded = crate::golden::decode(
        codec,
        &mut std::io::Cursor::new(fixture.to_vec()),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: decode failed: {error}"));
    let (ir, _decode_report, _source_fidelity) = decoded.into_parts();
    // Plan borrows the document until write completes.
    let written = match Encoder::plan(codec, EncodeInput::new(&ir, None), TargetRequest::Inherit) {
        Ok(plan) => {
            let path = plan.report().write_path().clone();
            let mut bytes = Vec::new();
            let report = plan
                .write_to(&mut bytes)
                .unwrap_or_else(|error| panic!("{label}: write failed: {error}"));
            Ok((path, report, bytes))
        }
        Err(error) => Err(error),
    };
    let outcome = match written {
        Ok((path, report, bytes)) => {
            assert!(
                matches!(path, WritePath::Synthesized { .. }),
                "{label}: source-less output must be synthesized"
            );
            SemanticOutcome::Written {
                report: Box::new(report),
                bytes,
            }
        }
        Err(error) => SemanticOutcome::Refused { error },
    };
    check(&outcome);
}

/// What the writer did for a document that was edited with its baseline left in
/// place.
#[derive(Debug)]
pub enum MutationOutcome {
    /// The writer ran and produced bytes.
    Written {
        /// The document that was written with the mutation applied.
        edited: Box<CadIr>,
        /// The bytes the writer produced.
        bytes: Vec<u8>,
    },
    /// The encoder refused the edit.
    Refused {
        /// The refusal error.
        error: CodecError,
    },
}

/// Which write path a mutation round trip demands.
///
/// A replay copies the retained bytes and would discard the edit, so this helper
/// cannot be asked for it and the variant does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedWritePath {
    /// The writer rewrote part of a container it did not author in full.
    Patched,
    /// The writer authored every output byte from neutral IR.
    Synthesized,
}

impl ExpectedWritePath {
    /// Whether `path` is the write path this value names.
    #[must_use]
    pub fn describes(self, path: &WritePath) -> bool {
        match self {
            Self::Patched => matches!(path, WritePath::Patched { .. }),
            Self::Synthesized => matches!(path, WritePath::Synthesized { .. }),
        }
    }
}

impl std::fmt::Display for ExpectedWritePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Patched => "patched",
            Self::Synthesized => "synthesized",
        })
    }
}

/// Decodes `fixture`, applies `mutate` to the decoded document, encodes it with
/// source fidelity supplied, and hands the outcome to `check`.
///
/// Returns whether `mutate` reported an edit. Retains fidelity and changes a
/// value; `check` can decode the output and demand that change back.
///
/// # Panics
///
/// Panics when the decode fails, when `mutate` reports an edit that left the
/// document equal to the decode, when the
/// write fails after planning succeeded, or when the encoder took a write path
/// other than `expected_path`.
pub fn mutation_roundtrip<C>(
    codec: &C,
    label: &str,
    fixture: &[u8],
    expected_path: ExpectedWritePath,
    mutate: impl FnOnce(&mut CadIr) -> bool,
    check: impl FnOnce(&MutationOutcome),
) -> bool
where
    C: Codec + Encoder,
{
    let decoded = crate::golden::decode(
        codec,
        &mut std::io::Cursor::new(fixture.to_vec()),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: decode failed: {error}"));
    let (mut edited, _decode_report, source_fidelity) = decoded.into_parts();
    let baseline = edited.clone();
    if !mutate(&mut edited) {
        return false;
    }
    assert!(
        edited != baseline,
        "{label}: the mutation reported an edit but left the document equal to the decode, so the encoder \
         would still be free to replay its retained bytes and this test would describe nothing"
    );
    // Plan borrows the document until write completes.
    let written = match Encoder::plan(
        codec,
        EncodeInput::new(&edited, Some(&source_fidelity)),
        TargetRequest::Inherit,
    ) {
        Ok(plan) => {
            let path = plan.report().write_path().clone();
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes)
                .unwrap_or_else(|error| panic!("{label}: write failed: {error}"));
            Ok((path, bytes))
        }
        Err(error) => Err(error),
    };
    let outcome = match written {
        Ok((path, bytes)) => {
            assert!(
                expected_path.describes(&path),
                "{label}: the document was edited, so the encoder was expected to write by the \
                 {expected_path} path, but it took the {path} path"
            );
            MutationOutcome::Written {
                edited: Box::new(edited),
                bytes,
            }
        }
        Err(error) => MutationOutcome::Refused { error },
    };
    check(&outcome);
    true
}
