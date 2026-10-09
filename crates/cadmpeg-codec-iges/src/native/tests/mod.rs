// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::decode::DecodeReport;

use crate::loss::IgesLossCode;
use crate::test_support::code_count;
use crate::test_support::test_curves_and_surfaces::point_file;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

#[test]
fn native_token_copy_refuses_outer_and_nested_allocations() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let tokens = [crate::parameter::Token {
        value: crate::parameter::TokenValue::String(b"abc".to_vec()),
        span: 0..3,
    }];
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "iges native token slots",
        ),
        (ResourceDimension::RetainedBytes, "iges native token bytes"),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                _ => unreachable!("test dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::copy_native_tokens(&ctx, &tokens)
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(super::copy_native_tokens(&ctx, &tokens).unwrap(), tokens);
}

#[test]
fn native_entity_links_refuse_slots_and_text_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "iges native test link scan",
        ),
        (ResourceDimension::CollectionItems, "iges native test links"),
        (
            ResourceDimension::RetainedBytes,
            "iges native linked entity id",
        ),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                _ => unreachable!("test dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::native_entity_ids(
                &ctx,
                [3].into_iter(),
                "iges native test link scan",
                "iges native test links",
                Some,
            )
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::native_entity_ids(
            &ctx,
            [3].into_iter(),
            "iges native test link scan",
            "iges native test links",
            Some,
        )
        .unwrap(),
        ["iges:entity:directory#3"]
    );
}

#[test]
fn native_entity_id_builder_steps_sparse_and_unfiltered_sources() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let id_bytes = u64::try_from("iges:entity:directory#2".len()).unwrap();
    let sparse_work = 3 + 2 * id_bytes;
    let arena = DecodeArena::new();
    let mut sparse_policy = DecodePolicy::service();
    sparse_policy.limits.max_work_units = sparse_work - 1;
    let (sparse_ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &sparse_policy).unwrap();
    let error = super::native_entity_ids(
        &sparse_ctx,
        [1, 2, 3].into_iter(),
        "iges native sparse link scan",
        "iges native sparse link slots",
        |sequence| (sequence == 2).then_some(sequence),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "iges native sparse link scan"
            && limit.used == sparse_policy.limits.max_work_units
            && limit.additional == 1));

    let arena = DecodeArena::new();
    let mut sparse_exact_policy = DecodePolicy::service();
    sparse_exact_policy.limits.max_work_units = sparse_work;
    let (sparse_exact_ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &sparse_exact_policy).unwrap();
    assert_eq!(
        super::native_entity_ids(
            &sparse_exact_ctx,
            [1, 2, 3].into_iter(),
            "iges native sparse link scan",
            "iges native sparse link slots",
            |sequence| (sequence == 2).then_some(sequence),
        )
        .unwrap(),
        ["iges:entity:directory#2"]
    );

    let full_work = 3 + 6 * id_bytes;
    let arena = DecodeArena::new();
    let mut full_policy = DecodePolicy::service();
    full_policy.limits.max_work_units = full_work - 1;
    let (full_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &full_policy).unwrap();
    let error = super::native_entity_ids(
        &full_ctx,
        [1, 2, 3].into_iter(),
        "iges native full link scan",
        "iges native full link slots",
        Some,
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "iges native linked entity id"
            && limit.used == full_policy.limits.max_work_units
            && limit.additional == 1));

    let arena = DecodeArena::new();
    let mut full_exact_policy = DecodePolicy::service();
    full_exact_policy.limits.max_work_units = full_work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &full_exact_policy).unwrap();
    assert_eq!(
        super::native_entity_ids(
            &ctx,
            [1, 2, 3].into_iter(),
            "iges native full link scan",
            "iges native full link slots",
            Some,
        )
        .unwrap(),
        [
            "iges:entity:directory#1",
            "iges:entity:directory#2",
            "iges:entity:directory#3",
        ]
    );
}

#[test]
fn native_parameter_record_refuses_bytes_tokens_and_comment() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let record = crate::parameter::ParameterRecord::from_test_tokens(
        1,
        1..2,
        b"abc".to_vec(),
        1,
        vec![crate::parameter::Token {
            value: crate::parameter::TokenValue::String(b"d".to_vec()),
            span: 0..1,
        }],
        b"e".to_vec(),
    );
    for (dimension, operation) in [
        (
            ResourceDimension::RetainedBytes,
            "iges native parameter bytes",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges native token slots",
        ),
        (ResourceDimension::RetainedBytes, "iges native token bytes"),
        (
            ResourceDimension::RetainedBytes,
            "iges native parameter comment",
        ),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                _ => unreachable!("test dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::copy_native_parameter_record(&ctx, &record)
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let copy = super::copy_native_parameter_record(&ctx, &record).unwrap();
    assert_eq!(copy.bytes, record.bytes);
    assert_eq!(copy.parameters, record.tokens());
    assert_eq!(copy.comment, record.comment);
}

#[test]
fn native_ambiguity_and_entity_slots_refuse_after_input_indexes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = point_file();
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined_directory) =
        crate::directory::parse(&scan, global.global_table(), &parse_ctx)
            .unwrap();
    let assembly = crate::parameter::assemble_with_context(
        &scan,
        &directory,
        &quarantined_directory,
        &global,
        &parse_ctx,
    )
    .unwrap();
    assert_eq!(directory.len(), 1);
    assert_eq!(directory[0].sequence, 1);
    assert_eq!(assembly.records.len(), 1);
    assert!(quarantined_directory.is_empty());
    assert!(assembly.quarantined.is_empty());

    for (ambiguous, operation) in [
        (false, "iges native entity slots"),
        (true, "iges native ambiguous boundary slots"),
    ] {
        let mut analysis = assembly.trailing_pointer_analysis.clone();
        if ambiguous {
            analysis.insert(
                1,
                crate::parameter::TrailingPointerAnalysis::Ambiguous {
                    candidates: 2,
                    valid: 2,
                },
            );
        }
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::store(
                    &mut cadmpeg_ir::CadIr::empty(),
                    super::NativeStoreInputs {
                        scan: &scan,
                        directory: &directory,
                        parameters: &assembly.records,
                        trailing_pointer_analysis: &analysis,
                        quarantine: super::QuarantinedRecords {
                            directory: &quarantined_directory,
                            parameters: &assembly.quarantined,
                        },
                        structure_admitted: None,
                        sequences: &crate::entities::geometry::SourceSequences::default(),
                        boundary_vertex_derivations: &[],
                    },
                    &mut std::collections::BTreeMap::new(),
                    &global,
                    super::ProductOccurrenceLimits::new(100_000, 64),
                    &ctx,
                )
                .map(|_| ())
            },
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems && limit.additional == 1
        ));
    }
}

#[test]
fn native_required_back_pointer_member_refuses_node_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 1,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        },
    ]);
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined_directory) =
        crate::directory::parse(&scan, global.global_table(), &parse_ctx)
            .unwrap();
    let assembly = crate::parameter::assemble_with_context(
        &scan,
        &directory,
        &quarantined_directory,
        &global,
        &parse_ctx,
    )
    .unwrap();
    assert_eq!(directory.len(), 2);
    assert_eq!(assembly.records.len(), 2);
    assert!(quarantined_directory.is_empty());
    assert!(assembly.quarantined.is_empty());

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges native required back-pointer member",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::store(
                &mut cadmpeg_ir::CadIr::empty(),
                super::NativeStoreInputs {
                    scan: &scan,
                    directory: &directory,
                    parameters: &assembly.records,
                    trailing_pointer_analysis: &assembly.trailing_pointer_analysis,
                    quarantine: super::QuarantinedRecords {
                        directory: &quarantined_directory,
                        parameters: &assembly.quarantined,
                    },
                    structure_admitted: None,
                    sequences: &crate::entities::geometry::SourceSequences::default(),
                    boundary_vertex_derivations: &[],
                },
                &mut std::collections::BTreeMap::new(),
                &global,
                super::ProductOccurrenceLimits::new(100_000, 64),
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems && limit.additional == 1
    ));
}

#[test]
fn native_input_cards_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = point_file();
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined_directory) =
        crate::directory::parse(&scan, global.global_table(), &parse_ctx)
            .unwrap();
    let assembly = crate::parameter::assemble_with_context(
        &scan,
        &directory,
        &quarantined_directory,
        &global,
        &parse_ctx,
    )
    .unwrap();
    assert!(quarantined_directory.is_empty());
    assert!(assembly.quarantined.is_empty());
    assert_eq!(assembly.records.len(), 1);
    let quarantine = || super::QuarantinedRecords {
        directory: &quarantined_directory,
        parameters: &assembly.quarantined,
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges native card slots",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::collect_native_inputs(&scan, quarantine(), &ctx).map(|_| ())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges native card slots"));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let indexes = super::collect_native_inputs(&scan, quarantine(), &ctx).unwrap();
    assert_eq!(
        indexes.cards.len(),
        (scan.cards().len() + scan.trailing().len())
    );
}

#[test]
fn native_quarantine_indexes_refuse_each_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: "116,1,2,3x4,0;".into(),
    }]);
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, parsed_quarantine) =
        crate::directory::parse(&scan, global.global_table(), &parse_ctx)
            .unwrap();
    let assembly = crate::parameter::assemble_with_context(
        &scan,
        &directory,
        &parsed_quarantine,
        &global,
        &parse_ctx,
    )
    .unwrap();
    assert_eq!(assembly.quarantined.len(), 1);
    let quarantined_directory = [crate::directory::QuarantinedDirectoryRecord {
        sequence: 3,
        source_offset: 0,
        bytes: Vec::new(),
        defect: crate::directory::DirectoryDefect::UnpairedCard,
    }];
    let quarantine = || super::QuarantinedRecords {
        directory: &quarantined_directory,
        parameters: &assembly.quarantined,
    };
    for operation in [
        "iges native quarantined directory slots",
        "iges native quarantined parameter slots",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::collect_native_inputs(&scan, quarantine(), &ctx).map(|_| ())
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let indexes = super::collect_native_inputs(&scan, quarantine(), &ctx).unwrap();
    assert_eq!(indexes.quarantined_directory_records.len(), 1);
    assert_eq!(indexes.quarantined_parameter_records.len(), 1);
}

mod allocation_limits;
mod annotations;
mod budget;
mod counted_lists;
mod fem;
mod macros;
mod occurrences;
mod serialization_limits;
mod visited_work;

fn codes_charged_to(report: &DecodeReport, sequence: u32) -> Vec<String> {
    let tag = format!("directory_entry:D{sequence}");
    report
        .losses
        .iter()
        .filter(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|source| source.tag.as_deref())
                == Some(&tag)
        })
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

/// Run the overdeclared-count contract for one defective Directory Entry: the
/// loss is charged once in both decode modes, it is the entry's only loss when
/// no projection runs, a full decode also refuses that entry's projection, and
/// a strict decode refuses the document on the count.
fn assert_overdeclared_contract(bytes: &[u8], sequence: u32) {
    let overdeclared = IgesLossCode::ParameterCountOverdeclared.kind();
    for container_only in [false, true] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.to_vec()),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let report = result.report();
        assert_eq!(
            code_count(report, IgesLossCode::ParameterCountOverdeclared),
            1,
            "container_only {container_only}"
        );
        let charged = codes_charged_to(report, sequence);
        assert_eq!(
            charged
                .iter()
                .filter(|code| **code == overdeclared.local_code())
                .count(),
            1,
            "D{sequence} must carry the count loss once, got {charged:?}"
        );
        let refused = charged.iter().any(|code| {
            matches!(
                code.as_str(),
                "entity.not-projected"
                    | "entity.retained-unprojected"
                    | "entity.outside-envelope"
                    | "presentation.display-data-not-projected"
            )
        });
        assert_eq!(
            refused, !container_only,
            "D{sequence} projection refusal, got {charged:?}"
        );
    }

    let mut strict = DecodeOptions::default();
    strict.policy.mode = DecodeMode::Strict;
    match IgesCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &strict)
        .unwrap_err()
    {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            assert_eq!(rejection.loss().code.to_string(), overdeclared.to_string());
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn every_admitted_entity_form_routes_to_a_typed_decoder_or_native_retention_loss() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let entities = matrix["entity"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entity| {
            let entity_type = entity["type"].as_integer().unwrap();
            let mut forms = entity["forms"].as_array().map_or_else(
                || vec![5001, 9999],
                |forms| {
                    forms
                        .iter()
                        .map(|form| form.as_integer().unwrap())
                        .collect()
                },
            );
            if entity
                .get("implementor_defined")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
            {
                forms.extend([5001, 9999]);
            }
            forms.into_iter().map(move |form| OwnedTestEntity {
                entity_type,
                form,
                label: format!("E{entity_type}"),
                status: "00000000",
                parameters: format!("{entity_type};"),
            })
        })
        .collect::<Vec<_>>();
    let bytes = owned_test_file(&entities);

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let generic_fallthroughs = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .ends_with("retained without neutral projection")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let mut expected = vec![
        "IGES entity type 124 form 0 retained without neutral projection".to_owned(),
        "IGES entity type 124 form 1 retained without neutral projection".to_owned(),
        "IGES entity type 124 form 10 retained without neutral projection".to_owned(),
        "IGES entity type 124 form 11 retained without neutral projection".to_owned(),
        "IGES entity type 124 form 12 retained without neutral projection".to_owned(),
    ];
    expected.extend([134, 136, 138].into_iter().map(|entity_type| {
        format!("IGES entity type {entity_type} form 0 retained without neutral projection")
    }));
    for entity_type in [146, 148] {
        expected.extend((0..=34).map(|form| {
            format!(
                "IGES entity type {entity_type} form {form} retained without neutral projection"
            )
        }));
    }
    expected.push("IGES entity type 418 form 0 retained without neutral projection".to_owned());
    expected.extend([
        "IGES entity type 406 form 5001 retained without neutral projection".to_owned(),
        "IGES entity type 406 form 9999 retained without neutral projection".to_owned(),
    ]);
    assert_eq!(
        generic_fallthroughs,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn decode_preserves_native_entities_and_graph() {
    let bytes = point_file();

    let result = EditableDecodeResult::from(
        IgesCodec
            .decode(
                &mut Cursor::new(bytes.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap(),
    );

    assert_eq!(result.ir().source.as_ref().unwrap().format(), "iges");
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["document_local_sha256"],
        crate::document_digest(result.ir()).unwrap()
    );
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap()
            .data(),
        Some(bytes.as_slice())
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["cards"].len(), 7);
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert!(native.arenas()["colors"].is_empty());
    assert_eq!(native.arenas()["display_attributes"].len(), 1);
    assert!(!native.arenas().contains_key("opaque_bytes"));
    assert_eq!(
        native.arenas()["entities"][0].id(),
        "iges:entity:directory#1"
    );
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position().get().x, 1.0);
    assert_eq!(result.ir().model.points[0].position().get().y, 2.0);
    assert_eq!(result.ir().model.points[0].position().get().z, 3.0);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.report().geometry_transferred());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn absent_native_parameter_record_keeps_empty_wire_fields() {
    #[derive(serde::Serialize)]
    struct Record {
        #[serde(flatten)]
        parameters: super::NativeParameterRecordSlot,
    }
    assert_eq!(
        serde_json::to_value(Record {
            parameters: super::NativeParameterRecordSlot(None)
        })
        .unwrap(),
        serde_json::json!({
            "parameter_line_start": null,
            "parameter_line_end": null,
            "parameter_bytes": [],
            "parameters": [],
            "comment": [],
        })
    );
}

mod source_visits;
