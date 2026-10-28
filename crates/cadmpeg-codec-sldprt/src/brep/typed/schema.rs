// SPDX-License-Identifier: Apache-2.0

use crate::layout::parasolid_edited_type_header as header;
use crate::parasolid::xt_integer;
use cadmpeg_core::decode::View;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::brep) enum Class {
    Body,
    Shell,
    Region,
    Face,
    World,
    Intersection,
}

impl Class {
    fn base_fields(self) -> usize {
        match self {
            Self::Body => 23,
            Self::Shell => 9,
            Self::Region => 7,
            Self::Face => 14,
            Self::World => 11,
            Self::Intersection => 11,
        }
    }
}

/// One complete edited first-of-type schema program.
pub(in crate::brep) struct Program<'a> {
    pub(in crate::brep) class: Class,
    /// Offset of the first node's fields, after the `Z` terminator.
    pub(in crate::brep) payload: usize,
    /// Every base field is copied in order, with no deletion or insertion.
    pub(in crate::brep) base_copied: bool,
    /// Names of the fields appended after the base fields, in order.
    pub(in crate::brep) appended: Vec<&'a str>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    fn short(&mut self) -> Option<u16> {
        let value = View::u16_be_at(self.bytes, self.at)?;
        self.at += 2;
        Some(value)
    }

    /// An XT positive integer: one less than its stored code.
    fn positive(&mut self) -> Option<u32> {
        xt_integer::read_code(self.bytes, &mut self.at)?.checked_sub(1)
    }

    fn string(&mut self) -> Option<&'a str> {
        let len = usize::from(self.byte()?);
        let end = self.at.checked_add(len)?;
        let value = std::str::from_utf8(self.bytes.get(self.at..end)?).ok()?;
        self.at = end;
        Some(value)
    }

    fn field(&mut self) -> Option<&'a str> {
        let name = self.string().filter(|name| !name.is_empty())?;
        let pointer = self.short()?;
        let elements = self.positive()?;
        if pointer == 0 {
            matches!(
                self.string()?,
                "u" | "c" | "l" | "n" | "w" | "d" | "p" | "f" | "i" | "v" | "b" | "h"
            )
            .then_some(())?;
        }
        if elements == 1 {
            (self.byte()? <= 1).then_some(())?;
        }
        Some(name)
    }
}

pub(super) fn edited_payload(bytes: &[u8], at: usize) -> Option<(Class, usize)> {
    edited_program(bytes, at).map(|program| (program.class, program.payload))
}

pub(in crate::brep) fn edited_program(bytes: &[u8], at: usize) -> Option<Program<'_>> {
    let class = match View::u16_be_at(bytes, at.checked_add(header::NODE_TYPE)?)? {
        12 => Class::Body,
        13 => Class::Shell,
        14 => Class::Face,
        19 => Class::Region,
        38 => Class::Intersection,
        101 => Class::World,
        _ => return None,
    };
    let mut cursor = Cursor {
        bytes,
        at: at.checked_add(header::FIELD_COUNT)?,
    };
    let current_fields = usize::from(cursor.byte()?);
    if current_fields == 255 {
        return None;
    }
    let mut base = 0;
    let mut current = 0;
    let mut base_copied = true;
    let mut appended = Vec::new();
    loop {
        match cursor.byte()? {
            b'Z' => {
                return (base == class.base_fields() && current == current_fields).then_some(
                    Program {
                        class,
                        payload: cursor.at,
                        base_copied,
                        appended,
                    },
                );
            }
            b'C' if base < class.base_fields() && current < current_fields => {
                base += 1;
                current += 1;
            }
            b'D' if base < class.base_fields() => {
                base += 1;
                base_copied = false;
            }
            b'I' if base < class.base_fields() && current < current_fields => {
                cursor.field()?;
                current += 1;
                base_copied = false;
            }
            b'A' if base == class.base_fields() && current < current_fields => {
                appended.push(cursor.field()?);
                current += 1;
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{edited_payload, Class};

    fn copied(tag: u8, count: u8) -> Vec<u8> {
        let mut bytes = vec![0, tag, count];
        bytes.extend(std::iter::repeat_n(b'C', usize::from(count)));
        bytes.push(b'Z');
        bytes
    }

    fn pointer(bytes: &mut Vec<u8>, name: &[u8], elements: u16) {
        bytes.push(u8::try_from(name.len()).unwrap());
        bytes.extend(name);
        bytes.extend([0, 82]);
        bytes.extend(elements.to_be_bytes());
    }

    #[test]
    fn complete_copy_programs_preserve_the_explicit_class() {
        for (tag, count, class) in [
            (12, 23, Class::Body),
            (13, 9, Class::Shell),
            (14, 14, Class::Face),
            (19, 7, Class::Region),
            (101, 11, Class::World),
        ] {
            let bytes = copied(tag, count);
            assert_eq!(edited_payload(&bytes, 0), Some((class, bytes.len())));
        }
    }

    #[test]
    fn edit_fields_consume_embedded_tags_and_terminators_atomically() {
        let mut bytes = copied(12, 23);
        bytes[2] = 24;
        bytes.pop();
        bytes.push(b'A');
        pointer(&mut bytes, b"Z\0\r\x09CCCCCCCCCZCIA", 1);
        bytes.push(b'Z');
        assert_eq!(edited_payload(&bytes, 0), Some((Class::Body, bytes.len())));
        for end in 0..bytes.len() {
            assert_eq!(edited_payload(&bytes[..end], 0), None);
        }
    }

    #[test]
    fn delete_insert_and_variable_field_grammar_is_bounded() {
        let mut bytes = vec![0, 13, 9, b'D', b'I'];
        pointer(&mut bytes, b"replacement", 2);
        bytes.push(1);
        bytes.extend([b'C'; 8]);
        bytes.push(b'Z');
        assert_eq!(edited_payload(&bytes, 0), Some((Class::Shell, bytes.len())));
        bytes[2] = 10;
        assert_eq!(edited_payload(&bytes, 0), None);
        bytes[2] = 9;
        let logical = bytes.len() - 10;
        bytes[logical] = 2;
        assert_eq!(edited_payload(&bytes, 0), None);
    }

    #[test]
    fn append_and_insert_must_match_the_base_cursor() {
        let mut early_append = vec![0, 12, 24, b'A'];
        pointer(&mut early_append, b"extra", 1);
        early_append.extend([b'C'; 23]);
        early_append.push(b'Z');
        assert_eq!(edited_payload(&early_append, 0), None);

        let mut late_insert = copied(12, 23);
        late_insert[2] = 24;
        late_insert.pop();
        late_insert.push(b'I');
        pointer(&mut late_insert, b"extra", 1);
        late_insert.push(b'Z');
        assert_eq!(edited_payload(&late_insert, 0), None);
        assert_eq!(edited_payload(&[0, 12, 255], 0), None);
    }
    #[test]
    fn world_program_extent_requires_all_fields_and_the_terminator() {
        let mut bytes = copied(101, 11);
        bytes[2] = 13;
        bytes.pop();
        bytes.push(b'A');
        pointer(&mut bytes, b"indices", 1);
        bytes.push(b'A');
        pointer(&mut bytes, b"b", 1);
        bytes.push(b'Z');
        assert_eq!(edited_payload(&bytes, 0), Some((Class::World, bytes.len())));
        for end in 0..bytes.len() {
            assert_eq!(edited_payload(&bytes[..end], 0), None);
        }
        bytes[2] = 12;
        assert_eq!(edited_payload(&bytes, 0), None);
        bytes[2] = 14;
        assert_eq!(edited_payload(&bytes, 0), None);
    }
}
