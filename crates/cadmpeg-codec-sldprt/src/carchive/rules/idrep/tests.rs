use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::carchive::reader::{Ptr, Reader, Stop, V};

/// A new-class declaration of `name` whose object follows.
fn declaration(name: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0, 0];
    bytes.extend_from_slice(
        &u16::try_from(name.len())
            .expect("name fits u16")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

/// A declared side-face identity and its declared `moFR_c` frame, up to the own bytes.
fn side_face_identity_head() -> Vec<u8> {
    let mut bytes = declaration("moSideFace3IntSurfIdRep_c");
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend(declaration("moFR_c"));
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    bytes.extend_from_slice(&11_u32.to_le_bytes());
    bytes
}

#[test]
fn a_side_face_identity_cut_short_at_the_stream_end_stops_the_walk() {
    let mut bytes = side_face_identity_head();
    for value in [4_u32, 5] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
    assert!(matches!(reader.ptr(None), Err(Stop::End(_))));
}

#[test]
fn a_side_face_identity_holds_three_u32_after_its_frame() {
    let mut bytes = side_face_identity_head();
    let mut own = Vec::new();
    for value in [4_u32, 5, 6] {
        own.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&own);
    let end = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xaa]);
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
    let Ptr::Obj { value, .. } = reader.ptr(None).expect("the identity reads") else {
        panic!("an object");
    };
    assert_eq!(value.get("own"), &V::Bytes(own));
    assert_eq!(reader.p, end);
}
