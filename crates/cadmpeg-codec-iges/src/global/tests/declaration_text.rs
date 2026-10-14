// SPDX-License-Identifier: Apache-2.0
use crate::global::{Resolution, Value};

#[test]
fn global_declaration_lossy_text_preserves_replacement_boundaries() {
    for (bytes, expected) in [
        (&b"a\xffb"[..], "a\u{fffd}b"),
        (&b"a\xe2\x82"[..], "a\u{fffd}"),
        (&b"a\xff\xffb"[..], "a\u{fffd}\u{fffd}b"),
        (&b"valid"[..], "valid"),
    ] {
        let resolution = Resolution {
            values: vec![Value::Malformed(bytes.to_vec())],
            losses: Vec::new(),
        };
        assert_eq!(resolution.declaration_text(0), expected);
    }
}
