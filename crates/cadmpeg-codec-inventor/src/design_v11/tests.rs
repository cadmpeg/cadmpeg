// SPDX-License-Identifier: Apache-2.0
use crate::test_support::root_part_ir;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureId, SketchFeatureBinding};

use super::*;

fn feature(key: &str, ordinal: u64) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("inventor:design:feature#{key}")).expect("id"),
        ordinal,
        name: Some(key.to_owned()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(LegacyOperation::Sketch {
                sketch: SketchFeatureBinding::Unresolved,
            }),
        ),
        native_ref: Some(format!("inventor:pmdc:sketch#{key}")),
    }
}

fn property(feature: &mut LegacyFeature, key: &str, value: &str) {
    feature.source_properties.insert(
        NonBlankString::new(key.to_owned()).expect("key"),
        value.to_owned(),
    );
}

fn part(features: Vec<LegacyFeature>) -> CadIr {
    let mut ir = root_part_ir();
    ir.model.features = features;
    ir
}

fn project_part(ir: &mut CadIr) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let flags = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed.is_some())
        .map(|feature| {
            let suppressed = super::property(feature, "suppressed", "true");
            (
                feature.native_ref.clone().expect("native id"),
                if suppressed {
                    crate::feature::history::SUPPRESSED_FLAG
                } else {
                    0
                },
            )
        })
        .collect();
    project_history(&ctx, ir, &flags).expect("projection");
}

fn timeline(ir: &CadIr) -> (&Vec<Member>, &Rollback) {
    let root = &ir.design.scopes[0].roots[0];
    let Container::Ordered {
        members,
        rollback,
        kind: OrderedKind::Timeline,
        ..
    } = ir
        .design
        .containers
        .iter()
        .find(|c| c.id() == root)
        .expect("root")
    else {
        panic!("timeline")
    };
    (members, rollback)
}

#[test]
fn browser_order_and_identity_survive_arena_sorting() {
    let a = feature("a", 2);
    let z = feature("z", 1);
    let mut ir = part(vec![a.clone(), z.clone()]);
    let original = ir.model.clone();
    project_part(&mut ir);
    assert_eq!(
        timeline(&ir).0,
        &[Member::Feature(z.id.clone()), Member::Feature(a.id.clone())]
    );
    assert_eq!(ir.design.features[0].id, a.id);
    assert_eq!(ir.design.features[0].name, a.name);
    assert_eq!(ir.design.features[0].native_ref, a.native_ref);
    assert_eq!(ir.model, original);
    let mut repeated = part(vec![z, a]);
    project_part(&mut repeated);
    assert_eq!(ir.design, repeated.design);
}

#[test]
fn rollback_uses_the_first_member_below_end_of_part() {
    for below in 0..=3 {
        let mut features = (0..3)
            .map(|i| feature(&i.to_string(), i))
            .collect::<Vec<_>>();
        for feature in features.iter_mut().skip(below) {
            feature.suppressed = Some(true);
            property(feature, "end_of_part", "below");
        }
        let mut ir = part(features.clone());
        project_part(&mut ir);
        let expected = features.get(below).map_or(Rollback::End, |f| {
            Rollback::Before(Member::Feature(f.id.clone()))
        });
        assert_eq!(timeline(&ir).1, &expected);
        let expected_at = below.checked_sub(1).map_or_else(
            || Position::Start(ir.design.scopes[0].roots[0].clone()),
            |i| Position::After(Member::Feature(features[i].id.clone())),
        );
        assert_eq!(ir.design.scopes[0].current[0].at, expected_at);
        assert!(ir
            .design
            .features
            .iter()
            .all(|f| f.suppressed == Some(Dimension::Literal(false))));
    }
}

#[test]
fn source_suppression_keeps_rollback_at_end() {
    let mut suppressed = feature("suppressed", 0);
    suppressed.suppressed = Some(true);
    property(&mut suppressed, "suppressed", "true");
    let mut unknown = feature("unknown", 1);
    unknown.suppressed = None;
    let mut ir = part(vec![suppressed, unknown]);
    project_part(&mut ir);
    assert_eq!(timeline(&ir).1, &Rollback::End);
    assert_eq!(
        ir.design.features[0].suppressed,
        Some(Dimension::Literal(true))
    );
    assert_eq!(ir.design.features[1].suppressed, None);
}

#[test]
fn current_state_lists_every_owned_geometry_body() {
    let mut ir = part(Vec::new());
    for key in ["a", "b", "other"] {
        ir.geometry.bodies.push(
            serde_json::from_value(
                serde_json::json!({"id":format!("inventor:brep:body#{key}"),"kind":"solid"}),
            )
            .expect("body"),
        );
    }
    ir.model.product_definitions[0].bodies = ir.geometry.bodies[..2]
        .iter()
        .map(|b| b.id.clone())
        .collect();
    project_part(&mut ir);
    let current = &ir.design.scopes[0].current[0];
    assert_eq!(current.bodies, ir.model.product_definitions[0].bodies);
    assert_eq!(current.status, EvaluationStatus::Current);
    assert_eq!(
        current.at,
        Position::Start(ir.design.scopes[0].roots[0].clone())
    );
    assert!(ir.design.evaluations.is_empty());
}

#[test]
fn flat_pattern_has_a_second_root_timeline() {
    let mut flat = feature("flat", 2);
    property(&mut flat, "history", "flat_pattern");
    let mut model = feature("flat-model", 3);
    model.source_tag = Some("flat_pattern".to_owned());
    let mut ir = part(vec![feature("folded", 1), flat.clone(), model.clone()]);
    project_part(&mut ir);
    assert_eq!(ir.design.scopes[0].roots.len(), 2);
    let id = &ir.design.scopes[0].roots[1];
    let Container::Ordered {
        members, rollback, ..
    } = ir
        .design
        .containers
        .iter()
        .find(|c| c.id() == id)
        .expect("flat")
    else {
        panic!("timeline")
    };
    assert_eq!(
        members,
        &[Member::Feature(flat.id), Member::Feature(model.id)]
    );
    assert_eq!(rollback, &Rollback::End);
    assert_eq!(ir.design.scopes[0].current.len(), 1);
}

#[test]
fn operations_keep_the_family_and_native_class() {
    assert_eq!(
        operation(&LegacyOperation::Unresolved {
            family: UnresolvedFamily::Extrude
        })
        .expect("family"),
        FeatureOperation::Unresolved {
            family: OperationFamily::Extrude
        }
    );
    let native_operation = LegacyOperation::Native {
        kind: cadmpeg_ir::features::NativeFeatureKind::Other("CustomClass".to_owned()),
        parameters: BTreeMap::default(),
    };
    assert_eq!(
        operation(&native_operation).expect("native"),
        native("CustomClass").expect("native")
    );
}

#[test]
fn native_header_flags_supply_suppression_for_unplaced_features() {
    let mut raw = crate::feature::tests::test_feature(7, 0, &[]);
    raw.edit(|payload| payload.header.flags = crate::feature::history::SUPPRESSED_FLAG);
    let mut unplaced = feature("unplaced", 0);
    unplaced.native_ref = Some(raw.id());
    unplaced.suppressed = None;
    let mut ir = part(vec![unplaced]);
    let mut inventory = crate::feature::FeatureInventory::default();
    inventory.features.push(raw);
    let sketches = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &inventory, &sketches, &[], &[], &mut losses).expect("projection");
    assert_eq!(
        ir.design.features[0].suppressed,
        Some(Dimension::Literal(true))
    );
    assert_eq!(timeline(&ir).1, &Rollback::End);
}
