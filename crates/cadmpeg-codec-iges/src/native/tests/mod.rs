// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::DecodeReport;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

mod annotations;
mod counted_lists;
mod fem;
mod macros;

fn code_count(report: &DecodeReport, code: IgesLossCode) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

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
        .map(|loss| loss.code.code.clone())
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
                .filter(|code| **code == overdeclared.code)
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
            assert_eq!(rejection.loss().code.as_str(), overdeclared.as_str());
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

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().source.as_ref().unwrap().format(), "iges");
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["document_local_sha256"],
        crate::document_digest(result.ir())
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
    assert_eq!(native.version, 6);
    assert_eq!(native.arenas["cards"].len(), 7);
    assert_eq!(native.arenas["entities"].len(), 1);
    assert!(native.arenas["colors"].is_empty());
    assert_eq!(native.arenas["display_attributes"].len(), 1);
    assert!(!native.arenas.contains_key("opaque_bytes"));
    assert_eq!(native.arenas["entities"][0].id(), "iges:entity:directory#1");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 1.0);
    assert_eq!(result.ir().model.points[0].position.y, 2.0);
    assert_eq!(result.ir().model.points[0].position.z, 3.0);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.report().geometry_transferred());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
