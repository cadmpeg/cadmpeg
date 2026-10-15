// SPDX-License-Identifier: Apache-2.0

use super::super::{
    feature_datum_csys_payloads, offset_data_block_bytes, FeatureDatumCsysConstruction,
};
use cadmpeg_core::CodecError;

#[test]
fn datum_csys_missing_second_source_releases_candidate_copies() {
    let construction: FeatureDatumCsysConstruction = serde_json::from_str(
        r#"{"id":"c","operation_label":"o","control":255,"object_indices":[0,256,1,512,2,768,3,1024],"raw_object_indices":[[240,0],[241,1,0],[240,1],[241,2,0],[240,2],[241,3,0],[240,3],[241,4,0]],"data_blocks":["nx:om-data-blocks-0:block#1","nx:om-data-blocks-0:block#99","c","d","e","f","g","h"],"source_offsets":[114,116,119,121,124,126,129,131]}"#,
    )
    .unwrap();
    let part =
        crate::test_support::test_om::composed_feature_history_payload(&[], &[b"abc", b"def"]);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, &file)?;
        offset_data_block_bytes(ctx, &container)?;
        Ok::<_, CodecError>(container)
    })
    .unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 65_536;
        },
        |ctx| {
            let rows = feature_datum_csys_payloads(ctx, &container, &[construction])?;
            assert!(rows.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            let _checkpoint = ctx.reserve_scoped(65_536, "test rejected CSYS scratch release")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
