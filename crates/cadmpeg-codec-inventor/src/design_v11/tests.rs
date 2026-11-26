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
    project_history(&ctx, ir, &flags, false).expect("projection");
    place_current_bodies(ir).expect("placement");
}

fn timeline(ir: &CadIr) -> (&Vec<Member>, Option<&Rollback>) {
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
    (members, rollback.as_ref())
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
        assert_eq!(timeline(&ir).1, Some(&expected));
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
    assert_eq!(timeline(&ir).1, Some(&Rollback::End));
    assert_eq!(
        ir.design.features[0].suppressed,
        Some(Dimension::Literal(true))
    );
    assert_eq!(ir.design.features[1].suppressed, None);
}

#[test]
fn geometry_without_design_records_keeps_no_scope() {
    let mut ir = part(Vec::new());
    for key in ["a", "b", "other"] {
        ir.geometry.bodies.push(
            cadmpeg_core::serde_erase::json::from_value(
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
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert_eq!(ir.model.product_definitions[0].bodies.len(), 2);
    assert_eq!(ir.geometry.bodies.len(), 3);
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
    assert_eq!(rollback, &Some(Rollback::End));
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
    assert_eq!(timeline(&ir).1, Some(&Rollback::End));
}

#[test]
fn block_definitions_are_members_of_the_unordered_blocks_group() {
    let mut block = feature("block", 0);
    block.evaluation =
        FeatureEvaluation::from_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            LegacyOperation::SketchBlockDefinition { sketch: None },
        ));
    let ordinary = feature("ordinary", 1);
    let mut ir = part(vec![block.clone(), ordinary.clone()]);
    project_part(&mut ir);
    assert_eq!(timeline(&ir).0, &[Member::Feature(ordinary.id)]);
    let group = ir
        .design
        .containers
        .iter()
        .find_map(|container| match container {
            Container::Unordered {
                kind: UnorderedKind::Group,
                members,
                ..
            } => Some(members),
            _ => None,
        })
        .expect("Blocks group");
    assert_eq!(group, &BTreeSet::from([Member::Feature(block.id)]));
}

fn extrude(key: &str, ordinal: u64) -> LegacyFeature {
    let mut extrude = feature(key, ordinal);
    extrude.evaluation = FeatureEvaluation::from_definition(
        cadmpeg_ir::features::FeatureDefinition::Operation(LegacyOperation::Unresolved {
            family: UnresolvedFamily::Extrude,
        }),
    );
    extrude
}

fn with_bodies(mut ir: CadIr) -> CadIr {
    for key in ["b", "a", "other"] {
        ir.geometry.bodies.push(
            cadmpeg_core::serde_erase::json::from_value(
                serde_json::json!({"id":format!("inventor:brep:body#{key}"),"kind":"solid"}),
            )
            .expect("body"),
        );
    }
    ir.model.product_definitions[0].bodies = ir.geometry.bodies[..2]
        .iter()
        .map(|b| b.id.clone())
        .collect();
    ir
}

fn owned(ir: &CadIr) -> Vec<cadmpeg_ir::ids::BodyId> {
    let mut owned = ir.model.product_definitions[0].bodies.clone();
    owned.sort();
    owned
}

#[test]
fn a_state_changing_member_produces_every_owned_body() {
    let mut ir = with_bodies(part(vec![feature("sketch", 0), extrude("extrude", 1)]));
    project_part(&mut ir);
    let scope = &ir.design.scopes[0];
    assert_eq!(scope.current[0].bodies, owned(&ir));
    assert!(scope.unproduced.is_empty());
    assert_eq!(unproduced_bodies(&ir), 0);
}

#[test]
fn stateless_members_leave_owned_bodies_unproduced() {
    let mut block = feature("block", 1);
    block.evaluation =
        FeatureEvaluation::from_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            LegacyOperation::SketchBlockDefinition { sketch: None },
        ));
    let mut ir = with_bodies(part(vec![feature("sketch", 0), block]));
    project_part(&mut ir);
    let scope = &ir.design.scopes[0];
    assert!(scope.current[0].bodies.is_empty());
    assert_eq!(
        scope.unproduced.iter().cloned().collect::<Vec<_>>(),
        owned(&ir)
    );
    assert_eq!(unproduced_bodies(&ir), 2);
}

#[test]
fn members_after_the_rollback_or_suppressed_produce_no_current_body() {
    let mut below = extrude("below", 1);
    property(&mut below, "end_of_part", "below");
    let mut suppressed = extrude("suppressed", 1);
    property(&mut suppressed, "suppressed", "true");
    for second in [below, suppressed] {
        let mut ir = with_bodies(part(vec![feature("sketch", 0), second]));
        project_part(&mut ir);
        let scope = &ir.design.scopes[0];
        assert!(scope.current[0].bodies.is_empty());
        assert_eq!(scope.unproduced.len(), 2);
    }
}

#[test]
fn timeline_production_reads_only_ordered_containers() {
    let ir = part(Vec::new());
    let group = Container::Unordered {
        id: ContainerId::mint("inventor:document:group#blocks").expect("id"),
        kind: UnorderedKind::Group,
        members: BTreeSet::new(),
    };
    assert!(!timeline_produces(&ir, &group).expect("group"));
}

#[test]
fn native_design_validation_uses_the_decoder_projection() {
    let mut ir = part(vec![feature("sketch", 0)]);
    let inventory = crate::feature::FeatureInventory::default();
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
    project(
        &ctx,
        &mut ir,
        &inventory,
        &sketches,
        &[],
        &[],
        &mut Vec::new(),
    )
    .expect("projection");
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
        .set_arena(
            &ctx,
            "pm_dc_features",
            Vec::<crate::feature::PmDcFeature>::new().as_slice(),
        )
        .expect("native arena");
    assert!(validate_from_native(&ctx, &ir)
        .expect("validation")
        .is_empty());
    ir.design.features[0].name = Some("changed".into());
    let findings = validate_from_native(&ctx, &ir).expect("validation");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert!(findings[0].message.contains(".features[0].name"));
}

#[test]
fn native_operation_keeps_all_source_parameter_strings() {
    let parameters = BTreeMap::from([
        (
            cadmpeg_core::nonblank_literal!("control"),
            "false".to_owned(),
        ),
        (cadmpeg_core::nonblank_literal!("empty"), String::new()),
    ]);
    let target = operation(&LegacyOperation::Native {
        kind: cadmpeg_ir::features::NativeFeatureKind::Other("synthetic".into()),
        parameters: parameters.clone(),
    })
    .expect("operation");
    let FeatureOperation::Native {
        kind,
        native_options,
        ..
    } = target
    else {
        panic!("native operation");
    };
    assert_eq!(kind.as_str(), "inventor:synthetic");
    let cadmpeg_ir::design::options::NativeOption::Text(Dimension::Literal(text)) =
        &native_options["inventor:parameters"]
    else {
        panic!("native parameter map");
    };
    let restored: BTreeMap<NonBlankString, String> =
        serde_json::from_str(text.as_str()).expect("parameter strings");
    assert_eq!(restored, parameters);
}

#[test]
fn current_body_membership_survives_geometry_arena_sorting() {
    let mut ir = part(vec![feature("producer", 0)]);
    for name in ["b", "a"] {
        let body: cadmpeg_ir::topology::Body =
            cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
                "id":format!("inventor:brep:body#{name}"), "kind":"solid"
            }))
            .expect("body");
        ir.model.product_definitions[0].bodies.push(body.id.clone());
        ir.geometry.bodies.push(body);
    }
    ir.model.features[0].evaluation = FeatureEvaluation::new(
        cadmpeg_ir::features::FeatureDefinition::Operation(LegacyOperation::StoredGeometry {}),
        ir.geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect(),
    );
    project_part(&mut ir);
    let original = ir.design.clone();
    assert_eq!(
        original.scopes[0].current[0].bodies,
        [
            cadmpeg_ir::ids::BodyId::mint("inventor:brep:body#a").expect("id"),
            cadmpeg_ir::ids::BodyId::mint("inventor:brep:body#b").expect("id"),
        ]
    );
    ir.geometry.finalize();
    ir.design = cadmpeg_ir::document::Design::default();
    project_part(&mut ir);
    assert_eq!(ir.design, original);
}

pub(super) fn validate_from_native(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    crate::validate::design_v11_findings_from_arenas(ctx, ir)
}

fn state_findings(ir: &CadIr) -> usize {
    validate_states(ir, &DecodePolicy::service().limits).len()
}

#[test]
fn state_validation_accepts_the_decoder_placement() {
    for features in [
        vec![feature("sketch", 0), extrude("extrude", 1)],
        vec![feature("sketch", 0)],
    ] {
        let mut ir = with_bodies(part(features));
        project_part(&mut ir);
        assert_eq!(state_findings(&ir), 0);
    }
}

#[test]
fn native_validation_rejects_bodies_the_predicate_places_elsewhere() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let findings = |ir: &CadIr| validate_from_native(&ctx, ir).expect("validation").len();
    let decode = |ir: &mut CadIr| {
        let sketches = crate::sketch::SketchInventory {
            sketches: Vec::new(),
            entities: Vec::new(),
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            text_members: Vec::new(),
            issues: Vec::new(),
        };
        project(
            &ctx,
            ir,
            &crate::feature::FeatureInventory::default(),
            &sketches,
            &[],
            &[],
            &mut Vec::new(),
        )
        .expect("projection");
    };

    let mut ir = with_bodies(part(vec![feature("sketch", 0), extrude("extrude", 1)]));
    decode(&mut ir);
    assert_eq!(findings(&ir), 0);
    let body = ir.design.scopes[0].current[0].bodies.pop().expect("body");
    ir.design.scopes[0].unproduced.insert(body);
    assert_eq!(findings(&ir), 1);

    let mut ir = with_bodies(part(vec![feature("sketch", 0)]));
    decode(&mut ir);
    assert_eq!(findings(&ir), 0);
    let scope = &mut ir.design.scopes[0];
    let body = scope.unproduced.pop_first().expect("body");
    scope.current[0].bodies.push(body);
    assert_eq!(findings(&ir), 1);
}

#[test]
fn state_validation_rejects_an_extent_with_another_owner() {
    let mut ir = with_bodies(part(vec![feature("sketch", 0), extrude("extrude", 1)]));
    project_part(&mut ir);
    let other = ir.geometry.bodies[2].id.clone();
    let mut definition = ir.model.product_definitions[0].clone();
    definition.id = cadmpeg_ir::ids::ProductDefinitionId::mint("inventor:product:definition#other")
        .expect("definition");
    definition.bodies = vec![other.clone()];
    definition.occurrences = Vec::default();
    ir.model.product_definitions.push(definition);
    ir.design.scopes[0].current[0].bodies.push(other);
    assert_eq!(state_findings(&ir), 1);
}

#[test]
fn state_validation_rejects_an_unresolved_evaluation() {
    let mut ir = with_bodies(part(vec![feature("sketch", 0), extrude("extrude", 1)]));
    project_part(&mut ir);
    let id = cadmpeg_ir::design::ids::EvaluationId::mint("inventor:test:evaluation#a").expect("id");
    ir.design
        .evaluations
        .push(cadmpeg_ir::design::evaluation::Evaluation {
            id: id.clone(),
            scope: ir.design.scopes[0].definition.clone(),
            configuration: None,
            of: cadmpeg_ir::design::refs::Evaluated::State(Position::After(Member::Feature(
                FeatureId::mint("inventor:design:feature#extrude").expect("id"),
            ))),
            status: EvaluationStatus::Current,
            content: Some(cadmpeg_ir::design::evaluation::StateContent {
                base: cadmpeg_ir::design::evaluation::StateBase::Evaluation(id),
                delta: cadmpeg_ir::design::evaluation::StateDelta::default(),
            }),
            native_ref: None,
        });
    assert_eq!(state_findings(&ir), 1);
}
