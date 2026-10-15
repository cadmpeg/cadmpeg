// SPDX-License-Identifier: Apache-2.0
use super::{next_header, rebase};
use crate::sab::{Record, Token};

#[test]
fn concatenated_header_requires_a_complete_line_and_known_boundaries() {
    assert_eq!(
        next_header(b"\n104 3 0 0\nbody $-1 $1 $-1 $-1 #\n", 0, false),
        Some(1)
    );
    assert!(next_header(b"104 3 0 #\n", 0, false).is_none());
    assert!(next_header(b"-0 body $-1 $1 $-1 $-1 #\n104 0 0 0\n", 0, true).is_none());
    let descriptive = b"result\n104 3 0 0\n";
    assert!(next_header(descriptive, 0, false).is_none());
    assert_eq!(next_header(descriptive, 0, true), Some(7));
    for terminator in ["End-of-ASM-data", "End-of-ACIS-data"] {
        let source = format!("{terminator}\nresult\n104 3 0 0\n");
        assert!(next_header(source.as_bytes(), 0, true).is_none());
    }
    for payload in [
        b"unknown @14\n104 3 0 0\n".as_slice(),
        b"unknown {\n104 3 0 0\n".as_slice(),
    ] {
        assert!(next_header(payload, 0, true).is_none());
    }
}

#[test]
fn table_rebasing_keeps_entity_and_subtype_references_in_their_stream() {
    let record = |index, tokens: Vec<Token>| Record {
        index,
        name: "audit".into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    let definition = || {
        vec![
            Token::SubtypeOpen,
            Token::Ident("exactcur".into()),
            Token::SubtypeClose,
        ]
    };
    let mut records = vec![
        record(0, definition()),
        record(
            1,
            vec![
                Token::Ref(0),
                Token::Ref(2),
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(0),
                Token::SubtypeClose,
            ],
        ),
        record(2, definition()),
        record(
            3,
            vec![
                Token::Ref(0),
                Token::Ref(-1),
                Token::SubtypeOpen,
                Token::Long(0),
                Token::SubtypeClose,
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(1),
                Token::SubtypeClose,
            ],
        ),
    ];
    rebase(&mut records, &[0..2, 2..4]).unwrap();
    assert_eq!(records[1].tokens[0], Token::Ref(0));
    assert_eq!(records[1].tokens[1], Token::Ref(6));
    assert_eq!(records[1].tokens[4], Token::Long(0));
    assert_eq!(records[3].tokens[0], Token::Ref(2));
    assert_eq!(records[3].tokens[1], Token::Ref(-1));
    assert_eq!(records[3].tokens[3], Token::Long(1));
    assert_eq!(records[3].tokens[7], Token::Long(3));
}
