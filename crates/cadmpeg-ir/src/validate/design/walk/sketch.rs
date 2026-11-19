// SPDX-License-Identifier: Apache-2.0
//! The walk over sketches (§6.1).

use cadmpeg_core::CodecError;

use crate::design::ops::sketch::{
    AngleOperands, Derivation, DerivationMode, DistanceAxis, EntityDefinition, EntityForm, Operand,
    Sketch, SketchConstraint, SketchEntity, SketchPattern, SketchPatternAxis,
    SketchPatternDirection, SketchPatternLayout, SketchPlane, SketchRelation, SketchSpace,
    SpatialSketch,
};

use super::{Collect, Walk};

impl<S: SketchSpace> Walk for AngleOperands<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Between(f0) => f0.walk(into),
            Self::Sweep(_) | Self::Points(_) => Ok(()),
        }
    }
}

impl Walk for Derivation {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            mode,
            source,
            piece: _,
            link: _,
        } = self;
        mode.walk(into)?;
        source.walk(into)?;
        Ok(())
    }
}

impl Walk for DerivationMode {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::OntoFaces { faces, direction } => {
                faces.walk(into)?;
                direction.walk(into)?;
                Ok(())
            }
            Self::Project
            | Self::Include
            | Self::Intersect
            | Self::Silhouette
            | Self::Offset { distance: _ }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for DistanceAxis<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along(f0) => f0.walk(into),
            Self::Aligned | Self::Horizontal | Self::Vertical => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for EntityDefinition<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Defined { form, solved: _ } => form.walk(into),
            Self::Free(_) => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for EntityForm<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Derived(f0) => f0.walk(into),
            Self::Interpolated(_)
            | Self::Conic { rho: _ }
            | Self::Text(_)
            | Self::BlockInstance(_)
            | Self::Instanced { instance: _, of: _ }
            | Self::Frame
            | Self::Native(_) => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for Operand<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Model(f0) => f0.walk(into),
            Self::Entity(_) | Self::Frame(_) => Ok(()),
        }
    }
}

impl Walk for Sketch {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            owner: _,
            name: _,
            plane,
            solve: _,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        plane.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl<S: SketchSpace> Walk for SketchConstraint<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            sketch: _,
            relation,
            name: _,
            active: _,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        relation.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl<S: SketchSpace> Walk for SketchEntity<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            sketch: _,
            definition,
            roles: _,
            construction: _,
            centerline: _,
            native: _,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        definition.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl<S: SketchSpace> Walk for SketchPattern<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            seeds: _,
            layout,
            instances: _,
        } = self;
        layout.walk(into)?;
        Ok(())
    }
}

impl<S: SketchSpace> Walk for SketchPatternAxis<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            direction,
            spacing,
            sides,
        } = self;
        direction.walk(into)?;
        spacing.walk(into)?;
        sides.walk(into)?;
        Ok(())
    }
}

impl<S: SketchSpace> Walk for SketchPatternDirection<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along { line, reversed: _ } => line.walk(into),
            Self::Angle(_) => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for SketchPatternLayout<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Linear { directions } => directions.walk(into),
            Self::Circular {
                center,
                spacing,
                sides,
            } => {
                center.walk(into)?;
                spacing.walk(into)?;
                sides.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for SketchPlane {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Placed(f0) => f0.walk(into),
            Self::Block => Ok(()),
        }
    }
}

impl<S: SketchSpace> Walk for SketchRelation<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Coincident { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Concentric { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Collinear { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Parallel { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Perpendicular { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Tangent { a, b, contact: _ } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Curvature { a, b, contact: _ } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Horizontal { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Vertical { a, b } => {
                a.walk(into)?;
                b.walk(into)?;
                Ok(())
            }
            Self::Midpoint { point: _, of } => of.walk(into),
            Self::Symmetric { a, b, about } => {
                a.walk(into)?;
                b.walk(into)?;
                about.walk(into)?;
                Ok(())
            }
            Self::Equal { members } => members.walk(into),
            Self::EqualDistance { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::Fixed { of } => of.walk(into),
            Self::Distance {
                a,
                b,
                along,
                doubled: _,
                value: _,
            } => {
                a.walk(into)?;
                b.walk(into)?;
                along.walk(into)?;
                Ok(())
            }
            Self::Angle {
                between,
                sector: _,
                value: _,
            } => between.walk(into),
            Self::Pattern(f0) => f0.walk(into),
            Self::Pierce { point: _, curve } => curve.walk(into),
            Self::Native {
                kind: _,
                operands,
                values: _,
            } => operands.walk(into),
            Self::Radial {
                of: _,
                measure: _,
                value: _,
            }
            | Self::Length { of: _, value: _ }
            | Self::Offset(_)
            | Self::Polygon(_)
            | Self::Slot(_)
            | Self::Weight { pole: _, value: _ }
            | Self::FixedLength { of: _ }
            | Self::FixedAngle { of: _ } => Ok(()),
        }
    }
}

impl Walk for SpatialSketch {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            owner: _,
            name: _,
            solve: _,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}
