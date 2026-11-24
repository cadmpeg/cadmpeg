// SPDX-License-Identifier: Apache-2.0
//! The walk over the option types shared by several feature families (§6).

use cadmpeg_core::CodecError;

use crate::design::ops::shared::{
    AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, CircularLayout, CompositeStage,
    Coupling, DimensionRun, DimensionTable, Increment, InstanceOrientation, Joint, JointConnector,
    JointKind, JointOffset, Law, Limit, LinearLayout, LinearRun, Locus, MirrorLayout,
    PathAxisControl, PathPosition, PathReference, PathRollControl, PatternLayout, PatternRun,
    RadialDimension, RunSides, ScaleLayout, Spacing, Span, TableRow, UpToFill, WidthMode,
};

use super::super::dimensions::Bound;
use super::{Collect, Walk};

impl Walk for ChainLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { path, links } = self;
        path.walk(into)?;
        links.walk(into)?;
        Ok(())
    }
}

impl Walk for ChainLink {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { pins } = self;
        pins.walk(into)?;
        Ok(())
    }
}

impl Walk for ChainLinks {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Connected(f0) => f0.walk(into),
            Self::Native { kind: _, picks } => picks.walk(into),
        }
    }
}

impl Walk for InstanceOrientation {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Rolled {
                reference,
                reference_axis: _,
                seed_axis,
            } => {
                reference.walk(into)?;
                seed_axis.walk(into)?;
                Ok(())
            }
            Self::Fixed | Self::Follow | Self::Frenet | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Joint {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            connectors,
            kind,
            alignment: _,
            native_options,
            source_settings,
        } = self;
        connectors.walk(into)?;
        kind.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for JointConnector {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Frame(f0) => f0.walk(into),
            Self::Plane(f0) => f0.walk(into),
            Self::Axis(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Entity(f0) => f0.walk(into),
        }
    }
}

impl Walk for JointKind {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Motion {
                motion: _,
                axes,
                offset,
                limits,
                drives: _,
            } => {
                offset.walk(into)?;
                limits.walk(into)?;
                axes.walk(into)
            }
            Self::Symmetric { plane } => plane.walk(into),
            Self::Width { pair, mode } => {
                mode.walk(into)?;
                pair.walk(into)
            }
            Self::Path {
                position,
                pitch_yaw,
                roll,
            } => {
                position.walk(into)?;
                pitch_yaw.walk(into)?;
                roll.walk(into)
            }
            Self::Coupling { coupling, joints } => {
                coupling.walk(into)?;
                joints.walk(into)
            }
            Self::Coincident { offset } => offset.walk(into),
            Self::Insert { offset } => offset.walk(into),
            Self::Distance { distance, limits } => {
                distance.walk(into)?;
                limits.walk(into)?;
                Ok(())
            }
            Self::Angle { angle, limits } => {
                angle.walk(into)?;
                limits.walk(into)?;
                Ok(())
            }
            Self::PointOnAxis { distance } => distance.walk(into),
            Self::Slot {
                position: _,
                lock_rotation,
            } => lock_rotation.walk(into),
            Self::Concentric
            | Self::Parallel
            | Self::Perpendicular
            | Self::Tangent { contact: _ }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for Law<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Curve(f0) => f0.walk(into),
            Self::Expression(f0) => {
                into.push_expression(super::ExpressionPlace::Law, f0);
                f0.walk(into)
            }
            Self::Constant(f0) => f0.walk(into),
            Self::Linear { start, end } => {
                start.walk(into)?;
                end.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for Limit<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::ToGeometry {
                target,
                offset,
                extend,
                far,
            } => {
                offset.walk(into)?;
                extend.walk(into)?;
                far.walk(into)?;
                target.walk(into)
            }
            Self::ToPlane(f0) => f0.walk(into),
            Self::ToPoint(f0) => f0.walk(into),
            Self::Amount(f0) => f0.walk(into),
            Self::AtOrigin | Self::ThroughAll | Self::Next | Self::Last | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for LinearRun {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { direction, run } = self;
        direction.walk(into)?;
        run.walk(into)?;
        Ok(())
    }
}

impl Walk for Locus {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(f0) => f0.walk(into),
            Self::Centroid => Ok(()),
        }
    }
}

impl Walk for PathReference {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Direction(f0) => f0.walk(into),
            Self::PlaneNormal => Ok(()),
        }
    }
}

impl Walk for PathRollControl {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpVector {
                vector,
                axis: _,
                flip,
            } => {
                flip.walk(into)?;
                vector.walk(into)
            }
            Self::Free => Ok(()),
        }
    }
}

impl Walk for PatternLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("PatternLayout")?;
        match self {
            Self::Linear(f0) => f0.walk(into),
            Self::Circular(f0) => f0.walk(into),
            Self::Path {
                path,
                run,
                orientation,
            } => {
                path.walk(into)?;
                run.walk(into)?;
                orientation.walk(into)?;
                Ok(())
            }
            Self::Points { points, reference } => {
                points.walk(into)?;
                reference.walk(into)?;
                Ok(())
            }
            Self::Mirror(f0) => f0.walk(into),
            Self::Composite(f0) => f0.walk(into),
            Self::Scale(f0) => f0.walk(into),
            Self::Follow(f0) => f0.walk(into),
            Self::Chain(f0) => f0.walk(into),
            Self::Dimensions(f0) => f0.walk(into),
            Self::Table(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for CompositeStage {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Linear(f0) => f0.walk(into),
            Self::Circular(f0) => f0.walk(into),
            Self::Mirror(f0) => f0.walk(into),
            Self::Scale(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for LinearLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            first,
            second,
            seed_only,
        } = self;
        seed_only.walk(into)?;
        first.walk(into)?;
        second.walk(into)?;
        Ok(())
    }
}

impl Walk for CircularLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            axis,
            angular,
            radial,
            orientation,
        } = self;
        axis.walk(into)?;
        angular.walk(into)?;
        radial.walk(into)?;
        orientation.walk(into)?;
        Ok(())
    }
}

impl Walk for MirrorLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            plane,
            stitch_tolerance,
        } = self;
        stitch_tolerance.walk(into)?;
        plane.walk(into)
    }
}

impl Walk for ScaleLayout {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            factor,
            count,
            center,
        } = self;
        factor.walk(into)?;
        count.walk(into)?;
        center.walk(into)
    }
}

impl<Q: Bound> Walk for PatternRun<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            spacing,
            sides,
            seed,
            increments,
        } = self;
        seed.walk(into)?;
        increments.walk(into)?;
        spacing.walk(into)?;
        sides.walk(into)?;
        Ok(())
    }
}

impl<Q: Bound> Walk for RunSides<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Reverse(f0) => f0.walk(into),
            Self::One
            | Self::Against
            | Self::Symmetric {
                count: _,
                extent: _,
            } => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for Spacing<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpTo {
                reference,
                fill,
                offset,
                from,
            } => {
                fill.walk(into)?;
                offset.walk(into)?;
                reference.walk(into)?;
                from.walk(into)?;
                Ok(())
            }
            Self::Pitch { count, pitch } => {
                count.walk(into)?;
                pitch.walk(into)?;
                Ok(())
            }
            Self::Span { count, span } => {
                count.walk(into)?;
                span.walk(into)?;
                Ok(())
            }
            Self::PitchSpan { pitch, span } => {
                pitch.walk(into)?;
                span.walk(into)?;
                Ok(())
            }
            Self::Fit { count } => count.walk(into),
            Self::List(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for Span<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Between { start, end } => {
                start.walk(into)?;
                end.walk(into)?;
                Ok(())
            }
            Self::Symmetric { amount, measure: _ } => amount.walk(into),
        }
    }
}

impl Walk for AxisLimits {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Rotation {
                axis: _,
                limits,
                rest,
            } => {
                limits.walk(into)?;
                rest.walk(into)?;
                Ok(())
            }
            Self::Translation {
                axis: _,
                limits,
                rest,
            } => {
                limits.walk(into)?;
                rest.walk(into)?;
                Ok(())
            }
        }
    }
}

impl<Q: Bound> Walk for Bounds<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { min, max } = self;
        min.walk(into)?;
        max.walk(into)?;
        Ok(())
    }
}

impl Walk for Coupling {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Gear { ratio, radii } => {
                ratio.walk(into)?;
                radii.walk(into)?;
                Ok(())
            }
            Self::Belt { ratio, radii } => {
                ratio.walk(into)?;
                radii.walk(into)?;
                Ok(())
            }
            Self::RackPinion {
                ratio,
                pitch_radius,
            } => {
                ratio.walk(into)?;
                pitch_radius.walk(into)?;
                Ok(())
            }
            Self::Screw { pitch } => pitch.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for DimensionRun {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { count, increments } = self;
        count.walk(into)?;
        increments.walk(into)?;
        Ok(())
    }
}

impl Walk for DimensionTable {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            parameters: _,
            rows,
        } = self;
        rows.walk(into)?;
        Ok(())
    }
}

impl Walk for Increment {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { parameter: _, step } = self;
        step.walk(into)?;
        Ok(())
    }
}

impl Walk for JointOffset {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { x, y, z, angle } = self;
        x.walk(into)?;
        y.walk(into)?;
        z.walk(into)?;
        angle.walk(into)?;
        Ok(())
    }
}

impl Walk for PathAxisControl {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::FollowPath { axis: _, flip } => flip.walk(into),
            Self::Free => Ok(()),
        }
    }
}

impl Walk for PathPosition {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Distance { distance, from_end } => {
                distance.walk(into)?;
                from_end.walk(into)?;
                Ok(())
            }
            Self::Percent { fraction, from_end } => {
                fraction.walk(into)?;
                from_end.walk(into)?;
                Ok(())
            }
            Self::Free => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for RadialDimension<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Radius(f0) => f0.walk(into),
            Self::Diameter(f0) => f0.walk(into),
        }
    }
}

impl Walk for TableRow {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            instance: _,
            values,
        } = self;
        values.walk(into)?;
        Ok(())
    }
}

impl<Q: Bound> Walk for UpToFill<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Count(f0) => f0.walk(into),
            Self::Pitch(f0) => f0.walk(into),
        }
    }
}

impl Walk for WidthMode {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Gap { distance, from: _ } => distance.walk(into),
            Self::Centered | Self::Free | Self::Native(_) => Ok(()),
        }
    }
}
