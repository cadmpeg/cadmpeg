// SPDX-License-Identifier: Apache-2.0

use super::{ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMappedName};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::Codec;

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
        .validate_native(&ctx, &reparsed, None)
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
        groups: vec![super::ElementMapGroup {
            indexed_name: "Face".into(),
            children: vec![descriptor.into()],
            names: Vec::new(),
        }],
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
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let mapped_name = || ElementMappedName {
        encoded: ";stable.0".into(),
        resolved: Some("stable".into()),
        string_ids: Vec::new(),
    };
    let group = ElementMapGroup {
        indexed_name: "Edge".into(),
        children: Vec::new(),
        names: vec![Vec::new(), vec![mapped_name()], Vec::new()],
    };
    let nodes = ElementMapNodes::try_from(vec![ElementMapNode {
        map_id: 0,
        groups: vec![group],
    }])
    .expect("valid name group");
    let native_before = serde_json::to_value(&nodes).unwrap();
    // CADIR v8 #213 removes bind_root_topology's native backlink mutation.
    // Retain its exact placement labels as typed key segments in real current neutral
    // identities, while preserving the original native name and empty slots.
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 4
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +4 0 -3 0 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +4 0 -3 0 *
+2 0 +2 0 +1 0 *";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    let labels = [
        "edge-first-placement",
        "edge-second-placement",
        "unmapped-edge",
    ];
    assert_eq!(ir.model.edges.len(), labels.len());
    let rename = ir
        .model
        .edges
        .iter()
        .zip(labels)
        .map(|(edge, label)| {
            (
                edge.id.as_str().to_owned(),
                format!("test:model:edge#{label}"),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut identities =
        IdentityMap::new(|id: &str| Ok(rename.get(id).cloned().unwrap_or_else(|| id.to_owned())));
    ir.model.edges = std::mem::take(&mut ir.model.edges)
        .rewrite_identities(&ctx, &mut identities)
        .unwrap();
    ir.model.shells = std::mem::take(&mut ir.model.shells)
        .rewrite_identities(&ctx, &mut identities)
        .unwrap();
    ir.model.coedges = std::mem::take(&mut ir.model.coedges)
        .rewrite_identities(&ctx, &mut identities)
        .unwrap();
    identities.finish().unwrap();
    let payloads = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let inverse = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
    let group = &nodes.root().groups[0];
    let (linked, placements) =
        inverse.selected_records(&payloads[0].property, &group.indexed_name, 1);
    assert!(linked);
    let placements = placements.collect::<Vec<_>>();
    assert_eq!(
        placements,
        [
            "test:model:edge#edge-first-placement",
            "test:model:edge#edge-second-placement"
        ]
    );
    assert!(ir.model.edges.iter().all(|edge| edge.native_ref.is_some()));
    assert!(group.names[0].is_empty());
    assert!(group.names[2].is_empty());
    let (linked, unmapped) =
        inverse.selected_records(&payloads[0].property, &group.indexed_name, 2);
    assert!(linked);
    assert_eq!(
        unmapped.collect::<Vec<_>>(),
        ["test:model:edge#unmapped-edge"]
    );
    assert_eq!(group.names[1][0].encoded, ";stable.0");
    assert_eq!(group.names[1][0].resolved.as_deref(), Some("stable"));
    assert_eq!(serde_json::to_value(&nodes).unwrap(), native_before);
}

#[test]
fn native_mapped_names_reject_the_removed_neutral_backlink_field() {
    // CADIR v8 #213: a native mapped name cannot retain a second link home.
    let native = ElementMappedName {
        encoded: ";stable.0".to_owned(),
        resolved: Some("stable".to_owned()),
        string_ids: Vec::new(),
    };
    let mut wire = serde_json::to_value(&native).unwrap();
    assert_eq!(wire["encoded"], ";stable.0");
    assert_eq!(wire["resolved"], "stable");
    wire["topology_ids"] = serde_json::json!(["fcstd:model:edge#source"]);
    assert!(serde_json::from_value::<ElementMappedName>(wire).is_err());
}
