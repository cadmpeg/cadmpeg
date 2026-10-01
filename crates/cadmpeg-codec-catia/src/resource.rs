// SPDX-License-Identifier: Apache-2.0
//! Charged fallible growth for CATIA decode collections.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::AnnotationBuilder;

use crate::loss::CatiaLossCode;

#[cfg(test)]
mod collection_tests {
    #[test]
    fn report_index_collections_refuse_before_growth() {
        let set = crate::test_support::with_collection_limit(0, |ctx| {
            ctx.collect_hash_set([7u32], "catia_report_set_test")
        });
        assert!(
            matches!(set, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_set_test")
        );
        let map = crate::test_support::with_collection_limit(0, |ctx| {
            ctx.collect_hash_map([(7u32, 9u32)], "catia_report_map_test")
        });
        assert!(
            matches!(map, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_map_test")
        );
        let owned = crate::test_support::with_retained_limit(0, |ctx| {
            ctx.collect_string_set(["entity"], "catia_report_owned_set_test")
        });
        assert!(
            matches!(owned, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_owned_set_test")
        );
        let service = crate::test_support::with_service_context(|ctx| {
            ctx.collect_string_set(["entity"], "catia_report_owned_set_test")
        })
        .expect("service profile admits one report key");
        assert!(service.contains("entity"));
    }

    #[test]
    fn coverage_entry_refuses_collection_and_retained_limits() {
        let key = cadmpeg_ir::report::decode::CoverageKey::new("decoded_entities");
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            cadmpeg_ir::report::decode::Coverage::default().record(ctx, key, 3)
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "decode coverage nodes"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            cadmpeg_ir::report::decode::Coverage::default().record(ctx, key, 3)
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "decode coverage names"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
        let coverage = crate::test_support::with_service_context(|ctx| {
            let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
            coverage
                .record(ctx, key, 3)
                .expect("service budget admits coverage entry");
            coverage
        });
        assert_eq!(coverage.get("decoded_entities"), Some(&3));
    }

    #[test]
    fn report_loss_refuses_retained_and_collection_limits() {
        let code = crate::loss::CatiaLossCode::HistoryModelingScopeUnresolved;
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::push_loss(
                ctx,
                &mut Vec::new(),
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_loss_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::push_loss(
                ctx,
                &mut Vec::new(),
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_loss_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
        let notes = crate::test_support::with_service_context(|ctx| {
            let mut notes = Vec::new();
            super::push_loss(
                ctx,
                &mut notes,
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
            .expect("service profile admits report loss");
            notes
        });
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code, code.kind());
    }
}

pub(crate) fn source_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(key, operation)?;
    let key = NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("CATIA source attribute key is blank"))?;
    let value = ctx.format_retained(value, operation)?;
    ctx.insert_btree_map(attributes, key, value, operation)?;
    Ok(())
}

pub(crate) fn string_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: &str,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let key = ctx.copy_retained_text(key, operation)?;
    let value = ctx.format_retained(value, operation)?;
    ctx.insert_btree_map(attributes, key, value, operation)?;
    Ok(())
}

pub(crate) fn push_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: CatiaLossCode,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(args, operation)?;
    let note = code.note_charged(ctx, message, operation)?;
    ctx.push_vec(losses, note, operation)
}

pub(crate) fn derived_annotation(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    field: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(field.len()), operation)?;
    annotations
        .derived_for_decode(ctx, id, field)
        .map_err(CodecError::from)?;
    Ok(())
}

#[cfg(test)]
mod derived_annotation_tests {
    #[test]
    fn repeated_annotation_and_coverage_keys_do_not_consume_retained_bytes() {
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
        let key = cadmpeg_ir::report::decode::CoverageKey::new("decoded_entities");
        crate::test_support::with_service_context(|ctx| {
            super::derived_annotation(
                ctx,
                &mut annotations,
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
            .expect("initial annotation");
            coverage.record(ctx, key, 1).expect("initial coverage");
        });
        crate::test_support::with_retained_limit(0, |ctx| {
            for _ in 0..64 {
                super::derived_annotation(
                    ctx,
                    &mut annotations,
                    "catia:test:vertex#0",
                    "point",
                    "catia_annotation_field",
                )
                .expect("count-only annotation update");
                coverage
                    .record(ctx, key, 1)
                    .expect("count-only coverage update");
            }
        });
        assert_eq!(
            annotations.build().exactness()["catia:test:vertex#0"]
                .fields()
                .get("point"),
            Some(&cadmpeg_ir::Exactness::Derived)
        );
    }

    #[test]
    fn derived_field_refuses_retained_and_collection_limits() {
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::derived_annotation(
                ctx,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "collect source exactness entities")
        );
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::derived_annotation(
                ctx,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "collect source exactness entities")
        );
        let annotations = crate::test_support::with_service_context(|ctx| {
            let mut builder = cadmpeg_ir::AnnotationBuilder::new();
            super::derived_annotation(
                ctx,
                &mut builder,
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
            .expect("service profile admits derived field");
            builder.build()
        });
        let fields = annotations.exactness()["catia:test:vertex#0"].fields();
        assert_eq!(fields.get("point"), Some(&cadmpeg_ir::Exactness::Derived));
    }
}

#[cfg(test)]
mod scoped_format_tests {
    #[test]
    fn scoped_format_refuses_before_temporary_string_growth() {
        let refused = crate::test_support::with_materialized_limit(0, |ctx| {
            ctx.format_scoped(format_args!("edge {}", 42), "catia_test_scoped_format")
                .map(|(text, _reservation)| text)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_test_scoped_format")
        );
        let text = crate::test_support::with_service_context(|ctx| {
            ctx.format_scoped(format_args!("edge {}", 42), "catia_test_scoped_format")
                .map(|(text, _reservation)| text)
        })
        .expect("service budget admits temporary text");
        assert_eq!(text, "edge 42");
    }
}

pub(crate) fn compose_index_id<T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    index: usize,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let id = ctx.format_retained(
        format_args!(
            "{}:{}:{}#{index}",
            namespace.format(),
            namespace.scope(),
            namespace.kind()
        ),
        operation,
    )?;
    construct(id).map_err(CodecError::malformed)
}

pub(crate) fn compose_u32_id<T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    value: u32,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let index = usize::try_from(value)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    compose_index_id(ctx, namespace, index, construct, operation)
}

#[cfg(test)]
mod id_format_tests {
    #[test]
    fn b5_numeric_identity_refuses_retained_limit_and_preserves_spelling() {
        use cadmpeg_ir::ids::CurveId;

        let namespace = cadmpeg_ir::identity_namespace!("catia", "b5", "profile");
        let limited = crate::test_support::with_retained_limit(1, |ctx| {
            super::compose_u32_id(ctx, &namespace, 42, CurveId::mint, "catia_b5_profile_id")
        });
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_b5_profile_id"));
        let id = crate::test_support::with_service_context(|ctx| {
            super::compose_u32_id(ctx, &namespace, 42, CurveId::mint, "catia_b5_profile_id")
        })
        .expect("service budget admits the identity");
        assert_eq!(id.as_str(), "catia:b5:profile#42");
    }

    #[test]
    fn native_owner_id_format_refuses_retained_limit() {
        let limited = crate::test_support::with_retained_limit(39, |ctx| {
            ctx.format_retained(
                format_args!("catia:consolidated:owner-packet#{:010}", 7),
                "catia_native_owner_packet_id",
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let id = crate::test_support::with_service_context(|ctx| {
            ctx.format_retained(
                format_args!("catia:consolidated:owner-packet#{:010}", 7),
                "catia_native_owner_packet_id",
            )
        })
        .expect("service retained budget");
        assert_eq!(id, "catia:consolidated:owner-packet#0000000007");
    }
}

pub(crate) fn copy_intcurve_support_context(
    ctx: &DecodeContext<'_>,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::IntcurveSupportContext, CodecError> {
    use cadmpeg_ir::geometry::{IntcurveSupportContext, IntcurveSupportSide, SupportPcurve};

    let [left, right] = context.sides().each_ref().map(|side| {
        Ok::<_, CodecError>(IntcurveSupportSide {
            surface: side
                .surface
                .as_ref()
                .map(|id| id.try_clone_for_decode(ctx, operation))
                .transpose()?,
            pcurve: side
                .pcurve
                .as_ref()
                .map(|pcurve| {
                    Ok::<_, CodecError>(SupportPcurve::new(
                        pcurve.geometry.try_clone_for_decode(ctx, operation)?,
                        pcurve.parameter_range,
                    ))
                })
                .transpose()?,
        })
    });
    let [first, second, third] = context
        .discontinuities()
        .each_ref()
        .map(|lane| ctx.copy_retained_slice(lane, operation));
    IntcurveSupportContext::from_parts(
        [left?, right?],
        context.parameter_range(),
        [first?, second?, third?],
    )
    .map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[test]
    fn zero_entity_source_cache_refuses_before_vacant_map_entry() {
        let mut limited_map = HashMap::<u32, u32>::new();
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            ctx.admit_hash_map_entry(&mut limited_map, &7, "catia_zero_wire_source_geometries")
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert!(limited_map.is_empty());

        crate::test_support::with_service_context(|ctx| {
            let mut admitted_map = HashMap::<u32, u32>::new();
            ctx.admit_hash_map_entry(&mut admitted_map, &7, "catia_zero_wire_source_geometries")
                .expect("service map entry budget");
            admitted_map.insert(7, 11);
            ctx.admit_hash_map_entry(&mut admitted_map, &7, "catia_zero_wire_source_geometries")
                .expect("existing entry needs no allocation");
            assert_eq!(admitted_map.get(&7), Some(&11));
        });
    }
}
