// SPDX-License-Identifier: Apache-2.0
//! Unique current operations and resolved procedural recipes.

use crate::container::ContainerScan;
use crate::feature::operations::{FeatureOperation, FeatureRecipe};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(super) struct OperationRows<'scan, 'ctx> {
    rows: HashMap<u32, Option<&'scan FeatureOperation>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'scan, 'ctx> OperationRows<'scan, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, operations: &'scan [FeatureOperation]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo current operation index")?;
        let mut rows = HashMap::new();
        for operation in ctx.admit_iter(operations, "creo current operation rows")? {
            match storage.with_storage(|| ctx.entry_hash_map(&mut rows, operation.feature_id, "creo current operation index nodes"))? {
                std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(operation)); }
                std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
            }
        }
        Ok(Self { rows, _storage: storage })
    }

    pub(super) fn get(&self, feature_id: u32) -> Option<&'scan FeatureOperation> {
        self.rows.get(&feature_id).copied().flatten()
    }
}

pub(super) fn feature_recipe(ctx: &DecodeContext<'_>, scan: &ContainerScan, feature_id: u32) -> Result<Option<FeatureRecipe>, CodecError> {
    Ok(crate::decode::uniqueness::exactly_one_by(ctx, &scan.features.operations,
        |operation| Ok(operation.feature_id == feature_id), "creo current feature operation lookup")?
        .and_then(|operation| operation.recipe.resolved()))
}


#[cfg(test)]
mod tests {
    use super::{feature_recipe, OperationRows};
    use crate::feature::operations::{FeatureOperation, FeatureRecipe, OperationKind, OperationName};

    fn operation(feature_id: u32, recipe: FeatureRecipe) -> FeatureOperation {
        FeatureOperation { feature_id, kind: OperationKind::Native, name: OperationName::Derived,
            recipe: Some(recipe).into(), display_state_conflict: false, depdb: None, offset: 0, state_offset: 0 }
    }

    #[test]
    fn duplicate_current_operations_are_ambiguous_even_when_recipes_agree() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.operations = vec![
            operation(7, FeatureRecipe::ProtrudeRevolve),
            operation(9, FeatureRecipe::CutExtrude),
            operation(7, FeatureRecipe::ProtrudeRevolve),
        ];
        crate::decode::with_test_decode_ctx(|ctx| {
            let rows = OperationRows::new(ctx, &scan.features.operations)?;
            assert!(rows.get(7).is_none());
            assert!(rows.get(8).is_none());
            assert!(std::ptr::eq(rows.get(9).expect("unique operation"), &scan.features.operations[1]));
            assert_eq!(feature_recipe(ctx, &scan, 7)?, None);
            assert_eq!(feature_recipe(ctx, &scan, 9)?, Some(FeatureRecipe::CutExtrude));
            Ok::<_, cadmpeg_core::CodecError>(())
        }).expect("service current operation queries");
        let error = crate::test_support::last_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo current operation index nodes", |ctx| {
                let rows = OperationRows::new(ctx, &scan.features.operations)?;
                Ok::<_, cadmpeg_core::CodecError>(rows.get(9).and_then(|row| row.recipe.resolved()))
            });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.operation == "creo current operation index nodes"));
    }
}
