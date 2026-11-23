// SPDX-License-Identifier: Apache-2.0
//! Sketches: every type in both spaces, written, read back strictly, and
//! written again, and the stated wire forms.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::num::NonZeroU32;

use serde_json::json;

use super::shared::{angle_sides, angle_spacings, length_sides, length_spacings};
use super::{
    angle, bound, construction, id, length, lit, native_options, point2, point3, positive,
    positive_real, real, refuses, round_trip, selection, text, to_value, unread, Report,
};
use crate::design::construction::{kinds, Construction, FrameValue, ScalarDimension};
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::LinkState;
use crate::design::ids::SpatialSketchConstraintId;
use crate::design::names::{CodecId, InstanceIndex, NativeName, NativeNameValue};
use crate::design::ops::shared::Spacing;
use crate::design::ops::sketch::{
    AngleOperands, AngleSector, BlockInstance, BoundedCurve, Contact, DefiningRole, Derivation,
    DerivationMode, DistanceAxis, EntityDefinition, EntityForm, EntityRole, InterpolatedSpline,
    Operand, Planar, PlanarCurve, RadialMeasure, Sketch, SketchConstraint, SketchEntity,
    SketchOffset, SketchPattern, SketchPatternAxis, SketchPatternDirection, SketchPatternInstance,
    SketchPatternLayout, SketchPlane, SketchPolygon, SketchRelation, SketchSlot, SketchSolve,
    SketchSpace, SketchText, SlotKind, SolveState, Spatial, SpatialCurve, SpatialSketch,
    SplineHandle, TextAlign, TextContent, TextHorizontalBox, TextLayout, TextVerticalBox,
};
use crate::design::options::{NativeOptions, ProjectionDirection};
use crate::design::selection::PrincipalEntity;
use crate::document::{CadIr, Design};
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::LineCurve;
use crate::geometry::InlineCurve;
use crate::sketches::{
    SketchConstraintId, SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId,
};
use crate::test_support::nurbs;
use crate::topology::IncreasingParameterInterval;
use crate::units::{UnitVector2, UnitVector3};

/// The ids, a direction, and curves of one sketch space.
trait Fixture: SketchSpace + Clone + Debug + PartialEq {
    fn sketch(key: u32) -> Self::Sketch;
    fn entity(key: u32) -> Self::Entity;
    fn constraint(key: u32) -> Self::Constraint;
    fn direction() -> Self::Direction;
    fn curves() -> Vec<Self::Curve>;
}

impl Fixture for Planar {
    fn sketch(key: u32) -> SketchId {
        id("sketch", key)
    }

    fn entity(key: u32) -> SketchEntityId {
        id("sketch-entity", key)
    }

    fn constraint(key: u32) -> SketchConstraintId {
        id("sketch-constraint", key)
    }

    fn direction() -> UnitVector2 {
        unit2(0.6, 0.8)
    }

    fn curves() -> Vec<PlanarCurve> {
        planar_curves()
    }
}

impl Fixture for Spatial {
    fn sketch(key: u32) -> SpatialSketchId {
        id("spatial-sketch", key)
    }

    fn entity(key: u32) -> SpatialSketchEntityId {
        id("spatial-sketch-entity", key)
    }

    fn constraint(key: u32) -> SpatialSketchConstraintId {
        id("spatial-sketch-constraint", key)
    }

    fn direction() -> UnitVector3 {
        UnitVector3::Y_AXIS
    }

    fn curves() -> Vec<SpatialCurve> {
        spatial_curves()
    }
}

fn unit2(x: f64, y: f64) -> UnitVector2 {
    UnitVector2::new([x, y]).unwrap()
}

fn frame() -> Construction<kinds::Frame> {
    construction(Some(FrameValue {
        origin: point3(0.0, 0.0, 0.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }))
}

fn native_name() -> NativeName {
    NativeName {
        codec: CodecId::from(text("sldprt")),
        value: NativeNameValue::Integers(vec![12]),
    }
}

fn sketch_planes() -> Vec<SketchPlane> {
    vec![
        SketchPlane::Placed(Box::new(frame())),
        SketchPlane::Placed(Box::new(construction(None))),
        SketchPlane::Block,
    ]
}

fn solve_states() -> Vec<SolveState> {
    vec![
        SolveState::Full,
        SolveState::Under,
        SolveState::NotFull,
        SolveState::Over,
        SolveState::Inconsistent,
        SolveState::Native(text("nx:status_4")),
    ]
}

fn solves() -> Vec<SketchSolve> {
    vec![
        SketchSolve {
            state: Some(SolveState::Full),
            status: EvaluationStatus::Current,
        },
        SketchSolve {
            state: None,
            status: EvaluationStatus::Stale,
        },
    ]
}

fn sketches() -> Vec<Sketch> {
    sketch_planes()
        .into_iter()
        .zip(solves().into_iter().cycle())
        .enumerate()
        .map(|(key, (plane, solve))| {
            let named = key % 2 == 0;
            Sketch {
                id: id("sketch", u32::try_from(key).unwrap()),
                owner: id("feature", 1),
                name: named.then(|| "Sketch1".to_owned()),
                plane,
                solve,
                native_options: native_options(),
                source_settings: NativeOptions::new(),
                native_ref: named.then(|| "sldprt:history:sketch#3".to_owned()),
            }
        })
        .collect()
}

fn spatial_sketches() -> Vec<SpatialSketch> {
    vec![
        SpatialSketch {
            id: id("spatial-sketch", 1),
            owner: id("feature", 2),
            name: Some("3DSketch1".to_owned()),
            solve: solves().remove(0),
            native_options: NativeOptions::new(),
            source_settings: native_options(),
            native_ref: Some("sldprt:history:sketch#4".to_owned()),
        },
        SpatialSketch {
            id: id("spatial-sketch", 2),
            owner: id("feature", 3),
            name: None,
            solve: solves().remove(1),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        },
    ]
}

fn line_carrier() -> InlineCurve {
    InlineCurve::Line(LineCurve::new(FinitePoint3::ZERO, UnitVector3::X_AXIS))
}

fn spatial_curves() -> Vec<SpatialCurve> {
    vec![
        SpatialCurve::Point(point3(1.0, 2.0, 3.0)),
        SpatialCurve::Bounded(BoundedCurve {
            carrier: line_carrier(),
            range: IncreasingParameterInterval::new([0.0, 4.0]),
        }),
        SpatialCurve::Bounded(BoundedCurve {
            carrier: line_carrier(),
            range: None,
        }),
    ]
}

fn planar_curves() -> Vec<PlanarCurve> {
    let mut curves = vec![
        PlanarCurve::Point(point2(1.0, 2.0)),
        PlanarCurve::Line {
            start: point2(0.0, 0.0),
            end: point2(3.0, 4.0),
        },
        PlanarCurve::InfiniteLine {
            through: point2(0.0, 1.0),
            direction: unit2(1.0, 0.0),
        },
        PlanarCurve::Circle {
            center: point2(0.0, 0.0),
            radius: positive(5.0),
        },
        PlanarCurve::Arc {
            center: point2(1.0, 1.0),
            radius: positive(2.0),
            start: angle(0.0),
            end: angle(1.5),
        },
        PlanarCurve::Ellipse {
            center: point2(0.0, 0.0),
            major: unit2(0.6, 0.8),
            radii: [positive(4.0), positive(2.0)],
            range: Some([angle(0.0), angle(3.0)]),
        },
        PlanarCurve::Ellipse {
            center: point2(0.0, 0.0),
            major: unit2(0.0, 1.0),
            radii: [positive(4.0), positive(2.0)],
            range: None,
        },
        PlanarCurve::Parabola {
            vertex: point2(0.0, 0.0),
            axis: unit2(0.0, 1.0),
            focal: positive(1.5),
            range: Some([real(-2.0), real(2.0)]),
        },
        PlanarCurve::Hyperbola {
            center: point2(0.0, 0.0),
            major: unit2(1.0, 0.0),
            radii: [positive(3.0), positive(1.0)],
            range: None,
        },
        PlanarCurve::Nurbs(nurbs::pcurve()),
    ];
    curves.extend(spatial_curves().into_iter().map(PlanarCurve::Spatial));
    curves
}

fn defining_roles() -> Vec<DefiningRole> {
    vec![
        DefiningRole::Start,
        DefiningRole::End,
        DefiningRole::Center,
        DefiningRole::Apex,
        DefiningRole::Focus(1),
        DefiningRole::MajorAxis,
        DefiningRole::MinorAxis,
        DefiningRole::XAxis,
        DefiningRole::YAxis,
        DefiningRole::Fit(3),
        DefiningRole::Pole(2),
        DefiningRole::Knot(4),
        DefiningRole::Handle(0),
        DefiningRole::Anchor,
        DefiningRole::Insertion,
        DefiningRole::Native(text("freecad:internal_alignment")),
    ]
}

fn entity_roles<S: Fixture>() -> Vec<EntityRole<S>> {
    vec![
        EntityRole {
            of: S::entity(1),
            role: DefiningRole::Start,
        },
        EntityRole {
            of: S::entity(2),
            role: DefiningRole::Pole(1),
        },
    ]
}

fn spline_handles<S: Fixture>() -> Vec<SplineHandle<S>> {
    vec![
        SplineHandle {
            direction: Some(S::direction()),
            magnitude: Some(lit(real(1.5))),
            curvature: Some(real(0.25)),
        },
        SplineHandle {
            direction: None,
            magnitude: Some(bound(3)),
            curvature: None,
        },
    ]
}

fn interpolated_splines<S: Fixture>() -> Vec<InterpolatedSpline<S>> {
    vec![
        InterpolatedSpline {
            degree: NonZeroU32::new(3),
            closed: Some(false),
            handles: spline_handles()
                .into_iter()
                .enumerate()
                .map(|(fit, handle)| (u32::try_from(fit * 4).unwrap(), handle))
                .collect(),
        },
        InterpolatedSpline {
            degree: None,
            closed: None,
            handles: BTreeMap::new(),
        },
    ]
}

fn derivation_modes() -> Vec<DerivationMode> {
    vec![
        DerivationMode::Project,
        DerivationMode::Include,
        DerivationMode::Intersect,
        DerivationMode::Silhouette,
        DerivationMode::Offset {
            distance: lit(length(-1.5)),
        },
        DerivationMode::OntoFaces {
            faces: selection(6),
            direction: Some(Box::new(ProjectionDirection::SurfaceNormal)),
        },
        DerivationMode::OntoFaces {
            faces: unread(),
            direction: None,
        },
        DerivationMode::Native(text("f3d:project_body")),
    ]
}

fn derivations() -> Vec<Derivation> {
    vec![
        Derivation {
            mode: DerivationMode::Project,
            source: selection(7),
            piece: Some(2),
            link: Some(LinkState::Broken),
        },
        Derivation {
            mode: DerivationMode::Include,
            source: unread(),
            piece: None,
            link: None,
        },
    ]
}

fn text_aligns() -> Vec<TextAlign> {
    vec![
        TextAlign::Start,
        TextAlign::Center,
        TextAlign::End,
        TextAlign::Baseline,
        TextAlign::Native(text("catia:anchor_7")),
    ]
}

fn text_contents() -> Vec<TextContent> {
    vec![
        TextContent {
            content: text("CADMPEG"),
            font: Some(text("Arial")),
            height: Some(lit(length(5.0))),
            width_factor: Some(lit(real(1.2))),
            angle: Some(lit(angle(0.5))),
            horizontal: Some(TextAlign::Center),
            vertical: Some(TextAlign::Start),
            horizontal_box: Some(TextHorizontalBox::Pen),
            vertical_box: Some(TextVerticalBox::CapHeight),
            tracking: Some(lit(length(0.2))),
            oblique: Some(lit(angle(0.1))),
            filled: Some(lit(false)),
        },
        TextContent {
            content: text("A"),
            font: None,
            height: None,
            width_factor: None,
            angle: None,
            horizontal: None,
            vertical: None,
            horizontal_box: None,
            vertical_box: None,
            tracking: None,
            oblique: None,
            filled: None,
        },
    ]
}

fn text_layouts<S: Fixture>() -> Vec<TextLayout<S>> {
    vec![
        TextLayout::Frame(vec![S::entity(1), S::entity(2)]),
        TextLayout::Path {
            curve: S::entity(3),
            reversed: Some(true),
        },
        TextLayout::Path {
            curve: S::entity(4),
            reversed: None,
        },
    ]
}

fn sketch_texts<S: Fixture>() -> Vec<SketchText<S>> {
    let mut layouts = text_layouts::<S>()
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    layouts.push(None);
    layouts
        .into_iter()
        .zip(text_contents().into_iter().cycle())
        .map(|(layout, text)| SketchText { text, layout })
        .collect()
}

fn block_instances() -> Vec<BlockInstance> {
    vec![
        BlockInstance {
            block: id("sketch", 9),
            angle: Some(lit(angle(0.75))),
            scale: Some(lit(real(2.0))),
        },
        BlockInstance {
            block: id("sketch", 9),
            angle: None,
            scale: None,
        },
    ]
}

fn entity_forms<S: Fixture>() -> Vec<EntityForm<S>> {
    let mut forms = interpolated_splines::<S>()
        .into_iter()
        .map(EntityForm::Interpolated)
        .collect::<Vec<_>>();
    forms.push(EntityForm::Conic {
        rho: lit(real(0.5)),
    });
    forms.extend(
        derivations()
            .into_iter()
            .map(|derivation| EntityForm::Derived(Box::new(derivation))),
    );
    forms.extend(
        sketch_texts::<S>()
            .into_iter()
            .map(|text| EntityForm::Text(Box::new(text))),
    );
    forms.extend(block_instances().into_iter().map(EntityForm::BlockInstance));
    forms.push(EntityForm::Instanced {
        instance: S::entity(5),
        of: id("sketch-entity", 1),
    });
    forms.push(EntityForm::Frame);
    forms.push(EntityForm::Native(text("creo:section_entity")));
    forms
}

fn entity_definitions<S: Fixture>() -> Vec<EntityDefinition<S>> {
    let mut definitions = S::curves()
        .into_iter()
        .map(EntityDefinition::Free)
        .collect::<Vec<_>>();
    let solved = S::curves().into_iter().map(Some).chain([None]).cycle();
    definitions.extend(
        entity_forms::<S>()
            .into_iter()
            .zip(solved)
            .map(|(form, solved)| EntityDefinition::Defined {
                form: Box::new(form),
                solved,
            }),
    );
    definitions
}

fn sketch_entities<S: Fixture>() -> Vec<SketchEntity<S>> {
    let mut definitions = entity_definitions::<S>();
    let free = definitions.remove(0);
    let defined = definitions.pop().unwrap();
    vec![
        SketchEntity {
            id: S::entity(1),
            sketch: S::sketch(1),
            definition: free,
            roles: entity_roles(),
            construction: Some(true),
            centerline: Some(false),
            native: Some(native_name()),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
            native_ref: Some("sldprt:sketch:segment#12".to_owned()),
        },
        SketchEntity {
            id: S::entity(2),
            sketch: S::sketch(1),
            definition: defined,
            roles: Vec::new(),
            construction: None,
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: native_options(),
            native_ref: None,
        },
    ]
}

fn operands<S: Fixture>() -> Vec<Operand<S>> {
    vec![
        Operand::Entity(S::entity(1)),
        Operand::Frame(PrincipalEntity::Origin),
        Operand::Model(Box::new(selection(8))),
    ]
}

fn entity<S: Fixture>(key: u32) -> Operand<S> {
    Operand::Entity(S::entity(key))
}

fn contacts<S: Fixture>() -> Vec<Contact<S>> {
    vec![
        Contact::Via(S::entity(3)),
        Contact::Ends([S::entity(4), S::entity(5)]),
    ]
}

fn distance_axes<S: Fixture>() -> Vec<DistanceAxis<S>> {
    vec![
        DistanceAxis::Aligned,
        DistanceAxis::Horizontal,
        DistanceAxis::Vertical,
        DistanceAxis::Along(Operand::Frame(PrincipalEntity::XAxis)),
    ]
}

fn angle_operands<S: Fixture>() -> Vec<AngleOperands<S>> {
    vec![
        AngleOperands::Between([entity(1), Operand::Frame(PrincipalEntity::XAxis)]),
        AngleOperands::Sweep(S::entity(2)),
        AngleOperands::Points([S::entity(3), S::entity(4), S::entity(5)]),
    ]
}

fn angle_sectors() -> Vec<AngleSector> {
    vec![
        AngleSector::Minor,
        AngleSector::Major,
        AngleSector::Directed {
            second_reversed: true,
            clockwise: false,
        },
        AngleSector::Native(text("nx:sector_3")),
    ]
}

fn radial_measures() -> Vec<RadialMeasure> {
    vec![
        RadialMeasure::Radius,
        RadialMeasure::Diameter,
        RadialMeasure::MajorRadius,
        RadialMeasure::MinorRadius,
    ]
}

fn sketch_offsets<S: Fixture>() -> Vec<SketchOffset<S>> {
    vec![SketchOffset {
        pairs: vec![[S::entity(1), S::entity(2)], [S::entity(3), S::entity(4)]],
        distance: lit(length(-2.0)),
    }]
}

fn sketch_polygons<S: Fixture>() -> Vec<SketchPolygon<S>> {
    vec![
        SketchPolygon {
            center: S::entity(1),
            edges: vec![S::entity(2), S::entity(3), S::entity(4)],
            circle: Some(S::entity(5)),
            inscribed: Some(true),
        },
        SketchPolygon {
            center: S::entity(1),
            edges: Vec::new(),
            circle: None,
            inscribed: None,
        },
    ]
}

fn slot_kinds() -> Vec<SlotKind> {
    vec![
        SlotKind::Straight,
        SlotKind::CenterStraight,
        SlotKind::Arc,
        SlotKind::CenterArc,
        SlotKind::Native(text("sldprt:slot_9")),
    ]
}

fn sketch_slots<S: Fixture>() -> Vec<SketchSlot<S>> {
    slot_kinds()
        .into_iter()
        .map(|kind| SketchSlot {
            kind,
            centers: [S::entity(1), S::entity(2)],
            edges: vec![S::entity(3), S::entity(4)],
        })
        .collect()
}

fn pattern_directions<S: Fixture>() -> Vec<SketchPatternDirection<S>> {
    vec![
        SketchPatternDirection::Along {
            line: entity(6),
            reversed: Some(true),
        },
        SketchPatternDirection::Along {
            line: Operand::Frame(PrincipalEntity::YAxis),
            reversed: None,
        },
        SketchPatternDirection::Angle(lit(angle(0.5))),
    ]
}

fn pattern_axes<S: Fixture>() -> Vec<SketchPatternAxis<S>> {
    let directions = pattern_directions::<S>()
        .into_iter()
        .map(Some)
        .chain([None]);
    let sides = length_sides().into_iter().map(Some).chain([None]).cycle();
    directions
        .zip(length_spacings())
        .zip(sides)
        .map(|((direction, spacing), sides)| SketchPatternAxis {
            direction,
            spacing,
            sides,
        })
        .collect()
}

fn pattern_layouts<S: Fixture>() -> Vec<SketchPatternLayout<S>> {
    let mut layouts = vec![
        SketchPatternLayout::Linear {
            directions: pattern_axes(),
        },
        SketchPatternLayout::Linear {
            directions: Vec::new(),
        },
    ];
    let sides = angle_sides().into_iter().map(Some).chain([None]).cycle();
    layouts.extend(
        angle_spacings()
            .into_iter()
            .zip(sides)
            .map(|(spacing, sides)| SketchPatternLayout::Circular {
                center: Operand::Frame(PrincipalEntity::Origin),
                spacing,
                sides,
            }),
    );
    layouts.push(SketchPatternLayout::Native(text("f3d:pattern_kind_7")));
    layouts
}

fn pattern_instances<S: Fixture>() -> Vec<SketchPatternInstance<S>> {
    vec![
        SketchPatternInstance {
            index: InstanceIndex(vec![0, 1]),
            entities: vec![S::entity(7), S::entity(8)],
            suppressed: None,
        },
        SketchPatternInstance {
            index: InstanceIndex(vec![1, 1]),
            entities: Vec::new(),
            suppressed: Some(true),
        },
    ]
}

fn sketch_patterns<S: Fixture>() -> Vec<SketchPattern<S>> {
    pattern_layouts::<S>()
        .into_iter()
        .map(|layout| SketchPattern {
            seeds: vec![S::entity(1), S::entity(2)],
            layout,
            instances: pattern_instances(),
        })
        .collect()
}

fn relations<S: Fixture>() -> Vec<SketchRelation<S>> {
    let pair = |a, b| (entity::<S>(a), entity::<S>(b));
    let mut relations = vec![
        {
            let (a, b) = pair(1, 2);
            SketchRelation::Coincident { a, b }
        },
        {
            let (a, b) = pair(1, 2);
            SketchRelation::Concentric { a, b }
        },
        {
            let (a, b) = pair(1, 2);
            SketchRelation::Collinear { a, b }
        },
        {
            let (a, b) = pair(1, 2);
            SketchRelation::Parallel { a, b }
        },
        SketchRelation::Perpendicular {
            a: entity(1),
            b: Operand::Model(Box::new(selection(3))),
        },
        SketchRelation::Tangent {
            a: entity(1),
            b: entity(2),
            contact: None,
        },
        SketchRelation::Horizontal {
            a: entity(1),
            b: Some(entity(2)),
        },
        SketchRelation::Horizontal {
            a: entity(1),
            b: None,
        },
        SketchRelation::Vertical {
            a: entity(1),
            b: None,
        },
        SketchRelation::Midpoint {
            point: S::entity(3),
            of: entity(1),
        },
        SketchRelation::Symmetric {
            a: entity(1),
            b: entity(2),
            about: Operand::Frame(PrincipalEntity::YAxis),
        },
        SketchRelation::Equal {
            members: operands(),
        },
        SketchRelation::EqualDistance {
            first: [entity(1), entity(2)],
            second: [entity(3), entity(4)],
        },
        SketchRelation::Fixed {
            of: Operand::Model(Box::new(unread())),
        },
        SketchRelation::Distance {
            a: Operand::Frame(PrincipalEntity::Origin),
            b: entity(1),
            along: None,
            doubled: None,
            value: lit(length(10.0)),
        },
        SketchRelation::Angle {
            between: AngleOperands::Sweep(S::entity(2)),
            sector: None,
            value: bound(4),
        },
        SketchRelation::Length {
            of: vec![S::entity(1), S::entity(2)],
            value: lit(length(30.0)),
        },
        SketchRelation::Pierce {
            point: S::entity(1),
            curve: Operand::Model(Box::new(selection(4))),
        },
        SketchRelation::Weight {
            pole: S::entity(5),
            value: lit(positive_real(1.5)),
        },
        SketchRelation::FixedLength {
            of: vec![S::entity(1)],
        },
        SketchRelation::FixedAngle { of: Vec::new() },
        SketchRelation::Native {
            kind: text("creo:skamp_12"),
            operands: operands(),
            values: vec![ScalarDimension::Length(lit(length(1.0)))],
        },
    ];
    relations.extend(
        contacts::<S>()
            .into_iter()
            .map(|contact| SketchRelation::Curvature {
                a: entity(1),
                b: entity(2),
                contact: Some(contact),
            }),
    );
    relations.extend(
        distance_axes::<S>()
            .into_iter()
            .map(|along| SketchRelation::Distance {
                a: entity(1),
                b: entity(2),
                along: Some(along),
                doubled: Some(true),
                value: bound(1),
            }),
    );
    relations.extend(angle_operands::<S>().into_iter().zip(angle_sectors()).map(
        |(between, sector)| SketchRelation::Angle {
            between,
            sector: Some(sector),
            value: lit(angle(0.5)),
        },
    ));
    relations.extend(
        radial_measures()
            .into_iter()
            .map(|measure| SketchRelation::Radial {
                of: S::entity(1),
                measure,
                value: lit(length(4.0)),
            }),
    );
    relations.extend(sketch_offsets().into_iter().map(SketchRelation::Offset));
    relations.extend(
        sketch_patterns()
            .into_iter()
            .map(|pattern| SketchRelation::Pattern(Box::new(pattern))),
    );
    relations.extend(sketch_polygons().into_iter().map(SketchRelation::Polygon));
    relations.extend(sketch_slots().into_iter().map(SketchRelation::Slot));
    relations
}

fn sketch_constraints<S: Fixture>() -> Vec<SketchConstraint<S>> {
    vec![
        SketchConstraint {
            id: S::constraint(1),
            sketch: S::sketch(1),
            relation: SketchRelation::Fixed { of: entity(1) },
            name: Some("Fix1".to_owned()),
            active: Some(bound(2)),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
            native_ref: Some("catia:sketch:constraint#4".to_owned()),
        },
        SketchConstraint {
            id: S::constraint(2),
            sketch: S::sketch(1),
            relation: SketchRelation::Equal {
                members: Vec::new(),
            },
            name: None,
            active: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        },
    ]
}

fn check_space<S: Fixture>(report: &mut Report, space: &str) {
    let name = |type_name: &str| format!("{type_name}<{space}>");
    report.check(&name("SketchEntity"), sketch_entities::<S>());
    report.check(&name("EntityDefinition"), entity_definitions::<S>());
    report.check(&name("EntityForm"), entity_forms::<S>());
    report.check(&name("EntityRole"), entity_roles::<S>());
    report.check(&name("InterpolatedSpline"), interpolated_splines::<S>());
    report.check(&name("SplineHandle"), spline_handles::<S>());
    report.check(&name("SketchText"), sketch_texts::<S>());
    report.check(&name("TextLayout"), text_layouts::<S>());
    report.check(&name("SketchConstraint"), sketch_constraints::<S>());
    report.check(&name("Operand"), operands::<S>());
    report.check(&name("SketchRelation"), relations::<S>());
    report.check(&name("Contact"), contacts::<S>());
    report.check(&name("DistanceAxis"), distance_axes::<S>());
    report.check(&name("AngleOperands"), angle_operands::<S>());
    report.check(&name("SketchOffset"), sketch_offsets::<S>());
    report.check(&name("SketchPolygon"), sketch_polygons::<S>());
    report.check(&name("SketchSlot"), sketch_slots::<S>());
    report.check(&name("SketchPattern"), sketch_patterns::<S>());
    report.check(&name("SketchPatternLayout"), pattern_layouts::<S>());
    report.check(&name("SketchPatternAxis"), pattern_axes::<S>());
    report.check(&name("SketchPatternDirection"), pattern_directions::<S>());
    report.check(&name("SketchPatternInstance"), pattern_instances::<S>());
}

#[test]
fn every_sketch_type_round_trips() {
    let mut report = Report::default();
    report.check("Sketch", sketches());
    report.check("SketchPlane", sketch_planes());
    report.check("SpatialSketch", spatial_sketches());
    report.check("SketchSolve", solves());
    report.check("SolveState", solve_states());
    report.check("DefiningRole", defining_roles());
    report.check("PlanarCurve", planar_curves());
    report.check("SpatialCurve", spatial_curves());
    report.check("Derivation", derivations());
    report.check("DerivationMode", derivation_modes());
    report.check("TextContent", text_contents());
    report.check("TextAlign", text_aligns());
    report.check(
        "TextHorizontalBox",
        vec![TextHorizontalBox::Ink, TextHorizontalBox::Pen],
    );
    report.check(
        "TextVerticalBox",
        vec![TextVerticalBox::Ink, TextVerticalBox::CapHeight],
    );
    report.check("BlockInstance", block_instances());
    report.check("AngleSector", angle_sectors());
    report.check("RadialMeasure", radial_measures());
    report.check("SlotKind", slot_kinds());
    check_space::<Planar>(&mut report, "Planar");
    check_space::<Spatial>(&mut report, "Spatial");
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// §2.5: a unit-only enum writes a string; an enum whose data variants all
/// hold structs is internally tagged by `kind`; every other enum is
/// adjacently tagged `kind`/`value`.
#[test]
fn each_enum_has_its_tag_form() {
    assert_eq!(to_value(&RadialMeasure::MajorRadius), json!("major_radius"));
    assert_eq!(to_value(&TextVerticalBox::CapHeight), json!("cap_height"));
    assert_eq!(to_value(&SketchPlane::Block), json!({"kind": "block"}));
    refuses::<SketchPlane>(r#"{"kind": "block", "extra": 1}"#);
    let placed = to_value(&SketchPlane::Placed(Box::new(construction(None))));
    assert_eq!(
        placed,
        json!({"kind": "placed", "inputs": [], "after": [], "resolved_in": {}})
    );
    assert_eq!(to_value(&SolveState::NotFull), json!({"kind": "not_full"}));
    assert_eq!(
        to_value(&DefiningRole::Fit(2)),
        json!({"kind": "fit", "value": 2})
    );
    assert_eq!(
        to_value(&Operand::<Planar>::Frame(PrincipalEntity::XAxis)),
        json!({"kind": "frame", "value": "x_axis"})
    );
    assert_eq!(
        to_value(&SketchRelation::<Planar>::Horizontal {
            a: entity(1),
            b: None
        }),
        json!({"kind": "horizontal", "value": {"a": {"kind": "entity", "value": "test:design:sketch-entity#1"}}})
    );
}

/// A planar direction writes `[x, y]` and reads only a unit vector of two
/// components.
#[test]
fn a_planar_direction_is_a_unit_pair() {
    let line = PlanarCurve::InfiniteLine {
        through: point2(0.0, 1.0),
        direction: unit2(0.6, 0.8),
    };
    let written = to_value(&line);
    assert_eq!(written["value"]["direction"], json!([0.6, 0.8]));
    let mut long = written.clone();
    long["value"]["direction"] = json!([1.0, 1.0]);
    refuses::<PlanarCurve>(&long.to_string());
    long["value"]["direction"] = json!([1.0, 0.0, 0.0]);
    refuses::<PlanarCurve>(&long.to_string());
}

/// §2.5: a map with a non-string key writes sorted `[key, value]` pairs and
/// refuses a repeated key.
#[test]
fn spline_handles_are_sorted_pairs() {
    let spline = interpolated_splines::<Planar>().remove(0);
    let written = to_value(&spline);
    assert_eq!(written["handles"][0][0], json!(0));
    assert_eq!(written["handles"][1][0], json!(4));
    let repeated = json!({
        "handles": [[1, {}], [1, {}]],
    });
    let message = refuses::<InterpolatedSpline<Planar>>(&repeated.to_string());
    assert!(message.contains("stated twice"), "{message}");
}

/// §2.5: every `Option` field is omitted for `None` and refuses `null`, and
/// every list and map field is always written and refuses omission.
#[test]
fn an_option_field_is_omitted_and_a_list_field_is_always_written() {
    let entity = sketch_entities::<Planar>().remove(1);
    let written = to_value(&entity);
    let keys = written
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "definition",
            "id",
            "native_options",
            "roles",
            "sketch",
            "source_settings"
        ]
    );
    let mut nulled = written.clone();
    nulled["construction"] = json!(null);
    let message = refuses::<SketchEntity<Planar>>(&nulled.to_string());
    assert!(message.contains("construction"), "{message}");
    let mut without_roles = written.clone();
    without_roles.as_object_mut().unwrap().remove("roles");
    refuses::<SketchEntity<Planar>>(&without_roles.to_string());
    let mut unknown = written;
    unknown["colour"] = json!(1);
    refuses::<SketchEntity<Planar>>(&unknown.to_string());
}

/// Fixed-size operand lists write as arrays and refuse another length.
#[test]
fn fixed_operand_lists_refuse_another_length() {
    let ends = to_value(&Contact::<Spatial>::Ends([
        Spatial::entity(1),
        Spatial::entity(2),
    ]));
    assert_eq!(ends["value"].as_array().unwrap().len(), 2);
    let mut long = ends.clone();
    long["value"]
        .as_array_mut()
        .unwrap()
        .push(json!("test:design:spatial-sketch-entity#3"));
    refuses::<Contact<Spatial>>(&long.to_string());
    let points = to_value(&AngleOperands::<Planar>::Points([
        Planar::entity(1),
        Planar::entity(2),
        Planar::entity(3),
    ]));
    let mut short = points.clone();
    short["value"].as_array_mut().unwrap().pop();
    refuses::<AngleOperands<Planar>>(&short.to_string());
}

/// A spatial sketch's records name spatial ids, and a circular pattern's
/// spacing is angular.
#[test]
fn each_space_names_its_own_records() {
    let entity = sketch_entities::<Spatial>().remove(0);
    let written = to_value(&entity);
    assert_eq!(written["sketch"], json!("test:design:spatial-sketch#1"));
    assert_eq!(
        written["definition"],
        json!({"kind": "free", "value": {"kind": "point", "value": {"x": 1.0, "y": 2.0, "z": 3.0}}})
    );
    let circular = SketchPatternLayout::<Planar>::Circular {
        center: Operand::Frame(PrincipalEntity::Origin),
        spacing: Spacing::Fit { count: lit(6) },
        sides: None,
    };
    assert_eq!(
        to_value(&circular),
        json!({"kind": "circular", "value": {"center": {"kind": "frame", "value": "origin"}, "spacing": {"kind": "fit", "value": {"count": {"kind": "literal", "value": 6}}}}})
    );
}

/// The keys of the design root's sketch arenas, in the order it writes them.
const SKETCH_ARENAS: [&str; 6] = [
    "sketches",
    "sketch_entities",
    "sketch_constraints",
    "spatial_sketches",
    "spatial_sketch_entities",
    "spatial_sketch_constraints",
];

/// A design whose sketch arenas each hold populated records and whose other
/// arenas are empty.
pub(in crate::design) fn sketch_arenas() -> Design {
    Design {
        sketches: sketches(),
        sketch_entities: sketch_entities::<Planar>(),
        sketch_constraints: sketch_constraints::<Planar>(),
        spatial_sketches: spatial_sketches(),
        spatial_sketch_entities: sketch_entities::<Spatial>(),
        spatial_sketch_constraints: sketch_constraints::<Spatial>(),
        ..Design::default()
    }
}

fn values<T: serde::Serialize>(records: &[T]) -> serde_json::Value {
    json!(records.iter().map(to_value).collect::<Vec<_>>())
}

/// §5: planar and spatial sketches, their entities, and their constraints
/// are arenas of the design root, and a document holding them reads back.
#[test]
fn the_sketch_arenas_are_arenas_of_the_design_root() {
    let design = sketch_arenas();
    let value = to_value(&design);
    assert_eq!(value["sketches"], values(&sketches()));
    assert_eq!(
        value["sketch_entities"],
        values(&sketch_entities::<Planar>())
    );
    assert_eq!(
        value["sketch_constraints"],
        values(&sketch_constraints::<Planar>())
    );
    assert_eq!(value["spatial_sketches"], values(&spatial_sketches()));
    assert_eq!(
        value["spatial_sketch_entities"],
        values(&sketch_entities::<Spatial>())
    );
    assert_eq!(
        value["spatial_sketch_constraints"],
        values(&sketch_constraints::<Spatial>())
    );
    for arena in SKETCH_ARENAS {
        assert!(!value[arena].as_array().unwrap().is_empty(), "{arena}");
    }
    round_trip(&design).unwrap();

    let mut ir = CadIr::empty();
    ir.design = design;
    round_trip(&ir).unwrap();
    for arena in SKETCH_ARENAS {
        assert!(
            ir.census()[format!("design.{arena}").as_str()] > 0,
            "{arena}"
        );
    }
}

/// §2.5: each sketch arena of the design root refuses omission, `null`, and
/// an unknown key in a record.
#[test]
fn each_sketch_arena_reads_strictly() {
    let written = to_value(&sketch_arenas());
    for arena in SKETCH_ARENAS {
        let mut omitted = written.clone();
        omitted.as_object_mut().unwrap().remove(arena);
        refuses::<Design>(&omitted.to_string());
        let mut null = written.clone();
        null[arena] = serde_json::Value::Null;
        refuses::<Design>(&null.to_string());
        let mut unknown = written.clone();
        unknown[arena][0]["colour"] = json!(3);
        refuses::<Design>(&unknown.to_string());
    }
}
