// SPDX-License-Identifier: Apache-2.0
//! Compile-time schema contract shared by every neutral model arena.

use serde::Serialize;

pub mod rewrite;
/// Caller-accounted structural JSON projection.
pub mod structural;

/// Canonical neutral arena kind.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    /// Body.
    Body,
    /// Region.
    Region,
    /// Shell.
    Shell,
    /// Face.
    Face,
    /// Loop.
    Loop,
    /// Coedge.
    Coedge,
    /// Edge.
    Edge,
    /// Vertex.
    Vertex,
    /// Point.
    Point,
    /// Surface.
    Surface,
    /// Curve.
    Curve,
    /// Subdivision surface.
    SubdSurface,
    /// Parameter-space curve.
    Pcurve,
    /// Procedural surface.
    ProceduralSurface,
    /// Procedural curve.
    ProceduralCurve,
    /// Embedded or externally referenced document resource.
    Asset,
    /// Feature.
    Feature,
    /// Feature input topology.
    FeatureInputTopology,
    /// Feature result topology.
    FeatureResultTopology,
    /// Design configuration of the `model` home.
    Configuration,
    /// Design parameter of the `model` home.
    Parameter,
    /// Planar sketch.
    Sketch,
    /// Planar sketch entity.
    SketchEntity,
    /// Planar sketch constraint.
    SketchConstraint,
    /// Spatial sketch.
    SpatialSketch,
    /// Spatial sketch entity.
    SpatialSketchEntity,
    /// Spatial sketch constraint.
    SpatialSketchConstraint,
    /// Spreadsheet.
    Spreadsheet,
    /// Product definition.
    ProductDefinition,
    /// Product occurrence.
    Occurrence,
    /// Assembly joint.
    AssemblyJoint,
    /// Drawing.
    Drawing,
    /// Semantic annotation.
    SemanticAnnotation,
    /// Presentation document.
    PresentationDocument,
    /// View presentation.
    ViewPresentation,
    /// Tessellation.
    Tessellation,
    /// Appearance asset.
    Appearance,
    /// Appearance binding.
    AppearanceBinding,
    /// Source attribute.
    SourceAttribute,
    /// Product-manufacturing annotation.
    PmiAnnotation,
    /// Presentation layer.
    PresentationLayer,
    /// Physical material.
    PhysicalMaterial,
    /// Mass properties.
    MassProperties,
    /// Rigid body.
    RigidBody,
    /// Collider.
    Collider,
    /// Articulation.
    Articulation,
    /// Physics scene.
    PhysicsScene,
    /// Solver settings.
    SolverSettings,
    /// Declared measure.
    DeclaredMeasure,
    /// Point cloud.
    PointCloud,
    /// History body.
    HistoryBody,
    /// Design scope.
    DesignScope,
    /// Design container.
    DesignContainer,
    /// Feature of the `design` home.
    DesignFeature,
    /// Planar sketch of the `design` home.
    DesignSketch,
    /// Planar sketch entity of the `design` home.
    DesignSketchEntity,
    /// Planar sketch constraint of the `design` home.
    DesignSketchConstraint,
    /// Spatial sketch of the `design` home.
    DesignSpatialSketch,
    /// Spatial sketch entity of the `design` home.
    DesignSpatialSketchEntity,
    /// Spatial sketch constraint of the `design` home.
    DesignSpatialSketchConstraint,
    /// Parameter of the `design` home.
    DesignParameter,
    /// Spreadsheet of the `design` home.
    DesignSpreadsheet,
    /// Sheet-metal rule.
    DesignSheetMetalRule,
    /// Parameter program.
    DesignProgram,
    /// Configuration of the `design` home.
    DesignConfiguration,
    /// Arrangement.
    DesignArrangement,
    /// Publication.
    DesignPublication,
    /// Evaluation.
    DesignEvaluation,
    /// Setup.
    ProcessSetup,
    /// Operation.
    ProcessOperation,
    /// Machining feature.
    ProcessFeature,
    /// Process group.
    ProcessGroup,
    /// Tool.
    ProcessTool,
    /// Tool preset.
    ProcessPreset,
    /// Machine.
    ProcessMachine,
    /// NC program.
    ProcessProgram,
    /// Process parameter.
    ProcessParameter,
    /// Toolpath.
    ProcessToolpath,
    /// Process state.
    ProcessState,
}

impl EntityKind {
    /// Position of this kind in the canonical arena registry.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Body => 0,
            Self::Region => 1,
            Self::Shell => 2,
            Self::Face => 3,
            Self::Loop => 4,
            Self::Coedge => 5,
            Self::Edge => 6,
            Self::Vertex => 7,
            Self::Point => 8,
            Self::Surface => 9,
            Self::Curve => 10,
            Self::SubdSurface => 11,
            Self::Pcurve => 12,
            Self::ProceduralSurface => 13,
            Self::ProceduralCurve => 14,
            Self::Asset => 15,
            Self::Feature => 16,
            Self::FeatureInputTopology => 17,
            Self::FeatureResultTopology => 18,
            Self::Configuration => 19,
            Self::Parameter => 20,
            Self::Sketch => 21,
            Self::SketchEntity => 22,
            Self::SketchConstraint => 23,
            Self::SpatialSketch => 24,
            Self::SpatialSketchEntity => 25,
            Self::SpatialSketchConstraint => 26,
            Self::Spreadsheet => 27,
            Self::ProductDefinition => 28,
            Self::Occurrence => 29,
            Self::AssemblyJoint => 30,
            Self::Drawing => 31,
            Self::SemanticAnnotation => 32,
            Self::PresentationDocument => 33,
            Self::ViewPresentation => 34,
            Self::Tessellation => 35,
            Self::Appearance => 36,
            Self::AppearanceBinding => 37,
            Self::SourceAttribute => 38,
            Self::PmiAnnotation => 39,
            Self::PresentationLayer => 40,
            Self::PhysicalMaterial => 41,
            Self::MassProperties => 42,
            Self::RigidBody => 43,
            Self::Collider => 44,
            Self::Articulation => 45,
            Self::PhysicsScene => 46,
            Self::SolverSettings => 47,
            Self::DeclaredMeasure => 48,
            Self::PointCloud => 49,
            Self::HistoryBody => 50,
            Self::DesignScope => 51,
            Self::DesignContainer => 52,
            Self::DesignFeature => 53,
            Self::DesignSketch => 54,
            Self::DesignSketchEntity => 55,
            Self::DesignSketchConstraint => 56,
            Self::DesignSpatialSketch => 57,
            Self::DesignSpatialSketchEntity => 58,
            Self::DesignSpatialSketchConstraint => 59,
            Self::DesignParameter => 60,
            Self::DesignSpreadsheet => 61,
            Self::DesignSheetMetalRule => 62,
            Self::DesignProgram => 63,
            Self::DesignConfiguration => 64,
            Self::DesignArrangement => 65,
            Self::DesignPublication => 66,
            Self::DesignEvaluation => 67,
            Self::ProcessSetup => 68,
            Self::ProcessOperation => 69,
            Self::ProcessFeature => 70,
            Self::ProcessGroup => 71,
            Self::ProcessTool => 72,
            Self::ProcessPreset => 73,
            Self::ProcessMachine => 74,
            Self::ProcessProgram => 75,
            Self::ProcessParameter => 76,
            Self::ProcessToolpath => 77,
            Self::ProcessState => 78,
        }
    }

    /// Every registered entity kind in canonical arena order.
    pub const ALL: [Self; 79] = [
        Self::Body,
        Self::Region,
        Self::Shell,
        Self::Face,
        Self::Loop,
        Self::Coedge,
        Self::Edge,
        Self::Vertex,
        Self::Point,
        Self::Surface,
        Self::Curve,
        Self::SubdSurface,
        Self::Pcurve,
        Self::ProceduralSurface,
        Self::ProceduralCurve,
        Self::Asset,
        Self::Feature,
        Self::FeatureInputTopology,
        Self::FeatureResultTopology,
        Self::Configuration,
        Self::Parameter,
        Self::Sketch,
        Self::SketchEntity,
        Self::SketchConstraint,
        Self::SpatialSketch,
        Self::SpatialSketchEntity,
        Self::SpatialSketchConstraint,
        Self::Spreadsheet,
        Self::ProductDefinition,
        Self::Occurrence,
        Self::AssemblyJoint,
        Self::Drawing,
        Self::SemanticAnnotation,
        Self::PresentationDocument,
        Self::ViewPresentation,
        Self::Tessellation,
        Self::Appearance,
        Self::AppearanceBinding,
        Self::SourceAttribute,
        Self::PmiAnnotation,
        Self::PresentationLayer,
        Self::PhysicalMaterial,
        Self::MassProperties,
        Self::RigidBody,
        Self::Collider,
        Self::Articulation,
        Self::PhysicsScene,
        Self::SolverSettings,
        Self::DeclaredMeasure,
        Self::PointCloud,
        Self::HistoryBody,
        Self::DesignScope,
        Self::DesignContainer,
        Self::DesignFeature,
        Self::DesignSketch,
        Self::DesignSketchEntity,
        Self::DesignSketchConstraint,
        Self::DesignSpatialSketch,
        Self::DesignSpatialSketchEntity,
        Self::DesignSpatialSketchConstraint,
        Self::DesignParameter,
        Self::DesignSpreadsheet,
        Self::DesignSheetMetalRule,
        Self::DesignProgram,
        Self::DesignConfiguration,
        Self::DesignArrangement,
        Self::DesignPublication,
        Self::DesignEvaluation,
        Self::ProcessSetup,
        Self::ProcessOperation,
        Self::ProcessFeature,
        Self::ProcessGroup,
        Self::ProcessTool,
        Self::ProcessPreset,
        Self::ProcessMachine,
        Self::ProcessProgram,
        Self::ProcessParameter,
        Self::ProcessToolpath,
        Self::ProcessState,
    ];
}

/// The kind and identity of a record of any registered arena.
///
/// Every [`EntitySchema`] record has them. A record of the `design` or
/// `process` home has them without the identity rewrite of [`EntitySchema`];
/// a design record shares the id of the `model` record it restates.
pub trait ArenaRecord {
    /// The record's canonical arena kind.
    const KIND: EntityKind;

    /// The record's identity within its home.
    fn record_identity(&self) -> &str;
}

impl<T: EntitySchema> ArenaRecord for T {
    const KIND: EntityKind = <T as EntitySchema>::KIND;

    fn record_identity(&self) -> &str {
        self.identity()
    }
}

macro_rules! impl_arena_record {
    ($type:ty, $kind:ident, $($identity:tt)+) => {
        impl ArenaRecord for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn record_identity(&self) -> &str {
                self.$($identity)+.as_str()
            }
        }
    };
}

impl_arena_record!(crate::design::scope::DesignScope, DesignScope, definition);
impl_arena_record!(crate::design::container::Container, DesignContainer, id());
impl_arena_record!(crate::design::feature::Feature, DesignFeature, id);
impl_arena_record!(crate::design::ops::sketch::Sketch, DesignSketch, id);
impl_arena_record!(
    crate::design::ops::sketch::SketchEntity<crate::design::ops::sketch::Planar>,
    DesignSketchEntity,
    id
);
impl_arena_record!(
    crate::design::ops::sketch::SketchConstraint<crate::design::ops::sketch::Planar>,
    DesignSketchConstraint,
    id
);
impl_arena_record!(
    crate::design::ops::sketch::SpatialSketch,
    DesignSpatialSketch,
    id
);
impl_arena_record!(
    crate::design::ops::sketch::SketchEntity<crate::design::ops::sketch::Spatial>,
    DesignSpatialSketchEntity,
    id
);
impl_arena_record!(
    crate::design::ops::sketch::SketchConstraint<crate::design::ops::sketch::Spatial>,
    DesignSpatialSketchConstraint,
    id
);
impl_arena_record!(
    crate::design::parameter::DesignParameter,
    DesignParameter,
    id
);
impl_arena_record!(
    crate::design::spreadsheet::Spreadsheet,
    DesignSpreadsheet,
    id
);
impl_arena_record!(
    crate::design::ops::sheet_metal::SheetMetalRule,
    DesignSheetMetalRule,
    id
);
impl_arena_record!(crate::design::program::ParameterProgram, DesignProgram, id);
impl_arena_record!(
    crate::design::configuration::DesignConfiguration,
    DesignConfiguration,
    id
);
impl_arena_record!(
    crate::design::configuration::Arrangement,
    DesignArrangement,
    id
);
impl_arena_record!(
    crate::design::configuration::Publication,
    DesignPublication,
    id
);
impl_arena_record!(crate::design::evaluation::Evaluation, DesignEvaluation, id);
impl_arena_record!(crate::process::setup::Setup, ProcessSetup, id);
impl_arena_record!(crate::process::operation::Operation, ProcessOperation, id);
impl_arena_record!(
    crate::process::feature::MachiningFeature,
    ProcessFeature,
    id
);
impl_arena_record!(crate::process::groups::ProcessGroup, ProcessGroup, id);
impl_arena_record!(crate::process::tool::Tool, ProcessTool, id);
impl_arena_record!(crate::process::tool::Preset, ProcessPreset, id);
impl_arena_record!(crate::process::tool::Machine, ProcessMachine, id);
impl_arena_record!(crate::process::setup::Program, ProcessProgram, id);
impl_arena_record!(
    crate::process::groups::ProcessParameter,
    ProcessParameter,
    id
);
impl_arena_record!(crate::process::toolpath::Toolpath, ProcessToolpath, id);
impl_arena_record!(crate::process::state::ProcessState, ProcessState, id);

/// Schema behavior required for every entity admitted to a model arena.
pub trait EntitySchema: Serialize + rewrite::typed::RewriteIdentities {
    /// Entity's canonical arena kind.
    const KIND: EntityKind;

    /// Globally unique entity identity.
    fn identity(&self) -> &str;

    /// Visit borrowed typed references through fields under the caller's context.
    fn visit_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        rewrite::typed::RewriteIdentities::visit_identity_references(self, ctx, &mut |target| {
            let identity = self.identity();

            if identity.len() == target.len() && identity == target {
                return Ok(());
            }
            visitor(target)
        })
    }
}

macro_rules! impl_entity_schema {
    ($type:ty, $kind:ident, $identity:ident $(.$inner:tt)?; $($field:ident),+ $(,)?) => {
        impl EntitySchema for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn identity(&self) -> &str {
                let Self { $($field: _),+ } = self;
                self.$identity $(.$inner)?.as_str()
            }
        }
    };
}

impl_entity_schema!(crate::topology::Body, Body, id; id, kind, transform, name, color, visible, unstated, native_ref);
impl_entity_schema!(crate::topology::Shell, Shell, id; id, region, members, unstated, native_ref);
impl_entity_schema!(crate::topology::Face, Face, id; id, shell, surface, sense, loops, name, color, tolerance, native_ref);
impl_entity_schema!(crate::topology::Loop, Loop, id; id, face, boundary, native_ref);
impl_entity_schema!(crate::topology::Coedge, Coedge, id; id, owner_loop, edge, radial_next, sense, pcurves, use_curve, native_ref);
impl_entity_schema!(crate::topology::Edge, Edge, id; id, carrier, ends, tolerance, native_ref);
impl_entity_schema!(crate::topology::Vertex, Vertex, id; id, point, tolerance, native_ref);
impl EntitySchema for crate::topology::Point {
    const KIND: EntityKind = EntityKind::Point;
    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::geometry::Surface, Surface, id; id, geometry, source_object, native_ref);
impl_entity_schema!(crate::geometry::Curve, Curve, id; id, geometry, parameter_range, source_object, native_ref);
impl_entity_schema!(crate::subd::SubdSurface, SubdSurface, id; id, scheme, cage, source_object, native_ref);
impl_entity_schema!(crate::geometry::pcurve::Pcurve, Pcurve, id; id, geometry, metadata, native_ref);
impl EntitySchema for crate::geometry::ProceduralSurface {
    const KIND: EntityKind = EntityKind::ProceduralSurface;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}

impl EntitySchema for crate::geometry::ProceduralCurve {
    const KIND: EntityKind = EntityKind::ProceduralCurve;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::assets::Asset, Asset, id; id, name, media_type, content, native_ref);
impl_entity_schema!(crate::features::Feature, Feature, id; id, product_definition, ordinal, name, suppressed, dependencies, source_properties, source_tag, source_text, source_content, evaluation, native_ref);
impl_entity_schema!(
    crate::features::evaluation::FeatureInputTopology,
    FeatureInputTopology,
    id;
    id, input_of, bodies, faces, edges, vertices, edge_geometry, face_owners, face_geometry, body_writers, native_ref
);
impl EntitySchema for crate::features::evaluation::FeatureResultTopology {
    const KIND: EntityKind = EntityKind::FeatureResultTopology;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(
    crate::features::parameters::DesignConfiguration,
    Configuration,
    id;
    id, ordinal, active, source_index, name, material, properties, parameter_overrides,
    bodies, parameter_values, feature_states, native_ref
);
impl_entity_schema!(crate::features::parameters::DesignParameter, Parameter, id; id, product_definition, owner, ordinal, name, expression, display, value, dependencies, properties, pmi, native_ref);
impl_entity_schema!(crate::sketches::Sketch, Sketch, id; id, name, configuration, visible, placement, profiles, native_ref);
impl EntitySchema for crate::sketches::SketchEntity {
    const KIND: EntityKind = EntityKind::SketchEntity;

    fn identity(&self) -> &str {
        self.id().as_str()
    }
}
impl_entity_schema!(crate::sketches::SketchConstraint, SketchConstraint, id; id, sketch, definition, name, driving, active, virtual_space, visible, orientation, label_distance, label_position, metadata, native_ref);
impl_entity_schema!(crate::sketches::SpatialSketch, SpatialSketch, id; id, name, configuration, visible, profiles, native_ref);
impl EntitySchema for crate::sketches::SpatialSketchEntity {
    const KIND: EntityKind = EntityKind::SpatialSketchEntity;

    fn identity(&self) -> &str {
        self.id().as_str()
    }
}
impl_entity_schema!(
    crate::sketches::SpatialSketchConstraint,
    SpatialSketchConstraint,
    id;
    id, sketch, definition, native_ref
);
impl EntitySchema for crate::spreadsheets::Spreadsheet {
    const KIND: EntityKind = EntityKind::Spreadsheet;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::products::ProductDefinition, ProductDefinition, id; id, kind, source_name, label, description, part_number, bom_properties, generated_from, storage, bodies, occurrences, instance_bodies, representations, sketches, units, general_tolerance, active, native_ref);
impl_entity_schema!(crate::products::Occurrence, Occurrence, id; id, prototype, transform, suppressed, visible, representation, overrides, name, native_ref);
impl EntitySchema for crate::products::AssemblyJoint {
    const KIND: EntityKind = EntityKind::AssemblyJoint;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::drawings::Drawing, Drawing, id; id, object, kind, runtime_type, order, visible, relationships, template, views, base_view, position, scale, direction, rotation, sheet, curves, template_geometry, x_direction, scale_from, projection, perspective, edge_classes, hidden_line, parameters, assets, native_ref);
impl_entity_schema!(
    crate::semantic_annotations::SemanticAnnotation,
    SemanticAnnotation,
    id;
    id, object, kind, runtime_type, order, text, references, value, format, number_format, position,
    reference_anchors, text_style, dimension_kind, text_override, upper_tolerance, lower_tolerance,
    measured_on, basic, reflex, negated, line_directions, parameters, assets, native_ref
);
impl EntitySchema for crate::presentation::PresentationDocument {
    const KIND: EntityKind = EntityKind::PresentationDocument;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(
    crate::presentation::ViewPresentation,
    ViewPresentation,
    id;
    id, object, order, expanded, visible, display_mode, selection_style, line_width,
    point_size, properties, native_ref
);
impl EntitySchema for crate::tessellation::Tessellation {
    const KIND: EntityKind = EntityKind::Tessellation;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::appearance::Appearance, Appearance, id; id, name, asset_guid, library_id, visual_guid, physical_token, schema, category, base_color, properties, textures, native_ref);
impl_entity_schema!(crate::appearance::AppearanceBinding, AppearanceBinding, id; id, target, appearance, precedence, source_entity_id, object_type, visible, channels, native_ref);
impl_entity_schema!(crate::attributes::SourceAttribute, SourceAttribute, id; id, target, name, values, native_ref);
impl_entity_schema!(crate::pmi::PmiAnnotation, PmiAnnotation, id; id, name, visible, targets, definition, native_ref, native_options, source_settings);
impl_entity_schema!(
    crate::presentation::PresentationLayer,
    PresentationLayer,
    id;
    id, name, parent, description, visible, items, native_ref
);
impl_entity_schema!(crate::physics::PhysicalMaterial, PhysicalMaterial, id; id, name, density, static_friction, dynamic_friction, restitution, native_ref);
impl_entity_schema!(crate::physics::MassProperties, MassProperties, id; id, target, mass, density, center_of_mass, inertia, material, native_ref);
impl_entity_schema!(crate::physics::RigidBody, RigidBody, id; id, target, kinematic, enabled, linear_velocity, angular_velocity, starts_asleep, native_ref);
impl_entity_schema!(crate::physics::Articulation, Articulation, id; id, root, native_ref);
impl_entity_schema!(crate::physics::PhysicsScene, PhysicsScene, id; id, name, gravity_direction, gravity_magnitude, native_ref);
impl_entity_schema!(crate::physics::solver::SolverSettings, SolverSettings, id; id, solver, schema, target, values, native_ref);
impl_entity_schema!(crate::declared_measures::DeclaredMeasure, DeclaredMeasure, id; id, target, quantity, label, basis, precision, native_ref);
impl_entity_schema!(crate::point_cloud::PointCloud, PointCloud, id; id, definition, name, color, visible, positions, normals, colors, channels, native_ref);
impl_entity_schema!(crate::history_body::HistoryBody, HistoryBody, id; id, produced_by, consumed_by, state_of, state, topology);
impl_entity_schema!(crate::physics::Collider, Collider, id; id, target, shape, enabled, material, native_ref);

#[cfg(test)]
mod tests;
