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
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        let bytes = record.payload();
        let read_token = |at: &mut usize| {
            let token = PayloadIndexToken::read(bytes.get(*at..)?)?;
            *at += token.raw().len();
            Some(token)
        };
        let consume = |at: &mut usize, expected: &[u8]| {
            let end = at.checked_add(expected.len())?;
            if bytes.get(*at..end) != Some(expected) {
                return None;
            }
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
            _ => return None,
        };
        super::unique_candidate((0..=bytes.len().saturating_sub(marker.len())).filter_map(
            |start| {
                if bytes.get(start..start + marker.len()) != Some(marker) {
                    return None;
                }
                decode(start)
            },
        ))
    }

    pub(crate) fn into_references(self) -> Vec<PayloadObjectReference<PayloadIndexToken>> {
        let mut references = Vec::new();
        let mut append = |at: &mut usize, token: PayloadIndexToken| {
            references.push(PayloadObjectReference { offset: *at, token });
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
        references
    }
}
