// SPDX-License-Identifier: Apache-2.0

use crate::layout::parasolid_edited_type_header as header;
use cadmpeg_core::decode::View;
use cadmpeg_parasolid::records::{read_edit_program, BaseVersion, EditProgram};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    Body,
    Shell,
    Region,
    Face,
    World,
}

/// The base-schema edit program at `at` and the offset just past its `Z`.
/// A count of `ff` declares the base field list without a program.
fn edit_program(bytes: &[u8], at: usize, kind: u16) -> Option<(EditProgram, usize)> {
    let start = at.checked_add(header::FIELD_COUNT)?;
    let mut view = View::over_retained(bytes.get(start..)?);
    let count = view.u8()?;
    (count != 255).then_some(())?;
    let program = read_edit_program(&mut view, BaseVersion::V13006, kind, count).ok()??;
    Some((program, start.checked_add(view.position())?))
}

/// An edited ownership-node header whose program consumes every base field
/// (spec §4.1), and the offset of the class's first record after it.
pub(super) fn edited_payload(bytes: &[u8], at: usize) -> Option<(Class, usize)> {
    let kind = View::u16_be_at(bytes, at.checked_add(header::NODE_TYPE)?)?;
    let class = match kind {
        12 => Class::Body,
        13 => Class::Shell,
        14 => Class::Face,
        19 => Class::Region,
        101 => Class::World,
        _ => return None,
    };
    let (program, end) = edit_program(bytes, at, kind)?;
    program.consumes_base().then_some((class, end))
}

/// An edited type header of any class (spec §4.1): its node type and the
/// offset just past its closing `Z`, where the class's first record follows
/// without a tag of its own.
pub(super) fn edited_header(bytes: &[u8], at: usize) -> Option<(u16, usize)> {
    let kind = View::u16_be_at(bytes, at.checked_add(header::NODE_TYPE)?)?;
    let (_, end) = edit_program(bytes, at, kind)?;
    Some((kind, end))
}

#[cfg(test)]
mod tests {
    use super::{edited_header, edited_payload, Class};

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
    fn an_edited_header_of_any_class_ends_after_its_terminator() {
        // A class `edited_payload` does not type, with one appended field.
        let mut bytes = copied(0x26, 11);
        bytes[2] = 12;
        bytes.pop();
        bytes.push(b'A');
        pointer(&mut bytes, b"intersection_data", 1);
        bytes.push(b'Z');
        assert_eq!(edited_payload(&bytes, 0), None);
        assert_eq!(edited_header(&bytes, 0), Some((0x26, bytes.len())));
        // A field count the program does not reach is not a header.
        bytes[2] = 13;
        assert_eq!(edited_header(&bytes, 0), None);
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
        pointer(&mut bytes, b"Z CCCCCCCCCZCIA", 1);
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
