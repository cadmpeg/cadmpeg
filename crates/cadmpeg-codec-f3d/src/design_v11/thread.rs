// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::{decode::DecodeContext, text::NonBlankString, CodecError};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
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

mod helix_hand;
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
    let helices = helix_hand::paths(&ir.geometry.procedural_surfaces);
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
            losses.push(unprojected(
                &legacy.id,
                "its Thread scope is not in the native scope arena",
            ));
            continue;
        };
        let Some(construction) = scope.thread_construction() else {
            losses.push(unprojected(
                &legacy.id,
                "its scope has no admitted construction",
            ));
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
                    spec_with_geometric_hand(construction, extent.as_ref(), &helices),
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
        // A Thread item that the timeline projection could not pair with its
        // v10 feature is keyed by its stream and scope record.
        let item = crate::ids::native_stream(&scope.id).and_then(|stream| {
            super::timeline::item_feature_id(stream, u64::from(scope.record_index)).ok()
        });
        let Some(feature) = thread_target(ir.design.features.iter_mut(), &legacy.id, item.as_ref())
        else {
            losses.push(unprojected(&legacy.id, "its design feature is absent"));
            continue;
        };
        let Some(face) = selection(face) else {
            losses.push(unprojected(&legacy.id, "its face selection is unresolved"));
            continue;
        };
        feature.definition.operation = FeatureOperation::Thread(Box::new(Thread {
            face,
            start: None,
            spec: spec_with_geometric_hand(construction, extent.as_ref(), &helices),
            native_options: options(construction),
            source_settings: BTreeMap::new(),
        }));
    }
    Ok(())
}

/// The design feature that a Thread projects into: the one the v10 feature
/// names, else the unresolved Thread item of its scope record.
fn thread_target<'a>(
    features: impl Iterator<Item = &'a mut cadmpeg_ir::design::feature::Feature>,
    legacy: &cadmpeg_ir::features::FeatureId,
    item: Option<&cadmpeg_ir::features::FeatureId>,
) -> Option<&'a mut cadmpeg_ir::design::feature::Feature> {
    let mut features = features;
    features.find(|feature| {
        feature.id == *legacy
            || (Some(&feature.id) == item
                && matches!(
                    feature.definition.operation,
                    FeatureOperation::Unresolved {
                        family: OperationFamily::Thread
                    }
                ))
    })
}

fn unprojected(
    feature: &cadmpeg_ir::features::FeatureId,
    reason: &str,
) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::ThreadV11Unprojected.note(format!(
        "Thread {feature} keeps its native construction because {reason}."
    ))
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
                cadmpeg_ir::codec_name!("f3d:external"),
                NativeOption::Bool(Dimension::Literal(external)),
            )
        })
        .collect()
}

/// The Thread specification, with the hand of the helical faces when the
/// stored construction does not state it.
fn spec_with_geometric_hand(
    construction: &DesignThreadConstruction,
    extent: Option<&CosmeticThreadExtent>,
    helices: &[helix_hand::HelicalPath],
) -> ThreadSpec {
    let mut projected = spec(construction, extent);
    if projected.hand.is_none() {
        projected.hand = helix_hand::hand(helices, construction);
    }
    projected
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
