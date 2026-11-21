// SPDX-License-Identifier: Apache-2.0
//! The walk over the process records (part 7) that hold a selection, a
//! construction, or an expression: their picks name design entities, so
//! the spellings of §5.8 and §5.9 hold for them as for a design record.

use cadmpeg_core::CodecError;

use crate::process::feature::MachiningFeature;
use crate::process::groups::{GroupKind, ProcessDefinition, ProcessGroup, ProcessParameter};
use crate::process::operation::{
    GeometrySource, Operation, ProcessGeometry, ProfileCurve, ToolAxisControl,
};
use crate::process::setup::{Setup, WorkpieceSetup};
use crate::process::stock::{AxisSpec, CylinderOffsets, StockRule, Wcs, WcsOrientation, WcsOrigin};
use crate::process::technology::{TabPlacement, Tabs, Technology};
use crate::process::tool::Preset;
use crate::process::{Choice, ProcessConstruction, WorkpieceSelection};

use super::{Kinded, Visit, Walk};

impl<T: Walk> Walk for Choice<T> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            value,
            parameters: _,
        } = self;
        value.walk(into)
    }
}

impl Walk for WorkpieceSelection {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            workpiece: _,
            selection,
        } = self;
        selection.walk(into)
    }
}

impl<K: Kinded> Walk for ProcessConstruction<K> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            workpiece: _,
            construction,
        } = self;
        construction.walk(into)
    }
}

impl Walk for Setup {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            name: _,
            job: _,
            axes: _,
            steps: _,
            workpieces,
            fixtures,
            stock,
            wcs,
            work_offsets: _,
            machine: _,
            native_ref: _,
        } = self;
        workpieces.walk(into)?;
        fixtures.walk(into)?;
        stock.walk(into)?;
        wcs.walk(into)
    }
}

impl Walk for WorkpieceSetup {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            workpiece,
            placement: _,
        } = self;
        workpiece.walk(into)
    }
}

impl Walk for StockRule {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::RelativeCylinder(offsets) => offsets.walk(into),
            Self::Solid(selection) => selection.walk(into),
            Self::Relative(_) | Self::Fixed { .. } | Self::PreviousSetup(_) | Self::Native(_) => {
                Ok(())
            }
        }
    }
}

impl Walk for CylinderOffsets {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            axis,
            radial: _,
            front: _,
            back: _,
        } = self;
        axis.walk(into)
    }
}

impl Walk for Wcs {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            orientation,
            origin,
            placement: _,
        } = self;
        orientation.walk(into)?;
        origin.walk(into)
    }
}

impl Walk for WcsOrientation {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Axes {
                primary,
                secondary,
                flips: _,
            } => {
                primary.walk(into)?;
                secondary.walk(into)
            }
            Self::Frame(frame) => frame.walk(into),
            Self::Model | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for AxisSpec {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self { axis: _, direction } = self;
        direction.walk(into)
    }
}

impl Walk for WcsOrigin {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(point) => point.walk(into),
            Self::ModelOrigin
            | Self::FrameOrigin
            | Self::StockPoint(_)
            | Self::ModelPoint(_)
            | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Operation {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            name: _,
            comment: _,
            kind: _,
            native_kind: _,
            tooling: _,
            features: _,
            groups: _,
            geometry,
            heights: _,
            radii: _,
            axial: _,
            technology,
            tool_frame,
            tool_axis,
            leads: _,
            plunge: _,
            linking: _,
            cycle: _,
            suppressed: _,
            configuration_states: _,
            optional: _,
            native_ref: _,
        } = self;
        geometry.walk(into)?;
        technology.walk(into)?;
        tool_frame.walk(into)?;
        tool_axis.walk(into)
    }
}

impl Walk for ProcessGeometry {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            role: _,
            source,
            slot: _,
            combine: _,
        } = self;
        source.walk(into)
    }
}

impl Walk for GeometrySource {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Selection(selections) => selections.walk(into),
            Self::Points(points) => points.walk(into),
            Self::Direction(direction) => direction.walk(into),
            Self::Axis(axis) => axis.walk(into),
            Self::Profile { plane, curves } => {
                plane.walk(into)?;
                curves.walk(into)
            }
        }
    }
}

impl Walk for ProfileCurve {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            geometry: _,
            source,
        } = self;
        source.walk(into)
    }
}

impl Walk for ToolAxisControl {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::InPlane { plane } => plane.walk(into),
            Self::ThroughLine { line } => line.walk(into),
            Self::NormalToSurface { surfaces } | Self::AlongRuling { surfaces } => {
                surfaces.walk(into)
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Technology {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            feeds: _,
            spindle: _,
            spindle_direction: _,
            coolant: _,
            stepover: _,
            stepdown: _,
            stock_to_leave_radial: _,
            stock_to_leave_axial: _,
            tolerance: _,
            cut_mode: _,
            stepdown_rule: _,
            stepover_rule: _,
            multiple_depths: _,
            finishing_stepdown: _,
            finishing_stepover: _,
            compensation: _,
            repeat_final_pass: _,
            closed_pass_overlap: _,
            corners: _,
            roughing_passes: _,
            finishing_passes: _,
            pass_transition: _,
            reduce_feed: _,
            pass_angle: _,
            roughing_pass_count: _,
            tabs,
            chamfer: _,
            jet_cut: _,
            rest: _,
            helix_rise: _,
            thread_hand: _,
            thread_depth: _,
            thread_infeed_angle: _,
            angular_stepover: _,
            axial_offset: _,
            intermediate_stepdown: _,
            stepdown_count: _,
            bottom_up: _,
            turning_side: _,
        } = self;
        tabs.walk(into)
    }
}

impl Walk for Tabs {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            height: _,
            width: _,
            shape: _,
            placement,
        } = self;
        placement.walk(into)
    }
}

impl Walk for TabPlacement {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::AtPoints(points) => points.walk(into),
            Self::PerContour(_) | Self::Spacing(_) | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for Preset {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            tool: _,
            name: _,
            nc: _,
            technology,
            native_ref: _,
        } = self;
        technology.walk(into)
    }
}

impl Walk for MachiningFeature {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            name: _,
            class: _,
            native_kind: _,
            geometry,
            native_ref: _,
        } = self;
        geometry.walk(into)
    }
}

impl Walk for ProcessGroup {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            kind,
            name: _,
            parent: _,
            native_ref: _,
        } = self;
        kind.walk(into)
    }
}

impl Walk for GroupKind {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Geometry { geometry } => geometry.walk(into),
            Self::Method | Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for ProcessParameter {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            owner: _,
            name: _,
            kind: _,
            definition,
            value: _,
            native_ref: _,
        } = self;
        definition.walk(into)
    }
}

impl Walk for ProcessDefinition {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Expression(expression) => expression.walk(into),
            Self::Computed => Ok(()),
        }
    }
}
