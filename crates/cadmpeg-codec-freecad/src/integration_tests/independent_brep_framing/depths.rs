// SPDX-License-Identifier: Apache-2.0
//! Finite all-table carriers retain identical native facts at every decode depth.
use super::*;
use cadmpeg_core::decode::DecodeDepth;

#[test]
fn container_and_geometry_depths_use_every_actual_brep_source_level() {
    let mut levels = BTreeSet::new();
    for binary in [false, true] {
        let bytes = fixtures::archive(binary, &fixtures::member(binary));
        for bytes in crate::test_support::framing::zip_forms(&bytes) {
            let full = decode(&bytes);
            let expected = full.ir().native.namespace("fcstd").unwrap();
            for depth in [DecodeDepth::Container, DecodeDepth::Geometry] {
                let actual = crate::FcstdCodec
                    .decode(
                        &mut Cursor::new(&bytes),
                        &DecodeOptions {
                            depth,
                            ..DecodeOptions::default()
                        },
                    )
                    .unwrap();
                for part in assert_parts_agree(&bytes, &actual) {
                    levels.insert(part.level);
                }
                let native = actual.ir().native.namespace("fcstd").unwrap();
                for arena in [
                    "shape_payloads",
                    "polygons3d",
                    "polygons_on_triangulations",
                    "carrier_census",
                ] {
                    assert_eq!(
                        native.arenas().get(arena),
                        expected.arenas().get(arena),
                        "{depth:?} {arena}"
                    );
                }
                assert!(crate::test_support::validate_native(actual.ir()).is_empty());
                crate::fidelity::b1::assert_classified_spans(&actual);
                if depth == DecodeDepth::Container {
                    assert!(actual.ir().model.curves.is_empty());
                    assert!(actual.ir().model.surfaces.is_empty());
                    assert!(actual.ir().model.pcurves.is_empty());
                    assert!(actual.ir().model.vertices.is_empty());
                    assert!(actual.ir().model.edges.is_empty());
                    assert!(actual.ir().model.faces.is_empty());
                    assert!(actual.ir().model.bodies.is_empty());
                    assert!(actual.ir().model.tessellations.is_empty());
                }
            }
        }
    }
    assert_eq!(
        levels.len(),
        16,
        "all Appendix C exact-shape levels are present"
    );
}
