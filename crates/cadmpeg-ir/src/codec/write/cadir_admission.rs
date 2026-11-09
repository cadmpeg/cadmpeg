// SPDX-License-Identifier: Apache-2.0
//! Model records the CADIR serialization cannot state (#217 §5.0).
//!
//! The CADIR writer states a procedural record through its unique owning
//! carrier and refuses feature parent relations that do not hold. Each such
//! record is written from the canonical side: the write omits it and a loss
//! names it.

use super::{loss::ExportLossCode, ArenaCoverage, ArenaDisposition, ArenaDispositions};
use crate::report::loss::LossNote;
use crate::schema::EntityKind;
use crate::CadIr;

/// `ir` without each record its serialization refuses, or `None` when every
/// model record serializes. A loss names each omitted record.
pub(super) fn admit(ir: &CadIr, losses: &mut Vec<LossNote>) -> Option<CadIr> {
    let model = &ir.model;
    let unowned_surfaces: Vec<_> = model
        .procedural_surfaces
        .iter()
        .filter(|procedural| model.procedural_surface_owner(&procedural.id).is_none())
        .map(|procedural| procedural.id.clone())
        .collect();
    let unowned_curves: Vec<_> = model
        .procedural_curves
        .iter()
        .filter(|procedural| model.procedural_curve_owner(&procedural.id).is_none())
        .map(|procedural| procedural.id.clone())
        .collect();
    if unowned_surfaces.is_empty()
        && unowned_curves.is_empty()
        && crate::document::feature_parents::validate(&[model]).is_ok()
    {
        return None;
    }
    let mut admitted = ir.clone();
    admitted
        .model
        .procedural_surfaces
        .retain(|procedural| !unowned_surfaces.contains(&procedural.id));
    admitted
        .model
        .procedural_curves
        .retain(|procedural| !unowned_curves.contains(&procedural.id));
    let mut refused_features = Vec::new();
    loop {
        let (owner, reason) = match crate::document::feature_parents::validate(&[&admitted.model]) {
            Ok(()) => break,
            Err(error) => (error.owner().clone(), error.to_string()),
        };
        let before = admitted.model.features.len();
        admitted
            .model
            .features
            .retain(|feature| feature.id != owner);
        let parent = admitted.model.remove_feature_regeneration_parent(&owner);
        if admitted.model.features.len() == before && !parent {
            break;
        }
        refused_features.push((owner, reason));
    }
    let mut omitted = |kind: EntityKind, arena: &str, mut ids: Vec<String>, reason: &str| {
        if ids.is_empty() {
            return;
        }
        ids.sort_unstable();
        losses.push(ExportLossCode::ArenaOmitted(kind).note(format!(
            "the CADIR serialization cannot state {} record(s) of model arena `{arena}`: {}; {reason}",
            ids.len(),
            ids.join(", ")
        )));
    };
    omitted(
        EntityKind::ProceduralSurface,
        "procedural_surfaces",
        unowned_surfaces.iter().map(ToString::to_string).collect(),
        "each has no unique owning surface",
    );
    omitted(
        EntityKind::ProceduralCurve,
        "procedural_curves",
        unowned_curves.iter().map(ToString::to_string).collect(),
        "each has no unique owning curve",
    );
    let feature_reasons: Vec<_> = refused_features
        .iter()
        .map(|(_, reason)| reason.as_str())
        .collect();
    omitted(
        EntityKind::Feature,
        "features",
        refused_features
            .iter()
            .map(|(id, _)| id.to_string())
            .collect(),
        &feature_reasons.join("; "),
    );
    Some(admitted)
}

/// The coverage of a serialization of `written` against the document `ir`.
pub(super) fn coverage(ir: &CadIr, written: Option<&CadIr>) -> ArenaCoverage {
    let Some(written) = written else {
        return ArenaCoverage::Complete;
    };
    let mut dispositions = ArenaDispositions::uniform(ArenaDisposition::Written);
    let reported = |before: usize, after: usize| {
        if before == after {
            ArenaDisposition::Written
        } else {
            ArenaDisposition::Reported
        }
    };
    dispositions.procedural_surfaces = reported(
        ir.model.procedural_surfaces.len(),
        written.model.procedural_surfaces.len(),
    );
    dispositions.procedural_curves = reported(
        ir.model.procedural_curves.len(),
        written.model.procedural_curves.len(),
    );
    dispositions.features = reported(ir.model.features.len(), written.model.features.len());
    ArenaCoverage::Declared(dispositions)
}

#[cfg(test)]
mod tests;
