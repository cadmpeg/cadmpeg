// SPDX-License-Identifier: Apache-2.0
//! IGES writer error and resource-limit contract checks.

use std::io::Cursor;

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
    if cfg!(debug_assertions) {
        paths.truncate(1);
    }
    let mut findings = Vec::new();
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let options = DecodeOptions {
            depth: encoder.request(),
            ..DecodeOptions::default()
        };
        let Ok(decoded) = source.decode(&mut Cursor::new(bytes), &options) else {
            continue;
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
        let count = ir.census().values().sum::<usize>() as u64;
        max_entities_findings(ir, encoder, cross_request(encoder), count)
    });
}
