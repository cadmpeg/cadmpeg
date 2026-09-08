// SPDX-License-Identifier: Apache-2.0
//! Fixed word-reference groups inside a byte-framed FSET graph.

use super::operation_record::OperationPayload;
use cadmpeg_core::decode::View;

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
            .checked_add(selector.len() as u64 + 22)
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
        let start = self.offset + 3 + self.selector.len() as u64;
        [start, start + 3]
    }
    pub(crate) fn second_offsets(&self) -> [u64; 3] {
        let start = self.offset + 10 + self.selector.len() as u64;
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
                (record.payload_offset() + start) as u64,
                selector.to_string(),
                first,
                second,
            )
            .ok()
        };
        super::unique_candidate((0..bytes.len().saturating_sub(1)).filter_map(decode))
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(u16) -> B,
    ) -> Result<FsetReferences<B>, &'static str> {
        let offset = self
            .offset
            .checked_add(file_base)
            .ok_or("source_offset: FSET frame overflows")?;
        FsetReferences::new(
            offset,
            self.selector,
            self.first.map(|(index, ())| (index, target(index))),
            self.second.map(|(index, ())| (index, target(index))),
        )
    }
}
