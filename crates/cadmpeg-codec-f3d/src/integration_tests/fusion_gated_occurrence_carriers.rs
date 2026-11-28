// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion assembly whose external occurrence carriers store the source
//! placement after the three gating attributes.

use super::{decode, f3d_native};
use crate::loss::F3dLossCode;

#[test]
fn source_placement_carriers_with_gating_attributes_place_each_screw() {
    let result = decode(
        include_bytes!("../../../../corpus/f3d/occurrence_carriers_gating_attributes.f3z").to_vec(),
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()),
        "{:#?}",
        result.report().losses
    );
    let ir = result.ir();
    let native = f3d_native(ir);
    // Three Fastener (Screw) features each insert one occurrence of the linked
    // screw design, seated on the plate's holes.
    let mut screws = native
        .xref_references
        .iter()
        .filter(|reference| reference.relative_path.starts_with("50d4777d"))
        .map(|reference| {
            let occurrence = ir
                .model
                .occurrences
                .iter()
                .find(|o| o.native_ref.as_deref() == Some(reference.id.as_str()))
                .expect("an occurrence per screw carrier");
            let t = cadmpeg_test_support::assembly::resolved_transform(ir, &occurrence.id)
                .expect("a resolved pose");
            let m = t.affine_rows();
            [m[0][3], m[1][3], m[2][3]]
        })
        .collect::<Vec<_>>();
    screws.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let expected = [[10.0, 10.0, 10.0], [30.0, 30.0, 10.0], [50.0, 10.0, 10.0]];
    assert_eq!(screws.len(), 3, "{screws:?}");
    for (got, want) in screws.iter().zip(expected) {
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1.0e-9, "{screws:?}");
        }
    }
}
