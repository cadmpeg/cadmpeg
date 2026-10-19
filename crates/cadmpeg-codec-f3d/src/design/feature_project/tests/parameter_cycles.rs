// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use std::io::{Cursor, Write};

use crate::loss::F3dLossCode;
use crate::test_support::lp_utf16;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::F3dCodec;

fn decode_parameters(records: &[(u32, &str, &str)]) -> DecodeResult {
    let mut bulk = Vec::new();
    for (ordinal, name, expression) in records {
        let start = bulk.len();
        // Discriminated document parameter, as specified in F3D section 3.1.
        bulk.extend_from_slice(&3_u32.to_le_bytes());
        bulk.extend_from_slice(b"305");
        bulk.extend_from_slice(&(100 + ordinal).to_le_bytes());
        bulk.extend_from_slice(&[0; 11]);
        bulk.extend_from_slice(&6_u64.to_le_bytes());
        bulk.push(0);
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        bulk.push(0);
        lp_utf16(&mut bulk, expression);
        bulk.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1]);
        lp_utf16(&mut bulk, "User Parameter");
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        lp_utf16(&mut bulk, "mm");
        lp_utf16(&mut bulk, name);
        bulk.extend_from_slice(&0.1_f64.to_le_bytes());
        bulk.extend_from_slice(&[0, 1, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(
            crate::design::decode::parameters::parse_design_parameter_record(&bulk[start..])
                .is_some()
        );
    }
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), records.len());
    decoded
}

fn dependencies(decoded: &DecodeResult) -> std::collections::BTreeMap<&str, Vec<&str>> {
    let parameters = &decoded.ir().model.parameters;
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_str(),
                parameter
                    .expression
                    .dependencies()
                    .map(|id| {
                        parameters
                            .iter()
                            .find(|candidate| &candidate.id == id)
                            .unwrap()
                            .name
                            .as_str()
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn parameter_cycle_preserves_an_acyclic_dependency_into_the_cycle() {
    let records = [(0, "C", "A"), (1, "A", "B"), (2, "B", "A")];
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let decoded = decode_parameters(&order.map(|index| records[index]));
        assert_eq!(
            dependencies(&decoded),
            std::collections::BTreeMap::from([
                ("A", vec!["B"]),
                ("B", vec!["A"]),
                ("C", vec!["A"])
            ])
        );
        let parameters = &decoded.ir().model.parameters;
        let ordinal = |name| parameters.iter().find(|p| p.name == name).unwrap().ordinal;
        assert!(ordinal("A") < ordinal("C"));
        assert!(ordinal("A") < ordinal("B"));
        assert!(decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::ParameterExpressionUnbound.kind()));
        let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).unwrap();
        assert!(validation.findings.iter().any(|finding| finding.severity
            == cadmpeg_ir::report::Severity::Warning
            && finding.message.contains("cycle")));
        let native = crate::test_support::native_test::f3d_native(decoded.ir());
        assert_eq!(native.design_parameters.len(), 3);
        assert!(native
            .design_parameters
            .iter()
            .any(|parameter| parameter.name() == "C" && parameter.expression() == "A"));
    }
}

#[test]
fn parameter_cycle_preserves_dependencies_between_distinct_cycles() {
    let decoded = decode_parameters(&[
        (0, "A", "B + D"),
        (1, "B", "A"),
        (2, "C", "D"),
        (3, "D", "C"),
    ]);
    assert_eq!(
        dependencies(&decoded),
        std::collections::BTreeMap::from([
            ("A", vec!["B", "D"]),
            ("B", vec!["A"]),
            ("C", vec!["D"]),
            ("D", vec!["C"]),
        ])
    );
    let parameters = &decoded.ir().model.parameters;
    let ordinal = |name| parameters.iter().find(|p| p.name == name).unwrap().ordinal;
    assert!(ordinal("C") < ordinal("D"));
    assert!(ordinal("D") < ordinal("A"));
    assert!(ordinal("A") < ordinal("B"));
}

#[test]
fn parameter_cycle_ordering_resumes_before_breaking_another_cycle() {
    let decoded = decode_parameters(&[(0, "A", "D"), (1, "B", "C"), (2, "C", "B"), (3, "D", "A")]);
    let mut ordered = decoded.ir().model.parameters.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|parameter| parameter.ordinal);
    assert_eq!(
        ordered
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "D", "B", "C"]
    );
}

/// A chain where `P{i}` names `P{i - 1}` and has the reverse source ordinal,
/// so ordering needs one round per link.
fn reversed_chain(length: u32) -> Vec<(u32, String, String)> {
    (0..length)
        .map(|index| {
            let expression = match index {
                0 => "1".to_owned(),
                _ => format!("P{}", index - 1),
            };
            (length - 1 - index, format!("P{index}"), expression)
        })
        .collect()
}

fn decode_chain(chain: &[(u32, String, String)]) -> DecodeResult {
    let records = chain
        .iter()
        .map(|(ordinal, name, expression)| (*ordinal, name.as_str(), expression.as_str()))
        .collect::<Vec<_>>();
    decode_parameters(&records)
}

#[test]
fn parameter_ordering_within_its_cap_orders_a_reversed_chain() {
    let decoded = decode_chain(&reversed_chain(10));
    let parameters = &decoded.ir().model.parameters;
    let by_name = |name: String| parameters.iter().find(|p| p.name == name).unwrap();
    for index in 1..10 {
        let parameter = by_name(format!("P{index}"));
        let dependency = by_name(format!("P{}", index - 1));
        assert_eq!(
            parameter
                .expression
                .dependencies()
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            [dependency.id.clone()]
        );
        assert!(dependency.ordinal < parameter.ordinal);
    }
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::ParameterOrderBudgetExceeded.kind()));
}

#[test]
fn parameter_ordering_past_its_cap_keeps_source_ordinals_and_all_references() {
    // Round `k` scans the `3000 - k` unresolved parameters and one dependency
    // each: about 3000^2 work units, past the per-scope cap.
    let chain = reversed_chain(3000);
    let decoded = decode_chain(&chain);
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| parameter.expression.dependencies().count())
            .sum::<usize>(),
        2999
    );
    for (ordinal, name, _) in &chain {
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.name == *name)
            .unwrap();
        assert_eq!(parameter.ordinal, *ordinal);
    }
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::ParameterExpressionUnbound.kind()));
    let skip = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::ParameterOrderBudgetExceeded.kind())
        .unwrap();
    assert_eq!(
        skip.message,
        "1 Design parameter owner scope(s) keep their source ordinals and all expression references because ordering exceeded the local work cap PARAMETER_ORDER_WORK (4194304)."
    );
}
