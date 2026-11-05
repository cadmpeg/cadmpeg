// SPDX-License-Identifier: Apache-2.0
//! F3D writer error and entity-limit contract checks.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_registry::Format;
use cadmpeg_test_support::cross_closure::cross_request;
use cadmpeg_test_support::robustness::{
    injected_error_findings, standard_injections,
};

fn check_fixtures(
    check: impl Fn(&cadmpeg_ir::CadIr, &dyn cadmpeg_ir::codec::write::Encoder) -> Vec<String>,
) {
    let encoder = cadmpeg_registry::build_encoder(Format::F3d);
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let source: &dyn Codec = catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == "f3d")
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .expect("F3D codec");
    let mut findings = Vec::new();
    for path in super::sampled_paths(super::conformance::fixtures("f3d")) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let options = DecodeOptions {
            depth: encoder.request(),
            ..DecodeOptions::default()
        };
        let decoded = source.decode(&mut Cursor::new(bytes), &options).expect("fixture decode");
        findings.extend(check(decoded.ir(), encoder.as_ref()).into_iter().map(|finding| {
            format!("{}: {finding}", path.display())
        }));
    }
    assert!(findings.is_empty(), "{} finding(s):\n{}", findings.len(), findings.join("\n"));
}

#[test]
fn f3d_injected_errors_name_the_record() {
    let injections = standard_injections();
    check_fixtures(|ir, encoder| {
        injected_error_findings(ir, encoder, cross_request(encoder), &injections)
    });
}

