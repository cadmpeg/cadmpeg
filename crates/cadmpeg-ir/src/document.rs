// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::annotations::Annotations;
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::features::Feature;
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
use crate::native::Native;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::units::{Tolerances, Units};
use crate::unknown::UnknownRecord;

macro_rules! arena_registry {
    ($macro:ident) => {
        $macro! {
            bodies: Body, "Body arena.", [] => |e| e.id.0.clone();
            regions: Region, "Region arena.", [] => |e| e.id.0.clone();
            shells: Shell, "Shell arena.", [] => |e| e.id.0.clone();
            faces: Face, "Face arena.", [] => |e| e.id.0.clone();
            loops: Loop, "Loop arena.", [] => |e| e.id.0.clone();
            coedges: Coedge, "Coedge arena.", [] => |e| e.id.0.clone();
            edges: Edge, "Edge arena.", [] => |e| e.id.0.clone();
            vertices: Vertex, "Vertex arena.", [] => |e| e.id.0.clone();
            points: Point, "Point arena.", [] => |e| e.id.0.clone();
            surfaces: Surface, "Surface arena.", [] => |e| e.id.0.clone();
            curves: Curve, "Curve arena.", [] => |e| e.id.0.clone();
            subds: SubdSurface, "Subdivision surface arena.", [] => |e| e.id.0.clone();
            pcurves: Pcurve, "Pcurve arena.", [] => |e| e.id.0.clone();
            procedural_surfaces: ProceduralSurface, "Procedural surface arena.", [] => |e| e.id.0.clone();
            procedural_curves: ProceduralCurve, "Procedural curve arena.", [] => |e| e.id.0.clone();
            features: Feature, "Feature arena.", [] => |e| e.id.0.clone();
            tessellations: Tessellation, "Tessellation arena.", [] => |e| e.id.clone();
            appearances: Appearance, "Appearance arena.", [] => |e| e.id.0.clone();
            appearance_bindings: AppearanceBinding, "Appearance binding arena.", [] => |e| e.id.clone();
            attributes: SourceAttribute, "Attribute arena.", [] => |e| e.id.0.clone();
        }
    };
}
pub(crate) use arena_registry;

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, $attrs:tt => $key:expr;)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
        pub struct Model {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
        }

        impl Model {
            /// Arena field names in canonical order.
            pub fn arena_names() -> &'static [&'static str] {
                &[$(stringify!($field)),*]
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(self.$field.sort_by_key($key);)*
            }
        }
    };
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "2";

arena_registry!(declare_model);

fn deserialize_ir_version<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let version = String::deserialize(deserializer)?;
    if version != IR_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported ir_version {version:?}; expected {IR_VERSION}"
        )));
    }
    Ok(version)
}

fn ir_version_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "const": IR_VERSION
    })
}

/// A versioned CAD document.
///
/// `model` holds the format-neutral graph. `annotations`, `native`, and
/// `unknowns` retain source fidelity without changing that graph's semantics.
/// Entity IDs must be globally unique across all document arenas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CadIr {
    /// IR schema version.
    #[serde(deserialize_with = "deserialize_ir_version")]
    #[schemars(schema_with = "ir_version_schema")]
    pub ir_version: String,
    /// Source-container metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceMeta>,
    /// Canonical unit declaration.
    pub units: Units,
    /// Document-wide tolerances.
    pub tolerances: Tolerances,
    /// Format-neutral model.
    pub model: Model,
    /// Sparse provenance and exactness tables.
    #[serde(default)]
    pub annotations: Annotations,
    /// Independently versioned native namespaces.
    #[serde(default)]
    pub native: Native,
    /// Recognized source records without a typed interpretation.
    #[serde(default)]
    pub unknowns: Vec<UnknownRecord>,
}

impl CadIr {
    /// Construct an empty current-version document with default tolerances.
    pub fn empty(units: Units) -> Self {
        Self {
            ir_version: IR_VERSION.to_owned(),
            source: None,
            units,
            tolerances: Tolerances::default(),
            model: Model::default(),
            annotations: Annotations::default(),
            native: Native::default(),
            unknowns: Vec::new(),
        }
    }

    /// Neutral model arena names in canonical order.
    pub fn arena_names() -> &'static [&'static str] {
        Model::arena_names()
    }

    /// Serialize the document as pretty JSON.
    ///
    /// Call [`CadIr::finalize`] first when canonical arena order is required.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse JSON and reject any unsupported `ir_version`.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(s)?;
        let version = value.get("ir_version").and_then(serde_json::Value::as_str);
        if version != Some(IR_VERSION) {
            return Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "unsupported ir_version {version:?}; expected {IR_VERSION}"
            )));
        }
        serde_json::from_value(value)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
        self.unknowns.sort_by(|left, right| left.id.cmp(&right.id));
    }
}

/// Source-container metadata preserved for reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceMeta {
    /// Source format id.
    pub format: String,
    /// Format-specific attributes.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}
