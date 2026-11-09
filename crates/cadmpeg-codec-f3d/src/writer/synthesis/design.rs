// SPDX-License-Identifier: Apache-2.0
//! Regenerate neutral design records; native envelopes cannot override edits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

mod configurations;
mod parameters;

pub(super) fn regenerate(
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), crate::writer::WriteError> {
    // Configurations first: binding and canonicalizing the neutral model
    // folds the active overrides into their parameters. Parameters author
    // from that canonical neutral, so the generator's re-projection of the
    // authored native agrees with the neutral model it checks.
    let source_parameters = target.model.parameters.clone();
    configurations::regenerate(target, native, &source_parameters, losses)?;
    let remapped = parameters::regenerate(target, native, losses, limits)?;
    // The parameter rewrite above re-identifies every authored parameter
    // from its native record. Retained configuration selections still name
    // the pre-rewrite identities, so point them at the live parameters. A
    // key no authored parameter continues is left for the projection
    // validator to decline.
    for configuration in &mut target.model.configurations {
        for (old, new) in &remapped {
            if old == new {
                continue;
            }
            if let Some(expression) = configuration.parameter_overrides.remove(old) {
                configuration
                    .parameter_overrides
                    .entry(new.clone())
                    .or_insert(expression);
            }
            if let Some(value) = configuration.parameter_values.remove(old) {
                configuration
                    .parameter_values
                    .entry(new.clone())
                    .or_insert(value);
            }
        }
    }
    Ok(())
}

pub(super) fn reconcile_body_tags(target: &CadIr, native: &mut F3dNative) {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    for attribute in &target.model.attributes {
        let AttributeTarget::Body(body_id) = &attribute.target else {
            continue;
        };
        if target.model.attributes.iter().filter(|candidate| {
            candidate.target == attribute.target && candidate.name == attribute.name
                && matches!(candidate.values.get(1), Some(Some(AttributeValue::String(value))) if value == "generic_tag_attrib_def")
        }).count() != 1 { continue; }
        let [Some(AttributeValue::Reference(_)), Some(AttributeValue::String(family)), Some(AttributeValue::Integer(3)), Some(AttributeValue::Integer(3)), Some(AttributeValue::Integer(-1)), Some(AttributeValue::String(marker)), Some(AttributeValue::Integer(count)), rest @ ..] =
            attribute.values.as_slice()
        else {
            continue;
        };
        if attribute.name != "ATTRIB_CUSTOM-attrib"
            || family != "generic_tag_attrib_def"
            || marker != "generic_tag_attrib_def "
        {
            continue;
        }
        let Ok(count) = usize::try_from(*count) else {
            continue;
        };
        if u32::try_from(count).is_err() || count.checked_mul(5) != Some(rest.len()) {
            continue;
        }
        let mut key = None;
        let mut links = Vec::new();
        let mut valid = true;
        for row in rest.chunks_exact(5) {
            let [Some(AttributeValue::Integer(kind)), Some(AttributeValue::String(id)), Some(AttributeValue::Integer(reference)), Some(AttributeValue::Integer(0)), Some(AttributeValue::Integer(0))] =
                row
            else {
                valid = false;
                break;
            };
            match (*kind, id.as_str()) {
                (4, "900") if key.is_none() => match u64::try_from(*reference) {
                    Ok(value) => key = Some(value),
                    Err(_) => {
                        valid = false;
                        break;
                    }
                },
                (3, _) => match id.clone().try_into() {
                    Ok(id) => links.push((id, *reference)),
                    Err(_) => {
                        valid = false;
                        break;
                    }
                },
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let Some(row) = target
            .model
            .bodies
            .iter()
            .find(|body| body.id == *body_id)
            .and_then(|body| body.native_ref.as_deref())
        else {
            continue;
        };
        if let Some(key) = key {
            if let Some(native_key) = native
                .body_native_keys
                .iter_mut()
                .find(|candidate| candidate.id() == row)
            {
                native_key.asm_body_key = Some(key);
            }
        }
        let old = std::mem::take(&mut native.persistent_design_links);
        for (ordinal, (id, reference)) in links.into_iter().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                break;
            };
            let previous = old.iter().find(|link| {
                link.target.as_str() == row
                    && link.target_coedge.is_none()
                    && link.ordinal == ordinal
            });
            native.persistent_design_links.push(
                crate::records::sketch_links::PersistentDesignLink {
                    id: previous.map_or_else(
                        || {
                            format!(
                                "f3d:design:persistent-design-link#{}:{ordinal}",
                                attribute.id.as_str().rsplit('#').next().unwrap_or_default()
                            )
                        },
                        |link| link.id.clone(),
                    ),
                    target: row.to_owned().into(),
                    target_coedge: None,
                    design_id: id,
                    design_reference: reference,
                    ordinal,
                },
            );
        }
        native.persistent_design_links.extend(
            old.into_iter()
                .filter(|link| link.target.as_str() != row || link.target_coedge.is_some()),
        );
    }
}

pub(super) fn with_context<T>(
    limits: cadmpeg_core::decode::ResourceLimits,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, crate::writer::WriteError>,
) -> Result<T, crate::writer::WriteError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..Default::default()
    };
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let result = run(&ctx);
    ctx.finish_session()?;
    result
}
