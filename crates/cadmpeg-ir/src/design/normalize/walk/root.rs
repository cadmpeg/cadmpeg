// SPDX-License-Identifier: Apache-2.0
//! The walk over the design root records, configurations, parameters, programs, and the shared option types.

use cadmpeg_core::CodecError;

use crate::design::configuration::{
    Arrangement, ArrangementMove, DesignConfiguration, FeatureStateOverride, MaterialOverride,
    Overrides, ParameterOverride, Publication,
};
use crate::design::construction::SourceMode;
use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, Link, StoredOrigin};
use crate::design::options::{
    Boolean, NativeOption, ProjectionDirection, SectionCondition, SheetOutput, SolidOutput,
    Support, Targets,
};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::program::{ParameterProgram, ProgramBody, Statement};
use crate::design::scope::{DesignScope, OccurrenceDesign};

use super::{Visit, Walk};

impl Walk for Arrangement {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            name: _,
            kind: _,
            configuration: _,
            overrides,
            appearances: _,
            moves,
            nested: _,
            native_ref: _,
        } = self;
        overrides.walk(into)?;
        moves.walk(into)?;
        Ok(())
    }
}

impl Walk for ArrangementMove {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            occurrences: _,
            step,
        } = self;
        step.walk(into)?;
        Ok(())
    }
}

impl Walk for DesignConfiguration {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            name: _,
            position: _,
            parent: _,
            comment: _,
            overrides,
            active_arrangements: _,
            materials,
            native_ref: _,
        } = self;
        overrides.walk(into)?;
        materials.walk(into)?;
        Ok(())
    }
}

impl Walk for FeatureStateOverride {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            suppressed: _,
            definition,
        } = self;
        definition.walk(into)?;
        Ok(())
    }
}

impl Walk for MaterialOverride {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            target,
            material: _,
        } = self;
        target.walk(into)?;
        Ok(())
    }
}

impl Walk for Overrides {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            parameters,
            features,
            occurrences: _,
            definitions: _,
        } = self;
        parameters.walk(into)?;
        features.walk(into)?;
        Ok(())
    }
}

impl Walk for ParameterOverride {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            expression,
            value: _,
        } = self;
        expression.walk(into)?;
        Ok(())
    }
}

impl Walk for Publication {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            name: _,
            target,
        } = self;
        target.walk(into)?;
        Ok(())
    }
}

impl Walk for SourceMode {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            codec: _,
            method: _,
            native_options,
            source_settings,
        } = self;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Container {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Ordered {
                id: _,
                kind: _,
                members: _,
                folders: _,
                rollback: _,
                placement,
            } => placement.walk(into),
            Self::Unordered {
                id: _,
                kind: _,
                members: _,
            } => Ok(()),
        }
    }
}

impl Walk for Feature {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            name: _,
            suppressed: _,
            definition,
            placement,
            component: _,
            driven_by: _,
            native_ref: _,
        } = self;
        definition.walk(into)?;
        placement.walk(into)?;
        Ok(())
    }
}

impl Walk for FeatureDefinition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            operation,
            assembly,
            refine: _,
            fuzzy_tolerance: _,
        } = self;
        operation.walk(into)?;
        assembly.walk(into)?;
        Ok(())
    }
}

impl Walk for FeatureOperation {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("FeatureOperation")?;
        match self {
            Self::Datum(f0) => f0.walk(into),
            Self::Extrude(f0) => f0.walk(into),
            Self::Revolve(f0) => f0.walk(into),
            Self::Sweep(f0) => f0.walk(into),
            Self::HelicalSweep(f0) => f0.walk(into),
            Self::Rib(f0) => f0.walk(into),
            Self::Primitive(f0) => f0.walk(into),
            Self::Loft(f0) => f0.walk(into),
            Self::Fillet(f0) => f0.walk(into),
            Self::Chamfer(f0) => f0.walk(into),
            Self::Shell(f0) => f0.walk(into),
            Self::Draft(f0) => f0.walk(into),
            Self::Thicken(f0) => f0.walk(into),
            Self::Offset(f0) => f0.walk(into),
            Self::Hole(f0) => f0.walk(into),
            Self::Thread(f0) => f0.walk(into),
            Self::Emboss(f0) => f0.walk(into),
            Self::Dome(f0) => f0.walk(into),
            Self::Decal(f0) => f0.walk(into),
            Self::Pattern(f0) => f0.walk(into),
            Self::TemplateInstance(f0) => f0.walk(into),
            Self::Combine(f0) => f0.walk(into),
            Self::Split(f0) => f0.walk(into),
            Self::DeleteBodies(f0) => f0.walk(into),
            Self::Extract(f0) => f0.walk(into),
            Self::TransformBodies(f0) => f0.walk(into),
            Self::Flex(f0) => f0.walk(into),
            Self::MoveFaces(f0) => f0.walk(into),
            Self::DeleteFaces(f0) => f0.walk(into),
            Self::ReplaceFaces(f0) => f0.walk(into),
            Self::Graft(f0) => f0.walk(into),
            Self::Patch(f0) => f0.walk(into),
            Self::Knit(f0) => f0.walk(into),
            Self::Unsew(f0) => f0.walk(into),
            Self::UnsewFaces(f0) => f0.walk(into),
            Self::Extend(f0) => f0.walk(into),
            Self::ImplicitSweep(f0) => f0.walk(into),
            Self::BoundaryFill(f0) => f0.walk(into),
            Self::Curve(f0) => f0.walk(into),
            Self::Form(f0) => f0.walk(into),
            Self::SheetMetal(f0) => f0.walk(into),
            Self::Mesh(f0) => f0.walk(into),
            Self::Place(f0) => f0.walk(into),
            Self::PlaceSet(f0) => f0.walk(into),
            Self::Joint(f0) => f0.walk(into),
            Self::RigidGroup(f0) => f0.walk(into),
            Self::ComponentPattern(f0) => f0.walk(into),
            Self::MirrorComponents(f0) => f0.walk(into),
            Self::Stored {
                bodies: _,
                tessellations: _,
                origin,
            } => origin.walk(into),
            Self::Native {
                kind: _,
                native_options,
                source_settings,
            } => {
                native_options.walk(into)?;
                source_settings.walk(into)?;
                Ok(())
            }
            Self::Sketch(_)
            | Self::SpatialSketch(_)
            | Self::Spreadsheet(_)
            | Self::MaterialRemoval
            | Self::Unresolved { family: _ } => Ok(()),
        }
    }
}

impl Walk for Link {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            source,
            document: _,
            transform: _,
            scale: _,
            associative: _,
            state: _,
            native_ref: _,
        } = self;
        source.walk(into)?;
        Ok(())
    }
}

impl Walk for StoredOrigin {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Linked(f0) => f0.walk(into),
            Self::Base {} | Self::Imported {} | Self::Snapshot {} => Ok(()),
        }
    }
}

impl Walk for Boolean {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Apply { op: _, targets } => targets.walk(into),
            Self::NewBody => Ok(()),
        }
    }
}

impl Walk for NativeOption {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Selection(f0) => f0.walk(into),
            Self::OrderedSelection(f0) => f0.walk(into),
            Self::Bool(_)
            | Self::Integer(_)
            | Self::Real(_)
            | Self::Length(_)
            | Self::Angle(_)
            | Self::Text(_)
            | Self::Blank(_) => Ok(()),
        }
    }
}

impl Walk for ProjectionDirection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Along(f0) => f0.walk(into),
            Self::SurfaceNormal | Self::SketchNormal | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for SectionCondition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Free { support } => support.walk(into),
            Self::Direction {
                direction,
                angle: _,
                weight: _,
            } => direction.walk(into),
            Self::Tangent {
                support,
                weight: _,
                flip: _,
            } => support.walk(into),
            Self::Curvature {
                support,
                weight: _,
                flip: _,
            } => support.walk(into),
            Self::Default
            | Self::NormalToProfile {
                angle: _,
                weight: _,
            }
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for SheetOutput {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { solid: _, boolean } = self;
        boolean.walk(into)?;
        Ok(())
    }
}

impl Walk for SolidOutput {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { body: _, boolean } = self;
        boolean.walk(into)?;
        Ok(())
    }
}

impl Walk for Support {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces(f0) => f0.walk(into),
            Self::Computed => Ok(()),
        }
    }
}

impl Walk for Targets {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Selected(f0) => f0.walk(into),
            Self::All => Ok(()),
        }
    }
}

impl Walk for DesignParameter {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            owner: _,
            position: _,
            name: _,
            kind: _,
            unit: _,
            definition,
            value: _,
            display: _,
            tolerance: _,
            decimals: _,
            comment: _,
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

impl Walk for ParameterDefinition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Expression(f0) => f0.walk(into),
            Self::Driven | Self::Assigned(_) | Self::Unset => Ok(()),
        }
    }
}

impl Walk for ParameterProgram {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            owner: _,
            stage: _,
            name: _,
            body,
            native_ref: _,
        } = self;
        body.walk(into)?;
        Ok(())
    }
}

impl Walk for ProgramBody {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Neutral(f0) => f0.walk(into),
            Self::Source {
                dialect: _,
                tokens: _,
            } => Ok(()),
        }
    }
}

impl Walk for Statement {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("Statement")?;
        match self {
            Self::Assign { target: _, value } => value.walk(into),
            Self::If {
                condition,
                then,
                otherwise,
            } => {
                condition.walk(into)?;
                then.walk(into)?;
                otherwise.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for DesignScope {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            definition: _,
            roots: _,
            active_configuration: _,
            active_arrangements: _,
            occurrences,
            current: _,
            unproduced: _,
        } = self;
        occurrences.walk(into)?;
        Ok(())
    }
}

impl Walk for OccurrenceDesign {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            configuration: _,
            arrangements: _,
            grounded: _,
            flexible: _,
            parameters,
        } = self;
        parameters.walk(into)?;
        Ok(())
    }
}
