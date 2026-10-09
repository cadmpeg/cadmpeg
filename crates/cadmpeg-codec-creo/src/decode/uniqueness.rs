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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut first = None;
    let mut pending = values.iter();
    while pending.len() != 0 {
        let Some(value) = ctx.next_charged(&mut pending, operation)? else {
            break;
        };
        if predicate(value)? {
            if first.is_some() {
                return Ok(None);
            }
            first = Some(value);
        }
    }
    Ok(first)
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
    )? else {
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut first = None;
    let mut pending = transforms.iter();
    while pending.len() != 0 {
        let Some(transform) =
            ctx.next_charged(&mut pending, "creo unique profile transform scan")?
        else {
            break;
        };
        if transform.feature_id == Some(feature_id) {
            if first.is_some() {
                return Ok(None);
            }
            first = Some(transform);
        }
    }
    match first {
        Some(transform) => unique_feature_definition_for_transform(ctx, definitions, transform),
        None => unique_owned_feature_definition(ctx, definitions, feature_id),
    }
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

    #[test]
    fn duplicate_datum_owner_ignores_unvisited_tail() {
        let datum = crate::datum::DatumPlaneRecord::new(
            1,
            7,
            crate::datum::DatumPlane::new(crate::axis::Axis::Z, 0.0).expect("finite plane"),
            0.0,
            [[None; 2]; 2],
            0,
        )
        .expect("finite datum");
        let short = [datum.clone(), datum.clone()];
        let long = vec![datum; 4096];
        let refusal = |datums: &[crate::datum::DatumPlaneRecord]| {
            crate::test_support::last_refusal_at(
                &[],
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "creo unique owner scan",
                |ctx| {
                    super::unique_feature_datum_plane(ctx, datums, 7)
                        .map(|value| value.map(|value| value.id))
                },
            )
        };
        let cadmpeg_core::CodecError::ResourceLimit(short_refusal) = refusal(&short) else {
            panic!("duplicate owner work refusal");
        };
        let cadmpeg_core::CodecError::ResourceLimit(long_refusal) = refusal(&long) else {
            panic!("duplicate owner work refusal");
        };
        assert_eq!(short_refusal, long_refusal);
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            super::unique_feature_datum_plane(ctx, &long, 7)
                .map(|value| value.map(|value| value.id))
        })
        .expect("duplicate owner query")
        .is_none());
    }

    #[test]
    fn unique_owner_empty_routes_are_free_and_keep_original_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(super::exactly_one_by(&ctx, &[] as &[u32], |_| panic!("absent row"),
            "test present unique rows").expect("empty query"), None);
        assert!(super::unique_feature_datum_plane(&ctx, &[], 7).expect("empty datums").is_none());
        assert!(super::unique_feature_profile_definition(&ctx, &[], &[], 7)
            .expect("empty profile sources").is_none());
        let original = ctx.charge_work_limit(1, "seed unique owner refusal").expect_err("zero cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(super::exactly_one_by(&ctx, &[] as &[u32], |_| panic!("absent row"),
            "test present unique rows"), Err(cadmpeg_core::CodecError::ResourceLimit(r)) if r == original));
        assert!(matches!(super::unique_feature_datum_plane(&ctx, &[], 7),
            Err(cadmpeg_core::CodecError::ResourceLimit(r)) if r == original));
        assert!(matches!(super::unique_feature_profile_definition(&ctx, &[], &[], 7),
            Err(cadmpeg_core::CodecError::ResourceLimit(r)) if r == original));
    }

    #[test]
    fn unique_owner_search_admits_present_rows_before_predicates() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        for (values, expected, visits) in [
            ([7, 7, 99], None, 2),
            ([1, 7, 2], Some(7), 3),
            ([1, 2, 3], None, 3),
        ] {
            for allowed in 0..=visits {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = allowed;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                let predicates = std::cell::Cell::new(0_u64);
                let result = super::exactly_one_by(&ctx, &values, |value| {
                    predicates.set(predicates.get() + 1);
                    assert_ne!(*value, 99, "second match stops before the tail");
                    Ok(*value == 7)
                }, "test present unique rows").map(Option::<&i32>::copied);
                if allowed < visits {
                    let cadmpeg_core::CodecError::ResourceLimit(r) = result.expect_err("next present row") else {
                        panic!("work refusal");
                    };
                    assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(r.operation, "test present unique rows");
                    assert_eq!((r.used, r.additional), (allowed, 1));
                    assert_eq!(predicates.get(), allowed);
                    assert!(matches!(super::exactly_one_by(&ctx, &[] as &[u32], |_| panic!("absent row"),
                        "test present unique rows"), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == r));
                } else {
                    assert_eq!(result.expect("all required rows"), expected);
                    assert_eq!(predicates.get(), visits);
                    let r = ctx.charge_work_limit(1, "after present unique rows").expect_err("exact cap");
                    assert_eq!((r.used, r.additional), (visits, 1));
                }
            }
        }
    }

    #[test]
    fn unique_profile_transform_search_charges_only_visited_rows() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let transform = |id, owner| crate::placement::FeatureSectionTransform::new(
            id, Some(owner), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
        ).expect("orthonormal source frame");
        for (transforms, visits) in [
            ([transform(1, 7), transform(2, 7), transform(3, 99)], 2),
            ([transform(1, 1), transform(2, 7), transform(3, 2)], 3),
            ([transform(1, 1), transform(2, 2), transform(3, 3)], 3),
        ] {
            for allowed in 0..=visits {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = allowed;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                let result = super::unique_feature_profile_definition(&ctx, &[], &transforms, 7);
                if allowed < visits {
                    let cadmpeg_core::CodecError::ResourceLimit(r) = result.expect_err("next transform") else {
                        panic!("work refusal");
                    };
                    assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(r.operation, "creo unique profile transform scan");
                    assert_eq!((r.used, r.additional), (allowed, 1));
                    assert!(matches!(super::unique_feature_profile_definition(&ctx, &[], &[], 7),
                        Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == r));
                } else {
                    assert!(result.expect("visited transforms and absent definitions").is_none());
                    let r = ctx.charge_work_limit(1, "after profile transforms").expect_err("exact cap");
                    assert_eq!((r.used, r.additional), (visits, 1));
                }
            }
        }
    }

}
