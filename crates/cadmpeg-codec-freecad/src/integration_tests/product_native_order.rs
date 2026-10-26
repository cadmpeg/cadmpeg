// SPDX-License-Identifier: Apache-2.0
//! Native declaration order carries authored root and sibling ranks.

use crate::native::PropertyRecord;
use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::products::{AssemblyGraph, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::{Cursor, Read};

#[test]
fn known_pose_roots_and_siblings_use_authored_native_declaration_order() {
    let placement = |x| {
        format!(
            r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="{x}" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>"#
        )
    };
    let linked = r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property>"#;
    let group = r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Early"/><Link value="Late"/></LinkList></Property>"#;
    // All local and ancestor poses are known. Definition ID ordering emits
    // Other before Parent without the helper; the authored Parent0/Other1
    // ranks require a root swap. Canonical occurrence IDs stay sorted as
    // Early,Late,Other,Parent; authored Late0/Early1 requires a sibling swap.
    // Both requested native orders disagree with the writer construction order.
    let records = [
        (
            "Parent",
            "App::Part",
            2,
            format!("{group}{}", placement(10)),
        ),
        ("Prototype", "Part::Feature", 1, placement(0)),
        ("Late", "App::Link", 3, format!("{linked}{}", placement(2))),
        ("Other", "App::Part", 1, placement(20)),
        ("Early", "App::Link", 3, format!("{linked}{}", placement(3))),
    ];
    let declarations = records
        .iter()
        .map(|(name, kind, _, _)| format!(r#"<Object name="{name}" type="{kind}"/>"#))
        .collect::<String>();
    let data = records
        .iter()
        .map(|(name, _, count, props)| {
            format!(
                r#"<Object name="{name}"><Properties Count="{count}">{props}</Properties></Object>"#
            )
        })
        .collect::<String>();
    let source = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="5">{declarations}</Objects><ObjectData Count="5">{data}</ObjectData></Document>"#
    );
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    let original = serde_json::to_value(decoded.ir()).unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.occurrences.len(), 4);
    for (name, rank) in [("Parent", 0), ("Other", 1), ("Late", 0), ("Early", 1)] {
        let native = crate::native::native_id("object", name);
        ir.model
            .occurrences
            .iter_mut()
            .find(|row| row.native_ref.as_deref() == Some(native.as_str()))
            .unwrap()
            .ordinal = rank;
    }
    // Identity arenas stay in their required canonical ID order. Ordinals
    // express authored physical order independently of that arena order.
    assert_eq!(
        ir.model
            .occurrences
            .iter()
            .map(|row| row.native_ref.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "fcstd:native:object#Early",
            "fcstd:native:object#Late",
            "fcstd:native:object#Other",
            "fcstd:native:object#Parent"
        ]
    );
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|row| row.transform.is_some()));
    let graph = AssemblyGraph::new(&ir.model.occurrences).unwrap();
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|row| graph.effective_transform(&row.id).is_some()));
    assert_valid_document(&ir);
    let supplied = serde_json::to_value(&ir).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_eq!(serde_json::to_value(&ir).unwrap(), supplied);
    assert_eq!(serde_json::to_value(decoded.ir()).unwrap(), original);
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let declaration_order = |section| {
        doc.root_element()
            .children()
            .find(|node| node.has_tag_name(section))
            .unwrap()
            .children()
            .filter(|node| node.has_tag_name("Object"))
            .map(|node| node.attribute("name").unwrap())
            .collect::<Vec<_>>()
    };
    let objects = declaration_order("Objects");
    let data = declaration_order("ObjectData");
    assert_eq!(
        objects, data,
        "both actual native declaration sections agree"
    );
    assert_eq!(objects.len(), 5);
    assert_eq!(
        objects
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        ["Parent", "Other", "Prototype", "Early", "Late"]
            .into_iter()
            .collect()
    );
    for order in [&objects, &data] {
        assert_eq!(
            order
                .iter()
                .copied()
                .filter(|name| ["Parent", "Other"].contains(name))
                .collect::<Vec<_>>(),
            ["Parent", "Other"]
        );
        assert_eq!(
            order
                .iter()
                .copied()
                .filter(|name| ["Late", "Early"].contains(name))
                .collect::<Vec<_>>(),
            ["Late", "Early"]
        );
    }
    let parent = doc
        .root_element()
        .children()
        .find(|node| node.has_tag_name("ObjectData"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("Object") && node.attribute("name") == Some("Parent"))
        .unwrap();
    let group = parent
        .descendants()
        .find(|node| node.has_tag_name("Property") && node.attribute("name") == Some("Group"))
        .unwrap();
    assert_eq!(
        group
            .descendants()
            .filter(|node| node.has_tag_name("Link"))
            .map(|node| node.attribute("value").unwrap())
            .collect::<Vec<_>>(),
        ["Late", "Early"]
    );
    let output = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_valid_document(output.ir());
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
    assert_eq!(
        output.ir().model.occurrences.len(),
        ir.model.occurrences.len()
    );
    for expected in &ir.model.occurrences {
        let actual = output
            .ir()
            .model
            .occurrences
            .iter()
            .find(|row| row.native_ref == expected.native_ref)
            .unwrap();
        assert_eq!(actual.ordinal, expected.ordinal);
        assert_eq!(actual.transform, expected.transform);
        assert_eq!(actual.linked_prototype, expected.linked_prototype);
        match (&expected.parent, &actual.parent) {
            (OccurrenceParent::Root {}, OccurrenceParent::Root {}) => {}
            (
                OccurrenceParent::Occurrence { occurrence: before },
                OccurrenceParent::Occurrence { occurrence: after },
            ) => {
                let before = ir
                    .model
                    .occurrences
                    .iter()
                    .find(|row| row.id == *before)
                    .unwrap();
                let after = output
                    .ir()
                    .model
                    .occurrences
                    .iter()
                    .find(|row| row.id == *after)
                    .unwrap();
                assert_eq!(after.native_ref, before.native_ref);
            }
            _ => panic!("held parent relationship changed"),
        }
        let (
            PrototypeReference::Local { definition: before },
            PrototypeReference::Local { definition: after },
        ) = (&expected.prototype, &actual.prototype)
        else {
            panic!("local prototype reference changed")
        };
        assert_eq!(
            output
                .ir()
                .model
                .product_definitions
                .iter()
                .find(|row| row.id == *after)
                .unwrap()
                .native_ref,
            ir.model
                .product_definitions
                .iter()
                .find(|row| row.id == *before)
                .unwrap()
                .native_ref
        );
    }
    let native = output.ir().native.namespace("fcstd").unwrap();
    let properties = native.arena_as::<PropertyRecord>("properties").unwrap();
    for name in ["Early", "Late"] {
        let owner = crate::native::native_id("object", name);
        let property = properties
            .iter()
            .find(|row| row.owner == owner && row.name == "LinkedObject")
            .unwrap();
        assert_eq!(property.type_name, "App::PropertyXLink");
        assert_eq!(property.links().len(), 1);
        assert_eq!(
            property.links()[0].as_ref().unwrap().object(),
            Some("fcstd:native:object#Prototype")
        );
    }
}
