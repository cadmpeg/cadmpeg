// SPDX-License-Identifier: Apache-2.0

use super::{feature_row_schema_classes, feature_schema_class, FeatureSchemaClasses};
use crate::feature::schema::SchemaClass;

fn row(schema_class: SchemaClass) -> crate::feature::rows::FeatureRow {
    feature_row(40, Some(schema_class))
}

fn feature_row(
    feature_id: u32,
    root_schema_class: Option<SchemaClass>,
) -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id,
        root_schema_class,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

fn operation(
    feature_id: u32,
    schema: Option<SchemaClass>,
) -> crate::feature::operations::FeatureOperation {
    crate::feature::operations::FeatureOperation {
        feature_id,
        kind: crate::feature::operations::OperationKind::Stored("Sweep".to_string()),
        name: crate::feature::operations::OperationName::Derived,
        recipe: crate::feature::operations::RecipeResolution::Resolved(
            crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
        ),
        display_state_conflict: false,
        depdb: schema.map(|schema| crate::feature::operations::DepdbPrefix { schema, parent: 0 }),
        offset: 0,
    }
}

#[test]
fn feature_schema_classes_keep_distinct_sorted_values() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(row(SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(SchemaClass::Chamfer));
    let classes = feature_row_schema_classes(&scan, 40);
    assert_eq!(classes.len(), 2);
    assert!(classes.contains(&SchemaClass::Round));
    assert!(classes.contains(&SchemaClass::Chamfer));
}

#[test]
fn indexed_feature_schema_classes_match_the_per_feature_scan() {
    let mut scan = crate::test_support::empty_container_scan();
    // 1: conflicting current and operation classes abstain; 2: duplicate operations
    // fall back to the rows; 3: operation without a class falls back to the
    // rows; 4: conflicting rows fall back to a legacy round; 5: agreeing rows
    // across both row lists; 6: classless rows only; 7: legacy round only;
    // 8: conflicting rows with no fallback.
    scan.features.operations.extend([
        operation(1, Some(SchemaClass::Protrusion)),
        operation(2, Some(SchemaClass::Cut)),
        operation(2, Some(SchemaClass::Protrusion)),
        operation(3, None),
    ]);
    scan.features.rows.extend([
        feature_row(1, Some(SchemaClass::Round)),
        feature_row(2, Some(SchemaClass::Chamfer)),
        feature_row(3, None),
        feature_row(3, Some(SchemaClass::Surface)),
        feature_row(4, Some(SchemaClass::Round)),
        feature_row(5, Some(SchemaClass::Cut)),
        feature_row(6, None),
        feature_row(8, Some(SchemaClass::Cut)),
        feature_row(8, Some(SchemaClass::Cut)),
    ]);
    scan.features.depdb_recipe_rows.extend([
        feature_row(4, Some(SchemaClass::Chamfer)),
        feature_row(5, Some(SchemaClass::Cut)),
        feature_row(8, None),
        feature_row(8, Some(SchemaClass::Protrusion)),
        feature_row(8, Some(SchemaClass::Cut)),
    ]);
    for feature_id in [4, 7] {
        scan.features
            .legacy_rounds
            .push(crate::legacy_feature::LegacyRoundFeature {
                feature_id,
                radius: crate::legacy_feature::LegacyRoundRadius::Constant(
                    cadmpeg_ir::scalar::PositiveReal::new(2.0).expect("positive radius"),
                ),
                edge_ids: None,
                offset: 0,
            });
    }
    let classes = FeatureSchemaClasses::new(&scan);
    for feature_id in 0..=9 {
        assert_eq!(
            classes.get(feature_id),
            feature_schema_class(&scan, feature_id),
            "{feature_id}"
        );
    }
}

#[test]
fn current_material_rows_reject_a_conflicting_creation_recipe() {
    use crate::feature::operations::{FeatureRecipe, RecipeResolution};

    for (recipe, stored, current, agrees) in [
        (
            FeatureRecipe::ProtrudeExtrude,
            SchemaClass::Protrusion,
            SchemaClass::Protrusion,
            true,
        ),
        (
            FeatureRecipe::ProtrudeExtrude,
            SchemaClass::Protrusion,
            SchemaClass::Cut,
            false,
        ),
        (
            FeatureRecipe::CutRevolve,
            SchemaClass::Cut,
            SchemaClass::Cut,
            true,
        ),
        (
            FeatureRecipe::CutRevolve,
            SchemaClass::Cut,
            SchemaClass::Protrusion,
            false,
        ),
    ] {
        let mut scan = crate::test_support::empty_container_scan();
        let mut audit = operation(40, Some(stored));
        audit.recipe = RecipeResolution::Resolved(recipe);
        audit.depdb.as_mut().expect("stored schema").parent = 7;
        scan.features.operations.push(audit.clone());
        scan.features.rows.push(row(current));
        let expected = agrees.then_some(recipe);
        assert_eq!(super::admitted_feature_recipe(&scan, 40), expected);
        assert_eq!(
            super::admitted_feature_recipe_parent(&scan, 40),
            agrees.then_some(7)
        );
        assert_eq!(
            super::current_feature_recipe_parent(&scan.features.operations, 40),
            Some(7)
        );
        assert_eq!(
            super::feature_recipe(&scan, 40),
            expected.map(FeatureRecipe::kind)
        );
        assert_eq!(
            super::feature_recipe_effect(&scan, 40),
            expected.map(FeatureRecipe::effect)
        );
        assert_eq!(
            super::feature_section_sweep_semantics_conflict(&scan, 40),
            !agrees
        );
        assert_eq!(feature_schema_class(&scan, 40), agrees.then_some(current));
        assert_eq!(
            FeatureSchemaClasses::new(&scan).get(40),
            agrees.then_some(current)
        );
        assert_eq!(scan.features.operations[0], audit);
    }
}
