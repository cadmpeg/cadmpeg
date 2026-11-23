// SPDX-License-Identifier: Apache-2.0
//! Parse edge, face, and body operand frames and recipe structure.

pub(crate) mod body_recipes;
pub(crate) mod construction_groups;
pub(crate) mod edge_operands;
pub(crate) mod entity_selections;
pub(crate) mod extrude_roles;
pub(crate) mod extrude_selections;
pub(crate) mod face_operand_frames;
pub(crate) mod face_operands;
pub(crate) mod face_sources;
pub(crate) mod fillets;
pub(crate) mod loft_carriers;
pub(crate) mod operand_candidates;
mod recipe_operands;
pub(crate) mod recipe_structure;
pub(crate) mod sketch_profiles;
pub(crate) mod work_constructions;

pub(crate) mod extrude_extent;
mod fillet_variable_groups;
mod split_groups;

pub(crate) mod compact_owner;
mod tracking_registration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceRecipeProgramKind {
    Terminal,
    Counted { header_value: usize },
}

pub(crate) fn face_recipe_program_kind(program: &[i32]) -> Option<FaceRecipeProgramKind> {
    if matches!(program, [0, -1 | 0]) {
        return Some(FaceRecipeProgramKind::Terminal);
    }
    if !matches!(program.get(0..2), Some([0, -1 | 0])) {
        return None;
    }
    let header_value = usize::try_from(*program.get(2)?).ok()?;
    (header_value > 0 && header_value <= 100_000)
        .then_some(FaceRecipeProgramKind::Counted { header_value })
}

#[cfg(test)]
mod tests;
