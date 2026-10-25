// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

#[test]
fn derived_object_record_constructs_its_owner_from_the_actual_declaration_field() {
    let declaration = "<Object name='A&amp;B' type='App::Part'/>";
    let xml = format!(
        "<Document><Objects Count='1'>{declaration}</Objects><ObjectData Count='0'/></Document>"
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default()).unwrap();
    let source = std::rc::Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let declarations =
        decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    let object = crate::native::native_id("object", "A&B");
    let id = crate::native::native_id("product", "A&B");
    let record = declarations.sources[&object]
        .record(&ctx, &id, |actual| {
            Ok(crate::native::ProductNodeRecord {
                id: id.clone(),
                object: actual,
                node: crate::native::ProductNode::Group(crate::native::ContainerNode {
                    members: Vec::new(),
                    local_transform: None,
                    placement_property: None,
                }),
            })
        })
        .unwrap();
    assert_eq!(record.object, object);
    let (_, mut native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "product_nodes", &[record])
        .unwrap();
    assert_eq!(
        native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
            .unwrap()[0]
            .object,
        object
    );
    assert!(losses.is_empty());
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures.iter().find(|capture| capture.id() == id).unwrap();
    let start = xml.find(declaration).unwrap();
    assert_eq!(capture.offset(), u64::try_from(start).unwrap());
    assert_eq!(
        capture.byte_len(),
        u64::try_from(declaration.len()).unwrap()
    );
    let field_start = xml.find(" name='A&amp;B'").unwrap();
    assert_eq!(
        capture.reads()["file"],
        [
            u64::try_from(field_start + 1).unwrap()..u64::try_from(field_start + 5).unwrap(),
            u64::try_from(field_start + " name='".len()).unwrap()
                ..u64::try_from(field_start + " name='A&amp;B".len()).unwrap(),
        ]
    );
}

#[test]
fn dependency_parts_keep_readable_list_entries_after_one_dep_fails() {
    let xml = b"<Document><Objects Count='2' Dependencies='1'><ObjectDeps Name='A' Count='2'><Dep/><Dep Name='B'/></ObjectDeps><ObjectDeps Name='B' Count='0'/><Object name='A' type='Part::Feature'/><Object name='B' type='Part::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let source = std::rc::Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(1));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let declarations =
        decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    assert_eq!(declarations.objects.len(), 2);
    assert_eq!(
        declarations.objects[0].dependencies,
        [crate::native::native_id("object", "B")]
    );
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let objects = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ObjectRecord>("objects")
        .unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(
        objects[0].dependencies,
        [crate::native::native_id("object", "B")]
    );
    let given_up: Vec<_> = fidelity
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .collect();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].level().as_str(), "freecad.object_dep");
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}

#[test]
fn malformed_declaration_drops_only_its_data_and_list_references() {
    let xml = b"<Document><Objects Count='2' Dependencies='1'><ObjectDeps Name='A' Count='1'><Dep Name='B'/></ObjectDeps><ObjectDeps Name='B' Count='0'/><Object name='A' type='Part::Feature'/><Object name='B' type='&undeclared;'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let source = std::rc::Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(1));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let declarations =
        decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    assert_eq!(declarations.objects.len(), 1);
    assert_eq!(
        declarations.declared_objects,
        BTreeSet::from([
            crate::native::native_id("object", "A"),
            crate::native::native_id("object", "B"),
        ])
    );
    assert!(context.has_declared_object(&crate::native::native_id("object", "B")));
    assert_eq!(
        declarations.admitted_objects,
        BTreeSet::from([crate::native::native_id("object", "A")])
    );
    assert!(context.has_admitted_object(&crate::native::native_id("object", "A")));
    assert!(!context.has_admitted_object(&crate::native::native_id("object", "B")));
    assert!(!context.has_declared_object(&crate::native::native_id("object", "NeverDeclared")));
    assert_eq!(declarations.data_sources.len(), 1);
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let objects = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ObjectRecord>("objects")
        .unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name(), "A");
    assert!(objects[0].dependencies.is_empty());
    let levels: BTreeSet<_> = fidelity
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .map(|part| part.level().as_str())
        .collect();
    assert_eq!(levels, BTreeSet::from(["freecad.object_declaration"]));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}

#[test]
fn failed_declaration_dependencies_keep_valid_peers_without_reading_the_lost_target() {
    let xml = b"<Document><Objects Count='3' Dependencies='1'><ObjectDeps Name='A' Count='2'><Dep Name='B'/><Dep Name='C'/></ObjectDeps><ObjectDeps Name='B' Count='0'/><ObjectDeps Name='C' Count='0'/><Object name='A' type='Part::Feature'/><Object name='B' type='&undeclared;'/><Object name='C' type='Part::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='C'><Properties Count='0'/></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let source = Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(1));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let declarations =
        decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    assert_eq!(declarations.objects.len(), 2);
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let objects = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ObjectRecord>("objects")
        .unwrap();
    let a = objects.iter().find(|object| object.name() == "A").unwrap();
    assert_eq!(a.dependencies, [crate::native::native_id("object", "C")]);
    assert!(objects.iter().any(|object| object.name() == "C"));
    let failed: Vec<_> = fidelity
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.object_declaration");
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == a.id())
        .unwrap();
    let text = std::str::from_utf8(xml).unwrap();
    let lost_start = u64::try_from(text.find(" Name='B'").unwrap()).unwrap();
    let lost_end = lost_start + u64::try_from(" Name='B'".len()).unwrap();
    assert!(capture.reads()["file"]
        .iter()
        .all(|read| read.end <= lost_start || read.start >= lost_end));
    let kept_start = u64::try_from(text.find(" Name='C'").unwrap()).unwrap();
    for kept in [
        kept_start + 1..kept_start + 5,
        kept_start + 7..kept_start + 8,
    ] {
        assert!(capture.reads()["file"]
            .iter()
            .any(|read| read.start <= kept.start && read.end >= kept.end));
    }
}
