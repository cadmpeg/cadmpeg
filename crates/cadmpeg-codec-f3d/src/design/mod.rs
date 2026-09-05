// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

pub mod assembly;
pub(crate) mod body;
pub mod components;
pub mod configurations;
pub mod constraints;
pub mod decode;
pub mod dimensions;
pub mod edge_resolve;
pub mod face_resolve;
pub mod feature_project;
pub mod geometry;
pub(crate) mod presentation;
pub mod profile_select;
pub mod sketch_project;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use crate::records::ConstructionRecipeKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesignFeatureFamily {
    Sketch,
    Assemble,
    Extrude,
    Fillet,
    Chamfer,
    Combine,
    Draft,
    ReplaceFace,
    CircularPattern,
    RectangularPattern,
    Mirror,
    Move,
    OffsetFaces,
    Revolve,
    Shell,
    Thicken,
    Coil,
    Loft,
    Sweep,
    Pipe,
    SurfacePatch,
    SurfaceExtend,
    SurfaceOffset,
    SurfaceRuled,
    SurfaceTrim,
    BoundaryFill,
    Hole,
    Split,
    Scale,
    Thread,
    SheetMetalEdgeFlange,
    SheetMetalHem,
}

/// Return the canonical operation family while preserving `kind` verbatim on
/// the native scope. Fusion serializes this field through its UI localization.
pub(crate) fn design_feature_family(kind: impl AsRef<str>) -> Option<DesignFeatureFamily> {
    match kind.as_ref() {
        "Sketch" | "Esquisse" | "Skizze" | "Esboço" => Some(DesignFeatureFamily::Sketch),
        "Assemble" | "As-built" => Some(DesignFeatureFamily::Assemble),
        "Extrude" | "Extrusion" | "Extrusão" => Some(DesignFeatureFamily::Extrude),
        "Fillet" | "Congé" | "Abrundung" | "Arredondamento" => Some(DesignFeatureFamily::Fillet),
        "Chamfer" | "Chanfrein" => Some(DesignFeatureFamily::Chamfer),
        "Combine" => Some(DesignFeatureFamily::Combine),
        "Draft" => Some(DesignFeatureFamily::Draft),
        "ReplaceFace" => Some(DesignFeatureFamily::ReplaceFace),
        "C-Pattern" | "Circular Pattern" | "Réseau C" => {
            Some(DesignFeatureFamily::CircularPattern)
        }
        "R-Pattern" | "Rectangular Pattern" => Some(DesignFeatureFamily::RectangularPattern),
        "Mirror" | "Symétrie miroir" => Some(DesignFeatureFamily::Mirror),
        "Move" => Some(DesignFeatureFamily::Move),
        "OffsetFaces" | "DécalerLesFaces" => Some(DesignFeatureFamily::OffsetFaces),
        "Revolve" => Some(DesignFeatureFamily::Revolve),
        "Shell" | "Schale" => Some(DesignFeatureFamily::Shell),
        "Thicken" => Some(DesignFeatureFamily::Thicken),
        "SpirePrimitive" | "CoilPrimitive" => Some(DesignFeatureFamily::Coil),
        "Loft" => Some(DesignFeatureFamily::Loft),
        "Sweep" => Some(DesignFeatureFamily::Sweep),
        "Pipe" => Some(DesignFeatureFamily::Pipe),
        "SurfacePatch" => Some(DesignFeatureFamily::SurfacePatch),
        "SurfaceExtend" => Some(DesignFeatureFamily::SurfaceExtend),
        "SurfaceOffset" => Some(DesignFeatureFamily::SurfaceOffset),
        "SurfaceRuled" => Some(DesignFeatureFamily::SurfaceRuled),
        "SurfaceTrim" => Some(DesignFeatureFamily::SurfaceTrim),
        "BoundaryFill" => Some(DesignFeatureFamily::BoundaryFill),
        "Hole" => Some(DesignFeatureFamily::Hole),
        "Split" => Some(DesignFeatureFamily::Split),
        "Scale" | "Maßstab" => Some(DesignFeatureFamily::Scale),
        "Thread" => Some(DesignFeatureFamily::Thread),
        "EdgeFlange" => Some(DesignFeatureFamily::SheetMetalEdgeFlange),
        "Hem" => Some(DesignFeatureFamily::SheetMetalHem),
        _ => None,
    }
}

/// Return whether `kind` is a localized spelling of an edge-treatment family.
///
/// Canonical Fillet and Chamfer scopes require every selection to use a
/// counted construction-operand group. Their localized spellings do not.
pub(crate) fn is_localized_edge_treatment_kind(kind: &str) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    ) && !matches!(kind, "Fillet" | "Chamfer")
}

pub(crate) const RECIPES: &[(&[u8], ConstructionRecipeKind)] = &[
    (b"body_recipe_data", ConstructionRecipeKind::Body),
    (b"face_recipe_data", ConstructionRecipeKind::Face),
    (
        b"bounded_face_recipe_data",
        ConstructionRecipeKind::BoundedFace,
    ),
    (b"edge_recipe_data", ConstructionRecipeKind::Edge),
    (b"vertex_recipe_data", ConstructionRecipeKind::Vertex),
];

pub(crate) const fn construction_recipe_family_name_len(kind: ConstructionRecipeKind) -> usize {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data".len(),
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        ConstructionRecipeKind::Edge => b"edge_recipe_data".len(),
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data".len(),
    }
}
