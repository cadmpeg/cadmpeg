// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_block_uses;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_plane_block_uses;
use crate::native::features::feature_input_blocks;

#[derive(Clone, Copy)]
enum DatumBlockUseRoute {
    Csys,
    Plane,
}

fn assert_datum_block_use_admits(route: DatumBlockUseRoute) {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let (constructions, headers, inputs) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_datum_csys_constructions(&container)?,
            feature_datum_plane_headers(ctx, &container)?,
            feature_input_blocks(&container)?,
        ))
    })
    .expect("datum block use inputs");
    let decode = || match route {
        DatumBlockUseRoute::Csys => {
            Ok(feature_datum_csys_block_uses(&constructions, &inputs).len())
        }
        DatumBlockUseRoute::Plane => {
            feature_datum_plane_block_uses(&headers, &inputs).map(|values| values.len())
        }
    };
    assert!(decode().expect("admitted datum block use") > 0);
}

macro_rules! datum_block_use_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_datum_block_use_admits($route);
        }
    };
}

datum_block_use_admit_test!(
    datum_csys_block_use_admits_fixture,
    DatumBlockUseRoute::Csys
);
datum_block_use_admit_test!(
    datum_plane_block_use_admits_fixture,
    DatumBlockUseRoute::Plane
);
