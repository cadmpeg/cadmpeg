// SPDX-License-Identifier: Apache-2.0
//! The design features that restate the input topology records of the
//! model features ([CADIR §5.7](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#57-current-names),
//! [§5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#58-selections)).
//!
//! The typed operation of a feature restates its input state through the
//! source-selector picks of its selections.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::features::Feature as LegacyFeature;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::CadIr;

/// State the restatement of each input topology record that a design
/// feature of `ir` restates.
///
/// # Errors
///
/// Returns a codec error where the decode's resource policy refuses the
/// visit of a feature's references.
pub(super) fn restate(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let typed: Vec<&LegacyFeature> = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            ir.design
                .features
                .binary_search_by(|item| item.id.cmp(&feature.id))
                .is_ok_and(|index| {
                    !matches!(
                        ir.design.features[index].definition.operation,
                        FeatureOperation::Native { .. } | FeatureOperation::Unresolved { .. }
                    )
                })
        })
        .collect();
    let mut restated = Vec::new();
    for input in &ir.model.feature_input_topologies {
        let Some(feature) = typed.iter().find(|feature| feature.id == input.input_of) else {
            continue;
        };
        let (references, members) = references(ctx, feature)?;
        if !references.contains(input.id.as_str()) {
            continue;
        }
        restated.push((
            input.id.as_str().to_owned(),
            input.input_of.as_str().to_owned(),
            input_cut(input, &members),
        ));
    }
    for (legacy, feature, cut) in restated {
        ir.model
            .restated_mut()
            .record(&legacy, Restater::Record(feature), cut);
    }
    Ok(())
}

/// The identities a feature references and the state-local members it names.
fn references(
    ctx: &DecodeContext<'_>,
    feature: &LegacyFeature,
) -> Result<(BTreeSet<String>, BTreeSet<String>), CodecError> {
    let mut references = BTreeSet::new();
    feature.visit_identity_references(ctx, &mut |id| {
        references.insert(id.to_owned());
        Ok(())
    })?;
    let mut members = BTreeSet::new();
    feature.visit_local_members(ctx, &mut |id| {
        members.insert(id.to_owned());
        Ok(())
    })?;
    Ok((references, members))
}

/// The wire keys of the fields of `input` that the picks of the feature
/// leave out: a pick holds the source selector of each face and edge it
/// names, and the interior point, tangent and end points of an edge.
fn input_cut(input: &FeatureInputTopology, picked: &BTreeSet<String>) -> Vec<&'static str> {
    let unpicked = |id: &str| !picked.contains(id);
    let mut cut = Vec::new();
    if input.faces.iter().any(|face| unpicked(face.as_str())) {
        cut.push("faces");
    }
    if input.edges.iter().any(|edge| unpicked(edge.as_str())) {
        cut.push("edges");
    }
    if input.edge_geometry.iter().any(|sample| {
        unpicked(sample.edge.as_str())
            || !sample.faces.is_empty()
            || !sample.supports.is_empty()
            || sample.carrier.is_some()
            || sample.start.is_some() != sample.end.is_some()
    }) {
        cut.push("edge_geometry");
    }
    for (key, held) in [
        ("bodies", !input.bodies.is_empty()),
        ("vertices", !input.vertices.is_empty()),
        ("face_owners", !input.face_owners.is_empty()),
        ("face_geometry", !input.face_geometry.is_empty()),
        ("body_writers", !input.body_writers.is_empty()),
    ] {
        if held {
            cut.push(key);
        }
    }
    cut
}

#[cfg(test)]
mod tests;
