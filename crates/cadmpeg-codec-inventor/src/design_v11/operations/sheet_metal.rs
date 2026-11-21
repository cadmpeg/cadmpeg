// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal operations with complete v11 operands.

use std::collections::BTreeMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sheet_metal::{
    AttachedRelief, AttachedReliefForm, BendSet, BendSpec, NormalSide, RadiusSide, SheetMetal,
    SheetMetalKind, TransitionForm,
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
    features: &[old::Feature],
) -> Option<Projected<FeatureOperation>> {
    let kind = match value {
        LegacyOperation::SheetMetalBaseFlange {
            profile,
            thickness,
            side,
        } => (|| {
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
        LegacyOperation::SheetMetalContourFlange { construction } => {
            contour(construction, sketches)
        }
        LegacyOperation::SheetMetalUnfold { construction }
            if construction.neutral_factor.is_none() =>
        {
            (|| {
                let bends = match (construction.all_bends, &construction.bends) {
                    (Some(true), None) => Some(BendSet::All),
                    (Some(false) | None, Some(faces)) => {
                        Some(BendSet::Selected(selections::face(faces)?))
                    }
                    (None, None) => None,
                    _ => return Err("the unfold has conflicting bend selection controls"),
                };
                Ok(SheetMetalKind::Unfold {
                    fixed: construction
                        .stationary
                        .as_ref()
                        .map(selections::face)
                        .transpose()?,
                    bends,
                })
            })()
        }
        LegacyOperation::SheetMetalAttachedPlate {
            profile,
            thickness,
            side,
            attachment,
        } => (|| {
            match profile {
                old::PlanarProfileRef::Sketch(id)
                | old::PlanarProfileRef::SketchRegions { sketch: id, .. }
                    if sketches
                        .iter()
                        .any(|sketch| &sketch.id == id && !sketch.profiles.is_empty()) => {}
                _ => return Err("the attached plate closed sketch profile is unresolved"),
            }
            let profile = selections::planar(profile, sketches)?;
            if !matches!(
                &profile.picks,
                cadmpeg_ir::design::selection::Picks::Named(_)
            ) {
                return Err("the attached plate profile is unresolved");
            }
            Ok(SheetMetalKind::AttachedPlate {
                profile,
                thickness: Dimension::Literal(*thickness),
                side: match side {
                    SheetMetalThicknessSide::Forward => NormalSide::Along,
                    SheetMetalThicknessSide::Reverse => NormalSide::Against,
                    SheetMetalThicknessSide::Symmetric => NormalSide::Symmetric,
                },
                attachment: attachments::edges(attachment, features)?,
            })
        })(),
        LegacyOperation::SheetMetalBend {
            plate,
            attachment,
            inside_radius,
            relief,
            transition,
        } => (|| {
            let source = features
                .iter()
                .find(|feature| &feature.id == plate)
                .ok_or("the attached bend plate is absent")?;
            if !matches!(
                source.evaluation.definition().operation(),
                LegacyOperation::SheetMetalAttachedPlate { .. }
            ) {
                return Err("the attached bend plate has a different kind");
            }
            let Some(Ok(_)) = operation(
                source.evaluation.definition().operation(),
                sketches,
                features,
            ) else {
                return Err("the attached bend plate operands are unresolved");
            };
            Ok(SheetMetalKind::AttachedBend {
                plate: plate.clone(),
                attachment: attachments::edges(attachment, features)?,
                inside_radius: Dimension::Literal(*inside_radius),
                relief: AttachedRelief {
                    form: match relief.form {
                        old::SheetMetalReliefForm::StyleDefault => AttachedReliefForm::StyleDefault,
                    },
                    width: Dimension::Literal(relief.width),
                    depth: Dimension::Literal(relief.depth),
                    minimum_remnant: Dimension::Literal(relief.minimum_remnant),
                    seam_gap: Dimension::Literal(relief.seam_gap),
                },
                transition_form: match transition.form {
                    old::SheetMetalTransitionForm::StyleDefault => TransitionForm::StyleDefault,
                },
                transition_radius: Dimension::Literal(transition.radius),
            })
        })(),
        LegacyOperation::SheetMetalUnfold { .. } => {
            Err("the unfold stores a feature-local neutral factor outside the v11 unfold contract")
        }
        LegacyOperation::SheetMetalRefold { construction } if construction.unfold.is_none() => {
            Ok(SheetMetalKind::Refold { bends: None })
        }
        LegacyOperation::SheetMetalRefold { .. } => {
            Err("the refold selects an unfold feature outside the v11 bend-set contract")
        }
        LegacyOperation::SheetMetalCorner {
            bend,
            effect: old::SheetMetalCornerEffect::Unchanged,
        } => (|| {
            let source = features
                .iter()
                .find(|feature| &feature.id == bend)
                .ok_or("the unchanged corner bend is absent")?;
            if !matches!(
                source.evaluation.definition().operation(),
                LegacyOperation::SheetMetalBend { .. }
            ) {
                return Err("the unchanged corner bend is unresolved");
            }
            if !matches!(
                operation(
                    source.evaluation.definition().operation(),
                    sketches,
                    features
                ),
                Some(Ok(_))
            ) {
                return Err("the unchanged corner bend operands are unresolved");
            }
            Ok(SheetMetalKind::UnchangedCorner { bend: bend.clone() })
        })(),
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

#[cfg(test)]
mod tests;

mod attachments;
