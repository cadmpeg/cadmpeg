// SPDX-License-Identifier: Apache-2.0
//! Validation of parameter owners, companions, and values.

use super::{design_stream, push_constant_finding, Ctx};
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate parameter owner frames and their indexed parameter links. The
/// companion links resolve into design-intent records, so a document without
/// them has nothing to check.
pub(super) fn validate_parameter_owners(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in &native.design_parameter_owners {
        let native_stream = design_stream(owner.id());
        let unique_index = owner_indices.insert((native_stream, owner.record_index()));
        let parameter = parameters_by_index.get(&(native_stream, owner.parameter_record_index()));
        let legacy_68_frame = owner.frame_length() == 68;
        let scope_key = (native_stream, owner.scope_record_index());
        let detached = !records_by_index.contains_key(&scope_key)
            || ctx
                .scopes_by_index
                .get(&scope_key)
                .is_some_and(|scope| !scope.names_owner(owner));
        let frame_layout = !matches!(owner.frame_length(), 68 | 88)
            || parameter.is_some_and(|parameter| {
                owner.evaluated_value_offset() == parameter.evaluated_value_offset()
            });
        // A text parameter's owner stores the parameter's text and takes its
        // evaluated scalar, and only a text parameter has such an owner.
        let text_link = parameter.is_some_and(|parameter| {
            parameter.text_value() == owner.text_value()
                && (owner.text_value().is_none()
                    || owner.evaluated_value_offset() == parameter.evaluated_value_offset())
        });
        let unique_local_ordinal = if legacy_68_frame || detached {
            true
        } else {
            owner_local_ordinals.insert((
                native_stream,
                owner.scope_record_index(),
                owner.local_ordinal(),
            ))
        };
        let standalone_thread = parameter.is_none()
            && owner.text_value().is_none()
            && matches!(owner.frame_length(), 103 | 104)
            && owner.local_ordinal() <= 1
            && ctx.scopes_by_index.get(&scope_key).is_some_and(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::Thread
                    && scope.names_owner(owner)
            });
        let linked = text_link
            && records_by_index.contains_key(&(native_stream, owner.parameter_record_index()))
            && records_by_index.contains_key(&(native_stream, owner.companion_record_index()))
            && companions_by_index
                .get(&(native_stream, owner.companion_record_index()))
                .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get().to_bits()
                        == owner.evaluated_value().get().to_bits()
            });
        let valid =
            frame_layout && (standalone_thread || linked) && unique_index && unique_local_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(owner.id().to_owned()),
            );
        }
    }
}

/// Validate parameter companion prefixes and owned recipe runs.
pub(super) fn validate_parameter_companions(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let payload = companion.payload();
        let payload_end =
            payload.and_then(|payload| payload.byte_offset().checked_add(payload.byte_length()));
        let mut expected_recipes = native
            .construction_recipes
            .iter()
            .filter(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && payload.is_some_and(|payload| {
                        payload_end.is_some_and(|end| {
                            recipe.byte_offset >= payload.byte_offset() && recipe.byte_offset < end
                        })
                    })
            })
            .collect::<Vec<_>>();
        expected_recipes.sort_by_key(|left| left.byte_offset);
        let unique_index = companion_indices.insert((native_stream, companion.record_index()));
        let unique_owner = companion_owners.insert((native_stream, companion.owner_record_index()));
        let owner = owners_by_index.get(&(native_stream, companion.owner_record_index()));
        // The timestamp follows a plain (offset 42) or typed (offset 82)
        // owner reference.
        let valid = [42, 82].into_iter().any(|relative| {
            companion
                .byte_offset()
                .checked_add(relative)
                .is_some_and(|expected_offset| {
                    companion.timestamp_micros_offset() == expected_offset
                })
        }) && payload.is_none_or(|payload| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| payload.byte_offset() == expected_offset)
        }) && (payload.is_none() || payload_end.is_some())
            && payload
                .map_or(
                    &[][..],
                    crate::records::parameters::DesignCompanionPayload::owned_recipe_ids,
                )
                .iter()
                .map(String::as_str)
                .eq(expected_recipes.iter().map(|recipe| recipe.id.as_str()))
            && records_by_index.contains_key(&(native_stream, companion.record_index()))
            && owner
                .is_some_and(|owner| owner.companion_record_index() == companion.record_index())
            && unique_index
            && unique_owner;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Validate parameter record identity uniqueness.
pub(super) fn validate_parameters(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut parameter_indices = HashSet::new();
    for parameter in &native.design_parameters {
        let native_stream = design_stream(&parameter.id);
        if !parameter_indices.insert((native_stream, parameter.record_index)) {
            let id = parameter.id.clone();
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter has an invalid frame, family discriminator, or owner",
                Some(id),
            );
        }
    }
}
