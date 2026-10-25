// SPDX-License-Identifier: Apache-2.0
//! Missing native source positions cannot become authored empty chains.

use super::*;
use crate::native::element_map::{ElementMapGroup, ElementMapNode, ElementMapNodes};

const THREE_FACES: &[u8] = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 3\n1 0 0 0 0 0 1 1 0 0 0 1 0\n1 0 0 1 0 0 1 1 0 0 0 1 0\n1 0 0 2 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 5\nFa 0 0.001 1 0 1001000 *\nFa 0 0.001 2 0 1001000 *\nFa 0 0.001 3 0 1001000 *\nSh 1001000 +5 0 +4 0 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";

fn assert_neutral_map_omission(ir: &CadIr, map: &ElementMapRecord, positions: &[&str]) {
    use cadmpeg_ir::codec::write::{
        target::TargetRequest, EncodeInput, Encoder, NativeDisposition,
    };
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("fcstd", "element_maps", &map.id),
        NativeDisposition::Dropped
    );
    for position in positions {
        assert!(
            plan.report().losses.iter().any(|loss| {
                loss.code.local_code() == "metadata.native-property-not-transferred"
                    && loss.message.contains(&map.id)
                    && loss.message.contains(position)
                    && loss
                        .message
                        .contains("no unique corresponding emitted topology index")
            }),
            "{} {position}: {:?}",
            map.id,
            plan.report().losses
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let face_geometry = |ir: &CadIr| {
        let mut rows = ir
            .model
            .faces
            .iter()
            .map(|face| {
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
                    .unwrap();
                crate::writer::geometry::surface_record(ir, surface).unwrap()
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    assert_eq!(output.ir().model.faces.len(), 3);
    assert_eq!(face_geometry(output.ir()), face_geometry(ir));
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn unavailable_table_payload_is_omitted_without_fabricating_an_empty_table() {
    use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};

    let ctx = cadmpeg_test_support::service_decode_context();
    let table = StringTableRecord::try_new(
        0,
        None,
        Some(false),
        Some(0),
        Some("missing-table.txt".into()),
        None,
    )
    .unwrap();
    let id = table.id();
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "string_tables", &[table])
        .unwrap();
    let mut losses = Vec::new();
    let mut entries = BTreeMap::new();
    let mut outcomes =
        NativeDispositions::for_namespace("fcstd", ["string_tables"], NativeDisposition::Written);
    let dictionary = write(
        &ir,
        &mut [],
        &HashMap::new(),
        &[],
        false,
        &mut entries,
        &mut losses,
        &mut outcomes,
    )
    .unwrap();
    assert!(dictionary.is_empty());
    assert!(entries.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "source.undecoded-omitted");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert!(losses[0].message.contains(&id));
    assert_eq!(
        outcomes.record_disposition("fcstd", "string_tables", &id),
        NativeDisposition::Dropped
    );
}

#[test]
fn sparse_naming_write_warns_and_omits_group_without_relabeling_its_names() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let property = PropertyRecord {
        dynamic: None,
        id: "fcstd:native:property#Shape".into(),
        owner: "fcstd:native:object#Shape".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: Some(crate::native::PropertyBody::Persisted {
            values: Vec::new().into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
        order: 0,
    };
    let map = ElementMapRecord {
        id: "fcstd:native:element-map#Shape".into(),
        property: property.id.clone(),
        version: "1".into(),
        hasher_index: None,
        string_table: None,
        source_entry: None,
        map_id: 7,
        declared_count: None,
        legacy_records: None,
        postfixes: Vec::new(),
        maps: ElementMapNodes::try_from(vec![ElementMapNode {
            map_id: 7,
            groups: vec![
                Some(ElementMapGroup {
                    indexed_name: "Face".into(),
                    children: Vec::new(),
                    names: vec![Some(Vec::new()), None],
                }),
                Some(ElementMapGroup {
                    indexed_name: "Edge".into(),
                    children: Vec::new(),
                    names: vec![Some(Vec::new())],
                }),
            ],
        }])
        .unwrap(),
    };
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "element_maps", &[map])
        .unwrap();
    let mut object = Object::new("Shape".into(), "Part::Feature", "Shape");
    object.set(
        "Shape",
        "<Property name=\"Shape\" type=\"Part::PropertyPartShape\"><Part file=\"\"/></Property>"
            .into(),
    );
    let mut objects = [object];
    let mut losses = Vec::new();
    write(
        &ir,
        &mut objects,
        &HashMap::from([(property.owner.clone(), "Shape".into())]),
        &[property],
        false,
        &mut BTreeMap::new(),
        &mut losses,
        &mut super::super::super::dispositions::initial(),
    )
    .unwrap();
    let xml = &objects[0].properties["Shape"];
    assert!(!xml.contains("Face"));
    let parsed = roxmltree::Document::parse(xml).unwrap();
    let content = parsed
        .descendants()
        .find(|node| node.has_tag_name("ElementMap2"))
        .unwrap()
        .text()
        .unwrap();
    assert!(content.contains("ElementMap 1 7 1\nEdge ChildCount 0  NameCount 1\n0\nEndMap"));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "source.undecoded-omitted");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert!(losses[0].message.contains("node[1].group[0].name_chain[1]"));
}

#[test]
fn rewritten_table_identity_resolves_to_the_emitted_hasher_ordinal() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let property = PropertyRecord {
        dynamic: None,
        id: "fcstd:native:property#Shape".into(),
        owner: "fcstd:native:object#Shape".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: None,
        order: 0,
    };
    let mut table =
        StringTableRecord::try_new(17, None, Some(false), Some(0), None, Some(Vec::new())).unwrap();
    table.id = "fcstd:native:string-table#rewritten".into();
    let map = ElementMapRecord {
        id: "fcstd:native:element-map#Shape".into(),
        property: property.id.clone(),
        version: "1".into(),
        hasher_index: Some(17),
        string_table: Some(table.id()),
        source_entry: None,
        map_id: 7,
        declared_count: None,
        legacy_records: None,
        postfixes: Vec::new(),
        maps: ElementMapNodes::try_from(vec![ElementMapNode {
            map_id: 7,
            groups: Vec::new(),
        }])
        .unwrap(),
    };
    let mut ir = CadIr::empty();
    let native = ir.native.namespace_mut("fcstd");
    native.set_arena(&ctx, "string_tables", &[table]).unwrap();
    native.set_arena(&ctx, "element_maps", &[map]).unwrap();
    let mut object = Object::new("Shape".into(), "Part::Feature", "Shape");
    object.set(
        "Shape",
        "<Property name=\"Shape\" type=\"Part::PropertyPartShape\"><Part file=\"\"/></Property>"
            .into(),
    );
    let mut objects = [object];
    let mut losses = Vec::new();
    let mut entries = BTreeMap::new();
    let dictionary = write(
        &ir,
        &mut objects,
        &HashMap::from([(property.owner.clone(), "Shape".into())]),
        &[property],
        false,
        &mut entries,
        &mut losses,
        &mut super::super::super::dispositions::initial(),
    )
    .unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    assert!(dictionary.contains("Cadmpeg.StringHasher.0.txt"));
    assert_eq!(
        entries["Cadmpeg.StringHasher.0.txt"],
        b"StringTableStart v1 0\n"
    );
    assert!(objects[0].properties["Shape"].contains("HasherIndex=\"0\""));
    assert!(!objects[0].properties["Shape"].contains("HasherIndex=\"17\""));
}

#[test]
fn source_less_legacy_write_keeps_explicit_labels_around_missing_record() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    let document = b"<Document SchemaVersion='4' FileVersion='2'><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp' ElementMap='legacy'/><ElementMap count='3'>Face1 first 2 0 -3\nFace2 \xffbroken 0\nFace3 third 0\n</ElementMap></Property></Properties></Object></ObjectData></Document>";
    let brep = THREE_FACES;
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.faces.len(), 3);
    let maps = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ElementMapRecord>("element_maps")
        .unwrap();
    let rows = maps[0].legacy_records.as_ref().unwrap();
    assert_eq!(maps[0].declared_count, Some(3));
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].as_ref().unwrap().indexed_name, "Face1");
    assert_eq!(rows[0].as_ref().unwrap().mapped_name, "first");
    assert_eq!(rows[0].as_ref().unwrap().string_ids, [0, -3]);
    assert!(rows[1].is_none());
    assert_eq!(rows[2].as_ref().unwrap().indexed_name, "Face3");
    assert_neutral_map_omission(
        decoded.ir(),
        &maps[0],
        &["legacy_records[0] Face1", "legacy_records[2] Face3"],
    );
    let native_only = crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(!native_only.ir().scope.model);
    assert_eq!(
        native_only.ir().model,
        cadmpeg_ir::document::Model::default()
    );
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(native_only.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("fcstd", "element_maps", &maps[0].id),
        cadmpeg_ir::codec::write::NativeDisposition::Written
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"
            && loss.message.contains("legacy_records[1]")));
    assert!(
        !plan.report().losses.iter().any(|loss| loss
            .message
            .contains("no unique corresponding emitted topology index")),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut payload = String::new();
    use std::io::Read;
    archive
        .by_name("Cadmpeg.LegacyMap.0.txt")
        .unwrap()
        .read_to_string(&mut payload)
        .unwrap();
    assert_eq!(payload, "2\nFace1 first 2 0 -3\nFace3 third 0\n");
    drop(archive);
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), 3);
    let maps = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ElementMapRecord>("element_maps")
        .unwrap();
    let rows = maps[0].legacy_records.as_ref().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].as_ref().unwrap().indexed_name, "Face1");
    assert_eq!(rows[1].as_ref().unwrap().indexed_name, "Face3");
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn unknown_hasher_policy_does_not_become_default_metadata_on_write() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let table = StringTableRecord::try_new(0, None, None, None, None, Some(Vec::new())).unwrap();
    let id = table.id();
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "string_tables", &[table])
        .unwrap();
    let mut entries = BTreeMap::new();
    let mut losses = Vec::new();
    let mut outcomes = super::super::super::dispositions::initial();
    assert!(write(
        &ir,
        &mut [],
        &HashMap::new(),
        &[],
        false,
        &mut entries,
        &mut losses,
        &mut outcomes
    )
    .unwrap()
    .is_empty());
    assert!(entries.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "source.undecoded-omitted");
    assert!(losses[0].message.contains("save_all") && losses[0].message.contains("threshold"));
    assert_eq!(
        outcomes.record_disposition("fcstd", "string_tables", &id),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
}

#[test]
fn map_relocation_accounts_for_each_actual_neutral_association_and_omission() {
    use cadmpeg_ir::codec::write::NativeDisposition;
    let original: ElementMapRecord = serde_json::from_value(serde_json::json!({
        "id":"fcstd:native:element-map#Shape", "property":"fcstd:native:property#Shape",
        "version":"1", "hasher_index":null, "string_table":null, "source_entry":null,
        "map_id":7, "declared_count":null, "postfixes":[],
        "maps":[{"index":1,"map_id":7,"groups":[{"indexed_name":"Face","children":[],
            "names":[[],[{"encoded":";first.0","resolved":"first","string_ids":[]}],
                [{"encoded":";second.0","resolved":"second","string_ids":[]}]]}]}]
    }))
    .unwrap();
    for (mappings, expected, omissions) in [
        (
            vec![("Face", 1, 2), ("Face", 2, 1)],
            NativeDisposition::Regenerated,
            0,
        ),
        (vec![("Face", 1, 2)], NativeDisposition::Regenerated, 1),
        (vec![], NativeDisposition::Dropped, 2),
    ] {
        let mut losses = Vec::new();
        let (map, disposition) = relocate::apply(
            &original,
            mappings.into_iter(),
            &mut losses,
            &mut [7].into_iter().collect(),
        )
        .unwrap();
        assert_eq!(disposition, expected);
        assert_eq!(losses.len(), omissions);
        assert!(losses.iter().all(|loss| loss.message.contains(&original.id)
            && loss.message.contains("root.groups[0].names[")));
        if expected == NativeDisposition::Regenerated {
            assert_eq!(
                map.maps.root().groups[0].as_ref().unwrap().names[2]
                    .as_ref()
                    .unwrap()[0]
                    .resolved
                    .as_deref(),
                Some("first")
            );
        }
    }
    let mut empty = original;
    empty.maps = ElementMapNodes::try_from(vec![ElementMapNode {
        map_id: 7,
        groups: vec![Some(ElementMapGroup {
            indexed_name: "Face".into(),
            children: Vec::new(),
            names: vec![Some(Vec::new())],
        })],
    }])
    .unwrap();
    let (emitted, disposition) = relocate::apply(
        &empty,
        std::iter::empty(),
        &mut Vec::new(),
        &mut [7].into_iter().collect(),
    )
    .unwrap();
    assert_eq!(emitted, empty);
    assert_eq!(disposition, NativeDisposition::Written);
}

#[test]
fn source_less_sparse_modern_names_keep_healthy_labels_and_unchanged_sid_prefixes() {
    use cadmpeg_ir::codec::write::{
        target::TargetRequest, EncodeInput, Encoder, NativeDisposition,
    };
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    let document = b"<Document SchemaVersion='4' FileVersion='2'><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp' HasherIndex='0' ElementMap='1'/><StringHasher count='3'>1.c firs\n2.c middle\n3.c thir\n</StringHasher><ElementMap new='1'/><ElementMap2>7 PostfixCount 2 t d MapCount 1 ElementMap 1 7 1 Face ChildCount 0 NameCount 4 0 ;firs.1.1 0 ;\xff.0.2 0 ;thir.2.3 0 EndMap</ElementMap2></Property></Properties></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Shape'><Properties Count='1'><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='DiffuseColor'/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=''/></Document>";
    let mut colors = 3_u32.to_le_bytes().to_vec();
    for color in [0xff00_00ff_u32, 0x00ff_00ff, 0x0000_ffff] {
        colors.extend_from_slice(&color.to_le_bytes());
    }
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", THREE_FACES),
        ("GuiDocument.xml", gui),
        ("DiffuseColor", &colors),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Face(_)
            ))
            .count(),
        2
    );
    let original = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ElementMapRecord>("element_maps")
        .unwrap();
    let group = original[0].maps.root().groups[0].as_ref().unwrap();
    assert_eq!(
        group.names[1].as_ref().unwrap()[0].resolved.as_deref(),
        Some("first")
    );
    assert!(group.names[2].is_none());
    assert_eq!(
        group.names[3].as_ref().unwrap()[0].resolved.as_deref(),
        Some("third")
    );
    assert_neutral_map_omission(
        decoded.ir(),
        &original[0],
        &["root.groups[0].names[1]", "root.groups[0].names[3]"],
    );
    let native_only = crate::FcstdCodec
        .decode(
            &mut Cursor::new(source),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(!native_only.ir().scope.model);
    assert_eq!(
        native_only.ir().model,
        cadmpeg_ir::document::Model::default()
    );
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(native_only.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "fcstd",
            "element_maps",
            &original[0].id
        ),
        NativeDisposition::Written
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"
            && loss.message.contains("name_chain[2]")));
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("omits topology association")),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(written.ir().model.faces.len(), 3);
    let maps = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ElementMapRecord>("element_maps")
        .unwrap();
    let map = &maps[0];
    assert_eq!(map.declared_count, original[0].declared_count);
    let root = map.maps.root().groups[0].as_ref().unwrap();
    assert!(root.names.is_empty());
    assert_eq!(root.children.len(), 2);
    for (output_index, expected, prefix, sid) in [(1, "first", "firs", 1), (3, "third", "thir", 3)]
    {
        let child = root
            .children
            .iter()
            .flatten()
            .find(|child| {
                child.split_ascii_whitespace().nth(1).unwrap() == output_index.to_string()
            })
            .unwrap();
        let fields = child.split_ascii_whitespace().collect::<Vec<_>>();
        assert_eq!(fields[0], "0");
        assert_eq!(fields[2], "1");
        let target = fields[4].parse::<usize>().unwrap();
        let node = map
            .maps
            .indexed()
            .find(|(index, _)| *index == target)
            .unwrap()
            .1;
        let chain = node.groups[0].as_ref().unwrap().names[0].as_ref().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].string_ids, [sid]);
        assert_eq!(chain[0].resolved.as_deref(), Some(prefix));
        assert_eq!(chain[0].encoded, format!(";{prefix}.0.{sid}"));
        assert_eq!(
            format!("{}{}", chain[0].resolved.as_deref().unwrap(), fields[5]),
            expected
        );
    }
    let occurrences = crate::test_support::topology_occurrences(written.ir());
    assert_eq!(crate::element_map::topology_index_count(root), 3);
    assert_eq!(
        crate::element_map::topology_indices(&map.maps, root, &occurrences, &map.property)
            .collect::<std::collections::BTreeSet<_>>(),
        [1, 3].into_iter().collect()
    );
    for index in [1, 3] {
        let ids =
            crate::element_map::topology_ids(&map.maps, root, &occurrences, &map.property, index)
                .collect::<Vec<_>>();
        assert_eq!(ids.len(), 1);
        assert!(written.ir().model.appearance_bindings.iter().any(|binding| matches!(&binding.target, cadmpeg_ir::appearance::AppearanceTarget::Face(face) if face.as_str() == ids[0])));
    }
    assert_eq!(
        written
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Face(_)
            ))
            .count(),
        2
    );
    assert!(crate::test_support::validate_native(written.ir()).is_empty());
}

#[test]
fn sparse_name_postfix_lifting_never_splits_sid_backed_base_tokens() {
    use cadmpeg_ir::codec::write::NativeDisposition;
    let original: ElementMapRecord = serde_json::from_value(serde_json::json!({
        "id":"fcstd:native:element-map#Sparse", "property":"fcstd:native:property#Shape",
        "version":"1", "hasher_index":null, "string_table":null, "source_entry":null,
        "map_id":7, "declared_count":null, "postfixes":["d"],
        "maps":[{"index":1,"map_id":7,"groups":[{"indexed_name":"Face","children":[],
            "names":[[],[{"encoded":";#1.0.1","resolved":"#1","string_ids":[1]}],null,
                [{"encoded":";thir.1.3","resolved":"third","string_ids":[3]}]]}]}]
    }))
    .unwrap();
    let mut losses = Vec::new();
    let (map, disposition) = relocate::apply(
        &original,
        [("Face", 1, 1), ("Face", 3, 3)].into_iter(),
        &mut losses,
        &mut [0, 1, 7].into_iter().collect(),
    )
    .unwrap();
    assert_eq!(disposition, NativeDisposition::Regenerated);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("root.groups[0].names[1]"));
    assert!(losses[0]
        .message
        .contains("unchanged nonempty prefix/SID tokens"));
    assert_eq!(map.postfixes, original.postfixes);
    // IDs0/1 belong to independent current maps in the same archive restore.
    assert_eq!(map.maps.indexed().next().unwrap().1.map_id, 2);
    let children = &map.maps.root().groups[0].as_ref().unwrap().children;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].as_deref(), Some("0 3 1 0 1 d 0"));
    let prefix = map.maps.indexed().next().unwrap().1.groups[0]
        .as_ref()
        .unwrap()
        .names[0]
        .as_ref()
        .unwrap();
    assert_eq!(prefix[0].encoded, ";thir.0.3");
    assert_eq!(prefix[0].string_ids, [3]);
    assert_eq!(
        original.maps.root().groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .encoded,
        ";#1.0.1"
    );
}
