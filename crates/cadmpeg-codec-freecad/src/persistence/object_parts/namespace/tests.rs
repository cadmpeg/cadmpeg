// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

#[test]
fn unreadable_declaration_name_has_one_home_and_keeps_the_actual_child_schedule() {
    let xml = b"<Document><Objects Count='2' Dependencies='1'><ObjectDeps Name='A' Count='1'><Dep Name='B'/></ObjectDeps><ObjectDeps Name='B' Count='0'/><Object name='A' type='App::Feature'/><Object type='App::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='1'><Property name='Value' type='App::PropertyString'><String value='held only if owner exists'/></Property></Properties></Object></ObjectData></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let source = Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let declarations =
        decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    document_properties::finish_pending(&ctx, &source, &context, &mut output).unwrap();
    assert_eq!(declarations.objects.len(), 1);
    assert!(declarations.objects[0].dependencies.is_empty());
    let object = crate::native::native_id("object", "B");
    assert!(context.has_declared_object(&object));
    assert!(!context.has_admitted_object(&object));
    let property = crate::native::native_child_id("property", &object, "Value");
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let failures = fidelity
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].level().as_str(), "freecad.object_declaration");
    assert!(fidelity
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.property_value"
            && part.state() == &(PartState::Decoded {})));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(&object) && loss.message.contains(&property)));
    let records = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(records.iter().all(|record| record.id != property));
    assert!(ctx
        .take_record_captures()
        .unwrap()
        .iter()
        .all(|record| record.id() != property));
}

#[test]
fn a_known_failed_declaration_does_not_license_an_unrelated_undeclared_target() {
    let xml = b"<Document><Objects Count='2' Dependencies='1'><ObjectDeps Name='A' Count='1'><Dep Name='NeverDeclared'/></ObjectDeps><ObjectDeps Name='B' Count='0'/><Object name='A' type='App::Feature'/><Object name='B'/></Objects><ObjectData Count='0'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::default()).unwrap();
    let source = Rc::new(crate::xml_framing::XmlSource::new(&ctx, view).unwrap());
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    decode_scanned(&ctx, &source, 0, Vocabulary::Objects, &context, &mut output).unwrap();
    let (_, _, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let levels = fidelity
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}))
        .map(|part| part.level().as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        levels,
        BTreeSet::from(["freecad.object_declaration", "freecad.object_dep"])
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        2
    );
    assert!(!context.has_declared_object(&crate::native::native_id("object", "NeverDeclared")));
}
