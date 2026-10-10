// SPDX-License-Identifier: Apache-2.0
//! Validation and GUI binding admission at orchestration boundaries.

use std::collections::{BTreeSet, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn materialized_peak(control: impl FnOnce(&DecodeContext<'_>)) -> u64 {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "storage peak",
        None,
    );
    control(&ctx);
    let CodecError::ResourceLimit(limit) =
        ctx.reserve_scoped(u64::MAX, "storage peak").unwrap_err()
    else {
        panic!("peak probe must refuse")
    };
    drop(probe);
    limit.limit
}

#[test]
fn native_comparison_stops_at_the_first_difference() {
    for count in [1, 4096] {
        let stored = vec![1_u8; count];
        let derived = vec![2_u8; count];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One pair visit and one byte from each operand.
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(
            crate::first_difference(&ctx, &stored, &derived, "first native difference")
                .expect("first pair"),
            Some(crate::SliceDifference::Pair(0)),
        ));
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn native_comparison_does_not_charge_exact_size_exhaustion() {
    for (stored, derived, budget, different_length) in [
        (&[][..], &[][..], 0, false),
        (&[1_u8][..], &[1_u8][..], 3, false),
        (&[1_u8][..], &[1_u8, 2][..], 3, true),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let difference = crate::first_difference(&ctx, stored, derived, "native equal prefix")
            .expect("only actual pairs are charged");
        if different_length {
            assert!(matches!(difference, Some(crate::SliceDifference::Length)));
        } else {
            assert!(difference.is_none());
        }
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn empty_orchestration_paths_preserve_a_fused_refusal() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut entries = Vec::new();
    let gui = crate::gui::Graph::default();
    let affected = BTreeSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(crate::validate_native(&ctx, &ir)
        .expect("no native namespace")
        .is_empty());
    assert!(
        crate::first_difference(&ctx, &[] as &[u8], &[], "empty comparison")
            .expect("empty comparison")
            .is_none()
    );
    crate::bind_gui_entry_references(&ctx, &mut entries, &gui).expect("empty binding");
    assert!(crate::semantic_losses(&ctx, &ir, &affected, Vec::new())
        .expect("no semantic sources")
        .is_empty());
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(1, "prior orchestration refusal")
        .expect_err("work limit")
    else {
        panic!("resource refusal")
    };
    for result in [
        crate::validate_native(&ctx, &ir).map(|_| ()),
        crate::first_difference(&ctx, &[] as &[u8], &[], "empty comparison").map(|_| ()),
        crate::bind_gui_entry_references(&ctx, &mut entries, &gui),
        crate::semantic_losses(&ctx, &ir, &affected, Vec::new()).map(|_| ()),
    ] {
        assert!(matches!(result, Err(CodecError::ResourceLimit(repeated)) if repeated == original));
    }
}

fn gui_property(side_entries: Vec<String>) -> crate::native::GuiPropertyRecord {
    crate::native::GuiPropertyRecord {
        id: "fcstd:gui:property#Owner:Data".into(),
        owner: "fcstd:gui:view-provider#Owner".into(),
        name: "Data".into(),
        type_name: "App::PropertyFileIncluded".into(),
        status: None,
        order: 0,
        values: Vec::new(),
        side_entries,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    }
}

#[test]
fn gui_binding_without_side_references_skips_the_entry_index() {
    let mut entries: Vec<_> = (0..128)
        .map(|index| {
            crate::test_support::entry_record(
                format!("fcstd:native:entry#Data{index}.bin"),
                format!("Data{index}.bin"),
                cadmpeg_core::container::ContainerRole::Auxiliary,
                Vec::new(),
                Vec::new(),
            )
        })
        .collect();
    let gui = crate::gui::Graph {
        properties: vec![gui_property(Vec::new())],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::bind_gui_entry_references(&ctx, &mut entries, &gui).expect("no entry-index consumer");
    assert!(entries.iter().all(|entry| entry.referenced_by().is_empty()));
    assert_eq!(ctx.resource_refusal(), None);

    let gui = crate::gui::Graph {
        properties: vec![gui_property(vec![entries[0].name().into()])],
        ..Default::default()
    };
    crate::test_support::assert_collection_refusal_at(&[], "FCStd GUI entry references", |ctx| {
        let mut entries = entries.clone();
        crate::bind_gui_entry_references(ctx, &mut entries, &gui)
    });
    crate::test_support::with_service_context(&[], |ctx| {
        crate::bind_gui_entry_references(ctx, &mut entries, &gui)
            .expect("real side-reference consumer");
    });
    assert_eq!(entries[0].referenced_by(), [gui.properties[0].id.as_str()]);
    assert!(entries[1..]
        .iter()
        .all(|entry| entry.referenced_by().is_empty()));
}

#[test]
fn empty_logical_ledger_skips_entry_and_owner_indexes() {
    let entries = [crate::test_support::entry_record(
        "fcstd:native:entry#Empty.bin".into(),
        "Empty.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new(),
    )];
    let properties: Vec<_> = (0..128).map(|_| gui_property(Vec::new())).collect();
    let owners = crate::LedgerOwners {
        entries: &entries,
        gui_properties: &properties,
        gui_documents: &[],
        shape_payloads: &[],
        string_tables: &[],
        element_maps: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_logical_ledger(&ctx, &[], &owners, &HashSet::new(), &mut findings)
        .expect("no ledger index consumer");
    assert!(findings.is_empty());
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(1, "prior ledger refusal")
        .expect_err("work limit")
    else {
        panic!("resource refusal")
    };
    assert!(matches!(crate::validate_logical_ledger(
        &ctx, &[], &owners, &HashSet::new(), &mut findings,
    ), Err(CodecError::ResourceLimit(repeated)) if repeated == original));
}

#[test]
fn logical_ledger_releases_a_consumed_group_before_the_larger_sort() {
    let counts = [128_usize, 1024];
    let entries: Vec<_> = ["A.bin", "B.bin"]
        .into_iter()
        .zip(counts)
        .map(|(name, count)| {
            crate::test_support::entry_record(
                format!("fcstd:native:entry#{name}"),
                name.into(),
                cadmpeg_core::container::ContainerRole::Auxiliary,
                Vec::new(),
                vec![0; count],
            )
        })
        .collect();
    let spans: Vec<_> = entries
        .iter()
        .zip(counts)
        .flat_map(|(entry, count)| {
            (0..count)
                .rev()
                .map(move |index| crate::native::LogicalSpan {
                    id: format!("fcstd:native:logical#{}:{index}", entry.name()),
                    entry: entry.name().into(),
                    span: crate::native::ByteSpan::try_new(
                        cadmpeg_core::decode::u64_from_index(index),
                        cadmpeg_core::decode::u64_from_index(index + 1),
                    )
                    .expect("nonempty interval"),
                    classification: crate::native::LogicalClassification::Structural,
                })
        })
        .collect();
    let control_entries = [
        crate::test_support::entry_record(
            "fcstd:native:entry#A.bin".into(),
            "A.bin".into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            Vec::new(),
            vec![0],
        ),
        entries[1].clone(),
    ];
    let mut control_spans: Vec<_> = std::iter::once(spans[0].clone())
        .chain(spans[counts[0]..].iter().cloned())
        .collect();
    control_spans[0].span = crate::native::ByteSpan::try_new(0, 1).unwrap();
    let cap = materialized_peak(|ctx| {
        let control_owners = crate::LedgerOwners {
            entries: &control_entries,
            gui_properties: &[],
            gui_documents: &[],
            shape_payloads: &[],
            string_tables: &[],
            element_maps: &[],
        };
        let mut findings = Vec::new();
        crate::validate_logical_ledger(
            ctx,
            &control_spans,
            &control_owners,
            &HashSet::new(),
            &mut findings,
        )
        .unwrap();
        assert!(findings.is_empty());
    });
    let owners = crate::LedgerOwners {
        entries: &entries,
        gui_properties: &[],
        gui_documents: &[],
        shape_payloads: &[],
        string_tables: &[],
        element_maps: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_logical_ledger(&ctx, &spans, &owners, &HashSet::new(), &mut findings)
        .expect("the consumed group no longer contributes to the later sort peak");
    assert!(findings.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn structural_ledger_and_property_owners_skip_unused_owner_indexes() {
    use crate::native::{ByteSpan, LogicalClassification, LogicalSpan, StringTableRecord};
    let entries = [crate::test_support::entry_record(
        "fcstd:native:entry#A.bin".into(),
        "A.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![0],
    )];
    let properties: Vec<_> = (0..128).map(|_| gui_property(Vec::new())).collect();
    let tables: Vec<_> = (0..128)
        .map(|index| {
            StringTableRecord::try_new(index, None, false, 0, None, Vec::new()).expect("table")
        })
        .collect();
    let owners = crate::LedgerOwners {
        entries: &entries,
        gui_properties: &properties,
        gui_documents: &[],
        shape_payloads: &[],
        string_tables: &tables,
        element_maps: &[],
    };
    let empty_owners = crate::LedgerOwners {
        entries: &entries,
        gui_properties: &[],
        gui_documents: &[],
        shape_payloads: &[],
        string_tables: &[],
        element_maps: &[],
    };
    let property_ids = HashSet::from(["fcstd:native:property#A:Shape"]);
    for classification in [
        LogicalClassification::Structural,
        LogicalClassification::Typed {
            owner: "fcstd:native:property#A:Shape".into(),
        },
    ] {
        let logical = [LogicalSpan {
            id: "fcstd:native:logical#A.bin:0".into(),
            entry: "A.bin".into(),
            span: ByteSpan::try_new(0, 1).expect("span"),
            classification,
        }];
        let budget = crate::test_support::with_service_context(&[], |ctx| {
            crate::validate_logical_ledger(
                ctx,
                &logical,
                &empty_owners,
                &property_ids,
                &mut Vec::new(),
            )
            .expect("short owner oracle");
            let CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure ledger work")
                .expect_err("work overflow")
            else {
                panic!("work refusal")
            };
            limit.used
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        crate::validate_logical_ledger(&ctx, &logical, &owners, &property_ids, &mut findings)
            .expect("no owner-index consumer");
        assert!(findings.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    }
}

fn mapped_name_record(
    string_ids: Vec<i64>,
    topology_ids: Vec<String>,
) -> crate::native::element_map::ElementMapRecord {
    use crate::native::element_map::{ElementMapNodes, ElementMapRecord, ElementMappedName};
    let groups = std::collections::BTreeMap::from([(
        "Vertex".into(),
        vec![vec![ElementMappedName {
            encoded: "Vertex1".into(),
            resolved: None,
            string_ids,
            topology_ids,
        }]],
    )]);
    let maps = crate::test_support::with_service_context(&[], |ctx| {
        let groups = ctx
            .collect_scoped_btree_map(groups, "validation fixture group tree")
            .expect("fixture group tree");
        ElementMapNodes::from_root_names(
            ctx,
            0,
            crate::native::element_map::ScopedData {
                data: groups.0,
                _storage: groups.1,
            },
        )
        .expect("root map")
    });
    ElementMapRecord {
        id: "fcstd:native:element-map#A:Shape".into(),
        property: "fcstd:native:property#A:Shape".into(),
        version: "1".into(),
        hasher_index: Some(0),
        source_entry: None,
        map_id: 0,
        declared_count: 1,
        postfixes: Vec::new(),
        maps,
    }
}

fn validation_string_table(index: usize) -> crate::native::StringTableRecord {
    crate::native::StringTableRecord::try_new(
        index,
        None,
        false,
        0,
        None,
        vec![crate::native::StringTableEntry {
            string_id: 1,
            flags: 0,
            components: Vec::new(),
            payload: String::new(),
            raw: String::new(),
        }],
    )
    .expect("table")
}

#[test]
fn element_maps_without_identity_references_skip_both_indexes() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    for index in 0..128 {
        ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
            id: cadmpeg_ir::ids::VertexId::mint(format!("fcstd:model:vertex#A:{index}"))
                .expect("vertex id"),
            point: cadmpeg_ir::ids::PointId::mint(format!("fcstd:model:point#A:{index}"))
                .expect("point id"),
            tolerance: None,
        });
    }
    let tables: Vec<_> = (0..128).map(validation_string_table).collect();
    let maps = [mapped_name_record(Vec::new(), Vec::new())];
    let property_ids = HashSet::from([maps[0].property.as_str()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units =
        4 + 2 * cadmpeg_core::decode::u64_from_index(maps[0].property.len());
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_element_maps(
        &ctx,
        &ir,
        &maps,
        &tables,
        &property_ids,
        &HashSet::new(),
        &mut findings,
    )
    .expect("no identity-index consumers");
    assert!(findings.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(u64::MAX, "prior element-map refusal")
        .expect_err("work overflow")
    else {
        panic!("work refusal")
    };
    assert!(matches!(crate::validate_element_maps(
        &ctx, &ir, &[], &[], &property_ids, &HashSet::new(), &mut findings,
    ), Err(CodecError::ResourceLimit(repeated)) if repeated == original));
}

#[test]
fn element_maps_index_only_queried_tables_and_validate_actual_references() {
    let ir = cadmpeg_ir::CadIr::empty();
    let maps = [mapped_name_record(vec![1], Vec::new())];
    let property_ids = HashSet::from([maps[0].property.as_str()]);
    let tables: Vec<_> = (0..128).map(validation_string_table).collect();
    let budget = crate::test_support::with_service_context(&[], |ctx| {
        crate::validate_element_maps(
            ctx,
            &ir,
            &maps,
            &tables[..1],
            &property_ids,
            &HashSet::new(),
            &mut Vec::new(),
        )
        .expect("one queried table");
        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(u64::MAX, "measure map work")
            .expect_err("work overflow")
        else {
            panic!("work refusal")
        };
        limit.used
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = budget;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_element_maps(
        &ctx,
        &ir,
        &maps,
        &tables,
        &property_ids,
        &HashSet::new(),
        &mut findings,
    )
    .expect("only the queried table is indexed");
    assert!(findings.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    crate::test_support::assert_collection_refusal_at(
        &[],
        "FreeCAD validation element maps",
        |ctx| {
            crate::validate_element_maps(
                ctx,
                &ir,
                &maps,
                &tables,
                &property_ids,
                &HashSet::new(),
                &mut Vec::new(),
            )
        },
    );
    let missing = [mapped_name_record(
        vec![2],
        vec!["fcstd:model:vertex#Absent".into()],
    )];
    let findings = crate::test_support::with_service_context(&[], |ctx| {
        let mut findings = Vec::new();
        crate::validate_element_maps(
            ctx,
            &ir,
            &missing,
            &tables,
            &property_ids,
            &HashSet::new(),
            &mut findings,
        )
        .expect("missing references produce findings");
        findings
    });
    assert_eq!(findings.len(), 2);
    assert!(findings[0].message.contains("missing persistent string id"));
    assert!(findings[1].message.contains("missing neutral topology"));
}

#[test]
fn logical_ledger_validates_gui_and_string_table_owners_on_demand() {
    use crate::native::{ByteSpan, LogicalClassification, LogicalSpan};
    let entries = [crate::test_support::entry_record(
        "fcstd:native:entry#A.bin".into(),
        "A.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![0],
    )];
    let properties = [gui_property(Vec::new())];
    let tables = [validation_string_table(0)];
    let owners = crate::LedgerOwners {
        entries: &entries,
        gui_properties: &properties,
        gui_documents: &[],
        shape_payloads: &[],
        string_tables: &tables,
        element_maps: &[],
    };
    for owner in [properties[0].id.clone(), tables[0].id()] {
        let logical = [LogicalSpan {
            id: "fcstd:native:logical#A.bin:0".into(),
            entry: "A.bin".into(),
            span: ByteSpan::try_new(0, 1).expect("span"),
            classification: LogicalClassification::Typed { owner },
        }];
        crate::test_support::with_service_context(&[], |ctx| {
            let mut findings = Vec::new();
            crate::validate_logical_ledger(ctx, &logical, &owners, &HashSet::new(), &mut findings)
                .expect("actual owner lookup");
            assert!(findings.is_empty());
        });
        crate::test_support::assert_collection_refusal_at(
            &[],
            "FreeCAD validation logical ledger",
            |ctx| {
                crate::validate_logical_ledger(
                    ctx,
                    &logical,
                    &owners,
                    &HashSet::new(),
                    &mut Vec::new(),
                )
            },
        );
    }
}

#[test]
fn decode_native_populations_use_scoped_storage_and_keep_serialized_output() {
    use cadmpeg_ir::codec::CodecBackend;
    let value = "x".repeat(16 * 1024);
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="P" type="App::PropertyString"><String value="{value}"/></Property></Properties></Object></ObjectData></Document>"#
    );
    let bytes = crate::test_support::test_archive::archive(&document);
    let cap = materialized_peak(|ctx| {
        crate::FcstdCodec
            .decode_impl(ctx, cadmpeg_core::decode::View::over_retained(&bytes))
            .unwrap();
    });
    for below in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap - u64::from(below);
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let result = crate::FcstdCodec.decode_impl(&ctx, root);
        if below {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        } else {
            let decoded = result.unwrap();
            let properties: Vec<crate::native::PropertyRecord> = decoded
                .ir
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as("properties")
                .unwrap();
            assert_eq!(properties[0].values()[0].attributes["value"], value);
        }
    }
    crate::test_support::with_service_context(&bytes, |ctx| {
        let decoded = crate::FcstdCodec
            .decode_impl(ctx, cadmpeg_core::decode::View::over_retained(&bytes))
            .unwrap();
        let namespace = decoded.ir.native.namespace("fcstd").unwrap();
        let properties: Vec<crate::native::PropertyRecord> =
            namespace.arena_as("properties").unwrap();
        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].name, "P");
        assert_eq!(properties[0].values()[0].attributes["value"], value);
        let entries: Vec<crate::native::EntryRecord> = namespace.arena_as("entries").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name(), "Document.xml");
        assert_eq!(entries[0].data(), document.as_bytes());
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(u64::MAX, "returned decode scratch")
            .unwrap_err()
        else {
            panic!("scratch query must refuse")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        );
        assert_eq!(limit.used, 0);
        // Retiring typed scratch leaves the serialized output readable.
        let reread: Vec<crate::native::PropertyRecord> = namespace.arena_as("properties").unwrap();
        assert_eq!(reread[0].values()[0].attributes["value"], value);
    });
}

#[test]
fn medium_detection_does_not_charge_unvisited_marker_suffixes() {
    // The fallback visits five windows before the marker at offset four.
    assert_detection_suffix_work(
        b"PK\x03\x04Document.xml".to_vec(),
        cadmpeg_ir::codec::Confidence::Medium,
        5,
    );
}

#[test]
fn high_detection_does_not_charge_unvisited_marker_suffixes() {
    // XML markers start at offsets zero and ten: one plus eleven visits.
    assert_detection_suffix_work(
        crate::test_support::test_archive::archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>",
        ),
        cadmpeg_ir::codec::Confidence::High,
        12,
    );
}

fn assert_detection_suffix_work(
    prefix: Vec<u8>,
    expected: cadmpeg_ir::codec::Confidence,
    work: u64,
) {
    use cadmpeg_ir::codec::Codec;
    for suffix in [0, 64 * 1024] {
        let mut bytes = prefix.clone();
        bytes.resize(bytes.len() + suffix, 0);
        for below in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work - u64::from(below);
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let result = crate::FcstdCodec.detect(&ctx, root);
            if below {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
            } else {
                assert_eq!(result.unwrap(), expected);
                assert_eq!(ctx.resource_refusal(), None);
            }
        }
    }
}
