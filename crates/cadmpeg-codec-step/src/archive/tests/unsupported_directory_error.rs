// SPDX-License-Identifier: Apache-2.0
//! Unsupported directory compression retains the error that leaves its scope.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use zip::CompressionMethod;

#[test]
fn unsupported_compression_error_escapes_directory_scope_as_retained_text() {
    let name = "A".repeat(4096);
    let mut bytes = super::step_zip(&[(name.as_str(), b"".as_slice(), CompressionMethod::Stored)]);
    let central = {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("synthetic directory");
        let file = archive.by_index_raw(0).expect("synthetic member");
        usize::try_from(file.central_header_start()).expect("central offset fits fixture")
    };
    // The fixture uses the same compression fields as the archive-detection
    // witnesses: local-header offset 8 and central-header offset 10.
    cadmpeg_test_support::bytes::put_u16(&mut bytes, 8, 12);
    cadmpeg_test_support::bytes::put_u16(&mut bytes, central + 10, 12);
    crate::test_support::with_service_context(&bytes, |source, ctx| {
        assert!(matches!(crate::archive::open_root(ctx, View::over_retained(source)),
            Err(CodecError::NotImplemented(message)) if message.contains(&name)));
    });
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes,
        "STEP ZIP directory error", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::test_support::with_policy_context(&bytes, &policy, |source, ctx| {
                crate::archive::open_root(ctx, View::over_retained(source)).map(|_| ())
            })
        });
}
