// SPDX-License-Identifier: Apache-2.0

use super::{named_feature_definition, schema_feature_definition};
use crate::feature::operations::{
    FeatureOperation, FeatureRecipe, FeatureRecipeEffect, FeatureRecipeKind, OperationKind,
    OperationName, RecipeResolution,
};
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation as IrOperation};

#[test]
fn generic_material_names_do_not_establish_a_sweep_family() {
    let scan = crate::test_support::empty_container_scan();
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    for kind in ["Protrusion", "Cut"] {
        let definition = crate::decode::with_service_context(|ctx| {
            named_feature_definition(ctx, &scan, &ir, &carriers, 824, kind)
        })
        .expect("generic name has no family authority");
        assert!(
            definition.is_none(),
            "generic label projected a sweep: {kind}"
        );
    }
}

#[test]
fn generic_material_schema_without_a_recipe_remains_native() {
    let scan = crate::test_support::empty_container_scan();
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    for (class, kind) in [
        (SchemaClass::Protrusion, "Protrusion"),
        (SchemaClass::Cut, "Cut"),
    ] {
        let definition = crate::decode::with_service_context(|ctx| {
            schema_feature_definition(ctx, &scan, &ir, &carriers, 824, Some(class), kind)
        })
        .expect("schema union has no sweep family");
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(IrOperation::Native { .. })
        ));
    }
}

#[test]
fn admitted_recipe_selects_the_generic_material_family() {
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    for recipe in [
        FeatureRecipe::ProtrudeExtrude,
        FeatureRecipe::CutExtrude,
        FeatureRecipe::ProtrudeRevolve,
        FeatureRecipe::CutRevolve,
    ] {
        let mut scan = crate::test_support::empty_container_scan();
        let (kind, class) = match recipe.effect() {
            FeatureRecipeEffect::Protrude => ("Protrusion", SchemaClass::Protrusion),
            FeatureRecipeEffect::Cut => ("Cut", SchemaClass::Cut),
        };
        scan.features.operations.push(FeatureOperation {
            feature_id: 824,
            kind: OperationKind::Stored(kind.to_owned()),
            name: OperationName::Derived,
            recipe: RecipeResolution::Resolved(recipe),
            display_state_conflict: false,
            depdb: None,
            offset: 1,
        });
        let named = crate::decode::with_service_context(|ctx| {
            named_feature_definition(ctx, &scan, &ir, &carriers, 824, kind)
        })
        .expect("admitted procedural family")
        .expect("owned family remains projected");
        let schema = crate::decode::with_service_context(|ctx| {
            schema_feature_definition(ctx, &scan, &ir, &carriers, 824, Some(class), kind)
        })
        .expect("admitted schema recipe");
        for definition in [named, schema] {
            match recipe.kind() {
                FeatureRecipeKind::Extrude => assert!(matches!(
                    definition,
                    FeatureDefinition::Operation(IrOperation::Extrude { .. })
                )),
                FeatureRecipeKind::Revolve => assert!(matches!(
                    definition,
                    FeatureDefinition::Operation(IrOperation::Revolve { .. })
                )),
            }
        }
    }
}

fn section_sweep_row(
    class: SchemaClass,
    class_bytes: &[u8],
    form: u8,
) -> crate::feature::rows::FeatureRow {
    let mut body = vec![0x90, 0x00, 0x00, 0x98, 0x00, 0xe3, 0xf6];
    body.extend_from_slice(class_bytes);
    body.extend_from_slice(&[0xe1, form, 0x00]);
    crate::feature::rows::FeatureRow {
        feature_id: 824,
        root_schema_class: Some(class),
        stream_offset: 0,
        body: body.try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn section_sweep_row_form_selects_the_generic_material_family() {
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    for (class, class_bytes, kind) in [
        (SchemaClass::Protrusion, [0x83, 0x95], "Protrusion"),
        (SchemaClass::Cut, [0x83, 0x94], "Cut"),
    ] {
        for (form, family) in [
            (1, FeatureRecipeKind::Extrude),
            (2, FeatureRecipeKind::Revolve),
        ] {
            let mut scan = crate::test_support::empty_container_scan();
            scan.features
                .rows
                .push(section_sweep_row(class, &class_bytes, form));
            let named = crate::decode::with_service_context(|ctx| {
                named_feature_definition(ctx, &scan, &ir, &carriers, 824, kind)
            })
            .expect("row form family")
            .expect("row form projects the family");
            let schema = crate::decode::with_service_context(|ctx| {
                schema_feature_definition(ctx, &scan, &ir, &carriers, 824, Some(class), kind)
            })
            .expect("row form schema family");
            for definition in [named, schema] {
                match family {
                    FeatureRecipeKind::Extrude => assert!(matches!(
                        definition,
                        FeatureDefinition::Operation(IrOperation::Extrude { .. })
                    )),
                    FeatureRecipeKind::Revolve => assert!(matches!(
                        definition,
                        FeatureDefinition::Operation(IrOperation::Revolve { .. })
                    )),
                }
            }
        }
    }
}

#[test]
fn other_row_forms_and_disagreeing_recipes_select_no_family() {
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .push(section_sweep_row(SchemaClass::Protrusion, &[0x83, 0x95], 4));
    let definition = crate::decode::with_service_context(|ctx| {
        named_feature_definition(ctx, &scan, &ir, &carriers, 824, "Protrusion")
    })
    .expect("unknown form");
    assert!(definition.is_none());

    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .push(section_sweep_row(SchemaClass::Protrusion, &[0x83, 0x95], 1));
    scan.features.operations.push(FeatureOperation {
        feature_id: 824,
        kind: OperationKind::Stored("Protrusion".to_owned()),
        name: OperationName::Derived,
        recipe: RecipeResolution::Resolved(FeatureRecipe::ProtrudeRevolve),
        display_state_conflict: false,
        depdb: None,
        offset: 1,
    });
    let definition = crate::decode::with_service_context(|ctx| {
        named_feature_definition(ctx, &scan, &ir, &carriers, 824, "Protrusion")
    })
    .expect("disagreeing recipe");
    assert!(definition.is_none());
}
