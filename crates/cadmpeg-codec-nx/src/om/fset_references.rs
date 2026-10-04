// SPDX-License-Identifier: Apache-2.0
//! Fixed word-reference groups inside a byte-framed FSET graph.

use super::operation_record::OperationPayload;
use cadmpeg_core::decode::{DecodeContext, View};
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
        match Self::validate(offset, &selector, |selector| {
            Ok::<_, std::convert::Infallible>(selector.chars())
        }) {
            Ok(valid) => valid?,
            Err(error) => match error {},
        }
        Ok(Self { offset, selector, first, second })
    }

    fn from_wire(ctx: &DecodeContext<'_>, offset: u64, selector: String, first: [(u16, B); 2], second: [(u16, B); 3]) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::validate(offset, &selector, |selector| {
            ctx.admit_iter(selector, "NX FSET selector syntax")
        })?.map(|()| Self { offset, selector, first, second }))
    }

    fn validate<'a, E, I: Iterator<Item = char>>(offset: u64, selector: &'a str, admit: impl FnOnce(&'a str) -> Result<I, E>) -> Result<Result<(), &'static str>, E> {
        if !(1..=247).contains(&selector.len()) || !admit(selector)?.all(|ch| ch.is_ascii_graphic() && ch != '>') {
            return Ok(Err("selector: requires 1 through 247 graphic ASCII bytes excluding >"));
        }
        Ok(offset.checked_add(cadmpeg_core::decode::u64_from_index(selector.len()) + 22)
            .map(|_| ()).ok_or("source_offset: FSET frame overflows"))
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
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
    ) -> Result<Option<Self>, CodecError> {
        if record.name() != "FSET" {
            return Ok(None);
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
        let decode = |start: usize| -> Result<Option<Self>, CodecError> {
            if bytes.get(start) != Some(&1) {
                return Ok(None);
            }
            let Some(len) = bytes.get(start + 1).copied().map(usize::from) else {
                return Ok(None);
            };
            if len < 9 {
                return Ok(None);
            }
            let Some(body_start) = start.checked_add(2) else {
                return Ok(None);
            };
            let Some(body_end) = body_start.checked_add(len) else {
                return Ok(None);
            };
            if bytes.get(body_start) != Some(&b'<') || bytes.get(body_end - 1) != Some(&b'>') {
                return Ok(None);
            }
            let selector_end = body_end - 7;
            let Some(raw) = bytes.get(body_start + 1..selector_end) else {
                return Ok(None);
            };
            let Ok(selector) =
                ctx.validate_utf8(raw, "NX FSET selector UTF-8 validation")?
            else {
                return Ok(None);
            };
            let mut at = selector_end;
            let Some(first_head) = read_word(&mut at) else { return Ok(None); };
            let Some(first_tail) = read_word(&mut at) else { return Ok(None); };
            let first = [first_head, first_tail];
            at += 1;
            let Some(second_head) = read_word(&mut at) else { return Ok(None); };
            let Some(second_middle) = read_word(&mut at) else { return Ok(None); };
            let Some(second_tail) = read_word(&mut at) else { return Ok(None); };
            let second = [second_head, second_middle, second_tail];
            let Some(end) = at.checked_add(3) else { return Ok(None); };
            if bytes.get(at..end) != Some(&[0, 3, 0]) {
                return Ok(None);
            }
            Ok(Self::from_wire(
                ctx,
                cadmpeg_core::decode::u64_from_index(record.payload_offset() + start),
                ctx.format_retained(format_args!("{selector}"), "NX FSET selector")?,
                first,
                second,
            )?.ok())
        };
        let Some(last) = bytes.len().checked_sub(1) else {
            return Ok(None);
        };
        let mut candidate = None;
        for start in ctx.admit_iter(&(0..last), "NX FSET reference candidate search")? {
            let Some(next) = decode(start)? else { continue; };
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(next);
        }
        Ok(candidate)
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
        let mapped = FsetReferences {
            offset,
            selector: self.selector,
            first: [
                (first_head, target(first_head)?),
                (first_tail, target(first_tail)?),
            ],
            second: [
                (second_head, target(second_head)?),
                (second_middle, target(second_middle)?),
                (second_tail, target(second_tail)?),
            ],
        };
        if offset.checked_add(cadmpeg_core::decode::u64_from_index(mapped.selector.len()) + 22).is_none() {
            return Ok(None);
        }
        Ok(Some(mapped))
    }
}

#[cfg(test)]
mod tests {
    use super::FsetReferences;
    use crate::om::operation_record::OperationPayload;

    fn graph_payload() -> [u8; 33] {
        [
            1, 0x13, 0x3c, b'T', b';', b':', b'S', b'5', b'6', b'7', b'R', b'8', b'9', b'3', 0x90,
            0x19, 0x40, 0x90, 0x19, 0x41, 0x3e, 0x90, 0x19, 0x30, 0x90, 0x19, 0x31, 0x90, 0x19,
            0x32, 0, 3, 0,
        ]
    }

    fn graph() -> FsetReferences<()> {
        let payload = graph_payload();
        let record = OperationPayload::new(&payload, 100, "FSET").expect("test FSET payload");
        crate::test_support::with_decode_context(|ctx| {
            FsetReferences::read(ctx, record).unwrap().expect("complete FSET graph")
        })
    }

    #[test]
    fn fset_resolution_returns_collection_refusal() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
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
            },
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

    fn fset_selector_format_refusal(dimension: cadmpeg_core::decode::ResourceDimension) {
        use cadmpeg_core::decode::ResourceDimension;
        let payload = graph_payload();
        let record = OperationPayload::new(&payload, 100, "FSET").unwrap();
        cadmpeg_test_support::refusal::resource_limit_at(dimension, "NX FSET selector", |cap| {
            crate::test_support::with_decode_context_over(
                &[],
                |policy| match dimension {
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    _ => panic!("selector formatting uses work and retained bytes"),
                },
                |ctx| {
                    let result = FsetReferences::read(ctx, record);
                    if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    result
                },
            )
        });
    }

    #[test]
    fn fset_selector_format_refuses_work() {
        fset_selector_format_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
    }

    #[test]
    fn fset_selector_format_refuses_retained_bytes() {
        fset_selector_format_refusal(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    }
}
