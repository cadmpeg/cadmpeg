// SPDX-License-Identifier: Apache-2.0
//! Element-map recovery unit tests.

use super::{
    node_text_bytes, owning_property, parse, parse_element_map, parse_legacy_string_ids,
    parse_mapped_name, parse_string_table, validate_string_hasher_framing,
};
use crate::native::{EntryRecord, PropertyRecord};
use crate::test_support::test_archive::{
    archive, archive_entries, assert_valid_document, GEOMETRY,
};
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::{Cursor, Read};

fn in_decode_context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within the input limit");
    f(&ctx)
}

fn with_collection_limit<T>(
    bytes: &[u8],
    limit: u64,
    f: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("input is within the root limit");
    f(&ctx)
}

#[test]
fn element_map_diagnostic_refuses_at_matching_retained_limit() {
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD element map diagnostic", |ctx| {
        Err::<(), _>(super::element_map_malformed(
            ctx,
            format_args!(
                "expected element-map token {:?}, found {:?}",
                "MapCount", "Other"
            ),
        ))
    });
}

#[test]
fn legacy_side_entry_name_refuses_at_matching_retained_limit() {
    let xml = roxmltree::Document::parse("<ElementMap file=\"Shape.Map.txt\"/>")
        .expect("valid legacy carrier");
    crate::test_support::assert_retained_refusal_at(
        &[],
        "FreeCAD legacy element map side-entry name",
        |ctx| {
            super::parse_legacy_element_map(
                ctx,
                xml.root_element(),
                1,
                &std::collections::HashMap::default(),
            )
        },
    );
}

#[test]
fn string_table_capacity_refuses_on_collection_limit() {
    let bytes = b"1.c name\n";
    let result = with_collection_limit(bytes, 0, |ctx| parse_string_table(ctx, bytes, 1, false));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD string table entries"
    ));
}

#[test]
fn string_table_component_refuses_on_collection_limit() {
    let bytes = b"1.c.2 name\n";
    let result = with_collection_limit(bytes, 1, |ctx| parse_string_table(ctx, bytes, 1, false));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD string table components"));
}

#[test]
fn string_table_value_word_refuses_on_collection_limit() {
    let bytes = b"1.8 prefix postfix\n";
    let result = with_collection_limit(bytes, 1, |ctx| parse_string_table(ctx, bytes, 1, false));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD string table value words"));
}

#[test]
fn string_table_record_refuses_on_collection_limit() {
    let document = b"<Document><StringHasher saveall=\"0\" threshold=\"0\" count=\"0\" new=\"1\"/><StringHasher2 count=\"0\"/></Document>";
    let result = with_collection_limit(document, 0, |ctx| parse(ctx, document, 1, &[], &[]));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD string table records"));
}

#[test]
fn element_map_group_capacity_refuses_on_collection_limit() {
    let bytes = b"1 PostfixCount 0 MapCount 1 ElementMap 1 1 1";
    let result = with_collection_limit(bytes, 1, |ctx| parse_element_map(ctx, bytes, false));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD element map groups"
    ));
}

#[test]
fn mapped_name_chain_refuses_on_collection_limit() {
    let bytes = b"7 PostfixCount 0 MapCount 1 ElementMap 1 7 1 Face ChildCount 0 NameCount 1 ;Generated.0.a 0 EndMap";
    let result = with_collection_limit(bytes, 3, |ctx| parse_element_map(ctx, bytes, false));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD mapped name chain"));
}

#[test]
fn element_map_postfixes_refuse_on_collection_limit() {
    let bytes = b"7 PostfixCount 1 :tag MapCount 0";
    let result = with_collection_limit(bytes, 0, |ctx| parse_element_map(ctx, bytes, false));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD element map postfixes"));
}

#[test]
fn mapped_name_fields_refuse_on_collection_limit() {
    let result = with_collection_limit(&[], 0, |ctx| parse_mapped_name(ctx, ";Generated.0.a", &[]));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD mapped name fields"));
}

#[test]
fn legacy_string_ids_refuse_on_collection_limit() {
    let result = with_collection_limit(&[], 0, |ctx| parse_legacy_string_ids(ctx, "12"));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD legacy string IDs"));
}

#[test]
fn inline_element_text_refuses_on_materialized_limit() {
    let xml = roxmltree::Document::parse("<Table>abc</Table>").expect("valid inline XML");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within input policy");
    assert!(
        matches!(node_text_bytes(&ctx, xml.root_element()), Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD inline element text")
    );
}

fn test_parse(
    document: &[u8],
    file_version: usize,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<
    (
        crate::native::StringTables,
        Vec<crate::native::element_map::ElementMapRecord>,
    ),
    CodecError,
> {
    in_decode_context(|ctx| parse(ctx, document, file_version, properties, entries))
}

fn test_parse_string_table(
    bytes: &[u8],
    count: usize,
    side_entry: bool,
) -> Result<Vec<crate::native::StringTableEntry>, CodecError> {
    in_decode_context(|ctx| parse_string_table(ctx, bytes, count, side_entry))
}

fn test_parse_element_map(bytes: &[u8], side_entry: bool) -> Result<super::ParsedMap, CodecError> {
    in_decode_context(|ctx| parse_element_map(ctx, bytes, side_entry))
}

#[test]
fn legacy_string_ids_keep_parse_errors_while_dropping_zero() {
    assert_eq!(
        in_decode_context(|ctx| parse_legacy_string_ids(ctx, "0,12,-3")).expect("valid ids"),
        vec![12, -3]
    );
    assert!(
        in_decode_context(|ctx| parse_legacy_string_ids(ctx, "12,9999999999999999999999")).is_err()
    );
}

fn test_property(type_name: &str, raw_xml: &str) -> PropertyRecord {
    PropertyRecord {
        id: "fcstd:test:property#Shape".into(),
        owner: "fcstd:test:object#Shape".into(),
        name: "Shape".into(),
        type_name: type_name.into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(raw_xml.into(), 0).unwrap(),
    }
}

#[test]
fn restores_absolute_and_relative_string_table_headers() {
    let records = test_parse_string_table(b"a.c.2 alpha\n-3.c.-1 beta\n", 2, false)
        .expect("parse relative string table");
    assert_eq!(records[0].string_id, 10);
    assert_eq!(records[0].components, [2]);
    assert_eq!(records[1].string_id, 13);
    assert_eq!(records[1].components, [1]);
    assert_eq!(records[1].payload, "beta");
}

#[test]
fn parses_map_nodes_and_mapped_name_chains() {
    let input = b"7 PostfixCount 1 :tag MapCount 1\n\
            ElementMap 1 7 1 Face ChildCount 0 NameCount 2\n\
            ;Generated.0.a 0 :1.a.0.b 0 EndMap";
    let parsed = test_parse_element_map(input, false).expect("parse element map");
    assert_eq!(parsed.map_id, 7);
    assert_eq!(parsed.postfixes, [":tag"]);
    assert_eq!(parsed.maps[0].groups[0].names.len(), 2);
    assert_eq!(parsed.maps[0].groups[0].names[1][0].string_ids, [11]);
    assert_eq!(
        parsed.maps[0].groups[0].names[1][0].resolved.as_deref(),
        Some(":tag10")
    );
}

#[test]
fn rejects_declared_string_table_count_mismatch() {
    assert!(test_parse_string_table(b"1.c name\n", 2, false).is_err());
}

#[test]
fn accepts_document_and_direct_shape_string_hasher_roots() {
    let xml = roxmltree::Document::parse(
        r#"<Document>
<StringHasher saveall="0" threshold="0" count="0" new="1"/>
<StringHasher2 count="0"></StringHasher2>
<Property name="Shape" type="Part::PropertyPartShape">
<Part/>
<StringHasher saveall="0" threshold="0" count="0" new="1"/>
<StringHasher2 count="0"></StringHasher2>
</Property>
</Document>"#,
    )
    .expect("framed string hashers");
    validate_string_hasher_framing(xml.root_element()).expect("valid roots");
}

#[test]
fn accepts_legacy_document_string_hasher_carrier() {
    let (tables, maps) = test_parse(
        br#"<Document><StringHasher count="1">a.c legacy</StringHasher></Document>"#,
        0,
        &[],
        &[],
    )
    .expect("legacy string table carrier");
    assert_eq!(tables.as_slice().len(), 1);
    assert_eq!(tables.as_slice()[0].entries()[0].payload, "legacy");
    assert!(maps.is_empty());
}

#[test]
fn rejects_nested_string_hasher_carrier() {
    let xml = roxmltree::Document::parse(
        r#"<Document><Property name="Shape" type="Part::PropertyPartShape">
<Part/><Wrapper><StringHasher count="0"></StringHasher></Wrapper>
</Property></Document>"#,
    )
    .expect("nested string hasher");
    assert!(validate_string_hasher_framing(xml.root_element()).is_err());
}

#[test]
fn rejects_duplicate_direct_string_hasher_carriers() {
    let xml = roxmltree::Document::parse(
        r#"<Document><Property name="Shape" type="Part::PropertyPartShape">
<Part/><StringHasher count="0"></StringHasher><StringHasher count="0"></StringHasher>
</Property></Document>"#,
    )
    .expect("duplicate string hashers");
    assert!(validate_string_hasher_framing(xml.root_element()).is_err());
}

#[test]
fn rejects_orphan_string_hasher2_carrier() {
    let xml = roxmltree::Document::parse("<Document><StringHasher2/></Document>")
        .expect("orphan string hasher successor");
    assert!(validate_string_hasher_framing(xml.root_element()).is_err());
}

#[test]
fn rejects_non_successor_string_hasher2_carrier() {
    let xml = roxmltree::Document::parse(
        r#"<Document><StringHasher new="1"/><Wrapper/><StringHasher2/></Document>"#,
    )
    .expect("non-successor string hasher");
    assert!(validate_string_hasher_framing(xml.root_element()).is_err());
}

#[test]
fn restores_multiline_length_prefixed_string() {
    let records = test_parse_string_table(b"1.0 1:first\nsecond\n", 1, false)
        .expect("parse multiline string table");
    assert_eq!(records[0].payload, "first\nsecond");
}

#[test]
fn parses_side_entry_headers() {
    let table = test_parse_string_table(b"StringTableStart v1 1\n1.c value\n", 1, true)
        .expect("parse absolute string table");
    assert_eq!(table[0].payload, "value");
    let map = test_parse_element_map(
        b"BeginElementMap v1 1 PostfixCount 0 MapCount 1 ElementMap 1 1 0 EndMap",
        true,
    )
    .expect("parse absolute element map");
    assert_eq!(map.map_id, 1);
}

fn legacy_entry(name: &str, data: &[u8]) -> EntryRecord {
    EntryRecord {
        id: name.into(),
        name: name.into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data: data.to_vec(),
    }
}

#[test]
fn admits_legacy_direct_element_carrier() {
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap count="3">
<Element key="FaceStable" value="Face1"/>
<Element key="EdgeStable" value="Edge1"/>
<Element key="VertexStable" value="Vertex1"/>
</ElementMap></Property>"#,
    );
    let (_, maps) = test_parse(br#"<Document FileVersion="1"/>"#, 1, &[property], &[])
        .expect("legacy direct element map");
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].declared_count, 3);
    assert_eq!(maps[0].source_entry, None);
    assert_eq!(maps[0].maps[0].groups[0].indexed_name, "Edge");
    assert_eq!(
        maps[0].maps[0].groups[0].names[1][0].resolved.as_deref(),
        Some("EdgeStable")
    );
    assert_eq!(
        maps[0].maps[0].groups[1].names[1][0].resolved.as_deref(),
        Some("FaceStable")
    );
}

#[test]
fn element_map_identity_refuses_at_retained_limit() {
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap count="1"><Element key="FaceStable" value="Face1"/></ElementMap></Property>"#,
    );
    crate::test_support::assert_retained_refusal_at(
        b"<Document/>",
        "FreeCAD native child identity",
        |ctx| parse(ctx, b"<Document/>", 1, std::slice::from_ref(&property), &[]),
    );
}

#[test]
fn admits_legacy_inline_stream_carrier() {
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap count="2">
Face1 FaceStable 0
Edge1 EdgeStable 1 7
</ElementMap></Property>"#,
    );
    let (_, maps) = test_parse(br#"<Document FileVersion="2"/>"#, 2, &[property], &[])
        .expect("legacy inline element map");
    assert_eq!(maps[0].declared_count, 2);
    let edge = &maps[0].maps[0].groups[0].names[1][0];
    assert_eq!(edge.resolved.as_deref(), Some("EdgeStable"));
    assert_eq!(edge.string_ids, [7]);
}

#[test]
fn admits_legacy_side_entry_record_stream() {
    let data = b"2\nFace1 FaceStable 0\nEdge1 EdgeStable 1 7\n";
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap file="Shape.Map.txt"/></Property>"#,
    );
    let (_, maps) = test_parse(
        br#"<Document FileVersion="1"/>"#,
        1,
        &[property],
        &[legacy_entry("Shape.Map.txt", data)],
    )
    .expect("legacy side-entry record stream");
    assert_eq!(maps[0].declared_count, 2);
    assert_eq!(maps[0].source_entry.as_deref(), Some("Shape.Map.txt"));
    assert_eq!(maps[0].maps[0].groups[0].indexed_name, "Edge");
    assert_eq!(maps[0].maps[0].groups[0].names[1][0].string_ids, [7]);
}

#[test]
fn admits_legacy_side_entry_v1_map_stream() {
    let data = b"BeginElementMap v1\n\
1 PostfixCount 0\n\
MapCount 1\n\
ElementMap 1 1 1\n\
Face\n\
ChildCount 1\n\
1 0 3 0 0 0 0\n\
NameCount 0\n\
EndMap\n";
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap file="Shape.Map.txt"/></Property>"#,
    );
    let (_, maps) = test_parse(
        br#"<Document FileVersion="1"/>"#,
        1,
        &[property],
        &[legacy_entry("Shape.Map.txt", data)],
    )
    .expect("legacy v1 side-entry map");
    assert_eq!(maps[0].declared_count, 3);
    assert_eq!(maps[0].maps[0].groups[0].children.len(), 1);
}

#[test]
fn rejects_child_map_index_that_is_not_a_prior_node() {
    let data = b"BeginElementMap v1\n\
1 PostfixCount 0\n\
MapCount 1\n\
ElementMap 1 1 1\n\
Face\n\
ChildCount 1\n\
1 0 3 0 1 0 0\n\
NameCount 0\n\
EndMap\n";
    let Err(error) = test_parse_element_map(data, true) else {
        panic!("forward child map index")
    };
    assert!(error.to_string().contains("mapIndex"), "{error}");
}

#[test]
fn treats_legacy_empty_marker_as_no_map() {
    let property = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap/></Property>"#,
    );
    let (_, maps) = test_parse(br#"<Document FileVersion="1"/>"#, 1, &[property], &[])
        .expect("legacy empty element map");
    assert!(maps.is_empty());
}

#[test]
fn rejects_legacy_element_count_and_side_entry_errors() {
    let direct = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part/><ElementMap count="2"><Element key="Face" value="Face1"/></ElementMap></Property>"#,
    );
    assert!(matches!(
        test_parse(br#"<Document FileVersion="1"/>"#, 1, &[direct], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let missing = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part/><ElementMap file="Missing.Map.txt"/></Property>"#,
    );
    assert!(matches!(
        test_parse(br#"<Document FileVersion="1"/>"#, 1, &[missing], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let trailing = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part/><ElementMap count="1">Face1 FaceStable 0 trailing</ElementMap></Property>"#,
    );
    assert!(matches!(
        test_parse(br#"<Document FileVersion="2"/>"#, 2, &[trailing], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn joins_inline_text_and_cdata_sections() {
    let xml = roxmltree::Document::parse("<Table>\n<![CDATA[1.c value\n]]>\n</Table>")
        .expect("inline table XML");
    let bytes =
        in_decode_context(|ctx| node_text_bytes(ctx, xml.root_element()).map(|(bytes, _)| bytes))
            .expect("inline text allocation");
    let table = test_parse_string_table(&bytes, 1, false).expect("inline string table");
    assert_eq!(table[0].payload, "value");
}

#[test]
fn connects_persistent_element_names_to_neutral_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1" StringHasher="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="2">
<Property name="AuxShape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="AuxShape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property>
<Property name="Shape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="Shape.brp"/>
<StringHasher saveall="0" threshold="16" count="0" new="1"/>
<StringHasher2 count="1">
a.c PersistentSource
</StringHasher2>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="4">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
<Property name="LineColorArray" type="App::PropertyColorList"><ColorList file="LineColorArray"/></Property>
<Property name="PointColorArray" type="App::PropertyColorList"><ColorList file="PointColorArray"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let face_colors = [1_u8, 0, 0, 0, 0, 0, 0, 255];
    let edge_colors = [2_u8, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255];
    let point_colors = [2_u8, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 255];
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("DiffuseColor", &face_colors),
        ("LineColorArray", &edge_colors),
        ("PointColorArray", &point_colors),
        ("AuxShape.brp", brep),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("persistent element map");
    let namespace = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("required invariant");
    let tables = namespace
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .expect("required invariant");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].entries()[0].string_id, 10);
    let maps = namespace
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .expect("required invariant");
    assert_eq!(maps.len(), 2);
    let shape_map = maps
        .iter()
        .find(|map| map.property.ends_with("#Shape:Shape"))
        .expect("displayed Shape element map");
    assert_eq!(shape_map.hasher_index, Some(0));
    let groups = &shape_map.maps[0].groups;
    assert_eq!(groups[0].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[2][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[2][0].topology_ids.len(), 1);
    let shape_face_ids = groups[0]
        .names
        .iter()
        .flatten()
        .flat_map(|name| &name.topology_ids)
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) if shape_face_ids.contains(face.as_str())
        ) && binding.channels.get("precedence").map(String::as_str) == Some("face_over_object")
    }));
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("edge_array_over_line")
            })
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("vertex_array_over_point")
            })
            .count(),
        2
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_interleaved_new_string_hasher_payload() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file=""/>
<StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/>
</Property></Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("interleaved string table must fail");

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_interleaved_new_string_hasher_payload_when_parsed_directly() {
    let document = br#"<Document><StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/></Document>"#;
    let error = test_parse(document, 0, &[], &[]).expect_err("interleaved string table must fail");

    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_ambiguous_shape_carriers() {
    let duplicate_part = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><Part/></Property>",
    );
    assert!(matches!(
        test_parse(b"<Document/>", 0, &[duplicate_part], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let duplicate_map = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><ElementMap2/><ElementMap2/></Property>",
    );
    assert!(matches!(
        test_parse(b"<Document/>", 0, &[duplicate_map], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_nested_element_map_successor() {
    let nested_map = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap><Wrapper><ElementMap2/></Wrapper></Property>"#,
    );
    assert!(matches!(
        test_parse(b"<Document/>", 0, &[nested_map], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_non_adjacent_element_map_successor() {
    let non_adjacent_map = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap><Wrapper/><ElementMap2/></Property>"#,
    );
    assert!(matches!(
        test_parse(b"<Document/>", 0, &[non_adjacent_map], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_element_map_without_compatibility_marker() {
    let unmarked_map = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap2/></Property>"#,
    );
    assert!(matches!(
        test_parse(b"<Document/>", 0, &[unmarked_map], &[]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn ignores_non_shape_runtime_names() {
    let custom = test_property(
        "Custom::PropertyPartShape",
        "<Property><Part/><ElementMap2/></Property>",
    );
    let (tables, maps) = test_parse(b"<Document/>", 0, &[custom], &[]).expect("unknown type");

    assert!(tables.as_slice().is_empty());
    assert!(maps.is_empty());
}

#[test]
fn rejects_ambiguous_string_table_property_ownership() {
    let xml = roxmltree::Document::parse("<Document><StringHasher count=\"0\"/></Document>")
        .expect("test XML");
    let node = xml.root_element().first_element_child().expect("hasher");
    let mut first = test_property("App::PropertyString", "<Property/>");
    first.xml = crate::native::RetainedXml::from_text(" ".repeat(1000), 0).unwrap();
    let mut second = test_property("App::PropertyString", "<Property/>");
    second.id = "fcstd:test:property#Other".into();
    second.xml = crate::native::RetainedXml::from_text(" ".repeat(1000), 0).unwrap();

    assert!(matches!(
        in_decode_context(|ctx| owning_property(ctx, node, &[first, second])),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn child_map_reference_is_rejected_by_complete_source_admission() {
    let mut source = zip::ZipArchive::new(Cursor::new(GEOMETRY)).expect("geometry archive");
    let mut entries = Vec::new();
    let mut changed = false;
    let original = b"1 0 3 1 0 ;:H,E;:H:5,E 0";
    let replacement = b"1 0 3 1 9 ;:H,E;:H:5,E 0";
    for index in 0..source.len() {
        let mut entry = source.by_index(index).expect("geometry member");
        let name = entry.name().to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read geometry member");
        if !changed {
            if let Some(offset) = data
                .windows(original.len())
                .position(|window| window == original)
            {
                data[offset..offset + replacement.len()].copy_from_slice(replacement);
                changed = true;
            }
        }
        entries.push((name, data));
    }
    assert!(
        changed,
        "geometry fixture must contain a child-map descriptor with a mapped child"
    );
    let references = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect::<Vec<_>>();
    let result = FcstdCodec.decode(
        &mut Cursor::new(archive_entries(&references)),
        &DecodeOptions::default(),
    );
    let error = result.expect_err("current source route rejects the mutated child-map index");
    assert!(error.to_string().contains("mapIndex"), "{error}");
}
