// SPDX-License-Identifier: Apache-2.0
//! Every dress-up and hole type, written, read back strictly, and written
//! again; the stated wire forms; and the schema of each type.

use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::Path;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::json;
use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::*;
use crate::design::construction::{
    ConstructionInput, ConstructionKind, FrameValue, InputReference, InputRole, PlaneValue,
    Relation, Subject,
};
use crate::design::names::{NameKind, TopoName};
use crate::design::options::{NativeOption, Solidify, WallSide};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection, Unordered,
};
use crate::design::tests::arm_failures;
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};
use crate::features::geometry::FinitePoint3;
use crate::features::ParameterId;
use crate::math::Point3;
use crate::units::UnitVector3;

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

fn literal<T>(value: T) -> Dimension<T> {
    Dimension::Literal(value)
}

fn parameter<T>(key: u32) -> Dimension<T> {
    Dimension::Parameter(id::<ParameterId>("parameter", key))
}

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

fn non_negative(value: f64) -> NonNegativeLength {
    NonNegativeLength::new(value).unwrap()
}

fn fraction(value: f64) -> Fraction {
    Fraction::new(value).unwrap()
}

fn slope(value: f64) -> SlopeAngle {
    SlopeAngle::new(value).unwrap()
}

fn interior(value: f64) -> InteriorAngle {
    InteriorAngle::new(value).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

fn picked(key: u32) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Edge,
            feature: id("feature", key),
            role: None,
            generator: None,
            index: None,
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn selection<O: MemberOrder>(key: u32) -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(vec![picked(key)]).unwrap()),
        native: None,
    }
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    }
}

fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    Construction {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(90))),
            value: None,
            reversed: None,
        }],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

fn point_construction() -> Construction<kinds::Point> {
    construction(Some(point(1.0, 2.0, 3.0)))
}

fn direction_construction() -> Construction<kinds::Direction> {
    construction(Some(UnitVector3::Z_AXIS))
}

fn plane_construction() -> Construction<kinds::Plane> {
    construction(Some(PlaneValue {
        origin: point(0.0, 0.0, 0.0),
        normal: UnitVector3::Z_AXIS,
        x_axis: UnitVector3::X_AXIS,
    }))
}

fn frame_construction() -> Construction<kinds::Frame> {
    construction(Some(FrameValue {
        origin: point(0.0, 0.0, 1.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }))
}

fn options() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("sldprt:attachment_edges"),
        NativeOption::Bool(literal(true)),
    )])
}

fn settings() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("freecad:Mode"),
        NativeOption::Text(literal(text("Skin"))),
    )])
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes and the same value.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    value: &T,
) -> Result<(), String> {
    let first = json::to_string(value).map_err(|error| error.to_string())?;
    let back: T = read::from_str(&first, &ResourceLimits::desktop())
        .map_err(|error| format!("{first}: {error}"))?;
    let second = json::to_string(&back).map_err(|error| error.to_string())?;
    if first != second {
        return Err(format!("{first} wrote back as {second}"));
    }
    if &back != value {
        return Err(format!("{first} read back as another value"));
    }
    Ok(())
}

fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    json::to_value(value).unwrap()
}

/// Reads `text`, requires a refusal, and returns its message.
fn refuses<T: DeserializeOwned + Debug>(text: &str) -> String {
    match read::from_str::<T>(text, &ResourceLimits::desktop()) {
        Ok(value) => panic!("{text} was read as {value:?}"),
        Err(error) => error.to_string(),
    }
}

#[derive(Default)]
struct Report {
    failures: Vec<String>,
}

impl Report {
    fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(
        &mut self,
        name: &str,
        samples: Vec<T>,
    ) {
        if samples.is_empty() {
            self.failures.push(format!("{name} has no sample"));
        }
        for sample in samples {
            if let Err(error) = round_trip(&sample) {
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }

    /// Requires one sample per variant, told apart by the wire tag.
    fn variants<T: Serialize>(&mut self, name: &str, samples: &[T], expected: usize) {
        let tags = samples
            .iter()
            .map(|sample| match to_value(sample) {
                serde_json::Value::Object(mut fields) => fields.remove("kind").unwrap_or_default(),
                tag => tag,
            })
            .map(|tag| tag.to_string())
            .collect::<BTreeSet<_>>();
        if tags.len() != expected {
            self.failures.push(format!(
                "{name}: {} of {expected} variants sampled",
                tags.len()
            ));
        }
    }
}

fn interpolations() -> Vec<Interpolation> {
    vec![
        Interpolation::Linear,
        Interpolation::Smooth,
        Interpolation::Native(text("catia:cubic")),
    ]
}

fn alongs() -> Vec<Along> {
    vec![Along::Chain {}, Along::Edge(selection(1))]
}

fn station_ats() -> Vec<StationAt> {
    vec![
        StationAt::Parameter {
            along: Along::Chain {},
            fraction: literal(fraction(0.5)),
        },
        StationAt::Parameter {
            along: Along::Edge(selection(2)),
            fraction: parameter(1),
        },
        StationAt::Point(point_construction()),
        StationAt::Native(text("nx:arc_length")),
    ]
}

fn value_stations<Q: Clone>(value: Q) -> Vec<ValueStation<Q>> {
    station_ats()
        .into_iter()
        .map(|at| ValueStation {
            at,
            value: literal(value.clone()),
        })
        .collect()
}

fn stations<Q: Clone>(value: Q) -> Vec<Stations<Q>> {
    vec![
        Stations {
            points: value_stations(value),
            law: Some(Interpolation::Smooth),
            chain_start: Some(selection(3)),
        },
        Stations {
            points: Vec::new(),
            law: None,
            chain_start: None,
        },
    ]
}

fn fillet_sizes() -> Vec<FilletSize> {
    vec![
        FilletSize::Radius(literal(positive(2.0))),
        FilletSize::Chord(parameter(2)),
        FilletSize::Offsets {
            first: literal(positive(1.0)),
            second: literal(positive(1.5)),
        },
        FilletSize::Variable(stations(non_negative(1.0)).remove(0)),
        FilletSize::Held,
        FilletSize::ByProfile,
        FilletSize::Native(text("creo:992")),
    ]
}

fn conic_shapes() -> Vec<ConicShape> {
    vec![
        ConicShape::Rho {
            rho: Some(literal(fraction(0.5))),
        },
        ConicShape::Rho { rho: None },
        ConicShape::BoundaryAndCenter {
            center: literal(positive(1.0)),
        },
        ConicShape::CenterAndRho {
            center: literal(positive(1.0)),
            rho: literal(fraction(0.4)),
        },
        ConicShape::ShoulderRadius(literal(positive(3.0))),
        ConicShape::Native(text("nx:conic")),
    ]
}

fn fillet_profiles() -> Vec<FilletProfile> {
    vec![
        FilletProfile::Circular,
        FilletProfile::Conic(ConicShape::Rho { rho: None }),
        FilletProfile::CurvatureContinuous {
            shape: Some(literal(fraction(0.5))),
        },
        FilletProfile::CurvatureContinuous { shape: None },
        FilletProfile::Native(text("nx:symmetric")),
    ]
}

fn fillet_sections() -> Vec<FilletSection> {
    vec![
        FilletSection {
            size: Some(FilletSize::Radius(literal(positive(2.0)))),
            profile: Some(FilletProfile::Circular),
            weight: Some(literal(PositiveReal::new(1.5).unwrap())),
        },
        FilletSection {
            size: None,
            profile: None,
            weight: None,
        },
    ]
}

fn face_sides() -> Vec<FaceSide> {
    vec![
        FaceSide {
            faces: selection(1),
            hold: Some(selection(2)),
        },
        FaceSide {
            faces: unread(),
            hold: None,
        },
    ]
}

fn round_sides() -> Vec<RoundSide> {
    vec![RoundSide::Automatic {}, RoundSide::Faces(selection(4))]
}

fn fillet_sets() -> Vec<FilletSet> {
    let [section, bare] = <[FilletSection; 2]>::try_from(fillet_sections()).unwrap();
    let [first, second] = <[FaceSide; 2]>::try_from(face_sides()).unwrap();
    vec![
        FilletSet::Edges {
            edges: selection(1),
            section: section.clone(),
            spine: Some(selection(2)),
            hold: [Some(selection(3)), None],
        },
        FilletSet::Edges {
            edges: unread(),
            section: bare.clone(),
            spine: None,
            hold: [None, None],
        },
        FilletSet::Faces {
            first: first.clone(),
            second: Box::new(second.clone()),
            section,
            spine: Some(selection(4)),
            near: Some(point_construction()),
            trim: Some(literal(true)),
        },
        FilletSet::Faces {
            first,
            second: Box::new(second),
            section: bare,
            spine: None,
            near: None,
            trim: None,
        },
        FilletSet::FullRound {
            first: RoundSide::Automatic {},
            center: selection(5),
            second: RoundSide::Faces(selection(6)),
        },
    ]
}

fn edge_setbacks() -> Vec<EdgeSetback> {
    vec![EdgeSetback {
        edge: selection(1),
        distance: literal(non_negative(0.5)),
    }]
}

fn setbacks() -> Vec<Setbacks> {
    vec![
        Setbacks::Uniform(literal(non_negative(1.0))),
        Setbacks::PerEdge(edge_setbacks()),
    ]
}

fn corners() -> Vec<Corner> {
    vec![
        Corner {
            vertex: selection(7),
            setbacks: Some(Setbacks::Uniform(literal(non_negative(1.0)))),
        },
        Corner {
            vertex: selection(8),
            setbacks: None,
        },
    ]
}

fn corner_styles() -> Vec<CornerStyle> {
    vec![
        CornerStyle::RollingBall,
        CornerStyle::Setback,
        CornerStyle::Native(text("nx:corner")),
    ]
}

fn limit_ats() -> Vec<LimitAt> {
    vec![
        LimitAt::Element(selection(1)),
        LimitAt::Point(point_construction()),
    ]
}

fn blend_limits() -> Vec<BlendLimit> {
    vec![
        BlendLimit {
            at: LimitAt::Element(selection(1)),
            reversed: Some(literal(false)),
        },
        BlendLimit {
            at: LimitAt::Point(point_construction()),
            reversed: None,
        },
    ]
}

fn transition_kinds() -> Vec<TransitionKind> {
    vec![
        TransitionKind::Continue,
        TransitionKind::Intersect,
        TransitionKind::Stop,
        TransitionKind::StopAt(LimitAt::Element(selection(2))),
        TransitionKind::CornerSphere {
            radius: Some(literal(positive(2.0))),
            lengths: vec![literal(non_negative(1.0)), parameter(3)],
        },
        TransitionKind::CornerSphere {
            radius: None,
            lengths: Vec::new(),
        },
        TransitionKind::RollingBall {
            radius: None,
            against: Some(selection(3)),
        },
        TransitionKind::SweptAround {
            radius: Some(parameter(4)),
            against: None,
        },
        TransitionKind::Blend,
        TransitionKind::Native(text("creo:6")),
    ]
}

fn transitions() -> Vec<Transition> {
    vec![
        Transition {
            between: BTreeSet::from([0, 1]),
            at: Some(selection(9)),
            kind: Some(TransitionKind::Blend),
            native_options: NativeOptions::from([(
                crate::codec_name!("creo:transition_type"),
                NativeOption::Integer(literal(12)),
            )]),
        },
        Transition {
            between: BTreeSet::new(),
            at: None,
            kind: None,
            native_options: NativeOptions::new(),
        },
    ]
}

fn overflows() -> Vec<Overflow> {
    vec![
        Overflow::Default,
        Overflow::KeepEdge,
        Overflow::KeepSurface,
        Overflow::Native(text("sldprt:overflow")),
    ]
}

fn blend_controls() -> Vec<BlendControls> {
    vec![
        BlendControls {
            corner_style: Some(CornerStyle::Setback),
            corners: corners(),
            limits: blend_limits(),
            keep: Some(selection(4)),
            transitions: transitions(),
            overflow: Some(Overflow::KeepEdge),
            keep_features: Some(literal(true)),
            round_corners: Some(parameter(5)),
        },
        BlendControls {
            corner_style: None,
            corners: Vec::new(),
            limits: Vec::new(),
            keep: None,
            transitions: Vec::new(),
            overflow: None,
            keep_features: None,
            round_corners: None,
        },
    ]
}

fn fillets() -> Vec<Fillet> {
    let [controls, bare] = <[BlendControls; 2]>::try_from(blend_controls()).unwrap();
    vec![
        Fillet {
            sets: fillet_sets(),
            controls,
            native_options: options(),
            source_settings: settings(),
        },
        Fillet {
            sets: Vec::new(),
            controls: bare,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn chamfer_sizes() -> Vec<ChamferSize> {
    vec![
        ChamferSize::Distance(literal(positive(1.0))),
        ChamferSize::Distances {
            first: literal(positive(1.0)),
            second: parameter(1),
        },
        ChamferSize::DistanceAngle {
            distance: literal(positive(1.0)),
            angle: literal(interior(0.785)),
        },
        ChamferSize::WidthAngle {
            width: literal(positive(1.4)),
            angle: literal(interior(0.785)),
        },
        ChamferSize::HeightAngle {
            height: literal(positive(0.7)),
            angle: parameter(2),
        },
        ChamferSize::Vertex(edge_setbacks()),
        ChamferSize::Native(text("sldprt:face_face")),
    ]
}

fn chamfer_measures() -> Vec<ChamferMeasure> {
    vec![
        ChamferMeasure::AlongFaces,
        ChamferMeasure::OffsetFaces,
        ChamferMeasure::Native(text("nx:offset_method")),
    ]
}

fn chamfer_sets() -> Vec<ChamferSet> {
    vec![
        ChamferSet {
            edges: selection(1),
            size: Some(ChamferSize::Distance(literal(positive(1.0)))),
            reference: Some(selection(2)),
            flip: Some(literal(true)),
            measure: Some(ChamferMeasure::OffsetFaces),
        },
        ChamferSet {
            edges: unread(),
            size: None,
            reference: None,
            flip: None,
            measure: None,
        },
    ]
}

fn chamfers() -> Vec<Chamfer> {
    let [controls, bare] = <[BlendControls; 2]>::try_from(blend_controls()).unwrap();
    vec![
        Chamfer {
            sets: chamfer_sets(),
            controls,
            native_options: options(),
            source_settings: settings(),
        },
        Chamfer {
            sets: Vec::new(),
            controls: bare,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn offset_joins() -> Vec<OffsetJoin> {
    vec![
        OffsetJoin::Round,
        OffsetJoin::Sharp,
        OffsetJoin::Native(text("freecad:Join")),
    ]
}

fn face_meetings() -> Vec<FaceMeeting> {
    vec![FaceMeeting::Neighbours, FaceMeeting::All]
}

fn face_thicknesses() -> Vec<FaceThickness> {
    vec![FaceThickness {
        faces: selection(5),
        thickness: literal(positive(2.0)),
    }]
}

fn shells() -> Vec<ShellFeature> {
    vec![
        ShellFeature {
            bodies: Some(selection(1)),
            removed: selection(2),
            thickness: Some(Thickness::OneSide {
                value: literal(positive(1.0)),
                side: Some(WallSide::Inside),
            }),
            faces: face_thicknesses(),
            join: Some(OffsetJoin::Round),
            face_meeting: Some(FaceMeeting::All),
            native_options: options(),
            source_settings: settings(),
        },
        ShellFeature {
            bodies: None,
            removed: Selection {
                at: Anchor::Input,
                picks: Picks::Named(Members::new(Vec::new()).unwrap()),
                native: None,
            },
            thickness: None,
            faces: Vec::new(),
            join: None,
            face_meeting: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn thickens() -> Vec<Thicken> {
    vec![
        Thicken {
            faces: selection(1),
            thickness: Some(Thickness::Offsets {
                outside: literal(length(1.0)),
                inside: literal(length(-0.5)),
            }),
            boolean: Some(Boolean::NewBody {}),
            native_options: options(),
            source_settings: settings(),
        },
        Thicken {
            faces: unread(),
            thickness: None,
            boolean: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn offset_sets() -> Vec<OffsetSet> {
    vec![
        OffsetSet {
            faces: selection(1),
            distance: Some(Dimension::Negated(id("parameter", 1))),
        },
        OffsetSet {
            faces: selection(2),
            distance: None,
        },
    ]
}

fn offset_results() -> Vec<OffsetResult> {
    vec![
        OffsetResult::Move {},
        OffsetResult::Copy(SheetOutput {
            solid: Some(Solidify::IfClosed),
            boolean: None,
        }),
    ]
}

fn offsets() -> Vec<Offset> {
    vec![
        Offset {
            sets: offset_sets(),
            result: offset_results().remove(1),
            join: Some(OffsetJoin::Sharp),
            face_meeting: Some(FaceMeeting::Neighbours),
            native_options: options(),
            source_settings: settings(),
        },
        Offset {
            sets: Vec::new(),
            result: OffsetResult::Move {},
            join: None,
            face_meeting: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn draft_angles() -> Vec<DraftAngle> {
    vec![
        DraftAngle::One(literal(slope(0.05))),
        DraftAngle::Two {
            first: literal(slope(0.05)),
            second: literal(slope(-0.1)),
        },
        DraftAngle::Symmetric(parameter(1)),
        DraftAngle::Variable(stations(slope(0.1)).remove(0)),
        DraftAngle::Native(text("inventor:mode_3")),
    ]
}

fn draft_sets() -> Vec<DraftSet> {
    vec![
        DraftSet {
            faces: Some(selection(1)),
            fixed: Some(selection(2)),
            angle: Some(DraftAngle::One(literal(slope(0.05)))),
        },
        DraftSet {
            faces: None,
            fixed: None,
            angle: None,
        },
    ]
}

fn step_faces() -> Vec<StepFaces> {
    vec![StepFaces::Tapered, StepFaces::Perpendicular]
}

fn drafts() -> Vec<Draft> {
    vec![
        Draft {
            sets: draft_sets(),
            pull: Some(direction_construction()),
            neutral: Some(plane_construction()),
            parting: Some(selection(3)),
            steps: Some(StepFaces::Tapered),
            native_options: options(),
            source_settings: settings(),
        },
        Draft {
            sets: Vec::new(),
            pull: None,
            neutral: None,
            parting: None,
            steps: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn cones() -> Vec<Cone> {
    vec![
        Cone {
            diameter: Some(literal(positive(10.0))),
            depth: None,
            angle: Some(literal(interior(1.57))),
        },
        Cone {
            diameter: None,
            depth: Some(parameter(1)),
            angle: None,
        },
    ]
}

fn hole_heads() -> Vec<HoleHead> {
    vec![
        HoleHead::Plain,
        HoleHead::Counterbore {
            diameter: Some(literal(positive(10.0))),
            depth: Some(literal(positive(5.0))),
        },
        HoleHead::Counterbore {
            diameter: None,
            depth: None,
        },
        HoleHead::Countersink(cones().remove(0)),
        HoleHead::Counterdrill {
            diameter: Some(literal(positive(8.0))),
            depth: Some(literal(positive(2.0))),
            angle: Some(literal(interior(1.57))),
        },
        HoleHead::Spotface {
            diameter: Some(literal(positive(12.0))),
            depth: Some(literal(non_negative(0.0))),
        },
        HoleHead::Native(text("nx:relief")),
    ]
}

fn hole_bottoms() -> Vec<HoleBottom> {
    vec![
        HoleBottom::Flat,
        HoleBottom::DrillPoint {
            angle: Some(literal(interior(2.07))),
            depth_to: Some(DepthTo::Tip),
        },
        HoleBottom::DrillPoint {
            angle: None,
            depth_to: None,
        },
        HoleBottom::Trimmed,
        HoleBottom::Native(text("catia:bottom")),
    ]
}

fn depth_tos() -> Vec<DepthTo> {
    vec![DepthTo::Shoulder, DepthTo::Tip]
}

fn bores() -> Vec<Bore> {
    let cone = cones().remove(0);
    vec![
        Bore {
            diameter: Some(literal(positive(6.0))),
            head: Some(HoleHead::Plain),
            taper: Some(literal(slope(0.02))),
            bottom: Some(HoleBottom::Flat),
            near_chamfer: Some(cone.clone()),
            neck_chamfer: Some(cone.clone()),
            far_chamfer: Some(cone),
        },
        Bore {
            diameter: None,
            head: None,
            taper: None,
            bottom: None,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        },
    ]
}

fn slot_lengths() -> Vec<SlotLength> {
    vec![SlotLength::CenterToCenter, SlotLength::TangentToTangent]
}

fn hole_shapes() -> Vec<HoleShape> {
    let [bore, bare] = <[Bore; 2]>::try_from(bores()).unwrap();
    vec![
        HoleShape::Bore(bore.clone()),
        HoleShape::Bore(bare.clone()),
        HoleShape::Revolved(selection(1)),
        HoleShape::Slot {
            bore,
            length: literal(positive(20.0)),
            measure: Some(SlotLength::CenterToCenter),
            direction: Box::new(direction_construction()),
        },
        HoleShape::Slot {
            bore: bare,
            length: parameter(1),
            measure: None,
            direction: Box::new(direction_construction()),
        },
    ]
}

fn center_sources() -> Vec<CenterSource> {
    vec![
        CenterSource::Points,
        CenterSource::Circles,
        CenterSource::Arcs,
    ]
}

fn hole_positions() -> Vec<HolePositions> {
    vec![
        HolePositions::Points {
            points: selection(1),
            centers: Some(BTreeSet::from([CenterSource::Arcs, CenterSource::Points])),
        },
        HolePositions::Points {
            points: unread(),
            centers: None,
        },
        HolePositions::Placed(vec![point_construction()]),
    ]
}

fn standard_sizes() -> Vec<StandardSize> {
    vec![
        StandardSize {
            standard: text("ISO"),
            kind: Some(text("Socket head cap screw")),
            size: Some(literal(text("M6"))),
            class: Some(text("6H")),
        },
        StandardSize {
            standard: text("ANSI Metric"),
            kind: None,
            size: Some(parameter(1)),
            class: None,
        },
    ]
}

fn thread_lengths() -> Vec<ThreadLength> {
    vec![
        ThreadLength::Full,
        ThreadLength::Blind(literal(positive(12.0))),
        ThreadLength::ToElement {
            element: selection(1),
            offset: Some(literal(length(-1.0))),
        },
        ThreadLength::ToElement {
            element: selection(2),
            offset: None,
        },
        ThreadLength::Standard,
        ThreadLength::Native(text("creo:thread_depth")),
    ]
}

fn thread_specs() -> Vec<ThreadSpec> {
    vec![
        ThreadSpec {
            standard: standard_sizes().pop(),
            major_diameter: Some(literal(positive(6.0))),
            minor_diameter: Some(literal(positive(4.9))),
            pitch_diameter: Some(literal(positive(5.35))),
            pitch: Some(parameter(1)),
            hand: Some(Hand::Right),
            length: Some(ThreadLength::Full),
            modeled: Some(literal(false)),
        },
        ThreadSpec {
            standard: None,
            major_diameter: None,
            minor_diameter: None,
            pitch_diameter: None,
            pitch: None,
            hand: None,
            length: None,
            modeled: None,
        },
    ]
}

fn stage_holes() -> Vec<StageHole> {
    vec![
        StageHole::Stated {
            shape: Box::new(hole_shapes().remove(0)),
            extent: Some(Box::new(Limit::Amount(literal(length(5.0))))),
            standard: standard_sizes().pop(),
            thread: thread_specs().pop().map(Box::new),
        },
        StageHole::Stated {
            shape: Box::new(hole_shapes().remove(1)),
            extent: None,
            standard: None,
            thread: None,
        },
        StageHole::PartFeature(id("feature", 4)),
    ]
}

fn series_stages() -> Vec<SeriesStage> {
    stage_holes()
        .into_iter()
        .map(|hole| SeriesStage {
            parts: selection(6),
            hole,
        })
        .collect()
}

fn holes() -> Vec<Hole> {
    vec![
        Hole {
            positions: hole_positions().remove(0),
            start: Some(selection(1)),
            direction: Some(direction_construction()),
            shape: hole_shapes().remove(0),
            extent: Some(Limit::ThroughAll),
            standard: standard_sizes().pop(),
            thread: thread_specs().pop(),
            targets: Some(Targets::All {}),
            series: series_stages(),
            native_options: options(),
            source_settings: settings(),
        },
        Hole {
            positions: HolePositions::Placed(Vec::new()),
            start: None,
            direction: None,
            shape: hole_shapes().remove(1),
            extent: None,
            standard: None,
            thread: None,
            targets: None,
            series: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn threads() -> Vec<Thread> {
    let [spec, bare] = <[ThreadSpec; 2]>::try_from(thread_specs()).unwrap();
    vec![
        Thread {
            face: selection(1),
            start: Some(selection(2)),
            spec,
            native_options: options(),
            source_settings: settings(),
        },
        Thread {
            face: unread(),
            start: None,
            spec: bare,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn emboss_effects() -> Vec<EmbossEffect> {
    vec![
        EmbossEffect::Emboss {
            depth: Some(literal(positive(1.0))),
        },
        EmbossEffect::Emboss { depth: None },
        EmbossEffect::Engrave {
            depth: Some(parameter(1)),
        },
        EmbossEffect::Scribe,
        EmbossEffect::Native(text("f3d:wrap_unresolved")),
    ]
}

fn emboss_mappings() -> Vec<EmbossMapping> {
    vec![
        EmbossMapping::Wrap,
        EmbossMapping::Project,
        EmbossMapping::Native(text("inventor:mapping")),
    ]
}

fn embosses() -> Vec<Emboss> {
    vec![
        Emboss {
            profile: selection(1),
            effect: emboss_effects().remove(0),
            face: Some(selection(2)),
            mapping: Some(EmbossMapping::Wrap),
            direction: Some(direction_construction()),
            targets: Some(Targets::Selected(selection(3))),
            native_options: options(),
            source_settings: settings(),
        },
        Emboss {
            profile: unread(),
            effect: EmbossEffect::Scribe,
            face: None,
            mapping: None,
            direction: None,
            targets: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn dome_shapes() -> Vec<DomeShape> {
    vec![
        DomeShape::Default,
        DomeShape::Elliptical,
        DomeShape::Continuous,
    ]
}

fn domes() -> Vec<Dome> {
    vec![
        Dome {
            faces: selection(1),
            height: Some(literal(non_negative(0.0))),
            concave: Some(literal(true)),
            shape: Some(DomeShape::Elliptical),
            direction: Some(direction_construction()),
            points: Some(selection(2)),
            native_options: options(),
            source_settings: settings(),
        },
        Dome {
            faces: unread(),
            height: None,
            concave: None,
            shape: None,
            direction: None,
            points: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn decal_placements() -> Vec<DecalPlacement> {
    vec![
        DecalPlacement {
            frame: frame_construction(),
            width: literal(positive(40.0)),
            height: parameter(1),
            mirrored: true,
        },
        DecalPlacement {
            frame: construction(None),
            width: literal(positive(10.0)),
            height: literal(positive(5.0)),
            mirrored: false,
        },
    ]
}

fn decals() -> Vec<Decal> {
    vec![
        Decal {
            image: id("asset", 1),
            faces: selection(1),
            chain_faces: Some(literal(false)),
            opacity: Some(literal(fraction(0.8))),
            placement: decal_placements().pop(),
            native_options: options(),
            source_settings: settings(),
        },
        Decal {
            image: id("asset", 2),
            faces: unread(),
            chain_faces: None,
            opacity: None,
            placement: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

#[test]
fn every_dressup_type_round_trips() {
    let mut report = Report::default();
    report.check("Stations<NonNegativeLength>", stations(non_negative(1.0)));
    report.check("Stations<SlopeAngle>", stations(slope(0.1)));
    report.check(
        "ValueStation<NonNegativeLength>",
        value_stations(non_negative(1.0)),
    );
    report.check("ValueStation<SlopeAngle>", value_stations(slope(0.1)));
    report.check("StationAt", station_ats());
    report.check("Along", alongs());
    report.check("Interpolation", interpolations());
    report.check("Fillet", fillets());
    report.check("FilletSet", fillet_sets());
    report.check("FaceSide", face_sides());
    report.check("RoundSide", round_sides());
    report.check("FilletSection", fillet_sections());
    report.check("FilletSize", fillet_sizes());
    report.check("FilletProfile", fillet_profiles());
    report.check("ConicShape", conic_shapes());
    report.check("Chamfer", chamfers());
    report.check("ChamferSet", chamfer_sets());
    report.check("ChamferSize", chamfer_sizes());
    report.check("ChamferMeasure", chamfer_measures());
    report.check("BlendControls", blend_controls());
    report.check("CornerStyle", corner_styles());
    report.check("Corner", corners());
    report.check("Setbacks", setbacks());
    report.check("EdgeSetback", edge_setbacks());
    report.check("BlendLimit", blend_limits());
    report.check("LimitAt", limit_ats());
    report.check("Transition", transitions());
    report.check("TransitionKind", transition_kinds());
    report.check("Overflow", overflows());
    report.check("ShellFeature", shells());
    report.check("FaceThickness", face_thicknesses());
    report.check("OffsetJoin", offset_joins());
    report.check("FaceMeeting", face_meetings());
    report.check("Thicken", thickens());
    report.check("Offset", offsets());
    report.check("OffsetSet", offset_sets());
    report.check("OffsetResult", offset_results());
    report.check("Draft", drafts());
    report.check("DraftSet", draft_sets());
    report.check("DraftAngle", draft_angles());
    report.check("StepFaces", step_faces());
    report.check("Hole", holes());
    report.check("SeriesStage", series_stages());
    report.check("StageHole", stage_holes());
    report.check("HolePositions", hole_positions());
    report.check("CenterSource", center_sources());
    report.check("HoleShape", hole_shapes());
    report.check("SlotLength", slot_lengths());
    report.check("Bore", bores());
    report.check("HoleHead", hole_heads());
    report.check("Cone", cones());
    report.check("HoleBottom", hole_bottoms());
    report.check("DepthTo", depth_tos());
    report.check("StandardSize", standard_sizes());
    report.check("Thread", threads());
    report.check("ThreadSpec", thread_specs());
    report.check("ThreadLength", thread_lengths());
    report.check("Emboss", embosses());
    report.check("EmbossEffect", emboss_effects());
    report.check("EmbossMapping", emboss_mappings());
    report.check("Dome", domes());
    report.check("DomeShape", dome_shapes());
    report.check("Decal", decals());
    report.check("DecalPlacement", decal_placements());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// §5.10: each dress-up operation is the `FeatureOperation` arm its family
/// names, holding the operation as its value.
#[test]
fn every_dressup_operation_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    let failures = [
        arm_failures(Family::Fillet, fillets(), Op::Fillet),
        arm_failures(Family::Chamfer, chamfers(), Op::Chamfer),
        arm_failures(Family::Shell, shells(), Op::Shell),
        arm_failures(Family::Draft, drafts(), Op::Draft),
        arm_failures(Family::Thicken, thickens(), Op::Thicken),
        arm_failures(Family::Offset, offsets(), Op::Offset),
        arm_failures(Family::Hole, holes(), Op::Hole),
        arm_failures(Family::Thread, threads(), Op::Thread),
        arm_failures(Family::Emboss, embosses(), Op::Emboss),
        arm_failures(Family::Dome, domes(), Op::Dome),
        arm_failures(Family::Decal, decals(), Op::Decal),
    ]
    .concat();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_variant_is_sampled() {
    let mut report = Report::default();
    report.variants("StationAt", &station_ats(), 3);
    report.variants("Along", &alongs(), 2);
    report.variants("Interpolation", &interpolations(), 3);
    report.variants("FilletSet", &fillet_sets(), 3);
    report.variants("RoundSide", &round_sides(), 2);
    report.variants("FilletSize", &fillet_sizes(), 7);
    report.variants("FilletProfile", &fillet_profiles(), 4);
    report.variants("ConicShape", &conic_shapes(), 5);
    report.variants("ChamferSize", &chamfer_sizes(), 7);
    report.variants("ChamferMeasure", &chamfer_measures(), 3);
    report.variants("CornerStyle", &corner_styles(), 3);
    report.variants("Setbacks", &setbacks(), 2);
    report.variants("LimitAt", &limit_ats(), 2);
    report.variants("TransitionKind", &transition_kinds(), 9);
    report.variants("Overflow", &overflows(), 4);
    report.variants("OffsetJoin", &offset_joins(), 3);
    report.variants("FaceMeeting", &face_meetings(), 2);
    report.variants("OffsetResult", &offset_results(), 2);
    report.variants("DraftAngle", &draft_angles(), 5);
    report.variants("StepFaces", &step_faces(), 2);
    report.variants("StageHole", &stage_holes(), 2);
    report.variants("HolePositions", &hole_positions(), 2);
    report.variants("CenterSource", &center_sources(), 3);
    report.variants("HoleShape", &hole_shapes(), 3);
    report.variants("SlotLength", &slot_lengths(), 2);
    report.variants("HoleHead", &hole_heads(), 6);
    report.variants("HoleBottom", &hole_bottoms(), 4);
    report.variants("DepthTo", &depth_tos(), 2);
    report.variants("ThreadLength", &thread_lengths(), 5);
    report.variants("EmbossEffect", &emboss_effects(), 4);
    report.variants("EmbossMapping", &emboss_mappings(), 3);
    report.variants("DomeShape", &dome_shapes(), 3);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn an_omitted_optional_field_refuses_null_and_unknown_fields_are_refused() {
    refuses::<HoleShape>(r#"{"kind": "bore", "diameter": null}"#);
    refuses::<HoleShape>(r#"{"kind": "bore", "extra": 1}"#);
    refuses::<RoundSide>(r#"{"kind": "automatic", "extra": 1}"#);
    refuses::<Along>(r#"{"kind": "chain", "extra": 1}"#);
    refuses::<OffsetResult>(r#"{"kind": "move", "extra": 1}"#);
    let bore = read::from_str::<HoleShape>(r#"{"kind": "bore"}"#, &ResourceLimits::desktop());
    assert_eq!(bore.unwrap(), HoleShape::Bore(bores().remove(1)));
}

#[test]
fn a_set_refuses_a_repeated_member() {
    assert!(refuses::<Transition>(r#"{"between": [1, 1]}"#).contains("twice"));
}

#[test]
fn hole_centers_are_a_sorted_set_that_refuses_null_and_repeats() {
    let positions = hole_positions().remove(0);
    let mut wire = to_value(&positions);
    assert_eq!(wire["value"]["centers"], json!(["points", "arcs"]));
    wire["value"]["centers"] = json!(null);
    assert!(refuses::<HolePositions>(&wire.to_string()).contains("centers"));
    wire["value"]["centers"] = json!(["arcs", "arcs"]);
    assert!(refuses::<HolePositions>(&wire.to_string()).contains("twice"));
}

#[test]
fn the_tag_forms_of_the_family() {
    assert_eq!(
        to_value(&FilletSet::FullRound {
            first: RoundSide::Automatic {},
            center: unread(),
            second: RoundSide::Automatic {},
        }),
        json!({
            "kind": "full_round",
            "first": {"kind": "automatic"},
            "center": to_value(&unread::<Unordered>()),
            "second": {"kind": "automatic"},
        })
    );
    assert_eq!(to_value(&FilletSize::Held), json!({"kind": "held"}));
    assert_eq!(
        to_value(&DraftAngle::One(literal(slope(0.05)))),
        json!({"kind": "one", "value": {"kind": "literal", "value": 0.05}})
    );
    assert_eq!(to_value(&StepFaces::Tapered), json!("tapered"));
    let edges = FilletSet::Edges {
        edges: unread(),
        section: fillet_sections().remove(1),
        spine: None,
        hold: [None, None],
    };
    assert_eq!(to_value(&edges)["hold"], json!([null, null]));
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 11] = [
        ("ChamferMeasure", reads::<ChamferMeasure>),
        ("CornerStyle", reads::<CornerStyle>),
        ("EmbossEffect", reads::<EmbossEffect>),
        ("EmbossMapping", reads::<EmbossMapping>),
        ("FilletProfile", reads::<FilletProfile>),
        ("FilletSize", reads::<FilletSize>),
        ("HoleBottom", reads::<HoleBottom>),
        ("HoleHead", reads::<HoleHead>),
        ("Overflow", reads::<Overflow>),
        ("ThreadLength", reads::<ThreadLength>),
        ("TransitionKind", reads::<TransitionKind>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/dressup.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

#[test]
fn a_native_option_key_stated_twice_is_refused() {
    let wire = json::to_string(&fillets().remove(0)).unwrap();
    let entry = "\"sldprt:attachment_edges\":{\"kind\":\"bool\",\"value\":{\"kind\":\"literal\",\"value\":true}}";
    let twice = wire.replacen(entry, &format!("{entry},{entry}"), 1);
    assert_ne!(twice, wire);
    refuses::<Fillet>(&twice);
}
