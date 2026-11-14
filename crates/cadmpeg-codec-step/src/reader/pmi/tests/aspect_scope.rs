// SPDX-License-Identifier: Apache-2.0

use crate::test_support::exchange::decode_inline;
use crate::StepCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn all_around_scope_uses_the_tolerances_direct_target() {
    use cadmpeg_ir::pmi::{PmiDefinition, PmiTarget};

    for scope in [
        "ALL_AROUND_SHAPE_ASPECT('scope','',#5,.T.)",
        "(ALL_AROUND_SHAPE_ASPECT() COMPOSITE_SHAPE_ASPECT() CONTINUOUS_SHAPE_ASPECT() SHAPE_ASPECT('scope','',#5,.T.))",
    ] {
        let decoded = decode_inline(&format!(
            "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6={scope};
#7=SHAPE_ASPECT('first','',#5,.T.);
#8=SHAPE_ASPECT('second','',#5,.T.);
#9=SHAPE_ASPECT_RELATIONSHIP('','',#6,#7);
#10=SHAPE_ASPECT_RELATIONSHIP('','',#6,#8);
#20=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.5),#1);
#30=SURFACE_PROFILE_TOLERANCE('around','',#20,#6);
#31=SURFACE_PROFILE_TOLERANCE('component','',#20,#7);
#99=UNRESOLVED_PRODUCT();"
        ));
        for (name, expected) in [("around", true), ("component", false)] {
            let annotation = decoded.ir().model.pmi.iter()
                .find(|annotation| annotation.name.as_deref() == Some(name))
                .expect("surface profile annotation");
            let PmiDefinition::GeometricTolerance { modifiers, .. } = &annotation.definition else {
                panic!("geometric tolerance")
            };
            assert_eq!(modifiers.iter().any(|modifier| modifier == "all_around"), expected);
            if expected {
                assert!(annotation.targets.iter().any(|target| {
                    matches!(target, PmiTarget::ShapeAspect { source_id } if source_id.as_str() == "#6")
                }));
            }
        }
    }
}

#[test]
fn nested_all_around_components_do_not_cross_into_sibling_annotations() {
    use cadmpeg_ir::pmi::PmiTarget;
    let source = std::str::from_utf8(include_bytes!("../../../../tests/fixtures/ap203_sheet.p21"))
        .expect("public fixture text")
        .replace("\r\n", "\n")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#38=PRODUCT_DEFINITION_SHAPE('PMI shape','',$);
#39=COMPOSITE_SHAPE_ASPECT('members','',#38,.T.);
#40=SHAPE_ASPECT('face member','',#38,.T.);
#41=SHAPE_ASPECT('point member','',#38,.T.);
#42=SHAPE_ASPECT_RELATIONSHIP('','',#39,#40);
#43=SHAPE_ASPECT_RELATIONSHIP('','',#39,#41);
#44=ALL_AROUND_SHAPE_ASPECT('scope','',#38,.T.);
#45=SHAPE_ASPECT_RELATIONSHIP('','',#44,#39);
#46=SHAPE_ASPECT('other member','',#38,.T.);
#47=SHAPE_ASPECT_RELATIONSHIP('','',#44,#46);
#50=CARTESIAN_POINT('PMI point',(1.,2.,3.));
#48=SHAPE_ASPECT_RELATIONSHIP('invalid point member','',#44,#50);
#49=GEOMETRIC_ITEM_SPECIFIC_USAGE('invalid definition','',#50,#32,#55);
#51=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#40,#32,#29);
#52=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#41,#32,#50);
#53=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#54=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.5),#53);
#55=CARTESIAN_POINT('unrelated point',(9.,8.,7.));
#56=SHAPE_ASPECT('unrelated','',#38,.T.);
#57=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#56,#32,#55);
#58=SURFACE_PROFILE_TOLERANCE('unrelated','',#54,#56);
#60=SURFACE_PROFILE_TOLERANCE('around','',#54,#44);
#61=SURFACE_PROFILE_TOLERANCE('point only','',#54,#41);
ENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested component ownership");
    let around = decoded
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("around"))
        .unwrap();
    assert!(around
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Face { .. })));
    assert!(around
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Point { .. })));
    assert_eq!(
        around
            .targets
            .iter()
            .filter(|target| matches!(target, PmiTarget::Point { .. }))
            .count(),
        1
    );
    let point = decoded
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("point only"))
        .unwrap();
    assert!(point
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Point { .. })));
    assert!(!point
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Face { .. })));
}
