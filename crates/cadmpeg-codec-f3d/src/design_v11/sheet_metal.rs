// SPDX-License-Identifier: Apache-2.0
//! Base-flange walls and the rules that govern their thickness.

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::sheet_metal::{
    BendSpec, NormalSide, SheetMetal, SheetMetalKind, SheetMetalRule,
};
use cadmpeg_ir::design::selection::{
    Anchor, Members, NativeOwner, PickTarget, Picked, Picks, Selection,
};
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::sheet_metal_forms::SheetMetalThicknessSide;
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::CadIr;

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::sheet_metal_settings::DesignSheetMetalSettings;

const CENTIMETRES_TO_MILLIMETRES: f64 = 10.0;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    definitions: &BTreeMap<String, ProductDefinitionId>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !ir.model.features.iter().any(|feature| {
        matches!(
            feature.evaluation.definition().operation(),
            LegacyOperation::SheetMetalBaseFlange { .. }
        )
    }) {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let scopes =
        namespace.arena_as_for_decode::<DesignParameterScope>(ctx, "design_parameter_scopes")?;
    let settings = namespace
        .arena_as_for_decode::<DesignSheetMetalSettings>(ctx, "design_sheet_metal_settings")?;
    let legacy = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    let projected_ids = ir
        .design
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    let mut rules = BTreeMap::new();
    for feature in &mut ir.design.features {
        let Some(source) = legacy.get(&feature.id) else {
            continue;
        };
        let LegacyOperation::SheetMetalBaseFlange {
            profile,
            thickness,
            side,
        } = source.evaluation.definition().operation()
        else {
            continue;
        };
        let Some(scope) = unique(
            scopes
                .iter()
                .filter(|scope| Some(scope.id.as_str()) == source.native_ref.as_deref()),
        ) else {
            continue;
        };
        let (profile_name, sketch, unread) = match profile {
            PlanarProfileRef::Sketch(sketch) => (sketch.as_str(), Some(sketch), false),
            PlanarProfileRef::SketchSelection { sketch, .. } => {
                (scope.id.as_str(), Some(sketch), false)
            }
            PlanarProfileRef::Native(name) => (name.as_str(), None, false),
            PlanarProfileRef::Unresolved(name) => (name.as_str(), None, true),
            _ => (scope.id.as_str(), None, true),
        };
        let (profile_name, unread) = if profile_name.trim().is_empty() {
            (scope.id.as_str(), true)
        } else {
            (profile_name, unread)
        };
        let operation = scope.base_flange_operation();
        if operation.is_none()
            && !matches!(
                scope.kind(),
                DesignFeatureKind::Face | DesignFeatureKind::BaseFlange
            )
        {
            continue;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(definition) = definitions.get(super::timeline::document_prefix(stream)) else {
            continue;
        };
        let rule = unique(settings.iter().filter(|settings| {
            native_stream(settings.id()) == Some(stream)
                && operation.map_or_else(
                    || {
                        settings.owner_record_index == u64::from(scope.record_index)
                            && scope
                                .reference_members()
                                .values()
                                .any(|&index| u64::from(index) == settings.record_index)
                    },
                    |operation| settings.record_index == u64::from(operation.settings_record_index),
                )
        }));
        let rule_id = rule.map(|settings| {
            let id = crate::ids::neutral_sheet_metal_rule_id(definition, stream, settings.rule_record_index);
            if let Entry::Vacant(entry) = rules.entry(id.clone()) {
                ctx.charge_entities(1, "project F3D sheet-metal rule")?;
                entry.insert(SheetMetalRule {
                    id: id.clone(), scope: definition.clone(), name: None,
                    thickness: settings.rule_thickness.and_then(|value| PositiveLength::new(value.get() * CENTIMETRES_TO_MILLIMETRES)).map(Dimension::Literal),
                    bend: BendSpec { radius: None, radius_side: None, relief: None },
                    allowance: None, native_options: BTreeMap::new(), source_settings: BTreeMap::new(), native_ref: Some(settings.id().to_owned()),
                });
                losses.push(F3dLossCode::SheetMetalRuleControlsNative.note(format!("Sheet-metal rule {id} retains its bend radius, allowance and relief in the source record (DR-93).")));
            }
            Ok::<_, CodecError>(id)
        }).transpose()?;
        if rule.is_none() {
            losses.push(F3dLossCode::SheetMetalRuleControlsNative.note(format!(
                "Base flange {} retains its rule through source scope {}.",
                feature.id, scope.id
            )));
        }
        let governing_thickness = rule
            .and_then(|settings| settings.rule_thickness)
            .and_then(|value| PositiveLength::new(value.get() * CENTIMETRES_TO_MILLIMETRES));
        if side.is_none() || governing_thickness.or(*thickness).is_none() {
            continue;
        }
        let thickness = if governing_thickness.is_some() {
            None
        } else {
            *thickness
        };
        let profile_owner = sketch.and_then(|sketch| unique(legacy.values().filter(|candidate| {
            candidate.ordinal < source.ordinal
                && projected_ids.contains(&candidate.id)
                && candidate.native_ref.as_deref().and_then(native_stream) == Some(stream)
                && matches!(candidate.evaluation.definition().operation(), LegacyOperation::Sketch { sketch: binding } if binding.id() == Some(sketch))
        })));
        let target = if matches!(profile, PlanarProfileRef::SketchSelection { .. }) {
            PickTarget::Native {
                within: profile_owner.map(|owner| NativeOwner::Feature(owner.id.clone())),
            }
        } else {
            profile_owner.map_or(PickTarget::Native { within: None }, |owner| {
                PickTarget::FeatureResult(owner.id.clone())
            })
        };
        let profile = if unread {
            Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: Some(
                    cadmpeg_ir::features::selections::NativeSelections::try_from(vec![
                        profile_name.to_owned(),
                    ])
                    .map_err(|error| CodecError::malformed(error.to_string()))?,
                ),
            }
        } else {
            Selection {
                at: Anchor::Input,
                picks: Picks::Named(
                    Members::new(vec![Picked {
                        target,
                        context: None,
                        native: Some(NativeName {
                            codec: cadmpeg_core::nonblank_literal!("f3d").into(),
                            value: NativeNameValue::Text(
                                NonBlankString::new(profile_name).ok_or_else(|| {
                                    CodecError::malformed("F3D profile identity is blank")
                                })?,
                            ),
                        }),
                        domain: None,
                        witness: None,
                        resolved: None,
                    }])
                    .map_err(CodecError::malformed)?,
                ),
                native: match profile {
                    PlanarProfileRef::SketchSelection { selections, .. } => {
                        Some(selections.clone())
                    }
                    _ => None,
                },
            }
        };
        losses.push(F3dLossCode::SheetMetalProfileNative.note(format!("Base flange {} retains profile {profile_name}; its complete profile definition remains in native records.", feature.id)));
        feature.definition.operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
            kind: SheetMetalKind::Wall {
                profile,
                thickness: thickness.map(Dimension::Literal),
                side: side.map(|side| match side {
                    SheetMetalThicknessSide::Forward => NormalSide::Along,
                    SheetMetalThicknessSide::Reverse => NormalSide::Against,
                    SheetMetalThicknessSide::Symmetric => NormalSide::Symmetric,
                }),
                rule: rule_id,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }));
    }
    ir.design.sheet_metal_rules.extend(rules.into_values());
    Ok(())
}

fn unique<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests;
