// SPDX-License-Identifier: Apache-2.0
//! Hand-built values of the design types, one or more per variant.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;

use crate::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, ConstructionKind,
    CurveValue, FrameAxis, FrameValue, InputReference, InputRole, PlaneValue, PostStep, Relation,
    ScalarDimension, SourceMode, Subject,
};
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::expr::{
    BendVariable, BinaryOp, Constant, Expression, ExpressionDialect, ExpressionToken, Function,
    Measure, MeasureKind, MeasureTarget, Neutral, Node, UnaryOp,
};
use crate::design::ids::ContainerId;
use crate::design::names::{
    Base64Bytes, GenerationRole, Generator, HoleFaceRole, InstanceIndex, NameEntry, NameKind,
    NameTable, NativeName, NativeNameValue, TopoName,
};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::refs::{
    ConfigurationRef, EntityRef, Evaluated, Member, Position, RecordRef, StateRef,
};
use crate::design::selection::{
    Anchor, BoundaryUse, Continuity, DatumPart, DefinitionRef, Fragment, MemberOrder, Members,
    NativeOwner, OccurrencePath, PathStep, PickContext, PickDomain, PickTarget, Picked, Picks,
    PrincipalEntity, Propagation, RuleTerm, Selection, SelectionRule, SketchRegion, StepRef,
    Witness,
};
use crate::design::units::{Unit, UnitFactor, UnitSymbol};
use crate::document::Topology;
use crate::examples::unit_cube;
use crate::features::geometry::FinitePoint3;
use crate::features::selections::NativeSelections;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::ProceduralCurve;
use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, OccurrenceId, PcurveId, PointId,
    ProceduralCurveId, ProceduralSurfaceId, ProductDefinitionId, RegionId, ShellId, SubdId,
    SurfaceId, VertexId,
};
use crate::math::Point3;
use crate::scalar::{Angle, FiniteReal, Length, NonNegativeReal, PositiveLength};
use crate::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};
use crate::transform::Transform;
use crate::units::UnitVector3;

/// An identity of kind `kind` with key `key`.
pub(super) fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

pub(super) fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

pub(super) fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

pub(super) fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

pub(super) fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

pub(super) fn positive_length(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

pub(super) fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

pub(super) fn feature(key: u32) -> FeatureId {
    id("feature", key)
}

pub(super) fn container(key: u32) -> ContainerId {
    id("container", key)
}

pub(super) fn parameter_ref(key: u32) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id::<ParameterId>("parameter", key)),
        through: None,
    }
}

pub(super) fn native_name() -> NativeName {
    NativeName {
        codec: crate::codec_id!("sldprt"),
        value: NativeNameValue::Text(text("Face1")),
    }
}

pub(super) fn native_name_values() -> Vec<NativeNameValue> {
    vec![
        NativeNameValue::Text(text("Face1")),
        NativeNameValue::Integers(vec![3, -1, 7]),
        NativeNameValue::Bytes(Base64Bytes(vec![0, 1, 254, 255])),
    ]
}

pub(super) fn entity_refs() -> Vec<EntityRef> {
    vec![
        EntityRef::Body(id::<BodyId>("body", 1)),
        EntityRef::Face(id::<FaceId>("face", 1)),
        EntityRef::Edge(id::<EdgeId>("edge", 1)),
        EntityRef::Vertex(id::<VertexId>("vertex", 1)),
        EntityRef::Surface(id::<SurfaceId>("surface", 1)),
        EntityRef::Curve(id::<CurveId>("curve", 1)),
        EntityRef::Subd(id::<SubdId>("subd", 1)),
    ]
}

pub(super) fn record_refs() -> Vec<RecordRef> {
    vec![
        RecordRef::Body(id::<BodyId>("body", 1)),
        RecordRef::Region(id::<RegionId>("region", 1)),
        RecordRef::Shell(id::<ShellId>("shell", 1)),
        RecordRef::Face(id::<FaceId>("face", 1)),
        RecordRef::Loop(id::<LoopId>("loop", 1)),
        RecordRef::Coedge(id::<CoedgeId>("coedge", 1)),
        RecordRef::Edge(id::<EdgeId>("edge", 1)),
        RecordRef::Vertex(id::<VertexId>("vertex", 1)),
        RecordRef::Point(id::<PointId>("point", 1)),
        RecordRef::Surface(id::<SurfaceId>("surface", 1)),
        RecordRef::Curve(id::<CurveId>("curve", 1)),
        RecordRef::Subd(id::<SubdId>("subd", 1)),
        RecordRef::Pcurve(id::<PcurveId>("pcurve", 1)),
        RecordRef::ProceduralSurface(id::<ProceduralSurfaceId>("procedural-surface", 1)),
        RecordRef::ProceduralCurve(id::<ProceduralCurveId>("procedural-curve", 1)),
    ]
}

pub(super) fn configuration_refs() -> Vec<ConfigurationRef> {
    vec![
        ConfigurationRef::Local(id::<ConfigurationId>("configuration", 1)),
        ConfigurationRef::Named(text("Default")),
    ]
}

pub(super) fn members() -> Vec<Member> {
    vec![Member::Feature(feature(1)), Member::Container(container(1))]
}

pub(super) fn positions() -> Vec<Position> {
    vec![
        Position::Start(container(1)),
        Position::After(Member::Feature(feature(2))),
    ]
}

pub(super) fn evaluated() -> Vec<Evaluated> {
    vec![
        Evaluated::State(Position::Start(container(1))),
        Evaluated::Tool(feature(3)),
    ]
}

pub(super) fn state_refs() -> Vec<StateRef> {
    vec![
        StateRef {
            configuration: None,
            of: Evaluated::Tool(feature(1)),
        },
        StateRef {
            configuration: Some(id("configuration", 2)),
            of: Evaluated::State(Position::After(Member::Container(container(2)))),
        },
    ]
}

pub(super) fn generated(key: u32) -> TopoName {
    TopoName::Generated {
        kind: NameKind::Face,
        feature: feature(key),
        role: None,
        generator: None,
        index: None,
    }
}

pub(super) fn topo_names() -> Vec<TopoName> {
    let leaf = generated(1);
    let other = generated(2);
    vec![
        TopoName::Generated {
            kind: NameKind::Edge,
            feature: feature(1),
            role: Some(GenerationRole::Hole(HoleFaceRole::BoreWall)),
            generator: Some(Generator::SketchEntity(id::<SketchEntityId>(
                "sketch-entity",
                1,
            ))),
            index: Some(2),
        },
        TopoName::Instance {
            feature: feature(4),
            instance: InstanceIndex(vec![1, 0]),
            of: Box::new(leaf.clone()),
        },
        TopoName::Blend {
            feature: feature(5),
            between: vec![other.clone(), leaf.clone()],
            index: Some(0),
        },
        TopoName::Bounded {
            faces: BTreeSet::from([leaf.clone(), other.clone()]),
            ends: BTreeSet::from([generated(3)]),
            index: None,
        },
        TopoName::Meeting {
            faces: BTreeSet::from([leaf.clone(), other.clone()]),
            index: Some(1),
        },
        TopoName::Modified {
            feature: feature(6),
            from: Box::new(leaf.clone()),
        },
        TopoName::Split {
            feature: Some(feature(7)),
            from: Box::new(leaf.clone()),
            piece: Some(1),
        },
        TopoName::Merged {
            feature: None,
            from: BTreeSet::from([leaf, other]),
        },
        TopoName::Root {
            container: container(1),
        },
    ]
}

pub(super) fn name_kinds() -> Vec<NameKind> {
    vec![
        NameKind::Body,
        NameKind::Surface,
        NameKind::Face,
        NameKind::Curve,
        NameKind::Edge,
        NameKind::Vertex,
    ]
}

pub(super) fn generators() -> Vec<Generator> {
    vec![
        Generator::SketchEntity(id("sketch-entity", 1)),
        Generator::SpatialSketchEntity(id::<SpatialSketchEntityId>("spatial-sketch-entity", 1)),
        Generator::Entity(Box::new(generated(1))),
    ]
}

pub(super) fn generation_roles() -> Vec<GenerationRole> {
    vec![
        GenerationRole::Start,
        GenerationRole::End,
        GenerationRole::Side,
        GenerationRole::Seam,
        GenerationRole::Body,
        GenerationRole::Hole(HoleFaceRole::HeadFloor),
        GenerationRole::Other(crate::codec_name!("sldprt:lip")),
    ]
}

pub(super) fn hole_face_roles() -> Vec<HoleFaceRole> {
    vec![
        HoleFaceRole::BoreWall,
        HoleFaceRole::Bottom,
        HoleFaceRole::HeadWall,
        HoleFaceRole::HeadFloor,
        HoleFaceRole::HeadCone,
    ]
}

pub(super) fn name_entries() -> Vec<NameEntry> {
    vec![
        NameEntry {
            name: Some(generated(1)),
            native: Some(native_name()),
        },
        NameEntry {
            name: None,
            native: None,
        },
    ]
}

pub(super) fn name_tables() -> Vec<NameTable> {
    let entries = entity_refs()
        .into_iter()
        .zip(name_entries().into_iter().cycle())
        .collect::<BTreeMap<_, _>>();
    vec![NameTable::default(), NameTable::from(entries)]
}

pub(super) fn picked(key: u32) -> Picked {
    Picked {
        target: PickTarget::Entity(generated(key)),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

pub(super) fn frame_value() -> FrameValue {
    FrameValue {
        origin: point(1.0, 2.0, 3.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }
}

/// A body that a witness backs up: the unit cube's geometry, with a
/// procedural carrier attached to its first curve.
pub(super) fn witness_body() -> Topology {
    let mut body = unit_cube().unwrap().geometry;
    let owner = body.curves[0].id.clone();
    body.add_procedural_curve(
        &owner,
        ProceduralCurve::new(
            ProceduralCurveId::mint("test:design:construction#witness").unwrap(),
            ProceduralCurveDefinition::Exact { cache: None },
        ),
    )
    .unwrap();
    body
}

pub(super) fn witnesses() -> Vec<Witness> {
    vec![
        Witness {
            point: Some(point(0.5, 0.0, -1.25)),
            direction: Some(UnitVector3::Z_AXIS),
            endpoints: Some([point(0.0, 0.0, 0.0), point(10.0, 0.0, 0.0)]),
            frame: Some(frame_value()),
            curves: Vec::new(),
            surface: None,
            body: Some(Box::new(witness_body())),
        },
        Witness {
            point: None,
            direction: None,
            endpoints: None,
            frame: None,
            curves: Vec::new(),
            surface: None,
            body: None,
        },
    ]
}

pub(super) fn pick_domains() -> Vec<PickDomain> {
    vec![
        PickDomain::EdgeInterval {
            start: real(0.25),
            end: real(0.75),
        },
        PickDomain::Limits {
            start: Some(Box::new(picked(8))),
            end: None,
        },
        PickDomain::Fragment(Fragment { index: 1, count: 3 }),
        PickDomain::Containing(point(1.0, 1.0, 0.0)),
    ]
}

pub(super) fn occurrence_path() -> OccurrencePath {
    OccurrencePath(vec![
        PathStep {
            occurrence: StepRef::Occurrence(id::<OccurrenceId>("occurrence", 1)),
            configuration: Some(ConfigurationRef::Named(text("Large"))),
        },
        PathStep {
            occurrence: StepRef::Native(native_name()),
            configuration: None,
        },
    ])
}

pub(super) fn definition_ref() -> DefinitionRef {
    DefinitionRef {
        definition: id::<ProductDefinitionId>("product-definition", 1),
        configuration: Some(ConfigurationRef::Local(id("configuration", 1))),
    }
}

pub(super) fn pick_contexts() -> Vec<PickContext> {
    vec![
        PickContext::Occurrence(occurrence_path()),
        PickContext::Definition(definition_ref()),
        PickContext::InContext {
            assembly: definition_ref(),
            own: Some(occurrence_path()),
            target: None,
        },
    ]
}

pub(super) fn principal_entities() -> Vec<PrincipalEntity> {
    vec![
        PrincipalEntity::XyPlane,
        PrincipalEntity::XzPlane,
        PrincipalEntity::YzPlane,
        PrincipalEntity::XAxis,
        PrincipalEntity::YAxis,
        PrincipalEntity::ZAxis,
        PrincipalEntity::Origin,
        PrincipalEntity::Frame,
    ]
}

pub(super) fn sketch_region() -> SketchRegion {
    SketchRegion {
        outer: vec![
            BoundaryUse {
                entity: id("sketch-entity", 1),
                fragment: None,
            },
            BoundaryUse {
                entity: id("sketch-entity", 2),
                fragment: Some(Fragment { index: 0, count: 2 }),
            },
        ],
        holes: vec![vec![BoundaryUse {
            entity: id("sketch-entity", 3),
            fragment: None,
        }]],
    }
}

pub(super) fn pick_targets() -> Vec<PickTarget> {
    vec![
        PickTarget::Entity(generated(1)),
        PickTarget::Native {
            within: Some(NativeOwner::Feature(feature(1))),
        },
        PickTarget::Native { within: None },
        PickTarget::Sketch(id::<SketchId>("sketch", 1)),
        PickTarget::SketchEntity(id("sketch-entity", 1)),
        PickTarget::SpatialSketchEntity(id("spatial-sketch-entity", 1)),
        PickTarget::SketchRegion {
            sketch: id("sketch", 1),
            region: sketch_region(),
        },
        PickTarget::SketchFrame {
            sketch: id("sketch", 1),
            entity: PrincipalEntity::XAxis,
        },
        PickTarget::Datum {
            feature: feature(2),
            part: Some(DatumPart::Point(1)),
        },
        PickTarget::Principal {
            entity: PrincipalEntity::Origin,
            container: Some(container(1)),
        },
        PickTarget::Feature(feature(3)),
        PickTarget::FeatureResult(feature(3)),
        PickTarget::TemplateInput {
            instance: feature(4),
            input: 0,
        },
        PickTarget::Component,
    ]
}

pub(super) fn native_owners() -> Vec<NativeOwner> {
    vec![
        NativeOwner::Sketch(id("sketch", 1)),
        NativeOwner::SpatialSketch(id::<SpatialSketchId>("spatial-sketch", 1)),
        NativeOwner::Feature(feature(1)),
    ]
}

pub(super) fn picks_full() -> Vec<Picked> {
    vec![
        picked(1),
        Picked {
            target: PickTarget::Datum {
                feature: feature(2),
                part: Some(DatumPart::Principal(PrincipalEntity::XyPlane)),
            },
            context: Some(PickContext::Occurrence(occurrence_path())),
            native: Some(native_name()),
            domain: Some(PickDomain::EdgeInterval {
                start: real(0.0),
                end: real(1.0),
            }),
            witness: witnesses().into_iter().next(),
            resolved: Some(vec![EntityRef::Face(id("face", 2))]),
        },
    ]
}

pub(super) fn members_of<O: MemberOrder>(keys: &[u32]) -> Members<Picked, O> {
    Members::new(keys.iter().map(|key| picked(*key)).collect()).unwrap()
}

pub(super) fn rule_terms<O: MemberOrder>() -> Vec<RuleTerm<O>> {
    vec![
        RuleTerm {
            rule: SelectionRule::Explicit,
            seeds: members_of(&[1]),
            propagation: None,
        },
        RuleTerm {
            rule: SelectionRule::Chain {
                end: Some(Box::new(picked(9))),
            },
            seeds: members_of(&[2, 1]),
            propagation: Some(Propagation::Connected {
                continuity: Some(Continuity::G1),
                angle: Some(Dimension::Literal(angle(0.1))),
                gap: None,
            }),
        },
    ]
}

pub(super) fn selection_rules() -> Vec<SelectionRule> {
    vec![
        SelectionRule::Explicit,
        SelectionRule::All {
            kind: NameKind::Face,
        },
        SelectionRule::Of {
            kind: NameKind::Edge,
        },
        SelectionRule::LoopEdges,
        SelectionRule::Boundary,
        SelectionRule::Chain { end: None },
        SelectionRule::Region {
            boundary: members_of(&[1, 2]),
        },
        SelectionRule::Intersection {
            with: members_of(&[3]),
        },
        SelectionRule::Affected,
        SelectionRule::Native(crate::codec_name!("nx:tangent_curves")),
    ]
}

pub(super) fn propagations() -> Vec<Propagation> {
    vec![
        Propagation::Off,
        Propagation::Minimal,
        Propagation::Connected {
            continuity: None,
            angle: None,
            gap: Some(Dimension::Parameter(id("parameter", 1))),
        },
        Propagation::Native(crate::codec_name!("catia:point_continuity")),
    ]
}

pub(super) fn continuities() -> Vec<Continuity> {
    vec![
        Continuity::G0,
        Continuity::G1,
        Continuity::G2,
        Continuity::G3,
        Continuity::Native(text("nx:g4")),
    ]
}

pub(super) fn anchors() -> Vec<Anchor> {
    vec![
        Anchor::Input,
        Anchor::Pinned(Position::After(Member::Feature(feature(1)))),
        Anchor::Current,
        Anchor::Native(native_name()),
    ]
}

pub(super) fn picks<O: MemberOrder>() -> Vec<Picks<O>> {
    vec![
        Picks::Unread,
        Picks::Named(members_of(&[2, 1])),
        Picks::Rule {
            terms: rule_terms(),
            excluded: Vec::new(),
            resolved: Some(members_of(&[1])),
        },
    ]
}

pub(super) fn selections<O: MemberOrder>() -> Vec<Selection<O>> {
    vec![
        Selection {
            at: Anchor::Input,
            picks: Picks::Named(members_of(&[1])),
            native: None,
        },
        Selection {
            at: Anchor::Current,
            picks: Picks::Unread,
            native: Some(NativeSelections::try_from(vec!["Edge<1>".to_owned()]).unwrap()),
        },
    ]
}

pub(super) fn source_mode() -> SourceMode {
    let options = NativeOptions::from([
        (
            crate::codec_name!("sldprt:flip"),
            NativeOption::Bool(Dimension::Literal(true)),
        ),
        (
            crate::codec_name!("sldprt:offset"),
            NativeOption::Length(Dimension::Literal(length(2.5))),
        ),
    ]);
    SourceMode {
        codec: crate::codec_id!("sldprt"),
        method: NativeNameValue::Integers(vec![4]),
        native_options: options,
        source_settings: NativeOptions::new(),
    }
}

pub(super) fn construction_inputs() -> Vec<ConstructionInput> {
    vec![
        ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Offset,
            }),
            reference: Some(InputReference::Picks(selections().remove(0))),
            value: Some(ScalarDimension::Length(Dimension::Literal(length(5.0)))),
            reversed: Some(false),
        },
        ConstructionInput {
            role: None,
            reference: None,
            value: None,
            reversed: None,
        },
    ]
}

pub(super) fn constructions<K: ConstructionKind>(
    resolved: K::Value,
    configuration: K::Value,
) -> Vec<Construction<K>> {
    vec![
        Construction {
            inputs: construction_inputs(),
            after: vec![PostStep::Flip(Dimension::Literal(true))],
            solution: Some(0),
            mode: Some(source_mode()),
            resolved: Some(resolved),
            resolved_in: BTreeMap::from([(id("configuration", 1), configuration)]),
        },
        Construction {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        },
    ]
}

pub(super) fn axis_value() -> AxisValue {
    AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }
}

pub(super) fn plane_value() -> PlaneValue {
    PlaneValue {
        origin: point(0.0, 0.0, 5.0),
        normal: UnitVector3::Z_AXIS,
        x_axis: UnitVector3::X_AXIS,
    }
}

pub(super) fn curve_values() -> Vec<CurveValue> {
    vec![
        CurveValue::Segment {
            start: point(0.0, 0.0, 0.0),
            end: point(1.0, 0.0, 0.0),
        },
        CurveValue::Arc {
            center: point(0.0, 0.0, 0.0),
            normal: UnitVector3::Z_AXIS,
            radius: positive_length(2.0),
            start: point(2.0, 0.0, 0.0),
            end: point(0.0, 2.0, 0.0),
        },
        CurveValue::Circle {
            center: point(0.0, 0.0, 0.0),
            normal: UnitVector3::Z_AXIS,
            radius: positive_length(3.0),
        },
    ]
}

pub(super) fn point_constructions() -> Vec<Construction<kinds::Point>> {
    constructions(point(1.0, 2.0, 3.0), point(1.0, 2.0, 4.0))
}

pub(super) fn plane_constructions() -> Vec<Construction<kinds::Plane>> {
    constructions(plane_value(), plane_value())
}

pub(super) fn direction_constructions() -> Vec<Construction<kinds::Direction>> {
    constructions(UnitVector3::X_AXIS, UnitVector3::Y_AXIS)
}

pub(super) fn any_constructions() -> Vec<AnyConstruction> {
    let [segment, arc, _] = <[CurveValue; 3]>::try_from(curve_values()).unwrap();
    vec![
        AnyConstruction::Point(point_constructions().remove(0)),
        AnyConstruction::Direction(direction_constructions().remove(0)),
        AnyConstruction::Axis(constructions(axis_value(), axis_value()).remove(0)),
        AnyConstruction::Plane(plane_constructions().remove(0)),
        AnyConstruction::Frame(constructions(frame_value(), frame_value()).remove(0)),
        AnyConstruction::Curve(constructions(segment, arc).remove(0)),
    ]
}

pub(super) fn input_references() -> Vec<InputReference> {
    vec![
        InputReference::Picks(selections().remove(0)),
        InputReference::Built(Box::new(any_constructions().remove(3))),
    ]
}

pub(super) fn subjects() -> Vec<Subject> {
    vec![
        Subject::Whole,
        Subject::Origin,
        Subject::XAxis,
        Subject::YAxis,
        Subject::ZAxis,
    ]
}

pub(super) fn relations() -> Vec<Relation> {
    vec![
        Relation::Coincident,
        Relation::Parallel,
        Relation::Perpendicular,
        Relation::Tangent,
        Relation::Offset,
        Relation::Angle,
        Relation::Midway,
        Relation::AtParameter,
        Relation::Coordinates,
        Relation::Center,
        Relation::Centroid,
        Relation::AxisOf,
        Relation::Projection,
        Relation::PrincipalNormal,
        Relation::Binormal,
        Relation::Radius,
        Relation::Limit,
        Relation::Fold,
        Relation::Native(crate::codec_name!("creo:on_surface")),
    ]
}

pub(super) fn scalar_dimensions() -> Vec<ScalarDimension> {
    let r = |value| Dimension::Literal(real(value));
    let l = |value| Dimension::Literal(length(value));
    vec![
        ScalarDimension::Length(l(1.0)),
        ScalarDimension::Angle(Dimension::Negated(id("parameter", 1))),
        ScalarDimension::Ratio(r(0.5)),
        ScalarDimension::Uv {
            u: r(0.1),
            v: r(0.9),
        },
        ScalarDimension::Position {
            x: l(1.0),
            y: l(2.0),
            z: l(3.0),
        },
        ScalarDimension::Components {
            x: r(0.0),
            y: r(0.0),
            z: r(1.0),
        },
        ScalarDimension::PlaneEquation {
            a: r(0.0),
            b: r(0.0),
            c: r(1.0),
            d: l(-4.0),
        },
        ScalarDimension::ParameterRatio(r(0.25)),
    ]
}

pub(super) fn post_steps() -> Vec<PostStep> {
    vec![
        PostStep::Flip(Dimension::Literal(false)),
        PostStep::Translate {
            axis: FrameAxis::X,
            distance: Dimension::Literal(length(3.0)),
        },
        PostStep::Rotate {
            axis: FrameAxis::Z,
            angle: Dimension::Literal(angle(1.5)),
        },
        PostStep::AxisRotate {
            axis: [
                Dimension::Literal(real(0.0)),
                Dimension::Literal(real(0.0)),
                Dimension::Literal(real(1.0)),
            ],
            angle: Dimension::Literal(angle(0.5)),
        },
        PostStep::Fixed(Transform::identity()),
    ]
}

pub(super) fn mm() -> Unit {
    Unit(vec![UnitFactor {
        symbol: UnitSymbol::Mm,
        power: NonZeroI8::new(1).unwrap(),
    }])
}

pub(super) fn number(value: f64, unit: Unit) -> Node<ParameterRef> {
    Node::Number {
        value: NonNegativeReal::new(value).unwrap(),
        unit,
    }
}

pub(super) fn measure() -> Measure {
    Measure {
        kind: MeasureKind::Coordinate(FrameAxis::Y),
        of: MeasureTarget::Selection(selections().remove(0)),
        cached: Some(real(12.5)),
    }
}

/// One node of each variant, each with leaves for operands.
pub(super) fn nodes() -> Vec<Node<ParameterRef>> {
    let leaf = || Box::new(Node::Reference(parameter_ref(1)));
    vec![
        number(10.0, mm()),
        Node::Constant(Constant::Pi),
        Node::Constant(Constant::E),
        Node::Boolean(true),
        Node::Text("say \"hi\" \\ there".to_owned()),
        Node::Reference(parameter_ref(1)),
        Node::Measure(Box::new(measure())),
        Node::LawParameter,
        Node::Bend(BendVariable::InsideRadius),
        Node::Unary {
            op: UnaryOp::Negate,
            operand: leaf(),
        },
        Node::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Node::Boolean(false)),
        },
        Node::Binary {
            op: BinaryOp::Power,
            left: leaf(),
            right: Box::new(number(2.0, Unit::default())),
        },
        Node::Call {
            function: Function::Atan2,
            args: vec![*leaf(), Node::Reference(parameter_ref(2))],
        },
        Node::branch(
            Node::Boolean(true),
            number(1.0, Unit::default()),
            number(0.0, Unit::default()),
        ),
    ]
}

pub(super) fn expressions() -> Vec<Expression<ParameterRef>> {
    let mut out = nodes()
        .into_iter()
        .map(|node| Expression::Neutral(Neutral(node)))
        .collect::<Vec<_>>();
    out.push(Expression::Source {
        dialect: ExpressionDialect::SolidWorks,
        tokens: vec![
            ExpressionToken::Reference(parameter_ref(1)),
            ExpressionToken::Text(" * 2".to_owned()),
        ],
    });
    out
}

pub(super) fn dialects() -> Vec<ExpressionDialect> {
    vec![
        ExpressionDialect::SolidWorks,
        ExpressionDialect::Fusion,
        ExpressionDialect::Inventor,
        ExpressionDialect::Catia,
        ExpressionDialect::Creo,
        ExpressionDialect::Nx,
        ExpressionDialect::FreeCad,
        ExpressionDialect::Spreadsheet,
        ExpressionDialect::Native(text("onshape:featurescript")),
    ]
}

pub(super) fn parameter_refs() -> Vec<ParameterRef> {
    vec![
        parameter_ref(1),
        ParameterRef {
            target: RefTarget::External {
                definition: id("product-definition", 2),
                name: text("d3"),
            },
            through: Some(occurrence_path()),
        },
    ]
}
