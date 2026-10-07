// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::decode::{admit_protein_asset, admit_protein_rejection};
use crate::native::protein::ProteinRejectionRecordWire;

#[test]
fn protein_admission_keeps_later_assets_and_rejections() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut issues = Vec::new();
    let assets = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            let wire = serde_json::from_value(serde_json::json!({
                "id": "asset", "entry_name": entry_name, "ordinal": 3,
                "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
                    "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
            }))
            .expect("Protein asset wire fixture");
            admit_protein_asset(&ctx, wire, &mut issues).expect("service admission")
        })
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 1);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].scope, "asset");
    assert!(issues[0].detail.contains("entry_name"));
    let rejections = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            admit_protein_rejection(
                &ctx,
                ProteinRejectionRecordWire {
                    id: "rejection".into(),
                    entry_name: entry_name.into(),
                    ordinal: 4,
                    detail: "invalid record".into(),
                },
                &mut issues,
            )
            .expect("service admission")
        })
        .collect::<Vec<_>>();
    assert_eq!(rejections.len(), 1);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[1].scope, "rejection");
    assert!(issues[1].detail.contains("entry_name"));
}
