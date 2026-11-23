// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::{decode::DecodeContext, text::NonBlankString, CodecError};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup::{StandardSize, Thread, ThreadLength, ThreadSpec};
use cadmpeg_ir::design::options::{Hand, NativeOption};
use cadmpeg_ir::design::selection::{Anchor, Picks, SetSelection};
use cadmpeg_ir::features::selections::{FaceSelection, NativeSelections};
use cadmpeg_ir::features::{CosmeticThreadExtent, FeatureOperation as LegacyOperation};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::CadIr;

use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread::DesignThreadConstruction;

const MILLIMETRES_PER_CENTIMETRE: f64 = 10.0;

mod hole;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let legacy = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition().operation(),
                LegacyOperation::CosmeticThread { .. }
            )
        })
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    if legacy.is_empty() {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let scopes =
        namespace.arena_as_for_decode::<DesignParameterScope>(ctx, "design_parameter_scopes")?;
    let scopes = scopes
        .iter()
        .map(|scope| (scope.id.as_str(), scope))
        .collect::<HashMap<_, _>>();
    let mut owner_counts = HashMap::<_, usize>::new();
    for source in legacy.values() {
        let Some(scope) = source.native_ref.as_deref().and_then(|id| scopes.get(id)) else {
            continue;
        };
        let Some(owner) = scope
            .thread_construction()
            .and_then(|construction| construction.form.hole_reference())
        else {
            continue;
        };
        *owner_counts
            .entry((crate::ids::native_stream(&scope.id), owner.value.get()))
            .or_default() += 1;
    }
    for legacy in legacy.values() {
        let Some(scope) = legacy.native_ref.as_deref().and_then(|id| scopes.get(id)) else {
            continue;
        };
        let Some(construction) = scope.thread_construction() else {
            continue;
        };
        let LegacyOperation::CosmeticThread { face, extent, .. } =
            legacy.evaluation.definition().operation()
        else {
            continue;
        };
        if let Some(owner) = construction.form.hole_reference() {
            let unique = owner_counts
                .get(&(crate::ids::native_stream(&scope.id), owner.value.get()))
                == Some(&1);
            let owner = scopes.values().find(|candidate| {
                unique
                    && candidate.record_index == owner.value.get()
                    && crate::ids::native_stream(&candidate.id)
                        == crate::ids::native_stream(&scope.id)
            });
            let owner = owner.and_then(|owner| {
                ir.model
                    .features
                    .iter()
                    .find(|feature| feature.native_ref.as_deref() == Some(owner.id.as_str()))
            });
            let resolved = owner.and_then(|owner| {
                let target = ir
                    .design
                    .features
                    .iter_mut()
                    .find(|feature| feature.id == owner.id)?;
                let operation = hole::operation(
                    owner,
                    spec(construction, extent.as_ref()),
                    options(construction),
                    losses,
                )?;
                target.definition.operation = operation;
                Some(())
            });
            if resolved.is_none() {
                losses.push(crate::loss::F3dLossCode::ThreadHoleNative.note(format!(
                    "Thread {} retains its specification natively because its owning Hole has no admitted v11 construction.", scope.id
                )));
            }
            continue;
        }
        let Some(feature) = ir
            .design
            .features
            .iter_mut()
            .find(|feature| feature.id == legacy.id)
        else {
            continue;
        };
        let Some(face) = selection(face) else {
            continue;
        };
        feature.definition.operation = FeatureOperation::Thread(Box::new(Thread {
            face,
            start: None,
            spec: spec(construction, extent.as_ref()),
            native_options: options(construction),
            source_settings: BTreeMap::new(),
        }));
    }
    Ok(())
}

fn selection(face: &FaceSelection) -> Option<SetSelection> {
    let native = match face {
        FaceSelection::Native(native) | FaceSelection::Resolved { native, .. } => native.clone(),
        FaceSelection::Historical { native, .. }
        | FaceSelection::HistoricalPartial { native, .. } => native.as_str().to_owned(),
        _ => return None,
    };
    Some(SetSelection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: Some(NativeSelections::try_from(vec![native]).ok()?),
    })
}

fn options(construction: &DesignThreadConstruction) -> cadmpeg_ir::design::options::NativeOptions {
    construction
        .external()
        .into_iter()
        .map(|external| {
            (
                cadmpeg_core::nonblank_literal!("f3d:external"),
                NativeOption::Bool(Dimension::Literal(external)),
            )
        })
        .collect()
}

fn spec(
    construction: &DesignThreadConstruction,
    extent: Option<&CosmeticThreadExtent>,
) -> ThreadSpec {
    let length = extent.map(|extent| match extent {
        CosmeticThreadExtent::Through {} => ThreadLength::Full,
        CosmeticThreadExtent::Blind { length } => ThreadLength::Blind(Dimension::Literal(*length)),
    });
    let millimetres =
        |value| PositiveLength::new(value * MILLIMETRES_PER_CENTIMETRE).map(Dimension::Literal);
    ThreadSpec {
        standard: Some(StandardSize {
            standard: construction.profile.clone(),
            kind: None,
            size: Some(Dimension::Literal(construction.designation.clone())),
            class: construction.class().and_then(NonBlankString::new),
        }),
        major_diameter: millimetres(construction.diameters.major()),
        minor_diameter: millimetres(construction.diameters.minor()),
        pitch_diameter: millimetres(construction.diameters.pitch()),
        pitch: millimetres(construction.pitch.get()),
        hand: construction
            .extended
            .as_ref()
            .and_then(|extended| extended.right_handed)
            .map(|right| if right { Hand::Right } else { Hand::Left }),
        length,
        modeled: Some(Dimension::Literal(construction.modeled)),
    }
}

#[cfg(test)]
mod tests;
