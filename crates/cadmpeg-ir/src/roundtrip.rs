// SPDX-License-Identifier: Apache-2.0
//! Round-trip test entry points, one per write path.
//!
//! A retaining codec copies source bytes when the document baseline still
//! matches, and runs the writer otherwise. A decode/encode/compare-bytes test
//! over an unedited document always takes the copy and never exercises the
//! writer. The three helpers here name the path they cover:
//!
//! - [`verbatim_replay_holds`] — copy happened and is byte-faithful.
//! - [`semantic_roundtrip`] — baseline removed; copy must not happen.
//! - [`mutation_roundtrip`] — baseline kept, one value edited; asserts the
//!   named write path and that the edit survives.
//!
//! Lives here because it drives [`Encoder`]; `cadmpeg-core` cannot name that.

use cadmpeg_core::CodecError;

use crate::codec::{Codec, CodecEntry, DecodeOptions, EncodeInput, Encoder, ExportPlan};
use crate::document::CadIr;
use crate::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use crate::report::{ExportReport, WritePath};

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
    let decoded = CodecEntry::decode(
        codec,
        &mut std::io::Cursor::new(fixture.to_vec()),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: decode failed: {error}"));
    let plan = Encoder::plan(
        codec,
        EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
        },
    )
    .unwrap_or_else(|error| panic!("{label}: plan failed: {error}"));
    let path = ExportPlan::write_path(&plan);
    let mut written = Vec::new();
    let report = plan
        .write_to(&mut written)
        .unwrap_or_else(|error| panic!("{label}: write failed: {error}"));
    assert_eq!(
        path,
        WritePath::VerbatimReplay,
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
        /// The document that was written, with its baseline removed.
        ir: Box<CadIr>,
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

/// Decodes `fixture`, removes the document-level write baseline, encodes it, and
/// hands the outcome to `check`.
///
/// Removing [`DOCUMENT_LOCAL_DIGEST_ATTRIBUTE`] denies verbatim replay without
/// editing the document. The encoder must write or refuse; it must not copy.
/// Lane digests stay so writers that need them can still plan.
///
/// # Panics
///
/// Panics when the decode fails, when the document carries no baseline to
/// remove, when the write fails after planning succeeded, or when the encoder
/// still took [`WritePath::VerbatimReplay`] — which would mean it replayed bytes
/// it could no longer show were current.
pub fn semantic_roundtrip<C>(
    codec: &C,
    label: &str,
    fixture: &[u8],
    check: impl FnOnce(&SemanticOutcome),
) where
    C: Codec + Encoder,
{
    let mut decoded = CodecEntry::decode(
        codec,
        &mut std::io::Cursor::new(fixture.to_vec()),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: decode failed: {error}"));
    let removed = decoded
        .ir
        .source
        .as_mut()
        .and_then(|source| source.attributes.remove(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE));
    assert!(
        removed.is_some(),
        "{label}: the decoded document carries no `{DOCUMENT_LOCAL_DIGEST_ATTRIBUTE}`, so removing it cannot \
         force the semantic path; this helper would report whatever the encoder happened to do"
    );
    // Plan borrows the document until write completes.
    let written = match Encoder::plan(
        codec,
        EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
        },
    ) {
        Ok(plan) => {
            let path = ExportPlan::write_path(&plan);
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
            assert_ne!(
                path,
                WritePath::VerbatimReplay,
                "{label}: the baseline was removed, so the encoder could not show the retained bytes still \
                 describe this document, yet it replayed them"
            );
            SemanticOutcome::Written {
                ir: Box::new(decoded.ir),
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
        /// The document as decoded, before the mutation.
        baseline: Box<CadIr>,
        /// The document that was written: `baseline` with the mutation applied.
        edited: Box<CadIr>,
        /// The encoder's report, whose `write_path` is the one the caller named.
        report: Box<ExportReport>,
        /// The bytes the writer produced.
        bytes: Vec<u8>,
    },
    /// The encoder refused the edit.
    Refused {
        /// The refusal error.
        error: CodecError,
    },
}

/// Decodes `fixture`, applies `mutate` to the decoded document, encodes it with
/// the write baseline left in place, and hands the outcome to `check`.
///
/// Returns whether `mutate` reported an edit. Denies verbatim replay the way a
/// user does: keep the baseline, change a value. `check` can decode the output
/// and demand that change back.
///
/// # Panics
///
/// Panics when the decode fails, when `mutate` reports an edit that left the
/// document equal to the decode, when `mutate` removed the baseline (which would
/// make this [`semantic_roundtrip`] under another name), when the write fails
/// after planning succeeded, or when the encoder took a write path other than
/// `expected_path`. Also panics when `expected_path` is
/// [`WritePath::VerbatimReplay`]: the document was edited, so replaying the
/// retained bytes would discard the edit and no caller may expect it.
pub fn mutation_roundtrip<C>(
    codec: &C,
    label: &str,
    fixture: &[u8],
    expected_path: WritePath,
    mutate: impl FnOnce(&mut CadIr) -> bool,
    check: impl FnOnce(&MutationOutcome),
) -> bool
where
    C: Codec + Encoder,
{
    assert_ne!(
        expected_path,
        WritePath::VerbatimReplay,
        "{label}: this helper edits the document, so replaying the retained bytes would discard the edit; \
         no caller may name that path as expected"
    );
    let mut decoded = CodecEntry::decode(
        codec,
        &mut std::io::Cursor::new(fixture.to_vec()),
        &DecodeOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{label}: decode failed: {error}"));
    let baseline = decoded.ir.clone();
    if !mutate(&mut decoded.ir) {
        return false;
    }
    assert!(
        decoded.ir != baseline,
        "{label}: the mutation reported an edit but left the document equal to the decode, so the encoder \
         would still be free to replay its retained bytes and this test would describe nothing"
    );
    assert!(
        decoded
            .ir
            .source
            .as_ref()
            .is_some_and(|source| source.attributes.contains_key(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE)),
        "{label}: the mutation removed `{DOCUMENT_LOCAL_DIGEST_ATTRIBUTE}`; this helper covers the edited \
         document with its baseline intact, and without it the codec answers a different question"
    );
    // Plan borrows the document until write completes.
    let written = match Encoder::plan(
        codec,
        EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
        },
    ) {
        Ok(plan) => {
            let path = ExportPlan::write_path(&plan);
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
            assert_eq!(
                path, expected_path,
                "{label}: the document was edited, so the encoder was expected to write by the \
                 {expected_path} path, but it took the {path} path"
            );
            MutationOutcome::Written {
                baseline: Box::new(baseline),
                edited: Box::new(decoded.ir),
                report: Box::new(report),
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
