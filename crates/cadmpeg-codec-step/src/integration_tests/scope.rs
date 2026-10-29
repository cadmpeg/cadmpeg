// SPDX-License-Identifier: Apache-2.0
//! Source-located STEP classes and the format's empty design class.

use std::io::Cursor;

use cadmpeg_core::decode::{ArenaClass, ContentScope, DecodeDepth};
use cadmpeg_ir::assets::AssetContent;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Point3;

use crate::StepCodec;

#[test]
fn geometry_request_keeps_the_full_step_model_without_claiming_design() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('point',(1.,2.,3.));#2=GEOMETRIC_SET('',(#1));ENDSEC;END-ISO-10303-21;";
    let full = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("full STEP model");
    let geometry = StepCodec::default()
        .decode(
            &mut Cursor::new(source),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .expect("geometry STEP model");
    assert_eq!(full.ir().scope, ContentScope::full());
    assert_eq!(geometry.ir().scope, ContentScope::geometry());
    assert_eq!(full.report().scope(), full.ir().scope);
    assert_eq!(geometry.report().scope(), geometry.ir().scope);
    assert_eq!(full.ir().model, geometry.ir().model);
    assert_eq!(geometry.ir().model.points.len(), 1);
    assert_eq!(
        geometry.ir().model.points[0].position().get(),
        Point3::new(1., 2., 3.)
    );
    assert!(geometry.report().geometry_transferred());
    assert!(full
        .ir()
        .model
        .populated_arena(ArenaClass::Design)
        .is_none());
    let container = StepCodec::default()
        .decode(
            &mut Cursor::new(source),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .expect("container request excludes both classes");
    assert_eq!(container.ir().scope, ContentScope::container());
    assert_eq!(container.report().scope(), ContentScope::container());
    assert_eq!(container.ir().model.entity_count(), 0);
    assert!(!container.report().geometry_transferred());
}

#[test]
fn located_empty_and_failed_data_sections_keep_model_in_scope() {
    for population in ["", "#1=BROKEN(%,3);"] {
        let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{population}ENDSEC;END-ISO-10303-21;");
        for depth in [DecodeDepth::Geometry, DecodeDepth::Full] {
            let decoded = StepCodec::default()
                .decode(
                    &mut Cursor::new(&source),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .expect("located DATA remains held without delivered geometry");
            assert_eq!(
                decoded.ir().scope,
                ContentScope {
                    model: true,
                    design: depth == DecodeDepth::Full
                }
            );
            assert_eq!(decoded.report().scope(), decoded.ir().scope);
            assert_eq!(decoded.ir().model.entity_count(), 0);
            assert!(!decoded.report().geometry_transferred());
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.to_string() == "decode/depth.not-delivered"));
            if !population.is_empty() {
                assert_eq!(
                    decoded
                        .report()
                        .losses
                        .iter()
                        .filter(|loss| loss.code.local_code() == "record.unreadable")
                        .count(),
                    1
                );
                assert!(decoded
                    .source_fidelity()
                    .retained_records()
                    .values()
                    .any(|record| !record.untyped().is_empty()));
            }
        }
    }
}

#[test]
fn header_only_edition_three_does_not_claim_a_found_model_part() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;";
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let decoded = StepCodec::default()
            .decode(
                &mut Cursor::new(source),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("edition-three header without a model population");
        assert_eq!(
            decoded.ir().scope,
            ContentScope {
                model: false,
                design: depth == DecodeDepth::Full
            }
        );
        assert_eq!(decoded.report().scope(), decoded.ir().scope);
        assert_eq!(decoded.ir().model.entity_count(), 0);
        assert!(!decoded.report().geometry_transferred());
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.to_string() == "decode/depth.not-delivered")
                .count(),
            usize::from(depth != DecodeDepth::Container)
        );
    }
}

#[test]
fn a_real_external_asset_holds_model_even_without_a_data_section() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('reference'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<child.step#part>;ENDSEC;END-ISO-10303-21;";
    for depth in [DecodeDepth::Geometry, DecodeDepth::Full] {
        let decoded = StepCodec::default()
            .decode(
                &mut Cursor::new(source),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("actual external resource without a DATA population");
        assert_eq!(
            decoded.ir().scope,
            ContentScope {
                model: true,
                design: depth == DecodeDepth::Full
            }
        );
        assert_eq!(decoded.report().scope(), decoded.ir().scope);
        assert_eq!(decoded.ir().model.assets.len(), 1);
        assert!(
            matches!(&decoded.ir().model.assets[0].content, AssetContent::External { uri } if uri.as_str() == "child.step#part")
        );
        assert!(!decoded.report().geometry_transferred());
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "decode/depth.not-delivered"));
    }
}
