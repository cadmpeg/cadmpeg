// SPDX-License-Identifier: Apache-2.0
//! Element-map recovery unit tests.

use super::{node_text_bytes, parse_element_map, parse_legacy_string_ids, parse_string_table};
use crate::native::PropertyRecord;
use crate::test_support::test_archive::{
    archive, archive_entries, assert_valid_document, GEOMETRY,
};
use crate::FcstdCodec;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::{Cursor, Read};

mod failed_string_dependencies;
mod hex_dictionary;

fn test_parse(
    document: &[u8],
    file_version: usize,
    properties: &[crate::test_support::source_fidelity::PropertyFixture],
    entries: &[crate::test_support::source_fidelity::EntryFixture],
) -> Result<
    (
        crate::native::StringTables,
        Vec<crate::native::element_map::ElementMapRecord>,
    ),
    CodecError,
> {
    let sources = properties.iter().collect::<Vec<_>>();
    crate::test_support::source_fidelity::with_decode_sources(
        document,
        &sources,
        &entries.iter().collect::<Vec<_>>(),
        &DecodePolicy::default(),
        |ctx, entry_sources| {
            parse(
                ctx,
                document,
                Some(file_version),
                properties,
                entries,
                entry_sources,
                &mut Vec::new(),
            )
        },
    )
}

fn test_parse_string_table(
    bytes: &[u8],
    count: usize,
    side_entry: bool,
) -> Result<Vec<crate::native::StringTableEntry>, CodecError> {
    parse_string_table(bytes, count, side_entry).map(|(entries, _)| entries)
}

fn test_parse_element_map(bytes: &[u8], side_entry: bool) -> Result<super::ParsedMap, CodecError> {
    parse_element_map(bytes, side_entry)
}

#[test]
fn legacy_string_ids_keep_parse_errors_while_dropping_zero() {
    assert_eq!(
        parse_legacy_string_ids("0,12,-3").expect("valid ids"),
        vec![12, -3]
    );
    assert!(parse_legacy_string_ids("12,9999999999999999999999").is_err());
}

fn test_property(
    type_name: &str,
    raw_xml: &str,
) -> crate::test_support::source_fidelity::PropertyFixture {
    crate::test_support::source_fidelity::PropertyFixture::new(
        PropertyRecord {
            dynamic: None,
            id: "fcstd:test:property#Shape".into(),
            owner: "fcstd:test:object#Shape".into(),
            name: "Shape".into(),
            type_name: type_name.into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Vec::new().into(),
                links: Vec::new(),
                side_entries: Vec::new(),
            }),
            order: 0,
        },
        raw_xml,
    )
}

#[test]
fn restores_absolute_and_relative_string_table_headers() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext};
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
    let input = b"a.c.2 alpha\n-3.c.-1 beta\n";
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(input, &arena, &DecodePolicy::default()).unwrap();
    let split = input.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut records = Vec::new();
    for range in [0..split, split..input.len()] {
        let view = source.child(range.start, range.end).unwrap();
        let header_end = view
            .window()
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap();
        let flag_start = view.window().iter().position(|byte| *byte == b'.').unwrap();
        let flag_end = view.window()[flag_start + 1..]
            .iter()
            .position(|byte| *byte == b'.')
            .map(|at| flag_start + at + 2)
            .unwrap();
        let framing = vec![
            u64_from_index(range.start + flag_start)..u64_from_index(range.start + flag_end),
            u64_from_index(range.start + header_end)..u64_from_index(range.start + header_end + 1),
            u64_from_index(range.end - 1)..u64_from_index(range.end),
        ];
        let record = ctx
            .decode_part(
                &mut output,
                view,
                PartSpec::new(
                    PartLevel::parse("freecad.string_hasher_record").unwrap(),
                    framing,
                ),
                |_stage| super::parts::read_record(view, records.last()),
            )
            .unwrap()
            .unwrap();
        records.push(record);
    }
    assert!(output.losses().is_empty());
    assert_eq!(records[0].string_id, 10);
    assert_eq!(records[0].components, [2]);
    assert_eq!(records[1].string_id, 13);
    assert_eq!(records[1].components, [1]);
    assert_eq!(records[1].payload, "beta");
    assert!(crate::native::StringTableRecord::try_new(
        0,
        None,
        Some(false),
        Some(0),
        None,
        Some(records)
    )
    .is_err());
}

#[test]
fn parses_map_nodes_and_mapped_name_chains() {
    let input = b"7 PostfixCount 1 :tag MapCount 1\n\
            ElementMap 1 7 1 Face ChildCount 0 NameCount 2\n\
            ;Generated.0.a 0 :1.a.0.b 0 EndMap";
    let parsed = test_parse_element_map(input, false).expect("parse element map");
    assert_eq!(parsed.map_id, 7);
    assert_eq!(parsed.postfixes, [Some(":tag".into())]);
    assert_eq!(parsed.maps[0].groups[0].as_ref().unwrap().names.len(), 2);
    assert_eq!(
        parsed.maps[0].groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .string_ids,
        [11]
    );
    assert_eq!(
        parsed.maps[0].groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .resolved
            .as_deref(),
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
    let output = adapters::hasher_parts(xml.input_text().as_bytes()).unwrap();
    assert!(output.losses().is_empty(), "{:?}", output.losses());
    assert_eq!(
        output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arenas()
            .get("string_tables")
            .unwrap()
            .len(),
        2
    );
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
    assert_eq!(
        tables.as_slice()[0].entries().unwrap()[0]
            .as_ref()
            .unwrap()
            .payload,
        "legacy"
    );
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
    assert_hasher_failures(xml.input_text(), &["StringHasher"]);
}

#[test]
fn rejects_duplicate_direct_string_hasher_carriers() {
    let xml = roxmltree::Document::parse(
        r#"<Document><Property name="Shape" type="Part::PropertyPartShape">
<Part/><StringHasher count="0"></StringHasher><StringHasher count="0"></StringHasher>
</Property></Document>"#,
    )
    .expect("duplicate string hashers");
    assert_hasher_failures(xml.input_text(), &["StringHasher", "StringHasher"]);
}

#[test]
fn rejects_orphan_string_hasher2_carrier() {
    let xml = roxmltree::Document::parse("<Document><StringHasher2/></Document>")
        .expect("orphan string hasher successor");
    assert_hasher_failures(xml.input_text(), &["StringHasher2"]);
}

#[test]
fn rejects_non_successor_string_hasher2_carrier() {
    let xml = roxmltree::Document::parse(
        r#"<Document><StringHasher new="1"/><Wrapper/><StringHasher2/></Document>"#,
    )
    .expect("non-successor string hasher");
    assert_hasher_failures(xml.input_text(), &["StringHasher", "StringHasher2"]);
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

fn legacy_entry(name: &str, data: &[u8]) -> crate::test_support::source_fidelity::EntryFixture {
    crate::test_support::entry_fixture(
        crate::native::native_id("entry", name),
        name.into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        data.to_vec(),
    )
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
    assert_eq!(maps[0].declared_count, Some(3));
    assert_eq!(maps[0].source_entry, None);
    assert_eq!(
        maps[0].maps[0].groups[0].as_ref().unwrap().indexed_name,
        "Edge"
    );
    assert_eq!(
        maps[0].maps[0].groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .resolved
            .as_deref(),
        Some("EdgeStable")
    );
    assert_eq!(
        maps[0].maps[0].groups[1].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .resolved
            .as_deref(),
        Some("FaceStable")
    );
}

#[test]
fn unverified_file_version_withholds_only_nonempty_inline_legacy_maps() {
    let inline = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap count="1"><Element key="FaceStable" value="Face1"/></ElementMap></Property>"#,
    );
    let side = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap file="Shape.Map.txt"/></Property>"#,
    );
    let empty = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part ElementMap="1.0"/><ElementMap count="0"/></Property>"#,
    );
    for (property, entries, count, loss_count) in [
        (inline, Vec::new(), 0, 1),
        (
            side,
            vec![legacy_entry("Shape.Map.txt", b"1\nFace1 FaceStable 0\n")],
            1,
            0,
        ),
        (empty, Vec::new(), 0, 0),
    ] {
        crate::test_support::source_fidelity::with_decode_sources(
            b"<Document/>",
            &[&property],
            &entries.iter().collect::<Vec<_>>(),
            &DecodePolicy::default(),
            |ctx, entry_sources| {
                let mut losses = Vec::new();
                let (_, maps) = parse(
                    ctx,
                    b"<Document/>",
                    None,
                    &[property.clone()],
                    &entries,
                    entry_sources,
                    &mut losses,
                )
                .unwrap();
                assert_eq!(maps.len(), count);
                assert_eq!(losses.len(), loss_count);
                if let Some(loss) = losses.first() {
                    assert_eq!(
                        loss.code,
                        cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                            .note("")
                            .code
                    );
                }
                Ok::<(), CodecError>(())
            },
        )
        .unwrap();
    }
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
    assert_eq!(maps[0].declared_count, Some(2));
    let edge = &maps[0].maps[0].groups[0].as_ref().unwrap().names[1]
        .as_ref()
        .unwrap()[0];
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
    assert_eq!(maps[0].declared_count, Some(2));
    assert_eq!(maps[0].source_entry.as_deref(), Some("Shape.Map.txt"));
    assert_eq!(
        maps[0].maps[0].groups[0].as_ref().unwrap().indexed_name,
        "Edge"
    );
    assert_eq!(
        maps[0].maps[0].groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .string_ids,
        [7]
    );
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
    assert_eq!(maps[0].declared_count, Some(3));
    assert_eq!(
        maps[0].maps[0].groups[0].as_ref().unwrap().children.len(),
        1
    );
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
        {
            let xml = roxmltree::Document::parse(direct.source.as_str()).unwrap();
            let marker = xml
                .root_element()
                .children()
                .find(|node| node.has_tag_name("ElementMap"))
                .unwrap();
            super::parse_legacy_element_map(
                marker,
                Some(1),
                &std::collections::HashMap::new(),
                &direct.id,
                &mut Vec::new(),
            )
            .map(|_| ())
        },
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let missing = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part/><ElementMap file="Missing.Map.txt"/></Property>"#,
    );
    // #215: an unavailable side file does not make its readable XML carrier
    // malformed. Its filename survives in the property; no map is invented.
    let (_, maps) = test_parse(br#"<Document FileVersion="1"/>"#, 1, &[missing], &[])
        .expect("readable map filename without an available payload");
    assert!(maps.is_empty());

    let trailing = test_property(
        "Part::PropertyPartShape",
        r#"<Property><Part/><ElementMap count="1">Face1 FaceStable 0 trailing</ElementMap></Property>"#,
    );
    assert!(matches!(
        {
            let xml = roxmltree::Document::parse(trailing.source.as_str()).unwrap();
            let marker = xml
                .root_element()
                .children()
                .find(|node| node.has_tag_name("ElementMap"))
                .unwrap();
            super::parse_legacy_element_map(
                marker,
                Some(2),
                &std::collections::HashMap::new(),
                &trailing.id,
                &mut Vec::new(),
            )
            .map(|_| ())
        },
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn joins_inline_text_and_cdata_sections() {
    let xml = roxmltree::Document::parse("<Table>\n<![CDATA[1.c value\n]]>\n</Table>")
        .expect("inline table XML");
    let bytes = node_text_bytes(xml.root_element());
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
    assert_eq!(
        tables[0].entries().unwrap()[0].as_ref().unwrap().string_id,
        10
    );
    let maps = namespace
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .expect("required invariant");
    assert_eq!(maps.len(), 2);
    let shape_map = maps
        .iter()
        .find(|map| map.property.ends_with("#Shape:Shape"))
        .expect("displayed Shape element map");
    assert_eq!(shape_map.hasher_index, Some(0));
    assert_eq!(
        shape_map.string_table.as_deref(),
        Some("fcstd:native:string-table#0")
    );
    let groups = &shape_map.maps[0].groups;
    let occurrences = crate::test_support::topology_occurrences(result.ir());
    for (group, index) in [(0, 1), (1, 1), (1, 2), (2, 1), (2, 2)] {
        assert_eq!(
            super::topology_ids(
                &shape_map.maps,
                groups[group].as_ref().unwrap(),
                &occurrences,
                &shape_map.property,
                index
            )
            .count(),
            1
        );
    }
    let shape_face_ids = (0..groups[0].as_ref().unwrap().names.len())
        .flat_map(|index| {
            super::topology_ids(
                &shape_map.maps,
                groups[0].as_ref().unwrap(),
                &occurrences,
                &shape_map.property,
                index,
            )
        })
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
fn retains_interleaved_new_string_hasher_payload_source_only() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file=""/>
<StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/>
</Property></Properties></Object></ObjectData></Document>"#;
    let recovered = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("independent shape carrier survives");

    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}

#[test]
fn retains_interleaved_new_string_hasher_payload_source_only_when_parsed_directly() {
    let document = br#"<Document><StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/></Document>"#;
    let (tables, maps) =
        test_parse(document, 0, &[], &[]).expect("naming carrier remains source-only");
    assert!(tables.as_slice().is_empty());
    assert!(maps.is_empty());
}

#[test]
fn rejects_ambiguous_shape_carriers() {
    let duplicate_part = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><Part/></Property>",
    );
    assert!(matches!(
        {
            let xml = roxmltree::Document::parse(duplicate_part.source.as_str()).unwrap();
            super::direct_element_map(xml.root_element()).map(|_| ())
        },
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let duplicate_map = test_property(
        "Part::PropertyPartShape",
        "<Property><Part/><ElementMap2/><ElementMap2/></Property>",
    );
    assert!(matches!(
        {
            let xml = roxmltree::Document::parse(duplicate_map.source.as_str()).unwrap();
            super::direct_element_map(xml.root_element()).map(|_| ())
        },
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
        {
            let xml = roxmltree::Document::parse(nested_map.source.as_str()).unwrap();
            super::direct_element_map(xml.root_element()).map(|_| ())
        },
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
        {
            let xml = roxmltree::Document::parse(non_adjacent_map.source.as_str()).unwrap();
            super::direct_element_map(xml.root_element()).map(|_| ())
        },
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
        {
            let xml = roxmltree::Document::parse(unmarked_map.source.as_str()).unwrap();
            super::direct_element_map(xml.root_element()).map(|_| ())
        },
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
fn string_table_ownership_follows_source_ranges_in_any_property_order() {
    let text = r#"<Document>
<Property name="First" type="Part::PropertyPartShape"><Part/><StringHasher count="0"/></Property>
<Property name="Second" type="Part::PropertyPartShape"><Part/><StringHasher count="0"/></Property>
<StringHasher count="0"/>
</Document>"#;
    let xml = roxmltree::Document::parse(text).unwrap();
    let mut properties = xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .map(|node| {
            let mut property = test_property("Part::PropertyPartShape", &text[node.range()]);
            property.id = format!("fcstd:test:property#{}", node.attribute("name").unwrap());
            property
        })
        .collect::<Vec<_>>();
    properties.reverse();
    let (tables, _) = test_parse(text.as_bytes(), 0, &properties, &[]).unwrap();
    assert_eq!(
        tables
            .as_slice()
            .iter()
            .map(|table| table.owner_property.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("fcstd:test:property#First"),
            Some("fcstd:test:property#Second"),
            None
        ]
    );
}

#[test]
fn ownership_distinguishes_enclosing_ended_and_future_property_ranges() {
    let text = b"<Document><StringHasher count=\"0\"/></Document>";
    let enclosing = test_property("App::PropertyString", "<Property/>");
    let enclosing_source = " ".repeat(1000);
    let mut ended = test_property("App::PropertyString", "<Property/>");
    ended.id = "fcstd:test:property#Ended".into();
    let ended_source = " ".repeat(5);
    let mut future = test_property("App::PropertyString", "<Property/>");
    future.id = "fcstd:test:property#Future".into();
    let future_source = " ".repeat(20);
    let external_ranges = [
        (&future.property, 200, future_source.as_str()),
        (&ended.property, 5, ended_source.as_str()),
        (&enclosing.property, 0, enclosing_source.as_str()),
    ];
    assert!(external_ranges
        .iter()
        .any(|(_, start, source)| *start == 0 && source.len() > text.len()));
    // #214 source ownership comes from the real carrier parent, not external spans.
    let output = adapters::hasher_parts(text).unwrap();
    assert!(output.losses().is_empty());
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].owner_property, None);
}

#[test]
fn rejects_ambiguous_string_table_property_ownership() {
    let xml = b"<Document><StringHasher count=\"0\"/></Document>";
    let first = test_property("App::PropertyString", "<Property/>");
    let first_source = " ".repeat(1000);
    let mut second = test_property("App::PropertyString", "<Property/>");
    second.id = "fcstd:test:property#Other".into();
    let second_source = " ".repeat(1000);
    let external_ranges = [
        (&first.property, 0, first_source.as_str()),
        (&second.property, 0, second_source.as_str()),
    ];
    assert_eq!(external_ranges[0].1, external_ranges[1].1);
    // The two fabricated spans cannot create two actual XML parents (#214).
    let output = adapters::hasher_parts(xml).unwrap();
    assert!(output.losses().is_empty());
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].owner_property, None);
}

#[test]
fn unreadable_child_map_reference_keeps_independent_geometry() {
    let mut source = zip::ZipArchive::new(Cursor::new(GEOMETRY)).expect("geometry archive");
    let mut entries = Vec::new();
    let mut changed = false;
    let original = b"1 0 3 1 0 ;:H,E;:H:5,E 0";
    let replacement = b"1 0 3 1 9 ;:H,E;:H:5,E 0";
    for index in 0..source.len() {
        let mut entry = source.by_index(index).expect("geometry member");
        let name = entry.name().expect("entry name").into_owned();
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
    let recovered = result.expect("independent BRep geometry survives invalid naming map");
    assert_eq!(recovered.ir().model.faces.len(), 48);
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss.message.contains("mapIndex")));
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}

pub(super) mod adapters;
mod metadata_recovery;
mod naming_framing;

fn parse(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    document: &[u8],
    file_version: Option<usize>,
    properties: &[crate::test_support::source_fidelity::PropertyFixture],
    entries: &[crate::test_support::source_fidelity::EntryFixture],
    entry_sources: &crate::entry_sources::EntrySources<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<
    (
        crate::native::StringTables,
        Vec<crate::native::element_map::ElementMapRecord>,
    ),
    CodecError,
> {
    let _ = entry_sources;
    adapters::parse(
        document,
        file_version,
        properties,
        entries,
        ctx.policy(),
        losses,
    )
}

#[test]
fn empty_legacy_element_map_markers_are_typed_absence_not_unread_payloads() {
    for marker in ["<ElementMap/>", "<ElementMap count=\"0\"> \n </ElementMap>"] {
        let xml = format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object name="Shape" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part/>{marker}</Property></Properties></Object></ObjectData></Document>"#
        );
        let source = archive(&xml);
        let result = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        crate::test_support::b1::assert_report(&result);
        assert!(result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            .unwrap()
            .is_empty());
        assert!(result
            .source_fidelity()
            .record_layout()
            .contains_key("fcstd:native:property#Shape:Shape"));
        assert!(result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "freecad.property_value")
            .all(|part| matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
            )));
    }
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object name="Shape" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part/><ElementMap count="bad"/></Property></Properties></Object></ObjectData></Document>"#;
    let source = archive(xml);
    let result = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.source_fidelity().unrepresented(),
        cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design,
        ])
    );
    crate::fidelity::b1::assert_classified_spans(&result);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note("")
            .code));
}

mod string_table_layout;

fn assert_hasher_failures(xml: &str, tags: &[&str]) {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let output = adapters::hasher_parts(xml.as_bytes()).unwrap();
    let mut failed = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    failed.sort_by_key(|part| part.offset());
    let source = roxmltree::Document::parse(xml).unwrap();
    let expected = source
        .descendants()
        .filter(|node| {
            node.is_element() && matches!(node.tag_name().name(), "StringHasher" | "StringHasher2")
        })
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), tags.len());
    assert_eq!(output.losses().len(), tags.len());
    for ((part, expected), tag) in failed.iter().zip(expected).zip(tags) {
        assert_eq!(expected.tag_name().name(), *tag);
        assert_eq!(
            part.offset(),
            u64::try_from(expected.range().start).unwrap()
        );
        assert_eq!(
            part.byte_len(),
            u64::try_from(expected.range().len()).unwrap()
        );
    }
}

#[test]
fn cross_owner_string_table_dependencies_are_resolved_before_map_value_callbacks() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="2"><Objects Count="2"><Object name="A" type="Part::Feature"/><Object name="B" type="Part::Feature"/></Objects><ObjectData Count="2"><Object name="A"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="" HasherIndex="1" ElementMap="1"/><StringHasher count="1">1.c first
</StringHasher><ElementMap new="1"/><ElementMap2>7 PostfixCount 0 MapCount 1 ElementMap 1 7 1 Face ChildCount 0 NameCount 2 0 ;A.0.2 0 EndMap</ElementMap2></Property></Properties></Object><Object name="B"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="" HasherIndex="0" ElementMap="1"/><StringHasher count="1">2.c second
</StringHasher><ElementMap new="1"/><ElementMap2>8 PostfixCount 0 MapCount 1 ElementMap 1 8 1 Face ChildCount 0 NameCount 2 0 ;B.0.1 0 EndMap</ElementMap2></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(&mut Cursor::new(archive(xml)), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir().native.namespace("fcstd").unwrap();
    let tables = namespace
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 2);
    let maps = namespace
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap();
    assert_eq!(maps.len(), 2);
    for (owner, index, string_id) in [("A", 1, 2), ("B", 0, 1)] {
        let property = crate::native::native_child_id(
            "property",
            &crate::native::native_id("object", owner),
            "Shape",
        );
        let map = maps.iter().find(|map| map.property == property).unwrap();
        assert_eq!(map.hasher_index, Some(index));
        assert_eq!(
            map.string_table.as_deref(),
            Some(crate::native::native_id("string-table", index.to_string()).as_str())
        );
        assert_eq!(
            map.maps.root().groups[0].as_ref().unwrap().names[1]
                .as_ref()
                .unwrap()[0]
                .string_ids,
            [string_id]
        );
    }
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}
