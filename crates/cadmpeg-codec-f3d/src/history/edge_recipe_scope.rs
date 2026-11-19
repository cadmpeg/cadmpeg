// SPDX-License-Identifier: Apache-2.0

use super::{entity_in_brep_source, historical_brep_source, AsmHistory};
use crate::records::{
    feature::scope::DesignParameterScope, recipes::ConstructionRecipe,
    sketch_links::PersistentSubentityTag, topology::edge_identity::DesignEdgeOperand,
};
use cadmpeg_ir::{attributes::AttributeTarget, ids::FaceId};

#[derive(Clone, Copy, Default)]
pub(crate) struct Sources<'a> {
    pub(crate) operands: &'a [DesignEdgeOperand],
    pub(crate) recipes: &'a [ConstructionRecipe],
    pub(crate) tags: &'a [PersistentSubentityTag],
}

fn qualified_faces<'a>(scope: &DesignParameterScope, sources: Sources<'a>) -> Vec<&'a FaceId> {
    let mut faces = Vec::new();
    for operand in sources.operands.iter().filter(|operand| {
        operand.scope_record_index == scope.record_index
            && crate::ids::native_stream(&operand.id) == crate::ids::native_stream(&scope.id)
    }) {
        faces.extend(&operand.candidate_faces);
        for reference in &operand.recipe_references {
            faces.extend(&reference.candidate_faces);
            faces.extend(sources.tags.iter().filter_map(|tag| match &tag.target {
                AttributeTarget::Face(face)
                    if tag.token.as_str() == reference.token
                        && tag.selector == reference.selector
                        && tag.design_references.contains(&reference.design_reference)
                        && crate::ids::same_native_occurrence(&tag.id, &operand.id) =>
                {
                    Some(face)
                }
                _ => None,
            }));
        }
        if let Some(reference) = sources
            .recipes
            .iter()
            .find(|recipe| recipe.id == operand.recipe_id)
            .and_then(|recipe| recipe.record_index)
            .map(|index| i64::from(index.value))
            .filter(|index| *index >= 0)
        {
            faces.extend(sources.tags.iter().filter_map(|tag| match &tag.target {
                AttributeTarget::Face(face)
                    if tag.design_references.contains(&reference)
                        && crate::ids::same_native_occurrence(&tag.id, &operand.id) =>
                {
                    Some(face)
                }
                _ => None,
            }));
        }
    }
    faces.retain(|face| face.as_str().starts_with("f3d:brep/"));
    faces
}

pub(super) fn select<'a>(
    scope: &DesignParameterScope,
    sources: Sources<'_>,
    histories: &[&'a AsmHistory],
) -> Option<&'a AsmHistory> {
    scope.previous_history_state_id()?;
    let faces = qualified_faces(scope, sources);
    faces.first()?;
    let mut matches = histories.iter().copied().filter(|history| {
        historical_brep_source(&history.id).is_some_and(|source| {
            faces
                .iter()
                .all(|face| entity_in_brep_source(face.as_str(), source))
        })
    });
    let history = matches.next()?;
    matches.next().is_none().then_some(history)
}
