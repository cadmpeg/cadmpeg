// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{DesignConfiguration, DesignParameter, Feature, FeatureInputTopology};
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
use crate::native::Native;
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Component, Occurrence};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::units::{Tolerances, Units};
use crate::unknown::{NativeUnknownRecord, UnknownRecord};

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
            feature_input_topologies: FeatureInputTopology, "Feature input-topology arena.", [serde(default, skip_serializing_if = "Vec::is_empty")] => |e| e.id.0.clone();
            configurations: DesignConfiguration, "Design configuration arena.", [serde(default)] => |e| e.id.0.clone();
            parameters: DesignParameter, "Design parameter arena.", [serde(default)] => |e| e.id.0.clone();
            sketches: Sketch, "Planar sketch arena.", [serde(default)] => |e| e.id.0.clone();
            sketch_entities: SketchEntity, "Solved sketch entity arena.", [serde(default)] => |e| e.id.0.clone();
            sketch_constraints: SketchConstraint, "Sketch constraint arena.", [serde(default)] => |e| e.id.0.clone();
            spatial_sketches: SpatialSketch, "Spatial sketch arena.", [serde(default)] => |e| e.id.0.clone();
            spatial_sketch_entities: SpatialSketchEntity, "Solved spatial sketch entity arena.", [serde(default)] => |e| e.id.0.clone();
            spatial_sketch_constraints: SpatialSketchConstraint, "Spatial sketch constraint arena.", [serde(default, skip_serializing_if = "Vec::is_empty")] => |e| e.id.0.clone();
            spreadsheets: Spreadsheet, "Spreadsheet arena.", [serde(default)] => |e| e.id.0.clone();
            components: Component, "Product component arena.", [serde(default)] => |e| e.id.0.clone();
            occurrences: Occurrence, "Product occurrence arena.", [serde(default)] => |e| e.id.0.clone();
            assembly_joints: AssemblyJoint, "Assembly joint arena.", [serde(default)] => |e| e.id.0.clone();
            drawings: Drawing, "Drawing page, resource, view, and annotation arena.", [serde(default)] => |e| e.id.0.clone();
            semantic_annotations: SemanticAnnotation, "Semantic dimension, note, symbol, and callout arena.", [serde(default)] => |e| e.id.0.clone();
            presentation_documents: PresentationDocument, "Document presentation arena.", [serde(default)] => |e| e.id.0.clone();
            view_presentations: ViewPresentation, "View-provider presentation arena.", [serde(default)] => |e| e.id.0.clone();
            tessellations: Tessellation, "Tessellation arena.", [] => |e| e.id.clone();
            appearances: Appearance, "Appearance arena.", [] => |e| e.id.0.clone();
            appearance_bindings: AppearanceBinding, "Appearance binding arena.", [] => |e| e.id.clone();
            attributes: SourceAttribute, "Attribute arena.", [] => |e| e.id.0.clone();
            products: crate::product::Product, "Product prototype arena.", [serde(default)] => |e| e.id.0.clone();
            product_occurrences: crate::product::ProductOccurrence, "Placed product occurrence arena.", [serde(default)] => |e| e.id.0.clone();
            pmi: crate::pmi::PmiAnnotation, "Product-manufacturing information arena.", [serde(default)] => |e| e.id.0.clone();
            presentation_layers: crate::presentation::PresentationLayer, "Presentation layer arena.", [serde(default)] => |e| e.id.0.clone();
        }
    };
}
pub(crate) use arena_registry;

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] => $key:expr;)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
        pub struct Model {
            $(
                $(#[$attribute])*
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
        }

        impl Model {
            /// Arena field names in canonical order.
            pub fn arena_names() -> &'static [&'static str] {
                &[$(stringify!($field)),*]
            }

            /// Whether any arena holds an entity whose identity equals `id`.
            ///
            /// Entity IDs are globally unique across arenas, so a single hit is
            /// authoritative. Used by transfer accounting to confirm that a
            /// `Typed` disposition names entities actually emitted into the IR.
            pub fn contains_id(&self, id: &str) -> bool {
                $({
                    let key = ($key) as fn(&$ty) -> String;
                    if self.$field.iter().any(|e| key(e) == id) { return true; }
                })*
                false
            }

            /// Every entity identity across all arenas, in canonical arena
            /// order. Derived from the same `arena_registry!` declaration as
            /// [`contains_id`](Self::contains_id), so a new arena is covered
            /// without editing per-arena call sites.
            pub fn entity_ids(&self) -> Vec<String> {
                let mut ids = Vec::new();
                $( ids.extend(self.$field.iter().map($key)); )*
                ids
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(self.$field.sort_by_key($key);)*
            }
        }
    };
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "56";

/// Immediately preceding IR version supported by the explicit JSON migration.
pub const PREVIOUS_IR_VERSION: &str = "55";

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
/// `model` holds the format-neutral graph. `native` retains typed
/// format-specific product data without changing that graph's semantics.
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
    /// Independently versioned native namespaces.
    #[serde(default)]
    pub native: Native,
}

impl CadIr {
    /// Deserialize the reserved `unknowns` arena for `format`.
    pub fn native_unknowns(
        &self,
        format: &str,
    ) -> Result<Vec<NativeUnknownRecord>, crate::native::NativeConvertError> {
        self.native.namespace(format).map_or_else(
            || Ok(Vec::new()),
            |namespace| namespace.arena_as("unknowns"),
        )
    }

    /// Deserialize every reserved native `unknowns` arena.
    pub fn all_native_unknowns(
        &self,
    ) -> Result<Vec<NativeUnknownRecord>, crate::native::NativeConvertError> {
        self.native
            .0
            .values()
            .filter(|namespace| namespace.arenas.contains_key("unknowns"))
            .try_fold(Vec::new(), |mut records, namespace| {
                records.extend(namespace.arena_as::<NativeUnknownRecord>("unknowns")?);
                Ok(records)
            })
    }

    /// Replace the reserved `unknowns` arena for `format`.
    pub fn set_native_unknowns(
        &mut self,
        format: &str,
        records: &[NativeUnknownRecord],
    ) -> Result<(), crate::native::NativeConvertError> {
        let namespace = self.native.namespace_mut(format);
        if namespace.version == 0 {
            namespace.version = 1;
        }
        namespace.set_arena("unknowns", records)
    }

    /// Replace the reserved `unknowns` arena for `format`, consuming the records.
    ///
    /// Codecs retaining large source populations should use this form to avoid
    /// keeping typed and generic native copies alive at the same time.
    pub fn set_native_unknowns_owned(&mut self, format: &str, records: Vec<UnknownRecord>) {
        let namespace = self.native.namespace_mut(format);
        if namespace.version == 0 {
            namespace.version = 1;
        }
        let mut converted = records
            .into_iter()
            .map(UnknownRecord::into_native_record)
            .collect::<Vec<_>>();
        converted.sort_by(|left, right| left.id.cmp(&right.id));
        namespace.arenas.insert("unknowns".into(), converted);
    }

    /// Append one record to the reserved `unknowns` arena for `format`.
    pub fn push_native_unknown(
        &mut self,
        format: &str,
        record: NativeUnknownRecord,
    ) -> Result<(), crate::native::NativeConvertError> {
        let mut records = self.native_unknowns(format)?;
        records.retain(|existing| existing.id != record.id);
        records.push(record);
        self.set_native_unknowns(format, &records)
    }

    /// Construct an empty current-version document with default tolerances.
    pub fn empty(units: Units) -> Self {
        Self {
            ir_version: IR_VERSION.to_owned(),
            source: None,
            units,
            tolerances: Tolerances::default(),
            model: Model::default(),
            native: Native::default(),
        }
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

    /// Migrate JSON from the immediately preceding IR version and parse it.
    ///
    /// The immediately preceding version is upgraded, including structural
    /// normalization required by the current schema. Older versions remain
    /// unsupported.
    pub fn migrate_json(s: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(s)?;
        let version = value.get("ir_version").and_then(serde_json::Value::as_str);
        match version {
            Some(IR_VERSION) => serde_json::from_value(value),
            Some(PREVIOUS_IR_VERSION) => {
                migrate_previous_extents(&mut value);
                value
                    .as_object_mut()
                    .expect("a versioned CADIR document is a JSON object")
                    .insert("ir_version".into(), IR_VERSION.into());
                serde_json::from_value(value)
            }
            _ => Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "cannot migrate ir_version {version:?}; expected {PREVIOUS_IR_VERSION} or {IR_VERSION}"
            ))),
        }
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
    }
}

fn migrate_previous_extents(value: &mut serde_json::Value) {
    let Some(model) = value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    if let Some(features) = model
        .get_mut("features")
        .and_then(serde_json::Value::as_array_mut)
    {
        for feature in features {
            if let Some(definition) = feature.get_mut("definition") {
                migrate_previous_extent_definition(definition);
            }
        }
    }
    if let Some(configurations) = model
        .get_mut("configurations")
        .and_then(serde_json::Value::as_array_mut)
    {
        for state in configurations
            .iter_mut()
            .filter_map(|configuration| configuration.get_mut("feature_states"))
            .filter_map(serde_json::Value::as_object_mut)
            .flat_map(|states| states.values_mut())
        {
            if let Some(definition) = state.get_mut("definition") {
                migrate_previous_extent_definition(definition);
            }
        }
    }
}

/// The sidedness of a version-55 extent, with per-side termination laws.
enum PreviousExtent {
    OneSided(serde_json::Value),
    TwoSided(serde_json::Value, serde_json::Value),
    Symmetric(serde_json::Value),
}

/// Splits a version-55 composite extent into sidedness plus one-sided
/// termination laws. A law kind passes through as the side's termination.
fn split_previous_extent(extent: serde_json::Value) -> PreviousExtent {
    let kind = extent
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let blind = |length: Option<&serde_json::Value>| serde_json::json!({"kind": "blind", "length": length.cloned().unwrap_or(serde_json::Value::Null)});
    let angle = |angle: Option<&serde_json::Value>| serde_json::json!({"kind": "angle", "angle": angle.cloned().unwrap_or(serde_json::Value::Null)});
    match kind {
        "symmetric" => PreviousExtent::Symmetric(blind(extent.get("length"))),
        "symmetric_angle" => PreviousExtent::Symmetric(angle(extent.get("angle"))),
        "symmetric_extent" => PreviousExtent::Symmetric(
            extent
                .get("extent")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        ),
        "two_sided" => {
            PreviousExtent::TwoSided(blind(extent.get("first")), blind(extent.get("second")))
        }
        "two_sided_angles" => {
            PreviousExtent::TwoSided(angle(extent.get("first")), angle(extent.get("second")))
        }
        "two_sided_extents" => PreviousExtent::TwoSided(
            extent
                .get("first")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            extent
                .get("second")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        ),
        "through_all_both" => PreviousExtent::TwoSided(
            serde_json::json!({"kind": "through_all"}),
            serde_json::json!({"kind": "through_all"}),
        ),
        _ => PreviousExtent::OneSided(extent),
    }
}

/// A version-55 extrusion side from its termination law and side modifiers.
fn previous_extrude_side(
    termination: serde_json::Value,
    draft: Option<serde_json::Value>,
    offset: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut side = serde_json::Map::new();
    side.insert("termination".into(), termination);
    if let Some(draft) = draft {
        side.insert("draft".into(), draft);
    }
    if let Some(offset) = offset {
        side.insert("offset".into(), offset);
    }
    serde_json::Value::Object(side)
}

fn migrate_previous_extent_definition(definition: &mut serde_json::Value) {
    let Some(definition) = definition.as_object_mut() else {
        return;
    };
    match definition
        .get("definition")
        .and_then(serde_json::Value::as_str)
    {
        Some("extrude") => {
            let Some(extent) = definition.remove("extent") else {
                return;
            };
            let draft = definition.remove("draft");
            let second_draft = definition
                .remove("second_draft")
                .or_else(|| definition.remove("reverse_draft"));
            let first_offset = definition.remove("first_offset");
            let second_offset = definition.remove("second_offset");
            let extent = match split_previous_extent(extent) {
                // Second-side modifiers alongside a one-sided extent had no
                // side to act on; they are dropped.
                PreviousExtent::OneSided(termination) => serde_json::json!({
                    "kind": "one_sided",
                    "side": previous_extrude_side(termination, draft, first_offset),
                }),
                PreviousExtent::TwoSided(first, second) => serde_json::json!({
                    "kind": "two_sided",
                    "first": previous_extrude_side(first, draft, first_offset),
                    "second": previous_extrude_side(second, second_draft, second_offset),
                }),
                // A mirrored side carries one draft; version 55 stated a
                // symmetric taper as equal first- and second-side drafts.
                PreviousExtent::Symmetric(termination) => serde_json::json!({
                    "kind": "symmetric",
                    "side": previous_extrude_side(
                        termination,
                        draft.or(second_draft),
                        first_offset,
                    ),
                }),
            };
            definition.insert("extent".into(), extent);
        }
        Some("revolve") => {
            let Some(construction) = definition
                .get_mut("construction")
                .and_then(serde_json::Value::as_object_mut)
            else {
                return;
            };
            let Some(extent) = construction.remove("extent") else {
                return;
            };
            let extent = match split_previous_extent(extent) {
                PreviousExtent::OneSided(termination) => serde_json::json!({
                    "kind": "one_sided",
                    "termination": termination,
                }),
                PreviousExtent::TwoSided(first, second) => serde_json::json!({
                    "kind": "two_sided",
                    "first": first,
                    "second": second,
                }),
                PreviousExtent::Symmetric(termination) => serde_json::json!({
                    "kind": "symmetric",
                    "termination": termination,
                }),
            };
            construction.insert("extent".into(), extent);
        }
        // Hole extents were already one-sided termination laws; their JSON is
        // unchanged.
        _ => {}
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
