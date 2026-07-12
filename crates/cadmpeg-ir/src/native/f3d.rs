// SPDX-License-Identifier: Apache-2.0
//! Autodesk Fusion native design and construction-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::{
    ActEntity, ActGuid, ActRootComponent, ConstructionRecipe, DesignBodyMember, DesignEntityHeader,
    DesignMaterialAssignment, DesignObject, DesignRecordHeader, LostEdgeReference,
    PersistentDesignLink, PersistentReference, SketchCurveIdentity, SketchCurveLink, SketchPoint,
    SketchRelation,
};
use crate::history::AsmHistory;

/// Current schema version for the Autodesk Fusion native namespace.
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

macro_rules! sort_f3d_arenas {
    ($($field:ident: $ty:ty;)*) => {
        impl F3dNative {
            pub fn load(namespace: &super::NativeNamespace) -> Result<Self, super::NativeConvertError> {
                let mut native = Self {
                    version: namespace.version,
                    $($field: namespace.arena_as(stringify!($field))?,)*
                };
                let mut states: Vec<crate::history::AsmDeltaState> = namespace.arena_as("asm_delta_states")?;
                let mut boards: Vec<crate::history::AsmBulletinBoard> = namespace.arena_as("asm_bulletin_boards")?;
                let changes: Vec<crate::history::AsmEntityChange> = namespace.arena_as("asm_entity_changes")?;
                let records: Vec<crate::history::AsmHistoryRecord> = namespace.arena_as("asm_history_records")?;
                for board in &mut boards { board.changes = changes.iter().filter(|change| change.parent == board.id).cloned().collect(); }
                for state in &mut states {
                    state.bulletin_boards = boards.iter().filter(|board| board.parent == state.id).cloned().collect();
                    state.records = records.iter().filter(|record| record.parent == state.id).cloned().collect();
                }
                for history in &mut native.asm_histories { history.states = states.iter().filter(|state| state.parent == history.id).cloned().collect(); }
                Ok(native)
            }

            pub fn store(&self, namespace: &mut super::NativeNamespace) -> Result<(), super::NativeConvertError> {
                namespace.version = F3D_NATIVE_VERSION;
                $(namespace.set_arena(stringify!($field), &self.$field)?;)*
                let histories = self.asm_histories.iter().cloned().map(|mut history| { history.states.clear(); history }).collect::<Vec<_>>();
                let states = self.asm_histories.iter().flat_map(|history| history.states.iter().cloned()).map(|mut state| { state.bulletin_boards.clear(); state.records.clear(); state }).collect::<Vec<_>>();
                let boards = self.asm_histories.iter().flat_map(|history| &history.states).flat_map(|state| state.bulletin_boards.iter().cloned()).map(|mut board| { board.changes.clear(); board }).collect::<Vec<_>>();
                let changes = self.asm_histories.iter().flat_map(|history| &history.states).flat_map(|state| &state.bulletin_boards).flat_map(|board| board.changes.iter().cloned()).collect::<Vec<_>>();
                let records = self.asm_histories.iter().flat_map(|history| &history.states).flat_map(|state| state.records.iter().cloned()).collect::<Vec<_>>();
                namespace.set_arena("asm_histories", &histories)?;
                namespace.set_arena("asm_delta_states", &states)?;
                namespace.set_arena("asm_bulletin_boards", &boards)?;
                namespace.set_arena("asm_entity_changes", &changes)?;
                namespace.set_arena("asm_history_records", &records)?;
                Ok(())
            }
            /// Sort every native arena by its normative record identity.
            pub(crate) fn finalize(&mut self) {
                $(self.$field.sort_by(|left, right| left.id.cmp(&right.id));)*
            }

            /// Return counts for every non-empty native arena.
            pub(crate) fn loss_counts(&self) -> Vec<(&'static str, usize)> {
                let mut counts = Vec::new();
                $(
                    if !self.$field.is_empty() {
                        counts.push((stringify!($field), self.$field.len()));
                    }
                )*
                let states = self.asm_histories.iter().flat_map(|history| &history.states);
                let state_count = states.clone().count();
                let board_count = states.clone().map(|state| state.bulletin_boards.len()).sum();
                let change_count = states
                    .clone()
                    .flat_map(|state| &state.bulletin_boards)
                    .map(|board| board.changes.len())
                    .sum();
                let record_count = states.map(|state| state.records.len()).sum();
                for (kind, count) in [
                    ("asm_delta_states", state_count),
                    ("asm_bulletin_boards", board_count),
                    ("asm_entity_changes", change_count),
                    ("asm_history_records", record_count),
                ] {
                    if count != 0 {
                        counts.push((kind, count));
                    }
                }
                counts
            }
        }
    };
}

/// Autodesk Fusion records retained outside the format-neutral model.
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
