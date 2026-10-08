// SPDX-License-Identifier: Apache-2.0
//! Missing archive members stop root-reference traversal.

use cadmpeg_core::CodecError;
use zip::CompressionMethod;

#[test]
fn missing_first_member_does_not_admit_later_references() {
    let work = |count| {
        let references = (1..=count).map(|id| {
            let uri = if id == 1 { "missing.p21" } else { "https://example.invalid/part" };
            format!("#{id}=<{uri}>;")
        }).collect::<String>();
        let root = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;{references}ENDSEC;DATA;ENDSEC;END-ISO-10303-21;");
        let bytes = super::step_zip(&[(super::ROOT_NAME, root.as_bytes(), CompressionMethod::Stored)]);
        crate::test_support::with_service_context(&bytes, |source, service| {
            let opened = crate::archive::open_root(service, cadmpeg_core::decode::View::over_retained(source)).expect("root archive");
            let (exchange, _) = crate::parse::parse_with_context(opened.view.window(), service).expect("root references");
            crate::test_support::with_service_context(&[], |_, ctx| {
                assert!(matches!(crate::archive::root_reference_notes(ctx, &opened.archive, &exchange),
                    Err(CodecError::Malformed(message)) if message.contains("has no archive member")));
                let CodecError::ResourceLimit(refusal) = ctx.charge_work(u64::MAX, "test archive work")
                    .expect_err("work probe refuses") else { panic!("work refusal required"); };
                refusal.used
            })
        })
    };
    assert_eq!(work(1), work(1024));
}
