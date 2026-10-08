// SPDX-License-Identifier: Apache-2.0
//! Missing archive members stop root-reference traversal.

use std::fmt::Write as _;

use cadmpeg_core::CodecError;
use zip::CompressionMethod;

#[test]
fn missing_first_member_does_not_admit_later_references() {
    let work = |count| {
        let references = (1..=count).fold(String::new(), |mut references, id| {
            let uri = if id == 1 {
                "missing.p21"
            } else {
                "https://example.invalid/part"
            };
            write!(references, "#{id}=<{uri}>;").expect("reference fixture text");
            references
        });
        let root = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;{references}ENDSEC;DATA;ENDSEC;END-ISO-10303-21;");
        let bytes =
            super::step_zip(&[(super::ROOT_NAME, root.as_bytes(), CompressionMethod::Stored)]);
        crate::test_support::with_service_context(&bytes, |source, service| {
            let opened = crate::archive::open_root(
                service,
                cadmpeg_core::decode::View::over_retained(source),
            )
            .expect("root archive");
            let (exchange, _) = crate::parse::parse_retained(opened.view.window(), service)
                .expect("root references");
            crate::test_support::with_service_context(&[], |_, ctx| {
                assert!(
                    matches!(crate::archive::root_reference_notes(ctx, &opened.archive, &exchange),
                    Err(CodecError::Malformed(message)) if message.contains("has no archive member"))
                );
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_work(u64::MAX, "test archive work")
                    .expect_err("work probe refuses")
                else {
                    panic!("work refusal required");
                };
                refusal.used
            })
        })
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn invalid_first_archive_name_does_not_admit_later_entries() {
    let work = |count| {
        let names = (1..count)
            .map(|id| format!("part-{id}.p21"))
            .collect::<Vec<_>>();
        let mut entries = vec![("../bad", b"".as_slice(), CompressionMethod::Stored)];
        entries.extend(
            names
                .iter()
                .map(|name| (name.as_str(), b"".as_slice(), CompressionMethod::Stored)),
        );
        let bytes = super::step_zip(&entries);
        let measured = |validate| {
            crate::test_support::with_service_context(&bytes, |source, ctx| {
                let view = cadmpeg_core::decode::View::over_retained(source);
                if validate {
                    assert!(
                        matches!(crate::archive::open_root(ctx, view), Err(CodecError::Malformed(message))
                    if message.contains("unsafe STEP ZIP entry path"))
                    );
                } else {
                    cadmpeg_container::ArchiveSnapshot::new(ctx, view)
                        .expect("archive index is valid");
                }
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_work(u64::MAX, "test archive validation work")
                    .expect_err("work probe refuses")
                else {
                    panic!("work refusal required");
                };
                refusal.used
            })
        };
        // Both routes build the same archive index. The difference is the
        // first entry visit, its name validation, and its error text.
        measured(true) - measured(false)
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn stored_root_directory_storage_is_temporary_until_drop() {
    let root = b"ISO-10303-21;";
    let bytes = super::step_zip(&[(super::ROOT_NAME, root.as_slice(), CompressionMethod::Stored)]);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65_536;
    crate::test_support::with_policy_context(&bytes, &policy, |source, ctx| {
        let opened =
            crate::archive::open_root(ctx, cadmpeg_core::decode::View::over_retained(source))
                .expect("directory scratch needs no retained allowance");
        assert_eq!(opened.view.window(), root);
        assert_eq!(opened.archive.entries().len(), 1);
        drop(opened);
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released directory",
        )
        .expect("directory storage is released with the snapshot");
    });
}

#[test]
fn directory_errors_preserve_retained_admission() {
    let source = b"PK\x03\x04";
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "STEP ZIP directory error",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::test_support::with_policy_context(source, &policy, |source, ctx| {
                crate::archive::open_root(ctx, cadmpeg_core::decode::View::over_retained(source))
                    .map(|_| ())
            })
        },
    );
}
