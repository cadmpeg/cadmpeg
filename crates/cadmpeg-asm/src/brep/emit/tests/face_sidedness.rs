// SPDX-License-Identifier: Apache-2.0
//! #213: source face identity replaces the native-to-neutral face association.

use super::*;
use crate::brep::records::{identity::NativeRecordNamespace, FaceNativeKey};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn sidedness(index: u32) -> FaceSidedness {
    FaceSidedness {
        source_namespace: NativeRecordNamespace::new(crate::asm_format!("f3d")),
        record_index: index,
        face_key: None,
        native_sense: Sense::Reversed,
        carrier_flipped: true,
        containment: Some(crate::brep::records::FaceContainment::Out),
    }
}

#[test]
fn sidedness_joins_only_actual_native_namespace_and_source_record() {
    let mut side = sidedness(4);
    let key = FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(crate::asm_format!("f3d")),
        record_index: 4,
        asm_face_key: Some(77),
    };
    side.face_key = Some(key.id());
    assert!(side.owns_native_ref(Some(&key.id()), std::slice::from_ref(&key)));
    assert!(side.owns_native_ref(Some(&side.id()), &[]));
    assert!(!side.owns_native_ref(None, std::slice::from_ref(&key)));
    assert!(!side.owns_native_ref(Some("f3d:asm:face-native-key#4"), &[]));
    assert!(!sidedness(5).owns_native_ref(Some(&key.id()), std::slice::from_ref(&key)));
    let foreign = FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(crate::asm_format!("sat")),
        ..key.clone()
    };
    assert!(!side.owns_native_ref(Some(&foreign.id()), std::slice::from_ref(&foreign)));
    for reference in [
        "f3d:brep:entity#4",
        "f3d:asm:face-sidedness#04",
        "f3d:asm:face-sidedness#4x",
    ] {
        assert!(!side.owns_native_ref(Some(reference), std::slice::from_ref(&key)));
    }
}

#[test]
fn sidedness_wire_keeps_native_sense_containment_and_rejects_removed_backlink() {
    let side = sidedness(4);
    let mut wire = serde_json::to_value(&side).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "id": "f3d:asm:face-sidedness#4", "record_index": 4,
            "native_sense": "reversed", "normalized_sense": "forward", "containment": "out"
        })
    );
    let restored: FaceSidedness = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored, side);
    wire["face"] = serde_json::json!("f3d:brep:entity#4");
    assert!(serde_json::from_value::<FaceSidedness>(wire).is_err());
}

#[test]
fn keyless_emitted_face_links_the_actual_sidedness_row_atomically() {
    use super::super::super::DecodePurpose;
    let records = [Record {
        index: 4,
        name: "face".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::False,
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(1),
            Token::Ref(-1),
            Token::Ref(2),
            Token::False,
            Token::False,
        ]
        .into(),
        offset: 0,
        len: 0,
    }];
    let by_index = records.iter().map(|record| (4, record)).collect();
    let reach = Reachable {
        faces: HashSet::from([4]),
        ..Reachable::default()
    };
    let expected_side = sidedness(4);
    let expected_reference = expected_side.id();
    let needed = cadmpeg_core::decode::u64_from_index(expected_reference.len());
    let mut outputs = Vec::new();
    for purpose in [DecodePurpose::Model, DecodePurpose::History] {
        for limit in [0, needed - 1, needed] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut out = AsmBrep::default();
            let result = emit_faces(
                &ctx,
                &mut out,
                FaceInputs {
                    records: &records,
                    by_index: &by_index,
                    reach: &reach,
                    inward_normal_surfaces: &HashSet::new(),
                    format: crate::asm_format!("f3d"),
                    purpose,
                },
            );
            if purpose == DecodePurpose::History || limit == needed {
                result.unwrap();
                assert_eq!(
                    out.faces[0].native_ref.as_deref(),
                    Some("f3d:asm:face-sidedness#4")
                );
                assert_eq!(out.face_sidedness[0].record_index, 4);
                assert!(out.face_sidedness[0]
                    .owns_native_ref(out.faces[0].native_ref.as_deref(), &out.face_native_keys));
                assert!(out.face_native_keys.is_empty());
                assert_eq!(
                    ctx.retained_bytes_available(),
                    if purpose == DecodePurpose::History {
                        limit
                    } else {
                        0
                    }
                );
                assert!(ctx.resource_refusal().is_none());
                outputs.push(
                    serde_json::to_value((&out.faces, &out.face_native_keys, &out.face_sidedness))
                        .unwrap(),
                );
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
                assert!(out.faces.is_empty());
                assert!(out.face_sidedness.is_empty());
                assert!(out.face_native_keys.is_empty());
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(refusal)) =
            ctx.retain_bytes(1, "keyless sidedness caller prior refusal")
        else {
            panic!("caller fixture must refuse")
        };
        let mut out = AsmBrep::default();
        assert!(
            matches!(emit_faces(&ctx, &mut out, FaceInputs { records: &records, by_index: &by_index, reach: &reach, inward_normal_surfaces: &HashSet::new(), format: crate::asm_format!("f3d"), purpose }), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
        );
        assert!(out.faces.is_empty());
        assert!(out.face_sidedness.is_empty());
        assert!(out.face_native_keys.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    }
    assert_eq!(outputs.len(), 4);
    assert!(outputs.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn native_namespace_outer_id_rewrites_preserve_keyed_and_direct_inverse_links() {
    use cadmpeg_ir::ids::Identity;
    use cadmpeg_ir::native::NativeRecord;
    fn rewritten(source: &str) -> String {
        match source {
            "f3d:asm:face-native-key#4" => "f3d:independent-key/asm:face-native-key#4",
            "f3d:asm:face-sidedness#4" => "f3d:independent-side/asm:face-sidedness#4",
            other => other,
        }
        .to_owned()
    }
    for keyed in [false, true] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let key = FaceNativeKey {
            source_namespace: NativeRecordNamespace::new(crate::asm_format!("f3d")),
            record_index: 4,
            asm_face_key: Some(77),
        };
        let mut side = sidedness(4);
        let reference = if keyed {
            side.face_key = Some(key.id());
            key.id()
        } else {
            side.id()
        };
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.model.faces[0].native_ref = Some(reference.clone());
        let namespace = ir.native.namespace_mut("f3d");
        namespace
            .set_arena(&ctx, "face_sidedness", &[side])
            .unwrap();
        if keyed {
            namespace
                .set_arena(&ctx, "face_native_keys", &[key])
                .unwrap();
        }
        for record in namespace.arenas_mut().get_mut("face_sidedness").unwrap() {
            let id = Identity::new(rewritten(record.id())).unwrap();
            let fields = record
                .rewrite_fields::<FaceSidedness, _>(&ctx, |source: &str| Ok(rewritten(source)))
                .unwrap();
            *record = NativeRecord::new(id, fields).unwrap();
        }
        if keyed {
            for record in namespace.arenas_mut().get_mut("face_native_keys").unwrap() {
                let id = Identity::new(rewritten(record.id())).unwrap();
                let fields = record
                    .rewrite_fields::<FaceNativeKey, _>(&ctx, |source: &str| Ok(rewritten(source)))
                    .unwrap();
                *record = NativeRecord::new(id, fields).unwrap();
            }
        }
        let sides: Vec<FaceSidedness> = namespace.arena_as("face_sidedness").unwrap();
        let keys: Vec<FaceNativeKey> = namespace.arena_as("face_native_keys").unwrap();
        let reference = rewritten(&reference);
        ir.model.faces[0].native_ref = Some(reference);
        assert_eq!(sides[0].id(), namespace.arenas()["face_sidedness"][0].id());
        assert!(sides[0].owns_native_ref(ir.model.faces[0].native_ref.as_deref(), &keys));
        assert!(!sides[0].owns_native_ref(Some("f3d:asm:face-sidedness#4"), &keys));
        if keyed {
            assert_ne!(sides[0].source_namespace, keys[0].source_namespace);
            assert!(keys[0].owns_native_ref(ir.model.faces[0].native_ref.as_deref(), &sides));
            assert!(keys[0].owns_native_ref(Some(&sides[0].id()), &sides));
            assert_eq!(
                sides[0].face_key.as_deref(),
                Some(namespace.arenas()["face_native_keys"][0].id())
            );
        }
        // The typed identity contract already rejects an outer id that changes
        // kind/index; no new canonical-spelling refusal is introduced by #213.
        let fields = namespace.arenas()["face_sidedness"][0].fields().unwrap();
        namespace.arenas_mut().get_mut("face_sidedness").unwrap()[0] =
            NativeRecord::new(Identity::new("f3d:asm:other-kind#4").unwrap(), fields).unwrap();
        assert!(namespace
            .arena_as::<FaceSidedness>("face_sidedness")
            .is_err());
    }
}
