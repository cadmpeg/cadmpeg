// SPDX-License-Identifier: Apache-2.0
//! The walk over dress-up features and holes (§6.4).

use cadmpeg_core::CodecError;

use crate::design::ops::dressup::{
    Along, BlendControls, BlendLimit, Chamfer, ChamferSet, ChamferSize, Corner, Decal,
    DecalPlacement, Dome, Draft, DraftAngle, DraftSet, EdgeSetback, Emboss, FaceSide,
    FaceThickness, Fillet, FilletSection, FilletSet, FilletSize, Hole, HolePositions, HoleShape,
    LimitAt, Offset, OffsetResult, OffsetSet, RoundSide, SeriesStage, Setbacks, ShellFeature,
    StageHole, StationAt, Stations, Thicken, Thread, ThreadLength, ThreadSpec, Transition,
    TransitionKind, ValueStation,
};

use super::{Collect, Walk};

impl Walk for Along {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Edge(f0) => f0.walk(into),
            Self::Chain {} => Ok(()),
        }
    }
}

impl Walk for BlendControls {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            corner_style: _,
            corners,
            limits,
            keep,
            transitions,
            overflow: _,
            keep_features: _,
            round_corners: _,
        } = self;
        corners.walk(into)?;
        limits.walk(into)?;
        keep.walk(into)?;
        transitions.walk(into)?;
        Ok(())
    }
}

impl Walk for BlendLimit {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { at, reversed: _ } = self;
        at.walk(into)?;
        Ok(())
    }
}

impl Walk for Chamfer {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            sets,
            controls,
            native_options,
            source_settings,
        } = self;
        sets.walk(into)?;
        controls.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ChamferSet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            size,
            reference,
            flip: _,
            measure: _,
        } = self;
        edges.walk(into)?;
        size.walk(into)?;
        reference.walk(into)?;
        Ok(())
    }
}

impl Walk for ChamferSize {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Vertex(f0) => f0.walk(into),
            Self::Distance(_)
            | Self::Distances {
                first: _,
                second: _,
            }
            | Self::DistanceAngle {
                distance: _,
                angle: _,
            }
            | Self::WidthAngle { width: _, angle: _ }
            | Self::HeightAngle {
                height: _,
                angle: _,
            }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Corner {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { vertex, setbacks } = self;
        vertex.walk(into)?;
        setbacks.walk(into)?;
        Ok(())
    }
}

impl Walk for Decal {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            image: _,
            faces,
            chain_faces: _,
            opacity: _,
            placement,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        placement.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for DecalPlacement {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            frame,
            width: _,
            height: _,
            mirrored: _,
        } = self;
        frame.walk(into)?;
        Ok(())
    }
}

impl Walk for Dome {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            height: _,
            concave: _,
            shape: _,
            direction,
            points,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        direction.walk(into)?;
        points.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Draft {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            sets,
            pull,
            neutral,
            parting,
            steps: _,
            native_options,
            source_settings,
        } = self;
        sets.walk(into)?;
        pull.walk(into)?;
        neutral.walk(into)?;
        parting.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for DraftAngle {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Variable(f0) => f0.walk(into),
            Self::One(_)
            | Self::Two {
                first: _,
                second: _,
            }
            | Self::Symmetric(_)
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for DraftSet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            fixed,
            angle,
        } = self;
        faces.walk(into)?;
        fixed.walk(into)?;
        angle.walk(into)?;
        Ok(())
    }
}

impl Walk for EdgeSetback {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { edge, distance: _ } = self;
        edge.walk(into)?;
        Ok(())
    }
}

impl Walk for Emboss {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            effect: _,
            face,
            mapping: _,
            direction,
            targets,
            native_options,
            source_settings,
        } = self;
        profile.walk(into)?;
        face.walk(into)?;
        direction.walk(into)?;
        targets.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for FaceSide {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { faces, hold } = self;
        faces.walk(into)?;
        hold.walk(into)?;
        Ok(())
    }
}

impl Walk for FaceThickness {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            thickness: _,
        } = self;
        faces.walk(into)?;
        Ok(())
    }
}

impl Walk for Fillet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            sets,
            controls,
            native_options,
            source_settings,
        } = self;
        sets.walk(into)?;
        controls.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for FilletSection {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            size,
            profile: _,
            weight: _,
        } = self;
        size.walk(into)?;
        Ok(())
    }
}

impl Walk for FilletSet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Edges {
                edges,
                section,
                spine,
                hold,
            } => {
                edges.walk(into)?;
                section.walk(into)?;
                spine.walk(into)?;
                hold.walk(into)?;
                Ok(())
            }
            Self::Faces {
                first,
                second,
                section,
                spine,
                near,
                trim: _,
            } => {
                first.walk(into)?;
                second.walk(into)?;
                section.walk(into)?;
                spine.walk(into)?;
                near.walk(into)?;
                Ok(())
            }
            Self::FullRound {
                first,
                center,
                second,
            } => {
                first.walk(into)?;
                center.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for FilletSize {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Variable(f0) => f0.walk(into),
            Self::Radius(_)
            | Self::Chord(_)
            | Self::Offsets {
                first: _,
                second: _,
            }
            | Self::Held
            | Self::ByProfile
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Hole {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            positions,
            start,
            direction,
            shape,
            extent,
            standard: _,
            thread,
            targets,
            series,
            native_options,
            source_settings,
        } = self;
        positions.walk(into)?;
        start.walk(into)?;
        direction.walk(into)?;
        shape.walk(into)?;
        extent.walk(into)?;
        thread.walk(into)?;
        targets.walk(into)?;
        series.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for HolePositions {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Points { points, centers: _ } => points.walk(into),
            Self::Placed(f0) => f0.walk(into),
        }
    }
}

impl Walk for HoleShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Revolved(f0) => f0.walk(into),
            Self::Slot {
                bore: _,
                length: _,
                measure: _,
                direction,
            } => direction.walk(into),
            Self::Bore(_) => Ok(()),
        }
    }
}

impl Walk for LimitAt {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Element(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
        }
    }
}

impl Walk for Offset {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            sets,
            result,
            join: _,
            face_meeting: _,
            native_options,
            source_settings,
        } = self;
        sets.walk(into)?;
        result.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for OffsetResult {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Copy(f0) => f0.walk(into),
            Self::Move {} => Ok(()),
        }
    }
}

impl Walk for OffsetSet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { faces, distance: _ } = self;
        faces.walk(into)?;
        Ok(())
    }
}

impl Walk for RoundSide {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces(f0) => f0.walk(into),
            Self::Automatic {} => Ok(()),
        }
    }
}

impl Walk for SeriesStage {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { parts, hole } = self;
        parts.walk(into)?;
        hole.walk(into)?;
        Ok(())
    }
}

impl Walk for Setbacks {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::PerEdge(f0) => f0.walk(into),
            Self::Uniform(_) => Ok(()),
        }
    }
}

impl Walk for ShellFeature {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bodies,
            removed,
            thickness: _,
            faces,
            join: _,
            face_meeting: _,
            native_options,
            source_settings,
        } = self;
        bodies.walk(into)?;
        removed.walk(into)?;
        faces.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for StageHole {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Stated {
                shape,
                extent,
                standard: _,
                thread,
            } => {
                shape.walk(into)?;
                extent.walk(into)?;
                thread.walk(into)?;
                Ok(())
            }
            Self::PartFeature(_) => Ok(()),
        }
    }
}

impl Walk for StationAt {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Parameter { along, fraction: _ } => along.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for Stations<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            points,
            law: _,
            chain_start,
        } = self;
        points.walk(into)?;
        chain_start.walk(into)?;
        Ok(())
    }
}

impl Walk for Thicken {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            thickness: _,
            boolean,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        boolean.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Thread {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            face,
            start,
            spec,
            native_options,
            source_settings,
        } = self;
        face.walk(into)?;
        start.walk(into)?;
        spec.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ThreadLength {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::ToElement { element, offset: _ } => element.walk(into),
            Self::Full | Self::Blind(_) | Self::Standard | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ThreadSpec {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            standard: _,
            major_diameter: _,
            minor_diameter: _,
            pitch_diameter: _,
            pitch: _,
            hand: _,
            length,
            modeled: _,
        } = self;
        length.walk(into)?;
        Ok(())
    }
}

impl Walk for Transition {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            between: _,
            at,
            kind,
            native_options,
        } = self;
        at.walk(into)?;
        kind.walk(into)?;
        native_options.walk(into)?;
        Ok(())
    }
}

impl Walk for TransitionKind {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::StopAt(f0) => f0.walk(into),
            Self::RollingBall { radius: _, against } => against.walk(into),
            Self::SweptAround { radius: _, against } => against.walk(into),
            Self::Continue
            | Self::Intersect
            | Self::Stop
            | Self::CornerSphere {
                radius: _,
                lengths: _,
            }
            | Self::Blend
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q> Walk for ValueStation<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { at, value: _ } = self;
        at.walk(into)?;
        Ok(())
    }
}
