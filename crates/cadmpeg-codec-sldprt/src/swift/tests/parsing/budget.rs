use super::super::{dimension_nominal, entity, length, reference, semantic_root};
use super::encode_entity;
use crate::swift::{
    annotations, parse_unique_root, pmi_id, project_lower_profile_tier, Entity, RelatedObject,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn append_rendered_literal(payload: &mut Vec<u8>, text: &str) {
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    payload.push(u8::try_from(text.encode_utf16().count()).unwrap());
    payload.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
}

#[test]
fn swift_root_ambiguity_does_not_prepay_marker_free_tails() {
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut prefix = Vec::new();
    encode_entity(&root, &mut prefix);
    encode_entity(&root, &mut prefix);
    let mut prefix_work = None;
    for tail in [32, 16_384] {
        let mut payload = prefix.clone();
        payload.extend(vec![0; tail]);
        crate::test_support::work_refusal_at("scan SWIFT entity roots", |ctx| {
            parse_unique_root(ctx, &payload).map(|_| ())
        });
        let error = crate::test_support::work_refusal_at("measure SWIFT root scan work", |ctx| {
            assert!(parse_unique_root(ctx, &payload)?.is_none());
            ctx.copy_retained_text("x", "measure SWIFT root scan work")
        });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("root scan measurement");
        };
        if let Some(expected) = prefix_work {
            assert_eq!(
                limit.used, expected,
                "marker-free bytes after ambiguity are unvisited"
            );
        } else {
            prefix_work = Some(limit.used);
        }
    }
}

#[test]
fn swift_unqueried_rendered_literals_are_not_decoded() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for roster in ["empty", "suppressed", "explicit"] {
        let mut root = Entity {
            class: crate::swift::ROOT_CLASS.into(),
            ..Entity::default()
        };
        if roster != "empty" {
            let mut annotation = entity("GdtDiameter");
            annotation.doubles.insert("Nominal".into(), 4.0);
            if roster == "suppressed" {
                annotation.integers.insert("IsSuppressed".into(), 1);
            }
            root.annotations
                .references
                .push(reference("A10", "GdtDiameter"));
            root.annotations.entities.push(annotation);
        }
        let mut payload = Vec::new();
        encode_entity(&root, &mut payload);
        for _ in 0..64 {
            append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000");
        }
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(
            0x40,
            "SWIFT/Schema",
            &payload,
        ));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan SWIFT rendered literals",
            None,
        );
        let result = annotations(
            &ctx,
            &scan,
            &mut cadmpeg_ir::annotations::Annotations::default(),
            None,
        )
        .unwrap();
        if roster == "explicit" {
            assert_eq!(result.annotations.len(), 1);
            assert_eq!(dimension_nominal(&result.annotations, "A10"), length(4.0));
        } else {
            assert!(result.annotations.is_empty());
        }
        assert!(result.unsupported_loss.is_none());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_ambiguous_stream_roots_skip_rendered_literals_and_name_copies() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    for _ in 0..64 {
        append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000");
    }
    let mut source = crate::test_support::container::synthetic_sldprt();
    for (tag, name) in [(0x40, "SWIFT/Schema"), (0x41, "SWIFT/OtherSchema")] {
        source.extend(crate::test_support::container::make_block(
            tag, name, &payload,
        ));
    }
    let scan = crate::test_support::container::scan(&source);
    for operation in [
        "scan SWIFT rendered literals",
        "copy SWIFT source stream name",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert!(crate::swift::scan_root(&ctx, &scan).unwrap().is_none());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_rendered_dimension_cache_is_built_at_first_query_and_reused() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let mut payload = Vec::new();
    append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cache = crate::swift::RenderedDimensions::Stream {
        payload: &payload,
        values: std::cell::OnceCell::new(),
    };
    assert_eq!(cache.get(&ctx).unwrap().len(), 1);
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "scan SWIFT rendered literals",
        None,
    );
    assert_eq!(cache.get(&ctx).unwrap().first().unwrap().value.get(), 4.0);
    drop(cache);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse rendered cache storage",
        )
        .unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_rendered_literals_hold_only_surviving_vector_storage() {
    let mut payload = Vec::new();
    for _ in 0..64 {
        append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000");
    }
    let live_storage = |decode| {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "measure rendered dimension storage",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let (values, storage) =
                    ctx.with_scoped_storage("rendered dimension test output", || {
                        if decode {
                            crate::swift::rendered_dimensions(&ctx, &payload)
                        } else {
                            let mut values = Vec::new();
                            for _ in 0..64 {
                                ctx.push_vec(
                                    &mut values,
                                    crate::swift::RenderedDimension {
                                        kind: crate::swift::RenderedDimensionKind::Diameter,
                                        value: crate::swift::PositiveReal::new(4.0).unwrap(),
                                        decimal_places: 3,
                                    },
                                    "expected rendered dimensions",
                                )?;
                            }
                            Ok(values)
                        }
                    })?;
                assert_eq!(values.len(), 64);
                assert!(values
                    .iter()
                    .all(|value| value.value.get() == 4.0 && value.decimal_places == 3));
                let measurement =
                    ctx.reserve_scoped(u64::MAX / 2, "measure rendered dimension storage")?;
                drop((measurement, values, storage));
                Ok(())
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("rendered storage measurement");
        };
        limit.used
    };
    assert_eq!(live_storage(true), live_storage(false));
}

#[test]
fn swift_unsupported_annotations_release_their_admitted_identity() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.annotations
        .references
        .push(reference("A10", "GdtUnrecognized"));
    root.annotations.entities.push(entity("GdtUnrecognized"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::swift::project_with_topology(
        &ctx,
        &root,
        None,
        &crate::swift::RenderedDimensions::Borrowed(&[]),
        None,
        None,
    )
    .unwrap();
    assert!(result.annotations.is_empty());
    assert_eq!(result.unsupported.get("GdtUnrecognized"), Some(&1));
    drop(result);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse unsupported identity storage",
        )
        .unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_lower_tier_identity_retains_only_its_final_text() {
    let reference = reference("A10", "GdtSurfaceProfile");
    let mut entity = entity("GdtSurfaceProfile");
    entity.doubles.insert("ToleranceLowerTier".into(), 0.25);
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "format SWIFT lower-tier ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
            project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), &geometry)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("lower-tier retained boundary");
    };
    assert_eq!(limit.used, 0, "the validation identity is temporary");
}

#[test]
fn swift_root_marker_search_skips_unvisited_tails() {
    let mut prefix_work = None;
    for tail in [0, 96] {
        let name = format!("SWIFT/Schema{}", "x".repeat(tail));
        let mut payload = crate::swift::ROOT_CLASS.as_bytes().to_vec();
        payload.extend(vec![
            0;
            tail.checked_mul(128).expect("test tail fits usize")
        ]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(
            0x40, &name, &payload,
        ));
        let scan = crate::test_support::container::scan(&source);
        for operation in [
            "find SWIFT schema section name",
            "scan SWIFT root class marker",
        ] {
            let error = crate::test_support::work_refusal_at(operation, |ctx| {
                crate::swift::has_root_marker(ctx, &scan)
            });
            let CodecError::ResourceLimit(limit) = error else {
                panic!("marker search boundary");
            };
            assert_eq!(limit.additional, 1);
        }
        let error = crate::test_support::work_refusal_at("measure SWIFT marker work", |ctx| {
            assert!(crate::swift::has_root_marker(ctx, &scan)?);
            ctx.copy_retained_text("x", "measure SWIFT marker work")
        });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("marker search measurement");
        };
        if let Some(expected) = prefix_work {
            assert_eq!(limit.used, expected);
        } else {
            prefix_work = Some(limit.used);
        }
    }
}

#[test]
fn swift_ambiguous_annotation_fallback_skips_unvisited_tails() {
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut prefix = Vec::new();
    encode_entity(&root, &mut prefix);
    encode_entity(&root, &mut prefix);
    let mut prefix_work = None;
    for tail in [32, 16_384] {
        let mut payload = prefix.clone();
        payload.extend(vec![0; tail]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(
            0x40,
            "SWIFT/Schema",
            &payload,
        ));
        let scan = crate::test_support::container::scan(&source);
        crate::test_support::work_refusal_at("scan SWIFT root class marker", |ctx| {
            annotations(
                ctx,
                &scan,
                &mut cadmpeg_ir::annotations::Annotations::default(),
                None,
            )
        });
        let error = crate::test_support::work_refusal_at(
            "measure ambiguous SWIFT annotation work",
            |ctx| {
                let result = annotations(
                    ctx,
                    &scan,
                    &mut cadmpeg_ir::annotations::Annotations::default(),
                    None,
                )?;
                assert!(result.annotations.is_empty());
                assert!(result
                    .unsupported_loss
                    .as_deref()
                    .unwrap()
                    .contains("GdtAnalysisGraphUnresolved"));
                ctx.copy_retained_text("x", "measure ambiguous SWIFT annotation work")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("annotation work measurement");
        };
        if let Some(expected) = prefix_work {
            assert_eq!(limit.used, expected);
        } else {
            prefix_work = Some(limit.used);
        }
    }
}

#[test]
fn swift_rejected_targets_release_partial_output_and_skip_datum_copy() {
    let mut datum = entity("GdtDatum");
    datum
        .strings
        .insert("DatumIdentifier".into(), "A".repeat(128));
    datum.features.references = vec![reference("F10", "GdtPlane"), reference(" ", "GdtPlane")];
    let features = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "copy SWIFT datum identifier",
        None,
    );
    assert!(crate::swift::project_datum(
        &ctx,
        pmi_id("A10").unwrap(),
        &datum,
        || Ok(&features),
        &geometry
    )
    .unwrap()
    .is_none());
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse rejected target storage",
        )
        .unwrap();
    drop(reuse);
    drop(geometry);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_roster_without_datum_references_skips_identity_and_system_indexes() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    for ordinal in 0..64 {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        root.annotations
            .references
            .push(reference(&format!("A{ordinal}"), "GdtDatum"));
        root.annotations.entities.push(datum);
    }
    let mut tolerance = entity("GdtFlatness");
    tolerance.doubles.insert("Tolerance".into(), 0.25);
    root.annotations
        .references
        .push(reference("A80", "GdtFlatness"));
    root.annotations.entities.push(tolerance);
    for operation in [
        "index SWIFT datum IDs",
        "match SWIFT datum-system references",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let projection = crate::swift::project_with_topology(
            &ctx,
            &root,
            None,
            &crate::swift::RenderedDimensions::Borrowed(&[]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(projection.annotations.len(), 65);
        assert_eq!(
            projection
                .annotations
                .iter()
                .filter(|annotation| matches!(
                    annotation.definition,
                    cadmpeg_ir::pmi::PmiDefinition::Datum { .. }
                ))
                .count(),
            64
        );
        assert_eq!(
            projection
                .annotations
                .iter()
                .filter(|annotation| matches!(
                    annotation.definition,
                    cadmpeg_ir::pmi::PmiDefinition::GeometricTolerance { .. }
                ))
                .count(),
            1
        );
        drop((projection, probe));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_datum_identity_index_is_admitted_once_and_releases_scratch() {
    let root = semantic_root();
    crate::test_support::work_refusal_at("index SWIFT datum IDs", |ctx| {
        let identities = crate::swift::DatumIdentities::new(&root);
        identities.lookup(ctx, "A10").map(|_| ())
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let identities = crate::swift::DatumIdentities::new(&root);
    assert_eq!(
        identities
            .lookup(&ctx, "A10")
            .unwrap()
            .and_then(pmi_id)
            .as_ref(),
        Some(&pmi_id("A10").unwrap())
    );
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SWIFT datum IDs",
        None,
    );
    assert_eq!(
        identities
            .lookup(&ctx, "A10")
            .unwrap()
            .and_then(pmi_id)
            .as_ref(),
        Some(&pmi_id("A10").unwrap())
    );
    assert!(identities.lookup(&ctx, "A20").unwrap().is_none());
    drop((identities, probe));
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse datum identity storage",
        )
        .unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_datum_grouping_counts_unresolved_applied_records_in_one_pass() {
    let root = semantic_root();
    for (class, expected_group) in [("GdtUnrecognized", None), ("GdtAppliedDatum", Some(1))] {
        let mut tolerance = root.annotations.entities.get(1).unwrap().clone();
        let collection = tolerance.related.first_mut().unwrap();
        assert_eq!(collection.name, "PrimaryDatums");
        let extra = entity(class);
        collection.entity.related.push(RelatedObject {
            name: "SubAnnotation1".into(),
            class: extra.class.clone(),
            entity: extra,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let identities = crate::swift::DatumIdentities::new(&root);
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan SLDPRT datum_references values",
            None,
        );
        let references = crate::swift::datum_references(&ctx, &tolerance, &identities).unwrap();
        assert_eq!(references.len(), 1);
        let first = references.first().unwrap();
        assert_eq!(first.datum, pmi_id("A10").unwrap());
        assert_eq!(first.precedence.get(), 1);
        assert_eq!(first.common_group, expected_group);
        drop((identities, probe));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_reused_datum_system_drops_temporary_reference_storage() {
    use std::cell::Cell;
    let retained = |count: usize| {
        let source = semantic_root();
        let mut root = Entity {
            class: crate::swift::ROOT_CLASS.into(),
            ..Entity::default()
        };
        root.annotations
            .references
            .push(reference("A10", "GdtDatum"));
        root.annotations
            .entities
            .push(source.annotations.entities.first().unwrap().clone());
        let mut tolerance = entity("GdtPosition");
        tolerance.doubles.insert("Tolerance".into(), 0.25);
        tolerance.related = source.annotations.entities.get(1).unwrap().related.clone();
        for id in ["A20", "A21"].into_iter().take(count) {
            root.annotations
                .references
                .push(reference(id, "GdtPosition"));
            root.annotations.entities.push(tolerance.clone());
        }
        let arena = DecodeArena::new();
        let capacity = Cell::new(0);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "measure SWIFT tolerance output storage",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                policy.limits.max_work_units = u64::MAX;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let projection = crate::swift::project_with_topology(
                    &ctx,
                    &root,
                    None,
                    &crate::swift::RenderedDimensions::Borrowed(&[]),
                    None,
                    None,
                )?;
                assert_eq!(projection.annotations.len(), count.checked_add(2).unwrap());
                assert_eq!(
                    projection
                        .annotations
                        .iter()
                        .filter(|annotation| matches!(
                            annotation.definition,
                            cadmpeg_ir::pmi::PmiDefinition::DatumSystem { .. }
                        ))
                        .count(),
                    1
                );
                capacity.set(projection.annotations.capacity());
                let measurement =
                    ctx.copy_retained_text("x", "measure SWIFT tolerance output storage");
                drop(projection);
                measurement
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("tolerance output storage measurement");
        };
        (limit.used, capacity.get())
    };
    let (first, first_capacity) = retained(1);
    let (second, second_capacity) = retained(2);
    assert_eq!(first_capacity, second_capacity);
    let extra_id = pmi_id("A21").unwrap();
    let system_id = format!("{}:datum-system", pmi_id("A20").unwrap().as_str());
    let expected = u64::try_from(
        extra_id
            .as_str()
            .len()
            .checked_add(system_id.len())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        second.checked_sub(first).unwrap(),
        expected,
        "the extra tolerance retains its two output identities, not a discarded reference vector"
    );
}

#[test]
fn swift_pmi_identity_grammar_preserves_resource_refusal() {
    crate::test_support::work_refusal_at("validate SWIFT PMI identity", |ctx| {
        crate::swift::pmi_id_charged(ctx, "A10")
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::swift::pmi_id_charged(&ctx, "A10").unwrap(),
        pmi_id("A10")
    );
    assert!(crate::swift::pmi_id_charged(&ctx, " ").unwrap().is_none());
}

#[test]
fn swift_derived_identity_grammars_preserve_resource_refusal() {
    let root = semantic_root();
    crate::test_support::work_refusal_at("validate SWIFT datum-system ID", |ctx| {
        crate::swift::project_with_topology(
            ctx,
            &root,
            None,
            &crate::swift::RenderedDimensions::Borrowed(&[]),
            None,
            None,
        )
        .map(|_| ())
    });
    let mut tier = entity("GdtCompositeSurfaceProfile");
    tier.doubles.insert("ToleranceLowerTier".into(), 0.1);
    crate::test_support::work_refusal_at("validate SWIFT lower-tier ID", |ctx| {
        let geometry = crate::swift::ProjectionGeometry::new(ctx, None, None, None);
        project_lower_profile_tier(
            ctx,
            &reference("A10", "GdtCompositeSurfaceProfile"),
            &tier,
            &BTreeMap::new(),
            &geometry,
        )
    });
}

#[test]
fn swift_modifier_strings_and_slots_are_admitted() {
    for value in [1, 2, 3] {
        let copy = if value == 3 {
            "format SWIFT PMI modifier"
        } else {
            "copy SWIFT PMI modifier"
        };
        crate::test_support::work_refusal_at(copy, |ctx| {
            crate::swift::integer_modifier(ctx, Some(value))
        });
        let arena = DecodeArena::new();
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "collect SWIFT PMI modifiers",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                crate::swift::integer_modifier(&ctx, Some(value))
            },
        );
    }
    let root = semantic_root();
    let tolerance = root.annotations.entities.get(1).unwrap();
    let arena = DecodeArena::new();
    for operation in ["copy SWIFT PMI modifier", "collect SWIFT PMI modifiers"] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                crate::swift::tolerance_modifiers(&ctx, tolerance)
            },
        );
    }
    let mut flags = entity("GdtPosition");
    for key in [
        "IsFreeState",
        "IsStatistical",
        "IsToBeInspected",
        "IsTangentPlane",
        "ProjectedZoneEnabled",
        "IsMaxTolerance",
    ] {
        flags.integers.insert(key.into(), 1);
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::swift::tolerance_modifiers(&ctx, &flags).unwrap(),
        [
            "free_state",
            "statistical",
            "inspection",
            "tangent_plane",
            "projected_zone",
            "maximum_tolerance"
        ]
    );
    flags.doubles.insert("ProjectedZoneValue".into(), 4.0);
    flags.doubles.insert("MaxTolerance".into(), 2.0);
    assert_eq!(
        crate::swift::tolerance_modifiers(&ctx, &flags).unwrap(),
        [
            "free_state",
            "statistical",
            "inspection",
            "tangent_plane",
            "projected_zone:4_mm",
            "maximum_tolerance:2_mm"
        ]
    );
    let mut tier = entity("GdtCompositeSurfaceProfile");
    tier.doubles.insert("ToleranceLowerTier".into(), 0.1);
    crate::test_support::work_refusal_at("copy SWIFT PMI modifier", |ctx| {
        let geometry = crate::swift::ProjectionGeometry::new(ctx, None, None, None);
        project_lower_profile_tier(
            ctx,
            &reference("A10", "GdtCompositeSurfaceProfile"),
            &tier,
            &BTreeMap::new(),
            &geometry,
        )
    });
    crate::test_support::work_refusal_at("copy SWIFT PMI modifier", |ctx| {
        let identities = crate::swift::DatumIdentities::new(&root);
        crate::swift::datum_references(ctx, tolerance, &identities)
    });
}

#[test]
fn swift_rejected_target_prefix_does_not_prepay_reference_tails() {
    let mut work = None;
    for tail in [1, 128] {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        datum.features.references = vec![reference("F10", "GdtPlane"), reference(" ", "GdtPlane")];
        datum
            .features
            .references
            .extend((0..tail).map(|index| reference(&format!("tail{index}"), "GdtPlane")));
        let features = BTreeMap::new();
        let query = |ctx: &DecodeContext<'_>| {
            let geometry = crate::swift::ProjectionGeometry::new(ctx, None, None, None);
            crate::swift::project_datum(
                ctx,
                pmi_id("A10").unwrap(),
                &datum,
                || Ok(&features),
                &geometry,
            )
        };
        crate::test_support::work_refusal_at("scan SLDPRT targets values", query);
        let error = crate::test_support::work_refusal_at("measure rejected target prefix", |ctx| {
            assert!(query(ctx)?.is_none());
            ctx.copy_retained_text("x", "measure rejected target prefix")
        });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("prefix measurement")
        };
        if let Some(expected) = work {
            assert_eq!(limit.used, expected);
        } else {
            work = Some(limit.used);
        }
    }
}

#[test]
fn swift_single_datum_query_does_not_mint_unqueried_identities() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let original = semantic_root();
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    for ordinal in 0..64 {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        root.annotations.references.push(reference(
            &format!("A{ordinal}{}", "x".repeat(128)),
            "GdtDatum",
        ));
        root.annotations.entities.push(datum);
    }
    root.annotations
        .references
        .push(reference("A10", "GdtDatum"));
    // Its own projection fails, but its source identity remains eligible.
    let mut datum = original.annotations.entities.first().unwrap().clone();
    datum.features.references.push(reference(" ", "GdtPlane"));
    root.annotations.entities.push(datum);
    let tolerance = original.annotations.entities.get(1).unwrap();
    let long_id = &root.annotations.references.first().unwrap().id;
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "format SWIFT PMI identity",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let (id, storage) = ctx.with_scoped_storage("datum identity control", || {
                crate::swift::pmi_id_charged(&ctx, long_id)
            })?;
            drop((id, storage));
            Ok(())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("long identity control")
    };
    assert_eq!(
        limit.additional,
        u64::try_from(format!("sldprt:model:pmi#{long_id}").len()).unwrap()
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let no_features = BTreeMap::new();
    assert!(crate::swift::project_datum(
        &ctx,
        pmi_id("A10").unwrap(),
        root.annotations.entities.last().unwrap(),
        || Ok(&no_features),
        &geometry
    )
    .unwrap()
    .is_none());
    let identities = crate::swift::DatumIdentities::new(&root);
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "format SWIFT PMI identity",
        Some(limit.additional),
    );
    let references = crate::swift::datum_references(&ctx, tolerance, &identities).unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references.first().unwrap().datum, pmi_id("A10").unwrap());
    drop((identities, probe, geometry));
    ctx.finish_session().unwrap();
    let mut invalid = root;
    invalid.annotations.references.last_mut().unwrap().id = " ".into();
    let mut tolerance = tolerance.clone();
    tolerance
        .related
        .first_mut()
        .unwrap()
        .entity
        .related
        .first_mut()
        .unwrap()
        .entity
        .annotations
        .references
        .first_mut()
        .unwrap()
        .id = " ".into();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(crate::swift::datum_references(
        &ctx,
        &tolerance,
        &crate::swift::DatumIdentities::new(&invalid)
    )
    .unwrap()
    .is_empty());
}

#[test]
fn swift_related_descriptor_steps_preserve_resource_refusal() {
    let mut payload = Vec::new();
    encode_entity(&semantic_root(), &mut payload);
    crate::test_support::work_refusal_at("scan SWIFT related descriptors", |ctx| {
        parse_unique_root(ctx, &payload).map(|_| ())
    });
}

#[test]
fn swift_defined_area_unit_storage_is_admitted() {
    for (kind, expected) in [
        (Some(0), "rectangular"),
        (Some(1), "circular"),
        (Some(2), "sldprt:2"),
        (None, "sldprt:unspecified"),
    ] {
        let mut tolerance = entity("GdtSurfaceProfile");
        tolerance.integers.insert("PerUnitArea".into(), 1);
        if let Some(kind) = kind {
            tolerance.integers.insert("PerUnitAreaType".into(), kind);
        }
        let operation = if kind == Some(2) {
            "format SWIFT defined-area unit"
        } else {
            "copy SWIFT defined-area unit"
        };
        crate::test_support::work_refusal_at(operation, |ctx| {
            crate::swift::defined_area(ctx, &tolerance)
        });
        let arena = DecodeArena::new();
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                crate::swift::defined_area(&ctx, &tolerance)
            },
        );
        assert_eq!(
            crate::swift::defined_area(&cadmpeg_test_support::service_decode_context(), &tolerance)
                .unwrap(),
            (None, Some(expected.into()), None)
        );
    }
}

#[test]
fn swift_literal_related_names_do_not_charge_the_unmatched_text_extent() {
    let mut work = None;
    for length in [1, 128] {
        let mut feature = entity("GdtCylinder");
        feature.related = vec![RelatedObject {
            name: "x".repeat(length),
            class: "Other".into(),
            entity: entity("Other"),
        }];
        let mut geometry = entity("GeoCylinder");
        geometry.doubles.insert("R".into(), 2.0);
        feature.related.push(RelatedObject {
            name: "NomCylinder".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        let error =
            crate::test_support::work_refusal_at("measure literal related name search", |ctx| {
                assert_eq!(
                    crate::swift::nominal_radius(ctx, &feature, "NomCylinder")?
                        .map(crate::swift::PositiveReal::get),
                    Some(2.0)
                );
                ctx.copy_retained_text("x", "measure literal related name search")
            });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("literal search measurement")
        };
        if let Some(expected) = work {
            assert_eq!(limit.used, expected);
        } else {
            work = Some(limit.used);
        }
    }
}

#[test]
fn swift_related_descriptors_copy_only_bound_entities() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, View};
    use super::put_pstr;
    let related_entity = entity("GdtCylinder");
    for operation in ["copy SWIFT related name", "copy SWIFT related class"] {
        for invalid_entity in [true, false] {
            for count in [2u32, 64] {
                let mut payload = Vec::new();
                payload.extend(count.to_le_bytes());
                for _ in 0..count {
                    put_pstr(&mut payload, &"n".repeat(200));
                    put_pstr(&mut payload, "DifferentClass");
                }
                if invalid_entity {
                    put_pstr(&mut payload, "Bad");
                } else {
                    encode_entity(&related_entity, &mut payload);
                }
                let arena = DecodeArena::new();
                let (ctx, mut cursor) = DecodeContext::from_root_bytes(
                    &payload, &arena, &DecodePolicy::service(),
                ).unwrap();
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                assert!(crate::swift::read_related(&ctx, &mut cursor, 0).unwrap().is_none());
                ctx.finish_session().unwrap();
            }
        }
        let mut payload = Vec::new();
        payload.extend(1u32.to_le_bytes());
        put_pstr(&mut payload, "NomCylinder");
        put_pstr(&mut payload, &related_entity.class);
        encode_entity(&related_entity, &mut payload);
        put_pstr(&mut payload, "EndRelatedObjects");
        crate::test_support::work_refusal_at(operation, |ctx| {
            crate::swift::read_related(ctx, &mut View::over_retained(&payload), 0)
        });
        let ctx = cadmpeg_test_support::service_decode_context();
        let objects = crate::swift::read_related(&ctx, &mut View::over_retained(&payload), 0)
            .unwrap().unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "NomCylinder");
        assert_eq!(objects[0].class, related_entity.class);
    }
}
