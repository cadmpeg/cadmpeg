// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn both_dialects_keep_reference_roles_and_full_width_selectors() {
    for tuple in [
        vec![0x82, 0xf2, 0xf0, 0x82],
        vec![0x82, 0x32, 0xe6, 0, 0, 0, 0x32, 0xe4, 0, 0, 0, 0x82],
    ] {
        for tail in [vec![0x81, 0x06], vec![0x82, 0xd4, 0x34, 0x06]] {
            let mut body = vec![0x10, 0xfe, 0xd4, 0x32];
            body.extend(&tuple);
            body.extend([0xd1, 0xfd]);
            body.extend(&tail);
            let field = parse(&body).expect("complete strict inline field");
            assert_eq!(
                field.self_field,
                FieldReference {
                    identity: 819,
                    offset: 2
                }
            );
            assert_eq!(field.anchor_field.identity, 254);
            assert_eq!(
                field.value_field.map(|value| value.identity),
                (tail[0] == 0x82).then_some(821)
            );
            if tuple.len() == 12 {
                assert_eq!(
                    field.schema,
                    SchemaTuple::Words {
                        outer: 230,
                        field: 228
                    }
                );
            }
            for length in 0..body.len() {
                assert!(parse(&body[..length]).is_none());
            }
            body.push(0);
            assert!(parse(&body).is_none());
        }
    }
}

#[test]
fn counts_and_delimiters_do_not_become_entity_references() {
    let body = [0x10, 0xfe, 0x82, 0x82, 0xf2, 0xf0, 0x82, 0x83, 0x81, 0x06];
    let field = parse(&body).expect("reference identity two in self slot");
    assert_eq!(field.self_field.identity, 2);
    assert_eq!(field.anchor_field.identity, 3);
    assert_eq!(field.value_field, None);
    for index in [0, 1, 3, 4, 5, 6, 8, 9] {
        let mut damaged = body;
        damaged[index] ^= 1;
        assert!(parse(&damaged).is_none(), "changed control at {index}");
    }
}
