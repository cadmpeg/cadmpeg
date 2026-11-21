// SPDX-License-Identifier: Apache-2.0
//! The walk over the option types shared by several feature families (§6).

use cadmpeg_core::CodecError;

use crate::design::ops::shared::{
    ChainLayout, ChainLink, ChainLinks, InstanceOrientation, Joint, JointConnector, JointKind, Law,
    Limit, LinearRun, Locus, PathReference, PathRollControl, PatternLayout, PatternRun, RunSides,
    Spacing, Span,
};

use super::{Visit, Walk};

impl Walk for ChainLayout {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { path, links } = self;
        path.walk(into)?;
        links.walk(into)?;
        Ok(())
    }
}

impl Walk for ChainLink {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { pins } = self;
        pins.walk(into)?;
        Ok(())
    }
}

impl Walk for ChainLinks {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Connected(f0) => f0.walk(into),
            Self::Native { kind: _, picks } => picks.walk(into),
        }
    }
}

impl Walk for InstanceOrientation {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Motion {
                motion: _,
                axes,
                offset: _,
                limits: _,
                drives: _,
            } => axes.walk(into),
            Self::Symmetric { plane } => plane.walk(into),
            Self::Width { pair, mode: _ } => pair.walk(into),
            Self::Path {
                position: _,
                pitch_yaw: _,
                roll,
            } => roll.walk(into),
            Self::Coupling {
                coupling: _,
                joints,
            } => joints.walk(into),
            Self::Coincident { offset: _ }
            | Self::Concentric
            | Self::Insert { offset: _ }
            | Self::Distance {
                distance: _,
                limits: _,
            }
            | Self::Angle {
                angle: _,
                limits: _,
            }
            | Self::Parallel
            | Self::Perpendicular
            | Self::Tangent { contact: _ }
            | Self::PointOnAxis { distance: _ }
            | Self::Slot {
                position: _,
                lock_rotation: _,
            }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for Law<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Curve(f0) => f0.walk(into),
            Self::Expression(f0) => f0.walk(into),
            Self::Constant(_) | Self::Linear { start: _, end: _ } | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for Limit<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::ToGeometry {
                target,
                offset: _,
                extend: _,
                far: _,
            } => target.walk(into),
            Self::ToPlane(f0) => f0.walk(into),
            Self::ToPoint(f0) => f0.walk(into),
            Self::AtOrigin
            | Self::Amount(_)
            | Self::ThroughAll
            | Self::Next
            | Self::Last
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for LinearRun {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { direction, run } = self;
        direction.walk(into)?;
        run.walk(into)?;
        Ok(())
    }
}

impl Walk for Locus {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(f0) => f0.walk(into),
            Self::Centroid => Ok(()),
        }
    }
}

impl Walk for PathReference {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Direction(f0) => f0.walk(into),
            Self::PlaneNormal => Ok(()),
        }
    }
}

impl Walk for PathRollControl {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpVector {
                vector,
                axis: _,
                flip: _,
            } => vector.walk(into),
            Self::Free => Ok(()),
        }
    }
}

impl Walk for PatternLayout {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("PatternLayout")?;
        match self {
            Self::Linear {
                first,
                second,
                seed_only: _,
            } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Circular {
                axis,
                angular,
                radial,
                orientation,
            } => {
                axis.walk(into)?;
                angular.walk(into)?;
                radial.walk(into)?;
                orientation.walk(into)?;
                Ok(())
            }
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
            Self::Mirror {
                plane,
                stitch_tolerance: _,
            } => plane.walk(into),
            Self::Composite(f0) => f0.walk(into),
            Self::Scale {
                factor: _,
                count: _,
                center,
            } => center.walk(into),
            Self::Follow(f0) => f0.walk(into),
            Self::Chain(f0) => f0.walk(into),
            Self::Dimensions(_) | Self::Table(_) | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for PatternRun<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            spacing,
            sides,
            seed: _,
            increments: _,
        } = self;
        spacing.walk(into)?;
        sides.walk(into)?;
        Ok(())
    }
}

impl<Q> Walk for RunSides<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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

impl<Q> Walk for Spacing<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpTo {
                reference,
                fill: _,
                offset: _,
                from,
            } => {
                reference.walk(into)?;
                from.walk(into)?;
                Ok(())
            }
            Self::Pitch { count: _, pitch: _ }
            | Self::Span { count: _, span: _ }
            | Self::PitchSpan { pitch: _, span: _ }
            | Self::Fit { count: _ }
            | Self::List(_)
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for Span<Q> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Between { start, end } => {
                start.walk(into)?;
                end.walk(into)?;
                Ok(())
            }
            Self::Symmetric {
                amount: _,
                measure: _,
            } => Ok(()),
        }
    }
}
