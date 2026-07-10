// SPDX-License-Identifier: Apache-2.0
//! The top-level IR document and its flat arenas.

use std::collections::BTreeMap;

use crate::annotations::Annotations;
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::design::{
    ActEntity, ActGuid, ActRootComponent, ConstructionRecipe, DesignBodyMember, DesignEntityHeader,
    DesignObject, DesignRecordHeader, LostEdgeReference, PersistentDesignLink, PersistentReference,
    SketchCurveIdentity, SketchCurveLink, SketchPoint, SketchRelation,
};
use crate::features::Feature;
use crate::geometry::{
    Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface, SurfaceParameterization,
};
use crate::history::{AsmHistory, FeatureHistory, FeatureInputLane};
use crate::native::Native;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Lump, Point, Shell, Vertex};
use crate::units::{Tolerances, Units};
use crate::unknown::UnknownRecord;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! arena_registry {
    ($macro:ident) => {
        $macro! {
            bodies: Body, "Body arena.", [] => |e| e.id.0.clone();
            lumps: Lump, "Lump arena.", [] => |e| e.id.0.clone();
            shells: Shell, "Shell arena.", [] => |e| e.id.0.clone();
            faces: Face, "Face arena.", [] => |e| e.id.0.clone();
            loops: Loop, "Loop arena.", [] => |e| e.id.0.clone();
            coedges: Coedge, "Coedge arena.", [] => |e| e.id.0.clone();
            edges: Edge, "Edge arena.", [] => |e| e.id.0.clone();
            vertices: Vertex, "Vertex arena.", [] => |e| e.id.0.clone();
            points: Point, "Point arena.", [] => |e| e.id.0.clone();
            surfaces: Surface, "Surface carrier arena.", [] => |e| e.id.0.clone();
            curves: Curve, "Curve carrier arena.", [] => |e| e.id.0.clone();
            pcurves: Pcurve, "Pcurve carrier arena.", [] => |e| e.id.0.clone();
            surface_parameterizations: SurfaceParameterization, "Parameter frames for surfaces whose UV convention is known.", [serde(default)] => |e| e.surface.0.clone();
            procedural_surfaces: ProceduralSurface, "Source-native constructions which produced solved surface carriers.", [serde(default)] => |e| e.surface.0.clone();
            procedural_curves: ProceduralCurve, "Native constructions which produced solved curve caches.", [serde(default)] => |e| e.curve.0.clone();
            features: Feature, "Format-neutral construction features.", [serde(default)] => |e| e.id.0.clone();
            sketch_curve_links: SketchCurveLink, "Typed provenance links from sketch curves to generated B-rep coedges.", [serde(default)] => |e| format!("{}:{}", e.coedge.0, e.sketch_curve_id);
            persistent_design_links: PersistentDesignLink, "Persistent Design `BulkStream` identifiers attached to solved B-rep entities.", [serde(default)] => |e| format!("{:?}:{}:{}", e.target, e.design_id, e.ordinal);
            construction_recipes: ConstructionRecipe, "Parametric regeneration recipes from the Design `BulkStream`.", [serde(default)] => |e| format!("{:?}:{:?}:{}", e.kind, e.design_id, e.recipe_index);
            persistent_references: PersistentReference, "Persistent point/curve references from Design construction records.", [serde(default)] => |e| format!("{:?}:{}", e.kind, e.meta.provenance.offset);
            lost_edge_references: LostEdgeReference, "Parametric edge selections that failed source-side re-resolution.", [serde(default)] => |e| format!("{}:{}", e.class_tag, e.record_index);
            design_objects: DesignObject, "GUID-owned Design `MetaStream` objects, including sketches and dimensions.", [serde(default)] => |e| e.self_guid.clone();
            design_entity_headers: DesignEntityHeader, "Self-validating per-entity Design `BulkStream` headers.", [serde(default)] => |e| format!("{}:{}", e.class_tag, e.entity_id);
            design_record_headers: DesignRecordHeader, "Indexed dynamic-class records in the Design `BulkStream`.", [serde(default)] => |e| format!("{}:{}", e.record_index, e.class_tag);
            sketch_relations: SketchRelation, "Typed bidirectional relations owned by sketch containers.", [serde(default)] => |e| e.record_index.to_string();
            sketch_points: SketchPoint, "Persistent source sketch points.", [serde(default)] => |e| e.record_index.to_string();
            sketch_curve_identities: SketchCurveIdentity, "Persistent identities bound to source sketch-curve records.", [serde(default)] => |e| e.record_index.to_string();
            design_body_members: DesignBodyMember, "Native Design `BodiesRoot` membership entries.", [serde(default)] => |e| e.entity_suffix.to_string();
            act_entities: ActEntity, "ACT table entities and their per-channel change-version handles.", [serde(default)] => |e| format!("{}:{}", e.record_index, e.entity_id);
            act_guids: ActGuid, "Ordered stream-wide ACT GUID pool.", [serde(default)] => |e| format!("{}:{}", e.ordinal, e.guid);
            act_root_components: ActRootComponent, "ACT root component and registry links.", [serde(default)] => |e| format!("{}:{}", e.record_index, e.entity_id);
            tessellations: Tessellation, "Source display/facet meshes retained independently of the exact B-rep.", [serde(default)] => |e| e.id.clone();
            feature_histories: FeatureHistory, "Parametric construction histories carried by the source document.", [serde(default)] => |e| format!("{}:{}", e.meta.provenance.stream, e.meta.provenance.offset);
            feature_input_lanes: FeatureInputLane, "Native feature-input streams and typed sketch-record views.", [serde(default)] => |e| e.id.clone();
            asm_histories: AsmHistory, "Raw replayable ASM construction-state graphs.", [serde(default)] => |e| format!("{}:{}", e.meta.provenance.stream, e.meta.provenance.offset);
            appearances: Appearance, "Decoded visual/physical appearance assets.", [serde(default)] => |e| e.id.0.clone();
            appearance_bindings: AppearanceBinding, "Explicit body/face appearance assignments.", [serde(default)] => |e| format!("{:?}:{}", e.target, e.appearance.0);
            attributes: SourceAttribute, "Source-native linked attributes.", [serde(default)] => |e| e.id.0.clone();
            unknowns: UnknownRecord, "Uninterpreted passthrough records.", [serde(default)] => |e| e.id.0.clone();
        }
    };
}
pub(crate) use arena_registry;

macro_rules! define_cad_ir {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
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
            /// Sparse document-wide provenance and exactness tables.
            #[serde(default)]
            pub annotations: Annotations,
            /// Independently versioned source-format namespaces.
            #[serde(default)]
            pub native: Native,
            $(
                #[doc = $doc]
                $(#[$attribute])*
                pub $field: Vec<$element>,
            )*
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
                    annotations: Annotations::default(),
                    native: Native::default(),
                    $($field: Vec::new(),)*
                }
            }

            /// Arena field names in canonical document order.
            pub fn arena_names() -> &'static [&'static str] {
                &[$(stringify!($field)),*]
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
    };
}

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

arena_registry!(define_cad_ir);
