// SPDX-License-Identifier: Apache-2.0
//! The walk over surfacing, wireframe, forms, and meshes (§6.6).

use cadmpeg_core::CodecError;

use crate::design::ops::surface::{
    BoundaryFill, BridgeEnd, CircleMode, CombineOperand, CurveDefinition, CurveFeature,
    CurveOffset, EquationCoordinates, Extend, ExtendLimit, FaceReference, Form, Healing,
    ImplicitSection, ImplicitSweep, Knit, LineLength, LineMode, MeshFeature, MeshKind, Patch,
    PatchBounds, PatchEdge, ReduceTarget, SplineCurve, SplinePoint, TextCurve, TextPlacement,
    Unsew, UnsewFaces,
};

use super::{Collect, Walk};

impl Walk for BoundaryFill {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            tools,
            cells,
            regions: _,
            cap_planar,
            merge,
            tools_after: _,
            boolean,
            native_options,
            source_settings,
        } = self;
        cap_planar.walk(into)?;
        merge.walk(into)?;
        tools.walk(into)?;
        cells.walk(into)?;
        boolean.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for BridgeEnd {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            curve,
            at,
            continuity: _,
            tension,
            reversed,
        } = self;
        tension.walk(into)?;
        reversed.walk(into)?;
        curve.walk(into)?;
        at.walk(into)?;
        Ok(())
    }
}

impl Walk for CircleMode {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::ThreeGuides { guides } => guides.walk(into),
            Self::TwoGuidesRadius { guides, radius } => {
                guides.walk(into)?;
                radius.walk(into)?;
                Ok(())
            }
            Self::CenterAngles {
                center,
                reference,
                from: _,
                angles,
            } => {
                center.walk(into)?;
                reference.walk(into)?;
                angles.walk(into)?;
                Ok(())
            }
            Self::CenterRadius { center, radius } => {
                center.walk(into)?;
                radius.walk(into)?;
                Ok(())
            }
            Self::TangentTwoGuides {
                passing,
                limit,
                surface,
            } => {
                passing.walk(into)?;
                limit.walk(into)?;
                surface.walk(into)?;
                Ok(())
            }
            Self::TangentGuideRadius {
                passing,
                surface,
                radius,
                trim,
            } => {
                trim.walk(into)?;
                passing.walk(into)?;
                surface.walk(into)?;
                radius.walk(into)?;
                Ok(())
            }
            Self::TangentLimitRadius {
                limit,
                surface,
                radius,
                angles,
            } => {
                limit.walk(into)?;
                surface.walk(into)?;
                radius.walk(into)?;
                angles.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for CombineOperand {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { curve, direction } = self;
        curve.walk(into)?;
        direction.walk(into)?;
        Ok(())
    }
}

impl Walk for CurveDefinition {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Spline(f0) => f0.walk(into),
            Self::Helix(f0) => f0.walk(into),
            Self::Projection {
                source,
                onto,
                direction,
            } => {
                source.walk(into)?;
                onto.walk(into)?;
                direction.walk(into)?;
                Ok(())
            }
            Self::Intersection { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Bridge { ends } => ends.walk(into),
            Self::Text(f0) => f0.walk(into),
            Self::Constructed(f0) => f0.walk(into),
            Self::Corner { arc, trim } => {
                trim.walk(into)?;
                arc.walk(into)
            }
            Self::Polygon {
                placement,
                sides,
                circumradius,
            } => {
                sides.walk(into)?;
                circumradius.walk(into)?;
                placement.walk(into)
            }
            Self::Point(f0) => f0.walk(into),
            Self::Equation { frame, coordinates } => {
                frame.walk(into)?;
                coordinates.walk(into)?;
                Ok(())
            }
            Self::Combined {
                first,
                second,
                nearest,
            } => {
                nearest.walk(into)?;
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Offset {
                curve,
                support,
                offset,
                measure: _,
                corner: _,
                smoothing: _,
            } => {
                curve.walk(into)?;
                support.walk(into)?;
                offset.walk(into)?;
                Ok(())
            }
            Self::Smoothed {
                curve,
                max_deviation,
                tangency_threshold,
                curvature_ratio,
                continuity: _,
            } => {
                max_deviation.walk(into)?;
                tangency_threshold.walk(into)?;
                curvature_ratio.walk(into)?;
                curve.walk(into)
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for CurveFeature {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            definition,
            native_options,
            source_settings,
        } = self;
        definition.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for CurveOffset {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::Through(f0) => f0.walk(into),
        }
    }
}

impl Walk for EquationCoordinates {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Cartesian { x, y, z } => {
                x.walk(into)?;
                y.walk(into)?;
                z.walk(into)?;
                Ok(())
            }
            Self::Cylindrical { r, theta, z } => {
                r.walk(into)?;
                theta.walk(into)?;
                z.walk(into)?;
                Ok(())
            }
            Self::Spherical { rho, theta, phi } => {
                rho.walk(into)?;
                theta.walk(into)?;
                phi.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Extend {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            limit,
            method: _,
            output,
            native_options,
            source_settings,
        } = self;
        edges.walk(into)?;
        limit.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ExtendLimit {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpTo(f0) => f0.walk(into),
            Self::Distance(f0) => f0.walk(into),
            Self::Natural | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for FaceReference {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces(f0) => f0.walk(into),
            Self::Adjacent { alternate } => alternate.walk(into),
        }
    }
}

impl Walk for Form {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            cage: _,
            native_options,
            source_settings,
        } = self;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Healing {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            objective,
            tangency,
            tangency_objective,
            frozen,
        } = self;
        objective.walk(into)?;
        tangency.walk(into)?;
        tangency_objective.walk(into)?;
        frozen.walk(into)?;
        Ok(())
    }
}

impl Walk for ImplicitSection {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Line { mode, lengths } => {
                mode.walk(into)?;
                lengths.walk(into)?;
                Ok(())
            }
            Self::Circle(f0) => f0.walk(into),
            Self::Conic {
                guides,
                supports,
                parameter,
            } => {
                guides.walk(into)?;
                supports.walk(into)?;
                parameter.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ImplicitSweep {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            section,
            spine,
            limits,
            solution: _,
            corner: _,
            output,
            native_options,
            source_settings,
        } = self;
        section.walk(into)?;
        spine.walk(into)?;
        limits.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Knit {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            elements,
            tolerance,
            simplify,
            healing,
            federation,
            output,
            native_options,
            source_settings,
        } = self;
        tolerance.walk(into)?;
        simplify.walk(into)?;
        elements.walk(into)?;
        healing.walk(into)?;
        federation.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for LineLength {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Law(f0) => f0.walk(into),
            Self::UpTo(f0) => f0.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for LineMode {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::TwoGuides { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Middle { guide, middle } => {
                guide.walk(into)?;
                middle.walk(into)?;
                Ok(())
            }
            Self::Surface {
                guide,
                reference,
                from: _,
                angle,
            } => {
                guide.walk(into)?;
                reference.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Curve {
                guide,
                curve,
                from: _,
                angle,
            } => {
                guide.walk(into)?;
                curve.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Direction {
                guide,
                direction,
                from: _,
                angle,
            } => {
                guide.walk(into)?;
                direction.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::TangentTo { guide, surface } => {
                guide.walk(into)?;
                surface.walk(into)?;
                Ok(())
            }
            Self::TwoTangents { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for MeshFeature {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            source,
            kind,
            native_options,
            source_settings,
        } = self;
        source.walk(into)?;
        kind.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for MeshKind {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::PlaneCut {
                plane,
                cut: _,
                fill: _,
            } => plane.walk(into),
            Self::Combine { tools, op: _ } => tools.walk(into),
            Self::Reduce { target } => target.walk(into),
            Self::Remesh { edge_length, shape } => {
                edge_length.walk(into)?;
                shape.walk(into)?;
                Ok(())
            }
            Self::Smooth { smoothness } => smoothness.walk(into),
            Self::Section { sketch: _ }
            | Self::Repair
            | Self::Remove
            | Self::ReverseNormals
            | Self::Convert { method: _ }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Patch {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bounds,
            passing,
            planar,
            simplified,
            output,
            native_options,
            source_settings,
        } = self;
        planar.walk(into)?;
        simplified.walk(into)?;
        bounds.walk(into)?;
        passing.walk(into)?;
        output.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for PatchBounds {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Loops { outer, inner } => {
                outer.walk(into)?;
                inner.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for PatchEdge {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { curve, condition } = self;
        curve.walk(into)?;
        condition.walk(into)?;
        Ok(())
    }
}

impl Walk for ReduceTarget {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Deviation(f0) => f0.walk(into),
            Self::Proportion(f0) => f0.walk(into),
            Self::FaceCount(f0) => f0.walk(into),
        }
    }
}

impl Walk for SplineCurve {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            points,
            closed,
            support,
        } = self;
        closed.walk(into)?;
        points.walk(into)?;
        support.walk(into)?;
        Ok(())
    }
}

impl Walk for SplinePoint {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            point,
            tangent,
            tension,
        } = self;
        tension.walk(into)?;
        point.walk(into)?;
        tangent.walk(into)?;
        Ok(())
    }
}

impl Walk for TextCurve {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { text, placement } = self;
        text.walk(into)?;
        placement.walk(into)?;
        Ok(())
    }
}

impl Walk for TextPlacement {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Plane(f0) => f0.walk(into),
            Self::OnCurve { path } => path.walk(into),
            Self::OnFace { faces, path } => {
                faces.walk(into)?;
                path.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for Unsew {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            native_options,
            source_settings,
        } = self;
        edges.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for UnsewFaces {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}
