// SPDX-License-Identifier: Apache-2.0
//! Active faces that the edge recipes of a parameter scope name.

/// The edge-recipe operands of the scopes, with the construction recipes and
/// persistent subentity tags that name the active faces of each recipe.
#[derive(Clone, Copy, Default)]
pub(crate) struct EdgeRecipeFaces<'a> {
    pub(crate) operands: &'a [crate::records::topology::edge_identity::DesignEdgeOperand],
    pub(crate) recipes: &'a [crate::records::recipes::ConstructionRecipe],
    pub(crate) tags: &'a [crate::records::sketch_links::PersistentSubentityTag],
}

impl EdgeRecipeFaces<'_> {
    /// The active faces that the edge recipes of `scope` name. The faces come
    /// from the persistent subentity tags, so history binding of the
    /// operands does not change them.
    pub(super) fn scope_faces(
        &self,
        scope: &crate::records::feature::scope::DesignParameterScope,
    ) -> Vec<cadmpeg_ir::ids::FaceId> {
        let mut faces = Vec::new();
        for operand in self.operands.iter().filter(|operand| {
            crate::ids::same_native_occurrence(&operand.id, &scope.id)
                && operand.scope_record_index == scope.record_index
        }) {
            let Some(design_reference) = self
                .recipes
                .iter()
                .find(|recipe| recipe.id == operand.recipe_id)
                .and_then(|recipe| recipe.record_index)
                .map(|record_index| i64::from(record_index.value))
                .filter(|value| *value >= 0)
            else {
                continue;
            };
            faces.extend(
                crate::design::decode::operands::edge_operand_candidate_faces(
                    design_reference,
                    self.tags,
                    Some(&operand.id),
                ),
            );
        }
        faces
    }
}
