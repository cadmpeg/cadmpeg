//! Synthetic byte-level tests for the archive reader, the walk, and dispatch.

use super::reader::{Reader, Stop};
use super::Status;
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
    assert!(walk.trace.is_empty());
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
fn walk_version_prefers_the_declaration_then_the_offset_then_17000() {
    assert_eq!(super::walk_version(Some("15000"), &[]), 15000);
    let mut config = vec![0; 26];
    config[22..26].copy_from_slice(&12_000_u32.to_le_bytes());
    assert_eq!(super::walk_version(None, &config), 12_000);
    config[22..26].copy_from_slice(&5_000_u32.to_le_bytes());
    assert_eq!(super::walk_version(None, &config), 17_000);
    assert_eq!(super::walk_version(Some("not a version"), &config), 17_000);
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
