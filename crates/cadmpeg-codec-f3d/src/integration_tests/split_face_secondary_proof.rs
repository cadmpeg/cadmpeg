// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::face::DesignFaceOperand;
use crate::test_support::{indexed_header, lp_ascii};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const FRAME_FINDING: &str = "Fusion Design face operand has an invalid scope or recipe frame";

fn fixture_scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope =
        DesignParameterScope::empty(&format!("{STREAM}:design-parameter-scope#50"), kind, 50);
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn header(index: u32, offset: u64) -> DesignRecordHeader {
    DesignRecordHeader {
        id: format!("{STREAM}:design-record-header#{offset}"),
        record_index: index,
        byte_offset: offset,
        class_tag: "256".to_owned().try_into().unwrap(),
    }
}

fn decoded(
    trailing_primary: bool,
) -> (DesignParameterScope, ConstructionRecipe, DesignFaceOperand) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"256", 100);
    indexed_header(&mut bytes, *b"257", 100);
    for index in 101..=103 {
        indexed_header(&mut bytes, *b"256", index);
    }
    lp_ascii(&mut bytes, "face_recipe_data");
    for word in [0i32, -1, 4] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"257", 103);
    if trailing_primary {
        indexed_header(&mut bytes, *b"256", 104);
    }
    assert_eq!(bytes.len(), if trailing_primary { 109 } else { 98 });
    let primary = [(100, 0), (101, 22), (102, 33), (103, 44), (104, 98)];
    let meta = crate::test_support::split_face::metadata(
        &[
            (
                "00000000-0000-0000-0000-000000000001",
                1,
                &[100, 101, 102, 103, 104],
            ),
            ("00000000-0000-0000-0000-000000000002", 1, &[]),
        ],
        &primary[..if trailing_primary { 5 } else { 4 }],
        &[(100, 11), (103, 87)],
    );
    let recipe = ConstructionRecipe {
        id: format!("{STREAM}:construction-recipe#59"),
        byte_offset: 59,
        kind: ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let scope = fixture_scope(DesignFeatureKind::SplitFace);
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &meta);
    let operands = crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::decode::operands::face_operands::decode_face_operands(
                ctx,
                scan,
                std::slice::from_ref(&scope),
                &[],
                &[header(100, 0), header(103, 44), header(104, 98)],
                std::slice::from_ref(&recipe),
                &[],
            )
            .unwrap()
            .0
        })
    });
    let [operand] = <[_; 1]>::try_from(operands).unwrap();
    assert_eq!(operand.recipe_record_index(), 103);
    assert_eq!(operand.recipe_record_byte_offset(), 44);
    assert_eq!(operand.next_record_index, 103);
    assert_eq!(operand.next_byte_offset(), 87);
    assert_eq!(operand.recipe_program, [0, -1, 4]);
    (scope, recipe, operand)
}

fn frame_findings(
    scope: DesignParameterScope,
    recipe: ConstructionRecipe,
    operand: DesignFaceOperand,
    headers: Vec<DesignRecordHeader>,
) -> usize {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.fragment = true;
    {
        let mut native = crate::test_support::native_test::f3d_native_mut(&mut ir);
        native.design_parameter_scopes = vec![scope];
        native.design_record_headers = headers;
        native.construction_recipes = vec![recipe];
        native.design_face_operands = vec![operand];
    }
    let loaded = crate::test_support::native_test::f3d_native(&ir);
    assert_eq!(loaded.design_face_operands.len(), 1);
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    assert!(findings
        .iter()
        .all(|finding| !finding.message.starts_with("skipped native arena row:")));
    findings
        .iter()
        .filter(|finding| finding.message == FRAME_FINDING)
        .count()
}

#[test]
fn decoded_secondary_recipe_frame_survives_native_admission_and_serde() {
    let (scope, recipe, operand) = decoded(true);
    let wire = json::to_value(operand.clone()).unwrap();
    assert_eq!(
        wire["registered_recipe_frame"],
        serde_json::json!({
            "record_index": 103, "primary_offset": 44, "secondary_offset": 87, "end_offset": 98,
            "next_primary_index": 104
        })
    );
    let restored: DesignFaceOperand = json::from_value(wire.clone()).unwrap();
    assert_eq!(restored, operand);
    assert_eq!(json::to_value(restored.clone()).unwrap(), wire);
    let rewritten = crate::test_support::with_decode_context(|ctx| {
        let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()));
        restored.clone().rewrite_identities(ctx, &mut map).unwrap()
    });
    assert_eq!(json::to_value(rewritten).unwrap(), wire);
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            restored,
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        0
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(104, 98)]
        ),
        0
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 44), header(104, 90)]
        ),
        1
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 44), header(104, 87)]
        ),
        1
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 45), header(104, 98)]
        ),
        1
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 44), header(104, 109)]
        ),
        1
    );
    assert_eq!(
        frame_findings(
            fixture_scope(DesignFeatureKind::OffsetFaces),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        1
    );
    let mut forged_end = wire.clone();
    forged_end["registered_recipe_frame"]["next_primary_index"] = serde_json::Value::Null;
    let forged_end: DesignFaceOperand = json::from_value(forged_end).unwrap();
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            forged_end,
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        1
    );
    let mut forged_later = wire.clone();
    forged_later["registered_recipe_frame"]["next_primary_index"] = 105.into();
    let forged_later: DesignFaceOperand = json::from_value(forged_later).unwrap();
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            forged_later,
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        1
    );
    let mut generic = wire.clone();
    generic
        .as_object_mut()
        .unwrap()
        .remove("registered_recipe_frame");
    let generic: DesignFaceOperand = json::from_value(generic).unwrap();
    assert_eq!(
        frame_findings(
            scope,
            recipe,
            generic,
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        1
    );
    for (field, value) in [
        ("record_index", 102),
        ("primary_offset", 45),
        ("secondary_offset", 88),
        ("end_offset", 97),
        ("next_primary_index", 103),
    ] {
        let mut forged = wire.clone();
        forged["registered_recipe_frame"][field] = value.into();
        assert!(json::from_value::<DesignFaceOperand>(forged).is_err());
    }
    let mut forged = wire;
    forged["next_byte_offset"] = 88.into();
    assert!(json::from_value::<DesignFaceOperand>(forged).is_err());
}

#[test]
fn decoded_secondary_recipe_frame_can_end_at_stream_end() {
    let (scope, recipe, operand) = decoded(false);
    let wire = json::to_value(&operand).unwrap();
    assert_eq!(
        wire["registered_recipe_frame"]["next_primary_index"],
        serde_json::Value::Null
    );
    assert_eq!(
        frame_findings(
            scope.clone(),
            recipe.clone(),
            operand.clone(),
            vec![header(100, 0), header(103, 44)]
        ),
        0
    );
    assert_eq!(
        frame_findings(
            scope,
            recipe,
            operand,
            vec![header(100, 0), header(103, 44), header(104, 98)]
        ),
        1
    );
}

#[test]
fn registered_secondary_recipe_frame_rejects_conflicting_duplicate_headers() {
    let (scope, recipe, operand) = decoded(true);
    for (index, offset) in [(103, 88), (103, 109), (104, 88)] {
        for conflict_first in [true, false] {
            let mut headers = vec![header(100, 0), header(103, 44), header(104, 98)];
            let conflict = header(index, offset);
            if conflict_first {
                headers.insert(0, conflict);
            } else {
                headers.push(conflict);
            }
            assert_eq!(
                frame_findings(scope.clone(), recipe.clone(), operand.clone(), headers),
                1
            );
        }
    }
    let mut conflict = header(105, 44);
    conflict.id = format!("{STREAM}:design-record-header#444");
    assert_eq!(
        frame_findings(
            scope,
            recipe,
            operand,
            vec![header(100, 0), conflict, header(103, 44), header(104, 98)]
        ),
        1
    );
}

#[test]
fn registered_secondary_recipe_frame_admits_consistent_paired_header_claims() {
    let (scope, recipe, operand) = decoded(true);
    for secondary_first in [true, false] {
        let mut headers = vec![header(100, 0), header(103, 44), header(104, 98)];
        let mut secondary = header(103, 87);
        secondary.class_tag = "257".to_owned().try_into().unwrap();
        headers.insert(if secondary_first { 1 } else { 2 }, secondary);
        let mut future_secondary = header(104, 109);
        future_secondary.class_tag = "257".to_owned().try_into().unwrap();
        headers.push(future_secondary);
        assert_eq!(
            frame_findings(scope.clone(), recipe.clone(), operand.clone(), headers),
            0
        );
    }
}
