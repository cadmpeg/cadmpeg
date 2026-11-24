// SPDX-License-Identifier: Apache-2.0
//! Every assembly operation type and every shared pattern layout type,
//! written, read back strictly, and written again; the stated wire forms; and
//! the schema of each type.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::Path;

use cadmpeg_core::decode::ResourceLimits;
#[cfg(feature = "schema")]
use schemars::{JsonSchema, SchemaGenerator};
use serde::de::DeserializeOwned;
use serde_json::json;

use super::*;
use crate::design::construction::{
    AxisValue, ConstructionInput, ConstructionKind, FrameAxis, FrameValue, InputReference,
    InputRole, PlaneValue, Relation, ScalarDimension, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::feature::OperationFamily;
use crate::design::names::{NameKind, TopoName};
use crate::design::ops::shared::{
    Alignment, AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, Coupling, DimensionRun,
    DimensionTable, DriveAxis, DriveMode, Increment, InstanceOrientation, JointConnector,
    JointDrive, JointKind, JointMotion, JointOffset, LinearRun, Locus, PairElement,
    PathAxisControl, PathPosition, PathReference, PathRollControl, PatternRun, RunSides,
    SlotPosition, Spacing, TableRow, TangentContact, UpToFill, WidthMode,
};
use crate::design::ops::shared::{
    CircularLayout, CompositeStage, LinearLayout, MirrorLayout, ScaleLayout,
};
use crate::design::options::{NativeOption, SymmetricMeasure};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PathStep, PickTarget, Picked, Picks, Selection, StepRef,
};
use crate::design::tests::unit_variants::{self, assert_units_refuse_value, Reader};
use crate::features::geometry::FinitePoint3;
use crate::features::ParameterId;
use crate::math::Point3;
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, PositiveReal,
};
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

fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn non_negative(value: f64) -> NonNegativeLength {
    NonNegativeLength::new(value).unwrap()
}

fn non_negative_real(value: f64) -> NonNegativeReal {
    NonNegativeReal::new(value).unwrap()
}

fn positive_real(value: f64) -> PositiveReal {
    PositiveReal::new(value).unwrap()
}

fn fraction(value: f64) -> Fraction {
    Fraction::new(value).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

fn occurrence(key: u32) -> OccurrenceId {
    id("occurrence", key)
}

fn picked(key: u32) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Face,
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

fn axis_construction() -> Construction<kinds::Axis> {
    construction(Some(AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }))
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
        crate::codec_name!("f3d:joint_mode"),
        NativeOption::Text(literal(text("as_built"))),
    )])
}

fn settings() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("f3d:item_kind"),
        NativeOption::Text(literal(text("insert"))),
    )])
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes and the same value.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    value: &T,
) -> Result<(), String> {
    let first = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let back: T = read::from_str(&first, &ResourceLimits::desktop())
        .map_err(|error| format!("{first}: {error}"))?;
    let second = serde_json::to_string(&back).map_err(|error| error.to_string())?;
    if first != second {
        return Err(format!("{first} wrote back as {second}"));
    }
    if &back != value {
        return Err(format!("{first} read back as another value"));
    }
    Ok(())
}

fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

fn reads<T: DeserializeOwned + Debug>(text: &str) -> T {
    read::from_str(text, &ResourceLimits::desktop()).unwrap()
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

fn pair_elements() -> Vec<PairElement> {
    vec![PairElement::First, PairElement::Second]
}

fn up_to_fills<Q>(pitch: Q) -> Vec<UpToFill<Q>> {
    vec![UpToFill::Count(literal(4)), UpToFill::Pitch(literal(pitch))]
}

fn loci() -> Vec<Locus> {
    vec![
        Locus::Centroid,
        Locus::Point(Box::new(point_construction())),
    ]
}

fn spacings<Q: Clone>(value: Q) -> Vec<Spacing<Q>> {
    vec![
        Spacing::Pitch {
            count: literal(3),
            pitch: literal(value.clone()),
        },
        Spacing::Span {
            count: parameter(1),
            span: parameter(2),
        },
        Spacing::PitchSpan {
            pitch: literal(value.clone()),
            span: literal(value.clone()),
        },
        Spacing::Fit { count: literal(6) },
        Spacing::UpTo {
            reference: Box::new(selection(3)),
            fill: UpToFill::Count(literal(5)),
            offset: Some(literal(value.clone())),
            from: Some(Locus::Centroid),
        },
        Spacing::UpTo {
            reference: Box::new(unread()),
            fill: UpToFill::Pitch(literal(value.clone())),
            offset: None,
            from: None,
        },
        Spacing::List(vec![literal(value.clone()), literal(value)]),
        Spacing::Native(text("nx:spacing_kind_9")),
    ]
}

fn run_sides<Q: Clone>(value: Q) -> Vec<RunSides<Q>> {
    vec![
        RunSides::One,
        RunSides::Against,
        RunSides::Symmetric {
            count: Some(SymmetricMeasure::PerSide),
            extent: Some(SymmetricMeasure::Total),
        },
        RunSides::Symmetric {
            count: None,
            extent: None,
        },
        RunSides::Reverse(Box::new(Spacing::Pitch {
            count: literal(2),
            pitch: literal(value),
        })),
    ]
}

fn increments() -> Vec<Increment> {
    vec![
        Increment {
            parameter: id("parameter", 7),
            step: ScalarDimension::Length(literal(length(2.5))),
        },
        Increment {
            parameter: id("parameter", 8),
            step: ScalarDimension::Angle(parameter(9)),
        },
    ]
}

fn pattern_runs<Q: Clone>(value: Q) -> Vec<PatternRun<Q>> {
    vec![
        PatternRun {
            spacing: Spacing::Pitch {
                count: literal(4),
                pitch: literal(value.clone()),
            },
            sides: Some(RunSides::Symmetric {
                count: Some(SymmetricMeasure::Total),
                extent: None,
            }),
            seed: Some(literal(1)),
            increments: increments(),
        },
        PatternRun {
            spacing: Spacing::Fit { count: literal(8) },
            sides: None,
            seed: None,
            increments: Vec::new(),
        },
    ]
}

fn linear_runs() -> Vec<LinearRun> {
    pattern_runs(length(10.0))
        .into_iter()
        .map(|run| LinearRun {
            direction: direction_construction(),
            run,
        })
        .collect()
}

fn path_references() -> Vec<PathReference> {
    vec![
        PathReference::PlaneNormal,
        PathReference::Direction(Box::new(direction_construction())),
    ]
}

fn instance_orientations() -> Vec<InstanceOrientation> {
    vec![
        InstanceOrientation::Fixed,
        InstanceOrientation::Follow,
        InstanceOrientation::Frenet,
        InstanceOrientation::Rolled {
            reference: PathReference::PlaneNormal,
            reference_axis: FrameAxis::Y,
            seed_axis: None,
        },
        InstanceOrientation::Rolled {
            reference: PathReference::Direction(Box::new(direction_construction())),
            reference_axis: FrameAxis::Z,
            seed_axis: Some(Box::new(direction_construction())),
        },
        InstanceOrientation::Native(text("nx:orientation_3")),
    ]
}

fn dimension_runs() -> Vec<DimensionRun> {
    vec![
        DimensionRun {
            count: literal(3),
            increments: increments(),
        },
        DimensionRun {
            count: parameter(4),
            increments: Vec::new(),
        },
    ]
}

fn table_rows() -> Vec<TableRow> {
    vec![
        TableRow {
            instance: 1,
            values: vec![Some(ScalarDimension::Length(literal(length(5.0)))), None],
        },
        TableRow {
            instance: 2,
            values: Vec::new(),
        },
    ]
}

fn dimension_tables() -> Vec<DimensionTable> {
    vec![
        DimensionTable {
            parameters: vec![id("parameter", 1), id("parameter", 2)],
            rows: table_rows(),
        },
        DimensionTable {
            parameters: Vec::new(),
            rows: Vec::new(),
        },
    ]
}

fn chain_links() -> Vec<ChainLink> {
    vec![ChainLink {
        pins: [axis_construction(), axis_construction()],
    }]
}

fn chain_linkss() -> Vec<ChainLinks> {
    vec![
        ChainLinks::Connected(chain_links()),
        ChainLinks::Native {
            kind: text("sldprt:distance_chain"),
            picks: selection(4),
        },
    ]
}

fn chain_layouts() -> Vec<ChainLayout> {
    chain_linkss()
        .into_iter()
        .map(|links| ChainLayout {
            path: selection(5),
            links,
        })
        .collect()
}

fn pattern_layouts() -> Vec<PatternLayout> {
    let mut runs = linear_runs();
    let second = runs.pop();
    let first = runs.remove(0);
    vec![
        PatternLayout::Linear(LinearLayout {
            first: first.clone(),
            second,
            seed_only: Some(literal(true)),
        }),
        PatternLayout::Linear(LinearLayout {
            first,
            second: None,
            seed_only: None,
        }),
        PatternLayout::Circular(CircularLayout {
            axis: axis_construction(),
            angular: pattern_runs(angle(0.5)).remove(0),
            radial: pattern_runs(length(4.0)).pop(),
            orientation: Some(InstanceOrientation::Follow),
        }),
        PatternLayout::Circular(CircularLayout {
            axis: axis_construction(),
            angular: pattern_runs(angle(0.25)).remove(1),
            radial: None,
            orientation: None,
        }),
        PatternLayout::Path {
            path: selection(6),
            run: pattern_runs(length(3.0)).remove(0),
            orientation: Some(InstanceOrientation::Frenet),
        },
        PatternLayout::Points {
            points: selection(7),
            reference: Some(Locus::Centroid),
        },
        PatternLayout::Points {
            points: unread(),
            reference: None,
        },
        PatternLayout::Dimensions(dimension_runs()),
        PatternLayout::Table(dimension_tables().remove(0)),
        PatternLayout::Mirror(MirrorLayout {
            plane: plane_construction(),
            stitch_tolerance: Some(literal(length(0.01))),
        }),
        PatternLayout::Composite(vec![
            CompositeStage::Mirror(MirrorLayout {
                plane: plane_construction(),
                stitch_tolerance: None,
            }),
            CompositeStage::Scale(ScaleLayout {
                factor: literal(positive_real(2.0)),
                count: literal(3),
                center: Locus::Centroid,
            }),
        ]),
        PatternLayout::Scale(ScaleLayout {
            factor: parameter(3),
            count: literal(2),
            center: Locus::Point(Box::new(point_construction())),
        }),
        PatternLayout::Follow(selection(8)),
        PatternLayout::Chain(chain_layouts().remove(0)),
        PatternLayout::Native(text("nx:spiral")),
    ]
}

fn placement_bases() -> Vec<PlacementBase> {
    vec![PlacementBase::Parent, PlacementBase::OwnOccurrence]
}

fn joint_connectors() -> Vec<JointConnector> {
    vec![
        JointConnector::Frame(frame_construction()),
        JointConnector::Plane(plane_construction()),
        JointConnector::Axis(axis_construction()),
        JointConnector::Point(point_construction()),
        JointConnector::Entity(selection(10)),
    ]
}

fn alignments() -> Vec<Alignment> {
    vec![
        Alignment::Aligned,
        Alignment::Opposed,
        Alignment::Nearest,
        Alignment::Native(text("catia:undefined")),
    ]
}

fn joint_motions() -> Vec<JointMotion> {
    vec![
        JointMotion::Rigid,
        JointMotion::Revolute { axis: FrameAxis::Y },
        JointMotion::Slider { axis: FrameAxis::Z },
        JointMotion::Cylindrical { axis: FrameAxis::X },
        JointMotion::Planar { axis: FrameAxis::Z },
        JointMotion::Ball,
        JointMotion::PinSlot {
            axis: FrameAxis::Z,
            slide: FrameAxis::X,
        },
        JointMotion::Native(text("f3d:freedom:3")),
    ]
}

fn joint_offsets() -> Vec<JointOffset> {
    vec![
        JointOffset {
            x: Some(literal(length(1.0))),
            y: Some(parameter(2)),
            z: Some(literal(length(-3.0))),
            angle: Some(literal(angle(0.5))),
        },
        JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
    ]
}

fn length_bounds() -> Vec<Bounds<Length>> {
    vec![
        Bounds {
            min: Some(literal(length(-5.0))),
            max: Some(literal(length(5.0))),
        },
        Bounds {
            min: None,
            max: Some(parameter(3)),
        },
    ]
}

fn angle_bounds() -> Vec<Bounds<Angle>> {
    vec![
        Bounds {
            min: Some(literal(angle(-1.0))),
            max: None,
        },
        Bounds {
            min: Some(parameter(4)),
            max: Some(literal(angle(1.0))),
        },
    ]
}

fn axis_limits() -> Vec<AxisLimits> {
    vec![
        AxisLimits::Rotation {
            axis: FrameAxis::Z,
            limits: angle_bounds().pop(),
            rest: Some(literal(angle(0.1))),
        },
        AxisLimits::Rotation {
            axis: FrameAxis::X,
            limits: None,
            rest: Some(parameter(5)),
        },
        AxisLimits::Translation {
            axis: FrameAxis::Y,
            limits: length_bounds().pop(),
            rest: Some(literal(length(2.0))),
        },
        AxisLimits::Translation {
            axis: FrameAxis::Z,
            limits: length_bounds().pop(),
            rest: None,
        },
    ]
}

fn drive_axes() -> Vec<DriveAxis> {
    vec![
        DriveAxis::Rotation { axis: FrameAxis::Z },
        DriveAxis::Translation { axis: FrameAxis::X },
    ]
}

fn drive_modes() -> Vec<DriveMode> {
    vec![DriveMode::Force, DriveMode::Acceleration]
}

fn joint_drives() -> Vec<JointDrive> {
    vec![
        JointDrive {
            axis: DriveAxis::Rotation { axis: FrameAxis::Z },
            mode: DriveMode::Force,
            target_position: Some(real(0.5)),
            target_velocity: Some(real(-1.5)),
            stiffness: Some(non_negative_real(100.0)),
            damping: Some(non_negative_real(0.0)),
            max_force: Some(positive_real(20.0)),
        },
        JointDrive {
            axis: DriveAxis::Translation { axis: FrameAxis::Y },
            mode: DriveMode::Acceleration,
            target_position: None,
            target_velocity: None,
            stiffness: None,
            damping: None,
            max_force: None,
        },
    ]
}

fn tangent_contacts() -> Vec<TangentContact> {
    vec![TangentContact::Outside, TangentContact::Inside]
}

fn width_modes() -> Vec<WidthMode> {
    vec![
        WidthMode::Centered,
        WidthMode::Free,
        WidthMode::Gap {
            distance: literal(non_negative(2.0)),
            from: Some(PairElement::Second),
        },
        WidthMode::Gap {
            distance: parameter(6),
            from: None,
        },
        WidthMode::Native(text("sldprt:percent")),
    ]
}

fn slot_positions() -> Vec<SlotPosition> {
    vec![
        SlotPosition::Free,
        SlotPosition::Centered,
        SlotPosition::Native(text("sldprt:slot_position")),
    ]
}

fn path_positions() -> Vec<PathPosition> {
    vec![
        PathPosition::Free,
        PathPosition::Distance {
            distance: literal(non_negative(12.0)),
            from_end: Some(literal(true)),
        },
        PathPosition::Distance {
            distance: parameter(7),
            from_end: None,
        },
        PathPosition::Percent {
            fraction: literal(fraction(0.25)),
            from_end: Some(parameter(8)),
        },
    ]
}

fn path_axis_controls() -> Vec<PathAxisControl> {
    vec![
        PathAxisControl::Free,
        PathAxisControl::FollowPath {
            axis: FrameAxis::X,
            flip: Some(literal(false)),
        },
        PathAxisControl::FollowPath {
            axis: FrameAxis::Y,
            flip: None,
        },
    ]
}

fn path_roll_controls() -> Vec<PathRollControl> {
    vec![
        PathRollControl::Free,
        PathRollControl::UpVector {
            vector: Box::new(direction_construction()),
            axis: FrameAxis::Z,
            flip: Some(literal(true)),
        },
        PathRollControl::UpVector {
            vector: Box::new(direction_construction()),
            axis: FrameAxis::Y,
            flip: None,
        },
    ]
}

fn couplings() -> Vec<Coupling> {
    vec![
        Coupling::Gear {
            ratio: Some(literal(real(2.0))),
            radii: [Some(literal(length(10.0))), Some(parameter(9))],
        },
        Coupling::Belt {
            ratio: None,
            radii: [None, Some(literal(length(4.0)))],
        },
        Coupling::RackPinion {
            ratio: Some(literal(real(0.1))),
            pitch_radius: Some(literal(length(8.0))),
        },
        Coupling::RackPinion {
            ratio: None,
            pitch_radius: None,
        },
        Coupling::Screw {
            pitch: Some(literal(length(1.25))),
        },
        Coupling::Screw { pitch: None },
        Coupling::Native(text("usd:coupled_hinge")),
    ]
}

fn joint_kinds() -> Vec<JointKind> {
    vec![
        JointKind::Motion {
            motion: JointMotion::Revolute { axis: FrameAxis::Z },
            axes: Some(Box::new(frame_construction())),
            offset: joint_offsets().remove(0),
            limits: axis_limits(),
            drives: joint_drives(),
        },
        JointKind::Motion {
            motion: JointMotion::Rigid,
            axes: None,
            offset: joint_offsets().remove(1),
            limits: Vec::new(),
            drives: Vec::new(),
        },
        JointKind::Coincident {
            offset: Some(literal(length(0.5))),
        },
        JointKind::Coincident { offset: None },
        JointKind::Concentric,
        JointKind::Insert {
            offset: Some(parameter(10)),
        },
        JointKind::Insert { offset: None },
        JointKind::Distance {
            distance: Some(literal(length(3.0))),
            limits: length_bounds().pop(),
        },
        JointKind::Distance {
            distance: None,
            limits: None,
        },
        JointKind::Angle {
            angle: Some(literal(angle(0.75))),
            limits: angle_bounds().pop(),
        },
        JointKind::Angle {
            angle: None,
            limits: None,
        },
        JointKind::Parallel,
        JointKind::Perpendicular,
        JointKind::Tangent {
            contact: Some(TangentContact::Inside),
        },
        JointKind::Tangent { contact: None },
        JointKind::PointOnAxis {
            distance: Some(literal(length(2.0))),
        },
        JointKind::PointOnAxis { distance: None },
        JointKind::Symmetric {
            plane: plane_construction(),
        },
        JointKind::Width {
            pair: Box::new([
                JointConnector::Plane(plane_construction()),
                JointConnector::Entity(selection(11)),
            ]),
            mode: Some(WidthMode::Free),
        },
        JointKind::Width {
            pair: Box::new([
                JointConnector::Entity(selection(12)),
                JointConnector::Entity(selection(13)),
            ]),
            mode: None,
        },
        JointKind::Slot {
            position: Some(SlotPosition::Centered),
            lock_rotation: Some(literal(true)),
        },
        JointKind::Slot {
            position: None,
            lock_rotation: None,
        },
        JointKind::Path {
            position: Some(PathPosition::Free),
            pitch_yaw: path_axis_controls().pop(),
            roll: path_roll_controls().pop(),
        },
        JointKind::Path {
            position: None,
            pitch_yaw: None,
            roll: None,
        },
        JointKind::Coupling {
            coupling: Coupling::Screw { pitch: None },
            joints: Some(selection(14)),
        },
        JointKind::Coupling {
            coupling: couplings().remove(0),
            joints: None,
        },
        JointKind::Native(text("creo:1450")),
    ]
}

fn joints() -> Vec<Joint> {
    vec![
        Joint {
            connectors: [
                JointConnector::Frame(frame_construction()),
                JointConnector::Frame(frame_construction()),
            ],
            kind: joint_kinds().remove(0),
            alignment: Some(Alignment::Opposed),
            native_options: options(),
            source_settings: settings(),
        },
        Joint {
            connectors: [
                JointConnector::Entity(unread()),
                JointConnector::Point(point_construction()),
            ],
            kind: JointKind::Concentric,
            alignment: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn places() -> Vec<Place> {
    vec![
        Place {
            occurrence: occurrence(1),
            placement: Some(frame_construction()),
            base: Some(PlacementBase::OwnOccurrence),
            constraints: joints(),
            packaged: Some(false),
            native_options: options(),
            source_settings: settings(),
        },
        Place {
            occurrence: occurrence(2),
            placement: None,
            base: None,
            constraints: Vec::new(),
            packaged: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn place_sets() -> Vec<PlaceSet> {
    vec![
        PlaceSet {
            occurrences: vec![occurrence(3), occurrence(4)],
            native_options: options(),
            source_settings: settings(),
        },
        PlaceSet {
            occurrences: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn occurrence_path(keys: &[u32]) -> OccurrencePath {
    OccurrencePath(
        keys.iter()
            .map(|&key| PathStep {
                occurrence: StepRef::Occurrence(occurrence(key)),
                configuration: None,
            })
            .collect(),
    )
}

fn rigid_groups() -> Vec<RigidGroup> {
    vec![
        RigidGroup {
            members: BTreeSet::from([
                occurrence_path(&[]),
                occurrence_path(&[5]),
                occurrence_path(&[5, 6]),
            ]),
            native_options: options(),
            source_settings: settings(),
        },
        RigidGroup {
            members: BTreeSet::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn component_instances() -> Vec<ComponentInstance> {
    vec![
        ComponentInstance {
            index: InstanceIndex(vec![1, 0]),
            occurrences: vec![occurrence(7), occurrence(8)],
        },
        ComponentInstance {
            index: InstanceIndex(vec![-1]),
            occurrences: Vec::new(),
        },
    ]
}

fn component_patterns() -> Vec<ComponentPattern> {
    vec![
        ComponentPattern {
            seeds: selection(15),
            layout: pattern_layouts().remove(0),
            instances: component_instances(),
            native_options: options(),
            source_settings: settings(),
        },
        ComponentPattern {
            seeds: unread(),
            layout: PatternLayout::Chain(chain_layouts().remove(0)),
            instances: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn mirror_reversals() -> Vec<MirrorReversal> {
    vec![
        MirrorReversal::X,
        MirrorReversal::Y,
        MirrorReversal::Z,
        MirrorReversal::All,
    ]
}

fn mirror_pivots() -> Vec<MirrorPivot> {
    vec![
        MirrorPivot::ExtentCentre,
        MirrorPivot::CentreOfMass,
        MirrorPivot::Origin,
    ]
}

fn mirror_orientations() -> Vec<MirrorOrientation> {
    vec![
        MirrorOrientation::SeedAxes {
            reversed: MirrorReversal::Z,
            about: Some(MirrorPivot::ExtentCentre),
        },
        MirrorOrientation::SeedAxes {
            reversed: MirrorReversal::All,
            about: None,
        },
        MirrorOrientation::Native(text("sldprt:mirror_orientation")),
    ]
}

fn mirror_results() -> Vec<MirrorResult> {
    vec![
        MirrorResult {
            occurrence: occurrence(9),
            orientation: mirror_orientations().pop(),
        },
        MirrorResult {
            occurrence: occurrence(10),
            orientation: None,
        },
    ]
}

fn mirror_components() -> Vec<MirrorComponents> {
    vec![
        MirrorComponents {
            seeds: selection(16),
            plane: plane_construction(),
            results: mirror_results(),
            native_options: options(),
            source_settings: settings(),
        },
        MirrorComponents {
            seeds: unread(),
            plane: plane_construction(),
            results: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn context_effects() -> Vec<ContextEffect> {
    vec![
        ContextEffect::Instances,
        ContextEffect::Definitions,
        ContextEffect::Native(text("creo:visibility_level_2")),
    ]
}

fn assembly_features() -> Vec<AssemblyFeature> {
    vec![
        AssemblyFeature {
            participants: selection(17),
            effect: Some(ContextEffect::Instances),
            operation: Box::new(FeatureOperation::Unresolved {
                family: OperationFamily::Hole,
            }),
        },
        AssemblyFeature {
            participants: unread(),
            effect: None,
            operation: Box::new(FeatureOperation::Native {
                kind: crate::codec_name!("creo:assem_cut"),
                native_options: options(),
                source_settings: NativeOptions::new(),
            }),
        },
    ]
}

/// One operation per §6.7 arm.
fn assembly_operations() -> Vec<FeatureOperation> {
    vec![
        FeatureOperation::Place(Box::new(places().remove(0))),
        FeatureOperation::PlaceSet(Box::new(place_sets().remove(0))),
        FeatureOperation::Joint(Box::new(joints().remove(0))),
        FeatureOperation::RigidGroup(Box::new(rigid_groups().remove(0))),
        FeatureOperation::ComponentPattern(Box::new(component_patterns().remove(0))),
        FeatureOperation::MirrorComponents(Box::new(mirror_components().remove(0))),
        FeatureOperation::AssemblyFeature(Box::new(assembly_features().remove(0))),
    ]
}

#[test]
fn every_assembly_operation_round_trips() {
    let mut report = Report::default();
    report.check("AssemblyFeature", assembly_features());
    report.check("FeatureOperation", assembly_operations());
    report.variants("FeatureOperation", &assembly_operations(), 7);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn an_assembly_operation_takes_the_adjacent_tag() {
    let tags: Vec<_> = assembly_operations()
        .iter()
        .map(|operation| to_value(operation)["kind"].clone())
        .collect();
    assert_eq!(
        tags,
        [
            "place",
            "place_set",
            "joint",
            "rigid_group",
            "component_pattern",
            "mirror_components",
            "assembly_feature"
        ]
    );
    let place = places().remove(1);
    assert_eq!(
        to_value(&FeatureOperation::Place(Box::new(place.clone()))),
        json!({"kind": "place", "value": to_value(&place)})
    );
    let feature = assembly_features().remove(0);
    assert_eq!(
        to_value(&feature),
        json!({
            "participants": to_value(&feature.participants),
            "effect": {"kind": "instances"},
            "operation": {"kind": "unresolved", "value": {"family": "hole"}},
        })
    );
    let mut bare = to_value(&assembly_features().remove(1));
    assert!(bare.get("effect").is_none());
    bare["effect"] = json!(null);
    assert!(refuses::<AssemblyFeature>(&bare.to_string()).contains("effect"));
    bare.as_object_mut().unwrap().remove("effect");
    bare.as_object_mut().unwrap().remove("operation");
    refuses::<AssemblyFeature>(&bare.to_string());
}

/// Runs `work` on a thread whose stack is 2 MiB, the default for a spawned
/// thread.
fn on_a_default_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

/// An assembly feature's operation can itself be an assembly feature, and
/// the spec gives that nesting no ceiling; the JSON depth limit bounds it.
/// One level past the deepest nesting the reader admits is refused by that
/// limit, and the deepest reads, prints the same bytes, clones, compares, and
/// drops on a default thread.
#[test]
fn assembly_features_nest_to_the_json_depth_limit_on_a_default_thread() {
    fn nested(depth: usize) -> String {
        let leaf = FeatureOperation::Unresolved {
            family: OperationFamily::Hole,
        };
        let operation = (0..depth).fold(leaf, |operation, _| {
            FeatureOperation::AssemblyFeature(Box::new(AssemblyFeature {
                participants: unread(),
                effect: None,
                operation: Box::new(operation),
            }))
        });
        serde_json::to_string(&operation).unwrap()
    }
    let past = (1..128)
        .find(|&depth| {
            read::from_str::<FeatureOperation>(&nested(depth), &ResourceLimits::desktop()).is_err()
        })
        .expect("the reader refuses some depth below the JSON limit");
    assert!(past > 1, "a single assembly feature is refused");
    let refusal = on_a_default_thread({
        let text = nested(past);
        move || read::from_str::<FeatureOperation>(&text, &ResourceLimits::desktop()).err()
    });
    assert!(
        matches!(&refusal, Some(cadmpeg_core::CodecError::Malformed(message)) if message.contains("recursion limit exceeded")),
        "depth {past}: {refusal:?}"
    );
    let deepest = nested(past - 1);
    let printed = on_a_default_thread({
        let deepest = deepest.clone();
        move || {
            let read: FeatureOperation =
                read::from_str(&deepest, &ResourceLimits::desktop()).unwrap();
            let printed = serde_json::to_string(&read).unwrap();
            let cloned = read.clone();
            assert_eq!(cloned, read);
            drop(cloned);
            drop(read);
            printed
        }
    });
    assert_eq!(printed, deepest);
}

#[test]
fn every_assembly_type_round_trips() {
    let mut report = Report::default();
    report.check("Place", places());
    report.check("PlacementBase", placement_bases());
    report.check("PlaceSet", place_sets());
    report.check("Joint", joints());
    report.check("JointConnector", joint_connectors());
    report.check("Alignment", alignments());
    report.check("JointKind", joint_kinds());
    report.check("JointMotion", joint_motions());
    report.check("JointOffset", joint_offsets());
    report.check("AxisLimits", axis_limits());
    report.check("Bounds<Length>", length_bounds());
    report.check("Bounds<Angle>", angle_bounds());
    report.check("Coupling", couplings());
    report.check("JointDrive", joint_drives());
    report.check("DriveAxis", drive_axes());
    report.check("DriveMode", drive_modes());
    report.check("TangentContact", tangent_contacts());
    report.check("WidthMode", width_modes());
    report.check("SlotPosition", slot_positions());
    report.check("PathPosition", path_positions());
    report.check("PathAxisControl", path_axis_controls());
    report.check("PathRollControl", path_roll_controls());
    report.check("RigidGroup", rigid_groups());
    report.check("ComponentPattern", component_patterns());
    report.check("ComponentInstance", component_instances());
    report.check("MirrorComponents", mirror_components());
    report.check("MirrorResult", mirror_results());
    report.check("MirrorOrientation", mirror_orientations());
    report.check("MirrorReversal", mirror_reversals());
    report.check("MirrorPivot", mirror_pivots());
    report.check("ContextEffect", context_effects());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn every_shared_layout_type_round_trips() {
    let mut report = Report::default();
    report.check("PairElement", pair_elements());
    report.check("PatternLayout", pattern_layouts());
    report.check("ChainLayout", chain_layouts());
    report.check("ChainLinks", chain_linkss());
    report.check("ChainLink", chain_links());
    report.check("LinearRun", linear_runs());
    report.check("PatternRun<Length>", pattern_runs(length(2.0)));
    report.check("PatternRun<Angle>", pattern_runs(angle(0.2)));
    report.check("RunSides<Length>", run_sides(length(2.0)));
    report.check("RunSides<Angle>", run_sides(angle(0.2)));
    report.check("Spacing<Length>", spacings(length(2.0)));
    report.check("Spacing<Angle>", spacings(angle(0.2)));
    report.check("UpToFill<Length>", up_to_fills(length(2.0)));
    report.check("Locus", loci());
    report.check("InstanceOrientation", instance_orientations());
    report.check("PathReference", path_references());
    report.check("Increment", increments());
    report.check("DimensionRun", dimension_runs());
    report.check("DimensionTable", dimension_tables());
    report.check("TableRow", table_rows());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn every_variant_is_sampled() {
    let mut report = Report::default();
    report.variants("PlacementBase", &placement_bases(), 2);
    report.variants("JointConnector", &joint_connectors(), 5);
    report.variants("Alignment", &alignments(), 4);
    report.variants("JointKind", &joint_kinds(), 16);
    report.variants("JointMotion", &joint_motions(), 8);
    report.variants("AxisLimits", &axis_limits(), 2);
    report.variants("Coupling", &couplings(), 5);
    report.variants("DriveAxis", &drive_axes(), 2);
    report.variants("DriveMode", &drive_modes(), 2);
    report.variants("TangentContact", &tangent_contacts(), 2);
    report.variants("WidthMode", &width_modes(), 4);
    report.variants("SlotPosition", &slot_positions(), 3);
    report.variants("PathPosition", &path_positions(), 3);
    report.variants("PathAxisControl", &path_axis_controls(), 2);
    report.variants("PathRollControl", &path_roll_controls(), 2);
    report.variants("MirrorOrientation", &mirror_orientations(), 2);
    report.variants("MirrorReversal", &mirror_reversals(), 4);
    report.variants("MirrorPivot", &mirror_pivots(), 3);
    report.variants("ContextEffect", &context_effects(), 3);
    report.variants("PairElement", &pair_elements(), 2);
    report.variants("PatternLayout", &pattern_layouts(), 12);
    report.variants("ChainLinks", &chain_linkss(), 2);
    report.variants("RunSides", &run_sides(length(1.0)), 4);
    report.variants("Spacing", &spacings(length(1.0)), 7);
    report.variants("UpToFill", &up_to_fills(length(1.0)), 2);
    report.variants("Locus", &loci(), 2);
    report.variants("InstanceOrientation", &instance_orientations(), 5);
    report.variants("PathReference", &path_references(), 2);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn the_tag_forms_of_the_family() {
    assert_eq!(
        to_value(&PlacementBase::OwnOccurrence),
        json!("own_occurrence")
    );
    assert_eq!(
        to_value(&JointKind::Concentric),
        json!({"kind": "concentric"})
    );
    assert_eq!(
        to_value(&JointKind::Coincident { offset: None }),
        json!({"kind": "coincident", "value": {}})
    );
    assert_eq!(
        to_value(&JointMotion::Revolute { axis: FrameAxis::Y }),
        json!({"kind": "revolute", "value": {"axis": "y"}})
    );
    assert_eq!(to_value(&PathPosition::Free), json!({"kind": "free"}));
    assert_eq!(
        to_value(&DriveAxis::Translation { axis: FrameAxis::X }),
        json!({"kind": "translation", "axis": "x"})
    );
    let mut frame = to_value(&frame_construction());
    frame["kind"] = json!("frame");
    assert_eq!(
        to_value(&JointConnector::Frame(frame_construction())),
        frame
    );
    assert_eq!(
        to_value(&MirrorPivot::CentreOfMass),
        json!("centre_of_mass")
    );
    assert_eq!(
        to_value(&Coupling::Belt {
            ratio: None,
            radii: [None, None],
        }),
        json!({"kind": "belt", "value": {"radii": [null, null]}})
    );
    assert_eq!(
        to_value(&PathReference::PlaneNormal),
        json!({"kind": "plane_normal"})
    );
}

#[test]
fn a_unit_arm_of_an_internally_tagged_enum_refuses_unknown_keys() {
    assert_eq!(
        reads::<PathPosition>(r#"{"kind": "free"}"#),
        PathPosition::Free
    );
    refuses::<PathPosition>(r#"{"kind": "free", "extra": 1}"#);
    refuses::<PathAxisControl>(r#"{"kind": "free", "extra": 1}"#);
    refuses::<PathRollControl>(r#"{"kind": "free", "extra": 1}"#);
    refuses::<PathReference>(r#"{"kind": "plane_normal", "extra": 1}"#);
    assert_eq!(reads::<Locus>(r#"{"kind": "centroid"}"#), Locus::Centroid);
    refuses::<Locus>(r#"{"kind": "centroid", "extra": 1}"#);
    refuses::<Locus>(r#"{"kind": "centroid", "value": null}"#);
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 1] =
        [("ContextEffect", unit_variants::reads::<ContextEffect>)];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/assembly.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

#[test]
fn an_omitted_optional_field_refuses_null_and_unknown_fields_are_refused() {
    let bare = places().remove(1);
    let wire = to_value(&bare);
    assert_eq!(
        wire,
        json!({
            "occurrence": "test:design:occurrence#2",
            "constraints": [],
            "native_options": {},
            "source_settings": {},
        })
    );
    assert_eq!(reads::<Place>(&wire.to_string()), bare);
    let mut null = wire.clone();
    null["packaged"] = json!(null);
    assert!(refuses::<Place>(&null.to_string()).contains("packaged"));
    let mut extra = wire;
    extra["extra"] = json!(1);
    refuses::<Place>(&extra.to_string());
    refuses::<JointOffset>(r#"{"x": null}"#);
    refuses::<Bounds<Length>>(r#"{"min": null}"#);
    refuses::<JointKind>(r#"{"kind": "tangent", "value": {"contact": null}}"#);
}

#[test]
fn rigid_group_members_are_a_sorted_set_that_refuses_repeats() {
    let group = rigid_groups().remove(0);
    let mut wire = to_value(&group);
    let members = wire["members"].as_array().unwrap().clone();
    assert_eq!(members.len(), 3);
    assert_eq!(members[0], json!([]));
    wire["members"] = json!([members[1], members[1]]);
    assert!(refuses::<RigidGroup>(&wire.to_string()).contains("twice"));
}

#[test]
fn a_native_option_key_stated_twice_is_refused() {
    let wire = serde_json::to_string(&places().remove(0)).unwrap();
    let entry = "\"f3d:item_kind\":{\"kind\":\"text\",\"value\":{\"kind\":\"literal\",\"value\":\"insert\"}}";
    let twice = wire.replacen(entry, &format!("{entry},{entry}"), 1);
    assert_ne!(twice, wire);
    refuses::<Place>(&twice);
}

#[cfg(feature = "schema")]
fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
    let schema = T::json_schema(generator);
    assert!(schema.as_object().is_some(), "{}", T::schema_name());
}

#[cfg(feature = "schema")]
#[test]
fn every_assembly_and_shared_layout_type_has_a_schema() {
    let mut generator = SchemaGenerator::default();
    generates::<Place>(&mut generator);
    generates::<PlacementBase>(&mut generator);
    generates::<PlaceSet>(&mut generator);
    generates::<Joint>(&mut generator);
    generates::<JointConnector>(&mut generator);
    generates::<Alignment>(&mut generator);
    generates::<JointKind>(&mut generator);
    generates::<JointMotion>(&mut generator);
    generates::<JointOffset>(&mut generator);
    generates::<AxisLimits>(&mut generator);
    generates::<Bounds<Length>>(&mut generator);
    generates::<Bounds<Angle>>(&mut generator);
    generates::<Coupling>(&mut generator);
    generates::<JointDrive>(&mut generator);
    generates::<DriveAxis>(&mut generator);
    generates::<DriveMode>(&mut generator);
    generates::<TangentContact>(&mut generator);
    generates::<WidthMode>(&mut generator);
    generates::<SlotPosition>(&mut generator);
    generates::<PathPosition>(&mut generator);
    generates::<PathAxisControl>(&mut generator);
    generates::<PathRollControl>(&mut generator);
    generates::<RigidGroup>(&mut generator);
    generates::<ComponentPattern>(&mut generator);
    generates::<ComponentInstance>(&mut generator);
    generates::<MirrorComponents>(&mut generator);
    generates::<MirrorResult>(&mut generator);
    generates::<MirrorOrientation>(&mut generator);
    generates::<MirrorReversal>(&mut generator);
    generates::<MirrorPivot>(&mut generator);
    generates::<AssemblyFeature>(&mut generator);
    generates::<ContextEffect>(&mut generator);
    generates::<PairElement>(&mut generator);
    generates::<PatternLayout>(&mut generator);
    generates::<ChainLayout>(&mut generator);
    generates::<ChainLinks>(&mut generator);
    generates::<ChainLink>(&mut generator);
    generates::<LinearRun>(&mut generator);
    generates::<PatternRun<Length>>(&mut generator);
    generates::<PatternRun<Angle>>(&mut generator);
    generates::<RunSides<Length>>(&mut generator);
    generates::<Spacing<Angle>>(&mut generator);
    generates::<UpToFill<Length>>(&mut generator);
    generates::<Locus>(&mut generator);
    generates::<InstanceOrientation>(&mut generator);
    generates::<PathReference>(&mut generator);
    generates::<Increment>(&mut generator);
    generates::<DimensionRun>(&mut generator);
    generates::<DimensionTable>(&mut generator);
    generates::<TableRow>(&mut generator);
}
