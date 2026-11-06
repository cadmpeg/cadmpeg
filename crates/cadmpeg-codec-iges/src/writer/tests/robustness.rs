// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for IGES (#234 item, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error and `max_entities` runners from `cadmpeg-test-support`;
//! the capacity rows with their helpers; the residual checker (no residual
//! positions are stated in `docs/formats/iges.md`, so the spec list is empty);
//! and the `REDUCED_FIELDS` row runner (IGES stores nothing in reduced form).

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::reduced::reduced_row_findings;
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::IgesCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "procedural_and_boolean_solids")
        .expect("commit the robustness seed fixture");
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Inherit
}

fn output_record_count(seed: &cadmpeg_ir::CadIr) -> u64 {
    let mut bytes = Vec::new();
    IgesCodec
        .plan(EncodeInput::new(seed, None), request())
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write the seed with the default limit");
    let cards = bytes
        .split(|byte| *byte == b'\n')
        .filter(|card| card.get(72) == Some(&b'D'))
        .count();
    assert_eq!(cards % 2, 0);
    u64::try_from(cards / 2).expect("valid test value")
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    let injections = standard_injections();
    let findings = injected_error_findings(&seed, &IgesCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    use cadmpeg_ir::codec::write::EncodeError;
    assert_eq!(super::super::capacity::CAPACITY_FIELDS.len(), 2);
    assert_eq!(
        super::super::capacity::CAPACITY_FIELDS[0].field,
        "card.payload"
    );
    assert_eq!(super::super::capacity::CAPACITY_FIELDS[0].limit, 72);
    assert_eq!(
        super::super::capacity::CAPACITY_FIELDS[1].field,
        "card.sequence"
    );
    assert_eq!(super::super::capacity::CAPACITY_FIELDS[1].limit, 9_999_999);
    let fields = super::super::capacity::CAPACITY_FIELDS;
    let payload = vec![b'A'; usize::try_from(fields[0].limit).expect("payload limit fits usize")];
    let sequence = u32::try_from(fields[1].limit).expect("seven-digit limit fits u32");
    let bytes = super::super::card(&payload, b'S', sequence).expect("limits write");
    let scan = crate::card::scan(&bytes).expect("written card scans");
    assert!(
        matches!(&scan.lines[0], crate::card::ScannedLine::Card { line, .. }
        if line.payload[..payload.len()] == payload)
    );
    assert!(scan
        .recoveries
        .notes()
        .iter()
        .any(|note| note.message.contains("declared 9999999")));
    let overlong = vec![b'A'; payload.len() + 1];
    assert!(matches!(super::super::capacity::card_payload(&overlong),
        Err(EncodeError::TargetCapacity { field, limit: 72, value: 73 }) if field == &fields[0]));
    assert!(
        matches!(super::super::capacity::card_sequence(sequence + 1),
        Err(EncodeError::TargetCapacity { field, limit: 9_999_999, value: 10_000_000 }) if field == &fields[1])
    );
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = output_record_count(&seed);
    let findings = max_entities_findings(&seed, &IgesCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn max_entities_counts_emitted_directory_records() {
    let seed = seed_golden();
    let records = output_record_count(&seed);
    assert!(records > 0);

    let mut input = EncodeInput::new(&seed, None);
    input.limits.max_entities = records;
    IgesCodec
        .plan(input, request())
        .expect("the exact record limit writes");
    input.limits.max_entities = records - 1;
    let error = IgesCodec
        .plan(input, request())
        .expect_err("one fewer record is refused");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("entity admission must return a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(
        (limit.limit, limit.used, limit.additional),
        (records - 1, records - 1, 1)
    );
}

#[test]
fn max_entities_counts_generated_transformations() {
    let source = super::trimmed_plane_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode the source surface");
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 5.0],
        ])
        .expect("rigid placement"),
    );
    let mut bytes = Vec::new();
    IgesCodec
        .plan(EncodeInput::new(&ir, None), request())
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write the placed surface");
    let reread = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the placed surface");
    let entities = &reread
        .ir()
        .native
        .namespace("iges")
        .expect("valid test value")
        .arenas()["entities"];
    assert!(entities
        .iter()
        .any(|record| record.field("entity_type").and_then(|value| value.as_i64()) == Some(124)));
    let records = output_record_count(&ir);
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = records;
    IgesCodec
        .plan(input, request())
        .expect("the exact record limit writes");
    input.limits.max_entities = records - 1;
    let error = IgesCodec
        .plan(input, request())
        .expect_err("one fewer record is refused");
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit)
        if (limit.limit, limit.used, limit.additional) == (records - 1, records - 1, 1))
    );
}

#[test]
fn reduced_rows_are_empty_for_iges() {
    // IGES stores no JSON path in reduced form; every written value
    // round-trips at full precision (#232 §5.2).
    assert!(super::super::REDUCED_FIELDS.is_empty());
    let seed = seed_golden();
    let encoder: &dyn Encoder = &IgesCodec;
    let redecode = |written: &[u8]| {
        let result = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .map_err(|error| error.to_string())?;
        Ok(cadmpeg_test_support::writer_closure::Redecoded {
            members: Vec::new(),
            ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
            loss_codes: result
                .report()
                .losses
                .iter()
                .map(|loss| loss.code.to_string())
                .collect(),
        })
    };
    let mut findings = Vec::new();
    for row in super::super::REDUCED_FIELDS {
        let probe = cadmpeg_test_support::cross_closure::ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: None,
        };
        findings.extend(reduced_row_findings(
            &seed,
            encoder,
            request(),
            &redecode,
            &probe,
            None,
        ));
    }
    assert!(
        findings.is_empty(),
        "reduced findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = IgesCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the residual output");
    let after = serde_json::to_value(decoded.ir()).expect("serialize the re-decode");
    let specs = residual_specs(&seed);
    let findings = residual_findings(&after, &report.losses, &specs);
    assert!(
        findings.is_empty(),
        "residual findings:\n{}",
        findings.join("\n")
    );
}

/// Residual positions the IGES writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/iges.md`; the first stated
/// rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}

#[test]
fn injected_errors_across_fixtures_name_the_record() {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let injections = standard_injections();
    let mut findings = Vec::new();
    for (name, bytes) in harness.fixture_inputs() {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let decoded = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        findings.extend(
            injected_error_findings(
                decoded.ir(),
                &IgesCodec,
                cadmpeg_test_support::cross_closure::cross_request(&IgesCodec),
                &injections,
            )
            .into_iter()
            .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn point_synthesis_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "point")
        .expect("point fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn sphere_vertex_loop_synthesis_preserves_layer_order() {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "colored_explicit_vertex_loop")
        .expect("vertex loop fixture");
    let input = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("fixture decodes");
    let plan = IgesCodec
        .plan(EncodeInput::new(input.ir(), None), request())
        .expect("fixture synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("fixture writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("fixture re-decodes");
    let kinds = |ir: &cadmpeg_ir::CadIr| {
        let layer = ir
            .model
            .presentation_layers
            .iter()
            .find(|layer| layer.name == "0")
            .expect("level zero");
        layer
            .items
            .iter()
            .map(|item| match item {
                cadmpeg_ir::presentation::PresentationItem::Surface { .. } => "surface",
                cadmpeg_ir::presentation::PresentationItem::Point { .. } => "point",
                cadmpeg_ir::presentation::PresentationItem::Face { .. } => "face",
                cadmpeg_ir::presentation::PresentationItem::Body { .. } => "body",
                _ => panic!("unexpected level member"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds(input.ir()), ["surface", "point", "face", "body"]);
    assert_eq!(kinds(output.ir()), kinds(input.ir()));
}

#[test]
fn trimmed_plane_retains_parameter_curve_name() {
    let bytes = crate::test_support::test_surface_fixtures::trimmed_plane_file();
    let input = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("trimmed plane decodes");
    let plan = IgesCodec
        .plan(EncodeInput::new(input.ir(), None), request())
        .expect("trimmed plane synthesizes");
    assert!(
        plan.report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("iges:model:curve#D5")),
        "losses: {:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("trimmed plane writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("trimmed plane re-decodes");
    assert!(output
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "name"
            && attribute.values
                == [Some(cadmpeg_ir::attributes::AttributeValue::String(
                    "PCURVE".into()
                ))]));
    let records = &output
        .ir()
        .native
        .namespace("iges")
        .expect("native namespace")
        .arenas()["entities"];
    assert!(records.iter().any(|record| record
        .field("entity_type")
        .and_then(|value| value.as_i64())
        == Some(106)
        && record.field("form").and_then(|value| value.as_i64()) == Some(63)
        && record.field("use_flag").and_then(|value| value.as_i64()) == Some(5)));
}

#[test]
fn trimmed_plane_synthesis_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "trimmed_plane")
        .expect("trimmed plane fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn trimmed_plane_with_inner_loop_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "trimmed_plane_inner_loop")
        .expect("trimmed plane with inner loop fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn bounded_associativity_synthesis_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "bounded_associativity_forms")
        .expect("associativity fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn dimension_forms_synthesis_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "dimension_forms")
        .expect("dimension forms fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn line_synthesis_preserves_records_or_names_losses() {
    use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let mut findings = Vec::new();
    let (name, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "line")
        .expect("line fixture");
    {
        let options = DecodeOptions {
            depth: IgesCodec.request(),
            ..DecodeOptions::default()
        };
        let input = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("fixture decodes");
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let output = IgesCodec
                .decode(&mut Cursor::new(written), &options)
                .map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
                loss_codes: output
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        };
        findings.extend(
            without_fidelity(&input, &IgesCodec, &redecode)
                .into_iter()
                .map(|finding| format!("{name}: {finding}")),
        );
    }
    assert!(
        findings.is_empty(),
        "fixture synthesis findings:\n{}",
        findings.join("\n")
    );
}
