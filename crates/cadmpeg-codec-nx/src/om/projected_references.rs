// SPDX-License-Identifier: Apache-2.0
//! Fixed projected-curve reference graphs and their derived positions.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::PayloadObjectReference;

const CPROJ_MIDDLE: [u8; 5] = [0x80, 0x57, 0x00, 0x02, 0x01];
const CPROJ_SUFFIX: [u8; 5] = [0xff, 0x01, 0x02, 0x02, 0x7d];
const CMB_PREFIX: [u8; 10] = [0x3c, 0x32, 0x01, 0x02, 0x32, 0x01, 0x04, 0x36, 0x01, 0x33];
const CMB_BRANCH_PREFIX: [u8; 3] = [0x16, 0x01, 0x02];
const CMB_BRANCH_MIDDLE: [u8; 10] = [0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01, 0x02];
const CMB_BRANCH_SUFFIX: [u8; 3] = [0x00, 0x81, 0x5c];
const CMB_TAIL_PREFIX: [u8; 4] = [0xff, 0x01, 0xff, 0x01];
const CMB_TAIL_SUFFIX: [u8; 2] = [0x04, 0x02];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedCurveReferences {
    offset: usize,
    body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Body {
    Projected([PayloadIndexToken; 3]),
    Combined([PayloadIndexToken; 8]),
}

impl ProjectedCurveReferences {
    pub(crate) fn read(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        record: OperationPayload<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let bytes = record.payload();
        let read_token = |at: &mut usize| {
            let token = PayloadIndexToken::read(bytes.get(*at..)?)?;
            *at += token.raw().len();
            Some(token)
        };
        let consume = |at: &mut usize, expected: &[u8]| -> Option<()> {
            let end = at.checked_add(expected.len())?;
            if bytes.get(*at..end) != Some(expected) { return None; }
            *at = end;
            Some(())
        };
        let decode = |start: usize| {
            let mut at = start;
            let body = match record.name() {
                "CPROJ" => {
                    consume(&mut at, &[1, 2])?;
                    let first = read_token(&mut at)?;
                    let second = read_token(&mut at)?;
                    consume(&mut at, &CPROJ_MIDDLE)?;
                    let third = read_token(&mut at)?;
                    consume(&mut at, &CPROJ_SUFFIX)?;
                    Body::Projected([first, second, third])
                }
                "CPROJ_CMB" => {
                    consume(&mut at, &CMB_PREFIX)?;
                    let first = read_token(&mut at)?;
                    consume(&mut at, &[0x33])?;
                    let second = read_token(&mut at)?;
                    consume(&mut at, &[0])?;
                    let third = read_token(&mut at)?;
                    consume(&mut at, &[0; 6])?;
                    let fourth = read_token(&mut at)?;
                    let mut read_branch = |anchor| {
                        consume(&mut at, &CMB_BRANCH_PREFIX)?;
                        if read_token(&mut at)? != anchor {
                            return None;
                        }
                        consume(&mut at, &CMB_BRANCH_MIDDLE)?;
                        let token = read_token(&mut at)?;
                        consume(&mut at, &CMB_BRANCH_SUFFIX)?;
                        Some(token)
                    };
                    let fifth = read_branch(first)?;
                    let sixth = read_branch(second)?;
                    consume(&mut at, &CMB_TAIL_PREFIX)?;
                    let seventh = read_token(&mut at)?;
                    let eighth = read_token(&mut at)?;
                    consume(&mut at, &CMB_TAIL_SUFFIX)?;
                    Body::Combined([first, second, third, fourth, fifth, sixth, seventh, eighth])
                }
                _ => return None,
            };
            Some(Self {
                offset: record.payload_offset() + start,
                body,
            })
        };
        let marker = match record.name() {
            "CPROJ" => &[1, 2][..],
            "CPROJ_CMB" => &CMB_PREFIX[..],
            _ => return Ok(None),
        };
        let Some(candidate_end) = bytes.len().checked_sub(marker.len()) else {
            return Ok(None);
        };
        let mut candidate = None;
        let mut starts = 0..=candidate_end;
        while let Some(start) = ctx.next_charged(&mut starts, "NX projected curve reference candidate search")? {
            if bytes.get(start..start + marker.len()) != Some(marker) {
                continue;
            }
            if let Some(parsed) = decode(start) {
                if candidate.is_some() {
                    return Ok(None);
                }
                candidate = Some(parsed);
            }
        }
        Ok(candidate)
    }

    pub(crate) fn into_references(self) -> impl ExactSizeIterator<Item = PayloadObjectReference<PayloadIndexToken>> {
        let first = match self.body { Body::Projected(tokens) => tokens[0], Body::Combined(tokens) => tokens[0] };
        let mut references: [_; 8] = std::array::from_fn(|_| PayloadObjectReference { offset: self.offset, token: first });
        let mut len = 0;
        let mut append = |at: &mut usize, token: PayloadIndexToken| {
            references[len] = PayloadObjectReference { offset: *at, token };
            len += 1;
            *at += token.raw().len();
        };
        match self.body {
            Body::Projected([first, second, third]) => {
                let mut at = self.offset + 2;
                append(&mut at, first);
                append(&mut at, second);
                at += CPROJ_MIDDLE.len();
                append(&mut at, third);
            }
            Body::Combined([first, second, third, fourth, fifth, sixth, seventh, eighth]) => {
                let mut at = self.offset + CMB_PREFIX.len();
                append(&mut at, first);
                at += 1;
                append(&mut at, second);
                at += 1;
                append(&mut at, third);
                at += 6;
                append(&mut at, fourth);
                for (anchor, token) in [(first, fifth), (second, sixth)] {
                    at += CMB_BRANCH_PREFIX.len() + anchor.raw().len() + CMB_BRANCH_MIDDLE.len();
                    append(&mut at, token);
                    at += CMB_BRANCH_SUFFIX.len();
                }
                at += CMB_TAIL_PREFIX.len();
                append(&mut at, seventh);
                append(&mut at, eighth);
            }
        }
        references.into_iter().take(len)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn projected_reference_search_refusal_propagates() {
        let bytes = [1, 2, 0xf0, 1];
        let payload = super::OperationPayload::new(&bytes, 100, "CPROJ").unwrap();
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX projected curve reference candidate search",
            |ctx| super::ProjectedCurveReferences::read(ctx, payload),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == 1)
        );
    }
}
