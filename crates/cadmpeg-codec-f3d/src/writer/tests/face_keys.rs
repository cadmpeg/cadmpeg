// SPDX-License-Identifier: Apache-2.0
//! Native face join keys remain independent of geometry and source layout.

use std::io::Cursor;

use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, FaceNativeKey};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, NativeDisposition};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

fn record(index: u32, key: Option<u64>) -> FaceNativeKey {
    FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
        record_index: index,
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
    let mut records = Vec::new();
    for (ordinal, (face, key)) in ir.model.faces.iter_mut().zip(keys).enumerate() {
        let key = record(u32::try_from(ordinal).unwrap(), key);
        face.native_ref = Some(key.id());
        records.push(key);
    }
    records.push(record(100, Some(123)));
    f3d_native_mut(&mut ir).face_native_keys = records;
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
                .find(|record| face.native_ref.as_deref() == Some(record.id().as_str()))
                .unwrap()
                .asm_face_key
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, keys);
}

#[test]
fn native_face_key_slot_refuses_duplicate_targets_and_capacity_overflow() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let first = record(0, Some(7));
    let second = record(1, Some(8));
    let face = ir.model.faces[0].id.clone();
    let mut first_wire = serde_json::to_value(&first).unwrap();
    let mut second_wire = serde_json::to_value(&second).unwrap();
    first_wire["face"] = serde_json::json!(face);
    second_wire["face"] = serde_json::json!(face);
    ir.native
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "face_native_keys",
            &[first_wire, second_wire],
        )
        .unwrap();
    let error = match F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit) {
        Ok(_) => panic!("the removed native backlink cannot encode duplicate target claims"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("unknown field `face`"),
        "{error}"
    );
    let oversized = record(0, Some(u64::MAX));
    ir.model.faces[0].native_ref = Some(oversized.id());
    ir.native
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "face_native_keys",
            &[oversized],
        )
        .unwrap();
    let error = match F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit) {
        Ok(_) => panic!("the signed native field cannot hold u64::MAX"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("signed 64-bit native slot"),
        "{error}"
    );
}

#[test]
fn shared_native_face_key_reference_writes_each_faces_single_slot() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let key = record(0, Some(7));
    ir.model.faces[0].native_ref = Some(key.id());
    ir.model.faces[1].native_ref = Some(key.id());
    f3d_native_mut(&mut ir).face_native_keys = vec![key];
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = f3d_native(output.ir());
    let keys: Vec<_> = output
        .ir()
        .model
        .faces
        .iter()
        .map(|face| {
            native
                .face_native_keys
                .iter()
                .find(|key| face.native_ref.as_deref() == Some(key.id().as_str()))
                .unwrap()
                .asm_face_key
        })
        .collect();
    assert_eq!(keys, [Some(7), Some(7), None, None, None, None]);
}
