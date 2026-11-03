// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::Confidence;

use super::{confidence, header, Cookie, Header};
use crate::test_support::usdc_header;

#[test]
fn a_text_header_names_its_cookie_and_version() {
    assert_eq!(
        header(b"#usda 1.0\ndef \"A\" {\n}\n"),
        Some(Header::Text {
            cookie: Cookie::Usda,
            version: "1.0".to_owned()
        })
    );
    assert_eq!(
        header(b"#sdf 1.4.32 trailing\n"),
        Some(Header::Text {
            cookie: Cookie::Sdf,
            version: "1.4.32".to_owned()
        })
    );
    assert_eq!(
        header(b"#usda 1.0\r\n"),
        Some(Header::Text {
            cookie: Cookie::Usda,
            version: "1.0".to_owned()
        })
    );
}

#[test]
fn a_binary_header_names_its_version_bytes() {
    assert_eq!(
        header(&usdc_header([0, 8, 0])),
        Some(Header::Binary { version: [0, 8, 0] })
    );
}

#[test]
fn other_bytes_are_not_a_layer() {
    for bytes in [
        &b"#usda\n"[..],
        b"#usda \n",
        b"ISO-10303-21;",
        b"PXR-USDX\0\x08\0",
        b"PXR-USDC\0",
        b"",
    ] {
        assert_eq!(header(bytes), None, "{bytes:?}");
        assert_eq!(confidence(bytes), Confidence::No);
    }
    assert_eq!(confidence(b"#usda 1.0\n"), Confidence::High);
}
