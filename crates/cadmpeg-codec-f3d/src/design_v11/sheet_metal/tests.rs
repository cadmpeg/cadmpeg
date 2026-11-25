// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::{
    scope::{DesignFeatureKind, DesignScopePayloadMut},
    sheet_metal::DesignBaseFlangeOperation,
};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, OperationFamily};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureId};
use cadmpeg_ir::scalar::PositiveReal;

const STREAM: &str = "f3d:test/Design1/BulkStream.dat";

fn fixture(ctx: &DecodeContext<'_>, rule_thickness: Option<f64>) -> CadIr {
    let mut ir = CadIr::empty();
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#0"),
        DesignFeatureKind::BaseFlange,
        10,
    );
    crate::design::test_support::name_scope_references(&mut scope, [100, 101, 102, 103]);
    let DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() else {
        panic!("base flange payload");
    };
    slot.get_or_insert_with(Default::default)
        .base_flange_operation = Some(DesignBaseFlangeOperation {
        thickness: PositiveReal::new(0.2).unwrap(),
        thickness_offset: 123,
        profile_group_record_index: 100,
        profile_record_index: 101,
        thickness_record_index: 102,
        settings_record_index: 103,
    });
    let id = FeatureId::mint("f3d:model:feature#base:10").unwrap();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: id.clone(),
        ordinal: 0,
        name: Some("BaseFlange".into()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                LegacyOperation::SheetMetalBaseFlange {
                    profile: PlanarProfileRef::Sketch(
                        cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#1").unwrap(),
                    ),
                    thickness: PositiveLength::new(2.0),
                    side: Some(SheetMetalThicknessSide::Forward),
                },
            ),
        ),
        native_ref: Some(scope.id.clone()),
    });
    ir.design.features.push(Feature {
        id,
        name: Some("BaseFlange".into()),
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::SheetMetal,
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: Some(scope.id.clone()),
    });
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(ctx, "design_parameter_scopes", &[scope])
        .unwrap();
    if let Some(value) = rule_thickness {
        let settings: DesignSheetMetalSettings = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
            "id": format!("{STREAM}:design-sheet-metal-settings#300"),
            "record_index": 103, "byte_offset": 300, "paired_byte_offset": 400, "mask": 0,
            "inputs": {"version": "v2", "slots": [null, null, null, null, null, null, null]},
            "rule_record_index": 104, "presentation_record_index": 105, "owner_record_index": 10, "context_record_index": 106,
            "rule_thickness": value, "rule_thickness_offset": 850,
        })).unwrap();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(ctx, "design_sheet_metal_settings", &[settings])
            .unwrap();
    }
    ir
}

#[test]
fn base_flange_wall_keeps_the_governing_rule_and_profile_identity() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, Some(0.125));
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        let mut losses = Vec::new();
        project(ctx, &mut ir, &definitions, &mut losses).unwrap();
        let FeatureOperation::SheetMetal(sheet) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet-metal wall");
        };
        let SheetMetalKind::Wall {
            profile,
            thickness,
            side,
            rule,
        } = &sheet.kind
        else {
            panic!("wall");
        };
        assert_eq!(*thickness, None);
        assert_eq!(*side, Some(NormalSide::Along));
        let [governing] = ir.design.sheet_metal_rules.as_slice() else {
            panic!("one rule");
        };
        assert_eq!(rule.as_ref(), Some(&governing.id));
        assert_eq!(governing.scope, crate::tree::file_root_id());
        assert_eq!(
            governing.thickness,
            PositiveLength::new(1.25).map(Dimension::Literal)
        );
        assert!(ir
            .native
            .namespace("f3d")
            .unwrap()
            .arenas()
            .values()
            .flatten()
            .any(|record| Some(record.id()) == governing.native_ref.as_deref()));
        assert_eq!(governing.bend.radius, None);
        assert_eq!(governing.allowance, None);
        let Picks::Named(picks) = &profile.picks else {
            panic!("profile identity");
        };
        assert_eq!(
            picks.as_slice()[0].native.as_ref().unwrap().value,
            NativeNameValue::Text(NonBlankString::new("f3d:model:sketch#1").unwrap())
        );
        assert_eq!(
            losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [
                F3dLossCode::SheetMetalRuleControlsNative.kind(),
                F3dLossCode::SheetMetalProfileNative.kind()
            ]
        );
    });
}

#[test]
fn base_flange_without_readable_settings_keeps_its_decoded_operands() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, None);
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
        let FeatureOperation::SheetMetal(sheet) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet-metal wall");
        };
        assert!(
            matches!(&sheet.kind, SheetMetalKind::Wall { thickness: Some(Dimension::Literal(value)), side: Some(NormalSide::Along), rule: None, .. } if value.get() == 2.0)
        );
        assert!(ir.design.sheet_metal_rules.is_empty());
    });
}

#[test]
fn same_numbered_settings_in_another_stream_preserve_the_scope_thickness() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, Some(0.125));
        let settings = ir
            .native
            .namespace("f3d")
            .unwrap()
            .arena_as::<DesignSheetMetalSettings>("design_sheet_metal_settings")
            .unwrap();
        let mut value = cadmpeg_core::serde_erase::json::to_value(&settings[0]).unwrap();
        value["id"] =
            serde_json::json!("f3d:other/Design1/BulkStream.dat:design-sheet-metal-settings#300");
        let foreign: DesignSheetMetalSettings =
            cadmpeg_core::serde_erase::json::from_value(value).unwrap();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(ctx, "design_sheet_metal_settings", &[foreign])
            .unwrap();
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
        let FeatureOperation::SheetMetal(sheet) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet-metal wall");
        };
        assert!(
            matches!(&sheet.kind, SheetMetalKind::Wall { thickness: Some(Dimension::Literal(value)), rule: None, .. } if value.get() == 2.0)
        );
        assert!(ir.design.sheet_metal_rules.is_empty());
    });
}

#[test]
fn base_flange_profile_binds_its_preceding_sketch_feature() {
    for selected in [false, true] {
        crate::test_support::with_decode_context(|ctx| {
            let mut ir = fixture(ctx, Some(0.125));
            let native = ir.model.features[0].native_ref.clone().unwrap();
            if selected {
                ir.model.features[0].evaluation.edit(|definition, _| {
                    let cadmpeg_ir::features::FeatureDefinition::Operation(
                        LegacyOperation::SheetMetalBaseFlange { profile, .. },
                    ) = definition
                    else {
                        panic!("base flange");
                    };
                    *profile = PlanarProfileRef::SketchSelection {
                        sketch: cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#1").unwrap(),
                        selections: cadmpeg_ir::features::selections::NativeSelections::try_from(
                            vec![native.clone()],
                        )
                        .unwrap(),
                    };
                });
            }
            ir.model.features[0].ordinal = 1;
            let mut owner = ir.model.features[0].clone();
            owner.id = FeatureId::mint("f3d:model:feature#sketch:20").unwrap();
            owner.ordinal = 0;
            owner.native_ref = Some(format!("{STREAM}:design-parameter-scope#20"));
            owner.evaluation = FeatureEvaluation::from_definition(
                cadmpeg_ir::features::FeatureDefinition::Operation(LegacyOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                        cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#1").unwrap(),
                    )),
                }),
            );
            let mut projected_owner = ir.design.features[0].clone();
            projected_owner.id = owner.id.clone();
            projected_owner.definition.operation = FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            };
            ir.design.features.insert(0, projected_owner);
            ir.model.features.push(owner.clone());
            let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
            project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
            let FeatureOperation::SheetMetal(sheet) = &ir.design.features[1].definition.operation
            else {
                panic!("sheet-metal wall");
            };
            let SheetMetalKind::Wall { profile, .. } = &sheet.kind else {
                panic!("wall");
            };
            let Picks::Named(picks) = &profile.picks else {
                panic!("profile");
            };
            assert_eq!(
                picks.as_slice()[0].target,
                if selected {
                    PickTarget::Native {
                        within: Some(NativeOwner::Feature(owner.id)),
                    }
                } else {
                    PickTarget::FeatureResult(owner.id)
                }
            );
            assert_eq!(
                profile
                    .native
                    .as_ref()
                    .map(cadmpeg_ir::features::selections::NativeSelections::as_slice),
                selected.then_some([native].as_slice())
            );
        });
    }
}

#[test]
fn native_region_profiles_keep_owned_rules_and_unresolved_sides() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, Some(0.125));
        let id = ir.model.features[0].native_ref.clone().unwrap();
        let mut scope = DesignParameterScope::empty(&id, DesignFeatureKind::Face, 10);
        crate::design::test_support::name_scope_references(&mut scope, [100, 101, 102, 103]);
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(ctx, "design_parameter_scopes", &[scope])
            .unwrap();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                LegacyOperation::SheetMetalBaseFlange { profile, side, .. },
            ) = definition
            else {
                panic!("base flange");
            };
            *profile = PlanarProfileRef::Native("f3d:test:profile#regions".into());
            *side = None;
        });
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
        assert!(matches!(
            ir.design.features[0].definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::SheetMetal
            }
        ));
        let [rule] = ir.design.sheet_metal_rules.as_slice() else {
            panic!("owned rule");
        };
        assert_eq!(
            rule.thickness,
            PositiveLength::new(1.25).map(Dimension::Literal)
        );
        assert!(
            matches!(ir.model.features[0].evaluation.definition().operation(), LegacyOperation::SheetMetalBaseFlange { profile: PlanarProfileRef::Native(name), thickness: Some(value), side: None } if name == "f3d:test:profile#regions" && value.get() == 2.0)
        );
    });
}

#[test]
fn unread_profiles_preserve_independent_thickness_side_and_rule() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, Some(0.125));
        let source = ir.model.features[0].native_ref.clone().unwrap();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                LegacyOperation::SheetMetalBaseFlange { profile, .. },
            ) = definition
            else {
                panic!("base flange");
            };
            *profile = PlanarProfileRef::Unresolved(source.clone());
        });
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
        let FeatureOperation::SheetMetal(sheet) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet-metal wall");
        };
        let SheetMetalKind::Wall {
            profile,
            thickness,
            side,
            rule,
        } = &sheet.kind
        else {
            panic!("wall");
        };
        assert_eq!(*thickness, None);
        assert_eq!(*side, Some(NormalSide::Along));
        assert!(rule.is_some());
        assert!(matches!(&profile.picks, Picks::Unread));
        assert_eq!(profile.native.as_ref().unwrap().as_slice(), [source]);
    });
}

#[test]
fn unread_required_thickness_keeps_the_v11_operation_unresolved() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = fixture(ctx, None);
        ir.model.features[0].evaluation.edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                LegacyOperation::SheetMetalBaseFlange { thickness, .. },
            ) = definition
            else {
                panic!("base flange");
            };
            *thickness = None;
        });
        let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
        project(ctx, &mut ir, &definitions, &mut Vec::new()).unwrap();
        assert!(matches!(
            ir.design.features[0].definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::SheetMetal
            }
        ));
        assert!(ir.design.sheet_metal_rules.is_empty());
    });
}
