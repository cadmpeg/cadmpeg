// SPDX-License-Identifier: Apache-2.0
//! The replay/patch/generate decision a writing codec makes before emitting a
//! container.
//!
//! A codec that retains its source image faces one decision at write time: can
//! the decoded document be written back by replaying the retained bytes
//! verbatim, by patching those bytes with the supported semantic edits, or must
//! it be regenerated from the neutral model? Four writing codecs smear that
//! decision across their `lib.rs` and patch modules, each re-deriving the same
//! record-presence, integrity, and baseline-comparison steps.
//!
//! [`plan_write`] is the total function that consolidates the decision.
//! [`verify_retained_bytes`] is the per-record integrity predicate the decision
//! turns on, extracted from the `data.len() != byte_len || sha != digest`
//! check the codecs inline. Every failure resolves to [`WritePlan::Generate`],
//! so the gate never errors: a codec calls it, then routes on the returned
//! plan.
//!
//! # Gate order
//!
//! The function evaluates, in order: sidecar present, record present, record
//! carries bytes, bytes pass [`verify_retained_bytes`], baseline present. Only
//! then does it compare the baseline against the current semantic hash:
//! [`WritePlan::Replay`] when they match, [`WritePlan::Patch`] when they
//! differ. An absent baseline is [`WritePlan::Generate`], not
//! [`WritePlan::Patch`] — a document with no recorded baseline cannot be shown
//! to differ only in supported ways, so it is regenerated.
//!
//! # Divergences from the codecs this consolidates
//!
//! The two real implementations are `cadmpeg-codec-f3d` (`write_preserved_bytes`)
//! and `cadmpeg-codec-sldprt` (`write_preserved_with_annotations`). Both reduce
//! to this gate under the sanctioned conversion of a write-refusal error into
//! [`WritePlan::Generate`], but two deltas remain that the adoption wave must
//! account for:
//!
//! - **Integrity is gated on both non-generate branches here.** f3d verifies
//!   the source image before splitting replay from patch, matching this
//!   function. sldprt verifies the source image only on its replay branch; when
//!   the baseline differs it enters the semantic writer without checking the
//!   source image's `byte_len`/`sha256`. Adopting this gate makes a corrupt
//!   retained image resolve to [`WritePlan::Generate`] even when the baseline
//!   differs, where sldprt would previously have attempted a patch.
//! - **sldprt carries a second integrity layer this gate does not model.** Its
//!   patch path re-validates the retained Parasolid partition against
//!   `brep_semantic_sha256` (`writer.rs::retained_partition`), a per-stream
//!   check distinct from the source-image `byte_len`/`sha256` that
//!   [`verify_retained_bytes`] captures. A [`WritePlan::Patch`] result still
//!   depends on that partition check succeeding inside the codec's patch
//!   writer; the gate only decides that a patch is admissible, not that every
//!   retained stream it consumes is intact.
//!
//! The evaluation order itself (f3d checks the baseline before integrity; this
//! function checks integrity before the baseline) is outcome-equivalent: both
//! orderings resolve every combination of a missing baseline and failed
//! integrity to [`WritePlan::Generate`].

use crate::source_fidelity::SourceFidelity;

/// How a writing codec should produce its output container.
///
/// The borrowed slice in [`Replay`](WritePlan::Replay) and
/// [`Patch`](WritePlan::Patch) is the retained source image the plan verified,
/// ready to be written verbatim or handed to the patch writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePlan<'a> {
    /// Write the retained bytes verbatim: the document is unchanged from decode.
    Replay(&'a [u8]),
    /// Patch the retained bytes: the document differs only in supported ways.
    Patch(&'a [u8]),
    /// Regenerate the container from the neutral model: no usable source image,
    /// or no baseline to prove the change is patchable.
    Generate,
}

/// Decides how a decoded document should be written back to its source format.
///
/// `record_id` names the retained source-image record inside `sidecar`.
/// `baseline_hash` is the semantic hash recorded at decode; `current_hash` is
/// the semantic hash of the document about to be written. The plan is
/// [`WritePlan::Generate`] unless the sidecar holds `record_id`, that record
/// carries bytes, those bytes pass [`verify_retained_bytes`], and a baseline is
/// present. Given all of that, the plan is [`WritePlan::Replay`] when the
/// baseline equals `current_hash` and [`WritePlan::Patch`] when it does not.
///
/// See the [module docs](self) for the full gate order and the divergences
/// from the codecs this consolidates.
pub fn plan_write<'a>(
    sidecar: Option<&'a SourceFidelity>,
    record_id: &str,
    baseline_hash: Option<&str>,
    current_hash: &str,
) -> WritePlan<'a> {
    let Some(sidecar) = sidecar else {
        return WritePlan::Generate;
    };
    let Some(record) = sidecar.retained_record(record_id) else {
        return WritePlan::Generate;
    };
    let Some(data) = record.data.as_deref() else {
        return WritePlan::Generate;
    };
    if !verify_retained_bytes(data, record.byte_len, &record.sha256) {
        return WritePlan::Generate;
    }
    let Some(baseline) = baseline_hash else {
        return WritePlan::Generate;
    };
    if baseline == current_hash {
        WritePlan::Replay(data)
    } else {
        WritePlan::Patch(data)
    }
}

/// Confirms retained bytes match their declared length and SHA-256 digest.
///
/// Returns `true` only when `data` is exactly `byte_len` bytes long and its
/// lowercase-hexadecimal SHA-256 (via [`crate::wire::hash::sha256_hex`]) equals
/// `sha256_hex`. This is the per-record integrity predicate that
/// [`SourceFidelity::validate`](crate::SourceFidelity::validate) applies across
/// every retained record; it is factored out here so a writing codec can gate a
/// single source image without walking the whole sidecar.
pub fn verify_retained_bytes(data: &[u8], byte_len: u64, sha256_hex: &str) -> bool {
    data.len() as u64 == byte_len && crate::wire::hash::sha256_hex(data) == sha256_hex
}

#[cfg(test)]
mod tests {
    use super::{plan_write, verify_retained_bytes, WritePlan};
    use crate::source_fidelity::{RetainedSourceRecord, SourceFidelity};
    use crate::wire::hash::sha256_hex;

    const ID: &str = "codec:file:source-image#0";

    fn record(id: &str, data: Option<&[u8]>, byte_len: u64, sha: &str) -> RetainedSourceRecord {
        RetainedSourceRecord {
            id: id.into(),
            stream: "source".into(),
            offset: 0,
            byte_len,
            sha256: sha.into(),
            data: data.map(<[u8]>::to_vec),
        }
    }

    fn sidecar_with(record: RetainedSourceRecord) -> SourceFidelity {
        SourceFidelity {
            retained_records: vec![record],
            ..SourceFidelity::default()
        }
    }

    /// A sidecar holding one intact source-image record for `bytes`.
    fn intact(bytes: &[u8]) -> SourceFidelity {
        sidecar_with(record(
            ID,
            Some(bytes),
            bytes.len() as u64,
            &sha256_hex(bytes),
        ))
    }

    #[test]
    fn verify_retained_bytes_truth_table() {
        let bytes = b"parasolid";
        let sha = sha256_hex(bytes);
        assert!(verify_retained_bytes(bytes, bytes.len() as u64, &sha));
        // Wrong declared length.
        assert!(!verify_retained_bytes(bytes, bytes.len() as u64 + 1, &sha));
        // Wrong digest.
        assert!(!verify_retained_bytes(
            bytes,
            bytes.len() as u64,
            &sha256_hex(b"other")
        ));
        // Empty data against a nonzero declaration.
        assert!(!verify_retained_bytes(&[], 1, &sha));
        // Empty data, honestly declared.
        assert!(verify_retained_bytes(&[], 0, &sha256_hex(&[])));
    }

    #[test]
    fn no_sidecar_generates() {
        assert_eq!(plan_write(None, ID, Some("a"), "a"), WritePlan::Generate);
    }

    #[test]
    fn absent_record_generates() {
        let sidecar = intact(b"image");
        assert_eq!(
            plan_write(Some(&sidecar), "other:id", Some("a"), "a"),
            WritePlan::Generate
        );
    }

    #[test]
    fn record_without_data_generates() {
        let bytes = b"image";
        let sidecar = sidecar_with(record(ID, None, bytes.len() as u64, &sha256_hex(bytes)));
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("a"), "a"),
            WritePlan::Generate
        );
    }

    #[test]
    fn byte_len_mismatch_generates() {
        let bytes = b"image";
        // Declared length lies about the retained bytes.
        let sidecar = sidecar_with(record(
            ID,
            Some(bytes),
            bytes.len() as u64 + 3,
            &sha256_hex(bytes),
        ));
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("a"), "a"),
            WritePlan::Generate
        );
    }

    #[test]
    fn sha_mismatch_generates() {
        let bytes = b"image";
        // Digest belongs to different bytes: tampered retained payload.
        let sidecar = sidecar_with(record(
            ID,
            Some(bytes),
            bytes.len() as u64,
            &sha256_hex(b"tampered"),
        ));
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("a"), "a"),
            WritePlan::Generate
        );
    }

    #[test]
    fn tampered_bytes_generate_even_when_baseline_matches() {
        let bytes = b"image";
        // Honest length and digest for `bytes`, but the stored data was swapped.
        let mut sidecar = sidecar_with(record(
            ID,
            Some(bytes),
            bytes.len() as u64,
            &sha256_hex(bytes),
        ));
        sidecar.retained_records[0].data = Some(b"swapped-out".to_vec());
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("h"), "h"),
            WritePlan::Generate
        );
    }

    #[test]
    fn absent_baseline_generates_even_when_integrity_holds() {
        let bytes = b"image";
        let sidecar = intact(bytes);
        assert_eq!(
            plan_write(Some(&sidecar), ID, None, "h"),
            WritePlan::Generate
        );
    }

    #[test]
    fn matching_baseline_replays() {
        let bytes = b"the source image";
        let sidecar = intact(bytes);
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("same"), "same"),
            WritePlan::Replay(bytes)
        );
    }

    #[test]
    fn differing_baseline_patches() {
        let bytes = b"the source image";
        let sidecar = intact(bytes);
        assert_eq!(
            plan_write(Some(&sidecar), ID, Some("decoded"), "edited"),
            WritePlan::Patch(bytes)
        );
    }
}
