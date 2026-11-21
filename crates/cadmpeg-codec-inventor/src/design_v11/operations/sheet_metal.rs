// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal operations with complete v11 operands.

use std::collections::BTreeMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sheet_metal::{
    BendSet, BendSpec, NormalSide, RadiusSide, SheetMetal, SheetMetalKind,
};
use cadmpeg_ir::design::options::{Thickness, WallSide};
use cadmpeg_ir::features::{
    self as old, FeatureOperation as LegacyOperation, SheetMetalThicknessSide,
};
use cadmpeg_ir::scalar::{Length, PositiveLength};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::units::UnitVector3;

use super::{selections, swept, Projected};

pub(super) fn operation(
    value: &LegacyOperation,
    sketches: &[Sketch],
) -> Option<Projected<FeatureOperation>> {
    let kind = match value {
        LegacyOperation::SheetMetalBaseFlange { profile, thickness, side } => (|| {
            Ok(SheetMetalKind::Wall {
                profile: selections::planar(profile, sketches)?,
                thickness: thickness.map(Dimension::Literal),
                side: side.map(|side| match side {
                    SheetMetalThicknessSide::Forward => NormalSide::Along,
                    SheetMetalThicknessSide::Reverse => NormalSide::Against,
                    SheetMetalThicknessSide::Symmetric => NormalSide::Symmetric,
                }),
                rule: None,
            })
        })(),
        LegacyOperation::SheetMetalContourFlange { construction } => contour(construction, sketches),
        LegacyOperation::SheetMetalUnfold { construction } if construction.neutral_factor.is_none() => (|| {
            let bends = match (construction.all_bends, &construction.bends) {
                (Some(true), None) => Some(BendSet::All),
                (Some(false) | None, Some(faces)) => Some(BendSet::Selected(selections::face(faces)?)),
                (None, None) => None,
                _ => return Err("the unfold has conflicting bend selection controls"),
            };
            Ok(SheetMetalKind::Unfold {
                fixed: construction.stationary.as_ref().map(selections::face).transpose()?,
                bends,
            })
        })(),
        LegacyOperation::SheetMetalAttachedPlate { .. } => Err("the attached plate has an attachment edge outside the v11 wall contract"),
        LegacyOperation::SheetMetalBend { .. } => Err("the attached bend stores plate, relief, and transition operands outside the v11 bend contract"),
        LegacyOperation::SheetMetalUnfold { .. } => Err("the unfold stores a feature-local neutral factor outside the v11 unfold contract"),
        LegacyOperation::SheetMetalRefold { construction } if construction.unfold.is_none() => Ok(SheetMetalKind::Refold { bends: None }),
        LegacyOperation::SheetMetalRefold { .. } => Err("the refold selects an unfold feature outside the v11 bend-set contract"),
        LegacyOperation::SheetMetalCorner { .. } => Err("the unchanged corner effect has no v11 corner-break operation"),
        _ => return None,
    };
    Some(kind.map(|kind| {
        FeatureOperation::SheetMetal(Box::new(SheetMetal {
            kind,
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
        }))
    }))
}

fn contour(
    value: &old::sheet_metal::SheetMetalContourFlangeConstruction,
    sketches: &[Sketch],
) -> Projected<SheetMetalKind> {
    if value.neutral_factor.is_some() {
        return Err(
            "the contour flange has a feature-local neutral factor outside its v11 contract",
        );
    }
    let profile = value
        .profile
        .as_ref()
        .ok_or("the contour profile is absent")?;
    let (old::PlanarProfileRef::Sketch(sketch)
    | old::PlanarProfileRef::SketchProfiles { sketch, .. }
    | old::PlanarProfileRef::SketchRegions { sketch, .. }
    | old::PlanarProfileRef::SketchEntities { sketch, .. }
    | old::PlanarProfileRef::SketchSelection { sketch, .. }) = profile
    else {
        return Err("the contour profile has no oriented sketch plane");
    };
    let (_, normal, _) = sketches
        .iter()
        .find(|item| &item.id == sketch)
        .and_then(|item| item.placement.resolved())
        .ok_or("the contour sketch plane is unresolved")?;
    let normal =
        UnitVector3::normalized_nonzero(normal).ok_or("the contour normal is degenerate")?;
    let travel = swept::direction(value.direction.ok_or("the contour direction is absent")?)?
        .resolved
        .ok_or("the contour direction is unresolved")?;
    let reversed = if travel == normal {
        false
    } else if travel == normal.reversed() {
        true
    } else {
        return Err("the contour direction differs from both senses of its sketch normal");
    };
    let Some(old::ExtrudeExtent::OneSided {
        side:
            old::ExtrudeSide {
                termination: old::LinearTermination::Blind { length },
                draft: None,
            },
    }) = &value.extent
    else {
        return Err("the contour extent requires additional v11 limits or drafts");
    };
    let amount = Length::new(if reversed {
        -length.get()
    } else {
        length.get()
    })
    .ok_or("the contour span is invalid")?;
    let (start, end) = if amount.get() < 0.0 {
        (Limit::Amount(Dimension::Literal(amount)), Limit::AtOrigin)
    } else {
        (Limit::AtOrigin, Limit::Amount(Dimension::Literal(amount)))
    };
    let thickness = match value.thickness {
        Some(value_thickness) => Some(match value.side {
            Some(old::sheet_metal::SheetMetalContourThicknessSide::Centered) => {
                Thickness::Symmetric(Dimension::Literal(value_thickness))
            }
            Some(old::sheet_metal::SheetMetalContourThicknessSide::Left) => Thickness::OneSide {
                value: Dimension::Literal(value_thickness),
                side: Some(WallSide::Left),
            },
            Some(old::sheet_metal::SheetMetalContourThicknessSide::Right) => Thickness::OneSide {
                value: Dimension::Literal(value_thickness),
                side: Some(WallSide::Right),
            },
            None => Thickness::OneSide {
                value: Dimension::Literal(value_thickness),
                side: None,
            },
        }),
        None if value.side.is_none() => None,
        None => return Err("the contour side has no thickness"),
    };
    let radius = value
        .inside_radius
        .map(|radius| {
            PositiveLength::new(radius.get())
                .map(Dimension::Literal)
                .ok_or("the zero contour radius is outside the v11 radius contract")
        })
        .transpose()?;
    Ok(SheetMetalKind::ContourFlange {
        profile: selections::planar(profile, sketches)?,
        edges: None,
        width: None,
        extent: Some(Span::Between {
            start: Box::new(start),
            end: Box::new(end),
        }),
        thickness,
        bend: BendSpec {
            radius,
            radius_side: value.inside_radius.map(|_| RadiusSide::Inside),
            relief: None,
        },
        vertex_bends: Vec::new(),
        rule: None,
    })
}
