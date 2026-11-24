// SPDX-License-Identifier: Apache-2.0
//! The walk over assembly operations (§6.7).

use cadmpeg_core::CodecError;

use crate::design::ops::assembly::{
    AssemblyContext, ComponentPattern, MirrorComponents, Place, PlaceSet, RigidGroup,
};

use super::{Visit, Walk};

impl Walk for AssemblyContext {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { participants } = self;
        participants.walk(into)
    }
}

impl Walk for ComponentPattern {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            seeds,
            layout,
            instances: _,
            native_options,
            source_settings,
        } = self;
        seeds.walk(into)?;
        layout.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for MirrorComponents {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            seeds,
            plane,
            results: _,
            native_options,
            source_settings,
        } = self;
        seeds.walk(into)?;
        plane.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for Place {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            occurrence: _,
            placement,
            scale,
            base: _,
            constraints,
            packaged: _,
            native_options,
            source_settings,
        } = self;
        placement.walk(into)?;
        scale.walk(into)?;
        constraints.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for PlaceSet {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            occurrences: _,
            native_options,
            source_settings,
        } = self;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for RigidGroup {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            members: _,
            native_options,
            source_settings,
        } = self;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}
