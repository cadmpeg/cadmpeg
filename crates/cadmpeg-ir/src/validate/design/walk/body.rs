// SPDX-License-Identifier: Apache-2.0
//! The walk over patterns, template instances, and body operations (§6.5).

use cadmpeg_core::CodecError;

use crate::design::ops::body::{
    BodyMate, Combine, DeleteBodies, DeleteFaces, Extract, Flex, Graft, InstanceChange,
    InstanceOverride, Kept, MoveFaces, Pattern, PatternInstances, PatternSeeds, ReplaceFaces,
    ScaleFactors, Split, SplitMethod, SplitMode, SplitTool, TemplateInput, TemplateInstance,
    TransformBodies, TransformStep, Transformation,
};

use super::{Collect, Walk};

impl Walk for BodyMate {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            connectors,
            kind,
            alignment: _,
        } = self;
        connectors.walk(into)?;
        kind.walk(into)?;
        Ok(())
    }
}

impl Walk for Combine {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            target,
            tools,
            op: _,
            tools_after: _,
            new_component: _,
            native_options,
            source_settings,
        } = self;
        target.walk(into)?;
        tools.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for DeleteBodies {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bodies,
            retain: _,
            native_options,
            source_settings,
        } = self;
        bodies.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for DeleteFaces {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            heal: _,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Extract {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            source,
            reversed: _,
            native_options,
            source_settings,
        } = self;
        source.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Flex {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bodies,
            frame,
            trims: _,
            kind: _,
            native_options,
            source_settings,
        } = self;
        bodies.walk(into)?;
        frame.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Graft {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            body,
            sheet,
            flip: _,
            simplify: _,
            native_options,
            source_settings,
        } = self;
        body.walk(into)?;
        sheet.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for InstanceChange {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Varied { parameters } => parameters.walk(into),
            Self::Skipped => Ok(()),
        }
    }
}

impl Walk for InstanceOverride {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { index: _, change } = self;
        change.walk(into)?;
        Ok(())
    }
}

impl<S> Walk for Kept<S> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Pieces { keep, remove } => {
                keep.walk(into)?;
                remove.walk(into)?;
                Ok(())
            }
            Self::All { form: _ } | Self::Side { side: _ } | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for MoveFaces {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            transformation,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        transformation.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Pattern {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            seeds,
            layout,
            instances,
            native_options,
            source_settings,
        } = self;
        seeds.walk(into)?;
        layout.walk(into)?;
        instances.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for PatternInstances {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Implicit(f0) => f0.walk(into),
            Self::Members(_) => Ok(()),
        }
    }
}

impl Walk for PatternSeeds {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Features {
                features,
                method: _,
            } => features.walk(into),
            Self::Faces(f0) => f0.walk(into),
            Self::Bodies { bodies, join: _ } => bodies.walk(into),
        }
    }
}

impl Walk for ReplaceFaces {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            faces,
            replacement,
            native_options,
            source_settings,
        } = self;
        faces.walk(into)?;
        replacement.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for ScaleFactors {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::PerAxis { factors: _, frame } => frame.walk(into),
            Self::Uniform(_) => Ok(()),
        }
    }
}

impl Walk for Split {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            mode,
            native_options,
            source_settings,
        } = self;
        mode.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for SplitMethod {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Project(f0) => f0.walk(into),
            Self::Intersect => Ok(()),
        }
    }
}

impl Walk for SplitMode {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::ByTools {
                targets,
                tools,
                method,
                kept,
            } => {
                targets.walk(into)?;
                tools.walk(into)?;
                method.walk(into)?;
                kept.walk(into)?;
                Ok(())
            }
            Self::Mutual { elements, kept } => {
                elements.walk(into)?;
                kept.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for SplitTool {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Plane(f0) => f0.walk(into),
            Self::Geometry(f0) => f0.walk(into),
        }
    }
}

impl Walk for TemplateInput {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { role: _, reference } = self;
        reference.walk(into)?;
        Ok(())
    }
}

impl Walk for TemplateInstance {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            template: _,
            inputs,
            parameters: _,
            members: _,
            native_options,
            source_settings,
        } = self;
        inputs.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for TransformBodies {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            bodies,
            transformation,
            mode: _,
            native_options,
            source_settings,
        } = self;
        bodies.walk(into)?;
        transformation.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for TransformStep {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Translate {
                direction,
                distance: _,
            } => direction.walk(into),
            Self::Components { frame, delta: _ } => frame.walk(into),
            Self::PointToPoint { from, to } => {
                from.walk(into)?;
                to.walk(into)?;
                Ok(())
            }
            Self::Rotate { axis, angle: _ } => axis.walk(into),
            Self::FrameToFrame { from, to } => {
                from.walk(into)?;
                to.walk(into)?;
                Ok(())
            }
            Self::Scale { center, factors } => {
                center.walk(into)?;
                factors.walk(into)?;
                Ok(())
            }
            Self::Constrained { mates } => mates.walk(into),
            Self::Fixed(_) | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Transformation {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { steps, resolved: _ } = self;
        steps.walk(into)?;
        Ok(())
    }
}
