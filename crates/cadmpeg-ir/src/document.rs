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
    de::DeserializeOwned, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer,
};

use cadmpeg_core::dialect::{DialectLayers, DialectMatch, FormatIdentity};

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignConfigurationReadWire, DesignParameter, Feature,
    FeatureInputTopology, FeatureReadWire, FeatureResultTopology, FeatureWriteWire,
};
use crate::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralCurve, ProceduralCurveReadWire, ProceduralSurface,
    ProceduralSurfaceReadWire, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
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

#[derive(Debug, Clone, Default, PartialEq)]
struct FeatureRegenerationParents(BTreeMap<crate::features::FeatureId, crate::features::FeatureId>);

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

struct SurfaceWire<'a>(&'a Surface);

impl Serialize for SurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Surface", 3)?;
        state.serialize_field("id", &self.0.id)?;
        state.serialize_field("geometry", self.0.geometry.wire_geometry())?;
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
        let mut state = serializer.serialize_struct("Curve", 3)?;
        state.serialize_field("id", &self.0.id)?;
        state.serialize_field("geometry", self.0.geometry.wire_geometry())?;
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
        let mut state = serializer.serialize_struct("ProceduralSurface", 5)?;
        state.serialize_field("id", &self.procedural.id)?;
        state.serialize_field("surface", owner)?;
        state.serialize_field("definition", self.procedural.definition())?;
        if let Some(cache_fit_tolerance) = self.procedural.cache_fit_tolerance() {
            state.serialize_field("cache_fit_tolerance", &cache_fit_tolerance)?;
        }
        if let Some(record_bounds) = self.procedural.record_bounds {
            state.serialize_field("record_bounds", &record_bounds)?;
        }
        state.end()
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
        let mut state = serializer.serialize_struct("ProceduralCurve", 4)?;
        state.serialize_field("id", &self.procedural.id)?;
        state.serialize_field("curve", owner)?;
        state.serialize_field("definition", self.procedural.definition())?;
        if let Some(cache_fit_tolerance) = self.procedural.cache_fit_tolerance() {
            state.serialize_field("cache_fit_tolerance", &cache_fit_tolerance)?;
        }
        state.end()
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
    ($model:expr, procedural_surfaces) => {
        $model
            .procedural_surfaces
            .iter()
            .map(|procedural| ProceduralSurfaceWire {
                owner: $model.procedural_surface_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, procedural_curves) => {
        $model
            .procedural_curves
            .iter()
            .map(|procedural| ProceduralCurveWire {
                owner: $model.procedural_curve_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, features) => {
        $model
            .features
            .iter()
            .map(|feature| FeatureWriteWire::new(feature, $model.feature_parent(&feature.id)))
            .collect()
    };
    ($model:expr, $field:ident) => {
        &$model.$field
    };
}

macro_rules! model_read_type {
    (procedural_surfaces, $ty:ty) => { Vec<ProceduralSurfaceReadWire> };
    (procedural_curves, $ty:ty) => { Vec<ProceduralCurveReadWire> };
    (configurations, $ty:ty) => { Vec<DesignConfigurationReadWire> };
    (features, $ty:ty) => { Vec<FeatureReadWire> };
    ($field:ident, $ty:ty) => { Vec<$ty> };
}

macro_rules! model_read_value {
    ($wire:expr, procedural_surfaces) => {
        Vec::new()
    };
    ($wire:expr, procedural_curves) => {
        Vec::new()
    };
    ($wire:expr, configurations) => {
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
struct SurfaceCacheRewrite {
    geometry: SurfaceGeometry,
}

#[derive(Serialize, Deserialize)]
struct CurveCacheRewrite {
    geometry: CurveGeometry,
}

#[derive(Serialize, Deserialize)]
struct FeatureRegenerationEdge {
    child: crate::features::FeatureId,
    parent: crate::features::FeatureId,
}

fn rewrite_surface<R: EntityRewrite>(
    rewrite: &mut R,
    mut surface: Surface,
) -> Result<Surface, R::Error> {
    let cache = match &mut surface.geometry {
        SurfaceGeometry::Procedural { cache, .. } => cache.take(),
        _ => None,
    };
    let cache = match cache {
        Some(cache) => Some(
            rewrite
                .rewrite(SurfaceCacheRewrite {
                    geometry: cache.into_geometry(),
                })?
                .geometry,
        ),
        None => None,
    };
    let mut surface = rewrite.rewrite(surface)?;
    if let (Some(cache), SurfaceGeometry::Procedural { cache: slot, .. }) =
        (cache, &mut surface.geometry)
    {
        *slot = SolvedSurfaceGeometry::new(cache).ok();
    }
    Ok(surface)
}

fn rewrite_curve<R: EntityRewrite>(rewrite: &mut R, mut curve: Curve) -> Result<Curve, R::Error> {
    let cache = match &mut curve.geometry {
        CurveGeometry::Procedural { cache, .. } => cache.take(),
        _ => None,
    };
    let cache = match cache {
        Some(cache) => Some(
            rewrite
                .rewrite(CurveCacheRewrite {
                    geometry: cache.into_geometry(),
                })?
                .geometry,
        ),
        None => None,
    };
    let mut curve = rewrite.rewrite(curve)?;
    if let (Some(cache), CurveGeometry::Procedural { cache: slot, .. }) =
        (cache, &mut curve.geometry)
    {
        *slot = SolvedCurveGeometry::new(cache).ok();
    }
    Ok(curve)
}

macro_rules! model_rewrite_entity {
    ($rewrite:expr, surfaces, $entity:expr) => {
        rewrite_surface($rewrite, $entity)
    };
    ($rewrite:expr, curves, $entity:expr) => {
        rewrite_curve($rewrite, $entity)
    };
    ($rewrite:expr, $field:ident, $entity:expr) => {
        $rewrite.rewrite($entity)
    };
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
        sorted_refs(&$model.surfaces)
            .into_iter()
            .map(SurfaceWire)
            .collect()
    };
    ($model:expr, curves) => {
        sorted_refs(&$model.curves)
            .into_iter()
            .map(CurveWire)
            .collect()
    };
    ($model:expr, procedural_surfaces) => {
        sorted_refs(&$model.procedural_surfaces)
            .into_iter()
            .map(|procedural| ProceduralSurfaceWire {
                owner: $model.procedural_surface_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, procedural_curves) => {
        sorted_refs(&$model.procedural_curves)
            .into_iter()
            .map(|procedural| ProceduralCurveWire {
                owner: $model.procedural_curve_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, features) => {
        sorted_refs(&$model.features)
            .into_iter()
            .map(|feature| FeatureWriteWire::new(feature, $model.feature_parent(&feature.id)))
            .collect()
    };
    ($model:expr, $field:ident) => {
        sorted_refs(&$model.$field)
    };
}

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct Model {
            $(
                $(#[$attribute])*
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
            #[cfg_attr(feature = "schema", schemars(skip))]
            feature_regeneration_parents: FeatureRegenerationParents,
        }

        #[derive(Serialize)]
        struct ModelWriteWire<'a> {
            $(
                $(#[$attribute])*
                $field: model_write_type!($field, $ty, 'a),
            )*
        }

        #[derive(Deserialize)]
        struct ModelReadWire {
            $(
                $(#[$attribute])*
                $field: model_read_type!($field, $ty),
            )*
        }

        impl Serialize for Model {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                crate::topology::install_coedge_ring_neighbors(&self.loops, &self.coedges);
                let result = ModelWriteWire {
                    $($field: model_write_value!(self, $field),)*
                }
                .serialize(serializer);
                crate::topology::clear_coedge_ring_neighbors();
                result
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
                let configurations = std::mem::take(&mut wire.configurations);
                let feature_wires = std::mem::take(&mut wire.features);
                let (features, feature_parents): (Vec<_>, Vec<_>) = feature_wires
                    .into_iter()
                    .map(FeatureReadWire::into_parts)
                    .unzip();
                let mut model = Self {
                    $($field: model_read_value!(wire, $field),)*
                    feature_regeneration_parents: FeatureRegenerationParents::default(),
                };
                model.features = features;
                reconcile_feature_parents(&mut model, feature_parents)
                    .map_err(serde::de::Error::custom)?;
                for wire in procedural_surfaces {
                    let (owner, procedural) = wire.into_parts().map_err(serde::de::Error::custom)?;
                    let owner = owner.ok_or_else(|| serde::de::Error::custom(
                        "procedural surface wire is missing surface",
                    ))?;
                    model
                        .add_procedural_surface(owner, procedural)
                        .map_err(serde::de::Error::custom)?;
                }
                for wire in procedural_curves {
                    let (owner, procedural) = wire.into_parts().map_err(serde::de::Error::custom)?;
                    let owner = owner.ok_or_else(|| serde::de::Error::custom(
                        "procedural curve wire is missing curve",
                    ))?;
                    model
                        .add_procedural_curve(owner, procedural)
                        .map_err(serde::de::Error::custom)?;
                }
                for wire in configurations {
                    model.configurations.push(
                        wire.into_configuration(&model.features)
                            .map_err(serde::de::Error::custom)?,
                    );
                }
                Ok(model)
            }
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
                for parent in self.feature_regeneration_parents.0.values() {
                    visitor(crate::schema::Reference {
                        target: parent.0.clone(),
                    });
                }
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
                        self.$field.push(model_rewrite_entity!(rewrite, $field, entity)?);
                    }
                )*
                for (child, parent) in other.feature_regeneration_parents.0 {
                    let edge = rewrite.rewrite(FeatureRegenerationEdge { child, parent })?;
                    self.feature_regeneration_parents.0
                        .insert(edge.child, edge.parent);
                }
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
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedModel<'_> {
                crate::topology::install_coedge_ring_neighbors(&self.loops, &self.coedges);
                SortedModel {
                    $($field: sorted_model_value!(self, $field),)*
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

macro_rules! declare_arena_name {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Name of a registered model arena.
        ///
        /// Variant identifiers match the `arena_registry!` field names so a
        /// new arena cannot be counted or diffed under a string the registry
        /// does not declare.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[allow(non_camel_case_types)]
        pub enum ArenaName {
            $(
                #[doc = $doc]
                $field,
            )*
        }

        impl ArenaName {
            /// Every registered arena, in registry order.
            pub const ALL: &'static [Self] = &[$(Self::$field),*];

            /// Registry field name, which is also the CADIR JSON object key.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$field => stringify!($field),)*
                }
            }

            /// Parse a registry field name.
            #[must_use]
            pub fn from_str(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($field) => Some(Self::$field),)*
                    _ => None,
                }
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
pub const IR_VERSION: &str = "5";

arena_registry!(declare_model);
arena_registry!(assert_entity_schemas);
arena_registry!(declare_model_view);
arena_registry!(declare_arena_name);

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
        if let Some(name) = ArenaName::from_str(&value) {
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

fn reconcile_feature_parents(
    model: &mut Model,
    wire_parents: Vec<Option<crate::features::FeatureId>>,
) -> Result<(), String> {
    use crate::features::FeatureDefinition;
    use std::collections::HashMap;

    let indices = model
        .features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut tree_parents = HashMap::<crate::features::FeatureId, crate::features::FeatureId>::new();
    for parent in &model.features {
        let FeatureDefinition::TreeNode {
            children,
            active_child,
            ..
        } = &parent.definition
        else {
            continue;
        };
        if active_child
            .as_ref()
            .is_some_and(|active| !children.contains(active))
        {
            return Err(format!(
                "feature `{}` has an active child outside its child list",
                parent.id
            ));
        }
        for child in children {
            if let Some(previous) = tree_parents.insert(child.clone(), parent.id.clone()) {
                return Err(format!(
                    "feature `{child}` has two tree parents `{previous}` and `{}`",
                    parent.id
                ));
            }
        }
    }

    for (child_index, wire_parent) in wire_parents.into_iter().enumerate() {
        let child_id = model.features[child_index].id.clone();
        let Some(parent_id) = wire_parent else {
            if let Some(parent) = tree_parents.get(&child_id) {
                return Err(format!(
                    "tree child `{child_id}` is missing its serialized parent `{parent}`"
                ));
            }
            continue;
        };
        let Some(&parent_index) = indices.get(&parent_id) else {
            return Err(format!(
                "feature `{child_id}` names missing parent `{parent_id}`"
            ));
        };
        if model.features[parent_index].ordinal >= model.features[child_index].ordinal {
            return Err(format!(
                "parent feature `{parent_id}` does not precede child `{child_id}`"
            ));
        }
        if matches!(
            model.features[parent_index].definition,
            FeatureDefinition::TreeNode { .. }
        ) {
            if let Some(existing) = tree_parents.get(&child_id) {
                if existing != &parent_id {
                    return Err(format!(
                        "tree child `{child_id}` names parent `{parent_id}` but is owned by `{existing}`"
                    ));
                }
            } else if let FeatureDefinition::TreeNode { children, .. } =
                &mut model.features[parent_index].definition
            {
                children.push(child_id.clone());
                tree_parents.insert(child_id.clone(), parent_id);
            }
        } else {
            model
                .feature_regeneration_parents
                .0
                .insert(child_id, parent_id);
        }
    }
    Ok(())
}

impl Model {
    /// Structural tree owner of `child`, derived from tree-node child lists.
    pub fn feature_tree_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.features.iter().find_map(|candidate| {
            let crate::features::FeatureDefinition::TreeNode { children, .. } =
                &candidate.definition
            else {
                return None;
            };
            children.contains(child).then_some(&candidate.id)
        })
    }

    /// Legacy parent projection: structural owner or regeneration predecessor.
    pub fn feature_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.feature_tree_parent(child)
            .or_else(|| self.feature_regeneration_parents.0.get(child))
    }

    /// Set the non-tree containing operation used to order regeneration.
    pub fn set_feature_regeneration_parent(
        &mut self,
        child: crate::features::FeatureId,
        parent: crate::features::FeatureId,
    ) -> Result<(), String> {
        if self.feature_tree_parent(&child).is_some() {
            return Err(format!(
                "tree child `{child}` already has a structural parent"
            ));
        }
        let child_ordinal = self
            .features
            .iter()
            .find(|feature| feature.id == child)
            .map(|feature| feature.ordinal)
            .ok_or_else(|| format!("missing child feature `{child}`"))?;
        let parent_ordinal = self
            .features
            .iter()
            .find(|feature| feature.id == parent)
            .map(|feature| feature.ordinal)
            .ok_or_else(|| format!("missing parent feature `{parent}`"))?;
        if parent_ordinal >= child_ordinal {
            return Err(format!(
                "parent feature `{parent}` does not precede child `{child}`"
            ));
        }
        self.feature_regeneration_parents.0.insert(child, parent);
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

    /// Attaches one procedural surface construction to its carrier.
    pub fn add_procedural_surface(
        &mut self,
        owner: SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<(), ProceduralCarrierError> {
        if self
            .procedural_surfaces
            .iter()
            .any(|existing| existing.id == procedural.id)
        {
            return Err(ProceduralCarrierError::new(format!(
                "procedural surface construction {} already exists",
                procedural.id
            )));
        }
        if let Some(existing_owner) = self.surfaces.iter().find(|surface| {
            surface.id != owner
                && surface.geometry.procedural_construction() == Some(&procedural.id)
        }) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural surface construction {} already owns surface {}",
                procedural.id, existing_owner.id
            )));
        }
        let surface = self
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == owner)
            .ok_or_else(|| {
                ProceduralCarrierError::new(format!(
                    "procedural surface {} references missing surface {owner}",
                    procedural.id
                ))
            })?;
        match &surface.geometry {
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
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
            geometry => {
                let cache = SolvedSurfaceGeometry::new(geometry.clone()).map_err(|_| {
                    ProceduralCarrierError::new(format!(
                        "surface {owner} already has a procedural construction"
                    ))
                })?;
                surface.geometry = SurfaceGeometry::Procedural {
                    construction: procedural.id.clone(),
                    cache: Some(cache),
                };
            }
        }
        self.procedural_surfaces.push(procedural);
        Ok(())
    }

    /// Attaches one procedural curve construction to its carrier.
    pub fn add_procedural_curve(
        &mut self,
        owner: CurveId,
        procedural: ProceduralCurve,
    ) -> Result<(), ProceduralCarrierError> {
        if self
            .procedural_curves
            .iter()
            .any(|existing| existing.id == procedural.id)
        {
            return Err(ProceduralCarrierError::new(format!(
                "procedural curve construction {} already exists",
                procedural.id
            )));
        }
        if let Some(existing_owner) = self.curves.iter().find(|curve| {
            curve.id != owner && curve.geometry.procedural_construction() == Some(&procedural.id)
        }) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural curve construction {} already owns curve {}",
                procedural.id, existing_owner.id
            )));
        }
        let curve = self
            .curves
            .iter_mut()
            .find(|curve| curve.id == owner)
            .ok_or_else(|| {
                ProceduralCarrierError::new(format!(
                    "procedural curve {} references missing curve {owner}",
                    procedural.id
                ))
            })?;
        match &curve.geometry {
            CurveGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
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
            geometry => {
                let cache = SolvedCurveGeometry::new(geometry.clone()).map_err(|_| {
                    ProceduralCarrierError::new(format!(
                        "curve {owner} already has a procedural construction"
                    ))
                })?;
                curve.geometry = CurveGeometry::Procedural {
                    construction: procedural.id.clone(),
                    cache: Some(cache),
                };
            }
        }
        self.procedural_curves.push(procedural);
        Ok(())
    }
}

fn deserialize_ir_version<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    let version = String::deserialize(deserializer)?;
    if version != IR_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported ir_version {version:?}; expected {IR_VERSION}"
        )));
    }
    Ok(())
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
    /// Independently versioned native namespaces.
    pub native: Native,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CadIrWriteWire<'a> {
    #[cfg_attr(feature = "schema", schemars(schema_with = "ir_version_schema"))]
    ir_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    model: &'a Model,
    native: &'a Native,
}

#[derive(Deserialize)]
struct CadIrReadWire {
    #[serde(rename = "ir_version", deserialize_with = "deserialize_ir_version")]
    _ir_version: (),
    #[serde(flatten)]
    payload: CadIrPayload,
}

#[derive(Deserialize)]
struct CadIrPayload {
    #[serde(default)]
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
            ir_version: IR_VERSION,
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
        Ok(wire.payload.into())
    }
}

impl From<CadIrPayload> for CadIr {
    fn from(payload: CadIrPayload) -> Self {
        Self {
            source: payload.source,
            tolerances: payload.tolerances,
            model: payload.model,
            native: payload.native,
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

    /// Return the `format` namespace used for version-1 unknown records.
    fn unknowns_namespace_mut(&mut self, format: &str) -> &mut crate::native::NativeNamespace {
        self.native.namespace_mut(format, std::num::NonZeroU32::MIN)
    }

    /// Construct an empty current-version document with default tolerances.
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
        serde_json::from_str::<CadIrPayload>(s).map(Into::into)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> BTreeMap<CensusKey, usize> {
        entity_census(self)
    }
}

macro_rules! define_registered_entity_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        fn registered_entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
            BTreeMap::from([
                $((CensusKey::model(ArenaName::$field), ir.model.$field.len())),*
            ])
        }
    };
}
arena_registry!(define_registered_entity_census);

/// Count the records represented by the IR arenas without running validation.
pub fn entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
    let mut counts = registered_entity_census(ir);
    counts.insert(
        CensusKey::surfaces_unknown_geometry(),
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
        counts.insert(CensusKey::native(&loss.format, &loss.kind), loss.count);
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMeta {
    classification: FormatIdentity<DialectLayers>,
    /// Format-specific attributes.
    pub attributes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SourceMetaReadWire {
    format: String,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
    #[serde(default)]
    dialects: Option<DialectLayers>,
    #[serde(default)]
    dialect: Option<DialectMatch>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SourceMetaWriteWire<'a> {
    format: &'a str,
    attributes: &'a BTreeMap<String, String>,
    dialects: Option<&'a DialectLayers>,
}

impl Serialize for SourceMeta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SourceMetaWriteWire {
            format: self.format(),
            attributes: &self.attributes,
            dialects: self.dialects(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SourceMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SourceMetaReadWire::deserialize(deserializer)?;
        let dialects = match (wire.dialects, wire.dialect) {
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "source metadata cannot contain both dialects and legacy dialect fields",
                ));
            }
            (Some(dialects), None) => Some(dialects),
            (None, Some(dialect)) => Some(DialectLayers::of(dialect)),
            (None, None) => None,
        };
        let classification =
            FormatIdentity::from_wire(wire.format, dialects).map_err(serde::de::Error::custom)?;
        Ok(Self {
            classification,
            attributes: wire.attributes,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SourceMeta {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SourceMeta".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::SourceMeta").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = SourceMetaWriteWire::json_schema(generator);
        crate::schema::require_object_fields(&mut schema, ["dialects"]);
        schema
    }
}

impl SourceMeta {
    /// Constructs metadata with dialect layers whose primary format is authoritative.
    #[must_use]
    pub fn classified(dialects: DialectLayers, attributes: BTreeMap<String, String>) -> Self {
        Self {
            classification: FormatIdentity::classified(dialects),
            attributes,
        }
    }

    /// The complete source identity: format plus classified layers, if any.
    #[must_use]
    pub(crate) fn classification(&self) -> &FormatIdentity<DialectLayers> {
        &self.classification
    }

    /// Registry format namespace of this source's primary layer.
    #[must_use]
    pub fn format(&self) -> &str {
        self.classification.format()
    }

    /// Returns every source dialect layer when the source was classified.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.classification.classified_payload()
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
