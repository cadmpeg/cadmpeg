// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_csys_payloads;
use crate::native::features::feature_datum_plane_payloads;

#[derive(Clone, Copy)]
enum DatumPayloadRoute {
    Csys,
    Plane,
}

fn assert_datum_payload_admits(route: DatumPayloadRoute) {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let (constructions, headers) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_datum_csys_constructions(&container)?,
            feature_datum_plane_headers(ctx, &container)?,
        ))
    })
    .expect("datum construction inputs");
    let decode = |_: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        DatumPayloadRoute::Csys => {
            feature_datum_csys_payloads(&container, &constructions).map(|rows| rows.len())
        }
        DatumPayloadRoute::Plane => {
            feature_datum_plane_payloads(&container, &headers).map(|rows| rows.len())
        }
    };
    let admitted = crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted datum payload route");
    assert_eq!(admitted, 1);
}

macro_rules! datum_payload_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_datum_payload_admits($route);
        }
    };
}

datum_payload_admit_test!(datum_csys_payload_admits_fixture, DatumPayloadRoute::Csys);
datum_payload_admit_test!(datum_plane_payload_admits_fixture, DatumPayloadRoute::Plane);
