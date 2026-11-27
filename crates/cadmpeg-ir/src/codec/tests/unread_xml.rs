// SPDX-License-Identifier: Apache-2.0
//! The decode wrapper charges what the session's XML parses left unread from
//! a DOCTYPE.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::super::loss::DecodeLossCode;
use super::super::{DecodeResult, FormatId};
use super::decoded;
use crate::examples::unit_cube;
use crate::report::strictness::Strictness;
use crate::report::Severity;

#[test]
fn unread_doctype_parts_are_charged_once_as_decode_losses() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default()).expect("empty root");
    let text = "<!DOCTYPE r SYSTEM 'r.dtd' [<!ENTITY x SYSTEM 'x.xml'>\
                <!ATTLIST r a CDATA 'd'>]><r>1&x;2&x;3</r>";
    for _ in 0..2 {
        let document = ctx
            .parse_xml(text, "read test XML")
            .expect("DOCTYPE admitted");
        assert_eq!(document.root_element().text(), Some("123"));
    }
    let finish = || {
        DecodeResult::from_decoded(
            &ctx,
            decoded(unit_cube().expect("valid unit cube fixture")),
            FormatId::new("test"),
            &Strictness::default(),
        )
        .expect("decode result")
    };
    let result = finish();
    let charged = |code: DecodeLossCode| {
        let kind = code.note("").code;
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == kind)
            .map(|loss| (loss.severity, loss.message.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        charged(DecodeLossCode::XmlExternalSubsetUnread),
        [(
            Severity::Info,
            "read test XML: 2 XML text(s) name an external DTD subset; the decoder does not \
             read it, so its declarations do not apply"
                .to_owned()
        )]
    );
    assert_eq!(
        charged(DecodeLossCode::XmlAttributeDefaultsUnapplied),
        [(
            Severity::Warning,
            "read test XML: the DTD of 2 XML text(s) declares attribute defaults; the decoder \
             does not apply them"
                .to_owned()
        )]
    );
    assert_eq!(
        charged(DecodeLossCode::XmlExternalEntityUnexpanded),
        [(
            Severity::Warning,
            "read test XML: 4 reference(s) in 2 XML text(s) name an external entity; the \
             decoder fetches no external entity and expands each to empty text"
                .to_owned()
        )]
    );
    let again = finish();
    assert!(again
        .report()
        .losses
        .iter()
        .all(|loss| !loss.code.to_string().starts_with("decode/xml.")));
}
