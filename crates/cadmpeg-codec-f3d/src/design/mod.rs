// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

pub mod assembly;
pub mod components;
pub mod configurations;
pub mod constraints;
pub mod decode;
pub mod dimensions;
pub mod edge_resolve;
pub mod face_resolve;
pub mod feature_project;
pub mod geometry;
pub mod profile_select;
pub mod sketch_project;
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
    SurfacePatch,
    SurfaceExtend,
    BoundaryFill,
    Split,
    Scale,
}

/// Return the canonical operation family while preserving `kind` verbatim on
/// the native scope. Fusion serializes this field through its UI localization.
pub(crate) fn design_feature_family(kind: &str) -> Option<DesignFeatureFamily> {
    match kind {
        "Sketch" | "Esquisse" | "Skizze" | "Esboço" => Some(DesignFeatureFamily::Sketch),
        "Assemble" => Some(DesignFeatureFamily::Assemble),
        "Extrude" | "Extrusion" | "Extrusão" => Some(DesignFeatureFamily::Extrude),
        "Fillet" | "Congé" | "Abrundung" | "Arredondamento" => Some(DesignFeatureFamily::Fillet),
        "Chamfer" | "Chanfrein" => Some(DesignFeatureFamily::Chamfer),
        "Combine" => Some(DesignFeatureFamily::Combine),
        "Draft" => Some(DesignFeatureFamily::Draft),
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
        "SurfacePatch" => Some(DesignFeatureFamily::SurfacePatch),
        "SurfaceExtend" => Some(DesignFeatureFamily::SurfaceExtend),
        "BoundaryFill" => Some(DesignFeatureFamily::BoundaryFill),
        "Split" => Some(DesignFeatureFamily::Split),
        "Scale" | "Maßstab" => Some(DesignFeatureFamily::Scale),
        _ => None,
    }
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
