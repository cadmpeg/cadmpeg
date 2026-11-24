// SPDX-License-Identifier: Apache-2.0
//! The walk over dress-up features and holes (§6.4).

use cadmpeg_core::CodecError;

use crate::design::ops::dressup::{
    Along, BlendControls, BlendLimit, Bore, Chamfer, ChamferSet, ChamferSize, Cone, ConicShape,
    Corner, Decal, DecalPlacement, Dome, Draft, DraftAngle, DraftSet, EdgeSetback, Emboss,
    EmbossEffect, FaceSide, FaceThickness, Fillet, FilletProfile, FilletSection, FilletSet,
    FilletSize, Hole, HoleBottom, HoleHead, HolePositions, HoleShape, LimitAt, Offset,
    OffsetResult, OffsetSet, RoundSide, SeriesStage, Setbacks, ShellFeature, StageHole,
    StandardSize, StationAt, Stations, Thicken, Thread, ThreadLength, ThreadSpec, Transition,
    TransitionKind, ValueStation,
};

use super::super::dimensions::Bound;
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
            keep_features,
            round_corners,
        } = self;
        keep_features.walk(into)?;
        round_corners.walk(into)?;
        corners.walk(into)?;
        limits.walk(into)?;
        keep.walk(into)?;
        transitions.walk(into)?;
        Ok(())
    }
}

impl Walk for BlendLimit {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { at, reversed } = self;
        reversed.walk(into)?;
        at.walk(into)?;
        Ok(())
    }
}

impl Walk for Bore {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            diameter,
            head,
            taper,
            bottom,
            near_chamfer,
            neck_chamfer,
            far_chamfer,
        } = self;
        diameter.walk(into)?;
        head.walk(into)?;
        taper.walk(into)?;
        bottom.walk(into)?;
        near_chamfer.walk(into)?;
        neck_chamfer.walk(into)?;
        far_chamfer.walk(into)?;
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
            flip,
            measure: _,
        } = self;
        flip.walk(into)?;
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
            Self::Distance(f0) => f0.walk(into),
            Self::Distances { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::DistanceAngle { distance, angle } => {
                distance.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::WidthAngle { width, angle } => {
                width.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::HeightAngle { height, angle } => {
                height.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Cone {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            diameter,
            depth,
            angle,
        } = self;
        diameter.walk(into)?;
        depth.walk(into)?;
        angle.walk(into)?;
        Ok(())
    }
}

impl Walk for ConicShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Rho { rho } => rho.walk(into),
            Self::BoundaryAndCenter { center } => center.walk(into),
            Self::CenterAndRho { center, rho } => {
                center.walk(into)?;
                rho.walk(into)?;
                Ok(())
            }
            Self::ShoulderRadius(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
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
            chain_faces,
            opacity,
            placement,
            native_options,
            source_settings,
        } = self;
        chain_faces.walk(into)?;
        opacity.walk(into)?;
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
            width,
            height,
            mirrored: _,
        } = self;
        width.walk(into)?;
        height.walk(into)?;
        frame.walk(into)?;
        Ok(())
    }
}

impl Walk for Dome {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            height,
            concave,
            shape: _,
            direction,
            points,
            native_options,
            source_settings,
        } = self;
        height.walk(into)?;
        concave.walk(into)?;
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
            Self::One(f0) => f0.walk(into),
            Self::Two { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Symmetric(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
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
        let Self { edge, distance } = self;
        distance.walk(into)?;
        edge.walk(into)?;
        Ok(())
    }
}

impl Walk for Emboss {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            effect,
            face,
            mapping: _,
            direction,
            targets,
            native_options,
            source_settings,
        } = self;
        effect.walk(into)?;
        profile.walk(into)?;
        face.walk(into)?;
        direction.walk(into)?;
        targets.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for EmbossEffect {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Emboss { depth } => depth.walk(into),
            Self::Engrave { depth } => depth.walk(into),
            Self::Scribe | Self::Native(_) => Ok(()),
        }
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
        let Self { faces, thickness } = self;
        thickness.walk(into)?;
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

impl Walk for FilletProfile {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Conic(f0) => f0.walk(into),
            Self::CurvatureContinuous { shape } => shape.walk(into),
            Self::Circular | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for FilletSection {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            size,
            profile,
            weight,
        } = self;
        profile.walk(into)?;
        weight.walk(into)?;
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
                trim,
            } => {
                trim.walk(into)?;
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
            Self::Radius(f0) => f0.walk(into),
            Self::Chord(f0) => f0.walk(into),
            Self::Offsets { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Held | Self::ByProfile | Self::Native(_) => Ok(()),
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
            standard,
            thread,
            targets,
            series,
            native_options,
            source_settings,
        } = self;
        standard.walk(into)?;
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

impl Walk for HoleBottom {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::DrillPoint { angle, depth_to: _ } => angle.walk(into),
            Self::Flat | Self::Trimmed | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for HoleHead {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Counterbore { diameter, depth } => {
                diameter.walk(into)?;
                depth.walk(into)?;
                Ok(())
            }
            Self::Countersink(f0) => f0.walk(into),
            Self::Counterdrill {
                diameter,
                depth,
                angle,
            } => {
                diameter.walk(into)?;
                depth.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Spotface { diameter, depth } => {
                diameter.walk(into)?;
                depth.walk(into)?;
                Ok(())
            }
            Self::Plain | Self::Native(_) => Ok(()),
        }
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
                bore,
                length,
                measure: _,
                direction,
            } => {
                bore.walk(into)?;
                length.walk(into)?;
                direction.walk(into)
            }
            Self::Bore(f0) => f0.walk(into),
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
        let Self { faces, distance } = self;
        distance.walk(into)?;
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
            Self::Uniform(f0) => f0.walk(into),
        }
    }
}

impl Walk for ShellFeature {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bodies,
            removed,
            thickness,
            faces,
            join: _,
            face_meeting: _,
            native_options,
            source_settings,
        } = self;
        thickness.walk(into)?;
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
                standard,
                thread,
            } => {
                standard.walk(into)?;
                shape.walk(into)?;
                extent.walk(into)?;
                thread.walk(into)?;
                Ok(())
            }
            Self::PartFeature(_) => Ok(()),
        }
    }
}

impl Walk for StandardSize {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            standard: _,
            kind: _,
            size,
            class: _,
        } = self;
        size.walk(into)?;
        Ok(())
    }
}

impl Walk for StationAt {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Parameter { along, fraction } => {
                fraction.walk(into)?;
                along.walk(into)
            }
            Self::Point(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for Stations<Q> {
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
            thickness,
            boolean,
            native_options,
            source_settings,
        } = self;
        thickness.walk(into)?;
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
            Self::ToElement { element, offset } => {
                offset.walk(into)?;
                element.walk(into)
            }
            Self::Blind(f0) => f0.walk(into),
            Self::Full | Self::Standard | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ThreadSpec {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            standard,
            major_diameter,
            minor_diameter,
            pitch_diameter,
            pitch,
            hand: _,
            length,
            modeled,
        } = self;
        standard.walk(into)?;
        major_diameter.walk(into)?;
        minor_diameter.walk(into)?;
        pitch_diameter.walk(into)?;
        pitch.walk(into)?;
        modeled.walk(into)?;
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
            Self::RollingBall { radius, against } => {
                radius.walk(into)?;
                against.walk(into)
            }
            Self::SweptAround { radius, against } => {
                radius.walk(into)?;
                against.walk(into)
            }
            Self::CornerSphere { radius, lengths } => {
                radius.walk(into)?;
                lengths.walk(into)?;
                Ok(())
            }
            Self::Continue | Self::Intersect | Self::Stop | Self::Blend | Self::Native(_) => Ok(()),
        }
    }
}

impl<Q: Bound> Walk for ValueStation<Q> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { at, value } = self;
        value.walk(into)?;
        at.walk(into)?;
        Ok(())
    }
}
