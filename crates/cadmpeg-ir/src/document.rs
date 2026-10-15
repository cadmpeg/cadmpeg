// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
    Deserialize, Deserializer, Serialize, Serializer,
};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch, FormatIdentity};
use cadmpeg_core::CodecError;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
    FeatureRowWire, FeatureWriteWire,
};
use crate::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, ProceduralCurve, ProceduralCurveRow, ProceduralSurface,
    ProceduralSurfaceRow, Surface, SurfaceGeometry,
};
use crate::hash::finite_json::CanonicalJsonError;
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
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
use crate::units::{CanonicalUnitsWire, Tolerances};
use crate::unknown::NativeUnknownRecord;
use cadmpeg_core::text::NonBlankString;

pub(crate) mod census;
pub(crate) mod feature_parents;
pub mod procedural;

struct UnknownProjection<T>(T);

impl<T: Borrow<NativeUnknownRecord>> Serialize for UnknownProjection<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.borrow().serialize(serializer)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FeatureRegenerationParents(
    BTreeMap<crate::features::FeatureId, crate::features::FeatureId>,
);

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
            surfaces: Surface, "Surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Surface>"))];
            curves: Curve, "Curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Curve>"))];
            subds: SubdSurface, "Subdivision surface arena.", [];
            pcurves: Pcurve, "Pcurve arena.", [];
            procedural_surfaces: ProceduralSurface, "Procedural surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralSurface>"))];
            procedural_curves: ProceduralCurve, "Procedural curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralCurve>"))];
            assets: crate::assets::Asset, "Embedded and externally referenced document resources.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            features: Feature, "Feature arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<FeatureRowWire>"))];
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

struct SurfaceWire<'a>(&'a Surface);

impl Serialize for SurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Surface", 3)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", cache)?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        state.end()
    }
}

struct CurveWire<'a>(&'a Curve);

impl Serialize for CurveWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Curve", 4)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", cache)?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(range) = &self.0.parameter_range {
            state.serialize_field("parameter_range", range)?;
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        state.end()
    }
}

struct ProceduralSurfaceWire<'a> {
    owner: Option<&'a SurfaceId>,
    procedural: &'a ProceduralSurface,
}

impl Serialize for ProceduralSurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let owner = self.owner.ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom(format_args!(
                "procedural surface {} has no unique owning surface",
                self.procedural.id
            ))
        })?;
        ProceduralSurfaceRow::new(owner.clone(), self.procedural).serialize(serializer)
    }
}

struct ProceduralCurveWire<'a> {
    owner: Option<&'a CurveId>,
    procedural: &'a ProceduralCurve,
}

impl Serialize for ProceduralCurveWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let owner = self.owner.ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom(format_args!(
                "procedural curve {} has no unique owning curve",
                self.procedural.id
            ))
        })?;
        ProceduralCurveRow::new(owner.clone(), self.procedural).serialize(serializer)
    }
}

macro_rules! model_write_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
    (features, $ty:ty, $l:lifetime) => { Vec<FeatureWriteWire<$l>> };
    ($field:ident, $ty:ty, $l:lifetime) => { &$l Vec<$ty> };
}

macro_rules! model_write_value {
    ($model:expr, surfaces) => {
        $model.surfaces.iter().map(SurfaceWire).collect()
    };
    ($model:expr, curves) => {
        $model.curves.iter().map(CurveWire).collect()
    };
    ($model:expr, procedural_surfaces) => {{
        if $model.procedural_surfaces.is_empty() {
            Vec::new()
        } else {
            let owners =
                procedural::standard_owners($model.surfaces.iter().filter_map(|carrier| {
                    carrier
                        .geometry
                        .procedural_construction()
                        .map(|construction| (construction.as_str(), &carrier.id))
                }));
            $model
                .procedural_surfaces
                .iter()
                .map(|procedural| ProceduralSurfaceWire {
                    owner: owners.get(procedural.id.as_str()).copied().flatten(),
                    procedural,
                })
                .collect()
        }
    }};
    ($model:expr, procedural_curves) => {{
        if $model.procedural_curves.is_empty() {
            Vec::new()
        } else {
            let owners = procedural::standard_owners($model.curves.iter().filter_map(|carrier| {
                carrier
                    .geometry
                    .procedural_construction()
                    .map(|construction| (construction.as_str(), &carrier.id))
            }));
            $model
                .procedural_curves
                .iter()
                .map(|procedural| ProceduralCurveWire {
                    owner: owners.get(procedural.id.as_str()).copied().flatten(),
                    procedural,
                })
                .collect()
        }
    }};
    ($model:expr, features) => {
        $model
            .features
            .iter()
            .map(|feature| {
                FeatureWriteWire::new(feature, $model.feature_regeneration_parent(&feature.id))
            })
            .collect()
    };
    ($model:expr, $field:ident) => {
        &$model.$field
    };
}

macro_rules! model_read_type {
    (procedural_surfaces, $ty:ty) => { Vec<ProceduralSurfaceRow> };
    (procedural_curves, $ty:ty) => { Vec<ProceduralCurveRow> };
    (features, $ty:ty) => { Vec<FeatureRowWire> };
    ($field:ident, $ty:ty) => { Vec<$ty> };
}

macro_rules! model_read_value {
    ($wire:expr, procedural_surfaces) => {
        Vec::new()
    };
    ($wire:expr, procedural_curves) => {
        Vec::new()
    };
    ($wire:expr, features) => {
        Vec::new()
    };
    ($wire:expr, $field:ident) => {
        std::mem::take(&mut $wire.$field)
    };
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureRegenerationEdge {
    child: crate::features::FeatureId,
    parent: crate::features::FeatureId,
}

macro_rules! sorted_model_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
    (features, $ty:ty, $l:lifetime) => { Vec<FeatureWriteWire<$l>> };
    ($field:ident, $ty:ty, $l:lifetime) => { Vec<&$l $ty> };
}

macro_rules! sorted_model_value {
    ($model:expr, surfaces) => {
        sorted_rows(&$model.surfaces, |value| SurfaceWire(value))
    };
    ($model:expr, curves) => {
        sorted_rows(&$model.curves, |value| CurveWire(value))
    };
    ($model:expr, procedural_surfaces) => {
        if $model.procedural_surfaces.is_empty() {
            Vec::new()
        } else {
            let owners =
                crate::hash::procedural_owners($model.surfaces.iter().filter_map(|surface| {
                    surface
                        .geometry
                        .procedural_construction()
                        .map(|construction| (construction.as_str(), &surface.id))
                }));
            sorted_rows(&$model.procedural_surfaces, |procedural| {
                ProceduralSurfaceWire {
                    owner: owners.get(procedural.id.as_str()),
                    procedural,
                }
            })
        }
    };
    ($model:expr, procedural_curves) => {
        if $model.procedural_curves.is_empty() {
            Vec::new()
        } else {
            let owners = crate::hash::procedural_owners($model.curves.iter().filter_map(|curve| {
                curve
                    .geometry
                    .procedural_construction()
                    .map(|construction| (construction.as_str(), &curve.id))
            }));
            sorted_rows(&$model.procedural_curves, |procedural| {
                ProceduralCurveWire {
                    owner: owners.get(procedural.id.as_str()),
                    procedural,
                }
            })
        }
    };
    ($model:expr, features) => {
        sorted_rows(&$model.features, |feature| {
            FeatureWriteWire::new(feature, $model.feature_regeneration_parent(&feature.id))
        })
    };
    ($model:expr, $field:ident) => {
        sorted_rows(&$model.$field, |value| value)
    };
}

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct Model {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
            pub(crate) feature_regeneration_parents: FeatureRegenerationParents,
        }

        #[cfg(feature = "schema")]
        impl JsonSchema for Model {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                "Model".into()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                concat!(module_path!(), "::Model").into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let mut schema = ModelReadWire::json_schema(generator);
                schema.ensure_object().remove("additionalProperties");
                schema.ensure_object().insert(
                    "description".into(),
                    "Format-neutral entity arenas connected by typed IDs.".into(),
                );
                schema
            }
        }

        #[derive(Serialize)]
        struct ModelWriteWire<'a> {
            $(
                $(#[$attribute])*
                $field: model_write_type!($field, $ty, 'a),
            )*
        }

        #[derive(Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(rename = "Model"))]
        #[serde(deny_unknown_fields)]
        struct ModelReadWire {
            $(
                $(#[$attribute])*
                $($(#[$schema_attr])*)?
                #[doc = $doc]
                $field: model_read_type!($field, $ty),
            )*
        }

        impl Serialize for Model {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                feature_parents::validate(&[self]).map_err(serde::ser::Error::custom)?;
                ModelWriteWire {
                    $($field: model_write_value!(self, $field),)*
                }
                .serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for Model {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let mut wire = ModelReadWire::deserialize(deserializer)?;
                let procedural_surfaces = std::mem::take(&mut wire.procedural_surfaces);
                let procedural_curves = std::mem::take(&mut wire.procedural_curves);
                let feature_wires = std::mem::take(&mut wire.features);
                let mut model = Self {
                    $($field: model_read_value!(wire, $field),)*
                    feature_regeneration_parents: FeatureRegenerationParents::default(),
                };
                for wire in feature_wires {
                    let (feature, parent) = wire.into_parts();
                    if let Some(parent) = parent {
                        model.feature_regeneration_parents.0.insert(feature.id.clone(), parent);
                    }
                    model.features.push(feature);
                }
                feature_parents::validate(&[&model]).map_err(serde::de::Error::custom)?;
                if !procedural_surfaces.is_empty() {
                    let mut admission = procedural::ReconstructedAdmission::new(model.surfaces.iter().map(|carrier| {
                        (carrier.id.as_str(), carrier.geometry.procedural_construction().map(|id| id.as_str()))
                    }));
                    for wire in procedural_surfaces {
                        let (owner, procedural) = wire.into_parts();
                        let slot = admission.owner("surface", owner.as_str(), procedural.id.as_str()).map_err(serde::de::Error::custom)?;
                        admission.accept(procedural.id.as_str());
                        model
                            .attach_procedural_surface(&owner, slot, procedural)
                            .map_err(serde::de::Error::custom)?;
                    }
                }
                if !procedural_curves.is_empty() {
                    let mut admission = procedural::ReconstructedAdmission::new(model.curves.iter().map(|carrier| {
                        (carrier.id.as_str(), carrier.geometry.procedural_construction().map(|id| id.as_str()))
                    }));
                    for wire in procedural_curves {
                        let (owner, procedural) = wire.into_parts();
                        let slot = admission.owner("curve", owner.as_str(), procedural.id.as_str()).map_err(serde::de::Error::custom)?;
                        admission.accept(procedural.id.as_str());
                        model
                            .attach_procedural_curve(&owner, slot, procedural)
                            .map_err(serde::de::Error::custom)?;
                    }
                }
                Ok(model)
            }
        }

        impl Model {
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

            /// Moves every arena and feature-parent relation from another model.
            pub(crate) fn append(&mut self, mut other: Self) {
                $(self.$field.append(&mut other.$field);)*
                self.feature_regeneration_parents.0
                    .append(&mut other.feature_regeneration_parents.0);
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(if !arena_is_ordered(&self.$field) {
                    self.$field.sort_by(|left, right| {
                        crate::schema::EntitySchema::identity(left)
                            .cmp(crate::schema::EntitySchema::identity(right))
                    });
                })*
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
                &mut self, other: Self, rewrite: &mut R,
            ) -> Result<(), ModelRewriteError<R::Error>> {
                $(
                    for entity in other.$field {
                        self.$field.push(rewrite.rewrite(entity).map_err(ModelRewriteError::Rewrite)?);
                    }
                )*
                for (child, parent) in other.feature_regeneration_parents.0 {
                    let edge = rewrite.rewrite(FeatureRegenerationEdge { child, parent }).map_err(ModelRewriteError::Rewrite)?;
                    self.feature_regeneration_parents.0.insert(edge.child, edge.parent);
                }
                Ok(())
            }
        }
    };
}

macro_rules! assert_entity_schemas {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        const _: fn() = || {
            fn assert_schema<T: crate::schema::EntitySchema>() {}
            $(assert_schema::<$ty>();)*
        };
    };
}

/// Refusal while rewriting or admitting model storage.
#[derive(Debug, thiserror::Error)]
pub enum ModelRewriteError<E> {
    /// The entity rewriter refused its input.
    #[error("{0}")]
    Rewrite(E),
    /// The destination storage exceeded its resource limit.
    #[error("resource refusal: {0:?}")]
    Resource(CodecError),
}

impl From<ModelRewriteError<CodecError>> for CodecError {
    fn from(error: ModelRewriteError<CodecError>) -> Self {
        match error {
            ModelRewriteError::Rewrite(error) => error,
            ModelRewriteError::Resource(error) => error,
        }
    }
}

/// Per-entity rewrite applied by [`Model::extend_rewritten`].
pub trait EntityRewrite {
    /// Failure raised while rewriting one entity.
    type Error;

    /// Rewrite one arena entity.
    fn rewrite<T: crate::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Every model arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> Result<SortedModel<'_>, CodecError> {
                if let Err(error) = feature_parents::validate(&[self]) {
                    return Err(CodecError::malformed(format_args!("{error}")));
                }
                Ok(SortedModel { $($field: sorted_model_value!(self, $field),)* })
            }
        }
    };
}

// Public arenas can change between calls. Check their current order instead
// of retaining a flag that direct mutations could leave stale.
fn arena_is_ordered<T: crate::schema::EntitySchema>(values: &[T]) -> bool {
    values
        .windows(2)
        .all(|pair| pair[0].identity() <= pair[1].identity())
}

fn sorted_rows<'a, T: crate::schema::EntitySchema, U>(
    entities: &'a [T],
    project: impl FnMut(&'a T) -> U,
) -> Vec<U> {
    let mut refs = entities.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.identity().cmp(right.identity()));
    refs.into_iter().map(project).collect()
}

macro_rules! declare_arena_name {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Name of a registered model arena.
        ///
        /// Values use the `arena_registry!` field names so a new arena is
        /// counted and diffed under its declared wire key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct ArenaName(&'static str);

        impl ArenaName {
            /// Every registered arena, in registry order.
            pub const ALL: &'static [Self] = &[$(Self(stringify!($field))),*];

            pub(crate) const fn registered(name: &'static str) -> Self {
                Self(name)
            }

            /// Registry field name, which is also the CADIR JSON object key.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                self.0
            }

            /// Parse a registry field name.
            #[must_use]
            pub fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|arena| arena.as_str() == name)
            }
        }

        impl fmt::Display for ArenaName {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl AsRef<str> for ArenaName {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "6";

/// The current IR wire version. Every value writes [`IR_VERSION`].
/// Deserialization refuses any other version.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IrVersion;

arena_registry!(declare_model);
arena_registry!(assert_entity_schemas);
arena_registry!(declare_model_view);
arena_registry!(declare_arena_name);

/// Borrowed geometry and topology arenas for an embedded semantic record.
pub struct GeometrySnapshot<'a> {
    model: &'a Model,
    kind: &'a str,
    curves: ProceduralCurveRows<'a>,
    surfaces: ProceduralSurfaceRows<'a>,
}

impl Model {
    /// Admit parent validation and indexed owners for a borrowed geometry serialization view.
    pub fn geometry_snapshot<'a>(
        &'a self,
        kind: &'a str,
    ) -> Result<GeometrySnapshot<'a>, CodecError> {
        if let Err(error) = feature_parents::validate(&[self]) {
            return Err(CodecError::malformed(format_args!("{error}")));
        }
        let curves = if self.procedural_curves.is_empty() {
            Vec::new()
        } else {
            let owners = crate::hash::procedural_owners(self.curves.iter().filter_map(|carrier| {
                carrier
                    .geometry
                    .procedural_construction()
                    .map(|id| (id.as_str(), &carrier.id))
            }));
            self.procedural_curves
                .iter()
                .map(|procedural| (owners.get(procedural.id.as_str()), procedural))
                .collect()
        };
        let surfaces = if self.procedural_surfaces.is_empty() {
            Vec::new()
        } else {
            let owners =
                crate::hash::procedural_owners(self.surfaces.iter().filter_map(|carrier| {
                    carrier
                        .geometry
                        .procedural_construction()
                        .map(|id| (id.as_str(), &carrier.id))
                }));
            self.procedural_surfaces
                .iter()
                .map(|procedural| (owners.get(procedural.id.as_str()), procedural))
                .collect()
        };
        Ok(GeometrySnapshot {
            model: self,
            kind,
            curves: ProceduralCurveRows(curves),
            surfaces: ProceduralSurfaceRows(surfaces),
        })
    }
}

struct SurfaceRows<'a>(&'a [Surface]);

impl Serialize for SurfaceRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for surface in self.0 {
            sequence.serialize_element(&SurfaceWire(surface))?;
        }
        sequence.end()
    }
}

struct CurveRows<'a>(&'a [Curve]);

impl Serialize for CurveRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for curve in self.0 {
            sequence.serialize_element(&CurveWire(curve))?;
        }
        sequence.end()
    }
}

struct ProceduralSurfaceRows<'a>(Vec<(Option<&'a SurfaceId>, &'a ProceduralSurface)>);

impl Serialize for ProceduralSurfaceRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Row<'a> {
            owner: &'a SurfaceId,
            procedural: &'a ProceduralSurface,
        }
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut state = serializer.serialize_struct("ProceduralSurfaceRow", 4)?;
                state.serialize_field("id", &self.procedural.id)?;
                state.serialize_field("surface", self.owner)?;
                state.serialize_field("definition", self.procedural.definition())?;
                if let Some(bounds) = self.procedural.record_bounds() {
                    state.serialize_field("record_bounds", &bounds)?;
                }
                state.end()
            }
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for &(owner, procedural) in &self.0 {
            let owner = owner.ok_or_else(|| {
                serde::ser::Error::custom(format_args!(
                    "procedural surface {} has no unique owning surface",
                    procedural.id
                ))
            })?;
            sequence.serialize_element(&Row { owner, procedural })?;
        }
        sequence.end()
    }
}

struct ProceduralCurveRows<'a>(Vec<(Option<&'a CurveId>, &'a ProceduralCurve)>);

impl Serialize for ProceduralCurveRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Row<'a> {
            owner: &'a CurveId,
            procedural: &'a ProceduralCurve,
        }
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut state = serializer.serialize_struct("ProceduralCurveRow", 3)?;
                state.serialize_field("id", &self.procedural.id)?;
                state.serialize_field("curve", self.owner)?;
                state.serialize_field("definition", self.procedural.definition())?;
                state.end()
            }
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for &(owner, procedural) in &self.0 {
            let owner = owner.ok_or_else(|| {
                serde::ser::Error::custom(format_args!(
                    "procedural curve {} has no unique owning curve",
                    procedural.id
                ))
            })?;
            sequence.serialize_element(&Row { owner, procedural })?;
        }
        sequence.end()
    }
}

impl Serialize for GeometrySnapshot<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let model = self.model;
        let mut map = serializer.serialize_map(Some(16))?;
        map.serialize_entry("bodies", &model.bodies)?;
        map.serialize_entry("coedges", &model.coedges)?;
        map.serialize_entry("curves", &CurveRows(&model.curves))?;
        map.serialize_entry("edges", &model.edges)?;
        map.serialize_entry("faces", &model.faces)?;
        map.serialize_entry("kind", self.kind)?;
        map.serialize_entry("loops", &model.loops)?;
        map.serialize_entry("pcurves", &model.pcurves)?;
        map.serialize_entry("points", &model.points)?;
        map.serialize_entry("procedural_curves", &self.curves)?;
        map.serialize_entry("procedural_surfaces", &self.surfaces)?;
        map.serialize_entry("regions", &model.regions)?;
        map.serialize_entry("shells", &model.shells)?;
        map.serialize_entry("surfaces", &SurfaceRows(&model.surfaces))?;
        map.serialize_entry("tessellations", &model.tessellations)?;
        map.serialize_entry("vertices", &model.vertices)?;
        map.end()
    }
}

const SURFACES_UNKNOWN_GEOMETRY: &str = "surfaces_unknown_geometry";

/// A document census key: a registered model arena, the unknown-surface
/// tally, a native arena row, or a target-record kind.
///
/// Construction from a wire string classifies registered arena names and
/// `surfaces_unknown_geometry` so those rows cannot be stored as an untyped
/// other key. Remaining strings are native arena rows (`native.{format}.{name}`)
/// and export target-record kinds.
#[derive(Debug, Clone)]
pub struct CensusKey(CensusKeyInner);

#[derive(Debug, Clone)]
enum CensusKeyInner {
    Model(ArenaName),
    SurfacesUnknownGeometry,
    Other(String),
}

impl CensusKey {
    /// Key for a registered model arena.
    #[must_use]
    pub const fn model(name: ArenaName) -> Self {
        Self(CensusKeyInner::Model(name))
    }

    /// Key for the unknown-surface geometry tally.
    #[must_use]
    pub const fn surfaces_unknown_geometry() -> Self {
        Self(CensusKeyInner::SurfacesUnknownGeometry)
    }

    /// Key for a native namespace arena row, serialized as `native.{format}.{name}`.
    #[must_use]
    pub fn native(format: &str, name: &str) -> Self {
        Self(CensusKeyInner::Other(format!("native.{format}.{name}")))
    }

    /// Classify a wire key, routing registry names and the unknown-surface
    /// tally to their closed variants.
    #[must_use]
    pub fn from_wire(value: impl Into<String>) -> Self {
        let value = value.into();
        if let Some(name) = ArenaName::parse(&value) {
            return Self::model(name);
        }
        if value == SURFACES_UNKNOWN_GEOMETRY {
            return Self::surfaces_unknown_geometry();
        }
        Self(CensusKeyInner::Other(value))
    }

    /// Rebuild a count map, classifying each wire key.
    #[must_use]
    pub fn count_map<K, I>(counts: I) -> BTreeMap<Self, usize>
    where
        I: IntoIterator<Item = (K, usize)>,
        K: Into<String>,
    {
        counts
            .into_iter()
            .map(|(key, count)| (Self::from_wire(key), count))
            .collect()
    }

    /// Wire spelling of this key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            CensusKeyInner::Model(name) => name.as_str(),
            CensusKeyInner::SurfacesUnknownGeometry => SURFACES_UNKNOWN_GEOMETRY,
            CensusKeyInner::Other(value) => value,
        }
    }
}

impl From<ArenaName> for CensusKey {
    fn from(name: ArenaName) -> Self {
        Self::model(name)
    }
}

impl fmt::Display for CensusKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for CensusKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for CensusKey {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for CensusKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CensusKey {}

impl PartialOrd for CensusKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CensusKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for CensusKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Serialize for CensusKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CensusKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_wire(String::deserialize(deserializer)?))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for CensusKey {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CensusKey".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

#[derive(Debug, thiserror::Error)]
enum RegenerationParentError<'a> {
    #[error("tree child `{0}` already has a structural parent")]
    TreeChild(&'a crate::features::FeatureId),
    #[error("missing child feature `{0}`")]
    MissingChild(&'a crate::features::FeatureId),
    #[error("missing parent feature `{0}`")]
    MissingParent(&'a crate::features::FeatureId),
    #[error("parent feature `{parent}` does not precede child `{child}`")]
    NotPreceding {
        child: &'a crate::features::FeatureId,
        parent: &'a crate::features::FeatureId,
    },
}

impl Model {
    /// Structural tree owner of `child`, derived from tree-node child lists.
    pub fn feature_tree_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.features.iter().find_map(|candidate| {
            let crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::TreeNode { children, .. },
            ) = candidate.evaluation.definition()
            else {
                return None;
            };
            children.contains(child).then_some(&candidate.id)
        })
    }

    pub(crate) fn has_feature_regeneration_parents(&self) -> bool {
        !self.feature_regeneration_parents.0.is_empty()
    }

    /// Regeneration predecessor of a feature that no tree node owns.
    pub fn feature_regeneration_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.feature_regeneration_parents.0.get(child)
    }

    /// Structural owner, or the regeneration predecessor when no tree owns it.
    pub fn feature_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.feature_tree_parent(child)
            .or_else(|| self.feature_regeneration_parents.0.get(child))
    }

    fn validate_regeneration_parent<'a>(
        &self,
        child: &'a crate::features::FeatureId,
        parent: &'a crate::features::FeatureId,
    ) -> Result<(), RegenerationParentError<'a>> {
        for candidate in &self.features {
            if let crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::TreeNode { children, .. },
            ) = candidate.evaluation.definition()
            {
                for member in children {
                    if member.as_str() == child.as_str() {
                        return Err(RegenerationParentError::TreeChild(child));
                    }
                }
            }
        }
        let mut child_ordinal = None;
        for feature in &self.features {
            if feature.id.as_str() == child.as_str() {
                child_ordinal = Some(feature.ordinal);
                break;
            }
        }
        let Some(child_ordinal) = child_ordinal else {
            return Err(RegenerationParentError::MissingChild(child));
        };
        let mut parent_ordinal = None;
        for feature in &self.features {
            if feature.id.as_str() == parent.as_str() {
                parent_ordinal = Some(feature.ordinal);
                break;
            }
        }
        let Some(parent_ordinal) = parent_ordinal else {
            return Err(RegenerationParentError::MissingParent(parent));
        };
        if parent_ordinal >= child_ordinal {
            return Err(RegenerationParentError::NotPreceding { child, parent });
        }
        Ok(())
    }

    /// Set a decoded regeneration predecessor.
    pub fn set_feature_regeneration_parent(
        &mut self,
        child: &crate::features::FeatureId,
        parent: &crate::features::FeatureId,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if let Err(error) = self.validate_regeneration_parent(child, parent) {
            return Err(cadmpeg_core::CodecError::malformed(format!("{error}")));
        }
        self.feature_regeneration_parents
            .0
            .insert(child.clone(), parent.clone());
        Ok(())
    }
}

/// Failure to attach a procedural construction to its sole carrier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProceduralCarrierError {
    message: String,
}

impl ProceduralCarrierError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Model {
    /// Returns the unique surface that carries `construction`.
    #[must_use]
    pub fn procedural_surface_owner(
        &self,
        construction: &ProceduralSurfaceId,
    ) -> Option<&SurfaceId> {
        let mut owners = self
            .surfaces
            .iter()
            .filter(|surface| surface.geometry.procedural_construction() == Some(construction));
        let owner = &owners.next()?.id;
        owners.next().is_none().then_some(owner)
    }

    /// Returns the unique curve that carries `construction`.
    #[must_use]
    pub fn procedural_curve_owner(&self, construction: &ProceduralCurveId) -> Option<&CurveId> {
        let mut owners = self
            .curves
            .iter()
            .filter(|curve| curve.geometry.procedural_construction() == Some(construction));
        let owner = &owners.next()?.id;
        owners.next().is_none().then_some(owner)
    }

    /// Attach a procedural surface to its owning carrier.
    pub fn add_procedural_surface(
        &mut self,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<(), ProceduralCarrierError> {
        for existing in &self.procedural_surfaces {
            if existing.id.as_str() == procedural.id.as_str() {
                return Err(ProceduralCarrierError::new(format!(
                    "procedural surface construction {} already exists",
                    procedural.id
                )));
            }
        }
        let mut owner_index = None;
        for (index, carrier) in self.surfaces.iter().enumerate() {
            let is_owner = carrier.id.as_str() == owner.as_str();
            if owner_index.is_none() && is_owner {
                owner_index = Some(index);
            }
            if !is_owner
                && match carrier.geometry.procedural_construction() {
                    Some(construction) => construction.as_str() == procedural.id.as_str(),
                    None => false,
                }
            {
                return Err(ProceduralCarrierError::new(format!(
                    "procedural surface construction {} already owns surface {}",
                    procedural.id, carrier.id
                )));
            }
        }
        let Some(index) = owner_index else {
            return Err(ProceduralCarrierError::new(format!(
                "procedural surface {} references missing surface {owner}",
                procedural.id
            )));
        };
        self.attach_procedural_surface(owner, index, procedural)
    }

    fn attach_procedural_surface(
        &mut self,
        owner: &SurfaceId,
        index: usize,
        procedural: ProceduralSurface,
    ) -> Result<(), ProceduralCarrierError> {
        let surface = &mut self.surfaces[index];
        match &surface.geometry {
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } if construction.as_str() == procedural.id.as_str() => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Err(ProceduralCarrierError::new(format!(
                        "direct procedural surface {owner} cannot carry a solved-cache tolerance"
                    )));
                }
            }
            SurfaceGeometry::Procedural { construction, .. } => {
                return Err(ProceduralCarrierError::new(format!(
                    "surface {owner} is already owned by procedural construction {construction}"
                )));
            }
            SurfaceGeometry::Solved(_) => {
                let construction = procedural.id.clone();
                let previous = std::mem::replace(
                    &mut surface.geometry,
                    SurfaceGeometry::Procedural {
                        construction,
                        cache: None,
                    },
                );
                if let (
                    SurfaceGeometry::Solved(geometry),
                    SurfaceGeometry::Procedural { cache, .. },
                ) = (previous, &mut surface.geometry)
                {
                    *cache = Some(geometry);
                }
            }
        }
        self.procedural_surfaces.push(procedural);
        Ok(())
    }

    /// Attach a procedural curve to its owning carrier.
    pub fn add_procedural_curve(
        &mut self,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<(), ProceduralCarrierError> {
        for existing in &self.procedural_curves {
            if existing.id.as_str() == procedural.id.as_str() {
                return Err(ProceduralCarrierError::new(format!(
                    "procedural curve construction {} already exists",
                    procedural.id
                )));
            }
        }
        let mut owner_index = None;
        for (index, carrier) in self.curves.iter().enumerate() {
            let is_owner = carrier.id.as_str() == owner.as_str();
            if owner_index.is_none() && is_owner {
                owner_index = Some(index);
            }
            if !is_owner
                && match carrier.geometry.procedural_construction() {
                    Some(construction) => construction.as_str() == procedural.id.as_str(),
                    None => false,
                }
            {
                return Err(ProceduralCarrierError::new(format!(
                    "procedural curve construction {} already owns curve {}",
                    procedural.id, carrier.id
                )));
            }
        }
        let Some(index) = owner_index else {
            return Err(ProceduralCarrierError::new(format!(
                "procedural curve {} references missing curve {owner}",
                procedural.id
            )));
        };
        self.attach_procedural_curve(owner, index, procedural)
    }

    fn attach_procedural_curve(
        &mut self,
        owner: &CurveId,
        index: usize,
        procedural: ProceduralCurve,
    ) -> Result<(), ProceduralCarrierError> {
        let curve = &mut self.curves[index];
        match &mut curve.geometry {
            CurveGeometry::Procedural {
                construction,
                cache: None,
            } if construction.as_str() == procedural.id.as_str() => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Err(ProceduralCarrierError::new(format!(
                        "direct procedural curve {owner} cannot carry a solved-cache tolerance"
                    )));
                }
            }
            CurveGeometry::Procedural { construction, .. } => {
                return Err(ProceduralCarrierError::new(format!(
                    "curve {owner} is already owned by procedural construction {construction}"
                )));
            }
            CurveGeometry::Solved(_) => {
                let construction = procedural.id.clone();
                let previous = std::mem::replace(
                    &mut curve.geometry,
                    CurveGeometry::Procedural {
                        construction,
                        cache: None,
                    },
                );
                if let (CurveGeometry::Solved(geometry), CurveGeometry::Procedural { cache, .. }) =
                    (previous, &mut curve.geometry)
                {
                    *cache = Some(geometry);
                }
            }
        }
        self.procedural_curves.push(procedural);
        Ok(())
    }
}

/// Accept only the supported `ir_version`.
///
/// The version stays a [`serde_json::Value`] so a missing member and a
/// non-string one are both reported as an unsupported version rather than as a
/// missing-field or type error. This is the single version gate: both
/// [`Deserialize`] for [`CadIr`] and [`CadIr::from_json`] call it.
fn check_ir_version<E: serde::de::Error>(version: Option<&serde_json::Value>) -> Result<(), E> {
    let version = version.and_then(serde_json::Value::as_str);
    if version != Some(IR_VERSION) {
        return Err(E::custom(format!(
            "unsupported ir_version {version:?}; expected {IR_VERSION}"
        )));
    }
    Ok(())
}

impl Serialize for IrVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(IR_VERSION)
    }
}

impl<'de> Deserialize<'de> for IrVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = serde_json::Value::deserialize(deserializer)?;
        check_ir_version(Some(&version))?;
        Ok(Self)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for IrVersion {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "IrVersion".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ir_version_schema(generator)
    }
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
/// [`crate::report::check::ValidationReport`] is produced separately by `validate_neutral` and
/// is not embedded in the document.
///
/// `model` holds the format-neutral graph. `native` retains typed
/// format-specific product data without changing that graph's semantics.
/// Entity IDs must be globally unique across all document arenas.
/// `ir_version` is a serialized constant checked by the read adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct CadIr {
    /// Source-container metadata.
    pub source: Option<SourceMeta>,
    /// Document-wide tolerances.
    pub tolerances: Tolerances,
    /// Format-neutral model.
    pub model: Model,
    /// Per-format native namespaces, each an arena map.
    pub native: Native,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CadIrWriteWire<'a> {
    ir_version: IrVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    model: &'a Model,
    native: &'a Native,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CadIrReadWire {
    #[serde(
        default,
        rename = "ir_version",
        deserialize_with = "deserialize_ir_version"
    )]
    ir_version: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_source")]
    source: Option<SourceMeta>,
    #[serde(default, rename = "units")]
    _units: CanonicalUnitsWire,
    tolerances: Tolerances,
    model: Model,
    #[serde(default)]
    native: Native,
}

impl Serialize for CadIr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CadIrWriteWire {
            ir_version: IrVersion,
            source: self.source.as_ref(),
            units: CanonicalUnitsWire::default(),
            tolerances: &self.tolerances,
            model: &self.model,
            native: &self.native,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CadIr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CadIrReadWire::deserialize(deserializer)?;
        check_ir_version(wire.ir_version.as_ref())?;
        Ok(wire.into())
    }
}

impl From<CadIrReadWire> for CadIr {
    fn from(wire: CadIrReadWire) -> Self {
        Self {
            source: wire.source,
            tolerances: wire.tolerances,
            model: wire.model,
            native: wire.native,
        }
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for CadIr {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CadIr".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::CadIr").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        CadIrWriteWire::json_schema(generator)
    }
}

impl CadIr {
    /// Appends staged neutral and native records, then admits the combined document.
    ///
    /// The admission callback can only read the document. On `Err`, every arena
    /// and feature-parent relation returns to its preceding state. Existing
    /// geometry and native payloads are not cloned. Source metadata and document
    /// tolerances are unchanged.
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::{CadIr, document::Model, native::Native};
    /// let mut ir = CadIr::empty();
    /// ir.try_append(Model::default(), Native::default(), |candidate| {
    ///     candidate.model.points.clear();
    ///     Ok(Ok::<(), ()>(()))
    /// });
    /// ```
    pub fn try_append<T, E>(
        &mut self,
        model: Model,
        native: Native,
        admit: impl FnOnce(&Self) -> Result<Result<T, E>, CodecError>,
    ) -> Result<Result<T, E>, CodecError> {
        let native_lengths = (!native.0.is_empty()).then(|| {
            self.native
                .0
                .iter()
                .map(|(format, namespace)| {
                    let arenas = namespace
                        .arenas()
                        .iter()
                        .map(|(arena, records)| (arena.to_owned(), records.len()))
                        .collect::<BTreeMap<_, _>>();
                    (format.to_owned(), arenas)
                })
                .collect::<BTreeMap<_, _>>()
        });
        let original_parents = (!model.feature_regeneration_parents.0.is_empty())
            .then(|| self.model.feature_regeneration_parents.clone());
        macro_rules! append_and_admit {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {{
                $(let $field = self.model.$field.len();)*
                self.model.append(model);
                for (format, mut namespace) in native.0 {
                    match self.native.0.entry(format) {
                        std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(namespace); },
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            let destination = entry.get_mut().arenas_mut();
                            for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
                                match destination.entry(arena) {
                                    std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(records); },
                                    std::collections::btree_map::Entry::Occupied(mut entry) => { entry.get_mut().append(&mut records); },
                                }
                            }
                        },
                    }
                }
                let result = admit(self);
                if !result.as_ref().is_ok_and(|inner| inner.is_ok()) {
                    $(self.model.$field.truncate($field);)*
                    if let Some(parents) = original_parents { self.model.feature_regeneration_parents = parents; }
                    if let Some(lengths) = &native_lengths {
                        self.native.0.retain(|format, namespace| {
                            let Some(arenas) = lengths.get(format) else { return false; };
                            namespace.arenas_mut().retain(|arena, records| {
                                let Some(length) = arenas.get(arena) else { return false; };
                                records.truncate(*length);
                                true
                            });
                            true
                        });
                    }
                }
                result
            }};
        }
        arena_registry!(append_and_admit)
    }

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

    /// Replace the reserved `unknowns` arena for `format`.
    pub fn set_native_unknowns(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: &str,
        records: &[NativeUnknownRecord],
    ) -> Result<(), crate::native::NativeConvertError> {
        self.set_native_unknowns_from(ctx, format, records.iter())
    }

    /// Replace the reserved `unknowns` arena for `format` one record at a time.
    ///
    /// A caller that derives its product references from a larger population
    /// should derive them inside the iterator: each reference is serialized as
    /// it is produced, so the derived population is never resident alongside the
    /// arena built from it.
    /// Duplicate identities are refused before the document changes.
    ///
    /// Only the reserved product projection can enter through this route.
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::CadIr;
    /// let ctx = cadmpeg_test_support::service_decode_context();
    /// let raw = serde_json::json!({"id": "test:native:unknown#0", "extra": true});
    /// CadIr::empty().set_native_unknowns_from(&ctx, "test", [raw]).unwrap();
    /// ```
    pub fn set_native_unknowns_from<T: Borrow<NativeUnknownRecord>, I: IntoIterator<Item = T>>(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        let records = crate::native::arena_from(
            ctx,
            records.into_iter().map(|record| {
                Ok::<_, crate::native::NativeConvertError>(UnknownProjection(record))
            }),
        )?;

        for pair in records.windows(2) {
            if pair[0].id() == pair[1].id() {
                return Err(crate::native::NativeConvertError::InvalidCollection(
                    format!("duplicate native unknown record {}", pair[0].id()),
                ));
            }
        }

        let key = "unknowns".to_owned();
        if let Some(namespace) = self.native.0.get_mut(format) {
            namespace.arenas_mut().insert(key, records);
        } else {
            let mut namespace = crate::native::NativeNamespace::default();
            namespace.arenas_mut().insert(key, records);
            self.native.0.insert(format.to_owned(), namespace);
        }
        Ok(())
    }

    /// Construct an empty document with default tolerances.
    ///
    /// Fixtures and in-progress assembly use this constructor. Decoders that
    /// have classified source metadata use [`Self::decoded`].
    pub fn empty() -> Self {
        Self {
            source: None,
            tolerances: Tolerances::default(),
            model: Model::default(),
            native: Native::default(),
        }
    }

    /// Construct a decoded document with classified source metadata.
    pub fn decoded(source: SourceMeta) -> Self {
        Self {
            source: Some(source),
            tolerances: Tolerances::default(),
            model: Model::default(),
            native: Native::default(),
        }
    }

    /// IR schema version emitted by serialization and accepted on deserialize.
    pub fn ir_version(&self) -> &str {
        IR_VERSION
    }

    /// Serialize a finalized, identity-sorted view as pretty JSON.
    ///
    /// Clones and [`finalize`](Self::finalize)s so callers need not pre-sort.
    ///
    /// The text is written through the same finite adapter the digest uses, so
    /// a non-finite float is refused rather than written as `null`.
    ///
    /// # Errors
    ///
    /// Refuses a document holding a non-finite float.
    pub fn to_canonical_json(&self) -> Result<String, CanonicalJsonError> {
        let mut canonical = self.clone();
        canonical.finalize();
        crate::hash::finite_json::to_canonical_json_string(&canonical)
    }

    /// Parse JSON and reject any unsupported `ir_version`.
    ///
    /// Version is probed first (`ir_version` only) so the full document is
    /// materialized once after the version gate.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        /// Every member but `ir_version` is skipped.
        #[derive(Deserialize)]
        struct VersionProbe {
            ir_version: Option<serde_json::Value>,
        }

        let probe = serde_json::from_str::<VersionProbe>(s)?;
        check_ir_version(probe.ir_version.as_ref())?;
        serde_json::from_str::<CadIrReadWire>(s).map(Into::into)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> BTreeMap<CensusKey, usize> {
        census::count(crate::native::view::NativeView::new(self, None))
    }
}

/// Source-container metadata preserved for reporting.
///
/// Attribute keys ending in [`crate::compare::LOCAL_DIGEST_SUFFIX`] hold
/// machine-local digests over decoded content for the write-path edit oracle.
/// Not portable across platforms; not tolerance-aware. Digests over retained
/// source bytes must not use that suffix. See
/// [`crate::hash::document_local_sha256`] and
/// [`crate::compare::is_local_digest_attribute`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceMeta {
    /// Format identity: classified dialect layers, or a bare known format.
    identity: FormatIdentity<DialectLayers>,
    /// Format-specific attributes.
    #[serde(default)]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub attributes: BTreeMap<NonBlankString, String>,
}

impl SourceMeta {
    /// Constructs metadata with dialect layers whose primary format is authoritative.
    #[must_use]
    pub fn classified(
        dialects: DialectLayers,
        attributes: BTreeMap<NonBlankString, String>,
    ) -> Self {
        Self {
            identity: FormatIdentity::classified(dialects),
            attributes,
        }
    }

    pub(crate) fn normalized_digest_copy(&self) -> Self {
        let mut copy = self.clone();
        copy.attributes
            .remove(crate::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);
        copy
    }

    /// The complete source identity: format plus classified layers, if any.
    #[must_use]
    pub(crate) const fn classification(&self) -> &FormatIdentity<DialectLayers> {
        &self.identity
    }

    /// Registry format namespace of this source's primary layer.
    #[must_use]
    pub fn format(&self) -> &str {
        self.identity.format()
    }

    /// Returns every source dialect layer when the source was classified.
    #[must_use]
    pub const fn dialects(&self) -> Option<&DialectLayers> {
        self.identity.classified_payload()
    }

    /// Returns the primary source dialect match when the source was classified.
    ///
    /// The match contains the registry dialect id, source declarations,
    /// admission, and optional instance discriminator. Its format is also the
    /// source format returned by [`Self::format`].
    #[must_use]
    pub fn dialect(&self) -> Option<&DialectMatch> {
        self.dialects().map(DialectLayers::primary)
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_ir_version, serde_json::Value, "ir_version");
cadmpeg_core::named_optional_field!(deserialize_source, SourceMeta, "source");

mod identity_rewrite;
