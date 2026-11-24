// SPDX-License-Identifier: Apache-2.0
//! Every swept and loft type, written, read back strictly, and written again;
//! the two texts are equal. The stated wire forms of these types, and their
//! schemas.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::num::NonZeroU32;
use std::path::Path;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use super::*;
use crate::design::construction::{
    AxisValue, ConstructionInput, ConstructionKind, FrameValue, InputReference, InputRole,
    PlaneValue, PostStep, Relation, Subject,
};
use crate::design::options::{BodyForm, BooleanOp, LimitOffset, Support, SymmetricMeasure};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection,
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

fn lit<T>(value: T) -> Dimension<T> {
    Dimension::Literal(value)
}

fn bound<T>(key: u32) -> Dimension<T> {
    Dimension::Parameter(id::<ParameterId>("parameter", key))
}

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

fn nonnegative(value: f64) -> NonNegativeLength {
    NonNegativeLength::new(value).unwrap()
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

fn selection<O: MemberOrder>(key: u32) -> Selection<O> {
    let pick = Picked {
        target: PickTarget::SketchEntity(id("sketch-entity", key)),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(vec![pick]).unwrap()),
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
    let inputs = if resolved.is_some() {
        vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(9))),
            value: None,
            reversed: None,
        }]
    } else {
        Vec::new()
    };
    Construction {
        inputs,
        after: vec![PostStep::Flip(lit(true))],
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

fn point_construction() -> Construction<kinds::Point> {
    construction(Some(point(1.0, 2.0, 3.0)))
}

fn direction() -> Construction<kinds::Direction> {
    construction(Some(UnitVector3::X_AXIS))
}

fn axis() -> Construction<kinds::Axis> {
    construction(Some(AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }))
}

fn plane() -> Construction<kinds::Plane> {
    construction(Some(PlaneValue {
        origin: point(0.0, 0.0, 5.0),
        normal: UnitVector3::Z_AXIS,
        x_axis: UnitVector3::X_AXIS,
    }))
}

fn frame() -> Construction<kinds::Frame> {
    construction(Some(FrameValue {
        origin: point(1.0, 2.0, 3.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }))
}

fn native_options() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("sldprt:translate_surface"),
        crate::design::options::NativeOption::Bool(lit(true)),
    )])
}

fn solid() -> SolidOutput {
    SolidOutput {
        body: Some(BodyForm::Solid),
        boolean: Some(Boolean::Apply {
            op: BooleanOp::Join,
            targets: None,
        }),
    }
}

fn unstated_output() -> SolidOutput {
    SolidOutput {
        body: None,
        boolean: None,
    }
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
        assert!(!samples.is_empty(), "{name} has no sample");
        for sample in samples {
            if let Err(error) = round_trip(&sample) {
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }
}

fn extrude_directions() -> Vec<ExtrudeDirection> {
    vec![
        ExtrudeDirection::ProfileNormal {
            reversed: Some(lit(true)),
        },
        ExtrudeDirection::ProfileNormal { reversed: None },
        ExtrudeDirection::Along {
            direction: Box::new(direction()),
            lengths_along_normal: Some(lit(false)),
        },
        ExtrudeDirection::Along {
            direction: Box::new(construction(None)),
            lengths_along_normal: None,
        },
    ]
}

fn extrude_origins() -> Vec<ExtrudeOrigin> {
    vec![
        ExtrudeOrigin::ProfilePlane,
        ExtrudeOrigin::Offset(lit(length(-2.0))),
        ExtrudeOrigin::Plane(plane()),
        ExtrudeOrigin::Point(point_construction()),
        ExtrudeOrigin::Faces {
            faces: selection(1),
            offset: Some(bound(1)),
        },
        ExtrudeOrigin::Faces {
            faces: unread(),
            offset: None,
        },
    ]
}

fn hole_drafts() -> Vec<HoleDraft> {
    vec![
        HoleDraft::WithRegion,
        HoleDraft::WithOuter,
        HoleDraft::Angles {
            end: lit(angle(0.1)),
            start: Some(lit(angle(0.0))),
        },
        HoleDraft::Angles {
            end: Dimension::Negated(id("parameter", 2)),
            start: None,
        },
    ]
}

fn extrude_drafts() -> Vec<ExtrudeDraft> {
    vec![
        ExtrudeDraft {
            end: Some(lit(angle(0.05))),
            start: Some(lit(angle(-0.05))),
            neutral: Some(DraftNeutral::Start),
            holes: Some(HoleDraft::WithOuter),
        },
        ExtrudeDraft {
            end: None,
            start: None,
            neutral: None,
            holes: None,
        },
    ]
}

fn offset_joins() -> Vec<OffsetJoin> {
    vec![
        OffsetJoin::Round,
        OffsetJoin::Sharp,
        OffsetJoin::Native(text("freecad:join:3")),
    ]
}

fn profile_fits() -> Vec<ProfileFit> {
    vec![
        ProfileFit {
            outer: Some(lit(length(0.5))),
            inner: Some(lit(length(-0.25))),
            outer_join: Some(OffsetJoin::Round),
            inner_join: Some(OffsetJoin::Sharp),
        },
        ProfileFit {
            outer: None,
            inner: None,
            outer_join: None,
            inner_join: None,
        },
    ]
}

fn length_spans() -> Vec<Span<Length>> {
    vec![
        Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::ToGeometry {
                target: selection(2),
                offset: Some(LimitOffset::Distance(lit(length(1.0)))),
                extend: None,
                far: None,
            }),
        },
        Span::Symmetric {
            amount: lit(length(10.0)),
            measure: Some(SymmetricMeasure::Total),
        },
    ]
}

fn extrudes() -> Vec<Extrude> {
    vec![
        Extrude {
            profile: selection(1),
            loops: Some(LoopNesting::Alternate),
            direction: extrude_directions().into_iter().next(),
            origin: Some(ExtrudeOrigin::Offset(lit(length(3.0)))),
            span: length_spans().into_iter().next(),
            draft: extrude_drafts().into_iter().next(),
            fit: profile_fits().into_iter().next(),
            outside: Some(lit(true)),
            sheet_cut: Some(SheetCutNormal::Both),
            output: solid(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Extrude {
            profile: unread(),
            loops: None,
            direction: None,
            origin: None,
            span: None,
            draft: None,
            fit: None,
            outside: None,
            sheet_cut: None,
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn revolves() -> Vec<Revolve> {
    vec![
        Revolve {
            profile: selection(1),
            loops: Some(LoopNesting::OneLevel),
            axis: Some(axis()),
            span: Some(Span::Between {
                start: Box::new(Limit::Amount(Dimension::Negated(id("parameter", 1)))),
                end: Box::new(Limit::ThroughAll),
            }),
            fit: profile_fits().pop(),
            output: solid(),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
        },
        Revolve {
            profile: unread(),
            loops: None,
            axis: None,
            span: None,
            fit: None,
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn corner_transitions() -> Vec<CornerTransition> {
    vec![
        CornerTransition::Mitre,
        CornerTransition::Round,
        CornerTransition::Open,
        CornerTransition::Carried,
        CornerTransition::Native(text("freecad:transition:9")),
    ]
}

fn section_shapes() -> Vec<SectionShape> {
    vec![
        SectionShape::Circle {
            diameter: lit(positive(6.0)),
            inner_diameter: Some(lit(positive(4.0))),
        },
        SectionShape::Circle {
            diameter: bound(1),
            inner_diameter: None,
        },
        SectionShape::Square {
            size: lit(positive(2.0)),
        },
        SectionShape::Triangle {
            size: lit(positive(3.0)),
            apex: Some(Apex::Inward),
        },
        SectionShape::Triangle {
            size: lit(positive(3.0)),
            apex: None,
        },
        SectionShape::Native(text("f3d:pipe-section")),
    ]
}

fn sweep_sections() -> Vec<SweepSection> {
    vec![
        SweepSection::Profile(selection(1)),
        SweepSection::Generated {
            shape: SectionShape::Square {
                size: lit(positive(2.0)),
            },
            turn: Some(SectionTurn::ZStart),
        },
        SweepSection::Generated {
            shape: section_shapes().remove(0),
            turn: None,
        },
    ]
}

fn sweep_orientations() -> Vec<SweepOrientation> {
    vec![
        SweepOrientation::FollowPath,
        SweepOrientation::MinimumTwist,
        SweepOrientation::Frenet,
        SweepOrientation::Fixed,
        SweepOrientation::Guides(GuideFrame::PathToFirst),
        SweepOrientation::Guides(GuideFrame::FirstToSecond),
        SweepOrientation::Direction(direction()),
        SweepOrientation::Surface(selection(3)),
        SweepOrientation::Point(point_construction()),
        SweepOrientation::Curve {
            path: selection(4),
            tangent: Some(lit(true)),
            curvilinear: Some(lit(false)),
        },
        SweepOrientation::Curve {
            path: unread(),
            tangent: None,
            curvilinear: None,
        },
        SweepOrientation::Native(text("sldprt:sweep_orientation")),
    ]
}

fn angle_laws() -> Vec<Law<Angle>> {
    vec![
        Law::Constant(lit(angle(0.5))),
        Law::Linear {
            start: lit(angle(0.0)),
            end: lit(angle(1.0)),
        },
    ]
}

fn twists() -> Vec<Twist> {
    let mut out = vec![Twist::Angle(lit(angle(3.0)))];
    out.extend(angle_laws().into_iter().map(Twist::Law));
    out
}

fn scaling_laws() -> Vec<ScalingLaw> {
    vec![
        ScalingLaw::Taper(lit(angle(0.1))),
        ScalingLaw::Law(Law::Linear {
            start: lit(FiniteReal::new(1.0).unwrap()),
            end: lit(FiniteReal::new(0.5).unwrap()),
        }),
        ScalingLaw::Area,
        ScalingLaw::Perimeter,
        ScalingLaw::Point(point_construction()),
        ScalingLaw::Curve(selection(5)),
        ScalingLaw::Native(text("nx:scaling:blend")),
    ]
}

fn sweep_controls() -> Vec<SweepControls> {
    vec![
        SweepControls {
            orientation: Some(SweepOrientation::FollowPath),
            twist: Some(Twist::Angle(lit(angle(1.0)))),
            scaling: Some(ScalingLaw::Area),
        },
        SweepControls {
            orientation: None,
            twist: None,
            scaling: None,
        },
    ]
}

fn path_extents() -> Vec<PathExtent> {
    vec![
        PathExtent {
            along: Some(lit(fraction(0.75))),
            against: Some(lit(fraction(0.0))),
        },
        PathExtent {
            along: None,
            against: None,
        },
    ]
}

fn sweep_guides() -> Vec<SweepGuide> {
    vec![
        SweepGuide {
            path: selection(6),
            extent: path_extents().into_iter().next(),
        },
        SweepGuide {
            path: unread(),
            extent: None,
        },
    ]
}

fn profile_positions() -> Vec<ProfilePosition> {
    vec![
        ProfilePosition {
            origin: Some(point_construction()),
            direction: Some(direction()),
            angle: Some(lit(angle(0.25))),
            swap_axes: Some(lit(true)),
        },
        ProfilePosition {
            origin: None,
            direction: None,
            angle: None,
            swap_axes: None,
        },
    ]
}

fn profile_placements() -> Vec<ProfilePlacement> {
    let mut out = vec![ProfilePlacement::InPlace];
    out.extend(
        profile_positions()
            .into_iter()
            .map(|position| ProfilePlacement::Positioned(Box::new(position))),
    );
    out
}

fn spine_limits() -> Vec<SpineLimits> {
    vec![
        SpineLimits {
            start: Some(selection(7)),
            end: Some(unread()),
        },
        SpineLimits {
            start: None,
            end: None,
        },
    ]
}

fn sweeps() -> Vec<Sweep> {
    vec![
        Sweep {
            section: SweepSection::Profile(selection(1)),
            path: selection(2),
            controls: sweep_controls().remove(0),
            guides: sweep_guides(),
            extent: path_extents().into_iter().next(),
            placement: Some(ProfilePlacement::InPlace),
            limits: spine_limits().remove(0),
            transition: Some(CornerTransition::Round),
            output: solid(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Sweep {
            section: sweep_sections().remove(1),
            path: unread(),
            controls: sweep_controls().remove(1),
            guides: Vec::new(),
            extent: None,
            placement: None,
            limits: spine_limits().remove(1),
            transition: None,
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn helical_sections() -> Vec<HelicalSection> {
    vec![
        HelicalSection::Profile(selection(1)),
        HelicalSection::Generated {
            shape: SectionShape::Triangle {
                size: lit(positive(1.0)),
                apex: Some(Apex::Outward),
            },
            placement: Some(SectionPlacement::Center),
        },
        HelicalSection::Generated {
            shape: SectionShape::Square {
                size: lit(positive(1.0)),
            },
            placement: None,
        },
    ]
}

fn helix_starts() -> Vec<HelixStart> {
    vec![
        HelixStart::Radius {
            radius: RadialDimension::Diameter(lit(positive(20.0))),
            angle: Some(lit(angle(0.5))),
        },
        HelixStart::Radius {
            radius: RadialDimension::Radius(bound(1)),
            angle: None,
        },
        HelixStart::Point(point_construction()),
        HelixStart::Circle(selection(3)),
    ]
}

fn helix_laws() -> Vec<HelixLaw> {
    vec![
        HelixLaw::PitchHeight {
            pitch: lit(positive(2.0)),
            height: lit(positive(10.0)),
        },
        HelixLaw::PitchTurns {
            pitch: lit(positive(2.0)),
            turns: lit(positive_real(5.0)),
        },
        HelixLaw::HeightTurns {
            height: bound(1),
            turns: lit(positive_real(2.5)),
        },
        HelixLaw::Native(text("inventor:coil_type:spiral")),
    ]
}

fn helix_tapers() -> Vec<HelixTaper> {
    vec![
        HelixTaper::Angle(lit(angle(0.1))),
        HelixTaper::Growth(lit(length(-0.5))),
        HelixTaper::Radius(Law::Linear {
            start: lit(length(5.0)),
            end: lit(length(8.0)),
        }),
    ]
}

fn spiral_laws() -> Vec<SpiralLaw> {
    vec![
        SpiralLaw::PitchTurns {
            pitch: lit(NonZeroLength::new(-1.5).unwrap()),
            turns: lit(positive_real(3.0)),
        },
        SpiralLaw::EndRadiusTurns {
            end: lit(positive(12.0)),
            turns: lit(positive_real(4.0)),
        },
        SpiralLaw::EndRadiusPitch {
            end: lit(positive(12.0)),
            pitch: lit(NonZeroLength::new(2.0).unwrap()),
        },
    ]
}

fn helix_shapes() -> Vec<HelixShape> {
    vec![
        HelixShape::Helix {
            law: helix_laws().into_iter().next(),
            taper: helix_tapers().into_iter().next(),
        },
        HelixShape::Helix {
            law: None,
            taper: None,
        },
        HelixShape::Spiral {
            law: spiral_laws().into_iter().next(),
        },
        HelixShape::Spiral { law: None },
        HelixShape::Native(text("catia:helix:law")),
    ]
}

fn helices() -> Vec<Helix> {
    vec![
        Helix {
            placement: frame(),
            start: helix_starts().into_iter().next(),
            shape: helix_shapes().remove(0),
            hand: Some(lit(Hand::Left)),
        },
        Helix {
            placement: construction(None),
            start: None,
            shape: HelixShape::Spiral { law: None },
            hand: None,
        },
    ]
}

fn helical_sweeps() -> Vec<HelicalSweep> {
    vec![
        HelicalSweep {
            section: helical_sections().remove(1),
            helix: helices().remove(0),
            output: solid(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        HelicalSweep {
            section: helical_sections().remove(0),
            helix: helices().remove(1),
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn rib_growths() -> Vec<RibGrowth> {
    vec![
        RibGrowth::InPlane,
        RibGrowth::Normal,
        RibGrowth::Along(direction()),
        RibGrowth::Native(text("creo:rib:growth")),
    ]
}

fn ribs() -> Vec<Rib> {
    vec![
        Rib {
            profile: selection(1),
            thickness: Some(Thickness::Symmetric(lit(positive(2.0)))),
            growth: Some(RibGrowth::Normal),
            flipped: Some(lit(true)),
            extent: Some(Limit::Next),
            draft: Some(lit(angle(0.02))),
            thickness_at: Some(RibEnd::Tip),
            root_fillet: Some(lit(positive(0.5))),
            extend_profile: Some(lit(false)),
            boolean: Some(Boolean::NewBody),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Rib {
            profile: unread(),
            thickness: None,
            growth: None,
            flipped: None,
            extent: None,
            draft: None,
            thickness_at: None,
            root_fillet: None,
            extend_profile: None,
            boolean: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn primitive_shapes() -> Vec<PrimitiveShape> {
    let latitudes = Some([lit(angle(-0.5)), lit(angle(0.5))]);
    vec![
        PrimitiveShape::Box {
            size: [lit(positive(1.0)), lit(positive(2.0)), bound(1)],
        },
        PrimitiveShape::Cylinder {
            radius: RadialDimension::Radius(lit(positive(2.0))),
            height: lit(positive(5.0)),
            angle: Some(lit(angle(3.0))),
        },
        PrimitiveShape::Cylinder {
            radius: RadialDimension::Diameter(lit(positive(4.0))),
            height: lit(positive(5.0)),
            angle: None,
        },
        PrimitiveShape::Cone {
            radii: [
                RadialDimension::Radius(lit(nonnegative(2.0))),
                RadialDimension::Radius(lit(nonnegative(0.0))),
            ],
            height: lit(positive(3.0)),
            angle: Some(lit(angle(1.0))),
        },
        PrimitiveShape::Sphere {
            radius: RadialDimension::Radius(lit(positive(5.0))),
            latitudes: latitudes.clone(),
            longitude: Some(lit(angle(6.0))),
        },
        PrimitiveShape::Sphere {
            radius: RadialDimension::Diameter(lit(positive(10.0))),
            latitudes: None,
            longitude: None,
        },
        PrimitiveShape::Torus {
            radii: [
                RadialDimension::Radius(lit(positive(10.0))),
                RadialDimension::Radius(lit(positive(2.0))),
            ],
            latitudes: latitudes.clone(),
            longitude: None,
        },
        PrimitiveShape::Ellipsoid {
            radii: [lit(positive(3.0)), lit(positive(2.0)), lit(positive(1.0))],
            latitudes,
            longitude: Some(lit(angle(2.0))),
        },
        PrimitiveShape::Prism {
            sides: lit(6),
            circumradius: lit(positive(2.0)),
            height: lit(positive(10.0)),
        },
        PrimitiveShape::Wedge {
            y: [lit(length(0.0)), lit(length(10.0))],
            base: [
                lit(length(0.0)),
                lit(length(10.0)),
                lit(length(0.0)),
                lit(length(10.0)),
            ],
            top: [
                lit(length(2.0)),
                lit(length(8.0)),
                lit(length(2.0)),
                lit(length(8.0)),
            ],
        },
        PrimitiveShape::Plane {
            size: [lit(positive(10.0)), lit(positive(20.0))],
        },
        PrimitiveShape::Native(text("nx:block_method")),
    ]
}

fn primitives() -> Vec<Primitive> {
    vec![
        Primitive {
            shape: primitive_shapes().remove(0),
            placement: Some(frame()),
            output: solid(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Primitive {
            shape: PrimitiveShape::Native(text("freecad:helix")),
            placement: None,
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn section_forms() -> Vec<SectionForm> {
    vec![
        SectionForm::Profile(selection(1)),
        SectionForm::Placed {
            profile: selection(2),
            placement: Box::new(frame()),
        },
    ]
}

fn seams() -> Vec<Seam> {
    vec![
        Seam::Vertex(selection(3)),
        Seam::Parameter(FiniteReal::new(0.25).unwrap()),
    ]
}

fn sections() -> Vec<Section> {
    vec![
        Section {
            form: section_forms().remove(1),
            flip: Some(lit(true)),
            seam: seams().into_iter().next(),
            condition: Some(SectionCondition::Tangent {
                support: Some(Support::Computed),
                weight: Some(lit(positive_real(1.5))),
                flip: None,
            }),
            trim_support: Some(lit(false)),
            spine_point: Some(point_construction()),
        },
        Section {
            form: SectionForm::Profile(unread()),
            flip: None,
            seam: None,
            condition: None,
            trim_support: None,
            spine_point: None,
        },
    ]
}

fn point_conditions() -> Vec<PointCondition> {
    vec![
        PointCondition::Sharp,
        PointCondition::Tangent {
            weight: Some(lit(positive_real(2.0))),
        },
        PointCondition::Tangent { weight: None },
        PointCondition::TangentToPlane {
            plane: Box::new(plane()),
            weight: Some(lit(positive_real(0.5))),
        },
        PointCondition::TangentToPlane {
            plane: Box::new(construction(None)),
            weight: None,
        },
    ]
}

fn point_sections() -> Vec<PointSection> {
    vec![
        PointSection {
            point: point_construction(),
            condition: Some(PointCondition::Sharp),
        },
        PointSection {
            point: construction(None),
            condition: None,
        },
    ]
}

fn guide_influences() -> Vec<GuideInfluence> {
    vec![
        GuideInfluence::NextGuide,
        GuideInfluence::NextSharp,
        GuideInfluence::NextEdge,
        GuideInfluence::Global,
    ]
}

fn guides() -> Vec<Guide> {
    vec![
        Guide {
            path: selection(4),
            tangency: Some(SectionCondition::Default),
            influence: Some(GuideInfluence::NextSharp),
        },
        Guide {
            path: unread(),
            tangency: None,
            influence: None,
        },
    ]
}

fn centerline_ats() -> Vec<CenterlineAt> {
    vec![
        CenterlineAt::Fraction(lit(fraction(0.5))),
        CenterlineAt::Distance(lit(nonnegative(12.0))),
    ]
}

fn area_stations() -> Vec<AreaStation> {
    vec![
        AreaStation {
            at: CenterlineAt::Fraction(lit(fraction(0.5))),
            area: lit(PositiveArea::new(25.0).unwrap()),
            driving: true,
        },
        AreaStation {
            at: CenterlineAt::Distance(bound(1)),
            area: bound(2),
            driving: false,
        },
    ]
}

fn spines() -> Vec<Spine> {
    vec![
        Spine::Path {
            path: selection(5),
            controls: Box::new(sweep_controls().remove(0)),
        },
        Spine::Centerline {
            path: selection(6),
            sections: Some(lit(4)),
            stations: area_stations(),
        },
        Spine::Centerline {
            path: unread(),
            sections: None,
            stations: Vec::new(),
        },
        Spine::Computed,
    ]
}

fn coupling_modes() -> Vec<CouplingMode> {
    vec![
        CouplingMode::Ratio,
        CouplingMode::Tangency,
        CouplingMode::TangencyThenCurvature,
        CouplingMode::Vertices,
        CouplingMode::ArcLength,
        CouplingMode::Parameter,
        CouplingMode::BySegment,
        CouplingMode::Native(text("nx:alignment:3")),
    ]
}

fn connectors() -> Vec<Connector> {
    vec![Connector {
        points: selection(7),
    }]
}

fn correspondences() -> Vec<Correspondence> {
    vec![
        Correspondence {
            mode: Some(CouplingMode::Parameter),
            connectors: connectors(),
        },
        Correspondence {
            mode: None,
            connectors: Vec::new(),
        },
    ]
}

fn loft_styles() -> Vec<LoftStyle> {
    vec![
        LoftStyle::Loose,
        LoftStyle::Uniform,
        LoftStyle::ChordLength,
        LoftStyle::SqrtChordLength,
        LoftStyle::Native(text("rhino:loft_style:4")),
    ]
}

fn rebuilds() -> Vec<Rebuild> {
    vec![
        Rebuild::Points(12),
        Rebuild::Refit {
            tolerance: Some(length(0.01)),
        },
        Rebuild::Refit { tolerance: None },
        Rebuild::Degree(3),
    ]
}

fn loft_smoothings() -> Vec<LoftSmoothing> {
    vec![
        LoftSmoothing {
            angle: Some(lit(angle(0.01))),
            deviation: Some(lit(nonnegative(0.001))),
        },
        LoftSmoothing {
            angle: None,
            deviation: None,
        },
    ]
}

fn interpolations() -> Vec<Interpolation> {
    vec![
        Interpolation::Linear,
        Interpolation::Smooth,
        Interpolation::Native(text("nx:interpolation:3")),
    ]
}

fn loft_shapes() -> Vec<LoftShape> {
    vec![
        LoftShape {
            style: Some(LoftStyle::ChordLength),
            max_degree: Some(lit(NonZeroU32::new(5).unwrap())),
            rebuild: Some(Rebuild::Points(8)),
            ruled: Some(lit(false)),
            closed: Some(lit(true)),
            relimit: Some(lit(true)),
            merge_tangent_faces: Some(lit(true)),
            smoothing: loft_smoothings().into_iter().next(),
            section_law: Some(Interpolation::Smooth),
        },
        LoftShape {
            style: None,
            max_degree: None,
            rebuild: None,
            ruled: None,
            closed: None,
            relimit: None,
            merge_tangent_faces: None,
            smoothing: None,
            section_law: None,
        },
    ]
}

fn lofts() -> Vec<Loft> {
    vec![
        Loft {
            start: point_sections().into_iter().next(),
            sections: sections(),
            end: Some(PointSection {
                point: point_construction(),
                condition: Some(PointCondition::Tangent { weight: None }),
            }),
            cross: vec![sections().remove(1)],
            spine: Some(Spine::Computed),
            guides: guides(),
            correspondence: correspondences().into_iter().next(),
            shape: loft_shapes().remove(0),
            output: solid(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Loft {
            start: None,
            sections: Vec::new(),
            end: None,
            cross: Vec::new(),
            spine: None,
            guides: Vec::new(),
            correspondence: None,
            shape: loft_shapes().remove(1),
            output: unstated_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

#[test]
fn every_swept_type_round_trips() {
    let mut report = Report::default();
    report.check("Extrude", extrudes());
    report.check("ExtrudeDirection", extrude_directions());
    report.check("ExtrudeOrigin", extrude_origins());
    report.check("ExtrudeDraft", extrude_drafts());
    report.check(
        "DraftNeutral",
        vec![DraftNeutral::Origin, DraftNeutral::Start],
    );
    report.check("HoleDraft", hole_drafts());
    report.check("ProfileFit", profile_fits());
    report.check("Revolve", revolves());
    report.check("Sweep", sweeps());
    report.check("CornerTransition", corner_transitions());
    report.check("SweepSection", sweep_sections());
    report.check(
        "SectionTurn",
        vec![SectionTurn::PathPlane, SectionTurn::ZStart],
    );
    report.check("SectionShape", section_shapes());
    report.check("Apex", vec![Apex::Outward, Apex::Inward]);
    report.check("SweepControls", sweep_controls());
    report.check("SweepOrientation", sweep_orientations());
    report.check(
        "GuideFrame",
        vec![GuideFrame::PathToFirst, GuideFrame::FirstToSecond],
    );
    report.check("Twist", twists());
    report.check("ScalingLaw", scaling_laws());
    report.check("SweepGuide", sweep_guides());
    report.check("PathExtent", path_extents());
    report.check("ProfilePlacement", profile_placements());
    report.check("ProfilePosition", profile_positions());
    report.check("SpineLimits", spine_limits());
    report.check("HelicalSweep", helical_sweeps());
    report.check("HelicalSection", helical_sections());
    report.check(
        "SectionPlacement",
        vec![
            SectionPlacement::Inside,
            SectionPlacement::Center,
            SectionPlacement::Outside,
        ],
    );
    report.check("Helix", helices());
    report.check("HelixStart", helix_starts());
    report.check("HelixShape", helix_shapes());
    report.check("HelixLaw", helix_laws());
    report.check("HelixTaper", helix_tapers());
    report.check("SpiralLaw", spiral_laws());
    report.check("Rib", ribs());
    report.check("RibGrowth", rib_growths());
    report.check("RibEnd", vec![RibEnd::Root, RibEnd::Tip]);
    report.check("Primitive", primitives());
    report.check("PrimitiveShape", primitive_shapes());
    report.check("Loft", lofts());
    report.check("Section", sections());
    report.check("SectionForm", section_forms());
    report.check("Seam", seams());
    report.check("PointSection", point_sections());
    report.check("PointCondition", point_conditions());
    report.check("Guide", guides());
    report.check("GuideInfluence", guide_influences());
    report.check("Spine", spines());
    report.check("AreaStation", area_stations());
    report.check("CenterlineAt", centerline_ats());
    report.check("Correspondence", correspondences());
    report.check("CouplingMode", coupling_modes());
    report.check("Connector", connectors());
    report.check("LoftShape", loft_shapes());
    report.check("LoftSmoothing", loft_smoothings());
    report.check("LoftStyle", loft_styles());
    report.check("Rebuild", rebuilds());
    report.check("Interpolation", interpolations());
    report.check("OffsetJoin", offset_joins());
    report.check(
        "SheetCutNormal",
        vec![
            SheetCutNormal::Defining,
            SheetCutNormal::Opposite,
            SheetCutNormal::Both,
        ],
    );
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// §5.10: each swept operation is the `FeatureOperation` arm its family
/// names, holding the operation as its value.
#[test]
fn every_swept_operation_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    let failures = [
        arm_failures(Family::Extrude, extrudes(), Op::Extrude),
        arm_failures(Family::Revolve, revolves(), Op::Revolve),
        arm_failures(Family::Sweep, sweeps(), Op::Sweep),
        arm_failures(Family::HelicalSweep, helical_sweeps(), Op::HelicalSweep),
        arm_failures(Family::Rib, ribs(), Op::Rib),
        arm_failures(Family::Primitive, primitives(), Op::Primitive),
        arm_failures(Family::Loft, lofts(), Op::Loft),
    ]
    .concat();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §2.5: every `Option` field is omitted for `None` and refuses `null`, and
/// every list, map, and set field is always written and refuses omission.
#[test]
fn an_option_field_is_omitted_and_a_list_field_is_always_written() {
    let empty = Correspondence {
        mode: None,
        connectors: Vec::new(),
    };
    assert_eq!(
        serde_json::to_string(&empty).unwrap(),
        r#"{"connectors":[]}"#
    );
    refuses::<Correspondence>("{}");
    refuses::<Correspondence>(r#"{"connectors": [], "mode": null}"#);
    refuses::<Correspondence>(r#"{"connectors": [], "colour": 1}"#);

    let rib = to_value(&ribs().remove(1));
    let keys = rib.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys, ["native_options", "profile", "source_settings"]);
    let mut without_settings = rib.clone();
    without_settings
        .as_object_mut()
        .unwrap()
        .remove("source_settings");
    refuses::<Rib>(&without_settings.to_string());
}

/// §2.5: a unit-only enum writes a string; an enum whose data variants all
/// hold structs is internally tagged by `kind`; every other enum is
/// adjacently tagged `kind`/`value`.
#[test]
fn each_enum_has_its_tag_form() {
    assert_eq!(to_value(&GuideInfluence::NextSharp), json!("next_sharp"));
    assert_eq!(to_value(&SheetCutNormal::Defining), json!("defining"));
    assert_eq!(
        to_value(&ExtrudeDirection::ProfileNormal {
            reversed: Some(lit(true))
        }),
        json!({"kind": "profile_normal", "reversed": {"kind": "literal", "value": true}})
    );
    assert_eq!(
        to_value(&HoleDraft::WithRegion),
        json!({"kind": "with_region"})
    );
    assert_eq!(to_value(&Spine::Computed), json!({"kind": "computed"}));
    assert_eq!(
        to_value(&SpiralLaw::EndRadiusTurns {
            end: lit(positive(12.0)),
            turns: lit(positive_real(4.0)),
        }),
        json!({
            "kind": "end_radius_turns",
            "end": {"kind": "literal", "value": 12.0},
            "turns": {"kind": "literal", "value": 4.0},
        })
    );
    assert_eq!(
        to_value(&ExtrudeOrigin::ProfilePlane),
        json!({"kind": "profile_plane"})
    );
    assert_eq!(
        to_value(&ExtrudeOrigin::Offset(lit(length(2.5)))),
        json!({"kind": "offset", "value": {"kind": "literal", "value": 2.5}})
    );
    assert_eq!(
        to_value(&Rebuild::Refit { tolerance: None }),
        json!({"kind": "refit", "value": {}})
    );
    assert_eq!(
        to_value(&Rebuild::Points(12)),
        json!({"kind": "points", "value": 12})
    );
    assert_eq!(
        to_value(&HelixShape::Spiral { law: None }),
        json!({"kind": "spiral", "value": {}})
    );
    refuses::<HelixShape>(r#"{"kind": "spiral", "value": {"law": null}}"#);
    refuses::<HoleDraft>(r#"{"kind": "with_region", "extra": 1}"#);
    refuses::<ProfilePlacement>(r#"{"kind": "in_place", "extra": 1}"#);
    refuses::<PointCondition>(r#"{"kind": "sharp", "extra": 1}"#);
    refuses::<Spine>(r#"{"kind": "computed", "extra": 1}"#);
    refuses::<HoleDraft>(
        r#"{"kind": "angles", "end": {"kind": "literal", "value": 0.1}, "start": null}"#,
    );
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 7] = [
        ("CornerTransition", reads::<CornerTransition>),
        ("CouplingMode", reads::<CouplingMode>),
        ("ExtrudeOrigin", reads::<ExtrudeOrigin>),
        ("LoftStyle", reads::<LoftStyle>),
        ("RibGrowth", reads::<RibGrowth>),
        ("ScalingLaw", reads::<ScalingLaw>),
        ("SweepOrientation", reads::<SweepOrientation>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/swept.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

/// A primitive's fixed-size arrays write as arrays and refuse another length.
#[test]
fn primitive_sizes_are_fixed_arrays() {
    let shape = PrimitiveShape::Plane {
        size: [lit(positive(1.0)), lit(positive(2.0))],
    };
    let written = to_value(&shape);
    assert_eq!(written["value"]["size"].as_array().unwrap().len(), 2);
    let mut long = written.clone();
    long["value"]["size"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind": "literal", "value": 3.0}));
    refuses::<PrimitiveShape>(&long.to_string());
}

#[cfg(feature = "schema")]
mod schema {
    use schemars::{JsonSchema, SchemaGenerator};

    use super::super::*;

    fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
        let schema = T::json_schema(generator);
        assert!(schema.as_object().is_some(), "{}", T::schema_name());
    }

    #[test]
    fn every_swept_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Extrude>(&mut generator);
        generates::<ExtrudeDirection>(&mut generator);
        generates::<ExtrudeOrigin>(&mut generator);
        generates::<ExtrudeDraft>(&mut generator);
        generates::<DraftNeutral>(&mut generator);
        generates::<HoleDraft>(&mut generator);
        generates::<ProfileFit>(&mut generator);
        generates::<Revolve>(&mut generator);
        generates::<Sweep>(&mut generator);
        generates::<CornerTransition>(&mut generator);
        generates::<SweepSection>(&mut generator);
        generates::<SectionTurn>(&mut generator);
        generates::<SectionShape>(&mut generator);
        generates::<Apex>(&mut generator);
        generates::<SweepControls>(&mut generator);
        generates::<SweepOrientation>(&mut generator);
        generates::<GuideFrame>(&mut generator);
        generates::<Twist>(&mut generator);
        generates::<ScalingLaw>(&mut generator);
        generates::<SweepGuide>(&mut generator);
        generates::<PathExtent>(&mut generator);
        generates::<ProfilePlacement>(&mut generator);
        generates::<ProfilePosition>(&mut generator);
        generates::<SpineLimits>(&mut generator);
        generates::<HelicalSweep>(&mut generator);
        generates::<HelicalSection>(&mut generator);
        generates::<SectionPlacement>(&mut generator);
        generates::<Helix>(&mut generator);
        generates::<HelixStart>(&mut generator);
        generates::<HelixShape>(&mut generator);
        generates::<HelixLaw>(&mut generator);
        generates::<HelixTaper>(&mut generator);
        generates::<SpiralLaw>(&mut generator);
        generates::<Rib>(&mut generator);
        generates::<RibGrowth>(&mut generator);
        generates::<RibEnd>(&mut generator);
        generates::<Primitive>(&mut generator);
        generates::<PrimitiveShape>(&mut generator);
        generates::<Loft>(&mut generator);
        generates::<Section>(&mut generator);
        generates::<SectionForm>(&mut generator);
        generates::<Seam>(&mut generator);
        generates::<PointSection>(&mut generator);
        generates::<PointCondition>(&mut generator);
        generates::<Guide>(&mut generator);
        generates::<GuideInfluence>(&mut generator);
        generates::<Spine>(&mut generator);
        generates::<AreaStation>(&mut generator);
        generates::<CenterlineAt>(&mut generator);
        generates::<Correspondence>(&mut generator);
        generates::<CouplingMode>(&mut generator);
        generates::<Connector>(&mut generator);
        generates::<LoftShape>(&mut generator);
        generates::<LoftSmoothing>(&mut generator);
        generates::<LoftStyle>(&mut generator);
        generates::<Rebuild>(&mut generator);
        generates::<Interpolation>(&mut generator);
        generates::<OffsetJoin>(&mut generator);
        generates::<SheetCutNormal>(&mut generator);
    }
}
