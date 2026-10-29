// SPDX-License-Identifier: Apache-2.0
//! #213: source-free writer reads sidedness through neutral forward references.

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;
use cadmpeg_asm::brep::records::{
    identity::NativeRecordNamespace, FaceContainment, FaceNativeKey, FaceSidedness,
};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::Sense;
use std::io::Cursor;

#[test]
fn source_free_writer_resolves_sidedness_with_key_and_direct_native_refs() {
    for keyed in [false, true] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut side = FaceSidedness {
            source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
            record_index: 17,
            face_key: None,
            native_sense: Sense::Forward,
            carrier_flipped: false,
            containment: Some(FaceContainment::Out),
        };
        let key = FaceNativeKey {
            source_namespace: side.source_namespace.clone(),
            record_index: 17,
            asm_face_key: Some(42),
        };
        let reference = if keyed {
            side.face_key = Some(key.id());
            key.id()
        } else {
            side.id()
        };
        ir.model.faces[0].native_ref = Some(reference.clone());
        ir.model.faces[1].native_ref = Some(reference);
        let expected_senses = [ir.model.faces[0].sense, ir.model.faces[1].sense];
        let mut native = f3d_native_mut(&mut ir);
        native.face_sidedness = vec![side];
        if keyed {
            native.face_native_keys = vec![key];
        }
        drop(native);
        let plan = F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let result = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = f3d_native(result.ir());
        assert_eq!(result.ir().model.faces.len(), 6);
        for (face, expected_sense) in result.ir().model.faces[..2].iter().zip(expected_senses) {
            let side = native
                .face_sidedness
                .iter()
                .find(|row| {
                    row.owns_native_ref(face.native_ref.as_deref(), &native.face_native_keys)
                })
                .unwrap();
            assert_eq!(side.containment, Some(FaceContainment::Out));
            assert_eq!(face.sense, expected_sense);
            assert!(!side.carrier_flipped);
            assert!(serde_json::to_value(side).unwrap().get("face").is_none());
        }
    }
}

#[test]
fn source_free_writer_preserves_a_key_reached_through_native_sidedness() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let key = FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
        record_index: 17,
        asm_face_key: Some(42),
    };
    let side = FaceSidedness {
        source_namespace: key.source_namespace.clone(),
        record_index: 17,
        face_key: Some(key.id()),
        native_sense: Sense::Forward,
        carrier_flipped: false,
        containment: Some(FaceContainment::Out),
    };
    ir.model.faces[0].native_ref = Some(side.id());
    let mut native = f3d_native_mut(&mut ir);
    native.face_native_keys = vec![key];
    native.face_sidedness = vec![side];
    drop(native);
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = f3d_native(result.ir());
    let face = &result.ir().model.faces[0];
    let key = native
        .face_native_keys
        .iter()
        .find(|row| row.owns_native_ref(face.native_ref.as_deref(), &native.face_sidedness))
        .unwrap();
    assert_eq!(key.asm_face_key, Some(42));
    let side = native
        .face_sidedness
        .iter()
        .find(|row| row.owns_native_ref(face.native_ref.as_deref(), &native.face_native_keys))
        .unwrap();
    assert_eq!(side.face_key.as_deref(), Some(key.id().as_str()));
    assert_eq!(side.containment, Some(FaceContainment::Out));
}
