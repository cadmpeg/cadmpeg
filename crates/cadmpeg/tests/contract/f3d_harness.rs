// SPDX-License-Identifier: Apache-2.0
//! F3D writer error and entity-limit contract checks.

use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_registry::Format;
use cadmpeg_test_support::cross_closure::cross_request;
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
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


#[test]
fn f3d_max_entities_refuses_below_brep_record_count() {
    check_fixtures(|ir, encoder| {
        let request = cross_request(encoder);
        let mut bytes = Vec::new();
        let plan = match encoder.plan(EncodeInput::new(ir, None), request) {
            Ok(plan) => plan,
            Err(error) => return vec![format!("the control plan refuses: {error}")],
        };
        if let Err(error) = plan.write_to(&mut bytes) {
            return vec![format!("the control write refuses: {error}")];
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("generated ZIP");
        let name = archive.file_names().map(|name| name.expect("ZIP entry name"))
            .find(|name| name.ends_with(".smbh")).expect("generated BREP").into_owned();
        let mut smbh = Vec::new();
        archive.by_name(&name).expect("BREP entry").read_to_end(&mut smbh).expect("BREP bytes");
        let start = cadmpeg_asm::asm_header::record_stream_start(&smbh).expect("BREP header");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
        let records = cadmpeg_asm::sab::frame(&ctx, &smbh, start, smbh.len(),
            cadmpeg_asm::kernel_header::RefWidth::Eight, None).expect("BREP records");
        let count = u64::try_from(records.len()).expect("record count") + 1;
        max_entities_findings(ir, encoder, request, count)
    });
}
