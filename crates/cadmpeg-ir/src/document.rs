// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

use cadmpeg_core::dialect::DialectId;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
use crate::native::Native;
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
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
            bodies: Body, "Body arena.", [];
            regions: Region, "Region arena.", [];
            shells: Shell, "Shell arena.", [];
            faces: Face, "Face arena.", [];
            loops: Loop, "Loop arena.", [];
            coedges: Coedge, "Coedge arena.", [];
            edges: Edge, "Edge arena.", [];
            vertices: Vertex, "Vertex arena.", [];
            points: Point, "Point arena.", [];
            surfaces: Surface, "Surface arena.", [];
            curves: Curve, "Curve arena.", [];
            subds: SubdSurface, "Subdivision surface arena.", [];
            pcurves: Pcurve, "Pcurve arena.", [];
            procedural_surfaces: ProceduralSurface, "Procedural surface arena.", [];
            procedural_curves: ProceduralCurve, "Procedural curve arena.", [];
            assets: crate::assets::Asset, "Embedded and externally referenced document resources.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            features: Feature, "Feature arena.", [];
            feature_input_topologies: FeatureInputTopology, "Feature input-topology arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            feature_result_topologies: FeatureResultTopology, "Feature result-topology arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            configurations: DesignConfiguration, "Design configuration arena.", [serde(default)];
            parameters: DesignParameter, "Design parameter arena.", [serde(default)];
            sketches: Sketch, "Planar sketch arena.", [serde(default)];
            sketch_entities: SketchEntity, "Solved sketch entity arena.", [serde(default)];
            sketch_constraints: SketchConstraint, "Sketch constraint arena.", [serde(default)];
            spatial_sketches: SpatialSketch, "Spatial sketch arena.", [serde(default)];
            spatial_sketch_entities: SpatialSketchEntity, "Solved spatial sketch entity arena.", [serde(default)];
            spatial_sketch_constraints: SpatialSketchConstraint, "Spatial sketch constraint arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            spreadsheets: Spreadsheet, "Spreadsheet arena.", [serde(default)];
            product_definitions: ProductDefinition, "Product definition arena.", [serde(default)];
            occurrences: Occurrence, "Product occurrence arena.", [serde(default)];
            assembly_joints: AssemblyJoint, "Assembly joint arena.", [serde(default)];
            drawings: Drawing, "Drawing page, resource, view, and annotation arena.", [serde(default)];
            semantic_annotations: SemanticAnnotation, "Semantic dimension, note, symbol, and callout arena.", [serde(default)];
            presentation_documents: PresentationDocument, "Document presentation arena.", [serde(default)];
            view_presentations: ViewPresentation, "View-provider presentation arena.", [serde(default)];
            tessellations: Tessellation, "Tessellation arena.", [];
            appearances: Appearance, "Appearance arena.", [];
            appearance_bindings: AppearanceBinding, "Appearance binding arena.", [];
            attributes: SourceAttribute, "Attribute arena.", [];
            pmi: crate::pmi::PmiAnnotation, "Product-manufacturing information arena.", [serde(default)];
            presentation_layers: crate::presentation::PresentationLayer, "Presentation layer arena.", [serde(default)];
        }
    };
}
pub(crate) use arena_registry;

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
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

            /// Returns the identity at one canonical arena slot.
            pub(crate) fn identity_at(
                &self,
                kind: crate::schema::EntityKind,
                index: usize,
            ) -> Option<&str> {
                $(if kind == <$ty as crate::schema::EntitySchema>::KIND {
                    return self
                        .$field
                        .get(index)
                        .map(crate::schema::EntitySchema::identity);
                })*
                None
            }

            /// Total number of admitted neutral entities across all arenas.
            pub fn entity_count(&self) -> usize {
                0 $(+ self.$field.len())*
            }

            /// Visits every typed identity reference in canonical arena order.
            pub fn visit_references(
                &self,
                visitor: &mut dyn FnMut(crate::schema::Reference),
            ) {
                $(for entity in &self.$field {
                    crate::schema::EntitySchema::visit_references(entity, visitor);
                })*
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(self.$field.sort_by(|left, right| {
                    crate::schema::EntitySchema::identity(left)
                        .cmp(crate::schema::EntitySchema::identity(right))
                });)*
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

macro_rules! assert_entity_schemas {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        const _: fn() = || {
            fn assert_schema<T: crate::schema::EntitySchema>() {}
            $(assert_schema::<$ty>();)*
        };
    };
}

/// Per-entity rewrite applied by [`Model::extend_rewritten`].
pub trait EntityRewrite {
    /// Failure raised while rewriting one entity.
    type Error;

    /// Rewrite one arena entity.
    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Every model arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: Vec<&'a $ty>,)*
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedModel<'_> {
                SortedModel {
                    $($field: sorted_refs(&self.$field),)*
                }
            }
        }
    };
}

fn sorted_refs<T: crate::schema::EntitySchema>(entities: &[T]) -> Vec<&T> {
    let mut refs = entities.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.identity().cmp(right.identity()));
    refs
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "5";

arena_registry!(declare_model);
arena_registry!(assert_entity_schemas);
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

#[cfg(feature = "schema")]
fn ir_version_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "const": IR_VERSION
    })
}

/// A versioned CAD document.
///
/// Construction state machine: [`crate::draft::ModelDraft`] (mutable, indexed)
/// commits into [`CadIr`] (structurally canonical after [`CadIr::finalize`]);
/// [`crate::ValidationReport`] is produced separately by `validate_neutral` and
/// is not embedded in the document. Plan prose names the draft stage
/// `DocumentDraft`; the runtime type is [`crate::draft::ModelDraft`].
///
/// `model` holds the format-neutral graph. `native` retains typed
/// format-specific product data without changing that graph's semantics.
/// Entity IDs must be globally unique across all document arenas.
/// `ir_version` is constructor- and deserializer-controlled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CadIr {
    /// IR schema version.
    #[serde(deserialize_with = "deserialize_ir_version")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "ir_version_schema"))]
    ir_version: String,
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

    /// IR schema version stamped by constructors and accepted on deserialize.
    pub fn ir_version(&self) -> &str {
        &self.ir_version
    }

    /// Serialize a finalized, identity-sorted view as pretty JSON.
    ///
    /// Clones and [`finalize`](Self::finalize)s so callers need not pre-sort.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        let mut canonical = self.clone();
        canonical.finalize();
        serde_json::to_string_pretty(&canonical)
    }

    /// Parse JSON and reject any unsupported `ir_version`.
    ///
    /// Version is probed first (`ir_version` only) so the full document is
    /// materialized once after the version gate.
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

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> std::collections::BTreeMap<String, usize> {
        entity_census(self)
    }

    /// Test-only: stamp a non-current `ir_version` for version-check coverage.
    #[cfg(test)]
    pub(crate) fn set_ir_version_for_test(&mut self, version: impl Into<String>) {
        self.ir_version = version.into();
    }
}

macro_rules! define_registered_entity_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        fn registered_entity_census(ir: &CadIr) -> BTreeMap<String, usize> {
            BTreeMap::from([
                $((stringify!($field).into(), ir.model.$field.len())),*
            ])
        }
    };
}
arena_registry!(define_registered_entity_census);

/// Count the records represented by the IR arenas without running validation.
pub fn entity_census(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = registered_entity_census(ir);
    counts.insert(
        "surfaces_unknown_geometry".into(),
        ir.model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry,
                    crate::geometry::SurfaceGeometry::Unknown { .. }
                )
            })
            .count(),
    );
    for loss in ir.native.loss_counts() {
        counts.insert(format!("native.{}.{}", loss.format, loss.kind), loss.count);
    }
    counts
}

/// Source-container metadata preserved for reporting.
///
/// Attribute keys ending in [`cadmpeg_ir::compare::LOCAL_DIGEST_SUFFIX`] hold
/// machine-local digests over decoded content for the write-path edit oracle.
/// Not portable across platforms; not tolerance-aware. Digests over retained
/// source bytes must not use that suffix. See
/// [`crate::hash::document_local_sha256`] and
/// [`cadmpeg_ir::compare::is_local_digest_attribute`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SourceMeta {
    /// Source format id.
    pub format: String,
    /// Format-specific attributes.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    /// Primary-layer dialect of the source document.
    ///
    /// Mirrors the [`cadmpeg_core::dialect::DialectMatch`] entry whose `format`
    /// equals [`Self::format`], so the round-trip default survives a CADIR
    /// intermediate.
    ///
    /// `None` on a synthetic document that no decode produced, and on a decode
    /// whose primary layer the registry does not name. A decode-produced
    /// document mirrors its report's primary layer exactly; `DecodeResult::new`
    /// debug-asserts that.
    ///
    /// Always serialized, as `null` when absent. Documents written before the
    /// field existed omit the key and read back `None`.
    #[serde(default)]
    pub dialect: Option<DialectId>,
    /// Version fields the source declared, verbatim, for the primary layer.
    ///
    /// Evidence, not a control input. Neutral model semantics never branch on
    /// it; [`Self::attributes`] keeps its other duties.
    ///
    /// Always serialized. Documents written before the field existed omit the
    /// key and read back empty.
    #[serde(default)]
    pub declared: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests;
