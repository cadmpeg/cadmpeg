// SPDX-License-Identifier: Apache-2.0
//! Bounded graphic names with a type-free leading or compact-typed frame.

use super::compact::{CompactIndexAtom, CompactIndexTarget, PositionedIndex};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::ops::Add;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Form<O, T> {
    Leading,
    Typed {
        offset: O,
        code: CompactIndexTarget<T>,
    },
}

/// A complete name frame. The type-free form starts at payload offset zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameField<S, O = u64, T = Option<u64>> {
    form: Form<O, T>,
    value: S,
}

impl<S: AsRef<str>, O: Copy + From<u8> + Add<Output = O>, T> NameField<S, O, T> {
    pub(crate) fn offset(&self) -> O {
        match self.form {
            Form::Leading => O::from(0),
            Form::Typed { offset, .. } => offset,
        }
    }

    pub(crate) fn code(&self) -> Option<PositionedIndex<'_, T, O>> {
        match &self.form {
            Form::Leading => None,
            Form::Typed { offset, code } => Some(PositionedIndex {
                atom: code.atom,
                target: &code.target,
                offset: *offset + O::from(1),
            }),
        }
    }

    pub(crate) fn value(&self) -> &str {
        self.value.as_ref()
    }
}

impl<T> NameField<String, u64, T> {
    pub(crate) fn new(
        value: String,
        offset: u64,
        code: Option<CompactIndexTarget<T>>,
    ) -> Result<Self, &'static str> {
        let form = match Self::validate(&value, offset, code, |text| {
            Ok::<_, std::convert::Infallible>(text.chars())
        }) {
            Ok(form) => form?,
            Err(error) => match error {},
        };
        Ok(Self { form, value })
    }

    fn from_wire(
        ctx: &DecodeContext<'_>, value: String, offset: u64,
        code: Option<CompactIndexTarget<T>>,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::validate(&value, offset, code, |text| {
            ctx.admit_iter(text, "NX native name field validation")
        })?.map(|form| Self { form, value }))
    }

    fn validate<'a, E, I: Iterator<Item = char>>(
        text: &'a str, offset: u64, code: Option<CompactIndexTarget<T>>,
        admit: impl FnOnce(&'a str) -> Result<I, E>,
    ) -> Result<Result<Form<u64, T>, &'static str>, E> {
        if text.is_empty() || text.len() > 253 || !admit(text)?.all(|ch| ch.is_ascii_graphic())
        {
            return Ok(Err("value: expected 1..=253 graphic ASCII bytes"));
        }
        let form = match code {
            None if offset == 0 => Form::Leading,
            None => return Ok(Err("payload_offset: payload-leading name must start at zero")),
            Some(code) => {
                let byte_len = 4
                    + cadmpeg_core::decode::u64_from_index(code.atom.raw().len())
                    + cadmpeg_core::decode::u64_from_index(text.len());
                if offset.checked_add(byte_len).is_none() {
                    return Ok(Err("payload_offset: name frame extent overflow"));
                }
                Form::Typed { offset, code }
            }
        };
        Ok(Ok(form))
    }
}

impl NameField<&str, usize, ()> {
    pub(crate) fn into_native(
        self,
        ctx: &DecodeContext<'_>,
        source_offset: impl FnOnce(u64) -> Option<u64>,
    ) -> Result<Option<NameField<String>>, CodecError> {
        let Some(offset) = u64::try_from(self.offset()).ok() else {
            return Ok(None);
        };
        let code = self.code().map(|code| CompactIndexTarget {
            atom: code.atom,
            target: source_offset(u64_from_index(code.offset)),
        });
        let mut value = ctx.retained_string(self.value.len(), "NX native name field")?;
        ctx.append_retained(&mut value, self.value, "NX admitted text append")?;
        Ok(NameField::from_wire(ctx, value, offset, code)?.ok())
    }
}

/// Decode exact `66, compact_type, 03, declared_len, text, 00` fields.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<NameField<&'a str, usize, ()>>, CodecError> {
    let mut fields = Vec::new();
    ctx.charge_work(u64_from_index(bytes.len()), "scan NX name fields")?;
    if bytes.first() == Some(&3) {
        if let Some(value) = name_text(ctx, bytes, 1)? {
            ctx.reserve_vec(&mut fields, 1, "NX name fields")?;
            fields.push(NameField {
                form: Form::Leading,
                value,
            });
        }
    }
    for start in bytes
        .len()
        .checked_sub(5)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if bytes[start] != 0x66 {
            continue;
        }
        let Some(atom) = bytes.get(start + 1..).and_then(CompactIndexAtom::read) else {
            continue;
        };
        let marker = start + 1 + atom.raw().len();
        if bytes.get(marker) != Some(&3) {
            continue;
        }
        let Some(value) = name_text(ctx, bytes, marker + 1)? else {
            continue;
        };
        ctx.reserve_vec(&mut fields, 1, "NX name fields")?;
        fields.push(NameField {
            form: Form::Typed {
                offset: start,
                code: atom.into(),
            },
            value,
        });
    }
    Ok(fields)
}

fn name_text<'a>(ctx: &DecodeContext<'_>, bytes: &'a [u8], length_offset: usize) -> Result<Option<&'a str>, CodecError> {
    (|| {
        let text_len = usize::from(bytes.get(length_offset).copied()?.checked_sub(2)?);
        let text_start = length_offset.checked_add(1)?;
        let text_end = text_start.checked_add(text_len)?;
        let text = bytes.get(text_start..text_end)?;
        if text.is_empty() || !propagate_resource!(ctx.admit_iter(text, "NX name text validation").map_err(CodecError::from)).all(u8::is_ascii_graphic)
            || bytes.get(text_end) != Some(&0) { return None; }
        std::str::from_utf8(text).ok().map(Ok)
    })().transpose()
}

#[cfg(test)]
mod tests {
    use super::super::compact::CompactIndexAtom;
    use super::super::compact::CompactIndexTarget;
    use super::{scan, NameField};

    fn scan_test(bytes: &[u8]) -> Vec<NameField<&str, usize, ()>> {
        crate::test_support::with_decode_context(|ctx| scan(ctx, bytes)).unwrap()
    }

    #[test]
    fn name_field_scan_refuses_collection_limit() {
        let bytes = [3, 3, b'A', 0];

        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = scan(ctx, &bytes).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn native_name_frames_bound_text_and_full_extent() {
        for raw in [&[1][..], &[128, 1][..]] {
            let code = CompactIndexTarget {
                atom: CompactIndexAtom::from_wire(1, raw).unwrap(),
                target: (),
            };
            for length in [1, 253] {
                let text = "A".repeat(length);
                let byte_len = 4
                    + cadmpeg_core::decode::u64_from_index(raw.len())
                    + cadmpeg_core::decode::u64_from_index(length);
                let last_offset = u64::MAX - byte_len;
                let frame = NameField::new(text.clone(), last_offset, Some(code)).unwrap();
                assert_eq!(frame.code().unwrap().offset, last_offset + 1);
                assert_eq!(frame.code().unwrap().atom.raw(), raw);
                assert!(NameField::new(text, last_offset + 1, Some(code)).is_err());
            }
            for text in [
                String::new(),
                "A".repeat(254),
                "A B".to_owned(),
                "A\0B".to_owned(),
                "é".to_owned(),
            ] {
                assert!(NameField::new(text, 0, Some(code)).is_err());
            }
        }
        assert!(NameField::<_, u64, ()>::new("A".to_owned(), 0, None).is_ok());
        assert!(NameField::<_, u64, ()>::new("A".to_owned(), 1, None).is_err());
    }

    #[test]
    fn source_name_mapping_keeps_optional_noncontiguous_source_positions() {
        let bytes = [0x66, 128, 1, 3, 3, b'A', 0];
        let field = scan_test(&bytes).pop().unwrap();
        let native = crate::test_support::with_decode_context(|ctx| {
            field.clone().into_native(ctx, |_| Some(900))
        })
        .unwrap()
        .unwrap();
        assert_eq!(native.offset(), 0);
        assert_eq!(native.code().unwrap().offset, 1);
        assert_eq!(*native.code().unwrap().target, Some(900));
        assert_eq!(native.value(), "A");
        let unmapped =
            crate::test_support::with_decode_context(|ctx| field.into_native(ctx, |_| None))
                .unwrap()
                .unwrap();
        assert_eq!(*unmapped.code().unwrap().target, None);
        let leading = scan_test(&[3, 3, b'A', 0]).pop().unwrap();
        let leading = crate::test_support::with_decode_context(|ctx| {
            leading.into_native(ctx, |_| panic!("leading name has no type token"))
        })
        .unwrap()
        .unwrap();
        assert_eq!(leading.offset(), 0);
        assert!(leading.code().is_none());
    }

    #[test]
    fn om_sketch_name_field_decodes_direct_and_extended_compact_type_codes() {
        let bytes = [
            0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0xaa, 0x66, 0x80,
            0x83, 0x03, 0x07, b'L', b'i', b'n', b'e', b'2', 0x00,
        ];
        let fields = scan_test(&bytes);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].offset(), 0);
        assert_eq!(fields[0].value(), "Point1");
        let first = fields[0].code().expect("typed name");
        assert_eq!(first.atom.value(), 0x32);
        assert_eq!(first.atom.raw(), vec![0x32]);
        assert_eq!(first.offset, 1);
        assert_eq!(fields[1].offset(), 12);
        assert_eq!(fields[1].value(), "Line2");
        let second = fields[1].code().expect("typed name");
        assert_eq!(second.atom.value(), 0x83);
        assert_eq!(second.atom.raw(), vec![0x80, 0x83]);
        assert_eq!(second.offset, 13);

        assert!(
            scan_test(&[0x66, 0xff, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00,])
                .is_empty()
        );
        assert!(scan_test(&[0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't',]).is_empty());
    }

    #[test]
    fn om_sketch_name_field_decodes_type_free_payload_leading_form() {
        let fields = scan_test(&[0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0x04]);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].offset(), 0);
        assert!(fields[0].code().is_none());
        assert_eq!(fields[0].value(), "Point1");

        assert!(scan_test(&[0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1',]).is_empty());
    }

    #[test]
    fn native_name_field_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[], ResourceDimension::WorkUnits, "NX native name field validation",
            |ctx| { NameField::<_, u64, ()>::from_wire(ctx, "Name".to_owned(), 0, None) },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX native name field validation"));
    }
}
