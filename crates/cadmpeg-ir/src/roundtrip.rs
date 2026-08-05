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
//! There are exactly two entry points here, and each one names the path it
//! covers, so choosing wrong is a compile-time or assertion-time event rather
//! than a green test that proves nothing:
//!
//! - [`verbatim_replay_holds`] covers the copy. It asserts the copy happened and
//!   that the copy is faithful. It says nothing about the writer.
//! - [`semantic_roundtrip`] covers the writer. It removes the baseline first, so
//!   the copy is not available, and asserts the copy did not happen.
//!
//! There is deliberately no third, path-agnostic entry point. One would be the
//! trap this module exists to close.
//!
//! This lives in `cadmpeg-ir` rather than beside the golden harness in
//! `cadmpeg_codec_core::golden` because it drives [`Encoder`], which
//! `cadmpeg-codec-core` cannot name: the dependency runs the other way.

use cadmpeg_codec_core::CodecError;

use crate::codec::{Codec, CodecEntry, DecodeOptions, EncodeInput, Encoder, ExportPlan};
use crate::document::CadIr;
use crate::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use crate::report::{ExportReport, WritePath};

/// Encodes an unedited decode of `fixture` and asserts the encoder replayed the
/// retained bytes verbatim, producing `fixture` again.
///
/// # What this proves
///
/// That the container survives a decode and comes back byte for byte: the
/// retained source image is complete, its integrity check passes, and the
/// baseline the decoder stamped still matches what it recomputes. That is
/// container fidelity.
///
/// # What this does not prove
///
/// Anything about the writer. On this path no writer code runs; the encoder
/// copies bytes. Use [`semantic_roundtrip`] to exercise the writer.
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
    /// The encoder refused. A codec that needs a baseline to write at all
    /// refuses once the baseline is gone, and that refusal is its contract, not
    /// a failure to paper over.
    Refused {
        /// The refusal. Carried as the error itself so a caller asserts on the
        /// variant rather than on wording.
        error: CodecError,
    },
}

/// Decodes `fixture`, removes the document-level write baseline, encodes it, and
/// hands the outcome to `check`.
///
/// # Why the strip
///
/// A codec that retains its source bytes decides between copying them and
/// running the writer by recomputing a digest over the decoded content and
/// comparing it to the [`DOCUMENT_LOCAL_DIGEST_ATTRIBUTE`] baseline its decoder
/// stamped on `ir.source.attributes`. On an unedited document the two agree and
/// the copy wins, so the writer never runs. Removing that one attribute removes
/// the answer to "was this edited since it was decoded?", which no codec may
/// resolve in favour of the copy: replaying bytes that might no longer describe
/// the document would discard edits. Each codec then either runs its writer or
/// refuses, and both reach `check`.
///
/// Editing the document would also force the writer, but it changes what is
/// being written at the same time. Removing the baseline moves only the branch.
///
/// # Why only that attribute
///
/// The document baseline is one member of the `_local_sha256` family that
/// [`cadmpeg_codec_core::compare::is_local_digest_attribute`] classifies, and it is the only member that gates
/// this decision. The others are per-lane baselines answering the narrower
/// question of *which* part changed, and a writer reads them to plan its edit.
/// Removing the whole family therefore does not force the writer harder; it
/// starves it. `SolidWorks` demonstrates the difference: removing the document
/// baseline alone puts nineteen of its twenty committed fixtures through the
/// writer, and removing the family as well makes all twenty refuse, because the
/// codec then sees a neutral sketch lane with no baseline beside a native sketch
/// lane that still has one and declines to guess which moved.
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
    // The plan borrows the document, so the write finishes and the borrow ends
    // before the outcome takes ownership of it.
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
