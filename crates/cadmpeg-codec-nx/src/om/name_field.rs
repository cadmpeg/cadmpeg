// SPDX-License-Identifier: Apache-2.0
//! Bounded graphic names with a type-free leading or compact-typed frame.

use super::compact::{CompactIndexAtom, CompactIndexTarget, PositionedIndex};
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
        let text = value.as_str();
        if text.is_empty() || text.len() > 253 || !text.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("value: expected 1..=253 graphic ASCII bytes");
        }
        let form = match code {
            None if offset == 0 => Form::Leading,
            None => return Err("payload_offset: payload-leading name must start at zero"),
            Some(code) => {
                let byte_len = 4 + code.atom.raw().len() as u64 + text.len() as u64;
                offset
                    .checked_add(byte_len)
                    .ok_or("payload_offset: name frame extent overflow")?;
                Form::Typed { offset, code }
            }
        };
        Ok(Self { form, value })
    }
}

impl NameField<&str, usize, ()> {
    pub(crate) fn into_native(
        self,
        source_offset: impl FnOnce(u64) -> Option<u64>,
    ) -> Option<NameField<String>> {
        let offset = u64::try_from(self.offset()).ok()?;
        let code = match self.code() {
            None => None,
            Some(code) => Some(CompactIndexTarget {
                atom: code.atom,
                target: source_offset(u64::try_from(code.offset).ok()?),
            }),
        };
        NameField::new(self.value.to_owned(), offset, code).ok()
    }
}

/// Decode exact `66, compact_type, 03, declared_len, text, 00` fields.
pub(crate) fn scan(bytes: &[u8]) -> Vec<NameField<&str, usize, ()>> {
    let mut fields = Vec::new();
    if bytes.first() == Some(&3) {
        if let Some(value) = name_text(bytes, 1) {
            fields.push(NameField {
                form: Form::Leading,
                value,
            });
        }
    }
    for start in 0..bytes.len().saturating_sub(5) {
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
        let Some(value) = name_text(bytes, marker + 1) else {
            continue;
        };
        fields.push(NameField {
            form: Form::Typed {
                offset: start,
                code: atom.into(),
            },
            value,
        });
    }
    fields
}

fn name_text(bytes: &[u8], length_offset: usize) -> Option<&str> {
    let text_len = usize::from(bytes.get(length_offset).copied()?.checked_sub(2)?);
    let text_start = length_offset.checked_add(1)?;
    let text_end = text_start.checked_add(text_len)?;
    let text = bytes.get(text_start..text_end)?;
    if text.is_empty() || !text.iter().all(u8::is_ascii_graphic) || bytes.get(text_end) != Some(&0)
    {
        return None;
    }
    std::str::from_utf8(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_name_frames_bound_text_and_full_extent() {
        for raw in [&[1][..], &[128, 1][..]] {
            let code = CompactIndexTarget {
                atom: CompactIndexAtom::from_wire(1, raw).unwrap(),
                target: (),
            };
            for length in [1, 253] {
                let text = "A".repeat(length);
                let byte_len = 4 + raw.len() as u64 + length as u64;
                let last_offset = u64::MAX - byte_len;
                let frame = NameField::new(text.clone(), last_offset, Some(code)).unwrap();
                assert_eq!(frame.code().unwrap().offset, last_offset + 1);
                assert_eq!(frame.code().unwrap().atom.raw(), raw);
                assert!(NameField::new(text, last_offset + 1, Some(code)).is_err());
            }
            for text in [
                "".to_owned(),
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
        let field = scan(&bytes).pop().unwrap();
        let native = field.clone().into_native(|_| Some(900)).unwrap();
        assert_eq!(native.offset(), 0);
        assert_eq!(native.code().unwrap().offset, 1);
        assert_eq!(*native.code().unwrap().target, Some(900));
        assert_eq!(native.value(), "A");
        let unmapped = field.into_native(|_| None).unwrap();
        assert_eq!(*unmapped.code().unwrap().target, None);
        let leading = scan(&[3, 3, b'A', 0])
            .pop()
            .unwrap()
            .into_native(|_| panic!("leading name has no type token"))
            .unwrap();
        assert_eq!(leading.offset(), 0);
        assert!(leading.code().is_none());
    }
}
