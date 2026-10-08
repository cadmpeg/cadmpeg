// SPDX-License-Identifier: Apache-2.0
//! Color precedence, source bindings and context-qualified styles.

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::StepCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn styled_free_curve_is_a_reachable_source_carrier() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
         #2=CARTESIAN_POINT('',(1.,0.,0.));
         #7=POLYLINE('',(#1,#2));
         #10=STYLED_ITEM('',(),#7);",
    );
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#7")
        .expect("styled polyline carrier");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#7")
    }));
}

#[test]
fn overriding_style_suppresses_the_base_binding() {
    let result = decode_inline(
        "#1=COLOUR_RGB('blue',0.,0.,1.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('red',1.,0.,0.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#10=STYLED_ITEM('',(#2),#20);
#11=OVER_RIDING_STYLED_ITEM('',(#4),#20,#10);
#20=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let binding = &result.ir().model.appearance_bindings[0];
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .expect("overriding appearance");
    let color = appearance.base_color.expect("override color");
    assert_eq!((color.r(), color.g(), color.b()), (1.0, 0.0, 0.0));
}

#[test]
fn independent_face_styles_keep_bindings_without_source_order_scalar_color() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#68=STYLED_ITEM('',(#66),#19);",
            "#68=STYLED_ITEM('',(#66),#19);\n#69=COLOUR_RGB('independent blue',0.,0.,1.);\n#70=FILL_AREA_STYLE_COLOUR('',#69);\n#71=FILL_AREA_STYLE('',(#70));\n#72=SURFACE_STYLE_FILL_AREA(#71);\n#73=SURFACE_SIDE_STYLE('',(#72));\n#74=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#75=PRESENTATION_STYLE_ASSIGNMENT((#74));\n#76=STYLED_ITEM('',(#75),#29);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent face styles");

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    assert!(face.color.is_none());
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .count(),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ConflictingScalarColors.kind()
            && loss.message.contains("#47")
            && loss.message.contains("#76")
            && loss.message.contains("scalar color omitted")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn independent_face_style_permutations_do_not_select_by_instance_order() {
    for input in [
        include_bytes!("data/ap05_independent_styles_first.p21").as_slice(),
        include_bytes!("data/ap05_independent_styles_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode independent style permutation");
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "step:data:face#29")
            .expect("styled face");
        assert!(face.color.is_none());

        let bindings = result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2);
        for (red, green, blue) in [(1.0, 0.0, 0.0), (0.0, 0.0, 1.0)] {
            assert!(result.ir().model.appearances.iter().any(|appearance| {
                appearance.base_color.is_some_and(|color| {
                    color.r() == red && color.g() == green && color.b() == blue
                })
            }));
        }
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ConflictingScalarColors.kind()
                && loss.message.contains("#47")
                && loss.message.contains("#76")
                && loss.message.contains("scalar color omitted")
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn independent_same_rgb_styles_choose_lower_alpha_for_scalar_color() {
    const EPS_ALPHA: f32 = 0.000_001;

    let source = String::from_utf8(
        include_bytes!("data/ap05_independent_styles_first.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#60=COLOUR_RGB('independent blue',0.,0.,1.);",
        "#60=COLOUR_RGB('independent red transparent',1.,0.,0.);",
    )
    .replace(
        "#43=SURFACE_STYLE_FILL_AREA(#42);",
        "#43=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#40,(#77));",
    )
    .replace(
        "#63=SURFACE_STYLE_FILL_AREA(#62);",
        "#63=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#60,(#78));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#77=SURFACE_STYLE_TRANSPARENT(0.25);\n#78=SURFACE_STYLE_TRANSPARENT(0.75);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode same-RGB independent styles");
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    let color = face.color.expect("same-RGB scalar face color");
    assert!((color.a() - 0.25).abs() < EPS_ALPHA);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::ConflictingScalarColors.kind() }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn same_type_surface_colors_do_not_select_by_alpha_or_source_order() {
    let source = "#1=COLOUR_RGB('red',1.,0.,0.);
#2=COLOUR_RGB('blue',0.,0.,1.);
#3=SURFACE_STYLE_TRANSPARENT(0.25);
#4=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#1,(#3));
#5=SURFACE_STYLE_TRANSPARENT(0.75);
#6=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#2,(#5));
#7=PRESENTATION_STYLE_ASSIGNMENT((#4,#6));
#8=STYLED_ITEM('',(#7),#9);
#9=(ADVANCED_FACE() FACE_SURFACE());";
    let reordered = source.replace("(#4,#6)", "(#6,#4)");

    for input in [source, reordered.as_str()] {
        let result = decode_inline(input);
        assert!(result.ir().model.appearance_bindings.is_empty());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ConflictingScalarColors.kind()
                && loss.message.contains("STYLED_ITEM #8")
                && loss.message.contains("equal-precedence")
        }));
        assert!(result
            .ir()
            .model
            .faces
            .iter()
            .all(|face| face.color.is_none()));
    }
}

#[test]
fn context_dependent_styles_are_not_flattened_without_context() {
    let result = decode_inline(
        "#1=COLOUR_RGB('shaded red',1.,0.,0.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('wire blue',0.,0.,1.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#5=CARTESIAN_POINT('shaded context',(0.,0.,0.));
#6=CARTESIAN_POINT('wire context',(1.,0.,0.));
#7=PRESENTATION_STYLE_BY_CONTEXT((#2),#5);
#8=PRESENTATION_STYLE_BY_CONTEXT((#4),#6);
#9=STYLED_ITEM('',(#7,#8),#10);
#10=CARTESIAN_POINT('styled point',(0.,1.,0.));",
    );

    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:styled_item#9",
        "step:data:presentation_style_by_context#7",
        "step:data:presentation_style_by_context#8",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
            && loss.message.contains("#7 in #5")
            && loss.message.contains("#8 in #6")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn context_dependent_styles_remain_native_for_distinct_contexts() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/ap08_context_styles.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode context-qualified style witness");

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert!(matches!(
        &result.ir().model.appearance_bindings[0].target,
        cadmpeg_ir::appearance::AppearanceTarget::Point(point)
            if point.as_str() == "step:data:point#13"
    ));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
            && loss.message.contains("#10 in #5")
            && loss.message.contains("#11 in #7")
    }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:styled_item#12",
        "step:data:presentation_style_by_context#10",
        "step:data:presentation_style_by_context#11",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(!unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:styled_item#15"));
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#3"));
}

#[test]
fn context_style_retention_is_independent_of_style_set_order() {
    for source in [
        include_bytes!("data/ap08_context_styles.p21").as_slice(),
        include_bytes!("data/ap08_context_styles_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode context style order witness");
        assert!(result.ir().model.appearance_bindings.iter().all(|binding| {
            !matches!(
                &binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Point(point)
                    if point.as_str() == "step:data:point#3"
            )
        }));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
                && loss.message.contains("#10 in #5")
                && loss.message.contains("#11 in #7")
        }));
        let unknowns = result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena");
        for id in [
            "step:data:styled_item#12",
            "step:data:presentation_style_by_context#10",
            "step:data:presentation_style_by_context#11",
        ] {
            assert!(
                unknowns.iter().any(|record| record.id.as_str() == id),
                "missing {id}"
            );
        }
    }
}

#[test]
fn scalar_conflict_losses_follow_face_then_body_identity_order() {
    let source = String::from_utf8_lossy(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
        .replace("ENDSEC;\nEND-ISO-10303-21;", "#100=STYLED_ITEM('',(#66),#29);\n#101=STYLED_ITEM('',(#46),#31);\n#102=STYLED_ITEM('',(#66),#31);\nENDSEC;\nEND-ISO-10303-21;");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("independent scalar conflicts");
    let conflicts = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ConflictingScalarColors.kind())
        .collect::<Vec<_>>();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts[0].message.contains("face#29"));
    assert!(conflicts[1].message.contains("body#31"));
}
