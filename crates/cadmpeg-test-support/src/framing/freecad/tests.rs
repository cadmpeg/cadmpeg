// SPDX-License-Identifier: Apache-2.0
use super::frame_xml_member;
use crate::framing::{FrameParent, FramedPart, PartTable};
use crate::zip_fixture::{RawEntry, archive};
use cadmpeg_core::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, View, XmlTermination,
};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

fn source(
    name: &str,
    xml: &[u8],
    run: impl for<'input> FnOnce(
        &DecodeContext<'input>,
        View<'input>,
        View<'input>,
        &mut Vec<FramedPart<'input>>,
    ),
) {
    let bytes = archive(&[RawEntry::stored(name, xml)]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("ZIP source");
    let root = ctx.name_stream(root, "file").expect("root name");
    // docs/layouts/zip.toml: local_header is 30 bytes, with the filename next.
    let name_end = 30 + name.len();
    let payload_end = name_end + xml.len();
    let name_view = root.child(30, name_end).expect("stored entry name");
    let member = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: u64::try_from(name_end).expect("name offset"),
                end: u64::try_from(payload_end).expect("payload end"),
            },
            name,
        )
        .expect("exact stored source provenance");
    let mut frames = vec![
        FramedPart::new(
            &ctx,
            PartLevel::parse("zip.archive").expect("table"),
            root,
            FrameParent::File,
            vec![],
            vec![],
        )
        .expect("ZIP archive frame"),
        FramedPart::new(
            &ctx,
            PartLevel::parse("zip.entry").expect("table"),
            root.child(0, payload_end).expect("actual local entry"),
            FrameParent::Part(0),
            vec![0..u64::try_from(name_end).expect("local header end")],
            vec![],
        )
        .expect("ZIP entry frame"),
    ];
    run(&ctx, member, name_view, &mut frames);
}

#[test]
fn actual_nested_xml_proves_physical_parent_and_separate_declared_owner() {
    let xml = br#"<Document SchemaVersion="4"><Objects><Object type="Part::Feature" name="A"/></Objects><ObjectData><Object name="A"><Extensions><Extension type="App::GroupExtension" name="G"><Properties><Property type="App::PropertyString" name="P"><String><Item><Nested/></Item></String></Property></Properties></Extension></Extensions></Object></ObjectData></Document>"#;
    source("Document.xml", xml, |ctx, member, name, frames| {
        let reached = ctx
            .record("fcstd:source:record#framing", member, || {
                frame_xml_member(ctx, member, name, 1, frames)
                    .map_err(cadmpeg_core::CodecError::Malformed)
            })
            .expect("actual XML frames");
        PartTable::load()
            .expect("immutable table")
            .validate(ctx, frames)
            .expect("nested physical parents plus declared ancestor owners");
        let nested = frames
            .iter()
            .position(|frame| frame.view().window() == b"<Nested/>")
            .expect("nested child");
        let FrameParent::Part(parent) = frames[nested].parent() else {
            panic!("physical XML parent");
        };
        assert_eq!(frames[parent].view().window(), b"<Item><Nested/></Item>");
        assert_eq!(frames[parent].level().as_str(), "freecad.value_element");
        assert_eq!(frames[nested].level().as_str(), "freecad.value_element");
        let proof = frames[nested]
            .xml_containment()
            .expect("opaque source XML proof");
        assert_eq!(
            frames[proof.owner].level().as_str(),
            "freecad.property_value"
        );
        let properties = frames
            .iter()
            .find(|frame| frame.level().as_str() == "freecad.properties")
            .expect("extension properties");
        let proof = properties.xml_containment().expect("source XML proof");
        assert_eq!(frames[proof.owner].level().as_str(), "freecad.document");
        assert!(reached.scan().faults().is_empty());
        let captures = ctx.take_record_captures().expect("framing scope capture");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), "fcstd:source:record#framing");
        assert!(captures[0].reads().is_empty());
    });
}

#[test]
fn missing_source_parent_proof_cannot_waive_table_or_sibling_rules() {
    let xml = b"<Document><Properties><Property><Value><Inner><Leaf/></Inner></Value></Property></Properties></Document>";
    source("Document.xml", xml, |ctx, member, name, frames| {
        let reached = frame_xml_member(ctx, member, name, 1, frames).expect("XML inventory");
        let leaf_node = reached
            .scan()
            .elements()
            .iter()
            .position(|node| node.name().window() == b"Leaf")
            .expect("leaf");
        let leaf = reached.frame_for_element(leaf_node).expect("leaf frame");
        let FrameParent::Part(parent) = frames[leaf].parent() else {
            panic!("parent");
        };
        let original = &frames[leaf];
        let no_proof = FramedPart::new(
            ctx,
            original.level(),
            original.view(),
            original.parent(),
            original.framing().to_vec(),
            original.dispatch().to_vec(),
        )
        .expect("bounded leaf");
        let old = std::mem::replace(&mut frames[leaf], no_proof);
        assert!(
            PartTable::load()
                .expect("table")
                .validate(ctx, frames)
                .is_err()
        );
        frames[leaf] = old;
        let owner = frames[leaf].xml_containment().expect("proof").owner;
        // The same real XML proof cannot claim that the more distant owner is
        // the immediate physical parent and turn nesting into overlapping siblings.
        let wrong_parent = FramedPart::new(
            ctx,
            frames[leaf].level(),
            frames[leaf].view(),
            FrameParent::Part(owner),
            frames[leaf].framing().to_vec(),
            vec![],
        )
        .expect("bounded leaf")
        .with_xml_parent(ctx, reached.scan(), leaf_node, owner)
        .expect("actual leaf proof");
        frames[leaf] = wrong_parent;
        assert!(
            PartTable::load()
                .expect("table")
                .validate(ctx, frames)
                .is_err()
        );
        assert_ne!(parent, owner);
    });
}

#[test]
fn damaged_and_unsupported_terminated_property_parts_preserve_later_sibling_boundaries() {
    let xml = b"<Document><Properties><Property type='Unknown'><Opaque><![CDATA[<Property/>]]></Opaque></Property><Property type='Broken'><Value a='&bad;'/></Property><Property type='App::PropertyString'><String value='kept'/></Property></Properties></Document>";
    source("Document.xml", xml, |ctx, member, name, frames| {
        let reached = frame_xml_member(ctx, member, name, 1, frames)
            .expect("framing precedes semantic XML parsing");
        PartTable::load()
            .expect("table")
            .validate(ctx, frames)
            .expect("terminated sibling extents");
        let properties = frames
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.property")
            .collect::<Vec<_>>();
        assert_eq!(properties.len(), 3);
        assert_eq!(
            properties[2].view().window(),
            b"<Property type='App::PropertyString'><String value='kept'/></Property>"
        );
        assert_eq!(
            reached
                .scan()
                .elements()
                .iter()
                .filter(|node| node.name().window() == b"Property")
                .count(),
            3
        );
        assert!(
            ctx.parse_xml(
                std::str::from_utf8(properties[1].view().window()).expect("literal ASCII"),
                "damaged property"
            )
            .is_err()
        );
        assert!(
            ctx.parse_xml(
                std::str::from_utf8(properties[2].view().window()).expect("literal ASCII"),
                "later property"
            )
            .is_ok()
        );
    });
}

#[test]
fn cut_short_elements_stay_cut_short_and_do_not_claim_decode_success() {
    let xml = b"<Document><Properties><Property><Value></Wrong></Property><Property><String/></Property></Properties>";
    source("Document.xml", xml, |ctx, member, name, frames| {
        let reached =
            frame_xml_member(ctx, member, name, 1, frames).expect("lexical damage inventory");
        assert_eq!(
            reached.scan().elements()[0].termination(),
            XmlTermination::EndOfInput
        );
        assert!(
            reached
                .scan()
                .elements()
                .iter()
                .any(|node| node.name().window() == b"Value"
                    && node.termination() == XmlTermination::AncestorBoundary)
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.level().as_str() == "freecad.property")
                .count(),
            2
        );
        assert!(!reached.scan().faults().is_empty());
        PartTable::load()
            .expect("table")
            .validate(ctx, frames)
            .expect("actual cut-short extents retain containment");
    });
}

#[test]
fn gui_nested_state_and_values_keep_real_parent_and_external_dispatch_source() {
    let xml = b"<Document SchemaVersion='1'><Camera settings='&lt;Camera/&gt;'><Clip><Plane/></Clip></Camera><ViewProviderData><ViewProvider name='A'><Properties><Property type='App::PropertyColor' name='ShapeColor'><PropertyColor><Item><R/></Item></PropertyColor></Property></Properties></ViewProvider></ViewProviderData></Document>";
    source("GuiDocument.xml", xml, |ctx, member, name, frames| {
        let reached = frame_xml_member(ctx, member, name, 1, frames).expect("GUI XML inventory");
        PartTable::load()
            .expect("table")
            .validate(ctx, frames)
            .expect("GUI nested ownership");
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.level().as_str() == "freecad.gui_state_element")
                .count(),
            2
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.level().as_str() == "freecad.gui_value_element")
                .count(),
            2
        );
        let value = frames
            .iter()
            .find(|frame| frame.level().as_str() == "freecad.gui_property_value")
            .expect("value");
        assert_eq!(
            value.external_dispatch()[0].window(),
            b"type='App::PropertyColor'"
        );
        assert!(reached.scan().faults().is_empty());
    });
}

#[test]
fn declared_owner_must_be_a_real_ancestor_not_a_prior_matching_level() {
    let xml = b"<Document><Properties><Property><First/></Property><Property><Second><Item><Leaf/></Item></Second></Property></Properties></Document>";
    source("Document.xml", xml, |ctx, member, name, frames| {
        let reached = frame_xml_member(ctx, member, name, 1, frames).expect("XML inventory");
        let first = frames
            .iter()
            .position(|frame| frame.view().window() == b"<First/>")
            .expect("earlier value");
        let leaf_node = reached
            .scan()
            .elements()
            .iter()
            .position(|node| node.name().window() == b"Leaf")
            .expect("leaf");
        let leaf = reached.frame_for_element(leaf_node).expect("leaf frame");
        let frame = &frames[leaf];
        let false_owner = FramedPart::new(
            ctx,
            frame.level(),
            frame.view(),
            frame.parent(),
            frame.framing().to_vec(),
            frame.dispatch().to_vec(),
        )
        .expect("bounded leaf")
        .with_xml_parent(ctx, reached.scan(), leaf_node, first)
        .expect("real physical parent proof");
        frames[leaf] = false_owner;
        assert!(
            PartTable::load()
                .expect("table")
                .validate(ctx, frames)
                .is_err()
        );
    });
}
