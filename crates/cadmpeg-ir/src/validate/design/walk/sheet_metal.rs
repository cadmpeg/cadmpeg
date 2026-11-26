// SPDX-License-Identifier: Apache-2.0
//! The walk over sheet metal (§6.6).

use cadmpeg_core::CodecError;

use crate::design::ops::sheet_metal::{
    AngleRange, BendAllowance, BendEquations, BendQuantity, BendSet, BendSpec, BendTable,
    BendTableLookup, ConnectBetween, Convert, ConvertBend, ConvertFrom, ConvertRip, Flange,
    FlangeHeight, FlangeWidth, Hem, HemForm, KFactorRow, KFactorTable, OutsideGrid, Relief, Rip,
    RipPath, RipWidth, SheetMetal, SheetMetalKind, SheetMetalRule, StampForm, StampOutline,
    VertexBend,
};

use super::{Collect, Walk};
use crate::design::units::Quantity;

impl Walk for BendAllowance {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Equations(f0) => f0.walk(into),
            Self::KFactor(f0) => f0.walk(into),
            Self::KFactorTable(f0) => f0.walk(into),
            Self::Table(f0) => f0.walk(into),
        }
    }
}

impl Walk for BendEquations {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            yields,
            angle: _,
            ranges,
        } = self;
        let quantity = match yields {
            BendQuantity::Compensation { .. }
            | BendQuantity::Deduction { .. }
            | BendQuantity::Allowance => Some(Quantity::LENGTH),
            BendQuantity::KFactor => Some(Quantity::REAL),
            BendQuantity::Native(_) => None,
        };
        for range in ranges {
            let AngleRange {
                from: _,
                from_inclusive: _,
                to: _,
                to_inclusive: _,
                value,
            } = range;
            into.push_expression(super::ExpressionPlace::BendAllowance(quantity), value);
            value.walk(into)?;
        }
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

impl Walk for BendSpec {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            radius,
            radius_side: _,
            relief,
        } = self;
        radius.walk(into)?;
        relief.walk(into)?;
        Ok(())
    }
}

impl Walk for BendTable {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            blocks: _,
            angle: _,
            value: _,
            lookup,
        } = self;
        lookup.walk(into)?;
        Ok(())
    }
}

impl Walk for BendTableLookup {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            thickness: _,
            radius: _,
            angle: _,
            outside,
        } = self;
        outside.walk(into)?;
        Ok(())
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
            thickness,
            from,
            rips,
            bend,
            keep_solid: _,
            rule: _,
        } = self;
        thickness.walk(into)?;
        bend.walk(into)?;
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
            Self::Edge { edge, radius } => {
                radius.walk(into)?;
                edge.walk(into)
            }
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
        let Self { path, gap } = self;
        gap.walk(into)?;
        path.walk(into)?;
        Ok(())
    }
}

impl Walk for Flange {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            height,
            angle,
            width,
            bend,
            rule: _,
            height_datum: _,
            position: _,
            outline,
        } = self;
        angle.walk(into)?;
        bend.walk(into)?;
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
            Self::UpTo { target, offset } => {
                offset.walk(into)?;
                target.walk(into)
            }
            Self::Distance(f0) => f0.walk(into),
        }
    }
}

impl Walk for FlangeWidth {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::FromReference {
                from,
                offset,
                width,
                reversed: _,
            } => {
                offset.walk(into)?;
                width.walk(into)?;
                from.walk(into)
            }
            Self::Centered { width } => width.walk(into),
            Self::FromEnds { first, second } => {
                first.walk(into)?;
                second.walk(into)?;
                Ok(())
            }
            Self::PerEdge(f0) => f0.walk(into),
            Self::CenteredPerEdge(f0) => f0.walk(into),
            Self::Full => Ok(()),
        }
    }
}

impl Walk for Hem {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            edges,
            form,
            side: _,
            position: _,
            rule: _,
        } = self;
        form.walk(into)?;
        edges.walk(into)?;
        Ok(())
    }
}

impl Walk for HemForm {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Flat { length, gap } => {
                length.walk(into)?;
                gap.walk(into)?;
                Ok(())
            }
            Self::Open { length, gap } => {
                length.walk(into)?;
                gap.walk(into)?;
                Ok(())
            }
            Self::Teardrop {
                radius,
                length,
                gap,
            } => {
                radius.walk(into)?;
                length.walk(into)?;
                gap.walk(into)?;
                Ok(())
            }
            Self::Rolled { radius, angle } => {
                radius.walk(into)?;
                angle.walk(into)?;
                Ok(())
            }
            Self::Native(_) => Ok(()),
        }
    }
}

impl Walk for KFactorRow {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { ratio: _, k_factor } = self;
        k_factor.walk(into)?;
        Ok(())
    }
}

impl Walk for KFactorTable {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self { rows, lookup: _ } = self;
        rows.walk(into)?;
        Ok(())
    }
}

impl Walk for OutsideGrid {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::BackupKFactor { k_factor } => k_factor.walk(into),
        }
    }
}

impl Walk for Relief {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            shape: _,
            width,
            width_ratio,
            depth,
        } = self;
        width.walk(into)?;
        width_ratio.walk(into)?;
        depth.walk(into)?;
        Ok(())
    }
}

impl Walk for Rip {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Faces { faces } => faces.walk(into),
            Self::Along { path, width } => {
                width.walk(into)?;
                path.walk(into)
            }
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

impl Walk for RipWidth {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Gap { gap, side: _ } => gap.walk(into),
            Self::Tear => Ok(()),
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
            Self::ApplyRule { rule: _, bodies } => bodies.walk(into),
            Self::Wall {
                profile,
                thickness,
                side: _,
                rule: _,
            } => {
                thickness.walk(into)?;
                profile.walk(into)
            }
            Self::ContourFlange {
                profile,
                edges,
                width,
                extent,
                thickness,
                bend,
                vertex_bends,
                rule: _,
            } => {
                thickness.walk(into)?;
                bend.walk(into)?;
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
                angle,
                radius,
                radius_side: _,
                flat_length,
                zone: _,
                line_face: _,
                fixed_side: _,
            } => {
                angle.walk(into)?;
                radius.walk(into)?;
                flat_length.walk(into)?;
                line.walk(into)
            }
            Self::Connect {
                between,
                radius,
                extension: _,
                double: _,
            } => {
                radius.walk(into)?;
                between.walk(into)
            }
            Self::Rip(f0) => f0.walk(into),
            Self::Convert(f0) => f0.walk(into),
            Self::BreakCorner {
                corners,
                size,
                shape: _,
            } => {
                size.walk(into)?;
                corners.walk(into)
            }
            Self::ExtendWall { edge, to } => {
                edge.walk(into)?;
                to.walk(into)?;
                Ok(())
            }
            Self::ExtrudedHole {
                bore,
                entry,
                height,
                root_fillet,
                rule: _,
            } => {
                height.walk(into)?;
                root_fillet.walk(into)?;
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
            thickness,
            bend,
            allowance,
            native_options,
            source_settings,
            native_ref: _,
        } = self;
        thickness.walk(into)?;
        bend.walk(into)?;
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
                height,
                wall_angle,
                rim_radius,
                floor_radius,
            } => {
                height.walk(into)?;
                wall_angle.walk(into)?;
                rim_radius.walk(into)?;
                floor_radius.walk(into)?;
                outline.walk(into)
            }
            Self::Bridge {
                placement,
                height,
                wall_angle,
                length,
                width,
                rim_radius,
                floor_radius,
                relief_length,
            } => {
                height.walk(into)?;
                wall_angle.walk(into)?;
                length.walk(into)?;
                width.walk(into)?;
                rim_radius.walk(into)?;
                floor_radius.walk(into)?;
                relief_length.walk(into)?;
                placement.walk(into)
            }
            Self::Louver {
                profile,
                open_edge,
                depth,
                flap_angle,
                wall_angle,
                rim_radius,
            } => {
                depth.walk(into)?;
                flap_angle.walk(into)?;
                wall_angle.walk(into)?;
                rim_radius.walk(into)?;
                profile.walk(into)?;
                open_edge.walk(into)?;
                Ok(())
            }
            Self::Bead {
                path,
                height,
                section_radius,
                end_radius,
            } => {
                height.walk(into)?;
                section_radius.walk(into)?;
                end_radius.walk(into)?;
                path.walk(into)
            }
            Self::StiffeningRib {
                bend,
                at,
                length,
                bottom_radius,
                wall_angle,
                rim_radius,
            } => {
                length.walk(into)?;
                bottom_radius.walk(into)?;
                wall_angle.walk(into)?;
                rim_radius.walk(into)?;
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
            Self::Circle { center, diameter } => {
                diameter.walk(into)?;
                center.walk(into)
            }
        }
    }
}

impl Walk for VertexBend {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            vertex,
            radius,
            radius_side: _,
        } = self;
        radius.walk(into)?;
        vertex.walk(into)?;
        Ok(())
    }
}
