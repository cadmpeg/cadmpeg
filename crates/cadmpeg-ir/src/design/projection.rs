// SPDX-License-Identifier: Apache-2.0
//! A decoder's projection of its records into the v11 design layer.
//!
//! The projection reads no source byte: it restates records the decoder
//! already read. A projection that fails is given up as one part (§3.3 of
//! the CADIR v11 specification): the design layer stays as it was before the
//! projection, and one `record.unreadable` loss names the layer.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::annotations::AnnotationBuilder;
use crate::document::CadIr;
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::unknown::part::record_unreadable_kind;

/// Runs `project`, the `codec` decoder's projection of the v11 design layer
/// of `ir`, as one part.
///
/// Where `project` fails with an error that does not end the session, `ir`
/// holds the design records and product definitions it held before the
/// projection, `losses` holds the losses it held before the projection and
/// one `record.unreadable` that names the layer. The geometry and the model
/// records stand.
///
/// # Errors
///
/// An error that ends the session: a refusal in force, a Session-scoped
/// resource refusal, or an I/O error.
pub fn project_layer(
    ctx: &DecodeContext<'_>,
    codec: &str,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    project: impl FnOnce(&mut CadIr, &mut Vec<LossNote>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let design = ir.design.clone();
    let definitions = ir.model.product_definitions.clone();
    let reported = losses.len();
    if let Err(error) = ctx.run_part(|| project(ir, losses))? {
        ir.design = design;
        ir.model.product_definitions = definitions;
        losses.truncate(reported);
        losses.push(
            LossNote::new(
                record_unreadable_kind(),
                format!("{codec} v11 design layer is not stated: {error}"),
            )
            .with_severity(Severity::Error),
        );
    }
    Ok(())
}

/// Each field of a design record that restates a model record, with the
/// fields of the model record it restates and whether the design record
/// states it.
type Restated = Vec<(&'static str, &'static [&'static str], bool)>;

/// Moves the exactness of each model feature and parameter that a design
/// record of `ir` restates under the same id onto that design record and its
/// fields. The CADIR wire carries the design record in place of the model
/// record (§10). A model field that no design field restates leaves with the
/// model record.
///
/// # Errors
///
/// A resource refusal while the overrides are stated.
pub fn restate_exactness(
    ir: &CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let modeled = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .chain(
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.id.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let features = ir
        .design
        .features
        .iter()
        .filter(|feature| modeled.contains(feature.id.as_str()))
        .map(|feature| {
            let fields: Restated = vec![
                ("name", &["name"][..], feature.name.is_some()),
                (
                    "suppressed",
                    &["suppressed"][..],
                    feature.suppressed.is_some(),
                ),
                ("definition", &["definition", "dependencies"][..], true),
                (
                    "native_ref",
                    &["native_ref"][..],
                    feature.native_ref.is_some(),
                ),
            ];
            (feature.id.as_str(), fields)
        });
    let parameters = ir
        .design
        .parameters
        .iter()
        .filter(|parameter| modeled.contains(parameter.id.as_str()))
        .map(|parameter| {
            let fields: Restated = vec![
                ("scope", &["owner"][..], true),
                ("name", &["name"][..], parameter.name.is_some()),
                ("kind", &["properties"][..], true),
                ("unit", &["properties"][..], parameter.unit.is_some()),
                ("definition", &["expression", "dependencies"][..], true),
                ("value", &["value"][..], parameter.value.is_some()),
                (
                    "native_ref",
                    &["native_ref"][..],
                    parameter.native_ref.is_some(),
                ),
            ];
            (parameter.id.as_str(), fields)
        });
    let mut restated = Vec::new();
    for (id, fields) in features.chain(parameters) {
        let Some(note) = annotations.annotations().exactness().get(id) else {
            continue;
        };
        let overrides = fields
            .into_iter()
            .filter(|(_, _, stated)| *stated)
            .filter_map(|(field, model, _)| {
                model
                    .iter()
                    .find_map(|model| note.fields().get(*model))
                    .map(|exactness| (field, *exactness))
            })
            .collect::<Vec<_>>();
        restated.push((id, note.entity(), overrides));
    }
    let ids = restated.iter().map(|(id, ..)| *id).collect::<BTreeSet<_>>();
    annotations.retain_exactness(|owner| !ids.contains(owner));
    for (id, entity, overrides) in restated {
        annotations.exactness(id, entity);
        for (field, exactness) in overrides {
            annotations.field_exactness(id, field, exactness)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
