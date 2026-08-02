// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

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
            assets: crate::assets::Asset, "Embedded and externally referenced document resources.", [serde(default, skip_serializing_if = "Vec::is_empty")] => |e| e.id.0.clone();
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

            /// Total number of admitted neutral entities across all arenas.
            pub fn entity_count(&self) -> usize {
                0 $(+ self.$field.len())*
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(self.$field.sort_by_key($key);)*
            }

            /// Append every arena of `other` onto the matching arena of this
            /// model, passing each entity through `rewrite`.
            ///
            /// Derived from the same `arena_registry!` declaration as
            /// [`finalize`](Self::finalize), so a new arena is merged without
            /// editing any call site. One entity is handed to `rewrite` at a
            /// time, which bounds a rewriting caller's transient storage by the
            /// largest single entity rather than by the whole model.
            pub fn extend_rewritten<R: EntityRewrite>(
                &mut self,
                other: Self,
                rewrite: &mut R,
            ) -> Result<(), R::Error> {
                $(
                    self.$field.reserve(other.$field.len());
                    for entity in other.$field {
                        self.$field.push(rewrite.rewrite(entity)?);
                    }
                )*
                Ok(())
            }
        }
    };
}

/// Per-entity rewrite applied by [`Model::extend_rewritten`].
///
/// The method is generic over the entity type because each arena holds its own,
/// so an implementation reaches every arena through one bound rather than one
/// method per arena. Entities are serializable, which lets a rewrite that is
/// uniform over the serialized shape — such as rescoping every identity string
/// in a subtree — run without a per-type traversal.
pub trait EntityRewrite {
    /// Failure raised while rewriting one entity.
    type Error;

    /// Rewrite one arena entity.
    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] => $key:expr;)*) => {
        /// Every model arena borrowed in canonical identity order.
        ///
        /// Serializes exactly as a [`Model`] that [`Model::finalize`] has
        /// already ordered, so canonical bytes can be produced from a shared
        /// reference. A whole-model copy is what makes a document digest cost a
        /// second resident copy of the largest arenas; this view costs one
        /// pointer per entity instead.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $(
                $(#[$attribute])*
                $field: Vec<&'a $ty>,
            )*
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedModel<'_> {
                SortedModel {
                    $($field: sorted_refs(&self.$field, $key),)*
                }
            }
        }
    };
}

/// Borrow `entities` in the order [`Model::finalize`] would put them in.
fn sorted_refs<T>(entities: &[T], key: impl Fn(&T) -> String) -> Vec<&T> {
    let mut refs = entities.iter().collect::<Vec<_>>();
    refs.sort_by_key(|entity| key(entity));
    refs
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "5";

arena_registry!(declare_model);
arena_registry!(declare_model_view);

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
        self.native_unknowns_iter(format).collect()
    }

    /// Deserialize the reserved `unknowns` arena for `format` one record at a
    /// time.
    ///
    /// Retained source populations reach hundreds of thousands of records, so a
    /// caller that rebuilds the arena — reducing it for hashing, say — should
    /// consume this rather than [`native_unknowns`](Self::native_unknowns) and
    /// convert each record before the next is read, which keeps the typed
    /// population and the rebuilt one from ever coexisting.
    pub fn native_unknowns_iter<'a>(
        &'a self,
        format: &str,
    ) -> impl Iterator<Item = Result<NativeUnknownRecord, crate::native::NativeConvertError>> + 'a
    {
        self.native
            .namespace(format)
            .into_iter()
            .flat_map(|namespace| namespace.arena_iter_as("unknowns"))
    }

    /// Deserialize every reserved native `unknowns` arena one record at a time.
    ///
    /// Each record is converted as it is read, so a caller that only scans the
    /// population — collecting link targets or record ids — keeps just what it
    /// retains resident rather than the whole typed population at once.
    pub fn all_native_unknowns_iter(
        &self,
    ) -> impl Iterator<Item = Result<NativeUnknownRecord, crate::native::NativeConvertError>> + '_
    {
        self.native
            .0
            .values()
            .flat_map(|namespace| namespace.arena_iter_as("unknowns"))
    }

    /// Replace the reserved `unknowns` arena for `format`.
    pub fn set_native_unknowns(
        &mut self,
        format: &str,
        records: &[NativeUnknownRecord],
    ) -> Result<(), crate::native::NativeConvertError> {
        self.set_native_unknowns_from(format, records.iter())
    }

    /// Replace the reserved `unknowns` arena for `format` one record at a time.
    ///
    /// A caller that derives its product references from a larger population
    /// should derive them inside the iterator: each reference is serialized as
    /// it is produced, so the derived population is never resident alongside the
    /// arena built from it.
    pub fn set_native_unknowns_from<T: Serialize, I: IntoIterator<Item = T>>(
        &mut self,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        self.unknowns_namespace_mut(format)
            .set_arena_from("unknowns", records)
    }

    /// Replace the reserved `unknowns` arena for `format`, consuming the records.
    ///
    /// Codecs retaining large source populations should use this form to avoid
    /// keeping typed and generic native copies alive at the same time.
    pub fn set_native_unknowns_owned(&mut self, format: &str, records: Vec<UnknownRecord>) {
        let namespace = self.unknowns_namespace_mut(format);
        let mut converted = records
            .into_iter()
            .map(UnknownRecord::into_native_record)
            .collect::<Vec<_>>();
        converted.sort_by(|left, right| left.id().cmp(right.id()));
        namespace.arenas.insert("unknowns".into(), converted);
    }

    /// Return the `format` namespace, versioning a newly created one.
    fn unknowns_namespace_mut(&mut self, format: &str) -> &mut crate::native::NativeNamespace {
        let namespace = self.native.namespace_mut(format);
        if namespace.version == 0 {
            namespace.version = 1;
        }
        namespace
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
    ///
    /// The version is read by a probe that names `ir_version` and nothing else,
    /// which serde skips past without building. Reading the version out of a
    /// [`serde_json::Value`] of the whole document instead would hold that tree
    /// — an allocation per member at every depth — for as long as it takes to
    /// build the typed document from it, so a load would peak at both. The
    /// document text is scanned twice and materialized once.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        /// Every member but `ir_version` is skipped, and the version stays a
        /// [`serde_json::Value`] so a non-string one is reported as an
        /// unsupported version rather than as a type error.
        #[derive(Deserialize)]
        struct VersionProbe {
            ir_version: Option<serde_json::Value>,
        }

        let probe = serde_json::from_str::<VersionProbe>(s)?;
        let version = probe
            .ir_version
            .as_ref()
            .and_then(serde_json::Value::as_str);
        if version != Some(IR_VERSION) {
            return Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "unsupported ir_version {version:?}; expected {IR_VERSION}"
            )));
        }
        serde_json::from_str(s)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
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
