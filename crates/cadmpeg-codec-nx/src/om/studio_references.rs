// SPDX-License-Identifier: Apache-2.0
//! Exact post-branch storage references in a bounded Studio Surface record.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::surface_branches::surface_feature_payload_branches;
use super::surface_envelope::surface_feature_payload_references;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

const PREFIX: [u8; 22] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 2, 1, 1, 1, 5, 1, 1, 1, 1, 1, 9,
];
const SUFFIX: [u8; 36] = [
    1, 5, 0xff, 0xff, 0xff, 0xff, 0x15, 1, 0x10, 4, 4, 2, 2, 0xa0, 0x5a, 0x50, 8, 8, 2, 2, 1, 1, 1,
    1, 2, 2, 1, 5, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b,
];

pub(crate) fn studio_post_branch_references(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<[(PayloadIndexToken, u64); 8]>, CodecError> {
    if record.name() != "Studio Surface" {
        return Ok(None);
    }
    let Some(envelope) = surface_feature_payload_references(record) else {
        return Ok(None);
    };
    let Some(group) = surface_feature_payload_branches(ctx, record)? else {
        return Ok(None);
    };
    let branches = group.into_branches();
    let parsed = (|| {
        let origin = u64_from_index(record.payload_offset());
        let start = branches.last()?.end_offset().checked_add(8)?;
        let mut at = usize::try_from(start.checked_sub(origin)?).ok()?;
        if record.payload().get(at..at.checked_add(PREFIX.len())?) != Some(&PREFIX) {
            return None;
        }
        at += PREFIX.len();
        let first = PayloadIndexToken::read(record.payload().get(at..)?)?;
        let mut references = [(first, 0); 8];
        for reference in &mut references {
            let token = PayloadIndexToken::read(record.payload().get(at..)?)?;
            if token.value() == 0 {
                return None;
            }
            *reference = (token, origin.checked_add(u64_from_index(at))?);
            at += token.raw().len();
        }
        if record.payload().get(at..at.checked_add(SUFFIX.len())?) != Some(&SUFFIX) {
            return None;
        }
        at += SUFFIX.len();
        // The lane ends exactly where the admitted envelope's trailing prefix
        // starts. Its fixed prefix and suffix cannot float inside other data.
        if origin.checked_add(u64_from_index(at))?.checked_add(10)? != envelope.references()[11].1 {
            return None;
        }
        Some(references)
    })();
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{studio_post_branch_references, OperationPayload};

    fn read(payload: &[u8], name: &str) -> Option<[(super::PayloadIndexToken, u64); 8]> {
        crate::test_support::with_decode_context(|ctx| {
            studio_post_branch_references(ctx, OperationPayload::new(payload, 100, name).unwrap())
                .unwrap()
        })
    }

    #[test]
    fn studio_post_branch_field_is_bounded_by_both_admitted_neighbors() {
        let payload = crate::test_support::studio_surface::payload();
        let refs = read(&payload, "Studio Surface").unwrap();
        assert_eq!(refs.map(|r| r.0.value()), [18, 19, 20, 21, 22, 23, 24, 25]);
        assert_eq!(refs.map(|r| r.1), [239, 241, 243, 245, 247, 249, 251, 253]);
        assert!(read(&payload, "SKIN").is_none());
        for offset in (117..139).chain(155..191).chain((139..155).step_by(2)) {
            let mut invalid = payload.clone();
            invalid[offset] ^= 0x7f;
            assert!(
                read(&invalid, "Studio Surface").is_none(),
                "offset {offset}"
            );
        }
        for end in 117..224 {
            assert!(read(&payload[..end], "Studio Surface").is_none());
        }
    }

    #[test]
    fn studio_post_branch_field_preserves_canonical_widths_and_rejects_nulls() {
        let mut payload = crate::test_support::studio_surface::payload();
        payload.splice(139..141, [0xf1, 1, 0]);
        let refs = read(&payload, "Studio Surface").unwrap();
        assert_eq!(refs[0].0.value(), 256);
        assert_eq!(refs[0].0.raw(), [0xf1, 1, 0]);
        assert_eq!(refs[1].1, 242);
        payload[140] = 0;
        assert!(read(&payload, "Studio Surface").is_none());
        let mut payload = crate::test_support::studio_surface::payload();
        payload[140] = 0;
        assert!(read(&payload, "Studio Surface").is_none());
    }
}
