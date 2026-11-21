// SPDX-License-Identifier: Apache-2.0
//! The walk over sheet metal (§6.6).

use cadmpeg_core::CodecError;

use crate::design::ops::sheet_metal::{
    AngleRange, BendAllowance, BendEquations, BendSet, ConnectBetween, Convert, ConvertBend,
    ConvertFrom, ConvertRip, Flange, FlangeHeight, FlangeWidth, Hem, Rip, RipPath, SheetMetal,
    SheetMetalKind, SheetMetalRule, StampForm, StampOutline, VertexBend,
};

use super::{Collect, Walk};

impl Walk for AngleRange {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            from: _,
            from_inclusive: _,
            to: _,
            to_inclusive: _,
            value,
        } = self;
        value.walk(into)?;
        Ok(())
    }
}

impl Walk for BendAllowance {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Equations(f0) => f0.walk(into),
            Self::KFactor(_) | Self::KFactorTable(_) | Self::Table(_) => Ok(()),
        }
    }
}

impl Walk for BendEquations {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            yields: _,
            angle: _,
            ranges,
        } = self;
        ranges.walk(into)?;
        Ok(())
    }
}

impl Walk for BendSet {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Selected(f0) => f0.walk(into),
            Self::All => Ok(()),
        }
    }
}

impl Walk for ConnectBetween {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Apart(f0) => f0.walk(into),
            Self::SharedEdge(f0) => f0.walk(into),
        }
    }
}

impl Walk for Convert {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            body,
            fixed,
            thickness: _,
            from,
            rips,
            bend: _,
            keep_solid: _,
            rule: _,
        } = self;
        body.walk(into)?;
        fixed.walk(into)?;
        from.walk(into)?;
        rips.walk(into)?;
        Ok(())
    }
}

impl Walk for ConvertBend {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Edge { edge, radius: _ } => edge.walk(into),
            Self::Face { face } => face.walk(into),
        }
    }
}

impl Walk for ConvertFrom {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces { side: _, bends } => bends.walk(into),
            Self::ThinSolid => Ok(()),
        }
    }
}

impl Walk for ConvertRip {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { path, gap: _ } = self;
        path.walk(into)?;
        Ok(())
    }
}

impl Walk for Flange {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            height,
            angle: _,
            width,
            bend: _,
            rule: _,
            height_datum: _,
            position: _,
            outline,
        } = self;
        edges.walk(into)?;
        height.walk(into)?;
        width.walk(into)?;
        outline.walk(into)?;
        Ok(())
    }
}

impl Walk for FlangeHeight {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::UpTo { target, offset: _ } => target.walk(into),
            Self::Distance(_) => Ok(()),
        }
    }
}

impl Walk for FlangeWidth {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::FromReference {
                from,
                offset: _,
                width: _,
                reversed: _,
            } => from.walk(into),
            Self::Full
            | Self::Centered { width: _ }
            | Self::FromEnds {
                first: _,
                second: _,
            }
            | Self::PerEdge(_)
            | Self::CenteredPerEdge(_) => Ok(()),
        }
    }
}

impl Walk for Hem {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            form: _,
            side: _,
            position: _,
            rule: _,
        } = self;
        edges.walk(into)?;
        Ok(())
    }
}

impl Walk for Rip {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces { faces } => faces.walk(into),
            Self::Along { path, width: _ } => path.walk(into),
        }
    }
}

impl Walk for RipPath {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Ruling { face, at } => {
                face.walk(into)?;
                at.walk(into)?;
                Ok(())
            }
            Self::Line { face, from, to } => {
                face.walk(into)?;
                from.walk(into)?;
                to.walk(into)?;
                Ok(())
            }
            Self::Edges(f0) => f0.walk(into),
            Self::Sketched {
                profile,
                through: _,
            } => profile.walk(into),
        }
    }
}

impl Walk for SheetMetal {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            kind,
            native_options,
            source_settings,
        } = self;
        kind.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for SheetMetalKind {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::AttachedPlate {
                profile,
                thickness: _,
                side: _,
                attachment,
            } => {
                profile.walk(into)?;
                attachment.walk(into)
            }
            Self::AttachedBend {
                plate: _,
                attachment,
                inside_radius: _,
                relief: _,
                transition_form: _,
                transition_radius: _,
            } => attachment.walk(into),
            Self::UnchangedCorner { bend: _ } => Ok(()),
            Self::ApplyRule { rule: _, bodies } => bodies.walk(into),
            Self::Wall {
                profile,
                thickness: _,
                side: _,
                rule: _,
            } => profile.walk(into),
            Self::ContourFlange {
                profile,
                edges,
                width,
                extent,
                thickness: _,
                bend: _,
                vertex_bends,
                rule: _,
            } => {
                profile.walk(into)?;
                edges.walk(into)?;
                width.walk(into)?;
                extent.walk(into)?;
                vertex_bends.walk(into)?;
                Ok(())
            }
            Self::Flange(f0) => f0.walk(into),
            Self::Hem(f0) => f0.walk(into),
            Self::Bend {
                line,
                angle: _,
                radius: _,
                radius_side: _,
                flat_length: _,
                zone: _,
                line_face: _,
                fixed_side: _,
            } => line.walk(into),
            Self::Connect {
                between,
                radius: _,
                extension: _,
                double: _,
            } => between.walk(into),
            Self::Rip(f0) => f0.walk(into),
            Self::Convert(f0) => f0.walk(into),
            Self::BreakCorner {
                corners,
                size: _,
                shape: _,
            } => corners.walk(into),
            Self::ExtendWall { edge, to } => {
                edge.walk(into)?;
                to.walk(into)?;
                Ok(())
            }
            Self::ExtrudedHole {
                bore,
                entry,
                height: _,
                root_fillet: _,
                rule: _,
            } => {
                bore.walk(into)?;
                entry.walk(into)?;
                Ok(())
            }
            Self::Stamp {
                entry,
                form,
                rule: _,
            } => {
                entry.walk(into)?;
                form.walk(into)?;
                Ok(())
            }
            Self::ToolForm {
                tool: _,
                placement,
                constraints,
                rule: _,
            } => {
                placement.walk(into)?;
                constraints.walk(into)?;
                Ok(())
            }
            Self::Unfold { fixed, bends } => {
                fixed.walk(into)?;
                bends.walk(into)?;
                Ok(())
            }
            Self::Refold { bends } => bends.walk(into),
            Self::FlatPattern { fixed } => fixed.walk(into),
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for SheetMetalRule {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            id: _,
            scope: _,
            name: _,
            thickness: _,
            bend: _,
            allowance,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        allowance.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for StampForm {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Raised {
                outline,
                height: _,
                wall_angle: _,
                rim_radius: _,
                floor_radius: _,
            } => outline.walk(into),
            Self::Bridge {
                placement,
                height: _,
                wall_angle: _,
                length: _,
                width: _,
                rim_radius: _,
                floor_radius: _,
                relief_length: _,
            } => placement.walk(into),
            Self::Louver {
                profile,
                open_edge,
                depth: _,
                flap_angle: _,
                wall_angle: _,
                rim_radius: _,
            } => {
                profile.walk(into)?;
                open_edge.walk(into)?;
                Ok(())
            }
            Self::Bead {
                path,
                height: _,
                section_radius: _,
                end_radius: _,
            } => path.walk(into),
            Self::StiffeningRib {
                bend,
                at,
                length: _,
                bottom_radius: _,
                wall_angle: _,
                rim_radius: _,
            } => {
                bend.walk(into)?;
                at.walk(into)?;
                Ok(())
            }
        }
    }
}

impl Walk for StampOutline {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Profile(f0) => f0.walk(into),
            Self::Circle {
                center,
                diameter: _,
            } => center.walk(into),
        }
    }
}

impl Walk for VertexBend {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            vertex,
            radius: _,
            radius_side: _,
        } = self;
        vertex.walk(into)?;
        Ok(())
    }
}
