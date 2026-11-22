// SPDX-License-Identifier: Apache-2.0
//! The walk over swept solids and the loft (§6.3).

use cadmpeg_core::CodecError;

use crate::design::ops::swept::{
    Connector, Correspondence, Extrude, ExtrudeDirection, ExtrudeOrigin, Guide, HelicalSection,
    HelicalSweep, Helix, HelixShape, HelixStart, HelixTaper, Loft, PointCondition, PointSection,
    Primitive, ProfilePlacement, ProfilePosition, Revolve, Rib, RibGrowth, ScalingLaw, Seam,
    Section, SectionForm, Spine, SpineLimits, Sweep, SweepControls, SweepGuide, SweepOrientation,
    SweepSection, Twist,
};

use super::{Visit, Walk};

impl Walk for Connector {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { points } = self;
        points.walk(into)?;
        Ok(())
    }
}

impl Walk for Correspondence {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            mode: _,
            connectors,
        } = self;
        connectors.walk(into)?;
        Ok(())
    }
}

impl Walk for Extrude {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            loops: _,
            direction,
            origin,
            span,
            draft: _,
            fit: _,
            outside: _,
            sheet_cut: _,
            output,
            native_options,
            source_settings,
        } = self;
        profile.walk(into)?;
        direction.walk(into)?;
        origin.walk(into)?;
        span.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ExtrudeDirection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along {
                direction,
                lengths_along_normal: _,
            } => direction.walk(into),
            Self::ProfileNormal { reversed: _ } => Ok(()),
        }
    }
}

impl Walk for ExtrudeOrigin {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Plane(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Faces { faces, offset: _ } => faces.walk(into),
            Self::ProfilePlane | Self::Offset(_) => Ok(()),
        }
    }
}

impl Walk for Guide {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            path,
            tangency,
            influence: _,
        } = self;
        path.walk(into)?;
        tangency.walk(into)?;
        Ok(())
    }
}

impl Walk for HelicalSection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Generated {
                shape: _,
                placement: _,
            } => Ok(()),
        }
    }
}

impl Walk for HelicalSweep {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            section,
            helix,
            output,
            native_options,
            source_settings,
        } = self;
        section.walk(into)?;
        helix.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Helix {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            placement,
            start,
            shape,
            hand: _,
        } = self;
        placement.walk(into)?;
        start.walk(into)?;
        shape.walk(into)?;
        Ok(())
    }
}

impl Walk for HelixShape {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Helix { law: _, taper } => taper.walk(into),
            Self::Spiral { law: _ } | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for HelixStart {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(f0) => f0.walk(into),
            Self::Circle(f0) => f0.walk(into),
            Self::Radius {
                radius: _,
                angle: _,
            } => Ok(()),
        }
    }
}

impl Walk for HelixTaper {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Radius(f0) => f0.walk(into),
            Self::Angle(_) | Self::Growth(_) => Ok(()),
        }
    }
}

impl Walk for Loft {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            start,
            sections,
            end,
            cross,
            spine,
            guides,
            correspondence,
            shape: _,
            output,
            native_options,
            source_settings,
        } = self;
        start.walk(into)?;
        sections.walk(into)?;
        end.walk(into)?;
        cross.walk(into)?;
        spine.walk(into)?;
        guides.walk(into)?;
        correspondence.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for PointCondition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::TangentToPlane { plane, weight: _ } => plane.walk(into),
            Self::Sharp | Self::Tangent { weight: _ } => Ok(()),
        }
    }
}

impl Walk for PointSection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { point, condition } = self;
        point.walk(into)?;
        condition.walk(into)?;
        Ok(())
    }
}

impl Walk for Primitive {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            shape: _,
            placement,
            output,
            native_options,
            source_settings,
        } = self;
        placement.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ProfilePlacement {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Positioned(f0) => f0.walk(into),
            Self::InPlace => Ok(()),
        }
    }
}

impl Walk for ProfilePosition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            origin,
            direction,
            angle: _,
            swap_axes: _,
        } = self;
        origin.walk(into)?;
        direction.walk(into)?;
        Ok(())
    }
}

impl Walk for Revolve {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            loops: _,
            axis,
            span,
            fit: _,
            output,
            native_options,
            source_settings,
        } = self;
        profile.walk(into)?;
        axis.walk(into)?;
        span.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Rib {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            thickness: _,
            growth,
            flipped: _,
            extent,
            draft: _,
            thickness_at: _,
            root_fillet: _,
            extend_profile: _,
            boolean,
            native_options,
            source_settings,
        } = self;
        profile.walk(into)?;
        growth.walk(into)?;
        extent.walk(into)?;
        boolean.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for RibGrowth {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along(f0) => f0.walk(into),
            Self::InPlane | Self::Normal | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ScalingLaw {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Curve(f0) => f0.walk(into),
            Self::Taper(_) | Self::Area | Self::Perimeter | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Seam {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Vertex(f0) => f0.walk(into),
            Self::Parameter(_) => Ok(()),
        }
    }
}

impl Walk for Section {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            form,
            flip: _,
            seam,
            condition,
            trim_support: _,
            spine_point,
        } = self;
        form.walk(into)?;
        seam.walk(into)?;
        condition.walk(into)?;
        spine_point.walk(into)?;
        Ok(())
    }
}

impl Walk for SectionForm {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Placed { profile, placement } => {
                profile.walk(into)?;
                placement.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for Spine {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Path { path, controls } => {
                path.walk(into)?;
                controls.walk(into)?;
                Ok(())
            }
            Self::Centerline {
                path,
                sections: _,
                stations: _,
            } => path.walk(into),
            Self::Computed => Ok(()),
        }
    }
}

impl Walk for SpineLimits {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { start, end } = self;
        start.walk(into)?;
        end.walk(into)?;
        Ok(())
    }
}

impl Walk for Sweep {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            section,
            path,
            controls,
            guides,
            extent: _,
            placement,
            limits,
            transition: _,
            output,
            native_options,
            source_settings,
        } = self;
        section.walk(into)?;
        path.walk(into)?;
        controls.walk(into)?;
        guides.walk(into)?;
        placement.walk(into)?;
        limits.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for SweepControls {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            orientation,
            twist,
            scaling,
        } = self;
        orientation.walk(into)?;
        twist.walk(into)?;
        scaling.walk(into)?;
        Ok(())
    }
}

impl Walk for SweepGuide {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { path, extent: _ } = self;
        path.walk(into)?;
        Ok(())
    }
}

impl Walk for SweepOrientation {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Direction(f0) => f0.walk(into),
            Self::Surface(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Curve {
                path,
                tangent: _,
                curvilinear: _,
            } => path.walk(into),
            Self::FollowPath
            | Self::MinimumTwist
            | Self::Frenet
            | Self::Fixed
            | Self::Guides(_)
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for SweepSection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Generated { shape: _, turn: _ } => Ok(()),
        }
    }
}

impl Walk for Twist {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::Angle(_) => Ok(()),
        }
    }
}
