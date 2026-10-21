// SPDX-License-Identifier: Apache-2.0
//! Native face join keys remain independent of geometry and source layout.

use std::io::Cursor;

use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, FaceNativeKey};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, NativeDisposition};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

fn record(face: cadmpeg_ir::ids::FaceId, index: u32, key: Option<u64>) -> FaceNativeKey {
    FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
        record_index: index,
        face,
        asm_face_key: key,
    }
}

#[test]
fn source_free_synthesis_preserves_face_keys_and_drops_only_unresolved_targets() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|face| face.id.clone())
        .collect::<Vec<_>>();
    let keys = [Some(0), Some(42), None, Some(42), Some(9), None];
    {
        let mut native = f3d_native_mut(&mut ir);
        for (ordinal, (face, key)) in faces.iter().zip(keys).enumerate() {
            native.face_native_keys.push(record(
                face.clone(),
                u32::try_from(ordinal).unwrap(),
                key,
            ));
        }
        native.face_native_keys.push(record(
            "authored:model:face#absent".to_owned().try_into().unwrap(),
            100,
            Some(123),
        ));
    }
    let input_native = f3d_native(&ir);
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for row in &input_native.face_native_keys[..faces.len()] {
        assert_eq!(
            plan.report().native_dispositions.record_disposition(
                "f3d",
                "face_native_keys",
                &row.id(),
            ),
            NativeDisposition::Written
        );
    }
    let orphan = input_native.face_native_keys.last().unwrap();
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "f3d",
            "face_native_keys",
            &orphan.id(),
        ),
        NativeDisposition::Dropped
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), faces.len());
    let native = f3d_native(output.ir());
    let actual = output
        .ir()
        .model
        .faces
        .iter()
        .map(|face| {
            native
                .face_native_keys
                .iter()
                .find(|record| record.face == face.id)
                .unwrap()
                .asm_face_key
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, keys);
}

#[test]
fn native_face_key_slot_refuses_duplicate_targets_and_capacity_overflow() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let face = ir.model.faces[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut ir);
        native
            .face_native_keys
            .push(record(face.clone(), 0, Some(7)));
        native
            .face_native_keys
            .push(record(face.clone(), 1, Some(8)));
    }
    let error = match F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit) {
        Ok(_) => panic!("one face has only one native key slot"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("multiple native join-key records"),
        "{error}"
    );
    {
        let mut native = f3d_native_mut(&mut ir);
        native.face_native_keys = vec![record(face, 0, Some(u64::MAX))];
    }
    let error = match F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit) {
        Ok(_) => panic!("the signed native field cannot hold u64::MAX"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("signed 64-bit native slot"),
        "{error}"
    );
}
