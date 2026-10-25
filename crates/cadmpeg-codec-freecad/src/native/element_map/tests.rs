// SPDX-License-Identifier: Apache-2.0

use super::{ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMappedName};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::Codec;

#[test]
fn modern_mapped_names_validate_current_encoded_dictionary_and_derived_fields() {
    let dictionary = (0..11).map(|index| format!("p{index}")).collect::<Vec<_>>();
    let valid = serde_json::json!({
        "id":"fcstd:native:element-map#consistent", "property":"fcstd:native:property#Shape",
        "version":"1", "hasher_index":null, "string_table":null, "source_entry":null,
        "map_id":0, "declared_count":null, "postfixes":dictionary,
        "maps":[{"index":1,"map_id":0,"groups":[{"indexed_name":"Face","children":[],
            "names":[[],[{"encoded":":a.2.0.a","resolved":"p92","string_ids":[10]}]]}]}]
    });
    serde_json::from_value::<super::ElementMapRecord>(valid.clone()).unwrap();
    for field in ["resolved", "string_ids", "encoded"] {
        let mut invalid = valid.clone();
        invalid["maps"][0]["groups"][0]["names"][1][0][field] = match field {
            "resolved" => serde_json::json!("stale"),
            "string_ids" => serde_json::json!([11]),
            _ => serde_json::json!(":a.2.0.b"),
        };
        assert!(
            serde_json::from_value::<super::ElementMapRecord>(invalid)
                .unwrap_err()
                .to_string()
                .contains("derived fields"),
            "{field}"
        );
    }
    let mut edited = valid;
    edited["postfixes"][9] = serde_json::json!("current");
    edited["maps"][0]["groups"][0]["names"][1][0]["resolved"] = serde_json::json!("current2");
    let current: super::ElementMapRecord = serde_json::from_value(edited).unwrap();
    assert_eq!(
        current.maps.root().groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0]
            .resolved
            .as_deref(),
        Some("current2")
    );
}

#[test]
fn child_map_reference_is_rejected_by_complete_cadir_admission() {
    let mut ir = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(crate::test_support::test_archive::GEOMETRY),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .expect("geometry fixture decodes")
        .ir()
        .clone();
    let namespace = ir.native.namespace_mut("fcstd");
    let mut maps = namespace
        .arena_as::<serde_json::Value>("element_maps")
        .expect("element maps");
    let mut changed = false;
    'maps: for map in &mut maps {
        let Some(nodes) = map["maps"].as_array_mut() else {
            continue;
        };
        for node in nodes {
            let Some(groups) = node["groups"].as_array_mut() else {
                continue;
            };
            for group in groups {
                let Some(children) = group["children"].as_array_mut() else {
                    continue;
                };
                for child in children {
                    let Some(descriptor) = child.as_str().map(str::to_owned) else {
                        continue;
                    };
                    let mut fields = descriptor
                        .split_ascii_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if fields.len() < 5 {
                        continue;
                    }
                    fields[4] = "-1".to_owned();
                    *child = serde_json::json!(fields.join(" "));
                    changed = true;
                    break 'maps;
                }
            }
        }
    }
    assert!(
        changed,
        "geometry fixture must contain a child-map descriptor"
    );
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "element_maps",
            &maps,
        )
        .expect("mutated element-map arena");
    let json = serde_json::to_string(&ir).expect("serialize mutated CADIR");
    let reparsed =
        cadmpeg_ir::CadIr::from_json(&json).expect("complete CADIR document remains parseable");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("validation context");
    let findings = crate::FcstdCodec
        .validate_native(&ctx, &reparsed)
        .expect("validation fits service policy");
    assert!(
        findings.iter().any(|finding| {
            finding.message.contains("mapIndex")
                && finding.check == cadmpeg_ir::report::check::Check::NativeLinks
        }),
        "invalid child mapIndex was not reported: {findings:#?}"
    );
}

#[test]
fn element_map_nodes_require_root_on_wire() {
    assert!(serde_json::from_str::<super::ElementMapNodes>("[]")
        .unwrap_err()
        .to_string()
        .contains("maps"));
    let wire = serde_json::json!([{"index": 1, "map_id": 0, "groups": []}]);
    let nodes: super::ElementMapNodes = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(nodes).unwrap(), wire);
}

#[test]
fn element_map_nodes_admit_only_contiguous_one_based_wire_indices() {
    for indices in [vec![0], vec![2], vec![1, 1], vec![1, 3]] {
        let wire = indices
            .into_iter()
            .map(|index| serde_json::json!({"index": index, "map_id": 0, "groups": []}))
            .collect::<Vec<_>>();
        let error = serde_json::from_value::<super::ElementMapNodes>(serde_json::Value::Array(
            wire.clone(),
        ))
        .unwrap_err();
        assert!(error.to_string().contains("maps[") || error.to_string().contains("maps"));

        let record = serde_json::json!({
            "id": "fcstd:native:element-map#test",
            "property": "fcstd:native:property#Shape",
            "version": "1.0",
            "hasher_index": null,
            "string_table": null,
            "source_entry": null,
            "map_id": 0,
            "declared_count": 0,
            "postfixes": [],
            "maps": wire
        });
        assert!(serde_json::from_value::<super::ElementMapRecord>(record.clone()).is_err());

        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(
                &cadmpeg_test_support::service_decode_context(),
                "element_maps",
                &[record],
            )
            .unwrap();
        let roundtrip: cadmpeg_ir::CadIr =
            serde_json::from_value(serde_json::to_value(&ir).unwrap()).unwrap();
        let findings = crate::test_support::validate_native(&roundtrip);
        assert!(findings.iter().any(|finding| {
            finding.message.contains("maps[")
                && finding.check == cadmpeg_ir::report::check::Check::NativeLinks
        }));
    }
}

#[test]
fn element_map_wire_rejects_forward_and_negative_child_map_indices() {
    let child = |descriptor: &str| {
        serde_json::json!([{
            "index": 1,
            "map_id": 0,
            "groups": [{
                "indexed_name": "Face",
                "children": [descriptor],
                "names": []
            }]
        }])
    };
    for descriptor in ["1 0 3 1 1 ;:H,E;:H:5,E 0", "1 0 3 1 -1 ;:H,E;:H:5,E 0"] {
        let error =
            serde_json::from_value::<super::ElementMapNodes>(child(descriptor)).unwrap_err();
        assert!(error.to_string().contains("mapIndex"), "{error}");
    }

    let valid = serde_json::json!([
        {"index": 1, "map_id": 0, "groups": []},
        {"index": 2, "map_id": 1, "groups": [{
            "indexed_name": "Face",
            "children": ["1 0 3 1 1 ;:H,E;:H:5,E 0"],
            "names": []
        }]}
    ]);
    serde_json::from_value::<super::ElementMapNodes>(valid)
        .expect("a child map may name an earlier node");
}

#[test]
fn element_map_child_admission_checks_constructors_and_wire_fields() {
    let node = |descriptor: &str| super::ElementMapNode {
        map_id: 0,
        groups: vec![Some(super::ElementMapGroup {
            indexed_name: "Face".into(),
            children: vec![Some(descriptor.into())],
            names: Vec::new(),
        })],
    };
    for descriptor in [
        "1 0 3 1 1 ;postfix 0",
        "1 0 3 1 -1 ;postfix 0",
        "1 0 3 1 0",
        "1 0 3 1 0 ;postfix 0 extra",
        "-1 0 3 1 0 ;postfix 0",
        "1 -1 3 1 0 ;postfix 0",
        "1 0 not-an-int 1 0 ;postfix 0",
        "1 0 2147483648 1 0 ;postfix 0",
        "1 0 3 not-an-int 0 ;postfix 0",
        // The string-id list starts with `0` and appends decimal ids
        // separated by periods.
        "1 0 3 1 0 ;postfix 1",
        "1 0 3 1 0 ;postfix 1.2",
        "1 0 3 1 0 ;postfix 0.",
        "1 0 3 1 0 ;postfix 0.not-an-int",
        "1 0 3 1 0 ;postfix 0.2.",
    ] {
        assert!(
            super::ElementMapNodes::try_from(vec![node(descriptor)]).is_err(),
            "constructor admitted {descriptor:?}",
        );
        let wire = serde_json::json!([{
            "index": 1,
            "map_id": 0,
            "groups": node(descriptor).groups,
        }]);
        assert!(
            serde_json::from_value::<super::ElementMapNodes>(wire).is_err(),
            "wire reader admitted {descriptor:?}",
        );
    }

    for descriptor in ["0 0 0 -1 0 ;postfix 0", "1 0 3 1 0 ;postfix 0.2.3"] {
        let admitted = super::ElementMapNodes::try_from(vec![node(descriptor)])
            .expect("zero extents and child string-id lists remain legal");
        let wire = serde_json::to_value(&admitted).unwrap();
        assert_eq!(wire[0]["groups"][0]["children"][0], descriptor);
        assert_eq!(
            serde_json::from_value::<super::ElementMapNodes>(wire).unwrap(),
            admitted,
        );
    }
}

#[test]
fn element_map_declared_count_is_independent_xml_metadata() {
    let wire = serde_json::json!({
        "id": "fcstd:native:element-map#test",
        "property": "fcstd:native:property#Shape",
        "version": "1.0",
        "hasher_index": null,
            "string_table": null,
        "source_entry": null,
        "map_id": 0,
        "declared_count": 999,
        "postfixes": [],
        "maps": [{"index": 1, "map_id": 0, "groups": []}]
    });
    let record = serde_json::from_value::<super::ElementMapRecord>(wire.clone()).unwrap();
    assert_eq!(record.declared_count, Some(999));
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
}

#[test]
fn topology_binding_preserves_empty_indexed_name_slots() {
    let mapped_name = || ElementMappedName {
        encoded: ";stable.0".into(),
        resolved: Some("stable".into()),
        string_ids: Vec::new(),
    };
    let group = ElementMapGroup {
        indexed_name: "Edge".into(),
        children: Vec::new(),
        names: vec![
            Some(Vec::new()),
            Some(vec![mapped_name()]),
            Some(Vec::new()),
        ],
    };
    let nodes = ElementMapNodes::try_from(vec![ElementMapNode {
        map_id: 0,
        groups: vec![Some(group)],
    }])
    .expect("valid name group");
    let occurrences = [
        (1, "edge-first-placement"),
        (2, "unmapped-edge"),
        (1, "edge-second-placement"),
    ]
    .map(
        |(source_index, id)| crate::topology_transfer::TopologyOccurrence {
            property: "property".into(),
            indexed_name: "Edge",
            source_index,
            topology_id: id.into(),
        },
    );
    let group = nodes.root().groups[0].as_ref().unwrap();
    assert_eq!(
        occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.property == "property"
                    && occurrence.indexed_name == group.indexed_name
                    && occurrence.source_index == 1
            })
            .map(|occurrence| occurrence.topology_id.as_str())
            .collect::<Vec<_>>(),
        ["edge-first-placement", "edge-second-placement"]
    );
    assert!(group.names[0].as_ref().unwrap().is_empty());
    assert_eq!(
        group.names[1].as_ref().unwrap()[0].resolved.as_deref(),
        Some("stable")
    );
    assert!(group.names[2].as_ref().unwrap().is_empty());
    assert_eq!(
        occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.property == "property"
                    && occurrence.indexed_name == group.indexed_name
                    && occurrence.source_index == 2
            })
            .map(|occurrence| occurrence.topology_id.as_str())
            .collect::<Vec<_>>(),
        ["unmapped-edge"]
    );
    assert!(
        serde_json::to_value(nodes).unwrap()[0]["groups"][0]["names"][1][0]
            .get("topology_ids")
            .is_none()
    );
}

#[test]
fn legacy_rows_preserve_null_positions_and_validate_their_grouped_projection() {
    let rows = vec![
        Some(super::LegacyMapRecord {
            indexed_name: "Face1".into(),
            mapped_name: "first".into(),
            string_ids: vec![1],
        }),
        None,
        Some(super::LegacyMapRecord {
            indexed_name: "Face3".into(),
            mapped_name: "third".into(),
            string_ids: vec![3],
        }),
    ];
    let map = super::ElementMapRecord {
        id: "fcstd:native:element-map#test".into(),
        property: "fcstd:native:property#Shape".into(),
        version: "legacy".into(),
        hasher_index: None,
        string_table: None,
        source_entry: None,
        map_id: 0,
        declared_count: Some(3),
        maps: super::legacy::project(&rows).unwrap(),
        legacy_records: Some(rows),
        postfixes: Vec::new(),
    };
    let wire = serde_json::to_value(&map).unwrap();
    assert_eq!(wire["legacy_records"][1], serde_json::Value::Null);
    assert_eq!(
        wire["maps"][0]["groups"][0]["names"][2],
        serde_json::Value::Null
    );
    assert_eq!(
        wire["maps"][0]["groups"][0]["names"][3][0]["resolved"],
        "third"
    );
    assert_eq!(
        serde_json::from_value::<super::ElementMapRecord>(wire.clone()).unwrap(),
        map
    );
    let mut changed = wire.clone();
    changed["legacy_records"][2]["indexed_name"] = "Face2".into();
    assert!(serde_json::from_value::<super::ElementMapRecord>(changed)
        .unwrap_err()
        .to_string()
        .contains("canonical legacy_records"));
    assert!(wire["maps"][0]["groups"][0]["names"][3][0]
        .get("topology_ids")
        .is_none());
}
