// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn different_revisions_keep_fillet_and_chamfer_selections_native() {
    let ctx = crate::native::test_ctx();
    for schema in [38, 40, 42, 43, 44] {
        for chamfer in [false, true] {
            for reverse in [false, true] {
                let bytes = selector_bytes(&[31, 37], &[], 29);
                let selector = parse(View::over_retained(&bytes), 29)
                    .expect("framed selector")
                    .expect("direct selector");
                let mut revisions = records(schema, &[31, 37]);
                second_owner(&mut revisions, schema, 1.0, "straight");
                if reverse {
                    revisions.reverse();
                }
                let edges = selected(&ctx, &revisions, &[selector], schema)
                    .expect("permissive revision join");
                assert!(edges.is_empty());
                let (mut inventory, mut feature) = fixture(&ctx, chamfer);
                inventory.edge_inputs.historical.clear();
                let original = selections(&feature)[0].clone();
                assert!(matches!(original, EdgeSelection::Native(_)));
                let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory)
                    .expect("permissive feature projection");
                assert!(states.is_empty());
                assert_eq!(selections(&feature)[0], &original);
                let losses = native_losses(std::slice::from_ref(&feature));
                assert_eq!(losses.len(), 1);
                assert_eq!(
                    losses[0].code,
                    crate::loss::InventorLossCode::FeatureEdgeSelectionNative
                        .note("unresolved body state")
                        .code
                );
            }
        }
    }
}
