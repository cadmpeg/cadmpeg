use super::super::{dimension_nominal, entity, length, reference, semantic_root};
use super::encode_entity;
use crate::swift::{annotations, parse_unique_root, pmi_id, project_lower_profile_tier, Entity, RelatedObject};
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
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
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
        let CodecError::ResourceLimit(limit) = error else { panic!("root scan measurement"); };
        if let Some(expected) = prefix_work {
            assert_eq!(limit.used, expected, "marker-free bytes after ambiguity are unvisited");
        } else {
            prefix_work = Some(limit.used);
        }
    }
}

#[test]
fn swift_unqueried_rendered_literals_are_not_decoded() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for roster in ["empty", "suppressed", "explicit"] {
        let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
        if roster != "empty" {
            let mut annotation = entity("GdtDiameter");
            annotation.doubles.insert("Nominal".into(), 4.0);
            if roster == "suppressed" {
                annotation.integers.insert("IsSuppressed".into(), 1);
            }
            root.annotations.references.push(reference("A10", "GdtDiameter"));
            root.annotations.entities.push(annotation);
        }
        let mut payload = Vec::new();
        encode_entity(&root, &mut payload);
        for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SWIFT rendered literals", None);
        let result = annotations(&ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None).unwrap();
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
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
    let mut source = crate::test_support::container::synthetic_sldprt();
    for (tag, name) in [(0x40, "SWIFT/Schema"), (0x41, "SWIFT/OtherSchema")] {
        source.extend(crate::test_support::container::make_block(tag, name, &payload));
    }
    let scan = crate::test_support::container::scan(&source);
    for operation in ["scan SWIFT rendered literals", "copy SWIFT source stream name"] {
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
    let cache = crate::swift::RenderedDimensions::Stream { payload: &payload, values: std::cell::OnceCell::new() };
    assert_eq!(cache.get(&ctx).unwrap().len(), 1);
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SWIFT rendered literals", None);
    assert_eq!(cache.get(&ctx).unwrap().first().unwrap().value.get(), 4.0);
    drop(cache);
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse rendered cache storage").unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_rendered_literals_hold_only_surviving_vector_storage() {
    let mut payload = Vec::new();
    for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
    let live_storage = |decode| {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, "measure rendered dimension storage", |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let (values, storage) = ctx.with_scoped_storage("rendered dimension test output", || {
                    if decode {
                        crate::swift::rendered_dimensions(&ctx, &payload)
                    } else {
                        let mut values = Vec::new();
                        for _ in 0..64 {
                            ctx.push_vec(&mut values, crate::swift::RenderedDimension {
                                kind: crate::swift::RenderedDimensionKind::Diameter,
                                value: crate::swift::PositiveReal::new(4.0).unwrap(), decimal_places: 3,
                            }, "expected rendered dimensions")?;
                        }
                        Ok(values)
                    }
                })?;
                assert_eq!(values.len(), 64);
                assert!(values.iter().all(|value| value.value.get() == 4.0 && value.decimal_places == 3));
                let measurement = ctx.reserve_scoped(u64::MAX / 2, "measure rendered dimension storage")?;
                drop((measurement, values, storage));
                Ok(())
            });
        let CodecError::ResourceLimit(limit) = error else { panic!("rendered storage measurement"); };
        limit.used
    };
    assert_eq!(live_storage(true), live_storage(false));
}

#[test]
fn swift_unsupported_annotations_release_their_admitted_identity() {
    let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    root.annotations.references.push(reference("A10", "GdtUnrecognized"));
    root.annotations.entities.push(entity("GdtUnrecognized"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::swift::project_with_topology(&ctx, &root, None,
        &crate::swift::RenderedDimensions::Borrowed(&[]), None, None).unwrap();
    assert!(result.annotations.is_empty());
    assert_eq!(result.unsupported.get("GdtUnrecognized"), Some(&1));
    drop(result);
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse unsupported identity storage").unwrap();
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
        ResourceDimension::RetainedBytes, "format SWIFT lower-tier ID", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
            project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), &geometry)
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("lower-tier retained boundary"); };
    assert_eq!(limit.used, 0, "the validation identity is temporary");
}

#[test]
fn swift_root_marker_search_skips_unvisited_tails() {
    let mut prefix_work = None;
    for tail in [0, 96] {
        let name = format!("SWIFT/Schema{}", "x".repeat(tail));
        let mut payload = crate::swift::ROOT_CLASS.as_bytes().to_vec();
        payload.extend(vec![0; tail.checked_mul(128).expect("test tail fits usize")]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, &name, &payload));
        let scan = crate::test_support::container::scan(&source);
        for operation in ["find SWIFT schema section name", "scan SWIFT root class marker"] {
            let error = crate::test_support::work_refusal_at(operation, |ctx| {
                crate::swift::has_root_marker(ctx, &scan)
            });
            let CodecError::ResourceLimit(limit) = error else { panic!("marker search boundary"); };
            assert_eq!(limit.additional, 1);
        }
        let error = crate::test_support::work_refusal_at("measure SWIFT marker work", |ctx| {
            assert!(crate::swift::has_root_marker(ctx, &scan)?);
            ctx.copy_retained_text("x", "measure SWIFT marker work")
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("marker search measurement"); };
        if let Some(expected) = prefix_work { assert_eq!(limit.used, expected); }
        else { prefix_work = Some(limit.used); }
    }
}

#[test]
fn swift_ambiguous_annotation_fallback_skips_unvisited_tails() {
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    let mut prefix = Vec::new();
    encode_entity(&root, &mut prefix);
    encode_entity(&root, &mut prefix);
    let mut prefix_work = None;
    for tail in [32, 16_384] {
        let mut payload = prefix.clone();
        payload.extend(vec![0; tail]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
        let scan = crate::test_support::container::scan(&source);
        crate::test_support::work_refusal_at("scan SWIFT root class marker", |ctx| {
            annotations(ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None)
        });
        let error = crate::test_support::work_refusal_at("measure ambiguous SWIFT annotation work", |ctx| {
            let result = annotations(ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None)?;
            assert!(result.annotations.is_empty());
            assert!(result.unsupported_loss.as_deref().unwrap().contains("GdtAnalysisGraphUnresolved"));
            ctx.copy_retained_text("x", "measure ambiguous SWIFT annotation work")
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("annotation work measurement"); };
        if let Some(expected) = prefix_work { assert_eq!(limit.used, expected); }
        else { prefix_work = Some(limit.used); }
    }
}

#[test]
fn swift_rejected_targets_release_partial_output_and_skip_datum_copy() {
    let mut datum = entity("GdtDatum");
    datum.strings.insert("DatumIdentifier".into(), "A".repeat(128));
    datum.features.references = vec![reference("F10", "GdtPlane"), reference(" ", "GdtPlane")];
    let features = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "copy SWIFT datum identifier", None,
    );
    assert!(crate::swift::project_datum(&ctx, pmi_id("A10").unwrap(), &datum, || Ok(&features), &geometry).unwrap().is_none());
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse rejected target storage").unwrap();
    drop(reuse);
    drop(geometry);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_datum_only_roster_skips_reference_identity_index() {
    let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    for ordinal in 0..64 {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        root.annotations.references.push(reference(&format!("A{ordinal}"), "GdtDatum"));
        root.annotations.entities.push(datum);
    }
    let mut tolerance = entity("GdtFlatness");
    tolerance.doubles.insert("Tolerance".into(), 0.25);
    root.annotations.references.push(reference("A80", "GdtFlatness"));
    root.annotations.entities.push(tolerance);
    for operation in ["index SWIFT datum IDs", "match SWIFT datum-system references"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, operation, None,
        );
        let projection = crate::swift::project_with_topology(&ctx, &root, None,
            &crate::swift::RenderedDimensions::Borrowed(&[]), None, None).unwrap();
        assert_eq!(projection.annotations.len(), 65);
        assert_eq!(projection.annotations.iter().filter(|annotation| matches!(annotation.definition,
            cadmpeg_ir::pmi::PmiDefinition::Datum { .. })).count(), 64);
        assert_eq!(projection.annotations.iter().filter(|annotation| matches!(annotation.definition,
            cadmpeg_ir::pmi::PmiDefinition::GeometricTolerance { .. })).count(), 1);
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
    assert_eq!(identities.lookup(&ctx, "A10").unwrap(), Some(&pmi_id("A10").unwrap()));
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "index SWIFT datum IDs", None,
    );
    assert_eq!(identities.lookup(&ctx, "A10").unwrap(), Some(&pmi_id("A10").unwrap()));
    assert!(identities.lookup(&ctx, "A20").unwrap().is_none());
    drop((identities, probe));
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse datum identity storage").unwrap();
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
            name: "SubAnnotation1".into(), class: extra.class.clone(), entity: extra,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let identities = crate::swift::DatumIdentities::new(&root);
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, "scan SLDPRT datum_references values", None,
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
        let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
        root.annotations.references.push(reference("A10", "GdtDatum"));
        root.annotations.entities.push(source.annotations.entities.first().unwrap().clone());
        let mut tolerance = entity("GdtPosition");
        tolerance.doubles.insert("Tolerance".into(), 0.25);
        tolerance.related = source.annotations.entities.get(1).unwrap().related.clone();
        for id in ["A20", "A21"].into_iter().take(count) {
            root.annotations.references.push(reference(id, "GdtPosition"));
            root.annotations.entities.push(tolerance.clone());
        }
        let arena = DecodeArena::new();
        let capacity = Cell::new(0);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes, "measure SWIFT tolerance output storage", |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                policy.limits.max_work_units = u64::MAX;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let projection = crate::swift::project_with_topology(&ctx, &root, None,
                    &crate::swift::RenderedDimensions::Borrowed(&[]), None, None)?;
                assert_eq!(projection.annotations.len(), count.checked_add(2).unwrap());
                assert_eq!(projection.annotations.iter().filter(|annotation| matches!(annotation.definition,
                    cadmpeg_ir::pmi::PmiDefinition::DatumSystem { .. })).count(), 1);
                capacity.set(projection.annotations.capacity());
                let measurement = ctx.copy_retained_text("x", "measure SWIFT tolerance output storage");
                drop(projection);
                measurement
            });
        let CodecError::ResourceLimit(limit) = error else { panic!("tolerance output storage measurement"); };
        (limit.used, capacity.get())
    };
    let (first, first_capacity) = retained(1);
    let (second, second_capacity) = retained(2);
    assert_eq!(first_capacity, second_capacity);
    let extra_id = pmi_id("A21").unwrap();
    let system_id = format!("{}:datum-system", pmi_id("A20").unwrap().as_str());
    let expected = u64::try_from(extra_id.as_str().len().checked_add(system_id.len()).unwrap()).unwrap();
    assert_eq!(second.checked_sub(first).unwrap(), expected,
        "the extra tolerance retains its two output identities, not a discarded reference vector");
}
