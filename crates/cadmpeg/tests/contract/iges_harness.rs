// SPDX-License-Identifier: Apache-2.0
//! IGES writer error and resource-limit contract checks.

use std::io::Cursor;

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_registry::Format;
use cadmpeg_test_support::cross_closure::cross_request;
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
};

use super::conformance::fixtures;

fn check_fixtures(
    check: impl Fn(&cadmpeg_ir::CadIr, &dyn cadmpeg_ir::codec::write::Encoder) -> Vec<String>,
) {
    let encoder = cadmpeg_registry::build_encoder(Format::Iges);
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let source: &dyn Codec = catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == "iges")
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .expect("IGES codec");
    let mut paths = fixtures("iges");
    paths.sort();
    let mut findings = Vec::new();
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let options = DecodeOptions {
            depth: encoder.request(),
            ..DecodeOptions::default()
        };
        let decoded = match source.decode(&mut Cursor::new(bytes), &options) {
            Ok(decoded) => decoded,
            Err(error) => {
                findings.push(format!(
                    "{}: fixture decode failed: {error}",
                    path.display()
                ));
                continue;
            }
        };
        findings.extend(
            check(decoded.ir(), encoder.as_ref())
                .into_iter()
                .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

#[test]
fn iges_injected_errors_name_the_record() {
    let injections = standard_injections();
    check_fixtures(|ir, encoder| {
        injected_error_findings(ir, encoder, cross_request(encoder), &injections)
    });
}

#[test]
fn iges_max_entities_refuses_below_count() {
    check_fixtures(|ir, encoder| {
        let request = cross_request(encoder);
        let mut bytes = Vec::new();
        let Ok(plan) = encoder.plan(EncodeInput::new(ir, None), request) else {
            return vec!["the control write without a ceiling refuses".into()];
        };
        if let Err(error) = plan.write_to(&mut bytes) {
            return vec![format!("the control write failed: {error}")];
        }
        let cards = bytes
            .split(|byte| *byte == b'\n')
            .filter(|card| card.get(72) == Some(&b'D'))
            .count();
        if cards % 2 != 0 {
            return vec!["the control output has an incomplete Directory entry".into()];
        }
        let count = (cards / 2) as u64;
        if count == 0 {
            let mut input = EncodeInput::new(ir, None);
            input.limits.max_entities = 0;
            return match encoder.plan(input, request) {
                Ok(_) => Vec::new(),
                Err(error) => vec![format!(
                    "empty output refused a zero entity ceiling: {error}"
                )],
            };
        }
        max_entities_findings(ir, encoder, request, count)
    });
}
