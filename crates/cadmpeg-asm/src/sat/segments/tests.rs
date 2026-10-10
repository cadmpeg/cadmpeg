// SPDX-License-Identifier: Apache-2.0
use super::{next_header, rebase};
use crate::sab::{Record, Token};

#[test]
fn concatenated_header_requires_a_complete_line_and_known_boundaries() {
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        next_header(&ctx, b"\n104 3 0 0\nbody $-1 $1 $-1 $-1 #\n", 0, false).unwrap(),
        Some(1)
    );
    assert!(next_header(&ctx, b"104 3 0 #\n", 0, false)
        .unwrap()
        .is_none());
    assert!(
        next_header(&ctx, b"-0 body $-1 $1 $-1 $-1 #\n104 0 0 0\n", 0, true)
            .unwrap()
            .is_none()
    );
    let descriptive = b"result\n104 3 0 0\n";
    assert!(next_header(&ctx, descriptive, 0, false).unwrap().is_none());
    assert_eq!(next_header(&ctx, descriptive, 0, true).unwrap(), Some(7));
    for terminator in ["End-of-ASM-data", "End-of-ACIS-data"] {
        let source = format!("{terminator}\nresult\n104 3 0 0\n");
        assert!(next_header(&ctx, source.as_bytes(), 0, true)
            .unwrap()
            .is_none());
    }
    for payload in [
        b"unknown @14\n104 3 0 0\n".as_slice(),
        b"unknown {\n104 3 0 0\n".as_slice(),
    ] {
        assert!(next_header(&ctx, payload, 0, true).unwrap().is_none());
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
    let ctx = cadmpeg_test_support::service_decode_context();
    rebase(&ctx, &mut records, &[0..2, 2..4]).unwrap();
    assert_eq!(records[1].tokens[0], Token::Ref(0));
    assert_eq!(records[1].tokens[1], Token::Ref(6));
    assert_eq!(records[1].tokens[4], Token::Long(0));
    assert_eq!(records[3].tokens[0], Token::Ref(2));
    assert_eq!(records[3].tokens[1], Token::Ref(-1));
    assert_eq!(records[3].tokens[3], Token::Long(1));
    assert_eq!(records[3].tokens[7], Token::Long(3));
}

#[test]
fn table_rebasing_charges_record_visits_without_payload_tokens() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut records: Vec<_> = (0..2)
        .map(|index| Record {
            index,
            name: "audit".into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rebase(&ctx, &mut records, &[0..1, 1..2]).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal required")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "rebase SAT stream references");
}
