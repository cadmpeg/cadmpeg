// SPDX-License-Identifier: Apache-2.0
//! Element-map demand and diagnostic-order regressions.

use super::{in_decode_context, legacy_entry, test_parse, test_property};
use crate::element_map::{parse, parse_legacy_stream, parse_mapped_name};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn malformed_message<T>(result: Result<T, CodecError>) -> String {
    match result {
        Err(CodecError::Malformed(message)) => message,
        Err(error) => panic!("expected malformed input, got {error}"),
        Ok(_) => panic!("expected malformed input"),
    }
}

fn first_visit_work<T>(
    operation: &str,
    read: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> u64 {
    let refusal =
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], operation, read);
    let CodecError::ResourceLimit(limit) = refusal else {
        panic!("expected resource refusal");
    };
    assert_eq!(limit.additional, 1);
    limit.used
}

#[test]
fn element_map_without_consumers_allocates_no_lookup_indexes() {
    let mut document = String::from("<Document><Properties Count=\"64\">");
    let mut properties = Vec::new();
    let mut entries = Vec::new();
    for index in 0..64 {
        let raw = format!(
            "<Property name=\"Text{index}\" type=\"App::PropertyString\"><String value=\"text\"/></Property>"
        );
        let start = cadmpeg_core::decode::u64_from_index(document.len());
        document.push_str(&raw);
        let mut property = test_property("App::PropertyString", &raw);
        property.name = format!("Text{index}");
        property.id = format!("fcstd:test:property#Text{index}");
        property.xml = crate::native::RetainedXml::from_text(raw, start).unwrap();
        properties.push(property);
        entries.push(legacy_entry(&format!("Aux{index}.txt"), b"auxiliary"));
    }
    document.push_str("</Properties></Document>");
    let xml = roxmltree::Document::parse(&document).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // The carrier walks and property visits fit; no lookup buffer is needed.
    policy.limits.max_work_units = 6_000;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (tables, maps) = parse(&ctx, &xml, 1, &properties, &entries).unwrap();
    assert!(tables.as_slice().is_empty());
    assert!(maps.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn inline_string_hasher_does_not_build_an_entry_lookup() {
    let xml = roxmltree::Document::parse(
        "<Document><StringHasher count=\"1\">1.c name</StringHasher></Document>",
    )
    .unwrap();
    let entries = [legacy_entry("Aux.txt", b"auxiliary")];
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_materialized_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(dimension, "FreeCAD element entry lookup", None);
        let (tables, maps) = parse(&ctx, &xml, 1, &[], &entries).unwrap();
        drop(probe);
        assert_eq!(tables.as_slice()[0].entries()[0].payload, "name");
        assert!(maps.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn inline_element_map_does_not_build_property_owners() {
    let property = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><ElementMap count=\"1\"><Element key=\"stable\" value=\"Edge1\"/></ElementMap></Property>",
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "FreeCAD property ownership endpoints",
        None,
    );
    let (_, maps) = super::parse_bytes(
        &ctx,
        b"<Document/>",
        1,
        std::slice::from_ref(&property),
        &[],
    )
    .unwrap();
    drop(probe);
    assert_eq!(maps[0].maps.root().groups[0].names[1][0].encoded, "stable");
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn side_entry_lookup_reuses_its_live_index() {
    let entries = [
        legacy_entry("A.txt", b"first"),
        legacy_entry("B.txt", b"second"),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut lookup = crate::element_map::EntryLookup {
        entries: &entries,
        index: None,
    };
    assert_eq!(lookup.get(&ctx, "A.txt").unwrap(), Some(&b"first"[..]));
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "FreeCAD element entry lookup",
        None,
    );
    assert_eq!(lookup.get(&ctx, "B.txt").unwrap(), Some(&b"second"[..]));
    drop(probe);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn mapped_name_invalid_first_id_does_not_admit_the_unvisited_tail() {
    let encoded = format!(";base.0.g{}", ".1".repeat(256));
    let work = first_visit_work("FreeCAD mapped-name string-id scan", |ctx| {
        parse_mapped_name(ctx, &encoded, &[])
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // One ID visit, one numeric byte, and both diagnostic formatting passes fit.
    // The 256 remaining IDs exceed this allowance and must stay unvisited.
    policy.limits.max_work_units = work + 128;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        malformed_message(parse_mapped_name(&ctx, &encoded, &[])),
        "invalid mapped-name string id \"g\""
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn string_hasher_invalid_first_threshold_does_not_admit_later_hashers() {
    let mut document = String::from("<Document>");
    for index in 0..257 {
        let threshold = if index == 0 { "nope" } else { "0" };
        document.push_str(&format!(
            "<Property type=\"Part::PropertyPartShape\"><Part/><StringHasher threshold=\"{threshold}\" count=\"0\"/></Property>"
        ));
    }
    document.push_str("</Document>");
    let xml = roxmltree::Document::parse(&document).unwrap();
    let work = first_visit_work("FreeCAD string-hasher scan", |ctx| {
        parse(ctx, &xml, 1, &[], &[])
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // Attribute lookup, the invalid number, and its diagnostic fit in 128 units.
    policy.limits.max_work_units = work + 128;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        malformed_message(parse(&ctx, &xml, 1, &[], &[])),
        "invalid threshold \"nope\""
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn element_map_invalid_first_property_does_not_admit_later_properties() {
    let mut document = String::from("<Document><Properties Count=\"8193\">");
    let mut properties = Vec::new();
    for index in 0..8193 {
        let (type_name, raw) = if index == 0 {
            (
                "Part::PropertyPartShape",
                "<Property name=\"Shape\" type=\"Part::PropertyPartShape\"><Part/><ElementMap count=\"nope\"/></Property>".to_owned(),
            )
        } else {
            (
                "App::PropertyString",
                format!("<Property name=\"Text{index}\" type=\"App::PropertyString\"><String/></Property>"),
            )
        };
        let start = cadmpeg_core::decode::u64_from_index(document.len());
        document.push_str(&raw);
        let mut property = test_property(type_name, &raw);
        property.id = format!("fcstd:test:property#Text{index}");
        property.xml = crate::native::RetainedXml::from_text(raw, start).unwrap();
        properties.push(property);
    }
    document.push_str("</Properties></Document>");
    let xml = roxmltree::Document::parse(&document).unwrap();
    let work = first_visit_work("FreeCAD element-map property scan", |ctx| {
        parse(ctx, &xml, 1, &properties, &[])
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // This includes the first property's XML parser admission and framing work.
    policy.limits.max_work_units = work + 4_096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        malformed_message(parse(&ctx, &xml, 1, &properties, &[])),
        "invalid ElementMap count \"nope\""
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn hasher_scratch_is_released_before_shape_property_xml() {
    let raw =
        "<Property type=\"Part::PropertyPartShape\"><Part/><StringHasher count=\"0\"/></Property>";
    let document = format!("<Document>{raw}</Document>");
    let xml = roxmltree::Document::parse(&document).unwrap();
    let mut property = test_property("Part::PropertyPartShape", raw);
    property.xml = crate::native::RetainedXml::from_text(raw.into(), 10).unwrap();
    let refusal = crate::test_support::materialized_refusal_at("FreeCAD XML tree", |ctx| {
        parse(ctx, &xml, 1, std::slice::from_ref(&property), &[])
    });
    let CodecError::ResourceLimit(limit) = refusal else {
        panic!("expected resource refusal");
    };
    assert_eq!(limit.used, 0);
}

#[test]
fn element_map_size_invalid_first_group_does_not_admit_later_groups() {
    let mut bytes = String::from(
        "1 PostfixCount 0 MapCount 1 ElementMap 1 1 257 Edge ChildCount 1 1 0 -1 0 0 stable 0 NameCount 0 ",
    );
    bytes.push_str(&"Edge ChildCount 0 NameCount 0 ".repeat(256));
    bytes.push_str("EndMap");
    let parsed = super::test_parse_element_map(bytes.as_bytes(), false).unwrap();
    let work = first_visit_work("FreeCAD element-map size groups", |ctx| {
        crate::element_map::element_map_size(ctx, &parsed)
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = work + 128;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        malformed_message(crate::element_map::element_map_size(&ctx, &parsed)),
        "invalid element-map child count \"-1\""
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn element_map_size_invalid_first_child_does_not_admit_later_children() {
    let mut bytes = String::from(
        "1 PostfixCount 0 MapCount 1 ElementMap 1 1 1 Edge ChildCount 257 1 0 -1 0 0 stable 0 ",
    );
    bytes.push_str(&"1 0 1 0 0 stable 0 ".repeat(256));
    bytes.push_str("NameCount 0 EndMap");
    let parsed = super::test_parse_element_map(bytes.as_bytes(), false).unwrap();
    let work = first_visit_work("FreeCAD element-map size children", |ctx| {
        crate::element_map::element_map_size(ctx, &parsed)
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = work + 128;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        malformed_message(crate::element_map::element_map_size(&ctx, &parsed)),
        "invalid element-map child count \"-1\""
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn string_hasher_orphan_successor_precedes_later_marker_errors() {
    assert_eq!(
        malformed_message(test_parse(
            b"<Document><StringHasher2/><Wrapper><StringHasher/></Wrapper></Document>",
            1,
            &[],
            &[],
        )),
        "StringHasher2 is not the direct successor of StringHasher"
    );
}

#[test]
fn legacy_stream_indexed_name_error_precedes_later_record_errors() {
    let bytes = b"2 1 bad 0 Edge1 good nope";
    assert_eq!(
        in_decode_context(|ctx| malformed_message(parse_legacy_stream(ctx, bytes, None))),
        "invalid legacy indexed name"
    );
}

#[test]
fn legacy_stream_indexed_name_error_precedes_trailing_data() {
    let bytes = b"1 1 bad 0 trailing";
    assert_eq!(
        in_decode_context(|ctx| malformed_message(parse_legacy_stream(ctx, bytes, None))),
        "invalid legacy indexed name"
    );
}

#[test]
fn legacy_element_indexed_name_error_precedes_later_attributes() {
    let property = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><ElementMap count=\"2\"><Element value=\"1\" key=\"bad\"/><Element/></ElementMap></Property>",
    );
    assert_eq!(
        malformed_message(test_parse(b"<Document/>", 1, &[property], &[])),
        "invalid legacy indexed name"
    );
}

#[test]
fn legacy_element_attribute_error_precedes_final_count() {
    let property = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><ElementMap count=\"2\"><Element/></ElementMap></Property>",
    );
    assert_eq!(
        malformed_message(test_parse(b"<Document/>", 1, &[property], &[])),
        "legacy Element has no value"
    );
}
