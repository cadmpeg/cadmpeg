// SPDX-License-Identifier: Apache-2.0
//! The top-level IR document and its flat arenas.

use std::collections::BTreeMap;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::design::{
    ActEntity, ActGuid, ActRootComponent, ConstructionRecipe, DesignBodyMember, DesignEntityHeader,
    DesignObject, DesignRecordHeader, LostEdgeReference, PersistentDesignLink, PersistentReference,
    SketchCurveIdentity, SketchCurveLink, SketchPoint, SketchRelation,
};
use crate::geometry::{
    Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface, SurfaceParameterization,
};
use crate::history::{AsmHistory, FeatureHistory, FeatureInputLane};
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Lump, Point, Shell, Vertex};
use crate::units::{Tolerances, Units};
use crate::unknown::UnknownRecord;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "0";

/// Source-container metadata preserved for provenance and reporting. Values are
/// free-form so a codec can record format-specific header facts (for `.f3d`:
/// `product_family`, `product_version`, `save_date`, the ASM version word)
/// without the IR needing a typed slot for each.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceMeta {
    /// Source container format id, e.g. `"f3d"`.
    pub format: String,
    /// Format-specific header attributes, sorted by key for canonical output.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// A decoded CAD document: units, tolerances, and the B-rep graph stored as
/// flat, id-referenced arenas.
///
/// Serializing this with [`CadIr::to_canonical_json`] produces the `.cadir.json`
/// artifact. Field order here is the canonical field order; arenas preserve
/// insertion order and the [`SourceMeta::attributes`] map is sorted, so equal
/// documents serialize byte-for-byte identically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CadIr {
    /// IR schema version (see [`IR_VERSION`]).
    pub ir_version: String,
    /// Source-container metadata, if decoded from a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceMeta>,
    /// Unit declaration for stored coordinates.
    pub units: Units,
    /// Kernel tolerances.
    pub tolerances: Tolerances,

    /// Body arena.
    pub bodies: Vec<Body>,
    /// Lump arena.
    pub lumps: Vec<Lump>,
    /// Shell arena.
    pub shells: Vec<Shell>,
    /// Face arena.
    pub faces: Vec<Face>,
    /// Loop arena.
    pub loops: Vec<Loop>,
    /// Coedge arena.
    pub coedges: Vec<Coedge>,
    /// Edge arena.
    pub edges: Vec<Edge>,
    /// Vertex arena.
    pub vertices: Vec<Vertex>,
    /// Point arena.
    pub points: Vec<Point>,
    /// Surface carrier arena.
    pub surfaces: Vec<Surface>,
    /// Curve carrier arena.
    pub curves: Vec<Curve>,
    /// Pcurve carrier arena.
    pub pcurves: Vec<Pcurve>,
    /// Parameter frames for surfaces whose UV convention is known.
    #[serde(default)]
    pub surface_parameterizations: Vec<SurfaceParameterization>,
    /// Source-native constructions which produced solved surface carriers.
    #[serde(default)]
    pub procedural_surfaces: Vec<ProceduralSurface>,
    /// Native constructions which produced solved curve caches.
    #[serde(default)]
    pub procedural_curves: Vec<ProceduralCurve>,
    /// Typed provenance links from sketch curves to generated B-rep coedges.
    #[serde(default)]
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent Design BulkStream identifiers attached to solved B-rep entities.
    #[serde(default)]
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Parametric regeneration recipes from the Design BulkStream.
    #[serde(default)]
    pub construction_recipes: Vec<ConstructionRecipe>,
    /// Persistent point/curve references from Design construction records.
    #[serde(default)]
    pub persistent_references: Vec<PersistentReference>,
    /// Parametric edge selections that failed source-side re-resolution.
    #[serde(default)]
    pub lost_edge_references: Vec<LostEdgeReference>,
    /// GUID-owned Design MetaStream objects, including sketches and dimensions.
    #[serde(default)]
    pub design_objects: Vec<DesignObject>,
    /// Self-validating per-entity Design BulkStream headers.
    #[serde(default)]
    pub design_entity_headers: Vec<DesignEntityHeader>,
    /// Indexed dynamic-class records in the Design BulkStream.
    #[serde(default)]
    pub design_record_headers: Vec<DesignRecordHeader>,
    /// Typed bidirectional relations owned by sketch containers.
    #[serde(default)]
    pub sketch_relations: Vec<SketchRelation>,
    /// Persistent source sketch points.
    #[serde(default)]
    pub sketch_points: Vec<SketchPoint>,
    /// Persistent identities bound to source sketch-curve records.
    #[serde(default)]
    pub sketch_curve_identities: Vec<SketchCurveIdentity>,
    /// Native Design `BodiesRoot` membership entries.
    #[serde(default)]
    pub design_body_members: Vec<DesignBodyMember>,
    /// ACT table entities and their per-channel change-version handles.
    #[serde(default)]
    pub act_entities: Vec<ActEntity>,
    /// Ordered stream-wide ACT GUID pool.
    #[serde(default)]
    pub act_guids: Vec<ActGuid>,
    /// ACT root component and registry links.
    #[serde(default)]
    pub act_root_components: Vec<ActRootComponent>,
    /// Source display/facet meshes retained independently of the exact B-rep.
    #[serde(default)]
    pub tessellations: Vec<Tessellation>,
    /// Parametric construction histories carried by the source document.
    #[serde(default)]
    pub feature_histories: Vec<FeatureHistory>,
    /// Native feature-input streams and typed sketch-record views.
    #[serde(default)]
    pub feature_input_lanes: Vec<FeatureInputLane>,
    /// Raw replayable ASM construction-state graphs.
    #[serde(default)]
    pub asm_histories: Vec<AsmHistory>,
    /// Decoded visual/physical appearance assets.
    #[serde(default)]
    pub appearances: Vec<Appearance>,
    /// Explicit body/face appearance assignments.
    #[serde(default)]
    pub appearance_bindings: Vec<AppearanceBinding>,
    /// Source-native linked attributes.
    #[serde(default)]
    pub attributes: Vec<SourceAttribute>,
    /// Uninterpreted passthrough records.
    #[serde(default)]
    pub unknowns: Vec<UnknownRecord>,
}

impl CadIr {
    /// A structurally valid empty document at the current IR version with the
    /// given units.
    pub fn empty(units: Units) -> Self {
        CadIr {
            ir_version: IR_VERSION.to_string(),
            source: None,
            units,
            tolerances: Tolerances::default(),
            bodies: Vec::new(),
            lumps: Vec::new(),
            shells: Vec::new(),
            faces: Vec::new(),
            loops: Vec::new(),
            coedges: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
            points: Vec::new(),
            surfaces: Vec::new(),
            curves: Vec::new(),
            pcurves: Vec::new(),
            surface_parameterizations: Vec::new(),
            procedural_surfaces: Vec::new(),
            procedural_curves: Vec::new(),
            sketch_curve_links: Vec::new(),
            persistent_design_links: Vec::new(),
            construction_recipes: Vec::new(),
            persistent_references: Vec::new(),
            lost_edge_references: Vec::new(),
            design_objects: Vec::new(),
            design_entity_headers: Vec::new(),
            design_record_headers: Vec::new(),
            sketch_relations: Vec::new(),
            sketch_points: Vec::new(),
            sketch_curve_identities: Vec::new(),
            design_body_members: Vec::new(),
            act_entities: Vec::new(),
            act_guids: Vec::new(),
            act_root_components: Vec::new(),
            tessellations: Vec::new(),
            feature_histories: Vec::new(),
            feature_input_lanes: Vec::new(),
            asm_histories: Vec::new(),
            appearances: Vec::new(),
            appearance_bindings: Vec::new(),
            attributes: Vec::new(),
            unknowns: Vec::new(),
        }
    }

    /// Serialize to canonical pretty JSON (stable field and key order).
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON produced by [`CadIr::to_canonical_json`].
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}
