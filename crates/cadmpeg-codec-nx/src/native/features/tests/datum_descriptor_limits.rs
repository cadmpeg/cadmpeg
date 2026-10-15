// SPDX-License-Identifier: Apache-2.0

use crate::native::features::datum_plane_header::feature_datum_plane_headers;
use crate::native::features::feature_datum_csys_constructions;
use crate::native::features::feature_datum_csys_descriptors;
use crate::native::features::feature_datum_plane_csys_identity_uses;
use crate::native::features::feature_datum_plane_descriptors;

#[derive(Clone, Copy)]
enum DatumDescriptorRoute {
    Csys,
    Plane,
    IdentityUse,
}

fn assert_datum_descriptor_admits(route: DatumDescriptorRoute) {
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
    .expect("datum descriptor inputs");
    let csys = feature_datum_csys_descriptors(&container, &constructions)
        .expect("admitted datum csys descriptors");
    let plane = feature_datum_plane_descriptors(&container, &headers)
        .expect("admitted datum plane descriptors");
    assert!(!csys.is_empty());
    assert!(!plane.is_empty());
    let decode = || match route {
        DatumDescriptorRoute::Csys => {
            feature_datum_csys_descriptors(&container, &constructions).map(|rows| rows.len())
        }
        DatumDescriptorRoute::Plane => {
            feature_datum_plane_descriptors(&container, &headers).map(|rows| rows.len())
        }
        DatumDescriptorRoute::IdentityUse => {
            feature_datum_plane_csys_identity_uses(&plane, &csys).map(|rows| rows.len())
        }
    };
    assert!(decode().expect("admitted datum descriptor route") > 0);
}

macro_rules! datum_descriptor_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_datum_descriptor_admits($route);
        }
    };
}

datum_descriptor_admit_test!(
    datum_csys_descriptor_admits_fixture,
    DatumDescriptorRoute::Csys
);
datum_descriptor_admit_test!(
    datum_plane_descriptor_admits_fixture,
    DatumDescriptorRoute::Plane
);
datum_descriptor_admit_test!(
    datum_identity_use_admits_fixture,
    DatumDescriptorRoute::IdentityUse
);
