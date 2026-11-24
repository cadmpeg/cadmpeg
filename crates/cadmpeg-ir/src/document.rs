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

use cadmpeg_core::decode::{ArenaClass, ContentScope, DecodeContext};
use cadmpeg_core::dialect::{DialectLayers, DialectMatch, FormatIdentity};
use cadmpeg_core::CodecError;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::evaluation::{FeatureInputTopology, FeatureResultTopology};
use crate::features::parameters::{DesignConfiguration, DesignParameter};
use crate::features::serialization::FeatureWriteWire;
use crate::features::Feature;
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
pub(crate) mod design_records;
mod design_references;
pub(crate) mod feature_parents;
mod legacy;
mod one_zero;
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

/// Whether a [`Model`] holds the arenas the CADIR wire leaves out
/// ([`ArenaName::WIRE_OMITTED`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OmittedArenas {
    /// The model holds them: an empty arena holds no records. `baseline` is
    /// their digest as the decode wrapper returned the document.
    Held { baseline: Option<String> },
    /// The model was read from CADIR JSON, which does not carry them, so
    /// their content is unknown.
    NotRead,
}

impl Default for OmittedArenas {
    fn default() -> Self {
        Self::Held { baseline: None }
    }
}

impl OmittedArenas {
    /// The state of a model that takes the records of `other`.
    fn merged(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::NotRead, _) | (_, Self::NotRead) => Self::NotRead,
            (Self::Held { baseline }, Self::Held { .. }) => Self::Held {
                baseline: baseline.clone(),
            },
        }
    }
}

/// Every neutral arena with its [`ArenaClass`] token (`model`, `design`, or
/// `process`) and its home token, the [`CadIr`] field that holds it
/// (`geometry`, `model`, `design`, or `process`).
///
/// Consumers that need the class match `$class:ident` after the type; every
/// other consumer goes through [`arena_registry!`] or
/// [`every_arena_registry!`], which drop the class token. A consumer reaches
/// an arena of a document as `ir.$home.$field`. The arenas of the `design`
/// and `process` homes follow every other, so the canonical index of every
/// other arena does not depend on them. A `model` arena whose attributes are
/// `serde(skip)` is held in memory and left out of the CADIR wire
/// ([`ArenaName::WIRE_OMITTED`]).
macro_rules! classified_arena_registry {
    ($macro:ident) => {
        $crate::document::classified_arena_registry! { @entries [$macro] }
    };
    (@entries [$($target:tt)+] $($prefix:ident)?) => {
        $($target)+! {
            $($prefix;)?
            bodies: Body, model, geometry, "Body arena.", [];
            regions: Region, model, geometry, "Region arena.", [];
            shells: Shell, model, geometry, "Shell arena.", [];
            faces: Face, model, geometry, "Face arena.", [];
            loops: Loop, model, geometry, "Loop arena.", [];
            coedges: Coedge, model, geometry, "Coedge arena.", [];
            edges: Edge, model, geometry, "Edge arena.", [];
            vertices: Vertex, model, geometry, "Vertex arena.", [];
            points: Point, model, geometry, "Point arena.", [];
            surfaces: Surface, model, geometry, "Surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Surface>"))];
            curves: Curve, model, geometry, "Curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Curve>"))];
            subds: SubdSurface, model, geometry, "Subdivision surface arena.", [];
            pcurves: Pcurve, model, geometry, "Pcurve arena.", [];
            procedural_surfaces: ProceduralSurface, model, geometry, "Procedural surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralSurface>"))];
            procedural_curves: ProceduralCurve, model, geometry, "Procedural curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralCurve>"))];
            assets: crate::assets::Asset, model, model, "Embedded and externally referenced document resources.", [];
            features: Feature, design, model, "Feature arena.", [serde(skip)];
            feature_input_topologies: FeatureInputTopology, design, model, "Feature input-topology arena.", [serde(skip)];
            feature_result_topologies: FeatureResultTopology, design, model, "Feature result-topology arena.", [serde(skip)];
            configurations: DesignConfiguration, design, model, "Design configuration arena.", [serde(skip)];
            parameters: DesignParameter, design, model, "Design parameter arena.", [serde(skip)];
            sketches: Sketch, design, model, "Planar sketch arena.", [serde(skip)];
            sketch_entities: SketchEntity, design, model, "Solved sketch entity arena.", [serde(skip)];
            sketch_constraints: SketchConstraint, design, model, "Sketch constraint arena.", [serde(skip)];
            spatial_sketches: SpatialSketch, design, model, "Spatial sketch arena.", [serde(skip)];
            spatial_sketch_entities: SpatialSketchEntity, design, model, "Solved spatial sketch entity arena.", [serde(skip)];
            spatial_sketch_constraints: SpatialSketchConstraint, design, model, "Spatial sketch constraint arena.", [serde(skip)];
            spreadsheets: Spreadsheet, design, model, "Spreadsheet arena.", [serde(skip)];
            product_definitions: ProductDefinition, model, model, "Product definition arena.", [];
            occurrences: Occurrence, model, model, "Product occurrence arena.", [];
            assembly_joints: AssemblyJoint, model, model, "Assembly joint arena.", [serde(skip)];
            drawings: Drawing, model, model, "Drawing page, resource, view, and annotation arena.", [];
            semantic_annotations: SemanticAnnotation, model, model, "Semantic dimension, note, symbol, and callout arena.", [];
            presentation_documents: PresentationDocument, model, model, "Document presentation arena.", [];
            view_presentations: ViewPresentation, model, model, "View-provider presentation arena.", [];
            tessellations: Tessellation, model, model, "Tessellation arena.", [];
            appearances: Appearance, model, model, "Appearance arena.", [];
            appearance_bindings: AppearanceBinding, model, model, "Appearance binding arena.", [];
            attributes: SourceAttribute, model, model, "Attribute arena.", [];
            pmi: crate::pmi::PmiAnnotation, model, model, "Product-manufacturing information arena.", [];
            presentation_layers: crate::presentation::PresentationLayer, model, model, "Presentation layer arena.", [];
            physical_materials: crate::physics::PhysicalMaterial, model, model, "Physical material arena.", [];
            mass_properties: crate::physics::MassProperties, model, model, "Mass property arena.", [];
            rigid_bodies: crate::physics::RigidBody, model, model, "Rigid body arena.", [];
            colliders: crate::physics::Collider, model, model, "Collider arena.", [];
            articulations: crate::physics::Articulation, model, model, "Articulation arena.", [];
            physics_scenes: crate::physics::PhysicsScene, model, model, "Physics scene arena.", [];
            solver_settings: crate::physics::solver::SolverSettings, model, model, "Solver settings arena.", [];
            declared_measures: crate::declared_measures::DeclaredMeasure, model, model, "Declared measure arena.", [];
            point_clouds: crate::point_cloud::PointCloud, model, model, "Point cloud arena.", [];
            history_bodies: crate::history_body::HistoryBody, design, model, "History body arena: bodies that only an earlier history state holds, each with a nest of its own.", [serde(skip)];
            scopes: crate::design::scope::DesignScope, design, design, "The design scopes, one per product definition that has a design.", [];
            containers: crate::design::container::Container, design, design, "The ordered and unordered containers of the scopes' histories.", [];
            features: crate::design::feature::Feature, design, design, "The features of the scopes' histories.", [];
            sketches: crate::design::ops::sketch::Sketch, design, design, "The planar sketches.", [];
            sketch_entities: crate::design::ops::sketch::SketchEntity<crate::design::ops::sketch::Planar>, design, design, "The entities of the planar sketches.", [];
            sketch_constraints: crate::design::ops::sketch::SketchConstraint<crate::design::ops::sketch::Planar>, design, design, "The constraints of the planar sketches.", [];
            spatial_sketches: crate::design::ops::sketch::SpatialSketch, design, design, "The spatial sketches.", [];
            spatial_sketch_entities: crate::design::ops::sketch::SketchEntity<crate::design::ops::sketch::Spatial>, design, design, "The entities of the spatial sketches.", [];
            spatial_sketch_constraints: crate::design::ops::sketch::SketchConstraint<crate::design::ops::sketch::Spatial>, design, design, "The constraints of the spatial sketches.", [];
            parameters: crate::design::parameter::DesignParameter, design, design, "The design parameters.", [];
            spreadsheets: crate::design::spreadsheet::Spreadsheet, design, design, "The sheets that own some of the design parameters.", [];
            sheet_metal_rules: crate::design::ops::sheet_metal::SheetMetalRule, design, design, "The sheet-metal defaults that several features and bodies share.", [];
            programs: crate::design::program::ParameterProgram, design, design, "The parameter programs.", [];
            configurations: crate::design::configuration::DesignConfiguration, design, design, "The configurations of the scopes.", [];
            arrangements: crate::design::configuration::Arrangement, design, design, "The arrangements of the scopes' occurrences.", [];
            publications: crate::design::configuration::Publication, design, design, "The publications: named entry points into the scopes.", [];
            evaluations: crate::design::evaluation::Evaluation, design, design, "The evaluations: the states of the scopes other than their current states.", [];
            setups: crate::process::setup::Setup, process, process, "The setups: placements of workpieces on a machine.", [];
            operations: crate::process::operation::Operation, process, process, "The operations and modifiers of the setups.", [];
            features: crate::process::feature::MachiningFeature, process, process, "The machining features.", [];
            groups: crate::process::groups::ProcessGroup, process, process, "The groups that operations share for values and geometry.", [];
            tools: crate::process::tool::Tool, process, process, "The tools.", [];
            presets: crate::process::tool::Preset, process, process, "The tool presets.", [];
            machines: crate::process::tool::Machine, process, process, "The machines.", [];
            programs: crate::process::setup::Program, process, process, "The NC programs.", [];
            parameters: crate::process::groups::ProcessParameter, process, process, "The process parameters.", [];
            toolpaths: crate::process::toolpath::Toolpath, process, process, "The toolpaths: the evaluations of the operations.", [];
            states: crate::process::state::ProcessState, process, process, "The process states: stored workpiece geometry along the process.", [];
        }
    };
}
pub(crate) use classified_arena_registry;

/// Every neutral arena of the `geometry` and `model` homes with its home
/// token, without its class token.
///
/// The records of the `design` and `process` homes have no identity rewrite,
/// and a design record shares the id of the `model` record it restates, so
/// identity indexes, drafts, and validators read only these.
macro_rules! arena_registry {
    ($macro:ident) => {
        $crate::document::classified_arena_registry! {
            @entries [$crate::document::strip_v11_homes] $macro
        }
    };
}
pub(crate) use arena_registry;

/// Every neutral arena of every home with its home token, without its class
/// token.
macro_rules! every_arena_registry {
    ($macro:ident) => {
        $crate::document::classified_arena_registry! {
            @entries [$crate::document::strip_arena_class] $macro
        }
    };
}
pub(crate) use every_arena_registry;

/// The `field: Type, class, home;` entry of every arena whose registry
/// attributes are exactly `serde(skip)`: the arenas a [`Model`] holds in
/// memory and the CADIR wire leaves out ([`ArenaName::WIRE_OMITTED`]).
macro_rules! wire_omitted_arenas {
    ($macro:ident) => {
        $crate::document::classified_arena_registry! {
            @entries [$crate::document::keep_wire_omitted] $macro
        }
    };
}
pub(crate) use wire_omitted_arenas;

macro_rules! keep_wire_omitted {
    (@munch $target:ident [$($kept:tt)*]) => {
        $target! { $($kept)* }
    };
    (@munch $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [serde(skip)] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        $crate::document::keep_wire_omitted! { @munch $target [$($kept)* $field: $ty, $class, $home;] $($rest)* }
    };
    (@munch $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        $crate::document::keep_wire_omitted! { @munch $target [$($kept)*] $($rest)* }
    };
    ($target:ident; $($entries:tt)*) => {
        $crate::document::keep_wire_omitted! { @munch $target [] $($entries)* }
    };
}
pub(crate) use keep_wire_omitted;

macro_rules! strip_arena_class {
    ($target:ident; $($field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        $target! { $($field: $ty, $home, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;)* }
    };
}
pub(crate) use strip_arena_class;

macro_rules! strip_v11_homes {
    (@keep $target:ident [$($kept:tt)*]) => {
        $target! { $($kept)* }
    };
    (@keep $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, design, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        $crate::document::strip_v11_homes! { @keep $target [$($kept)*] $($rest)* }
    };
    (@keep $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, process, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        $crate::document::strip_v11_homes! { @keep $target [$($kept)*] $($rest)* }
    };
    (@keep $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        $crate::document::strip_v11_homes! { @keep $target [$($kept)* $field: $ty, $home, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] $($rest)* }
    };
    ($target:ident; $($entries:tt)*) => {
        $crate::document::strip_v11_homes! { @keep $target [] $($entries)* }
    };
}
pub(crate) use strip_v11_homes;

/// A registered arena's name: its JSON key, qualified by its home in the
/// `design` and `process` homes, where the key alone also names an arena of
/// another home.
macro_rules! arena_label {
    ($field:ident, design) => {
        concat!("design.", stringify!($field))
    };
    ($field:ident, process) => {
        concat!("process.", stringify!($field))
    };
    ($field:ident, $home:ident) => {
        stringify!($field)
    };
}
pub(crate) use arena_label;

/// A registered arena's [`ArenaName`].
macro_rules! arena_name {
    ($field:ident, $class:ident, $home:ident) => {
        $crate::document::ArenaName::registered(
            $crate::document::arena_label!($field, $home),
            stringify!($field),
            $crate::document::arena_class!($class),
            $crate::document::arena_home!($home),
        )
    };
}
pub(crate) use arena_name;

/// Maps a registry class token to its [`ArenaClass`]; any other token fails
/// to compile.
macro_rules! arena_class {
    (model) => {
        cadmpeg_core::decode::ArenaClass::Model
    };
    (design) => {
        cadmpeg_core::decode::ArenaClass::Design
    };
    (process) => {
        cadmpeg_core::decode::ArenaClass::Process
    };
}
pub(crate) use arena_class;

/// Maps a registry home token to its [`ArenaHome`]; any other token fails to
/// compile.
macro_rules! arena_home {
    (geometry) => {
        $crate::document::ArenaHome::Geometry
    };
    (model) => {
        $crate::document::ArenaHome::Model
    };
    (design) => {
        $crate::document::ArenaHome::Design
    };
    (process) => {
        $crate::document::ArenaHome::Process
    };
}
pub(crate) use arena_home;

/// The [`CadIr`] field that holds a neutral arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArenaHome {
    /// [`CadIr::geometry`], the current geometry.
    Geometry,
    /// [`CadIr::model`], the model-class records and the design records that
    /// build them.
    Model,
    /// [`CadIr::design`].
    Design,
    /// [`CadIr::process`].
    Process,
}

impl ArenaHome {
    /// Every home, in wire order.
    pub const ALL: [Self; 4] = [Self::Geometry, Self::Model, Self::Design, Self::Process];

    /// The home whose JSON key is `key`.
    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|home| home.as_str() == key)
    }

    /// The document's JSON key for this home.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::Model => "model",
            Self::Design => "design",
            Self::Process => "process",
        }
    }
}

impl fmt::Display for ArenaHome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A procedural carrier's retained cache, written as the carrier's `solved`
/// geometry arm.
struct SolvedGeometryWire<'a, T>(&'a T);

impl<T: Serialize> Serialize for SolvedGeometryWire<'_, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Geometry", 2)?;
        state.serialize_field("kind", "solved")?;
        state.serialize_field("value", self.0)?;
        state.end()
    }
}

struct SurfaceWire<'a>(&'a Surface);

impl Serialize for SurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Surface", 4)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", &SolvedGeometryWire(cache))?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        if let Some(native_ref) = &self.0.native_ref {
            state.serialize_field("native_ref", native_ref)?;
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
        let mut state = serializer.serialize_struct("Curve", 5)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", &SolvedGeometryWire(cache))?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(range) = &self.0.parameter_range {
            state.serialize_field("parameter_range", range)?;
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        if let Some(native_ref) = &self.0.native_ref {
            state.serialize_field("native_ref", native_ref)?;
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
    ($model:expr, $field:ident) => {
        &$model.$field
    };
}

macro_rules! model_read_type {
    (procedural_surfaces, $ty:ty) => { Vec<ProceduralSurfaceRow> };
    (procedural_curves, $ty:ty) => { Vec<ProceduralCurveRow> };
    ($field:ident, $ty:ty) => { Vec<$ty> };
}

macro_rules! model_read_value {
    ($wire:expr, procedural_surfaces) => {
        Vec::new()
    };
    ($wire:expr, procedural_curves) => {
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

/// Splits the registry by home token and declares [`Topology`] from the
/// `geometry` arenas, [`Model`] from the `model` arenas, [`Design`] from the
/// `design` arenas, and [`Process`] from the `process` arenas.
macro_rules! declare_homes {
    (@split [$($geometry:tt)*] [$($model:tt)*] [$($design:tt)*] [$($process:tt)*] $field:ident: $ty:ty, geometry, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        declare_homes! { @split [$($geometry)* $field: $ty, geometry, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] [$($model)*] [$($design)*] [$($process)*] $($rest)* }
    };
    (@split [$($geometry:tt)*] [$($model:tt)*] [$($design:tt)*] [$($process:tt)*] $field:ident: $ty:ty, model, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        declare_homes! { @split [$($geometry)*] [$($model)* $field: $ty, model, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] [$($design)*] [$($process)*] $($rest)* }
    };
    (@split [$($geometry:tt)*] [$($model:tt)*] [$($design:tt)*] [$($process:tt)*] $field:ident: $ty:ty, design, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        declare_homes! { @split [$($geometry)*] [$($model)*] [$($design)* $field: $ty, design, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] [$($process)*] $($rest)* }
    };
    (@split [$($geometry:tt)*] [$($model:tt)*] [$($design:tt)*] [$($process:tt)*] $field:ident: $ty:ty, process, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        declare_homes! { @split [$($geometry)*] [$($model)*] [$($design)*] [$($process)* $field: $ty, process, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] $($rest)* }
    };
    (@split [$($geometry:tt)*] [$($model:tt)*] [$($design:tt)*] [$($process:tt)*]) => {
        declare_topology! { $($geometry)* }
        declare_model! { $($model)* }
        declare_design! { $($design)* }
        declare_process! { $($process)* }
    };
    ($($entries:tt)*) => {
        declare_homes! { @split [] [] [] [] $($entries)* }
    };
}

/// Methods every home struct derives from its own arenas.
macro_rules! declare_home_arenas {
    ($home:ident, $($field:ident: $ty:ty;)*) => {
        impl $home {
            /// Total number of admitted neutral entities across this home's arenas.
            pub fn entity_count(&self) -> usize {
                0 $(+ self.$field.len())*
            }

            /// Whether every arena is in increasing order of id, as
            /// [`Self::finalize`] leaves it.
            pub(crate) fn is_finalized(&self) -> bool {
                true $(&& arena_is_ordered(&self.$field))*
            }

            /// States one record of each id in each sorted arena whose copies
            /// hold one value, and returns each id whose copies differ, all of
            /// whose copies it keeps.
            pub(crate) fn repeated_ids(&mut self, home: &'static str) -> Vec<RepeatedId> {
                let mut repeated = Vec::new();
                $(drop_repeated_copies(
                    &mut self.$field,
                    crate::schema::ArenaRecord::record_identity,
                    &crate::give_up::Arena::Home { home, field: stringify!($field) },
                    &mut repeated,
                );)*
                repeated
            }

            /// The id of the record at `index` of the arena whose wire key is
            /// `arena`.
            pub(crate) fn id_at(&self, arena: &str, index: usize) -> Option<String> {
                $(if arena == stringify!($field) {
                    return self
                        .$field
                        .get(index)
                        .map(|record| crate::schema::ArenaRecord::record_identity(record).to_owned());
                })*
                None
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(if !arena_is_ordered(&self.$field) {
                    self.$field.sort_by(|left, right| {
                        crate::schema::ArenaRecord::record_identity(left)
                            .cmp(crate::schema::ArenaRecord::record_identity(right))
                    });
                })*
            }

            fn repro_select_arenas(&self, ids: &std::collections::BTreeSet<String>) -> Self {
                use crate::schema::ArenaRecord;
                let mut selected = Self::default();
                $(selected.$field = self.$field.iter()
                    .filter(|entity| ids.contains(entity.record_identity())).cloned().collect();)*
                selected
            }
        }
    };
}

/// The merges of a home whose records rewrite their identities.
macro_rules! declare_home_rewrite {
    ($home:ident, $($field:ident: $ty:ty;)*) => {
        impl $home {
            fn append_arenas(&mut self, other: &mut Self) {
                $(self.$field.append(&mut other.$field);)*
            }

            fn extend_rewritten_arenas<R: EntityRewrite>(
                &mut self, other: &mut Self, rewrite: &mut R,
            ) -> Result<(), ModelRewriteError<R::Error>> {
                $(
                    for entity in std::mem::take(&mut other.$field) {
                        self.$field.push(rewrite.rewrite(entity).map_err(ModelRewriteError::Rewrite)?);
                    }
                )*
                Ok(())
            }
        }
    };
}

macro_rules! declare_topology {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// The current geometry: B-rep topology, its carriers, and
        /// subdivision surfaces, each arena sorted by identity.
        #[derive(Debug, Clone, Default, PartialEq, cadmpeg_derive::StructuralOrd)]
        pub struct Topology {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
        }

        declare_home_arenas!(Topology, $($field: $ty;)*);
        declare_home_rewrite!(Topology, $($field: $ty;)*);

        impl crate::schema::rewrite::typed::RewriteIdentities for Topology {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("nested topology references")?;
                let mut local = std::collections::HashSet::new();
                $(for entity in &self.$field {local.insert(entity.id.as_str());})*
                $(self.$field.visit_identity_references(ctx, &mut |id| if local.contains(id) {Ok(())} else {visitor(id)})?;)*
                Ok(())
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, map: &mut crate::schema::rewrite::typed::IdentityMap<'_,F>) -> Result<Self,cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("nested topology identities")?;
                Ok(Self {$($field: self.$field.rewrite_identities(ctx,map)?,)*})
            }
        }

        #[cfg(feature = "schema")]
        impl JsonSchema for Topology {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                "Topology".into()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                concat!(module_path!(), "::Topology").into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let mut schema = TopologyReadWire::json_schema(generator);
                schema.ensure_object().remove("additionalProperties");
                schema.ensure_object().insert(
                    "description".into(),
                    "The current geometry: B-rep topology, its carriers, and subdivision surfaces.".into(),
                );
                schema
            }
        }

        #[derive(Serialize)]
        struct TopologyWriteWire<'a> {
            $(
                $(#[$attribute])*
                $field: model_write_type!($field, $ty, 'a),
            )*
        }

        #[derive(Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(rename = "Topology"))]
        #[serde(deny_unknown_fields)]
        struct TopologyReadWire {
            $(
                $(#[$attribute])*
                $($(#[$schema_attr])*)?
                #[doc = $doc]
                $field: model_read_type!($field, $ty),
            )*
        }

        impl Serialize for Topology {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                if !self.is_finalized() {
                    let mut sorted = self.clone();
                    sorted.finalize();
                    return sorted.serialize(serializer);
                }
                TopologyWriteWire {
                    $($field: model_write_value!(self, $field),)*
                }
                .serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for Topology {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let mut wire = TopologyReadWire::deserialize(deserializer)?;
                let procedural_surfaces = std::mem::take(&mut wire.procedural_surfaces);
                let procedural_curves = std::mem::take(&mut wire.procedural_curves);
                let mut topology = Self {
                    $($field: model_read_value!(wire, $field),)*
                };
                if !procedural_surfaces.is_empty() {
                    let mut admission = procedural::ReconstructedAdmission::new(topology.surfaces.iter().map(|carrier| {
                        (carrier.id.as_str(), carrier.geometry.procedural_construction().map(|id| id.as_str()))
                    }));
                    for wire in procedural_surfaces {
                        let (owner, procedural) = wire.into_parts();
                        let slot = admission.owner("surface", owner.as_str(), procedural.id.as_str()).map_err(serde::de::Error::custom)?;
                        admission.accept(procedural.id.as_str());
                        topology
                            .attach_procedural_surface(&owner, slot, procedural)
                            .map_err(serde::de::Error::custom)?;
                    }
                }
                if !procedural_curves.is_empty() {
                    let mut admission = procedural::ReconstructedAdmission::new(topology.curves.iter().map(|carrier| {
                        (carrier.id.as_str(), carrier.geometry.procedural_construction().map(|id| id.as_str()))
                    }));
                    for wire in procedural_curves {
                        let (owner, procedural) = wire.into_parts();
                        let slot = admission.owner("curve", owner.as_str(), procedural.id.as_str()).map_err(serde::de::Error::custom)?;
                        admission.accept(procedural.id.as_str());
                        topology
                            .attach_procedural_curve(&owner, slot, procedural)
                            .map_err(serde::de::Error::custom)?;
                    }
                }
                Ok(topology)
            }
        }

        /// Every topology arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedTopology<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Topology {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedTopology<'_> {
                SortedTopology { $($field: sorted_model_value!(self, $field),)* }
            }

            /// Moves every arena from another topology.
            pub(crate) fn append(&mut self, mut other: Self) {
                self.append_arenas(&mut other);
            }

            /// The records of this topology whose identity `ids` holds.
            pub(crate) fn repro_select(&self, ids: &std::collections::BTreeSet<String>) -> Self {
                self.repro_select_arenas(ids)
            }

            /// Append every arena of `other` onto the matching arena of this
            /// topology, passing each entity through `rewrite`.
            ///
            /// One entity is handed to `rewrite` at a time, which bounds a
            /// rewriting caller's transient storage by the largest single
            /// entity rather than by the whole topology.
            pub fn extend_rewritten<R: EntityRewrite>(
                &mut self, mut other: Self, rewrite: &mut R,
            ) -> Result<(), ModelRewriteError<R::Error>> {
                self.extend_rewritten_arenas(&mut other, rewrite)
            }
        }
    };
}

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Product structure, presentation, physics, assets, drawings, PMI,
        /// and the design records that build the model, connected by typed IDs.
        #[derive(Debug, Clone, Default)]
        pub struct Model {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
            pub(crate) feature_regeneration_parents: FeatureRegenerationParents,
            pub(crate) omitted: OmittedArenas,
        }

        /// Two models are equal when they hold the same records. Whether the
        /// arenas the wire omits were read is no part of a model's value.
        impl PartialEq for Model {
            fn eq(&self, other: &Self) -> bool {
                $(self.$field == other.$field &&)*
                    self.feature_regeneration_parents == other.feature_regeneration_parents
            }
        }

        declare_home_arenas!(Model, $($field: $ty;)*);
        declare_home_rewrite!(Model, $($field: $ty;)*);

        impl Model {
            /// Moves every arena and feature-parent relation from another model.
            pub(crate) fn append(&mut self, mut other: Self) {
                self.omitted = self.omitted.merged(&other.omitted);
                self.append_arenas(&mut other);
                self.feature_regeneration_parents.0
                    .append(&mut other.feature_regeneration_parents.0);
            }

            /// The records and feature-parent relations of this model whose
            /// identities `ids` holds.
            pub(crate) fn repro_select(&self, ids: &std::collections::BTreeSet<String>) -> Self {
                let mut selected = self.repro_select_arenas(ids);
                if self.omitted == OmittedArenas::NotRead {
                    selected.omitted = OmittedArenas::NotRead;
                }
                selected.feature_regeneration_parents.0 = self.feature_regeneration_parents.0.iter()
                    .filter(|(child, parent)| ids.contains(child.as_str()) && ids.contains(parent.as_str()))
                    .map(|(child, parent)| (child.clone(), parent.clone())).collect();
                selected
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
                &mut self, mut other: Self, rewrite: &mut R,
            ) -> Result<(), ModelRewriteError<R::Error>> {
                self.extend_rewritten_arenas(&mut other, rewrite)?;
                self.omitted = self.omitted.merged(&other.omitted);
                for (child, parent) in other.feature_regeneration_parents.0 {
                    let edge = rewrite.rewrite(FeatureRegenerationEdge { child, parent }).map_err(ModelRewriteError::Rewrite)?;
                    self.feature_regeneration_parents.0.insert(edge.child, edge.parent);
                }
                Ok(())
            }
        }
    };
}

macro_rules! declare_design {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// The design class's arenas: scopes, containers, features, sketches,
        /// parameters, spreadsheets, programs, configurations, arrangements,
        /// publications, and evaluations, and the names of the current
        /// geometry.
        ///
        /// A record here keeps the id of the `model` record it restates, so
        /// the design arenas are a namespace of their own.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(deny_unknown_fields)]
        pub struct Design {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
            /// The names of the entities of the current geometry.
            pub names: crate::design::names::NameTable,
        }

        declare_home_arenas!(Design, $($field: $ty;)*);

        /// Every design arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedDesign<'a> {
            $($field: Vec<&'a $ty>,)*
            names: &'a crate::design::names::NameTable,
        }

        impl Design {
            /// Whether every design arena and the current name table are empty.
            pub(crate) fn is_empty(&self) -> bool {
                $(self.$field.is_empty() &&)* self.names.entries().is_empty()
            }

            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedDesign<'_> {
                SortedDesign {
                    $($field: sorted_rows(&self.$field, |value| value),)*
                    names: &self.names,
                }
            }

            /// The records of this design whose identity `ids` holds, and the
            /// names of the entities `ids` holds.
            pub(crate) fn repro_select(&self, ids: &std::collections::BTreeSet<String>) -> Self {
                let mut selected = self.repro_select_arenas(ids);
                selected.names = self
                    .names
                    .entries()
                    .iter()
                    .filter(|(entity, _)| ids.contains(entity.id()))
                    .map(|(entity, entry)| (entity.clone(), entry.clone()))
                    .collect::<BTreeMap<_, _>>()
                    .into();
                selected
            }
        }
    };
}

macro_rules! declare_process {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// The process class's arenas: setups, operations, machining
        /// features, groups, tools, presets, machines, programs, parameters,
        /// toolpaths, and process states.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(deny_unknown_fields)]
        pub struct Process {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
        }

        declare_home_arenas!(Process, $($field: $ty;)*);

        /// Every process arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedProcess<'a> {
            $($field: Vec<&'a $ty>,)*
        }

        impl Process {
            /// Whether every process arena is empty.
            pub(crate) fn is_empty(&self) -> bool {
                $(self.$field.is_empty() &&)* true
            }

            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedProcess<'_> {
                SortedProcess {
                    $($field: sorted_rows(&self.$field, |value| value),)*
                }
            }

            /// The records of this process whose identity `ids` holds.
            pub(crate) fn repro_select(&self, ids: &std::collections::BTreeSet<String>) -> Self {
                self.repro_select_arenas(ids)
            }
        }
    };
}

/// Methods that walk every neutral arena of a document, in registry order.
macro_rules! declare_document_arenas {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        impl<'a> Arenas<'a> {
            /// Returns the identity at one canonical arena slot.
            pub(crate) fn identity_at(
                self,
                kind: crate::schema::EntityKind,
                index: usize,
            ) -> Option<&'a str> {
                $(if kind == <$ty as crate::schema::ArenaRecord>::KIND {
                    return self
                        .$home
                        .$field
                        .get(index)
                        .map(crate::schema::ArenaRecord::record_identity);
                })*
                None
            }

            /// Total number of admitted neutral entities across all arenas.
            pub(crate) fn entity_count(self) -> usize {
                self.geometry.entity_count()
                    + self.model.entity_count()
                    + self.design.entity_count()
                    + self.process.entity_count()
            }

            /// Each record that names an id of `gone` other than by its own
            /// id, with the paths at which it names one (§3.3).
            pub(crate) fn holders(self, gone: &std::collections::BTreeSet<String>) -> Vec<crate::give_up::Holder> {
                use crate::schema::ArenaRecord;
                let mut found = Vec::new();
                $(found.extend(self.$home.$field.iter().filter_map(|entity| {
                    crate::give_up::Holder::of(stringify!($home), stringify!($field), entity.record_identity(), entity, gone)
                }));)*
                found
            }

            pub(crate) fn repro_graph(self, finding: &crate::report::check::Finding) -> Result<crate::repro::selection::Graph<'a>, CodecError> {
                use crate::schema::ArenaRecord;
                let roots: std::collections::BTreeSet<&str> = finding.entity.iter().chain(&finding.related).map(String::as_str).collect();
                let mut found = false;
                $(found |= self.$home.$field.iter().any(|entity| roots.contains(entity.record_identity()));)*
                let mut graph = crate::repro::selection::Graph::default();
                $(for entity in self.$home.$field.iter() {
                    graph.add(arena_label!($field, $home), entity.record_identity());
                })*
                graph.index();
                if !found { return Ok(graph); }
                $(for entity in self.$home.$field.iter() {
                    graph.references(entity)?;
                })*
                for region in &self.geometry.regions {
                    graph.child(region.body.as_str(), region.id.as_str());
                }
                for face in &self.geometry.faces {
                    graph.child(face.shell.as_str(), face.id.as_str());
                }
                Ok(graph)
            }
        }

        impl CadIr {
            /// The canonical JSON text of the records of `id` in the arena
            /// `field` of `home`, or `None` where the arena holds none.
            pub(crate) fn record_text(&self, home: &str, field: &str, id: &str) -> Option<String> {
                use crate::schema::ArenaRecord;
                $(if home == stringify!($home) && field == stringify!($field) {
                    let copies: Vec<&$ty> = self.$home.$field
                        .iter()
                        .filter(|entity| entity.record_identity() == id)
                        .collect();
                    return (!copies.is_empty()).then(|| crate::give_up::record_text(&copies));
                })*
                None
            }

            /// Removes every record of `id` from the arena `field` of `home`,
            /// returning the number removed.
            pub(crate) fn remove_records(&mut self, home: &str, field: &str, id: &str) -> usize {
                use crate::schema::ArenaRecord;
                $(if home == stringify!($home) && field == stringify!($field) {
                    let before = self.$home.$field.len();
                    self.$home.$field.retain(|entity| entity.record_identity() != id);
                    return before - self.$home.$field.len();
                })*
                0
            }

            /// Whether a record of any arena, neutral or native, has the id
            /// `id`.
            pub(crate) fn holds_id(&self, id: &str) -> bool {
                use crate::schema::ArenaRecord;
                $(self.$home.$field.iter().any(|entity| entity.record_identity() == id) ||)*
                    self.native.holds_id(id)
            }
        }
    };
}

macro_rules! assert_entity_schemas {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        const _: fn() = || {
            fn assert_schema<T: crate::schema::EntitySchema>() {}
            $(assert_schema::<$ty>();)*
        };
    };
}

macro_rules! assert_arena_records {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        const _: fn() = || {
            fn assert_record<T: crate::schema::ArenaRecord>() {}
            $(assert_record::<$ty>();)*
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

// Public arenas can change between calls. Check their current order instead
// of retaining a flag that direct mutations could leave stale.
fn arena_is_ordered<T: crate::schema::ArenaRecord>(values: &[T]) -> bool {
    values
        .windows(2)
        .all(|pair| pair[0].record_identity() <= pair[1].record_identity())
}

/// An id a sorted arena held more than once, in copies that differ.
#[derive(Debug)]
pub(crate) struct RepeatedId {
    /// The arena.
    pub(crate) arena: crate::give_up::Arena,
    /// The repeated id.
    pub(crate) id: String,
    /// The number of copies the decoder emitted.
    pub(crate) copies: usize,
}

/// States one record for each id of a sorted arena whose copies hold one
/// value, and records in `repeated` each id whose copies differ, keeping
/// every copy of it for the give-up (§2.8).
pub(crate) fn drop_repeated_copies<T: PartialEq>(
    records: &mut Vec<T>,
    id: impl Fn(&T) -> &str,
    arena: &crate::give_up::Arena,
    repeated: &mut Vec<RepeatedId>,
) {
    let mut run: Option<(RepeatedId, bool)> = None;
    records.dedup_by(|later, earlier| {
        if id(later) != id(earlier) {
            return false;
        }
        match run.as_mut() {
            Some((current, differs)) if current.id == id(later) => {
                current.copies += 1;
                *differs |= later != earlier;
            }
            _ => {
                repeated.extend(
                    run.take()
                        .filter(|(_, differs)| *differs)
                        .map(|(repeated_id, _)| repeated_id),
                );
                run = Some((
                    RepeatedId {
                        arena: arena.clone(),
                        id: id(later).to_owned(),
                        copies: 2,
                    },
                    later != earlier,
                ));
            }
        }
        later == earlier
    });
    repeated.extend(
        run.filter(|(_, differs)| *differs)
            .map(|(repeated_id, _)| repeated_id),
    );
}

fn sorted_rows<'a, T: crate::schema::ArenaRecord, U>(
    entities: &'a [T],
    project: impl FnMut(&'a T) -> U,
) -> Vec<U> {
    let mut refs = entities.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.record_identity().cmp(right.record_identity()));
    refs.into_iter().map(project).collect()
}

macro_rules! declare_arena_name {
    ($($field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Name of a registered neutral arena.
        ///
        /// Values use the `arena_registry!` field names so a new arena is
        /// counted and diffed under its declared wire key. An arena of the
        /// `design` home is named `design.<key>`, since its key alone also
        /// names an arena of the `model` home.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct ArenaName {
            name: &'static str,
            key: &'static str,
            class: ArenaClass,
            home: ArenaHome,
        }

        impl ArenaName {
            /// Every registered arena, in registry order.
            pub const ALL: &'static [Self] = &[$(arena_name!($field, $class, $home)),*];

            pub(crate) const fn registered(name: &'static str, key: &'static str, class: ArenaClass, home: ArenaHome) -> Self {
                Self { name, key, class, home }
            }

            /// The arena's name: its JSON key, qualified by its home in the
            /// `design` home.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                self.name
            }

            /// The arena's JSON object key inside its home.
            #[must_use]
            pub const fn key(self) -> &'static str {
                self.key
            }

            /// The class of the records this arena holds.
            #[must_use]
            pub const fn class(self) -> ArenaClass {
                self.class
            }

            /// The document field that holds this arena.
            #[must_use]
            pub const fn home(self) -> ArenaHome {
                self.home
            }

            /// Parse an arena name.
            #[must_use]
            pub fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|arena| arena.as_str() == name)
            }
        }

        impl Arenas<'_> {
            /// The first registered arena of `class` that holds a record.
            pub(crate) fn populated_arena(self, class: ArenaClass) -> Option<ArenaName> {
                $(if arena_class!($class) == class && !self.$home.$field.is_empty() {
                    return Some(arena_name!($field, $class, $home));
                })*
                None
            }

            /// Whether an arena of `class` in `home` holds a record.
            pub(crate) fn holds(self, class: ArenaClass, home: ArenaHome) -> bool {
                $(if arena_class!($class) == class && arena_home!($home) == home && !self.$home.$field.is_empty() {
                    return true;
                })*
                false
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
pub const IR_VERSION: &str = "11";

/// The current IR wire version. Every value writes [`IR_VERSION`].
/// Deserialization refuses any other version.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IrVersion;

macro_rules! count_arenas {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        [$(stringify!($field)),*].len()
    };
}

/// The number of arenas of the `geometry` and `model` homes. Their kinds
/// take the first canonical indices.
pub(crate) const MODEL_ARENA_COUNT: usize = arena_registry!(count_arenas);

every_arena_registry!(declare_homes);
arena_registry!(assert_entity_schemas);
every_arena_registry!(assert_arena_records);
every_arena_registry!(declare_document_arenas);
classified_arena_registry!(declare_arena_name);

/// Declares the CADIR wire form of [`Model`] from the arenas the wire carries:
/// its serializer, its reader, its schema, and its digest view.
macro_rules! declare_model_wire {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
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
                    "Product structure, presentation, physics, assets, drawings, PMI, and the design records that build the model, connected by typed IDs.".into(),
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
                if !self.is_finalized() {
                    let mut sorted = self.clone();
                    sorted.finalize();
                    return sorted.serialize(serializer);
                }
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
                Ok(Self {
                    $($field: model_read_value!(wire, $field),)*
                    omitted: OmittedArenas::NotRead,
                    ..Self::default()
                })
            }
        }

        /// Every model arena the wire carries, borrowed in canonical identity
        /// order.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Model {
            /// Borrow every arena the wire carries in canonical identity order.
            pub(crate) fn sorted(&self) -> Result<SortedModel<'_>, CodecError> {
                if let Err(error) = feature_parents::validate(&[self]) {
                    return Err(CodecError::malformed(format_args!("{error}")));
                }
                Ok(SortedModel { $($field: sorted_model_value!(self, $field),)* })
            }
        }
    };
}

/// The entries of the `model` home that the CADIR wire carries: every entry
/// whose registry attributes are other than `serde(skip)`.
macro_rules! keep_model_wire {
    (@munch $target:ident [$($kept:tt)*]) => {
        $target! { $($kept)* }
    };
    (@munch $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [serde(skip)] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        keep_model_wire! { @munch $target [$($kept)*] $($rest)* }
    };
    (@munch $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, model, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        keep_model_wire! { @munch $target [$($kept)* $field: $ty, model, $doc, [$($attribute),*] $(, [$($schema_attr),*])?;] $($rest)* }
    };
    (@munch $target:ident [$($kept:tt)*] $field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; $($rest:tt)*) => {
        keep_model_wire! { @munch $target [$($kept)*] $($rest)* }
    };
    ($target:ident; $($entries:tt)*) => {
        keep_model_wire! { @munch $target [] $($entries)* }
    };
}

classified_arena_registry! { @entries [keep_model_wire] declare_model_wire }

macro_rules! declare_wire_omitted {
    ($($field:ident: $ty:ty, $class:ident, $home:ident;)*) => {
        impl ArenaName {
            /// The arenas of the `model` home that a decoded [`Model`] holds in
            /// memory and the CADIR wire leaves out. A model read from CADIR JSON
            /// holds none of their records.
            pub const WIRE_OMITTED: &'static [Self] = &[$(arena_name!($field, $class, $home)),*];
        }

        /// The arenas the wire omits, with the feature-parent relations their
        /// features carry, borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedOmittedArenas<'a> {
            $($field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Model {
            /// Drops what the CADIR wire omits: the records of those arenas,
            /// their feature-parent relations, and `ProductDefinition.sketches`.
            /// The model then holds what its CADIR JSON reads back as.
            pub fn drop_omitted_arenas(&mut self) {
                $(self.$field.clear();)*
                self.feature_regeneration_parents = FeatureRegenerationParents::default();
                for definition in &mut self.product_definitions {
                    definition.sketches.clear();
                }
                self.omitted = OmittedArenas::NotRead;
            }

            /// Takes what the CADIR wire omits from `decoded`, the decode of
            /// the source this model was saved from: the records of those
            /// arenas, their feature-parent relations, the sketches of each
            /// product definition both models hold, and the digest the
            /// decode wrapper recorded of them. It undoes
            /// [`Self::drop_omitted_arenas`] for a model whose saved source is
            /// at hand.
            pub fn adopt_omitted_arenas(&mut self, decoded: &Self) {
                $(self.$field.clone_from(&decoded.$field);)*
                self.feature_regeneration_parents
                    .clone_from(&decoded.feature_regeneration_parents);
                for definition in &mut self.product_definitions {
                    definition.sketches = decoded
                        .product_definitions
                        .iter()
                        .find(|candidate| candidate.id == definition.id)
                        .map(|candidate| candidate.sketches.clone())
                        .unwrap_or_default();
                }
                self.omitted.clone_from(&decoded.omitted);
            }

            /// The identities of the records of the arenas the wire omits.
            #[must_use]
            pub fn omitted_record_ids(&self) -> std::collections::BTreeSet<&str> {
                use crate::schema::ArenaRecord;
                let mut ids = std::collections::BTreeSet::new();
                $(ids.extend(self.$field.iter().map(|record| record.record_identity()));)*
                ids
            }

            /// Borrow the arenas the wire omits in canonical identity order.
            pub(crate) fn sorted_omitted(&self) -> SortedOmittedArenas<'_> {
                SortedOmittedArenas { $($field: sorted_model_value!(self, $field),)* }
            }
        }
    };
}

wire_omitted_arenas!(declare_wire_omitted);

impl Model {
    /// Whether this model holds the arenas the CADIR wire leaves out
    /// ([`ArenaName::WIRE_OMITTED`]). A model read from CADIR JSON does not:
    /// its empty arenas there state no deletion, and a writer that applies
    /// edits from them has nothing to compare.
    #[must_use]
    pub fn omitted_arenas_held(&self) -> bool {
        matches!(self.omitted, OmittedArenas::Held { .. })
    }

    /// The digest of the arenas the wire omits that the decode wrapper
    /// recorded, while the model holds them.
    pub(crate) fn omitted_arenas_baseline(&self) -> Option<&str> {
        match &self.omitted {
            OmittedArenas::Held { baseline } => baseline.as_deref(),
            OmittedArenas::NotRead => None,
        }
    }

    /// The arenas the wire omits as one JSON object keyed by arena, each
    /// record in its own JSON form and the arenas in canonical identity order,
    /// for a caller that compares two documents in one process.
    pub fn omitted_arenas_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self.sorted_omitted())
    }

    /// Record `digest` as the baseline of the arenas the wire omits.
    pub(crate) fn record_omitted_arenas_baseline(&mut self, digest: String) {
        if let OmittedArenas::Held { baseline } = &mut self.omitted {
            *baseline = Some(digest);
        }
    }
}

/// Borrowed geometry and topology arenas for an embedded semantic record.
pub struct GeometrySnapshot<'a> {
    geometry: &'a Topology,
    tessellations: &'a [Tessellation],
    kind: &'a str,
    curves: ProceduralCurveRows<'a>,
    surfaces: ProceduralSurfaceRows<'a>,
}

impl Topology {
    /// Admit `model`'s feature parents and indexed owners for a borrowed
    /// geometry serialization view that embeds `model`'s tessellations.
    pub fn geometry_snapshot<'a>(
        &'a self,
        model: &'a Model,
        kind: &'a str,
    ) -> Result<GeometrySnapshot<'a>, CodecError> {
        if let Err(error) = feature_parents::validate(&[model]) {
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
            geometry: self,
            tessellations: &model.tessellations,
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
        let geometry = self.geometry;
        let mut map = serializer.serialize_map(Some(16))?;
        map.serialize_entry("bodies", &geometry.bodies)?;
        map.serialize_entry("coedges", &geometry.coedges)?;
        map.serialize_entry("curves", &CurveRows(&geometry.curves))?;
        map.serialize_entry("edges", &geometry.edges)?;
        map.serialize_entry("faces", &geometry.faces)?;
        map.serialize_entry("kind", self.kind)?;
        map.serialize_entry("loops", &geometry.loops)?;
        map.serialize_entry("pcurves", &geometry.pcurves)?;
        map.serialize_entry("points", &geometry.points)?;
        map.serialize_entry("procedural_curves", &self.curves)?;
        map.serialize_entry("procedural_surfaces", &self.surfaces)?;
        map.serialize_entry("regions", &geometry.regions)?;
        map.serialize_entry("shells", &geometry.shells)?;
        map.serialize_entry("surfaces", &SurfaceRows(&geometry.surfaces))?;
        map.serialize_entry("tessellations", self.tessellations)?;
        map.serialize_entry("vertices", &geometry.vertices)?;
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

impl Topology {
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
        return Err(E::custom(legacy::unsupported_version(version)));
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

/// The neutral arenas of one document, borrowed: its current geometry, its
/// model, its design, and its process.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arenas<'a> {
    pub(crate) geometry: &'a Topology,
    pub(crate) model: &'a Model,
    pub(crate) design: &'a Design,
    pub(crate) process: &'a Process,
}

/// A versioned CAD document.
///
/// Construction state machine: [`crate::draft::ModelDraft`] (mutable, indexed)
/// commits into [`CadIr`] (structurally canonical after [`CadIr::finalize`]);
/// [`crate::report::check::ValidationReport`] is produced separately by `validate_neutral` and
/// is not embedded in the document.
///
/// `geometry` holds the current geometry and `model` the rest of the
/// format-neutral graph. `native` retains typed format-specific product data
/// without changing that graph's semantics. Entity IDs must be globally
/// unique across all document arenas. `ir_version` is a serialized constant
/// checked by the read adapter. Every arena of a class the
/// [`scope`](Self::scope) excludes is empty; serialization and parsing refuse
/// a document that holds a record in one.
#[derive(Debug, Clone, PartialEq)]
pub struct CadIr {
    /// Closed fragment of a larger document.
    pub fragment: bool,
    /// Metadata for a reproducible fragment.
    pub repro: Option<Box<crate::repro::BundleMetadata>>,
    scope: ContentScope,
    /// Source-container metadata.
    pub source: Option<SourceMeta>,
    /// Document-wide tolerances.
    pub tolerances: Tolerances,
    /// The current geometry.
    pub geometry: Topology,
    /// Product structure, presentation, physics, assets, drawings, PMI, and
    /// the design records that build them.
    pub model: Model,
    /// The design layer.
    pub design: Design,
    /// The manufacturing process.
    pub process: Process,
    /// Per-format native namespaces, each an arena map.
    pub native: Native,
}

/// A document scope that excludes a class the document needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeViolation {
    /// The excluded class.
    pub class: ArenaClass,
    /// The first arena of `class` that holds a record, the first arena of
    /// another class whose records reference `class`, or `None` when a class
    /// the scope holds requires `class`.
    pub arena: Option<ArenaName>,
}

impl fmt::Display for ScopeViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.arena {
            Some(arena) if arena.class() == self.class => write!(
                f,
                "the document's scope excludes {} but {} arena `{arena}` holds records",
                self.class, self.class
            ),
            Some(arena) => write!(
                f,
                "the document's scope excludes {} but {} arena `{arena}` holds a reference into it",
                self.class,
                arena.class()
            ),
            None => write!(
                f,
                "the document's scope excludes {}, which a class it holds requires",
                self.class
            ),
        }
    }
}

impl std::error::Error for ScopeViolation {}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CadIrWriteWire<'a> {
    ir_version: IrVersion,
    fragment: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    repro: Option<&'a crate::repro::BundleMetadata>,
    /// The classes the document holds.
    scope: ContentScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    geometry: &'a Topology,
    model: &'a Model,
    design: &'a Design,
    process: &'a Process,
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
    fragment: bool,
    #[serde(default, deserialize_with = "deserialize_repro")]
    repro: Option<Box<crate::repro::BundleMetadata>>,
    scope: ContentScope,
    #[serde(default, deserialize_with = "deserialize_source")]
    source: Option<SourceMeta>,
    #[serde(rename = "units")]
    _units: CanonicalUnitsWire,
    tolerances: Tolerances,
    geometry: Topology,
    model: Model,
    design: Design,
    process: Process,
    native: Native,
}

impl Serialize for CadIr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.check_scope().map_err(serde::ser::Error::custom)?;
        if !(self.design.is_finalized()
            && self.process.is_finalized()
            && self.native.is_finalized())
        {
            let mut sorted = self.clone();
            sorted.finalize();
            return sorted.serialize(serializer);
        }
        CadIrWriteWire {
            ir_version: IrVersion,
            fragment: self.fragment,
            repro: self.repro.as_deref(),
            scope: self.scope,
            source: self.source.as_ref(),
            units: CanonicalUnitsWire::default(),
            tolerances: &self.tolerances,
            geometry: &self.geometry,
            model: &self.model,
            design: &self.design,
            process: &self.process,
            native: &self.native,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CadIr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CadIrReadWire::deserialize(deserializer)?;
        check_ir_version(wire.ir_version.as_ref())?;
        wire.admit_state_chains()
            .map_err(serde::de::Error::custom)?;
        wire.try_into().map_err(serde::de::Error::custom)
    }
}

impl CadIrReadWire {
    /// Walks every chain of state bases, through the process states and the
    /// evaluations they rest on, under the state chain ceiling.
    fn admit_state_chains(&self) -> Result<(), String> {
        let evaluations = crate::design::evaluation::admit_state_chains(&self.design.evaluations)?;
        crate::process::state::admit_state_chains(&self.process.states, &evaluations)
    }
}

impl TryFrom<CadIrReadWire> for CadIr {
    type Error = ScopeViolation;

    fn try_from(wire: CadIrReadWire) -> Result<Self, Self::Error> {
        let ir = Self {
            fragment: wire.fragment,
            repro: wire.repro,
            scope: wire.scope,
            source: wire.source,
            tolerances: wire.tolerances,
            geometry: wire.geometry,
            model: wire.model,
            design: wire.design,
            process: wire.process,
            native: wire.native,
        };
        ir.check_scope()?;
        Ok(ir)
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
    /// use cadmpeg_ir::{CadIr, document::{Model, Topology}, native::Native};
    /// let mut ir = CadIr::empty();
    /// ir.try_append(Topology::default(), Model::default(), Native::default(), |candidate| {
    ///     candidate.geometry.points.clear();
    ///     Ok(Ok::<(), ()>(()))
    /// });
    /// ```
    pub fn try_append<T, E>(
        &mut self,
        geometry: Topology,
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
            ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {{
                $(let $field = self.$home.$field.len();)*
                self.geometry.append(geometry);
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
                    $(self.$home.$field.truncate($field);)*
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
    /// let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    /// let raw = serde_json::json!({"id": "test:native:unknown#0", "extra": true});
    /// CadIr::empty().set_native_unknowns_from(&ctx, "test", [raw]).unwrap();
    /// ```
    pub fn set_native_unknowns_from<T: Borrow<NativeUnknownRecord>, I: IntoIterator<Item = T>>(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        let codec = crate::native::namespace_codec(format)?;
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

        self.native
            .namespace_mut(codec)
            .arenas_mut()
            .insert(cadmpeg_core::nonblank_literal!("unknowns"), records);
        Ok(())
    }

    /// Construct an empty document with default tolerances and full scope.
    ///
    /// Fixtures and in-progress assembly use this constructor. Decoders that
    /// have classified source metadata use [`Self::decoded`].
    pub fn empty() -> Self {
        Self {
            fragment: false,
            repro: None,
            scope: ContentScope::full(),
            source: None,
            tolerances: Tolerances::default(),
            geometry: Topology::default(),
            model: Model::default(),
            design: Design::default(),
            process: Process::default(),
            native: Native::default(),
        }
    }

    /// Construct a decoded document with classified source metadata.
    ///
    /// The document holds full scope until the sealed decode wrapper records
    /// the scope the backend delivered.
    pub fn decoded(source: SourceMeta) -> Self {
        Self {
            source: Some(source),
            ..Self::empty()
        }
    }

    /// IR schema version emitted by serialization and accepted on deserialize.
    pub fn ir_version(&self) -> &str {
        IR_VERSION
    }

    /// The classes whose neutral arenas this document holds.
    #[must_use]
    pub const fn scope(&self) -> ContentScope {
        self.scope
    }

    /// Declare which neutral arena classes the document holds.
    ///
    /// # Errors
    ///
    /// Refuses, without changing the scope, a scope that holds a class
    /// without the class it requires, or that excludes a class with a
    /// populated arena or, for the design class, with a model record that
    /// references it.
    pub fn set_scope(&mut self, scope: ContentScope) -> Result<(), ScopeViolation> {
        self.check_scope_value(scope)?;
        self.scope = scope;
        Ok(())
    }

    /// Check the scope's class requirements, that every populated neutral
    /// arena belongs to a held class, and that no model record references
    /// the design class when the scope excludes it.
    ///
    /// # Errors
    ///
    /// Names the excluded class and its first populated arena, or the first
    /// model arena that references it, or the excluded class that a held
    /// class requires.
    pub fn check_scope(&self) -> Result<(), ScopeViolation> {
        self.check_scope_value(self.scope)
    }

    fn check_scope_value(&self, scope: ContentScope) -> Result<(), ScopeViolation> {
        if let Some(unmet) = scope.unmet_requirement() {
            return Err(ScopeViolation {
                class: unmet.required,
                arena: None,
            });
        }
        for class in ArenaClass::ALL {
            if !scope.holds(class) {
                if let Some(arena) = self.populated_arena(class) {
                    return Err(ScopeViolation {
                        class,
                        arena: Some(arena),
                    });
                }
            }
        }
        if !scope.holds(ArenaClass::Design) {
            if let Some(arena) = design_references::first(&self.model) {
                return Err(ScopeViolation {
                    class: ArenaClass::Design,
                    arena: Some(arena),
                });
            }
        }
        Ok(())
    }

    /// The first registered arena of `class` that holds a record.
    #[must_use]
    pub fn populated_arena(&self, class: ArenaClass) -> Option<ArenaName> {
        self.arenas().populated_arena(class)
    }

    /// Borrow the document's neutral arenas.
    pub(crate) const fn arenas(&self) -> Arenas<'_> {
        Arenas {
            geometry: &self.geometry,
            model: &self.model,
            design: &self.design,
            process: &self.process,
        }
    }

    /// Total number of admitted neutral entities across all arenas.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.arenas().entity_count()
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

    /// Parse JSON and reject any unsupported `ir_version`, or a document
    /// whose scope excludes a class it holds records of, or that nests
    /// deeper than a CADIR document's bound. Every float zero, `-0` and
    /// `-0.0` included, reads as 0 (§2.5).
    ///
    /// Version is probed first (`ir_version` only) so the full document is
    /// materialized once after the version gate.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        /// Every member but `ir_version` is skipped.
        #[derive(Deserialize)]
        struct VersionProbe {
            #[serde(default, deserialize_with = "deserialize_probe_version")]
            ir_version: Option<serde_json::Value>,
        }
        cadmpeg_core::named_optional_field!(
            deserialize_probe_version,
            serde_json::Value,
            "ir_version"
        );

        let probe = crate::bounds::read_json::<VersionProbe>(s)?;
        check_ir_version(probe.ir_version.as_ref())?;
        one_zero::from_str::<Self>(s).map_err(|error| {
            match legacy::omitted_arena_refusal(&error.to_string()) {
                Some(refusal) => serde::de::Error::custom(refusal),
                None => error,
            }
        })
    }

    /// Sorts every arena of the document's homes and native namespaces by
    /// id, then states one record of each id whose copies hold one value.
    /// Returns each id whose copies differ, all of whose copies stay for the
    /// give-up (§2.8). The arenas of a state's delta are left as they are.
    pub(crate) fn repeated_ids(&mut self) -> Vec<RepeatedId> {
        self.finalize();
        let mut repeated = self.geometry.repeated_ids("geometry");
        repeated.extend(self.model.repeated_ids("model"));
        repeated.extend(self.design.repeated_ids("design"));
        repeated.extend(self.process.repeated_ids("process"));
        repeated.extend(self.native.repeated_ids());
        repeated
    }

    /// The record at `place`, as the give-up names it.
    pub(crate) fn record_at(&self, place: &crate::bounds::Place) -> Option<crate::give_up::Seed> {
        use crate::bounds::Place;
        use crate::give_up::{Arena, Seed};
        match place {
            Place::Home { home, arena, index } => {
                let id = match home.as_str() {
                    "geometry" => self.geometry.id_at(arena, *index),
                    "model" => self.model.id_at(arena, *index),
                    "design" => self.design.id_at(arena, *index),
                    "process" => self.process.id_at(arena, *index),
                    _ => None,
                }?;
                Some(Seed {
                    arena: Arena::home(home, arena)?,
                    id,
                })
            }
            Place::Native {
                codec,
                arena,
                index,
            } => Some(Seed {
                id: self.native.id_at(codec, arena, *index)?,
                arena: Arena::Native {
                    codec: codec.clone(),
                    arena: arena.clone(),
                },
            }),
        }
    }

    /// Removes each id of `gone`, which no record of the document holds, from
    /// the collections keyed by their members' own ids: the current name
    /// table and the regeneration parents of features (§3.3).
    pub(crate) fn release_keyed(&mut self, gone: &std::collections::BTreeSet<String>) {
        if self
            .design
            .names
            .entries()
            .keys()
            .any(|entity| gone.contains(entity.id()))
        {
            self.design.names = self
                .design
                .names
                .entries()
                .iter()
                .filter(|(entity, _)| !gone.contains(entity.id()))
                .map(|(entity, entry)| (entity.clone(), entry.clone()))
                .collect::<BTreeMap<_, _>>()
                .into();
        }
        self.model
            .feature_regeneration_parents
            .0
            .retain(|child, parent| {
                !gone.contains(child.as_str()) && !gone.contains(parent.as_str())
            });
    }

    /// Sort geometry, model, design, process, native, and unknown-record
    /// arenas by identity.
    pub fn finalize(&mut self) {
        self.geometry.finalize();
        self.model.finalize();
        self.design.finalize();
        self.process.finalize();
        self.native.finalize();
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> BTreeMap<CensusKey, usize> {
        census::count(crate::native::view::NativeView::new(self, None))
    }
}

/// Source-container metadata preserved for reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceMeta {
    /// Format identity: classified dialect layers, or a bare known format.
    identity: FormatIdentity<DialectLayers>,
    /// Format-specific attributes.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub attributes: BTreeMap<NonBlankString, String>,
}

impl SourceMeta {
    pub(crate) fn repro_header(&self) -> Self {
        Self {
            identity: FormatIdentity::unclassified(self.identity.codec()),
            attributes: BTreeMap::new(),
        }
    }
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

cadmpeg_core::named_optional_field!(
    deserialize_repro,
    Box<crate::repro::BundleMetadata>,
    "repro"
);
