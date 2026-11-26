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

use crate::annotations::{AnnotationBuilder, ExactnessNote};
use crate::design::feature::Feature;
use crate::design::parameter::DesignParameter;
use crate::document::CadIr;
use crate::provenance::Exactness;
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

/// Each serialized field of a design record, with the fields of the model
/// record it restates and whether the design record states it. A field that
/// restates no model field is the design record's own.
type Restated = Vec<(&'static str, &'static [&'static str], bool)>;

fn feature_fields(feature: &Feature) -> Restated {
    let Feature {
        id: _,
        name,
        suppressed,
        definition: _,
        placement: _,
        component: _,
        driven_by: _,
        native_ref,
    } = feature;
    vec![
        ("name", &["name"][..], name.is_some()),
        ("suppressed", &["suppressed"][..], suppressed.is_some()),
        ("definition", &["definition", "dependencies"][..], true),
        ("placement", &[][..], true),
        ("component", &[][..], true),
        ("driven_by", &[][..], true),
        ("native_ref", &["native_ref"][..], native_ref.is_some()),
    ]
}

fn parameter_fields(parameter: &DesignParameter) -> Restated {
    let DesignParameter {
        id: _,
        scope: _,
        owner,
        name,
        kind: _,
        unit,
        definition: _,
        value,
        display: _,
        tolerance: _,
        decimals: _,
        comment: _,
        native_options: _,
        source_settings: _,
        native_ref,
    } = parameter;
    vec![
        ("scope", &[][..], true),
        ("owner", &["owner"][..], owner.is_some()),
        ("name", &["name"][..], name.is_some()),
        ("kind", &["properties"][..], true),
        ("unit", &["properties"][..], unit.is_some()),
        (
            "definition",
            &["expression", "dependencies", "properties"][..],
            true,
        ),
        ("value", &["value"][..], value.is_some()),
        ("display", &[][..], true),
        ("tolerance", &[][..], true),
        ("decimals", &[][..], true),
        ("comment", &[][..], true),
        ("native_options", &[][..], true),
        ("source_settings", &[][..], true),
        ("native_ref", &["native_ref"][..], native_ref.is_some()),
    ]
}

/// The less exact of `left` and `right`, in the declared order of
/// [`Exactness`].
fn least_exact(left: Exactness, right: Exactness) -> Exactness {
    let rank = |exactness| match exactness {
        Exactness::ByteExact => 0,
        Exactness::Derived => 1,
        Exactness::Inferred => 2,
        Exactness::Unknown => 3,
    };
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

/// The field overrides of `note` on a design record with `fields`.
///
/// A stated design field takes the least exact of the model fields it
/// restates, each at its own mark or else at the entity exactness, and of its
/// own mark. A mark on a design field that restates no model field is kept.
fn restated_fields(note: &ExactnessNote, fields: &Restated) -> Vec<(String, Exactness)> {
    let entity = note.entity();
    let marked = |field: &str| note.fields().get(field).copied();
    let mut overrides = Vec::new();
    for (field, restates, stated) in fields {
        if restates.is_empty() || !stated {
            continue;
        }
        let own = (!restates.contains(field)).then(|| marked(field)).flatten();
        let exactness = restates
            .iter()
            .map(|model| marked(model).unwrap_or(entity))
            .chain(own)
            .fold(Exactness::ByteExact, least_exact);
        if exactness != entity {
            overrides.push(((*field).to_owned(), exactness));
        }
    }
    for (path, exactness) in note.fields() {
        let top = path.as_str().split('.').next().unwrap_or_default();
        if fields
            .iter()
            .any(|(field, restates, _)| *field == top && restates.is_empty())
        {
            overrides.push((path.to_string(), *exactness));
        }
    }
    overrides
}

/// Restates onto each design feature and parameter of `ir` the exactness of
/// the model record with its id. The CADIR wire carries the design record in
/// place of the model record (§10). A model field that no stated design field
/// restates leaves with the model record.
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
        .map(|feature| (feature.id.as_str(), feature_fields(feature)));
    let parameters = ir
        .design
        .parameters
        .iter()
        .filter(|parameter| modeled.contains(parameter.id.as_str()))
        .map(|parameter| (parameter.id.as_str(), parameter_fields(parameter)));
    let mut restated = Vec::new();
    for (id, fields) in features.chain(parameters) {
        if let Some(note) = annotations.annotations().exactness().get(id) {
            restated.push((id, note.entity(), restated_fields(note, &fields)));
        }
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
