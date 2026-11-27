// SPDX-License-Identifier: Apache-2.0

use super::ReplaceFaceOperands;
use crate::features::selections::FaceSelection;
use cadmpeg_core::serde_erase::json;

#[test]
fn optional_tangent_chain_preserves_legacy_wire_forms_and_refuses_null() {
    for tangent_chain in [None, Some(false), Some(true)] {
        let operands = ReplaceFaceOperands::new(
            FaceSelection::Native("native:target".into()),
            FaceSelection::Native("native:replacement".into()),
        )
        .unwrap()
        .with_tangent_chain(tangent_chain);
        let mut expected = cadmpeg_core::json!({
            "targets": {"kind": "native", "value": "native:target"},
            "replacements": {"kind": "native", "value": "native:replacement"}
        });
        if let Some(value) = tangent_chain {
            expected["tangent_chain"] = cadmpeg_core::json!(value);
        }
        let wire = json::to_value(&operands).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(
            json::from_value::<ReplaceFaceOperands>(wire.clone()).unwrap(),
            operands
        );
        let mut unknown = wire.clone();
        unknown["unknown"] = cadmpeg_core::json!(true);
        assert!(json::from_value::<ReplaceFaceOperands>(unknown).is_err());
        let mut null = wire;
        null["tangent_chain"] = serde_json::Value::Null;
        assert!(json::from_value::<ReplaceFaceOperands>(null).is_err());
    }
}

#[test]
fn admitted_edits_retain_tangent_chain_and_refused_edits_retain_both_selections() {
    let first = FaceSelection::Faces(vec![
        crate::ids::FaceId::mint("synthetic:test:face#1").unwrap()
    ]);
    let second = FaceSelection::Faces(vec![
        crate::ids::FaceId::mint("synthetic:test:face#2").unwrap()
    ]);
    let third = FaceSelection::Faces(vec![
        crate::ids::FaceId::mint("synthetic:test:face#3").unwrap()
    ]);
    for tangent_chain in [None, Some(false), Some(true)] {
        let mut operands = ReplaceFaceOperands::new(first.clone(), second.clone())
            .unwrap()
            .with_tangent_chain(tangent_chain);
        operands
            .try_edit(|targets, _| *targets = third.clone())
            .unwrap();
        assert_eq!(operands.targets(), &third);
        assert_eq!(operands.replacements(), &second);
        assert_eq!(operands.tangent_chain(), tangent_chain);
        let admitted = operands.clone();
        assert!(operands
            .try_edit(|targets, replacements| *replacements = targets.clone())
            .is_err());
        assert_eq!(operands, admitted);
    }
}

#[test]
fn identity_rewriting_keeps_tangent_chain_and_rewrites_both_face_selections() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operands = ReplaceFaceOperands::new(
        FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("synthetic:before:face#1").unwrap()
        ]),
        FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("synthetic:before:face#2").unwrap()
        ]),
    )
    .unwrap()
    .with_tangent_chain(Some(true));
    let mut map = IdentityMap::new(|source: &str| Ok(source.replace(":before:", ":after:")));
    let rewritten = operands.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(rewritten.tangent_chain(), Some(true));
    assert_eq!(
        rewritten.targets(),
        &FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("synthetic:after:face#1").unwrap()
        ])
    );
    assert_eq!(
        rewritten.replacements(),
        &FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("synthetic:after:face#2").unwrap()
        ])
    );
}
