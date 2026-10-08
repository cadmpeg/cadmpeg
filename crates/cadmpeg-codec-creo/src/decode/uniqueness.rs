// SPDX-License-Identifier: Apache-2.0
//! Unique-owner lookups for feature definitions, transforms, profiles, and datum planes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ProfileRef;

use crate::container::ContainerScan;

use super::feature_history::axes::section_profile_ref;
use super::sketch_ids::feature_sketch_record_id_in_scan;

pub(crate) fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

/// The only value the predicate accepts. Each value is charged as it is
/// visited: through the first match, then through the rest for a second one,
/// so a repeated match stops the search where it is found.
pub(crate) fn exactly_one_by<'values, T>(
    ctx: &DecodeContext<'_>,
    values: &'values [T],
    mut predicate: impl FnMut(&'values T) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<&'values T>, CodecError> {
    let Some(first) = ctx.position_by(values, &mut predicate, operation)? else {
        return Ok(None);
    };
    if ctx.any_by(&values[first + 1..], &mut predicate, operation)? {
        return Ok(None);
    }
    Ok(Some(&values[first]))
}

pub(super) fn unique_owned_feature_definition<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    feature_id: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    exactly_one_by(
        ctx,
        definitions,
        |definition| Ok(definition.identity.owner_feature_id() == Some(feature_id)),
        "creo unique owner scan",
    )
}

pub(super) fn unique_feature_section_transform<'a>(
    ctx: &DecodeContext<'_>,
    transforms: &'a [crate::placement::FeatureSectionTransform],
    definition_id: u32,
    section_offset: usize,
) -> Result<Option<&'a crate::placement::FeatureSectionTransform>, CodecError> {
    let Some(transform) = exactly_one_by(
        ctx,
        transforms,
        |transform| {
            Ok(transform.definition_id == definition_id && transform.offset == section_offset)
        },
        "creo unique owner scan",
    )?
    else {
        return Ok(None);
    };
    if let Some(feature_id) = transform.feature_id {
        if exactly_one_by(
            ctx,
            transforms,
            |candidate| Ok(candidate.feature_id == Some(feature_id)),
            "creo unique transform owner scan",
        )?
        .is_none()
        {
            return Ok(None);
        }
    }
    Ok(Some(transform))
}

pub(super) fn unique_feature_definition_for_transform<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transform: &crate::placement::FeatureSectionTransform,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    exactly_one_by(
        ctx,
        definitions,
        |definition| {
            Ok(definition.identity.id() == transform.definition_id
                && definition
                    .section_3d
                    .as_ref()
                    .is_some_and(|section| section.offset == transform.offset))
        },
        "creo unique owner scan",
    )
}

pub(super) fn unique_feature_profile_definition<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transforms: &'a [crate::placement::FeatureSectionTransform],
    feature_id: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    let matches = |transform: &crate::placement::FeatureSectionTransform| {
        Ok(transform.feature_id == Some(feature_id))
    };
    let Some(index) = ctx.position_by(transforms, matches, "creo unique profile transform scan")?
    else {
        return unique_owned_feature_definition(ctx, definitions, feature_id);
    };
    if ctx.any_by(
        &transforms[index + 1..],
        matches,
        "creo unique profile transform scan",
    )? {
        return Ok(None);
    }
    unique_feature_definition_for_transform(ctx, definitions, &transforms[index])
}

pub(super) fn unique_feature_profile_ref(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<ProfileRef>, cadmpeg_core::CodecError> {
    let Some(definition) = unique_feature_profile_definition(
        ctx,
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(section_profile_ref(
        ctx,
        ir,
        feature_sketch_record_id_in_scan(ctx, scan, definition)?,
    )?))
}

pub(super) fn unique_feature_datum_plane<'a>(
    ctx: &DecodeContext<'_>,
    datums: &'a [crate::datum::DatumPlaneRecord],
    feature_id: u32,
) -> Result<Option<&'a crate::datum::DatumPlaneRecord>, CodecError> {
    exactly_one_by(
        ctx,
        datums,
        |datum| Ok(datum.feature_id == feature_id),
        "creo unique owner scan",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn unique_query_stops_at_second_match_and_preserves_refusals() {
        let values = [7, 7, 99];
        let found = crate::test_support::assert_work_boundaries(&["test unique query"], |ctx| {
            super::exactly_one_by(
                ctx,
                &values,
                |value| {
                    assert_ne!(*value, 99, "second match ends the search");
                    Ok(*value == 7)
                },
                "test unique query",
            )
            .map(Option::<&i32>::copied)
        });
        assert_eq!(found, None);
        for values in [&[][..], &[1, 7, 2][..], &[1, 2][..]] {
            let found = crate::decode::with_test_decode_ctx(|ctx| {
                super::exactly_one_by(ctx, values, |value| Ok(*value == 7), "test unique query")
                    .map(Option::<&i32>::copied)
            })
            .expect("query admitted");
            assert_eq!(found, values.contains(&7).then_some(7));
        }
    }

    #[test]
    fn datum_unique_owner_scan_refuses_work_before_query() {
        let datum = crate::datum::DatumPlaneRecord::new(
            1,
            7,
            crate::datum::DatumPlane::new(crate::axis::Axis::Z, 0.0).expect("finite plane"),
            0.0,
            [[None; 2]; 2],
            0,
        )
        .expect("finite datum");
        let datums = [datum];
        crate::test_support::assert_work_boundaries(&["creo unique owner scan"], |ctx| {
            super::unique_feature_datum_plane(ctx, &datums, 7)
                .map(|value| value.map(|value| value.id))
        });
    }
}
