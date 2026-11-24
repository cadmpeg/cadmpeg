// SPDX-License-Identifier: Apache-2.0
//! The walk over swept solids and the loft (§6.3).

use cadmpeg_core::CodecError;

use crate::design::ops::swept::{
    AreaStation, CenterlineAt, Connector, Correspondence, Extrude, ExtrudeDirection, ExtrudeDraft,
    ExtrudeOrigin, Guide, HelicalSection, HelicalSweep, Helix, HelixLaw, HelixShape, HelixStart,
    HelixTaper, HoleDraft, Loft, LoftShape, LoftSmoothing, PathExtent, PointCondition,
    PointSection, Primitive, PrimitiveShape, ProfileFit, ProfilePlacement, ProfilePosition,
    Revolve, Rib, RibGrowth, ScalingLaw, Seam, Section, SectionForm, SectionShape, Spine,
    SpineLimits, SpiralLaw, Sweep, SweepControls, SweepGuide, SweepOrientation, SweepSection,
    Twist,
};

use super::{Collect, Walk};

impl Walk for AreaStation {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            at,
            area,
            driving: _,
        } = self;
        at.walk(into)?;
        area.walk(into)?;
        Ok(())
    }
}

impl Walk for CenterlineAt {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Fraction(f0) => f0.walk(into),
            Self::Distance(f0) => f0.walk(into),
        }
    }
}

impl Walk for Connector {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { points } = self;
        points.walk(into)?;
        Ok(())
    }
}

impl Walk for Correspondence {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            mode: _,
            connectors,
        } = self;
        connectors.walk(into)?;
        Ok(())
    }
}

impl Walk for Extrude {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            loops: _,
            direction,
            origin,
            span,
            draft,
            fit,
            outside,
            sheet_cut: _,
            output,
            native_options,
            source_settings,
        } = self;
        draft.walk(into)?;
        fit.walk(into)?;
        outside.walk(into)?;
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along {
                direction,
                lengths_along_normal,
            } => {
                lengths_along_normal.walk(into)?;
                direction.walk(into)
            }
            Self::ProfileNormal { reversed } => reversed.walk(into),
        }
    }
}

impl Walk for ExtrudeDraft {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            end,
            start,
            neutral: _,
            holes,
        } = self;
        end.walk(into)?;
        start.walk(into)?;
        holes.walk(into)?;
        Ok(())
    }
}

impl Walk for ExtrudeOrigin {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Plane(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Faces { faces, offset } => {
                offset.walk(into)?;
                faces.walk(into)
            }
            Self::Offset(f0) => f0.walk(into),
            Self::ProfilePlane => Ok(()),
        }
    }
}

impl Walk for Guide {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Generated {
                shape,
                placement: _,
            } => shape.walk(into),
        }
    }
}

impl Walk for HelicalSweep {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            placement,
            start,
            shape,
            hand,
        } = self;
        hand.walk(into)?;
        placement.walk(into)?;
        start.walk(into)?;
        shape.walk(into)?;
        Ok(())
    }
}

impl Walk for HelixLaw {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::PitchHeight { pitch, height } => {
                pitch.walk(into)?;
                height.walk(into)?;
                Ok(())
            }
            Self::PitchTurns { pitch, turns } => {
                pitch.walk(into)?;
                turns.walk(into)?;
                Ok(())
            }
            Self::HeightTurns { height, turns } => {
                height.walk(into)?;
                turns.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for HelixShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Helix { law, taper } => {
                law.walk(into)?;
                taper.walk(into)
            }
            Self::Spiral { law } => law.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for HelixStart {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(f0) => f0.walk(into),
            Self::Circle(f0) => f0.walk(into),
            Self::Radius { radius, angle } => {
                radius.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for HelixTaper {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Radius(f0) => f0.walk(into),
            Self::Angle(f0) => f0.walk(into),
            Self::Growth(f0) => f0.walk(into),
        }
    }
}

impl Walk for HoleDraft {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Angles { end, start } => {
                end.walk(into)?;
                start.walk(into)?;
                Ok(())
            }
            Self::WithRegion | Self::WithOuter => Ok(()),
        }
    }
}

impl Walk for Loft {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            start,
            sections,
            end,
            cross,
            spine,
            guides,
            correspondence,
            shape,
            output,
            native_options,
            source_settings,
        } = self;
        shape.walk(into)?;
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

impl Walk for LoftShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            style: _,
            max_degree,
            rebuild: _,
            ruled,
            closed,
            relimit,
            merge_tangent_faces,
            smoothing,
            section_law: _,
        } = self;
        max_degree.walk(into)?;
        ruled.walk(into)?;
        closed.walk(into)?;
        relimit.walk(into)?;
        merge_tangent_faces.walk(into)?;
        smoothing.walk(into)?;
        Ok(())
    }
}

impl Walk for LoftSmoothing {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { angle, deviation } = self;
        angle.walk(into)?;
        deviation.walk(into)?;
        Ok(())
    }
}

impl Walk for PathExtent {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { along, against } = self;
        along.walk(into)?;
        against.walk(into)?;
        Ok(())
    }
}

impl Walk for PointCondition {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::TangentToPlane { plane, weight } => {
                weight.walk(into)?;
                plane.walk(into)
            }
            Self::Tangent { weight } => weight.walk(into),
            Self::Sharp => Ok(()),
        }
    }
}

impl Walk for PointSection {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { point, condition } = self;
        point.walk(into)?;
        condition.walk(into)?;
        Ok(())
    }
}

impl Walk for Primitive {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            shape,
            placement,
            output,
            native_options,
            source_settings,
        } = self;
        shape.walk(into)?;
        placement.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for PrimitiveShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Box { size } => size.walk(into),
            Self::Cylinder {
                radius,
                height,
                angle,
            } => {
                radius.walk(into)?;
                height.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Cone {
                radii,
                height,
                angle,
            } => {
                radii.walk(into)?;
                height.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Sphere {
                radius,
                latitudes,
                longitude,
            } => {
                radius.walk(into)?;
                latitudes.walk(into)?;
                longitude.walk(into)?;
                Ok(())
            }
            Self::Torus {
                radii,
                latitudes,
                longitude,
            } => {
                radii.walk(into)?;
                latitudes.walk(into)?;
                longitude.walk(into)?;
                Ok(())
            }
            Self::Ellipsoid {
                radii,
                latitudes,
                longitude,
            } => {
                radii.walk(into)?;
                latitudes.walk(into)?;
                longitude.walk(into)?;
                Ok(())
            }
            Self::Prism {
                sides,
                circumradius,
                height,
            } => {
                sides.walk(into)?;
                circumradius.walk(into)?;
                height.walk(into)?;
                Ok(())
            }
            Self::Wedge { y, base, top } => {
                y.walk(into)?;
                base.walk(into)?;
                top.walk(into)?;
                Ok(())
            }
            Self::Plane { size } => size.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ProfileFit {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            outer,
            inner,
            outer_join: _,
            inner_join: _,
        } = self;
        outer.walk(into)?;
        inner.walk(into)?;
        Ok(())
    }
}

impl Walk for ProfilePlacement {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Positioned(f0) => f0.walk(into),
            Self::InPlace => Ok(()),
        }
    }
}

impl Walk for ProfilePosition {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            origin,
            direction,
            angle,
            swap_axes,
        } = self;
        angle.walk(into)?;
        swap_axes.walk(into)?;
        origin.walk(into)?;
        direction.walk(into)?;
        Ok(())
    }
}

impl Walk for Revolve {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            loops: _,
            axis,
            span,
            fit,
            output,
            native_options,
            source_settings,
        } = self;
        fit.walk(into)?;
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            profile,
            thickness,
            growth,
            flipped,
            extent,
            draft,
            thickness_at: _,
            root_fillet,
            extend_profile,
            boolean,
            native_options,
            source_settings,
        } = self;
        thickness.walk(into)?;
        flipped.walk(into)?;
        draft.walk(into)?;
        root_fillet.walk(into)?;
        extend_profile.walk(into)?;
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along(f0) => f0.walk(into),
            Self::InPlane | Self::Normal | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ScalingLaw {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Curve(f0) => f0.walk(into),
            Self::Taper(f0) => f0.walk(into),
            Self::Area | Self::Perimeter | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Seam {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Vertex(f0) => f0.walk(into),
            Self::Parameter(_) => Ok(()),
        }
    }
}

impl Walk for Section {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            form,
            flip,
            seam,
            condition,
            trim_support,
            spine_point,
        } = self;
        flip.walk(into)?;
        trim_support.walk(into)?;
        form.walk(into)?;
        seam.walk(into)?;
        condition.walk(into)?;
        spine_point.walk(into)?;
        Ok(())
    }
}

impl Walk for SectionForm {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
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

impl Walk for SectionShape {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Circle {
                diameter,
                inner_diameter,
            } => {
                diameter.walk(into)?;
                inner_diameter.walk(into)?;
                Ok(())
            }
            Self::Square { size } => size.walk(into),
            Self::Triangle { size, apex: _ } => size.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Spine {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Path { path, controls } => {
                path.walk(into)?;
                controls.walk(into)?;
                Ok(())
            }
            Self::Centerline {
                path,
                sections,
                stations,
            } => {
                sections.walk(into)?;
                stations.walk(into)?;
                path.walk(into)
            }
            Self::Computed => Ok(()),
        }
    }
}

impl Walk for SpineLimits {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { start, end } = self;
        start.walk(into)?;
        end.walk(into)?;
        Ok(())
    }
}

impl Walk for SpiralLaw {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::PitchTurns { pitch, turns } => {
                pitch.walk(into)?;
                turns.walk(into)?;
                Ok(())
            }
            Self::EndRadiusTurns { end, turns } => {
                end.walk(into)?;
                turns.walk(into)?;
                Ok(())
            }
            Self::EndRadiusPitch { end, pitch } => {
                end.walk(into)?;
                pitch.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for Sweep {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            section,
            path,
            controls,
            guides,
            extent,
            placement,
            limits,
            transition: _,
            output,
            native_options,
            source_settings,
        } = self;
        extent.walk(into)?;
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { path, extent } = self;
        extent.walk(into)?;
        path.walk(into)?;
        Ok(())
    }
}

impl Walk for SweepOrientation {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Direction(f0) => f0.walk(into),
            Self::Surface(f0) => f0.walk(into),
            Self::Point(f0) => f0.walk(into),
            Self::Curve {
                path,
                tangent,
                curvilinear,
            } => {
                tangent.walk(into)?;
                curvilinear.walk(into)?;
                path.walk(into)
            }
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
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Generated { shape, turn: _ } => shape.walk(into),
        }
    }
}

impl Walk for Twist {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::Angle(f0) => f0.walk(into),
        }
    }
}
