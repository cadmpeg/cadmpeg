// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Autodesk Fusion native design and construction-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::history_records::AsmHistory;
use crate::records::{
    ActEntity, ActGuid, ActRootComponent, BodyNativeKey, BodyVisibility, ConstructionRecipe,
    CreationTimestamp, DesignBodyBinding, DesignBodyBounds, DesignBodyMember,
    DesignBodyRecipeOperand, DesignCanvasImage, DesignConfiguration,
    DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
    DesignDimensionAnnotationFrame, DesignDimensionLocusGroup, DesignDimensionLocusPair,
    DesignDimensionNullLocusPair, DesignDimensionRecipeRecord, DesignEdgeIdentityOperand,
    DesignEdgeOperand, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignFaceOperand,
    DesignFilletRadiusGroup, DesignMaterialAssignment, DesignObject, DesignParameter,
    DesignParameterCompanion, DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
    DesignSketchPlacement, EdgeContinuity, EdgeOwnership, FaceSidedness, LostEdgeReference,
    MeshSurfaceSentinel, PersistentDesignLink, PersistentReference, PersistentSubentityTag,
    SketchCurveIdentity, SketchCurveLink, SketchPoint, SketchRelation, SketchSurface, SketchText,
    TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail, TransformHints,
    VertexOwnership, WireTopology, XrefDesign, XrefReference,
};

/// Current schema version for the Autodesk Fusion native namespace.
pub const F3D_NATIVE_VERSION: u32 = 6;

pub(crate) const F3D_ARENA_NAMES: &[&str] = &[
    "act_entities",
    "act_guids",
    "act_root_components",
    "asm_bulletin_boards",
    "asm_delta_states",
    "asm_entity_changes",
    "asm_histories",
    "asm_history_records",
    "body_native_keys",
    "body_visibilities",
    "construction_recipes",
    "creation_timestamps",
    "design_body_bindings",
    "design_body_bounds",
    "design_body_members",
    "design_body_recipe_operands",
    "design_canvas_images",
    "design_configurations",
    "design_construction_operand_groups",
    "design_construction_operand_identities",
    "design_dimension_annotation_frames",
    "design_dimension_locus_groups",
    "design_dimension_locus_pairs",
    "design_dimension_null_locus_pairs",
    "design_dimension_recipe_records",
    "design_edge_identity_operands",
    "design_edge_operands",
    "design_entity_headers",
    "design_entity_selection_operands",
    "design_extrude_selection_groups",
    "design_extrude_selection_members",
    "design_face_operands",
    "design_fillet_radius_groups",
    "design_material_assignments",
    "design_objects",
    "design_parameter_companions",
    "design_parameter_owners",
    "design_parameter_scopes",
    "design_parameters",
    "design_record_headers",
    "design_sketch_placements",
    "edge_continuities",
    "edge_ownerships",
    "face_sidedness",
    "lost_edge_references",
    "mesh_surface_sentinels",
    "persistent_design_links",
    "persistent_references",
    "persistent_subentity_tags",
    "sketch_curve_identities",
    "sketch_curve_links",
    "sketch_points",
    "sketch_relations",
    "sketch_surfaces",
    "sketch_texts",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "vertex_ownerships",
    "wire_topologies",
    "xref_designs",
    "xref_references",
];

macro_rules! f3d_arenas {
    ($macro:ident) => {
        $macro! {
            act_entities: ActEntity;
            act_guids: ActGuid;
            act_root_components: ActRootComponent;
            body_native_keys: BodyNativeKey;
            body_visibilities: BodyVisibility;
            design_objects: DesignObject;
            design_body_recipe_operands: DesignBodyRecipeOperand;
            design_canvas_images: DesignCanvasImage;
            design_dimension_annotation_frames: DesignDimensionAnnotationFrame;
            design_dimension_locus_groups: DesignDimensionLocusGroup;
            design_dimension_locus_pairs: DesignDimensionLocusPair;
            design_dimension_null_locus_pairs: DesignDimensionNullLocusPair;
            design_dimension_recipe_records: DesignDimensionRecipeRecord;
            design_edge_operands: DesignEdgeOperand;
            design_edge_identity_operands: DesignEdgeIdentityOperand;
            design_entity_selection_operands: DesignEntitySelectionOperand;
            design_face_operands: DesignFaceOperand;
            design_construction_operand_groups: DesignConstructionOperandGroup;
            design_construction_operand_identities: DesignConstructionOperandIdentity;
            design_extrude_selection_groups: DesignExtrudeSelectionGroup;
            design_extrude_selection_members: DesignExtrudeSelectionMember;
            design_fillet_radius_groups: DesignFilletRadiusGroup;
            design_parameter_companions: DesignParameterCompanion;
            design_parameter_owners: DesignParameterOwner;
            design_parameter_scopes: DesignParameterScope;
            design_parameters: DesignParameter;
            design_entity_headers: DesignEntityHeader;
            design_record_headers: DesignRecordHeader;
            design_sketch_placements: DesignSketchPlacement;
            design_body_bindings: DesignBodyBinding;
            design_body_bounds: DesignBodyBounds;
            design_body_members: DesignBodyMember;
            design_configurations: DesignConfiguration;
            design_material_assignments: DesignMaterialAssignment;
            edge_continuities: EdgeContinuity;
            edge_ownerships: EdgeOwnership;
            face_sidedness: FaceSidedness;
            construction_recipes: ConstructionRecipe;
            creation_timestamps: CreationTimestamp;
            persistent_design_links: PersistentDesignLink;
            persistent_references: PersistentReference;
            persistent_subentity_tags: PersistentSubentityTag;
            sketch_curve_links: SketchCurveLink;
            sketch_relations: SketchRelation;
            sketch_points: SketchPoint;
            sketch_curve_identities: SketchCurveIdentity;
            sketch_surfaces: SketchSurface;
            sketch_texts: SketchText;
            lost_edge_references: LostEdgeReference;
            mesh_surface_sentinels: MeshSurfaceSentinel;
            vertex_ownerships: VertexOwnership;
            tolerant_coedge_parameters: TolerantCoedgeParameters;
            tolerant_edge_tails: TolerantEdgeTail;
            tolerant_vertex_tails: TolerantVertexTail;
            transform_hints: TransformHints;
            wire_topologies: WireTopology;
            xref_designs: XrefDesign;
            xref_references: XrefReference;
            asm_histories: AsmHistory;
        }
    };
}

macro_rules! sort_f3d_arenas {
    ($($field:ident: $ty:ty;)*) => {
        impl F3dNative {
            pub fn load(namespace: &cadmpeg_ir::NativeNamespace) -> Result<Self, cadmpeg_ir::NativeConvertError> {
                let mut native = Self {
                    version: namespace.version,
                    $($field: namespace.arena_as(stringify!($field))?,)*
                };
                let mut states: Vec<crate::history_records::AsmDeltaState> = namespace.arena_as("asm_delta_states")?;
                let mut boards: Vec<crate::history_records::AsmBulletinBoard> = namespace.arena_as("asm_bulletin_boards")?;
                let changes: Vec<crate::history_records::AsmEntityChange> = namespace.arena_as("asm_entity_changes")?;
                let records: Vec<crate::history_records::AsmHistoryRecord> = namespace.arena_as("asm_history_records")?;
                for board in &mut boards { board.changes = changes.iter().filter(|change| change.parent == board.id).cloned().collect(); }
                for state in &mut states {
                    state.bulletin_boards = boards.iter().filter(|board| board.parent == state.id).cloned().collect();
                    state.records = records.iter().filter(|record| record.parent == state.id).cloned().collect();
                }
                for history in &mut native.asm_histories { history.states = states.iter().filter(|state| state.parent == history.id).cloned().collect(); }
                Ok(native)
            }

            pub fn store(&self, namespace: &mut cadmpeg_ir::NativeNamespace) -> Result<(), cadmpeg_ir::NativeConvertError> {
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
                debug_assert!(F3D_ARENA_NAMES
                    .iter()
                    .all(|name| namespace.arenas.contains_key(*name)));
                Ok(())
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
    /// Native Design-join keys stored on ASM bodies.
    #[serde(default)]
    pub body_native_keys: Vec<BodyNativeKey>,
    /// Design browser-node visibility joined to solved ASM bodies.
    #[serde(default)]
    pub body_visibilities: Vec<BodyVisibility>,
    /// Design `MetaStream` object-table records.
    #[serde(default)]
    pub design_objects: Vec<DesignObject>,
    /// Whole-body operands joined to persistent body construction recipes.
    #[serde(default)]
    pub design_body_recipe_operands: Vec<DesignBodyRecipeOperand>,
    /// Exact image-plane bindings owned by Canvas timeline objects.
    #[serde(default)]
    pub design_canvas_images: Vec<DesignCanvasImage>,
    /// Annotated paired dimension frames governing parameter companions.
    #[serde(default)]
    pub design_dimension_annotation_frames: Vec<DesignDimensionAnnotationFrame>,
    /// Typed paired loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_pairs: Vec<DesignDimensionLocusPair>,
    /// Counted typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_groups: Vec<DesignDimensionLocusGroup>,
    /// Null-plus-typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_null_locus_pairs: Vec<DesignDimensionNullLocusPair>,
    /// Indexed records containing dimension-owned construction recipes.
    #[serde(default)]
    pub design_dimension_recipe_records: Vec<DesignDimensionRecipeRecord>,
    /// Edge-selection operands recovered from Fillet and Chamfer scopes.
    #[serde(default)]
    pub design_edge_operands: Vec<DesignEdgeOperand>,
    /// Persistent selection identities recovered from Fillet and Chamfer groups.
    #[serde(default)]
    pub design_edge_identity_operands: Vec<DesignEdgeIdentityOperand>,
    /// Face-selection operands recovered from Extrude construction groups.
    #[serde(default)]
    pub design_face_operands: Vec<DesignFaceOperand>,
    /// Counted construction-operand groups owned by feature parameter scopes.
    #[serde(default)]
    pub design_construction_operand_groups: Vec<DesignConstructionOperandGroup>,
    /// Persistent identity frames named by construction-operand groups.
    #[serde(default)]
    pub design_construction_operand_identities: Vec<DesignConstructionOperandIdentity>,
    /// Counted selection groups owned by Extrude parameter scopes.
    #[serde(default)]
    pub design_extrude_selection_groups: Vec<DesignExtrudeSelectionGroup>,
    /// Fixed-width members named by Extrude selection groups.
    #[serde(default)]
    pub design_extrude_selection_members: Vec<DesignExtrudeSelectionMember>,
    /// Nested persistent-entity operands named by counted construction groups.
    #[serde(default)]
    pub design_entity_selection_operands: Vec<DesignEntitySelectionOperand>,
    /// Radius parameters paired with counted Fillet edge groups.
    #[serde(default)]
    pub design_fillet_radius_groups: Vec<DesignFilletRadiusGroup>,
    /// Fixed prefixes of indexed records paired with parameter owners.
    #[serde(default)]
    pub design_parameter_companions: Vec<DesignParameterCompanion>,
    /// Fixed-width owner frames for indexed Design parameters.
    #[serde(default)]
    pub design_parameter_owners: Vec<DesignParameterOwner>,
    /// Sketch and construction-operation records that scope parameters.
    #[serde(default)]
    pub design_parameter_scopes: Vec<DesignParameterScope>,
    /// Indexed Design parameter and expression records.
    #[serde(default)]
    pub design_parameters: Vec<DesignParameter>,
    /// Local-to-model placement frames for Design sketches.
    #[serde(default)]
    pub design_sketch_placements: Vec<DesignSketchPlacement>,
    /// Self-validating per-entity headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_entity_headers: Vec<DesignEntityHeader>,
    /// Indexed dynamic-class record headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_record_headers: Vec<DesignRecordHeader>,
    /// `BodiesRoot` list members from the Design `BulkStream`.
    #[serde(default)]
    pub design_body_members: Vec<DesignBodyMember>,
    /// Ordered BREP body-map pairs from Design streams.
    #[serde(default)]
    pub design_body_bindings: Vec<DesignBodyBinding>,
    /// Triplicated axis-aligned bounds cached by Design body containers.
    #[serde(default)]
    pub design_body_bounds: Vec<DesignBodyBounds>,
    /// Design configuration tables and rules with complete JSON payloads.
    #[serde(default)]
    pub design_configurations: Vec<DesignConfiguration>,
    /// Design entity-to-material assignment records.
    #[serde(default)]
    pub design_material_assignments: Vec<DesignMaterialAssignment>,
    /// Kernel continuity classifications stored on solved ASM edges.
    #[serde(default)]
    pub edge_continuities: Vec<EdgeContinuity>,
    /// Native owner-coedge selectors stored on ASM edges.
    #[serde(default)]
    pub edge_ownerships: Vec<EdgeOwnership>,
    /// Native single/double-sided classifications stored on ASM faces.
    #[serde(default)]
    pub face_sidedness: Vec<FaceSidedness>,
    /// Parametric regeneration recipes from the Design `BulkStream`.
    #[serde(default)]
    pub construction_recipes: Vec<ConstructionRecipe>,
    /// Original authoring times attached to solved entities.
    #[serde(default)]
    pub creation_timestamps: Vec<CreationTimestamp>,
    /// Persistent Fusion design identifiers attached to solved B-rep entities.
    #[serde(default)]
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Persistent point/curve references from Design construction records.
    #[serde(default)]
    pub persistent_references: Vec<PersistentReference>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    #[serde(default)]
    pub persistent_subentity_tags: Vec<PersistentSubentityTag>,
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
    /// Persistent tensor-product surfaces owned by spatial sketches.
    #[serde(default)]
    pub sketch_surfaces: Vec<SketchSurface>,
    /// Persistent text entities owned by planar sketches.
    #[serde(default)]
    pub sketch_texts: Vec<SketchText>,
    /// Construction-history edge selections that Fusion could not re-resolve.
    #[serde(default)]
    pub lost_edge_references: Vec<LostEdgeReference>,
    /// Zero-payload ASM mesh-surface sentinels linked to unknown exact surfaces.
    #[serde(default)]
    pub mesh_surface_sentinels: Vec<MeshSurfaceSentinel>,
    /// Native owner-edge and endpoint-slot fields stored on ASM vertices.
    #[serde(default)]
    pub vertex_ownerships: Vec<VertexOwnership>,
    /// Native parameter intervals stored on tolerant ASM coedges.
    #[serde(default)]
    pub tolerant_coedge_parameters: Vec<TolerantCoedgeParameters>,
    /// Native trailing LONG slots stored on tolerant ASM edges.
    #[serde(default)]
    pub tolerant_edge_tails: Vec<TolerantEdgeTail>,
    /// Native trailing f32 slots stored on tolerant ASM vertices.
    #[serde(default)]
    pub tolerant_vertex_tails: Vec<TolerantVertexTail>,
    /// Native transform rotation/reflection/shear classifications.
    #[serde(default)]
    pub transform_hints: Vec<TransformHints>,
    /// Native wire records and their side classifications.
    #[serde(default)]
    pub wire_topologies: Vec<WireTopology>,
    /// Container external-reference design entries
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub xref_designs: Vec<XrefDesign>,
    /// Container outgoing XREF placements
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub xref_references: Vec<XrefReference>,
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
            body_native_keys: Vec::new(),
            body_visibilities: Vec::new(),
            design_objects: Vec::new(),
            design_body_recipe_operands: Vec::new(),
            design_canvas_images: Vec::new(),
            design_dimension_annotation_frames: Vec::new(),
            design_dimension_locus_pairs: Vec::new(),
            design_dimension_locus_groups: Vec::new(),
            design_dimension_null_locus_pairs: Vec::new(),
            design_dimension_recipe_records: Vec::new(),
            design_edge_operands: Vec::new(),
            design_edge_identity_operands: Vec::new(),
            design_face_operands: Vec::new(),
            design_construction_operand_groups: Vec::new(),
            design_construction_operand_identities: Vec::new(),
            design_extrude_selection_groups: Vec::new(),
            design_extrude_selection_members: Vec::new(),
            design_entity_selection_operands: Vec::new(),
            design_fillet_radius_groups: Vec::new(),
            design_parameter_companions: Vec::new(),
            design_parameter_owners: Vec::new(),
            design_parameter_scopes: Vec::new(),
            design_parameters: Vec::new(),
            design_sketch_placements: Vec::new(),
            design_entity_headers: Vec::new(),
            design_record_headers: Vec::new(),
            design_body_members: Vec::new(),
            design_body_bindings: Vec::new(),
            design_body_bounds: Vec::new(),
            design_configurations: Vec::new(),
            design_material_assignments: Vec::new(),
            edge_continuities: Vec::new(),
            edge_ownerships: Vec::new(),
            face_sidedness: Vec::new(),
            construction_recipes: Vec::new(),
            creation_timestamps: Vec::new(),
            persistent_design_links: Vec::new(),
            persistent_references: Vec::new(),
            persistent_subentity_tags: Vec::new(),
            sketch_curve_links: Vec::new(),
            sketch_relations: Vec::new(),
            sketch_points: Vec::new(),
            sketch_curve_identities: Vec::new(),
            sketch_surfaces: Vec::new(),
            sketch_texts: Vec::new(),
            lost_edge_references: Vec::new(),
            mesh_surface_sentinels: Vec::new(),
            vertex_ownerships: Vec::new(),
            tolerant_coedge_parameters: Vec::new(),
            tolerant_edge_tails: Vec::new(),
            tolerant_vertex_tails: Vec::new(),
            transform_hints: Vec::new(),
            wire_topologies: Vec::new(),
            xref_designs: Vec::new(),
            xref_references: Vec::new(),
            asm_histories: Vec::new(),
        }
    }
}

f3d_arenas!(sort_f3d_arenas);
