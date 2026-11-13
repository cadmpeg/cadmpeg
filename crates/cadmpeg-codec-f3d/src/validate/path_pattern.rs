// SPDX-License-Identifier: Apache-2.0
//! Validate the record links of decoded `PathPattern` settings.

use super::{design_stream, Ctx};
use crate::layout::{path_pattern_settings as settings_layout, path_pattern_start as start_layout};
use crate::records::feature::patterns::{PATH_PATTERN_SETTINGS_TYPE, PATH_PATTERN_START_TYPE};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    for scope in &ctx.native.design_parameter_scopes {
        let Some(settings) = scope.path_pattern_settings() else {
            continue;
        };
        let stream = design_stream(&scope.id);
        let type_stream = stream.strip_suffix("BulkStream.dat");
        let mut rows = ctx
            .native
            .design_types
            .iter()
            .filter(|row| design_stream(row.id()).strip_suffix("MetaStream.dat") == type_stream)
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.byte_offset);
        let typed_header = |record_index: u32, (guid, version): (&str, u32)| {
            let mut headers = ctx.native.design_record_headers.iter().filter(|header| {
                design_stream(&header.id) == stream
                    && header.record_index == record_index
                    && header
                        .class_tag
                        .as_str()
                        .parse::<usize>()
                        .ok()
                        .and_then(|tag| tag.checked_sub(256))
                        .and_then(|ordinal| rows.get(ordinal))
                        .is_some_and(|row| row.type_guid.as_str().eq_ignore_ascii_case(guid))
            });
            let header = headers.next()?;
            let row = rows.get(
                header
                    .class_tag
                    .as_str()
                    .parse::<usize>()
                    .ok()?
                    .checked_sub(256)?,
            )?;
            (headers.next().is_none()
                && row.version == version
                && row
                    .entities
                    .values()
                    .any(|entity| *entity == u64::from(record_index)))
            .then_some(header.byte_offset)
        };
        let at = |base: u64, offset: usize| base.checked_add(u64_from_index(offset));
        let settings_valid = match (
            settings.settings_record_index,
            settings.settings_byte_offset,
        ) {
            (Some(index), Some(offset)) => typed_header(index, PATH_PATTERN_SETTINGS_TYPE)
                .filter(|base| *base == offset)
                .is_some_and(|base| {
                    settings.flipped.as_ref().is_none_or(|value| {
                        Some(value.offset) == at(base, settings_layout::FLIPPED)
                    }) && settings.path_oriented.as_ref().is_none_or(|value| {
                        Some(value.offset) == at(base, settings_layout::PATH_ORIENTED)
                    }) && settings.total_extent.as_ref().is_none_or(|value| {
                        Some(value.offset) == at(base, settings_layout::DISTANCE_TYPE)
                    })
                }),
            (None, None) => {
                settings.flipped.is_none()
                    && settings.path_oriented.is_none()
                    && settings.total_extent.is_none()
                    && settings.path_group_record_index.is_none()
                    && !settings.reordered_owners
            }
            _ => false,
        };
        let start_base = settings
            .start_record_index
            .and_then(|index| typed_header(index, PATH_PATTERN_START_TYPE));
        let start_valid = match (
            settings.start_record_index,
            start_base,
            settings.start_frame_end,
            &settings.start_parameter,
        ) {
            (None, None, None, None) => true,
            (Some(_), Some(base), Some(end), value) => {
                let no_inner_header = ctx.native.design_record_headers.iter().all(|header| {
                    design_stream(&header.id) != stream
                        || header.byte_offset <= base
                        || header.byte_offset >= end
                });
                at(base, start_layout::LEN) == Some(end)
                    && no_inner_header
                    && value.as_ref().is_none_or(|value| {
                        Some(value.offset) == at(base, start_layout::START_PARAMETER)
                    })
            }
            _ => false,
        };
        let members = scope.reference_members();
        let valid = scope.kind() == crate::records::feature::scope::DesignFeatureKind::PathPattern
            && settings_valid
            && start_valid
            && settings
                .start_record_index
                .is_none_or(|index| members.values().any(|member| *member == index))
            && settings
                .path_group_record_index
                .is_none_or(|index| members.values().any(|member| *member == index));
        if !valid {
            super::push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-pattern settings have an invalid record link",
                Some(scope.id.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests;
