// SPDX-License-Identifier: Apache-2.0
//! Document.xml persistence-graph unit tests.
use cadmpeg_test_support::wire;

use crate::test_support::test_archive::{archive, archive_entries};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

mod exhaustion;
mod storage_lifetimes;

#[test]
fn persistence_invalid_xml_diagnostic_refuses_at_retained_limit() {
    let bytes = b"<Document";
    crate::test_support::assert_retained_refusal_at(bytes, "FCStd persistence diagnostic", |ctx| {
        super::parse_with_context(bytes, "4", ctx)
    });
}

#[test]
fn persistence_object_identity_refuses_at_retained_limit() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Body"/></Objects><ObjectData Count="1"><Object name="Body"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let id_len = crate::native::native_id("object", "Body").len();
    assert_retained_operation(
        &parse_with_retained_limit(
            document,
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<super::ObjectRecord>()
                    + "Body".len()
                    + "Part::Feature".len()
                    + id_len,
            ) - 1,
        ),
        "FreeCAD native identity",
    );
}

#[test]
fn persistence_object_data_lookup_holds_borrowed_names() {
    let name = "Body";
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="{name}"/></Objects><ObjectData Count="1"><Object name="{name}"><Properties Count="0"/></Object></ObjectData></Document>"#
    );
    let nodes = 2 * document.bytes().filter(|byte| *byte == b'<').count() + 2;
    let attributes = document.bytes().filter(|byte| *byte == b'=').count();
    // The XML tree admission holds one scoped reservation for the whole parse:
    // node records (2 * nodes + 4 capacity, 192 bytes each), attribute records
    // (2 * attributes + 16 capacity, 256 bytes each), the single namespace
    // record (capacity 6, 66 bytes), inherited namespace indices
    // (2 * nodes + 4 capacity, 2 bytes each), 32 bytes for each node,
    // attribute and namespace string, eight input lengths and 1024 fixed bytes.
    let xml = (2 * nodes + 4) * (128 + 64)
        + (2 * attributes + 16) * (128 * 2)
        + (2 + 4) * (64 + 2)
        + (2 * nodes + 4) * 2
        + (nodes + attributes + 1) * 32
        + 8 * document.len()
        + 1024;
    // The first object-data record grows the empty lookup map to three entries:
    // a four-bucket table of `(&str, Node)` elements, 15 bytes of group
    // padding (alignment 16), four control bytes and a 16-byte trailer. The
    // key borrows the document, so no name copy follows the table.
    let lookup = 4 * std::mem::size_of::<(&str, roxmltree::Node<'_, '_>)>() + 15 + 4 + 16;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(xml + lookup - 1);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)
            .expect("source context");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        super::parse_with_context(document.as_bytes(), "4", &ctx)
    else {
        panic!("expected a resource refusal");
    };
    assert_eq!(
        (
            limit.dimension,
            limit.operation,
            limit.used,
            limit.additional
        ),
        (
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            "FCStd object data lookup",
            cadmpeg_core::decode::u64_from_index(xml),
            cadmpeg_core::decode::u64_from_index(lookup)
        )
    );
}

#[test]
fn persistence_property_identity_refuses_at_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let owner = "fcstd:native:object#Body";
    let expected = crate::native::native_child_id("property", owner, "Shape");
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len()) - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(
        matches!(crate::native::native_child_id_charged(&ctx, "property", owner, "Shape"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD native child identity")
    );
    assert_eq!(
        crate::test_support::with_service_context(&[], |ctx| {
            crate::native::native_child_id_charged(ctx, "property", owner, "Shape")
                .expect("admitted ID")
        }),
        expected
    );
}

#[test]
fn persistence_extension_identity_refuses_at_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let owner = "fcstd:native:object#Body";
    let child = "2:Proxy";
    let expected = crate::native::native_child_id("extension", owner, child);
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len()) - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::extension_id(&ctx, owner, "Proxy", 2),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD native child identity"));
    assert_eq!(
        crate::test_support::with_service_context(&[], |ctx| {
            super::extension_id(ctx, owner, "Proxy", 2).expect("admitted ID")
        }),
        expected
    );
}

#[test]
fn persistence_extension_order_refuses_at_scoped_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::extension_id(&ctx, "fcstd:native:object#Body", "Proxy", 2)
        .expect_err("extension order text must be admitted");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && failure.operation == "FCStd extension order text"),
        "{error:?}"
    );
}

fn parse_with_retained_limit(document: &str, limit: u64) -> cadmpeg_core::CodecError {
    let service_arena = cadmpeg_core::decode::DecodeArena::new();
    let service_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (service_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        document.as_bytes(),
        &service_arena,
        &service_policy,
    )
    .expect("service persistence context");
    super::parse_with_context(document.as_bytes(), "4", &service_ctx)
        .expect("service profile admits the persistence fixture");

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)
            .expect("persistence test context");
    super::parse_with_context(document.as_bytes(), "4", &ctx)
        .err()
        .expect("retained copy must be refused")
}

fn assert_retained_operation(error: &cadmpeg_core::CodecError, operation: &str) {
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == operation
    ));
}

fn assert_persistence_diagnostic_refusal(document: &str, expected: &str) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let service = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &service)
            .expect("source bytes are within policy");
    let admitted = super::parse_with_context(document.as_bytes(), "4", &ctx)
        .err()
        .expect("fixture must be malformed");
    assert!(
        matches!(admitted, cadmpeg_core::CodecError::Malformed(_)),
        "{admitted:?}"
    );
    assert!(admitted.to_string().contains(expected), "{admitted:?}");

    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let mut limit = 0;
    for _ in 0..128 {
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            document.as_bytes(),
            &arena,
            &policy,
        )
        .expect("source bytes are within policy");
        let error = super::parse_with_context(document.as_bytes(), "4", &ctx)
            .err()
            .expect("diagnostic must refuse below its retained need");
        let cadmpeg_core::CodecError::ResourceLimit(failure) = error else {
            panic!("diagnostic was reached without a retained refusal: {error:?}");
        };
        assert_eq!(
            failure.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );
        if failure.operation == "FCStd persistence diagnostic" {
            let exact = failure.used + failure.additional - 1;
            policy.limits.max_retained_bytes = exact;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                document.as_bytes(),
                &arena,
                &policy,
            )
            .expect("source bytes are within policy");
            assert_retained_operation(
                &super::parse_with_context(document.as_bytes(), "4", &ctx)
                    .err()
                    .expect("one byte below diagnostic need must refuse"),
                "FCStd persistence diagnostic",
            );
            return;
        }
        let next = failure.used + failure.additional;
        assert!(next > limit, "{failure:?}");
        limit = next;
    }
    panic!("diagnostic admission was not reached: {expected}");
}

#[test]
fn persistence_document_diagnostics_refuse_at_retained_limit() {
    let cases = [
        ("<Document SchemaVersion=\"4\"><ObjectData Count=\"0\"/></Document>",
            "has no Objects section"),
        ("<Document SchemaVersion=\"4\"><Objects/><ObjectData Count=\"0\"/></Document>",
            "Objects Count is missing or invalid"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"2\"><Object name=\"A\"/><Object name=\"A\"/></ObjectData></Document>",
            "duplicate ObjectData name A"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"2\"><Object name=\"A\" type=\"T\"/><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"/></ObjectData></Document>",
            "duplicate object declaration name A"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Extensions Count=\"0\"/><Extensions Count=\"0\"/></Object></ObjectData></Document>",
            "multiple direct Extensions containers"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"0\"/><Properties Count=\"0\"/></Object></ObjectData></Document>",
            "multiple direct Properties containers"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"0\"/><Extensions Count=\"0\"/></Object></ObjectData></Document>",
            "writes Properties before Extensions"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Extensions Count=\"2\"><Extension name=\"E\" type=\"T\"/><Extension name=\"E\" type=\"U\"/></Extensions></Object></ObjectData></Document>",
            "duplicate extension name E"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Extensions Count=\"2\"><Extension name=\"E\" type=\"T\"/><Extension name=\"F\" type=\"T\"/></Extensions></Object></ObjectData></Document>",
            "duplicate extension type T"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"2\"><Property name=\"P\" type=\"T\"/><Property name=\"P\" type=\"T\"/></Properties></Object></ObjectData></Document>",
            "duplicate property name P"),
        ("<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"1\"><Property name=\"L\" type=\"App::PropertyLink\"><Link value=\"A\" Object=\"A\"/></Property></Properties></Object></ObjectData></Document>",
            "unsupported link carrier Object"),
    ];
    for (document, expected) in cases {
        assert_persistence_diagnostic_refusal(document, expected);
    }
}

#[test]
fn persistence_link_diagnostics_refuse_at_retained_limit() {
    let cases = [
        ("<Property name=\"L\" type=\"App::PropertyLink\"/>",
            "App::PropertyLink requires one Link value"),
        ("<Property name=\"L\" type=\"App::PropertyLinkList\"><LinkList count=\"bad\"/></Property>",
            "App::PropertyLinkList count is invalid"),
        ("<Property name=\"L\" type=\"App::PropertyLinkSub\"><LinkSub value=\"A\" count=\"1\"><Sub/></LinkSub></Property>",
            "Sub element has no value attribute"),
        ("<Property name=\"L\" type=\"App::PropertyXLink\"><XLink file=\"\" name=\"A\" Object=\"A\"/></Property>",
            "unsupported link carrier Object"),
    ];
    for (property, expected) in cases {
        let document = format!(
            "<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"1\">{property}</Properties></Object></ObjectData></Document>"
        );
        assert_persistence_diagnostic_refusal(&document, expected);
    }
}

#[test]
fn x62_persistence_dependency_collection_is_admitted_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd object dependency records");
}

#[test]
fn x62_persistence_object_record_push_refuses_at_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd object records");
}

#[test]
fn persistence_count_parse_refuses_at_work_limit() {
    let document = r#"<Document><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    // The count parse reads the one-byte Count value.
    let error = parse_document_work_refusal(document, "FCStd object count parse");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.additional == 1)
    );
}

fn parse_document_work_refusal(document: &str, operation: &str) -> cadmpeg_core::CodecError {
    let xml = roxmltree::Document::parse(document).expect("XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                document.as_bytes(),
                &arena,
                &policy,
            )
            .expect("root");
            super::parse_document(document, &xml, crate::dialect::FcstdDialect::Schema4, &ctx)
                .map(|_| ())
        },
    )
}

fn successful_work_cap<T>(
    input: &[u8],
    mut parse: impl FnMut(
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<T, cadmpeg_core::CodecError>,
) -> u64 {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("work-oracle input is within root limits");
    parse(&ctx).expect("short-prefix Work oracle succeeds under the service policy");
    let error = ctx
        .charge_work(u64::MAX, "short-prefix successful Work oracle")
        .expect_err("the oracle reads the successful parse's Work usage");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("successful Work oracle did not produce a resource refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "short-prefix successful Work oracle");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    limit.used
}

fn work_before_operation<T>(
    input: &[u8],
    operation: &str,
    mut parse: impl FnMut(
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<T, cadmpeg_core::CodecError>,
) -> u64 {
    let mut run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
            .expect("work-oracle input is within root limits");
        let result = parse(&ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    };
    // Framing can move Vec<Node> storage under the same label. Those charges
    // are whole node sizes; only the semantic source visit charges one unit.
    assert!(std::mem::size_of::<roxmltree::Node<'_, '_>>() > 1);
    let error = {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            Some(1),
        );
        run(u64::MAX)
    };
    let error = error.err().expect("named source visit must be reached");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("named Work boundary was not found: {operation}");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    assert!(
        matches!(run(limit.used), Err(cadmpeg_core::CodecError::ResourceLimit(replay))
        if replay.dimension == limit.dimension
            && replay.operation == limit.operation
            && replay.used == limit.used
            && replay.additional == 1)
    );
    limit.used
}

#[test]
fn dependency_lookup_refuses_on_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd dependency lookup");
}

#[test]
fn dependency_item_push_refuses_at_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="1"><Dep Name="B"/></ObjectDeps><ObjectDeps Name="B" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd object dependencies");
}

#[test]
fn dependency_vector_iteration_refuses_at_work_limit() {
    let document = r#"<Document><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object name="A" type="T"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#;
    // One admitted visit for the one dependency record.
    let error = parse_document_work_refusal(document, "FCStd object dependency records");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.additional == 1)
    );
}

#[test]
fn dependency_map_entry_refuses_at_work_limit() {
    let document = r#"<Document><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object name="A" type="T"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#;
    parse_document_work_refusal(document, "FCStd dependency lookup");
}

fn assert_persistence_collection_at_operation(document: &str, operation: &str) {
    crate::test_support::with_service_context(document.as_bytes(), |ctx| {
        super::parse_with_context(document.as_bytes(), "4", ctx)
            .expect("service profile admits the persistence fixture");
    });
    crate::test_support::assert_collection_refusal_at(document.as_bytes(), operation, |ctx| {
        super::parse_with_context(document.as_bytes(), "4", ctx)
    });
}

#[test]
fn extension_type_set_refuses_at_matching_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd extension type set");
}

#[test]
fn extension_owner_refuses_at_matching_retained_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions></Object></ObjectData></Document>"#;
    crate::test_support::assert_retained_refusal_at(
        document.as_bytes(),
        "FCStd extension owner",
        |ctx| super::parse_with_context(document.as_bytes(), "4", ctx),
    );
}

fn assert_link_collection_at_operation(xml: &str, type_name: &str, operation: &str) {
    let document = roxmltree::Document::parse(xml).expect("valid link XML");
    crate::test_support::assert_collection_refusal_at(xml.as_bytes(), operation, |ctx| {
        super::parse_link_targets(document.root_element(), type_name, ctx)
    });
}

#[test]
fn singleton_link_target_refuses_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><Link value=\"A\"/></Property>",
        "App::PropertyLink",
        "FCStd link target records",
    );
}

#[test]
fn link_list_nodes_refuse_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><LinkList count=\"1\"><Link value=\"A\"/></LinkList></Property>",
        "App::PropertyLinkList",
        "FCStd link nodes",
    );
}

#[test]
fn link_list_targets_refuse_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><LinkList count=\"1\"><Link value=\"A\"/></LinkList></Property>",
        "App::PropertyLinkList",
        "FCStd link target or subelement records",
    );
}

#[test]
fn link_sub_subelements_refuse_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><LinkSub value=\"A\" count=\"1\"><Sub value=\"Face1\"/></LinkSub></Property>",
        "App::PropertyLinkSub",
        "FCStd link target or subelement records",
    );
}

#[test]
fn cross_document_link_target_refuses_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><XLink/></Property>",
        "App::PropertyXLink",
        "FCStd link target records",
    );
}

#[test]
fn cross_document_link_list_refuses_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><XLinkSubList count=\"1\"><XLink/></XLinkSubList></Property>",
        "App::PropertyXLinkSubList",
        "FCStd link target or subelement records",
    );
}

#[test]
fn link_sublist_subelement_push_refuses_at_matching_collection_limit() {
    assert_link_collection_at_operation(
        "<Property><LinkSubList count=\"1\"><Link obj=\"A\" sub=\"Face1\"/></LinkSubList></Property>",
        "App::PropertyLinkSubList",
        "FCStd link subelements",
    );
}

#[test]
fn link_carrier_search_charges_each_attribute_step() {
    let property = r#"<Property><Link value="A"/></Property>"#;
    let xml = roxmltree::Document::parse(property).expect("XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "FCStd link carrier search",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                property.as_bytes(),
                &arena,
                &policy,
            )
            .expect("root");
            super::parse_link_targets(xml.root_element(), "App::PropertyLink", &ctx)
        },
    );
    // One step per attribute and one end probe; the literal carrier names add none.
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.additional == 1)
    );
}

#[test]
fn extension_name_set_refuses_on_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd extension name set");
}

#[test]
fn extension_duplicate_name_visit_stops_before_long_suffix() {
    const OPERATION: &str = "FCStd extension nodes";
    let short = b"<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Extensions Count=\"2\"><Extension name=\"E\" type=\"T\"/><Extension name=\"F\" type=\"U\"/></Extensions><Properties Count=\"0\"/></Object></ObjectData></Document>";
    let short_parse = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let graph = super::parse_with_context(short, "4", ctx)?;
        let _duplicate_diagnostic = ctx.format_retained(
            format_args!("duplicate extension name E for fcstd:native:object#A"),
            "FCStd persistence diagnostic",
        )?;
        Ok(graph)
    };
    let short_cap = successful_work_cap(short, short_parse);
    let short_start = work_before_operation(short, OPERATION, short_parse);
    let after_visit_allowance = short_cap
        .checked_sub(short_start)
        .expect("successful short parse reaches the extension visit");

    crate::test_support::with_service_context(short, |ctx| {
        let graph = super::parse_with_context(short, "4", ctx).expect("short extension graph");
        assert_eq!(graph.extensions.len(), 2);
        assert_eq!(graph.extensions[0].name, "E");
        assert_eq!(graph.extensions[0].order, 0);
        assert_eq!(graph.extensions[1].name, "F");
        assert_eq!(graph.extensions[1].order, 1);
    });

    let suffix_extensions = usize::try_from(after_visit_allowance)
        .expect("short-prefix Work allowance fits usize")
        .checked_add(1)
        .expect("extension suffix length fits usize");
    let declared_extensions = suffix_extensions
        .checked_add(2)
        .expect("extension count fits usize");
    assert!(cadmpeg_core::decode::u64_from_index(declared_extensions) > after_visit_allowance);
    let suffix = (0..suffix_extensions)
        .map(|index| format!("<Extension name=\"Suffix{index}\" type=\"T\"/>"))
        .collect::<String>();
    let long = format!(
        "<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Extensions Count=\"{}\"><Extension name=\"E\" type=\"T\"/><Extension name=\"E\" type=\"U\"/>{suffix}</Extensions><Properties Count=\"0\"/></Object></ObjectData></Document>",
        declared_extensions
    );
    let long_start = work_before_operation(long.as_bytes(), OPERATION, |ctx| {
        super::parse_with_context(long.as_bytes(), "4", ctx).map(|_| ())
    });
    let cap = long_start
        .checked_add(after_visit_allowance)
        .expect("long-prefix Work cap fits");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(long.as_bytes(), &arena, &policy)
            .expect("long extension list is within root limits");
    let error = super::parse_with_context(long.as_bytes(), "4", &ctx)
        .err()
        .expect("second extension with a duplicate name is malformed");
    assert!(matches!(
        &error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("duplicate extension name E for fcstd:native:object#A")
    ));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn x62_persistence_extension_collection_is_admitted_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd extension nodes");
}

#[test]
fn x62_persistence_property_collection_is_admitted_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="P" type="T"/></Properties></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd property nodes");
}

#[test]
fn x62_persistence_value_collection_is_admitted_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="App::PropertyString"><String value="x"/></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd property value records");
}

#[test]
fn x62_persistence_extension_record_push_refuses_at_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions></Object></ObjectData></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd extension records");
}

#[test]
fn x62_persistence_property_record_pushes_refuse_at_collection_limit() {
    let transient = r#"<Document SchemaVersion="4"><Properties Count="0" TransientCount="1"><_Property name="P" type="T"/></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    assert_persistence_collection_at_operation(transient, "FCStd transient property records");
    let persisted = r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="T"/></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    assert_persistence_collection_at_operation(persisted, "FCStd persisted property records");
}

#[test]
fn x62_persistence_side_entry_push_refuses_at_collection_limit() {
    let document = r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="App::PropertyFile"><File name="document.txt"/></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    assert_persistence_collection_at_operation(document, "FCStd side entry references");
}

#[test]
fn x63_object_xml_copy_is_charged_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/></Object></ObjectData></Document>"#;
    crate::test_support::assert_retained_refusal_at(
        document.as_bytes(),
        "FCStd object XML",
        |ctx| super::parse_with_context(document.as_bytes(), "4", ctx),
    );
}

#[test]
fn x63_decode_keeps_object_copies_in_scoped_storage() {
    assert_decode_copy_is_scoped("FCStd object XML");
}

#[test]
fn x63_decode_keeps_entry_copies_in_scoped_storage() {
    assert_decode_copy_is_scoped("retain FCStd entry");
}

fn assert_decode_copy_is_scoped(operation: &str) {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{ResourceDimension, View};
    use cadmpeg_ir::codec::CodecBackend;

    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let bytes = archive(document);
    FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("service profile admits the object");

    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = u64::MAX;
    let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &options)
        .expect("typed copies use scoped storage");
    drop(probe);
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let objects: Vec<crate::native::ObjectRecord> = namespace.arena_as("objects").unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name(), "A");
    assert_eq!(
        objects[0].data.as_ref().unwrap().text(),
        r#"<Object name="A"><Properties Count="0"/></Object>"#
    );
    let entries: Vec<crate::native::EntryRecord> = namespace.arena_as("entries").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name(), "Document.xml");
    assert_eq!(entries[0].data(), document.as_bytes());
    let error =
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &bytes, operation, |ctx| {
            FcstdCodec.decode_impl(ctx, View::over_retained(&bytes))
        });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation)
    );
}

#[test]
fn x63_extension_xml_copy_is_charged_after_its_object_copy() {
    let object_data = r#"<Object name="A"><Extensions Count="1"><Extension name="E" type="T"/></Extensions><Properties Count="0"/></Object>"#;
    let document = format!(
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1">{object_data}</ObjectData></Document>"#
    );
    crate::test_support::assert_retained_refusal_at(
        document.as_bytes(),
        "FCStd extension XML",
        |ctx| super::parse_with_context(document.as_bytes(), "4", ctx),
    );
}

#[test]
fn x63_transient_and_persisted_property_xml_copies_are_charged() {
    let transient = r#"<Document SchemaVersion="4"><Properties Count="0" TransientCount="1"><_Property name="P" type="T"/></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    crate::test_support::assert_retained_refusal_at(
        transient.as_bytes(),
        "FCStd transient property XML",
        |ctx| super::parse_with_context(transient.as_bytes(), "4", ctx),
    );
    let persisted = r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="T"/></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    crate::test_support::assert_retained_refusal_at(
        persisted.as_bytes(),
        "FCStd persisted property XML",
        |ctx| super::parse_with_context(persisted.as_bytes(), "4", ctx),
    );
}

#[test]
fn x63_nested_value_xml_charges_the_actual_copied_bytes() {
    let document = r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="App::PropertyString"><String value="x"/></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    crate::test_support::assert_retained_refusal_at(
        document.as_bytes(),
        "FCStd value XML",
        |ctx| super::parse_with_context(document.as_bytes(), "4", ctx),
    );
}

#[test]
fn x63_link_target_attribute_copy_is_charged() {
    let value = r#"<Link value="A"/>"#;
    let document = format!(
        r#"<Document SchemaVersion="4"><Properties Count="1"><Property name="P" type="App::PropertyLink">{value}</Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#
    );
    crate::test_support::assert_retained_refusal_at(
        document.as_bytes(),
        "FCStd link object",
        |ctx| super::parse_with_context(document.as_bytes(), "4", ctx),
    );
}

fn parse_document_graph(document: &str) -> Result<super::Graph, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)?;
    let (_facts, schema_version, document_xml) =
        crate::container::parse_document(&ctx, document.as_bytes()).map_err(
            |error| match error {
                cadmpeg_core::CodecError::WrongFormat(message) => {
                    cadmpeg_core::CodecError::Malformed(message)
                }
                error => error,
            },
        )?;
    super::parse_document(
        document_xml.text,
        document_xml.xml.document(),
        crate::dialect::FcstdDialect::from_schema_version(&schema_version),
        &ctx,
    )
}

#[test]
pub(crate) fn schema_three_uses_the_object_envelope_and_defaults_file_version() {
    let document = r#"<Document SchemaVersion="3">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Legacy"/></Property></Properties>
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Thing"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let bytes = archive(document);
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    assert!(summary.notes.iter().any(|note| note == "FileVersion=0"));
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("schema-three decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].type_name, "App::FeaturePython");
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links()[0].as_ref().expect("link").object(),
        Some(objects[0].id().as_str())
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
pub(crate) fn schema_two_uses_the_feature_envelope_and_common_property_grammar() {
    let document = r#"<Document SchemaVersion="2" ProgramVersion="0.13">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties>
<Features Count="2"><Feature type="App::Feature" name="First"/><Feature type="App::FeaturePython" name="Second"/></Features>
<FeatureData Count="2"><Feature name="First"><Properties Count="0"/></Feature><Feature name="Second"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="First"/></Property></Properties></Feature></FeatureData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("schema-two decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(
        objects
            .iter()
            .map(|object| object.name().as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links()[0].as_ref().expect("link").object(),
        Some(objects[0].id().as_str())
    );
    assert!(objects.iter().all(|object| object.persistent_id.is_none()));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_duplicate_root_property_containers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="First" type="App::PropertyString"><String value="one"/></Property></Properties>
<Properties Count="1"><Property name="Second" type="App::PropertyString"><String value="two"/></Property></Properties>
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("duplicate root Properties containers");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_duplicate_property_names_for_one_owner() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Part" name="Part"/></Objects>
<ObjectData Count="1"><Object name="Part"><Properties Count="2">
 <Property name="Label" type="App::PropertyString"><String value="one"/></Property>
 <Property name="Label" type="App::PropertyString"><String value="two"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("duplicate object property names");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
            if message.contains("duplicate property name Label")
    ));
}

#[test]
fn rejects_nested_xlink_value_children() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target"><XLink file="" name="Nested"/></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" sub="Face1"><Sub value="Face2"/></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" count="1"><Sub value="Face1"><Sub value="Face2"/></Sub></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" count="0"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
pub(crate) fn legacy_schema_dispatch_rejects_wrong_envelopes_and_inconsistent_counts() {
    let cases = [
        r#"<Document SchemaVersion="2"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="3"><Features Count="0"/><FeatureData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="2"><Features Count="1"><Feature type="App::Feature" name="A"/></Features><FeatureData Count="0"><Feature name="A"/></FeatureData></Document>"#,
        r#"<Document SchemaVersion="2"><Features Count="1"><Feature type="App::Feature" name="A"/></Features><FeatureData Count="1"><Feature name="B"/></FeatureData></Document>"#,
    ];
    for document in cases {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default()
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn recovers_objects_dynamic_properties_links_and_side_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Demo"/></Property></Properties>
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="Body" Count="1" AllowPartial="2"><Dep Name="Sketch"/></ObjectDeps>
<ObjectDeps Name="Sketch" Count="0"/>
<Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
<Object type="PartDesign::Feature" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
<Object name="Body" Extensions="True"><Extensions Count="1"><Extension type="Demo::Extension" name="Demo"><Properties Count="1"><Property name="ExtensionValue" type="App::PropertyString"><String value="kept"/></Property></Properties></Extension></Extensions><Properties Count="4" TransientCount="1">
<_Property name="TransientState" type="App::PropertyInteger" status="8"/>
<Property name="Support" type="App::PropertyLinkSub" status="4" group="Attachment" doc="Support object" attr="2" ro="1" hide="0"><LinkSub value="Sketch" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="Members" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value=""/></LinkList></Property>
<Property name="Payload" type="App::PropertyFileIncluded"><File file="Payload.bin"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="" file="Shape.brp"/></Property>
</Properties></Object>
<Object name="Sketch"><Properties Count="0"></Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Payload.bin", b"payload"),
        (
            "Shape.brp",
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 4\n1 10 20 30 1 0 0\n7 0 0 2 3 2 0 0 0 5 0 0 10 0 0 0 3 1 3\n8 0 5 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 5\n1 0 0 0 0 0 1 1 0 0 0 1 0\n9 0 0 0 0 1 1 2 2 2 2 0 0 0 0 1 0 1 0 0 1 1 0 0 2 1 2 0 2 1 2\n6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0.01 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*",
        ),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode graph");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let extensions = namespace
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions");
    assert_eq!(objects.len(), 2);
    assert_eq!(
        objects[0]
            .dependency_allow_partial
            .map(std::num::NonZeroU64::get),
        Some(2)
    );
    assert_eq!(objects[1].dependency_allow_partial, None);
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].owner, "fcstd:native:object#Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:native:object#Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:native:object#Body");
    assert_eq!(
        support.links()[0].as_ref().expect("link").object(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(support.family, crate::native::PropertyFamily::Link);
    assert_eq!(
        support.links()[0].as_ref().expect("link").subelements(),
        vec!["Face1"]
    );
    let crate::native::PropertyBody::Persisted { dynamic, .. } = &support.body else {
        panic!("support property is not persisted");
    };
    assert_eq!(dynamic.as_ref().and_then(|meta| meta.read_only), Some(true));
    let members = properties
        .iter()
        .find(|property| property.name == "Members")
        .expect("members");
    assert_eq!(members.links().len(), 2);
    assert_eq!(
        members.links()[0].as_ref().expect("link").object(),
        Some("fcstd:native:object#Sketch")
    );
    assert!(members.links()[1].is_none());
    let transient = properties
        .iter()
        .find(|property| property.name == "TransientState")
        .expect("transient");
    assert!(transient.is_transient());
    assert_eq!(transient.status, Some(8));
    let payload = properties
        .iter()
        .find(|property| property.name == "Payload")
        .expect("payload");
    assert_eq!(payload.side_entries(), vec!["Payload.bin"]);
    let shape = properties
        .iter()
        .find(|property| property.name == "Shape")
        .expect("shape");
    assert_eq!(shape.family, crate::native::PropertyFamily::Geometry);
    assert_eq!(shape.side_entries(), vec!["Shape.brp"]);
    let shape_payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .expect("shape payloads");
    assert_eq!(shape_payloads.len(), 1);
    assert_eq!(shape_payloads[0].payload.topology_version(), Some(1));
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.curves.len(), 8);
    match &result.ir().model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    match &result.ir().model.curves[1].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            assert_eq!(nurbs.degree(), 2);
            assert_eq!(nurbs.control_points().len(), 3);
            assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert!(nurbs.weights().is_none());
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_curves.len(), 2);
    match result.ir().model.procedural_curves[0].definition() {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(definition_payload) => {
            let parameter_range = definition_payload.parameter_range();
            assert_eq!(parameter_range.endpoints(), [0.0, 5.0]);
        }
        other => panic!("unexpected trimmed construction {other:?}"),
    }
    match result.ir().model.procedural_curves[1].definition() {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset(payload) => {
            let distance = wire::field::<f64>(&payload, "distance");
            let cadmpeg_ir::geometry::OffsetSide::Direction {
                direction,
                support: None,
            } = payload.side()
            else {
                panic!("unexpected offset construction {payload:?}");
            };
            assert_eq!(distance, 2.0);
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 1.0]);
        }
        other => panic!("unexpected offset construction {other:?}"),
    }
    assert_eq!(result.ir().model.surfaces.len(), 7);
    match &result.ir().model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0]);
            assert_eq!([normal.x, normal.y, normal.z], [0.0, 0.0, 1.0]);
            assert_eq!([u_axis.x, u_axis.y, u_axis.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_surfaces.len(), 4);
    assert!(matches!(
        result.ir().model.procedural_surfaces[0].definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(_)
    ));
    assert!(
        match result.ir().model.procedural_surfaces[1].definition() {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(matched_payload) =>
                matches!((&matched_payload.parameter_interval(),), (None,)),
            _ => false,
        }
    );
    assert!(
        match result.ir().model.procedural_surfaces[2].definition() {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(matched_payload) => matches!(
                (matched_payload.u_sense(), matched_payload.v_sense(),),
                (None, None,)
            ),
            _ => false,
        }
    );
    assert!(matches!(
        result.ir().model.procedural_surfaces[3].definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset(_)
    ));
    match &result.ir().model.surfaces[1].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            assert_eq!((nurbs.u_degree(), nurbs.v_degree()), (1, 1));
            assert_eq!((nurbs.u_count(), nurbs.v_count()), (2, 2));
            assert_eq!(nurbs.poles().len(), 4);
            assert_eq!(nurbs.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert!(nurbs.weights().is_none());
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices().len(), 3);
    assert_eq!(result.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
    assert!(result.ir().model.tessellations[0].body.is_none());
    assert!(result.ir().model.tessellations[0].faces.is_empty());
    assert_eq!(
        result.ir().model.tessellations[0]
            .chordal_deflection()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(0.01)
    );
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let payload_entry = entries
        .iter()
        .find(|entry| entry.name() == "Payload.bin")
        .expect("payload entry");
    assert_eq!(payload_entry.referenced_by(), vec![payload.id.clone()]);
    assert_eq!(payload_entry.data(), b"payload");
    let ledger = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    for entry in &entries {
        let mut spans = ledger
            .iter()
            .filter(|span| span.entry == entry.name())
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.span.start());
        assert_eq!(spans.first().map(|span| span.span.start()), Some(0));
        assert_eq!(
            spans.last().map(|span| span.span.end()),
            Some(entry.byte_len())
        );
        assert!(spans
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start()));
    }
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Shape.brp")
        .all(|span| span.classification.as_str() == "typed"));
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Payload.bin")
        .all(|span| span.classification.as_str() == "named_opaque"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification.as_str() == "typed"));
    assert!(ledger.iter().any(|span| {
        span.entry == "Document.xml" && span.classification.as_str() == "structural"
    }));
    let coverage = namespace
        .arena_as::<crate::native::ByteCoverageRecord>("byte_coverage")
        .expect("byte coverage");
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].exact);
    assert_eq!(coverage[0].logical_entry_count, entries.len());
    assert_eq!(
        coverage[0].logical_byte_len,
        entries
            .iter()
            .map(super::super::native::EntryRecord::byte_len)
            .sum::<u64>()
    );
    assert_eq!(
        coverage[0].classification_bytes.values().sum::<u64>(),
        coverage[0].logical_byte_len
    );
    assert!(coverage[0]
        .named_opaque_entries
        .contains(&"Payload.bin".to_owned()));
    let findings = crate::test_support::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:#?}");

    let mut corrupted = result.ir().clone();
    let missing_payload = ledger
        .iter()
        .filter(|span| span.entry != "Payload.bin")
        .cloned()
        .collect::<Vec<_>>();
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "logical_ledger",
            &missing_payload,
        )
        .expect("replace logical ledger");
    assert!(crate::test_support::validate_native(&corrupted)
        .iter()
        .any(|finding| {
            finding
                .message
                .contains("logical ledger omits nonempty entry Payload.bin")
        }));
}

#[test]
fn rejects_inconsistent_object_dependency_envelopes() {
    let cases = [
        r#"<Document SchemaVersion="4"><Objects Count="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="1"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="B" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
    ];

    for document in cases {
        assert!(matches!(
            parse_document_graph(document),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_ambiguous_persistence_carriers() {
    let cases = [
        r#"<Document schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4" schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><ObjectDeps Name="B" Count="0"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="2"><Property name="Same" type="App::PropertyString"/><Property name="Same" type="App::PropertyString"/></Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="0"/><Extensions Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::First" name="Same"/><Extension type="Vendor::Second" name="Same"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::Same" name="First"/><Extension type="Vendor::Same" name="Second"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Properties Count="0"/><Extensions Count="0"/></Object></ObjectData></Document>"#,
    ];

    for document in cases {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default()
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn binds_nested_extension_properties_to_their_enclosing_record() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2">
<Extension type="Vendor::First" name="First"><Properties Count="1"><Property name="FirstValue" type="App::PropertyString"><String value="first"/></Property></Properties></Extension>
<Extension type="Vendor::Second" name="Second"><Properties Count="1"><Property name="SecondValue" type="App::PropertyString"><String value="second"/></Property></Properties></Extension>
</Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let graph = parse_document_graph(document).expect("extension graph");
    let first = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "First")
        .expect("first extension");
    let second = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "Second")
        .expect("second extension");
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "FirstValue")
            .expect("first property")
            .owner,
        first.id
    );
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "SecondValue")
            .expect("second property")
            .owner,
        second.id
    );
}

#[test]
fn native_validation_rejects_duplicate_extension_identity() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="1"><Extension type="Vendor::Extension" name="Extension"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("extension graph");
    let mut corrupted = result.ir().clone();
    let mut extensions = corrupted
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions")
        .clone();
    extensions.push(extensions[0].clone());
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "extensions",
            &extensions,
        )
        .expect("replace extensions");
    let findings = crate::test_support::validate_native(&corrupted);
    assert!(findings.iter().any(|finding| {
        finding.message.contains("duplicate FCStd native identity")
            || finding.message.contains("duplicates extension name")
    }));
}

#[test]
fn unknown_property_runtime_names_do_not_select_a_family_by_substring() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="Custom" type="Vendor::PropertyLinkAndPropertyString"><Link value="A"/></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("unknown property runtime type is retained");
    let property = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .into_iter()
        .find(|property| property.name == "Custom")
        .expect("custom property");
    assert_eq!(property.family, crate::native::PropertyFamily::Unknown);
    assert!(property.links().is_empty());
}

#[test]
fn empty_and_absent_xlink_file_attributes_decode_to_one_typed_value() {
    let link = |markup: &str| {
        let parsed = roxmltree::Document::parse(markup).expect("parse XLink markup");
        crate::test_support::with_service_context(markup.as_bytes(), |ctx| {
            super::xlink(parsed.root_element(), ctx).expect("decode XLink")
        })
    };
    let empty = link(r#"<XLink file="" name="Body"/>"#);
    let absent = link(r#"<XLink name="Body"/>"#);
    assert_eq!(empty, absent);
    let empty = empty.as_ref().expect("link");
    assert_eq!(empty.document(), None);
    assert_eq!(empty.document_attribute(), None);
}

#[test]
fn both_xlink_list_property_types_use_xlink_sub_list_carriers() {
    for type_name in ["App::PropertyXLinkList", "App::PropertyXLinkSubList"] {
        let markup = format!(
            r#"<Property name="References" type="{type_name}"><XLinkSubList count="2"><XLink name="Local"/><XLink file="parts.FCStd" name="Remote" sub="Face1"/></XLinkSubList></Property>"#
        );
        let xml = roxmltree::Document::parse(&markup).unwrap();
        let links = crate::test_support::with_service_context(markup.as_bytes(), |ctx| {
            super::parse_link_targets(xml.root_element(), type_name, ctx).unwrap()
        });
        assert_eq!(links.len(), 2);
        let first = links[0].as_ref().expect("first link");
        let second = links[1].as_ref().expect("second link");
        assert_eq!(first.object(), Some("Local"));
        assert_eq!(first.document_name(), None);
        assert_eq!(second.object(), Some("Remote"));
        assert_eq!(second.document_name(), Some("parts.FCStd"));
        assert_eq!(second.subelements(), ["Face1"]);
        let invalid = markup.replace("XLinkSubList", "XLinkList");
        let xml = roxmltree::Document::parse(&invalid).unwrap();
        assert!(crate::test_support::with_service_context(
            invalid.as_bytes(),
            |ctx| { super::parse_link_targets(xml.root_element(), type_name, ctx).is_err() }
        ));
    }
}

#[test]
fn link_child_framing_rejects_count_and_tag_mismatches() {
    for xml in [
        r#"<Links count="0"><Link/><Link/><Link/></Links>"#,
        r#"<Links count="1000000"/>"#,
        r#"<Links count="1"><Sub/></Links>"#,
    ] {
        let tree = roxmltree::Document::parse(xml).expect("framed XML");
        crate::test_support::with_service_context(xml.as_bytes(), |ctx| {
            let result =
                super::counted_children(tree.root_element(), "Link", "App::PropertyLinkList", ctx);
            assert!(matches!(
                result,
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }
}

#[test]
fn link_child_scan_propagates_work_refusal() {
    let xml = r#"<Links count="0"><Link/></Links>"#;
    let tree = roxmltree::Document::parse(xml).expect("framed XML");
    crate::test_support::with_service_context(xml.as_bytes(), |ctx| {
        let refusal = ctx.refuse_codec_limit("test link work", 0, 1);
        let error =
            super::counted_children(tree.root_element(), "Link", "App::PropertyLinkList", ctx)
                .expect_err("fused context");
        assert_eq!(error.to_string(), refusal.to_string());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    });
}

#[test]
fn empty_link_target_list_returns_no_targets() {
    let markup = "<Property><LinkList count=\"0\"/></Property>";
    let xml = roxmltree::Document::parse(markup).expect("empty LinkList XML");
    crate::test_support::with_service_context(markup.as_bytes(), |ctx| {
        let targets = super::parse_link_targets(xml.root_element(), "App::PropertyLinkList", ctx)
            .expect("empty LinkList has no targets");
        assert!(targets.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_graph_returns_empty_native_populations() {
    let document =
        b"<Document SchemaVersion=\"4\"><Objects Count=\"0\"/><ObjectData Count=\"0\"/></Document>";
    crate::test_support::with_service_context(document, |ctx| {
        let graph = super::parse_with_context(document, "4", ctx).expect("empty graph");
        assert!(graph.objects.is_empty());
        assert!(graph.extensions.is_empty());
        assert!(graph.properties.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn link_target_visits_stop_at_first_nested_value_before_long_suffix() {
    const OPERATION: &str = "FCStd link target nodes";
    let short = b"<Property><LinkList count=\"1\"><Link value=\"Target\"/></LinkList></Property>";
    let short_xml = roxmltree::Document::parse(std::str::from_utf8(short).expect("UTF-8"))
        .expect("valid short link list");
    let short_parse = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::parse_link_targets(short_xml.root_element(), "App::PropertyLinkList", ctx)
            .map(|_| ())
    };
    let short_cap = successful_work_cap(short, short_parse);
    let short_start = work_before_operation(short, OPERATION, short_parse);
    let after_visit_allowance = short_cap
        .checked_sub(short_start)
        .expect("successful short parse reaches the target visit");
    crate::test_support::with_service_context(short, |ctx| {
        let targets =
            super::parse_link_targets(short_xml.root_element(), "App::PropertyLinkList", ctx)
                .expect("short LinkList");
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].as_ref().and_then(|target| target.object()),
            Some("Target")
        );
    });

    let suffix_links = usize::try_from(after_visit_allowance)
        .expect("short-prefix Work allowance fits usize")
        .checked_add(1)
        .expect("link suffix length fits usize");
    let declared_links = suffix_links.checked_add(1).expect("link count fits usize");
    assert!(cadmpeg_core::decode::u64_from_index(declared_links) > after_visit_allowance);
    let suffix = "<Link value=\"Target\"/>".repeat(suffix_links);
    let long = format!(
        "<Property><LinkList count=\"{}\"><Link><Nested/></Link>{suffix}</LinkList></Property>",
        declared_links
    );
    let long_xml = roxmltree::Document::parse(&long).expect("valid long link list");
    let long_start = work_before_operation(long.as_bytes(), OPERATION, |ctx| {
        super::parse_link_targets(long_xml.root_element(), "App::PropertyLinkList", ctx).map(|_| ())
    });
    let cap = long_start
        .checked_add(after_visit_allowance)
        .expect("long-prefix Work cap fits");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(long.as_bytes(), &arena, &policy)
            .expect("long link list is within root limits");
    let error = super::parse_link_targets(long_xml.root_element(), "App::PropertyLinkList", &ctx)
        .err()
        .expect("first nested link value is malformed");
    assert!(matches!(
        &error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("link carrier contains nested element values")
    ));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn object_declaration_framing_precedes_collection_admission() {
    for document in [
        r#"<Document><Objects Count="0"><Object name="A"/><Object name="B"/></Objects><ObjectData Count="0"/></Document>"#,
        r#"<Document><Objects Count="1000000"/><ObjectData Count="0"/></Document>"#,
    ] {
        let xml = roxmltree::Document::parse(document).expect("framed XML");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            document.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root");
        assert!(matches!(
            super::parse_document(document, &xml, crate::dialect::FcstdDialect::Schema4, &ctx),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

fn assert_duplicate_name_work_refusal<T>(
    input: &[u8],
    operation: &str,
    name_bytes: usize,
    parse: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for _ in 0..1024 {
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
            .expect("root");
        let error = parse(&ctx).err().expect("comparison must refuse");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected name comparison refusal: {error:?}");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::WorkUnits
        );
        assert_eq!(Some(limit), ctx.resource_refusal());
        let threshold = limit
            .used
            .checked_add(limit.additional)
            .expect("work threshold");
        assert!(threshold > policy.limits.max_work_units);
        if limit.operation == operation
            && limit.additional == cadmpeg_core::decode::u64_from_index(name_bytes)
        {
            // Each equal operand is admitted separately by its exact UTF-8 byte length.
            policy.limits.max_work_units = threshold - 1;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
                    .expect("root");
            assert!(
                matches!(parse(&ctx), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.operation == operation && refusal.additional == limit.additional
                    && refusal.used + refusal.additional == threshold
                    && Some(*refusal) == ctx.resource_refusal())
            );
            return;
        }
        policy.limits.max_work_units = threshold;
    }
    panic!("{operation} operand admission was not reached");
}

#[test]
fn duplicate_object_name_comparisons_admit_prefix_bytes() {
    let prefix = "a".repeat(10000);
    let document = format!(
        r#"<Document><Objects Count="2"><Object name="{prefix}A" type="Part::Feature"/><Object name="{prefix}B" type="Part::Feature"/></Objects><ObjectData Count="2"><Object name="{prefix}A"/><Object name="{prefix}B"/></ObjectData></Document>"#
    );
    let xml = roxmltree::Document::parse(&document).expect("XML");
    assert_duplicate_name_work_refusal(
        document.as_bytes(),
        "FCStd duplicate object names",
        prefix.len() + 1,
        |ctx| super::parse_document(&document, &xml, crate::dialect::FcstdDialect::Schema4, ctx),
    );
}

#[test]
fn duplicate_property_name_comparisons_admit_lookup_and_prefix_bytes() {
    let prefix = "a".repeat(10000);
    let document = format!(
        r#"<Properties Count="2"><Property name="{prefix}A" type="App::PropertyString"/><Property name="{prefix}B" type="App::PropertyString"/></Properties>"#
    );
    let xml = roxmltree::Document::parse(&document).expect("XML");
    assert_duplicate_name_work_refusal(
        document.as_bytes(),
        "FCStd duplicate property names",
        prefix.len() + 1,
        |ctx| super::parse_properties(&document, xml.root_element(), "owner", &mut Vec::new(), ctx),
    );
}

#[test]
fn object_ceiling_is_resource_refusal() {
    let document = r#"<Document><Objects Count="2"><Object name="A"/><Object name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#;
    let xml = roxmltree::Document::parse(document).expect("XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)
            .expect("root");
    let error = super::parse_document(document, &xml, crate::dialect::FcstdDialect::Schema4, &ctx)
        .err()
        .expect("object ceiling");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "FCStd object count" && limit.limit == 1
            && limit.used + limit.additional == 2 && Some(limit) == ctx.resource_refusal())
    );
}

#[test]
fn retained_property_xml_ceiling_is_resource_refusal() {
    let payload = "a".repeat(6 * 1024 * 1024);
    let document = format!(
        r#"<Properties Count="1"><Property name="P" type="App::PropertyString"><Outer><Middle><Leaf>{payload}</Leaf></Middle></Outer></Property></Properties>"#
    );
    let xml = roxmltree::Document::parse(&document).expect("XML");
    crate::test_support::with_service_context(document.as_bytes(), |ctx| {
        let error =
            super::parse_properties(&document, xml.root_element(), "owner", &mut Vec::new(), ctx)
                .expect_err("cumulative descendant XML ceiling");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "FCStd property retained value XML"
                && limit.limit == 16 * 1024 * 1024 && Some(limit) == ctx.resource_refusal())
        );
    });
}
