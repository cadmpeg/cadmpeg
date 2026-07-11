// SPDX-License-Identifier: Apache-2.0
//! Fusion 360 native design and construction-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::{
    ActEntity, ActGuid, ActRootComponent, ConstructionRecipe, DesignBodyMember, DesignEntityHeader,
    DesignMaterialAssignment, DesignObject, DesignRecordHeader, LostEdgeReference,
    PersistentDesignLink, PersistentReference, SketchCurveIdentity, SketchCurveLink, SketchPoint,
    SketchRelation,
};
use crate::history::AsmHistory;

/// Current schema version for the Fusion 360 native namespace.
pub const F3D_NATIVE_VERSION: u32 = 1;

macro_rules! f3d_arenas {
    ($macro:ident) => {
        $macro! {
            act_entities: ActEntity;
            act_guids: ActGuid;
            act_root_components: ActRootComponent;
            design_objects: DesignObject;
            design_entity_headers: DesignEntityHeader;
            design_record_headers: DesignRecordHeader;
            design_body_members: DesignBodyMember;
            design_material_assignments: DesignMaterialAssignment;
            construction_recipes: ConstructionRecipe;
            persistent_design_links: PersistentDesignLink;
            persistent_references: PersistentReference;
            sketch_curve_links: SketchCurveLink;
            sketch_relations: SketchRelation;
            sketch_points: SketchPoint;
            sketch_curve_identities: SketchCurveIdentity;
            lost_edge_references: LostEdgeReference;
            asm_histories: AsmHistory;
        }
    };
}
pub(crate) use f3d_arenas;

macro_rules! sort_f3d_arenas {
    ($($field:ident: $ty:ty;)*) => {
        impl F3dNative {
            /// Sort every native arena by its normative record identity.
            pub(crate) fn finalize(&mut self) {
                $(self.$field.sort_by(|left, right| left.id.cmp(&right.id));)*
            }
        }
    };
}

/// Fusion 360 records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct F3dNative {
    /// Schema version this namespace was written under; see [`F3D_NATIVE_VERSION`].
    pub version: u32,
    /// Fusion ACT change-tracking table entities.
    #[serde(default)]
    pub act_entities: Vec<ActEntity>,
    /// Fusion ACT stream-wide asset/change-version GUID pool.
    #[serde(default)]
    pub act_guids: Vec<ActGuid>,
    /// Fusion ACT document-root-to-registry links.
    #[serde(default)]
    pub act_root_components: Vec<ActRootComponent>,
    /// Design `MetaStream` object-table records.
    #[serde(default)]
    pub design_objects: Vec<DesignObject>,
    /// Self-validating per-entity headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_entity_headers: Vec<DesignEntityHeader>,
    /// Indexed dynamic-class record headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_record_headers: Vec<DesignRecordHeader>,
    /// `BodiesRoot` list members from the Design `BulkStream`.
    #[serde(default)]
    pub design_body_members: Vec<DesignBodyMember>,
    /// Design entity-to-material assignment records.
    #[serde(default)]
    pub design_material_assignments: Vec<DesignMaterialAssignment>,
    /// Parametric regeneration recipes from the Design `BulkStream`.
    #[serde(default)]
    pub construction_recipes: Vec<ConstructionRecipe>,
    /// Persistent Fusion design identifiers attached to solved B-rep entities.
    #[serde(default)]
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Persistent point/curve references from Design construction records.
    #[serde(default)]
    pub persistent_references: Vec<PersistentReference>,
    /// Provenance links from sketch curves to generated B-rep coedges.
    #[serde(default)]
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Bidirectional relations owned by sketch containers.
    #[serde(default)]
    pub sketch_relations: Vec<SketchRelation>,
    /// Persistent source sketch points.
    #[serde(default)]
    pub sketch_points: Vec<SketchPoint>,
    /// Persistent identity pairs attached to source sketch-curve records.
    #[serde(default)]
    pub sketch_curve_identities: Vec<SketchCurveIdentity>,
    /// Construction-history edge selections that Fusion could not re-resolve.
    #[serde(default)]
    pub lost_edge_references: Vec<LostEdgeReference>,
    /// ASM construction-history containers and their linked delta states.
    #[serde(default)]
    pub asm_histories: Vec<AsmHistory>,
}

impl Default for F3dNative {
    fn default() -> Self {
        Self {
            version: F3D_NATIVE_VERSION,
            act_entities: Vec::new(),
            act_guids: Vec::new(),
            act_root_components: Vec::new(),
            design_objects: Vec::new(),
            design_entity_headers: Vec::new(),
            design_record_headers: Vec::new(),
            design_body_members: Vec::new(),
            design_material_assignments: Vec::new(),
            construction_recipes: Vec::new(),
            persistent_design_links: Vec::new(),
            persistent_references: Vec::new(),
            sketch_curve_links: Vec::new(),
            sketch_relations: Vec::new(),
            sketch_points: Vec::new(),
            sketch_curve_identities: Vec::new(),
            lost_edge_references: Vec::new(),
            asm_histories: Vec::new(),
        }
    }
}

f3d_arenas!(sort_f3d_arenas);
