// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::View;

use crate::layout::{
    mfc_new_class_header as class, part_root_long as long, part_root_short as short,
};

pub(crate) const TOKEN: &[u8] = b"moPart_c";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Prefix {
    pub(crate) token_offset: usize,
    pub(crate) body_offset: usize,
    pub(crate) first_word: u32,
    pub(crate) trailing_word: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Root {
    Absent,
    Unsupported,
    Supported(Prefix),
}

pub(super) fn declaration(bytes: &[u8], at: usize) -> Option<(u16, &[u8], usize)> {
    (View::u16_le_at(bytes, at + class::TAG)? == u16::MAX).then_some(())?;
    let schema = View::u16_le_at(bytes, at + class::SCHEMA)?;
    let count = usize::from(View::u16_le_at(bytes, at + class::NAME_LENGTH)?);
    let start = at.checked_add(class::LEN)?;
    let end = start.checked_add(count)?;
    Some((schema, bytes.get(start..end)?, end))
}

pub(crate) fn parse(bytes: &[u8]) -> Root {
    let Some((schema, name, body)) = declaration(bytes, 0) else {
        return Root::Absent;
    };
    if name != TOKEN {
        return Root::Absent;
    }
    if schema != 1 {
        return Root::Unsupported;
    }
    let Some(first_word) = View::u32_le_at(bytes, body + short::FIRST_WORD) else {
        return Root::Unsupported;
    };
    let short = declaration(bytes, body + short::LEN)
        .is_some_and(|(schema, name, _)| schema == 1 && name == b"moHeader_c");
    let long = View::u32_le_at(bytes, body + long::RESERVED) == Some(0)
        && declaration(bytes, body + long::LEN)
            .is_some_and(|(schema, name, _)| schema == 1 && name == b"moNodeName_c");
    let trailing_word = match (short, long) {
        (true, false) => None,
        (false, true) => View::u32_le_at(bytes, body + long::TRAILING_WORD),
        _ => return Root::Unsupported,
    };
    Root::Supported(Prefix {
        token_offset: class::LEN,
        body_offset: body,
        first_word,
        trailing_word,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse, Root};

    fn declaration(name: &[u8]) -> Vec<u8> {
        let mut bytes = vec![255, 255, 1, 0];
        bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend(name);
        bytes
    }

    #[test]
    fn root_forms_require_their_complete_scalar_prefix_and_successor_declaration() {
        for words in [vec![0u32], vec![15], vec![0, 0, 0], vec![29, 0, 37]] {
            let mut bytes = declaration(b"moPart_c");
            for word in &words {
                bytes.extend(word.to_le_bytes());
            }
            bytes.extend(declaration(if words.len() == 1 {
                b"moHeader_c"
            } else {
                b"moNodeName_c"
            }));
            let Root::Supported(prefix) = parse(&bytes) else {
                panic!("complete root")
            };
            assert_eq!(prefix.first_word, words[0]);
            assert_eq!(prefix.trailing_word, words.get(2).copied());
            for end in 0..bytes.len() {
                assert!(!matches!(parse(&bytes[..end]), Root::Supported(_)), "{end}");
            }
            let mut embedded = declaration(b"AuthoredOpaque_c");
            embedded.extend(&bytes);
            assert_eq!(parse(&embedded), Root::Absent);
            let mut unsupported = bytes.clone();
            unsupported[2] = 2;
            assert_eq!(parse(&unsupported), Root::Unsupported);
            if words.len() == 3 {
                unsupported = bytes.clone();
                unsupported[18] = 1;
                assert_eq!(parse(&unsupported), Root::Unsupported);
            }
        }
        assert_eq!(parse(b"unqlite\xa5class\xa8moPart_c\0\0\0\0"), Root::Absent);
        assert_eq!(parse(b"moPart_c\0\0\0\0"), Root::Absent);
    }
}
