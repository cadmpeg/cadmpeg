//! Synthetic byte-level tests for the archive reader, the walk, and dispatch.

use super::reader::{Reader, Stop};
use super::Status;
use crate::test_support::carchive::{
    config_object_stream, integer_parameter_body, real_parameter_body, resolved_features_stream,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use std::collections::{BTreeMap, HashMap};

const NODE_NAME: &str = "moNodeName_c";

fn known() -> BTreeMap<u32, String> {
    BTreeMap::from([(1, NODE_NAME.to_owned())])
}

/// The count u32, a u16, then `rest` as the object stream.
fn stream(count: u32, rest: &[u8]) -> Vec<u8> {
    let mut bytes = count.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(rest);
    bytes
}

/// A `moNodeName_c` body: the framed UTF-16 name `A`.
fn name_body() -> Vec<u8> {
    vec![0xff, 0xfe, 0xff, 0x01, b'A', 0x00]
}

fn walk_with(bytes: &[u8], policy: &DecodePolicy) -> super::Walk {
    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, policy).expect("context");
    super::walk(&ctx, &known(), bytes, Some(17000)).expect("the stream holds a count")
}

fn walk_with_known(bytes: &[u8], known: &BTreeMap<u32, String>) -> super::Walk {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop()).expect("context");
    super::walk(&ctx, known, bytes, Some(17000)).expect("the stream holds a count")
}

#[test]
fn a_named_object_reaches_the_end_of_the_stream() {
    let mut object = vec![0x01, 0x80];
    object.extend(name_body());
    let bytes = stream(2, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.tops, 1);
    assert_eq!(walk.trace.len(), 1);
    assert_eq!(&*walk.trace[0].class, NODE_NAME);
}

#[test]
fn null_and_short_pointers_open_no_object() {
    let bytes = stream(2, &[0, 0, 1, 0]);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.tops, 2);
    assert_eq!(walk.trace.len(), 0);
}

#[test]
fn a_long_tag_resolves_a_known_class() {
    let mut object = vec![0xff, 0x7f];
    object.extend_from_slice(&0x8000_0001_u32.to_le_bytes());
    object.extend(name_body());
    let bytes = stream(2, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.tops, 1);
    assert_eq!(&*walk.trace[0].class, NODE_NAME);
}

#[test]
fn an_inline_declaration_names_its_class() {
    let mut object = vec![0xff, 0xff, 0, 0];
    object.extend_from_slice(&u16::try_from(NODE_NAME.len()).unwrap().to_le_bytes());
    object.extend_from_slice(NODE_NAME.as_bytes());
    object.extend(name_body());
    let bytes = stream(2, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.tops, 1);
    assert_eq!(&*walk.trace[0].class, NODE_NAME);
}

#[test]
fn an_unknown_inline_class_stops_the_walk() {
    let mut object = vec![0xff, 0xff, 0, 0];
    object.extend_from_slice(&3_u16.to_le_bytes());
    object.extend_from_slice(b"xyz");
    let bytes = stream(2, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert!(matches!(walk.status, Status::Stop(Stop::Unknown(ref class)) if &**class == "xyz"));
}

#[test]
fn a_counter_wrap_is_refused() {
    let mut object = vec![0x01, 0x80];
    object.extend(name_body());
    let bytes = stream(u32::MAX, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert!(
        matches!(walk.status, Status::Stop(Stop::Bad(ref message)) if message.contains("counter"))
    );
}

#[test]
fn the_context_depth_limit_stops_nesting() {
    let mut object = vec![0x01, 0x80];
    object.extend(name_body());
    let bytes = stream(2, &object);
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_recursion_depth: 0,
            ..ResourceLimits::desktop()
        },
        ..DecodePolicy::desktop()
    };
    let walk = walk_with(&bytes, &policy);
    assert!(matches!(
        walk.status,
        Status::Stop(Stop::Refused(limit))
            if limit.operation == "walk SLDPRT CArchive object"
    ));
}

#[test]
fn the_parameter_classes_dispatch_and_unknown_classes_do_not() {
    for class in [
        "moLengthParameter_c",
        "moAngleParameter_c",
        "moScalerParameter_c",
        "moIntegerParameter_c",
    ] {
        assert!(super::rules::rule(class).is_some(), "{class}");
    }
    assert!(super::rules::rule("noSuchClass_c").is_none());
}

#[test]
fn walk_version_prefers_the_declaration_then_the_offset_without_a_default() {
    assert_eq!(super::walk_version(Some("15000"), &[]), Some(15_000));
    let mut config = vec![0; 26];
    config[22..26].copy_from_slice(&12_000_u32.to_le_bytes());
    assert_eq!(super::walk_version(None, &config), Some(12_000));
    config[22..26].copy_from_slice(&5_000_u32.to_le_bytes());
    assert_eq!(super::walk_version(None, &config), None);
    assert_eq!(super::walk_version(Some("not a version"), &config), None);
}

#[test]
fn short_reads_stop_at_the_end_and_slices_clip() {
    let bytes = [1_u8, 2, 3];
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
    assert_eq!(reader.u32(), Err(Stop::End(0)));
    assert!(matches!(reader.cstring(), Err(Stop::Bad(_))));
    assert_eq!(reader.slice(1, 9), &bytes[1..]);
    assert_eq!(reader.byte(3), Err(Stop::End(3)));
}

#[test]
fn a_pointer_that_names_the_wrong_class_is_refused() {
    let mut bytes = vec![0x01, 0x80];
    bytes.extend(name_body());
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let known = known()
        .into_iter()
        .map(|(index, name)| (index, std::sync::Arc::<str>::from(name)))
        .collect::<HashMap<_, _>>();
    let mut reader = Reader::new(&ctx, &bytes, 0, known, 2, None);
    assert!(matches!(
        reader.ptr(Some(&["Other_c"])),
        Err(Stop::Bad(ref message)) if message.contains("Other_c")
    ));
    let mut reader = Reader::new(&ctx, &[0, 0], 0, HashMap::new(), 0, None);
    assert!(matches!(reader.obj(None), Err(Stop::Bad(_))));
}

/// The walk that types lane scalars reads `value_at`, the offset of the
/// parameter's value, and the parameter's class. Each parameter class carries
/// it at the same offset: tag, name object, named-object header, u16 `0`.
#[test]
fn every_parameter_class_reports_the_class_and_value_offset() {
    let known = BTreeMap::from([
        (4, "moNodeName_c".to_owned()),
        (7, "moLengthParameter_c".to_owned()),
    ]);
    for (class, value, expected) in [
        ("moLengthParameter_c", 0.025_f64.to_le_bytes(), 0.025),
        ("moAngleParameter_c", 0.5_f64.to_le_bytes(), 0.5),
        ("moScalerParameter_c", (-2.0_f64).to_le_bytes(), -2.0),
    ] {
        let mut known = known.clone();
        known.insert(7, class.to_owned());
        let mut object = vec![0x07, 0x80];
        object.extend(real_parameter_body(value));
        let bytes = stream(7, &object);
        let walk = walk_with_known(&bytes, &known);
        assert_eq!(walk.status, Status::End, "{class}");
        assert_eq!(walk.parameters.len(), 1, "{class}");
        let parameter = &walk.parameters[0];
        assert_eq!(&*parameter.class, class);
        assert_eq!(parameter.value_at, 38, "{class}");
        assert_eq!(parameter.value, expected, "{class}");
    }

    let mut known = known;
    known.insert(7, "moIntegerParameter_c".to_owned());
    let mut object = vec![0x07, 0x80];
    object.extend(integer_parameter_body(42));
    let bytes = stream(7, &object);
    let walk = walk_with_known(&bytes, &known);
    assert_eq!(walk.status, Status::End);
    assert_eq!(&*walk.parameters[0].class, "moIntegerParameter_c");
    assert_eq!(walk.parameters[0].value_at, 38);
    assert_eq!(walk.parameters[0].value, 42.0);
}

#[test]
fn a_walk_that_meets_an_unknown_class_stops() {
    let known = BTreeMap::from([
        (4, "moNodeName_c".to_owned()),
        (7, "moLengthParameter_c".to_owned()),
    ]);
    let mut object = vec![0x07, 0x80];
    object.extend(real_parameter_body(0.025_f64.to_le_bytes()));
    object.extend_from_slice(&[0xff, 0xff, 0, 0]);
    object.extend_from_slice(&3_u16.to_le_bytes());
    object.extend_from_slice(b"xyz");
    let bytes = stream(7, &object);
    let walk = walk_with_known(&bytes, &known);
    assert!(matches!(
        walk.status,
        Status::Stop(Stop::Unknown(ref class)) if &**class == "xyz"
    ));
}

#[test]
fn a_reconciled_configuration_map_types_the_parameter_object() {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).expect("context");
    let walk = super::walk_configuration(
        &ctx,
        &config_object_stream(),
        &resolved_features_stream(),
        Some(17_000),
    )
    .expect("the class walk succeeds")
    .expect("the map reconciles with the leading count");
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.parameters.len(), 1);
    assert_eq!(&*walk.parameters[0].class, "moLengthParameter_c");
    assert_eq!(walk.parameters[0].value_at, 38);
}

#[test]
fn a_declared_blob_size_above_the_expansion_allowance_is_refused() {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"x").expect("encode");
    let compressed = encoder.finish().expect("finish");
    let mut bytes = vec![0];
    let b = 24_usize.checked_add(compressed.len()).expect("length");
    bytes.extend_from_slice(&u32::try_from(b + 5).expect("a fits u32").to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&u32::try_from(b).expect("b fits u32").to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&3_000_000_000_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(compressed.len())
            .expect("cl fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&compressed);
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
    assert!(matches!(
        super::rules::sketch::blob(&mut reader),
        Err(Stop::Refused(_))
    ));
}

#[test]
fn a_restored_snapshot_rolls_back_the_map_counter_and_records() {
    let mut object = vec![0xff, 0xff, 0, 0];
    object.extend_from_slice(
        &u16::try_from(NODE_NAME.len())
            .expect("name fits u16")
            .to_le_bytes(),
    );
    object.extend_from_slice(NODE_NAME.as_bytes());
    object.extend(name_body());
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&object, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &object, 0, HashMap::new(), 0, None);
    let snapshot = reader.snapshot();
    assert!(reader.ptr(None).expect("inline declaration").is_obj());
    assert_ne!(reader.counter, 0);
    reader.restore(snapshot);
    assert_eq!(reader.counter, 0);
    assert_eq!(reader.p, 0);
    assert_eq!(reader.known.len(), 0);
    assert_eq!(reader.objs.len(), 0);
    assert_eq!(reader.parameters.len(), 0);
    assert_eq!(reader.trace.len(), 0);
}

/// An inline class declaration: `ff ff`, schema, name length and name.
fn inline_declaration(name: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0, 0];
    bytes.extend_from_slice(
        &u16::try_from(name.len())
            .expect("name fits u16")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

fn reader_over<'a>(ctx: &'a DecodeContext<'a>, bytes: &'a [u8]) -> Reader<'a> {
    Reader::new(ctx, bytes, 0, HashMap::new(), 0, None)
}

#[test]
fn the_identity_rules_read_their_bodies() {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).expect("context");

    let cstring = [0xff, 0xfe, 0xff, 0x01, b'A', 0];
    let mut reader = reader_over(&ctx, &cstring);
    assert_eq!(
        super::rules::idrep::cstringhandle(&mut reader),
        Ok(super::reader::V::Str("A".to_owned()))
    );

    let mut fr = Vec::new();
    fr.extend_from_slice(&[0, 0]);
    fr.extend_from_slice(&7_u32.to_le_bytes());
    fr.extend_from_slice(&11_u32.to_le_bytes());
    let mut reader = reader_over(&ctx, &fr);
    let value = super::rules::idrep::fr(&mut reader).expect("frame identity");
    assert_eq!(value.get("fid").as_int(), Some(7));
    assert_eq!(value.get("ts").as_int(), Some(11));
}

#[test]
fn the_sketch_handle_rule_reads_its_slot() {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).expect("context");
    let mut body = Vec::new();
    body.extend_from_slice(&3_u16.to_le_bytes());
    body.extend_from_slice(&u32::MAX.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    let mut reader = reader_over(&ctx, &body);
    assert_eq!(
        super::rules::sketch::handle(&mut reader),
        Ok(super::reader::V::Int(3))
    );
}

#[test]
fn the_reference_rule_reads_an_empty_hop_list() {
    const MARKER: [u8; 16] = [
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ];
    let mut body = Vec::new();
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&MARKER);
    body.extend_from_slice(&0_u16.to_le_bytes());
    body.extend_from_slice(&[0; 8]);
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = reader_over(&ctx, &body);
    let value = super::rules::refs::ref_(&mut reader).expect("empty reference");
    assert_eq!(value.get("cnt").as_int(), Some(0));
}

#[test]
fn a_walk_reads_several_inline_class_families() {
    let mut object = inline_declaration(NODE_NAME);
    object.extend(name_body());
    object.extend(inline_declaration("moCStringHandle_c"));
    object.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'A', 0]);
    object.extend(inline_declaration("sgLineHandle"));
    object.extend_from_slice(&1_u16.to_le_bytes());
    object.extend_from_slice(&u32::MAX.to_le_bytes());
    object.extend_from_slice(&0_u32.to_le_bytes());
    let bytes = stream(2, &object);
    let walk = walk_with(&bytes, &DecodePolicy::desktop());
    assert_eq!(walk.status, Status::End);
    assert_eq!(walk.tops, 3);
    assert_eq!(
        walk.trace
            .iter()
            .map(|trace| &*trace.class)
            .collect::<Vec<_>>(),
        [NODE_NAME, "moCStringHandle_c", "sgLineHandle"]
    );
}
