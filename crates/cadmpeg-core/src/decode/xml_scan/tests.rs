// SPDX-License-Identifier: Apache-2.0
use super::{XmlElementScan, XmlEncoding, XmlFramingIssue, XmlTermination};
use crate::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension,
};
use crate::CodecError;

fn names(scan: &XmlElementScan<'_>) -> Vec<Vec<u8>> {
    scan.elements()
        .iter()
        .map(|element| element.name().window().to_vec())
        .collect()
}

#[test]
fn complete_nested_and_empty_elements_keep_exact_xml_framing() {
    let bytes = b"<root><same><same/></same><last /></root>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
        .expect("scan");
    assert!(scan.faults().is_empty());
    assert_eq!(
        names(&scan),
        [
            b"root".to_vec(),
            b"same".to_vec(),
            b"same".to_vec(),
            b"last".to_vec()
        ]
    );
    assert_eq!(
        scan.elements()
            .iter()
            .map(|node| node.parent())
            .collect::<Vec<_>>(),
        [None, Some(0), Some(1), Some(0)]
    );
    assert_eq!(scan.elements()[0].extent().window(), bytes);
    assert_eq!(
        scan.elements()[1].extent().window(),
        b"<same><same/></same>"
    );
    assert_eq!(
        scan.elements()[2].termination(),
        XmlTermination::EmptyElement
    );
    assert_eq!(
        scan.elements()[3].termination(),
        XmlTermination::EmptyElement
    );
    assert!(scan.elements().iter().all(|node| !node.nesting_damage()));
    assert_eq!(
        scan.elements()[0]
            .framing()
            .map(|view| view.window())
            .collect::<Vec<_>>(),
        [b"<root".as_slice(), b">", b"</root>"]
    );
    assert_eq!(
        scan.elements()[3]
            .framing()
            .map(|view| view.window())
            .collect::<Vec<_>>(),
        [b"<last".as_slice(), b"/>"]
    );
}

#[test]
fn comments_cdata_pi_quotes_and_doctype_subsets_do_not_create_elements() {
    let bytes = br#"<?xml version="1.0"?><!DOCTYPE root [<!ENTITY x "</root><fake/>"><!-- ]> <fake/> --><?inside ]> ?>]><root value="</root><fake/>" other='>'><!-- </root><fake/> --><![CDATA[</root><fake/>]]><?inside <fake/> ?><good/></root>"#;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
        .expect("scan");
    assert!(scan.faults().is_empty());
    assert_eq!(names(&scan), [b"root".to_vec(), b"good".to_vec()]);
    assert_eq!(scan.elements()[0].termination(), XmlTermination::EndTag);
    assert_eq!(scan.elements()[1].parent(), Some(0));
}

#[test]
fn damaged_and_unsupported_elements_do_not_hide_valid_later_siblings() {
    let bytes = b"<root><bad a=unquoted>&missing;</bad><unsupported/><good value='7'/></root>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
        .expect("framed content");
    assert_eq!(
        names(&scan),
        [
            b"root".to_vec(),
            b"bad".to_vec(),
            b"unsupported".to_vec(),
            b"good".to_vec()
        ]
    );
    let bad = scan.elements()[1].extent();
    let damaged = ctx
        .part_transaction(bad, || {
            let text = std::str::from_utf8(bad.window()).map_err(CodecError::malformed)?;
            ctx.parse_xml(text, "bad XML element").map(drop)
        })
        .expect("part failure is recoverable");
    assert!(matches!(damaged.result(), Err(CodecError::Malformed(_))));
    let unknown = scan.elements()[2].extent();
    let unsupported = ctx
        .part_transaction(unknown, || {
            Err::<(), _>(CodecError::NotImplemented("unknown element family".into()))
        })
        .expect("unsupported child");
    assert!(matches!(
        unsupported.result(),
        Err(CodecError::NotImplemented(_))
    ));
    let good = scan.elements()[3].extent();
    let parsed = ctx
        .parse_xml(
            std::str::from_utf8(good.window()).expect("ASCII"),
            "good XML element",
        )
        .expect("later sibling");
    assert_eq!(parsed.root_element().attribute("value"), Some("7"));
}

#[test]
fn mismatched_nested_end_tags_recover_at_an_actual_ancestor_close() {
    let bytes = b"<root><bad><inner></wrong></bad><good/></root>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
        .expect("recover");
    assert_eq!(
        scan.elements()[1].extent().window(),
        b"<bad><inner></wrong></bad>"
    );
    assert_eq!(scan.elements()[1].termination(), XmlTermination::EndTag);
    assert!(scan.elements()[1].nesting_damage());
    assert_eq!(scan.elements()[2].extent().window(), b"<inner></wrong>");
    assert_eq!(
        scan.elements()[2].termination(),
        XmlTermination::AncestorBoundary
    );
    assert_eq!(scan.elements()[3].extent().window(), b"<good/>");
    assert_eq!(scan.elements()[3].parent(), Some(0));
    assert_eq!(scan.faults().len(), 1);
    assert_eq!(scan.faults()[0].issue(), XmlFramingIssue::UnexpectedEndTag);
    assert_eq!(scan.faults()[0].extent().window(), b"</wrong>");
}

#[test]
fn cut_short_suffix_and_unclosed_shielding_never_invent_sibling_boundaries() {
    let arena = DecodeArena::new();
    let source = b"<root><before/><bad><child>tail";
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
        .expect("cut short");
    assert_eq!(
        scan.elements()[1].termination(),
        XmlTermination::EmptyElement
    );
    assert_eq!(scan.elements()[2].extent().window(), b"<bad><child>tail");
    assert_eq!(scan.elements()[2].termination(), XmlTermination::EndOfInput);
    assert_eq!(scan.elements()[3].termination(), XmlTermination::EndOfInput);
    for bytes in [
        b"<root><bad value='unterminated <fake/></root>".as_slice(),
        b"<root><!-- missing end <fake/></root>",
        b"<root><![CDATA[ missing end <fake/></root>",
        b"<!DOCTYPE root [ <!ENTITY x '>'> <fake/>",
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
        let scan = ctx
            .scan_xml_elements(root, XmlEncoding::Utf8, "XML framing")
            .expect("framing facts");
        assert!(scan
            .elements()
            .iter()
            .all(|node| node.name().window() != b"fake"));
        assert_eq!(scan.faults().len(), 1);
        assert_eq!(scan.faults()[0].extent().end(), bytes.len());
    }
}

#[test]
fn utf16_code_units_keep_exact_bytes_and_byte_offsets() {
    for encoding in [XmlEncoding::Utf16Le, XmlEncoding::Utf16Be] {
        let text = "<root note='</root>'><π/><last/></root>";
        let bytes = text
            .encode_utf16()
            .flat_map(|unit| {
                if encoding == XmlEncoding::Utf16Le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            })
            .collect::<Vec<_>>();
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let scan = ctx
            .scan_xml_elements(root, encoding, "UTF16 XML framing")
            .expect("scan");
        assert_eq!(scan.elements().len(), 3);
        assert_eq!(scan.elements()[0].extent().window(), bytes);
        let start = text.find("<π/>").expect("source position") * 2;
        assert_eq!(
            (
                scan.elements()[1].extent().start(),
                scan.elements()[1].extent().end()
            ),
            (start, start + 8)
        );
        assert_eq!(scan.elements()[1].parent(), Some(0));
        let mut truncated = bytes.clone();
        truncated.push(0xff);
        let other_arena = DecodeArena::new();
        let (other, root) =
            DecodeContext::from_root_bytes(&truncated, &other_arena, &DecodePolicy::service())
                .expect("root");
        let scan = other
            .scan_xml_elements(root, encoding, "UTF16 tail")
            .expect("facts");
        assert_eq!(scan.faults().len(), 1);
        assert_eq!(
            scan.faults()[0].issue(),
            XmlFramingIssue::IncompleteCodeUnit
        );
        assert_eq!(scan.faults()[0].extent().window(), [0xff]);
    }
}

#[test]
fn framing_preserves_alias_provenance_without_claiming_semantic_reads() {
    let bytes = b"HEAD<root><value>7</value></root>TAIL";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let root = ctx.name_stream(root, "file").expect("root name");
    let alias = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: 4,
                end: crate::decode::u64_from_index(bytes.len() - 4),
            },
            "document",
        )
        .expect("stored XML stream");
    let scan = ctx
        .record("xml:source:record#framing", alias, || {
            ctx.scan_xml_elements(alias, XmlEncoding::Utf8, "XML scan")
        })
        .expect("scan record");
    assert_eq!(scan.elements()[0].extent().location(), alias.location());
    assert_eq!(scan.elements()[1].name().window(), b"value");
    let capture = ctx
        .take_record_captures()
        .expect("capture")
        .pop()
        .expect("record");
    assert!(capture.reads().is_empty());
    assert_eq!(
        (capture.stream(), capture.offset(), capture.byte_len()),
        (
            "document",
            0,
            crate::decode::u64_from_index(bytes.len() - 8)
        )
    );
}

#[test]
fn semantic_depth_limit_is_local_to_one_framed_element() {
    let bytes = b"<root><deep><a><b><c/></b></a></deep><good/></root>";
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 3;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root");
    let scan = ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "iterative scan")
        .expect("framing is not recursive parsing");
    let deep = scan.elements()[1].extent();
    let outcome = ctx
        .part_transaction(deep, || {
            ctx.parse_xml(
                std::str::from_utf8(deep.window()).expect("ASCII"),
                "deep XML",
            )
            .map(drop)
        })
        .expect("part refusal");
    assert!(
        matches!(outcome.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(ctx.resource_refusal(), None);
    let good = scan
        .elements()
        .iter()
        .find(|node| node.name().window() == b"good")
        .expect("later sibling")
        .extent();
    assert!(ctx
        .parse_xml(
            std::str::from_utf8(good.window()).expect("ASCII"),
            "later XML"
        )
        .is_ok());
}

#[test]
fn scanner_obeys_shared_session_limits_and_checks_view_ownership() {
    for retained in [false, true] {
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = 0;
        } else {
            policy.limits.max_entities = 1;
        }
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(b"<root><child/></root>", &arena, &policy)
            .expect("root");
        let result = ctx.scan_xml_elements(root, XmlEncoding::Utf8, "bounded scan");
        if retained {
            let error = result.expect_err("shared refusal");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
            );
            assert!(ctx.resource_refusal().is_some());
        } else {
            // A framing node is not an entity; the records built from the
            // elements are charged by their decoder.
            let scan = result.expect("framing charges no entities");
            assert_eq!(scan.elements().len(), 2);
            assert_eq!(ctx.resource_refusal(), None);
            ctx.charge_entities(1, "the whole entity allowance remains")
                .expect("no entity charged by the scan");
        }
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"<a/>", &arena, &DecodePolicy::service()).expect("root");
    let other_arena = DecodeArena::new();
    let (other, _) =
        DecodeContext::from_root_bytes(b"<b/>", &other_arena, &DecodePolicy::service())
            .expect("other");
    assert!(matches!(
        other.scan_xml_elements(root, XmlEncoding::Utf8, "foreign scan"),
        Err(CodecError::Malformed(_))
    ));
    assert!(ctx
        .scan_xml_elements(root, XmlEncoding::Utf8, "correct owner")
        .is_ok());
}

#[test]
fn temporary_name_index_limit_is_part_scoped_and_does_not_clear_session_totals() {
    let bytes = b"<deep><inner/></deep><good/>";
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    policy.limits.max_entities = 2;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root");
    let split = bytes
        .windows(7)
        .position(|window| window == b"<good/>")
        .expect("literal sibling");
    let first = root.child(0, split).expect("first complete element");
    let second = root
        .child(split, bytes.len())
        .expect("next complete element");
    let retained_before = ctx.retained_bytes_available();
    let outcome = ctx
        .part_transaction(first, || {
            ctx.scan_xml_elements(first, XmlEncoding::Utf8, "limited name index")
        })
        .expect("part recovery");
    assert!(
        matches!(outcome.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert_eq!(ctx.resource_refusal(), None);
    let scan = ctx
        .scan_xml_elements(second, XmlEncoding::Utf8, "later empty sibling")
        .expect("no active-name index needed");
    assert_eq!(scan.elements().len(), 1);
    assert_eq!(scan.elements()[0].extent().window(), b"<good/>");
    assert!(
        ctx.retained_bytes_available() < retained_before,
        "attempted frame storage remains charged to the session"
    );
    // Framing nodes are not entities: the session entity allowance is whole.
    ctx.charge_entities(2, "the whole entity allowance remains")
        .expect("no entity charged by either scan");
}
