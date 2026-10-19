// SPDX-License-Identifier: Apache-2.0
//! Round-trip test entry points, one per write path.
//!
//! A codec that retains its source bytes answers "write this document" two ways:
//! it copies the retained bytes out when the document is unchanged since the
//! decode that stamped its baseline, and it runs the writer otherwise. A test
//! spelled decode, encode, compare bytes reads the same at the call site in both
//! cases, and over an unedited document it always takes the copy. It then passes
//! no matter what the writer does, because the writer never ran.
//!
//! There are exactly three entry points here, one per way the question can be
//! asked:
//!
//! - [`verbatim_replay_holds`] covers the copy. It asserts the copy happened and
//!   that the copy is faithful. It says nothing about the writer.
//! - [`semantic_roundtrip`] covers the writer with the baseline taken away, so
//!   the copy is not available, and asserts the copy did not happen.
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
//! Removing the baseline and editing a value both deny the copy, but they are
//! not the same test and one does not subsume the other. Removing the baseline
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

/// Encodes an unedited decode of `fixture` and asserts the encoder replayed the
/// retained bytes verbatim, producing `fixture` again.
///
/// Proves container fidelity (retained image, integrity, matching baseline).
/// Does not exercise the writer; use [`semantic_roundtrip`] for that.
///
/// # Panics
///
/// Panics when the decode or the encode fails, when the encoder took any path
/// other than [`WritePath::VerbatimReplay`], or when the replayed bytes differ
/// from `fixture`.
pub fn verbatim_replay_holds<C>(codec: &C, label: &str, fixture: &[u8]) -> ExportReport
where
    C: Codec + Encoder,
{
    let decoded = crate::EditableDecodeResult::from(
        Codec::decode(
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
        matches!(path, WritePath::VerbatimReplay { .. }),
        "{label}: this fixture was expected to replay its retained bytes, but the encoder took the {path} path; \
         a byte comparison here would not describe the replay it claims to cover"
    );
    assert!(
        written == fixture,
        "{label}: verbatim replay produced different bytes: {}",
        describe_byte_difference(fixture, &written)
    );
    report
}

/// What the writer did when it was denied the verbatim-replay shortcut.
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
    let decoded = Codec::decode(
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
    let decoded = Codec::decode(
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

/// Locates the first differing byte, or the length disagreement when one side is
/// a prefix of the other.
fn describe_byte_difference(expected: &[u8], actual: &[u8]) -> String {
    match expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
    {
        Some(offset) => format!(
            "first difference at offset {offset}: fixture 0x{:02x}, written 0x{:02x} (lengths {} and {})",
            expected[offset],
            actual[offset],
            expected.len(),
            actual.len()
        ),
        None => format!(
            "one side is a prefix of the other (lengths {} and {})",
            expected.len(),
            actual.len()
        ),
    }
}
