// SPDX-License-Identifier: Apache-2.0
//! Fixed word-reference groups inside a byte-framed FSET graph.

use super::operation_record::OperationPayload;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FsetReferences<B> {
    offset: u64,
    selector: String,
    first: [(u16, B); 2],
    second: [(u16, B); 3],
}

impl<B> FsetReferences<B> {
    pub(crate) fn new(
        offset: u64,
        selector: String,
        first: [(u16, B); 2],
        second: [(u16, B); 3],
    ) -> Result<Self, &'static str> {
        if !(1..=247).contains(&selector.len())
            || !selector
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'>')
        {
            return Err("selector: requires 1 through 247 graphic ASCII bytes excluding >");
        }
        offset
            .checked_add(cadmpeg_core::decode::u64_from_index(selector.len()) + 22)
            .ok_or("source_offset: FSET frame overflows")?;
        Ok(Self {
            offset,
            selector,
            first,
            second,
        })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn selector(&self) -> &str {
        &self.selector
    }
    pub(crate) fn first(&self) -> &[(u16, B); 2] {
        &self.first
    }
    pub(crate) fn second(&self) -> &[(u16, B); 3] {
        &self.second
    }
    pub(crate) fn first_offsets(&self) -> [u64; 2] {
        let start = self.offset + 3 + cadmpeg_core::decode::u64_from_index(self.selector.len());
        [start, start + 3]
    }
    pub(crate) fn second_offsets(&self) -> [u64; 3] {
        let start = self.offset + 10 + cadmpeg_core::decode::u64_from_index(self.selector.len());
        [start, start + 3, start + 6]
    }
}

pub(crate) fn word_reference_bytes(index: u16) -> [u8; 3] {
    let [high, low] = index.to_be_bytes();
    [0x90, high, low]
}

impl FsetReferences<()> {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        if record.name() != "FSET" {
            return None;
        }
        let bytes = record.payload();
        let read_word = |at: &mut usize| {
            if bytes.get(*at) != Some(&0x90) {
                return None;
            }
            let index = View::u16_be_at(bytes, *at + 1)?;
            *at += 3;
            Some((index, ()))
        };
        let decode = |start: usize| {
            if bytes.get(start) != Some(&1) {
                return None;
            }
            let len = usize::from(*bytes.get(start + 1)?);
            if len < 9 {
                return None;
            }
            let body_start = start.checked_add(2)?;
            let body_end = body_start.checked_add(len)?;
            if bytes.get(body_start) != Some(&b'<') || bytes.get(body_end - 1) != Some(&b'>') {
                return None;
            }
            let selector_end = body_end - 7;
            let selector = std::str::from_utf8(bytes.get(body_start + 1..selector_end)?).ok()?;
            let mut at = selector_end;
            let first = [read_word(&mut at)?, read_word(&mut at)?];
            at += 1;
            let second = [
                read_word(&mut at)?,
                read_word(&mut at)?,
                read_word(&mut at)?,
            ];
            if bytes.get(at..at.checked_add(3)?) != Some(&[0, 3, 0]) {
                return None;
            }
            Self::new(
                cadmpeg_core::decode::u64_from_index(record.payload_offset() + start),
                selector.to_string(),
                first,
                second,
            )
            .ok()
        };
        super::unique_candidate(bytes.len().checked_sub(1).into_iter().flat_map(|last| 0..last).filter_map(decode))
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(u16) -> Result<B, CodecError>,
    ) -> Result<Option<FsetReferences<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let [(first_head, ()), (first_tail, ())] = self.first;
        let [(second_head, ()), (second_middle, ()), (second_tail, ())] = self.second;
        Ok(FsetReferences::new(
            offset,
            self.selector,
            [
                (first_head, target(first_head)?),
                (first_tail, target(first_tail)?),
            ],
            [
                (second_head, target(second_head)?),
                (second_middle, target(second_middle)?),
                (second_tail, target(second_tail)?),
            ],
        )
        .ok())
    }
}

#[cfg(test)]
mod tests {
    use super::FsetReferences;
    use crate::om::operation_record::OperationPayload;

    fn graph() -> FsetReferences<()> {
        let payload = [
            1, 0x13, 0x3c, b'T', b';', b':', b'S', b'5', b'6', b'7', b'R', b'8', b'9', b'3', 0x90,
            0x19, 0x40, 0x90, 0x19, 0x41, 0x3e, 0x90, 0x19, 0x30, 0x90, 0x19, 0x31, 0x90, 0x19,
            0x32, 0, 3, 0,
        ];
        let record = OperationPayload::new(&payload, 100, "FSET").expect("test FSET payload");
        FsetReferences::read(record).expect("complete FSET graph")
    }

    #[test]
    fn fset_resolution_returns_collection_refusal() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test root");
        let error = graph()
            .resolve(0, |index| {
                ctx.charge_collection_items(1, "NX FSET target")?;
                Ok(Some(index))
            })
            .expect_err("target resolution refusal");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn fset_resolution_preserves_both_groups() {
        let resolved = graph()
            .resolve(20, |index| Ok::<_, cadmpeg_core::CodecError>(Some(index)))
            .expect("target resolution")
            .expect("valid relocated graph");
        assert_eq!(resolved.offset(), 120);
        assert_eq!(
            resolved.first().map(|(index, value)| (index, value)),
            [(6464, Some(6464)), (6465, Some(6465))]
        );
        assert_eq!(
            resolved.second().map(|(index, value)| (index, value)),
            [(6448, Some(6448)), (6449, Some(6449)), (6450, Some(6450))]
        );
    }
}
