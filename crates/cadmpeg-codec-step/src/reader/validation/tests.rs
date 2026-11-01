// SPDX-License-Identifier: Apache-2.0
//! STEP geometric validation-property tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::declared_measures::{DeclaredMeasure, DeclaredQuantity, DeclaredTarget};
use cadmpeg_ir::CadIr;

use crate::test_support::exchange::decode_inline;
use crate::test_support::validated_part::{part, validation_property, Item, PartBody};
use crate::StepCodec;

const CUBE: PartBody = PartBody::Solid {
    min: [0.0; 3],
    max: [10.0; 3],
};

fn decode_source(source: &str) -> cadmpeg_ir::codec::DecodeResult {
    StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode validation fixture")
}

fn tessellation_fixture() -> String {
    String::from_utf8(include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec())
        .expect("fixture is UTF-8")
}

/// The declared measures of the property `#property`.
fn of_property(ir: &CadIr, property: u64) -> Vec<&DeclaredMeasure> {
    let prefix = format!("step:data:declared_measure#{property}-");
    ir.model
        .declared_measures
        .iter()
        .filter(|measure| measure.id.as_str().starts_with(&prefix))
        .collect()
}

fn quantities(measures: &[&DeclaredMeasure]) -> Vec<DeclaredQuantity> {
    measures
        .iter()
        .map(|measure| measure.quantity.clone())
        .collect()
}

fn scalar(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> cadmpeg_ir::features::FinitePoint3 {
    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, y, z)).unwrap()
}

fn unsupported_items(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "validation.item-unsupported")
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn complex_validation_measure_carrier_is_decoded() {
    let source = tessellation_fixture()
        .replace(
            "#42=REPRESENTATION('surface area',(#43),#2);",
            "#42=(REPRESENTATION('surface area',(#43),#2) SHAPE_REPRESENTATION());",
        )
        .replace(
            "#43=MEASURE_REPRESENTATION_ITEM('surface area measure',AREA_MEASURE(50.),#44);",
            "#43=(MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(AREA_MEASURE(50.),#44) REPRESENTATION_ITEM('surface area measure'));",
        );
    let result = decode_source(&source);

    assert_eq!(
        quantities(&of_property(result.ir(), 41)),
        [DeclaredQuantity::SolidArea {
            value: scalar(50.0)
        }]
    );
    assert!(unsupported_items(&result).is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_area_and_volume_unit_subtypes_scale_validation_measures() {
    let source = tessellation_fixture()
        .replace("#44=DERIVED_UNIT((#55));", "#44=AREA_UNIT((#55));")
        .replace("#53=DERIVED_UNIT((#56));", "#53=VOLUME_UNIT((#56));");
    let result = decode_source(&source);

    assert_eq!(
        quantities(&of_property(result.ir(), 41)),
        [DeclaredQuantity::SolidArea {
            value: scalar(50.0)
        }]
    );
    assert_eq!(
        quantities(&of_property(result.ir(), 50)),
        [DeclaredQuantity::Volume { value: scalar(0.0) }]
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("unit scale did not resolve") }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [44, 53, 55, 56] {
        assert!(
            !unknowns
                .iter()
                .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
            "validation unit carrier #{id} was not typed"
        );
    }
}

#[test]
fn validation_representation_decodes_all_measure_items() {
    let source = tessellation_fixture().replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=REPRESENTATION('surface area',(#43,#52),#2);",
    );
    let result = decode_source(&source);

    assert_eq!(
        quantities(&of_property(result.ir(), 41)),
        [
            DeclaredQuantity::SolidArea {
                value: scalar(50.0)
            },
            DeclaredQuantity::Volume { value: scalar(0.0) },
        ]
    );
    assert!(unsupported_items(&result).is_empty());
}

#[test]
fn a_property_of_a_face_has_no_body_target() {
    let result = decode_source(&tessellation_fixture());

    for property in [41, 46, 50, 57] {
        for measure in of_property(result.ir(), property) {
            assert_eq!(measure.target, DeclaredTarget::Unresolved {}, "{measure:?}");
        }
    }
}

#[test]
fn ps06_combined_validation_items_ignore_set_order_and_keep_siblings() {
    let source_order = decode_source(
        std::str::from_utf8(include_bytes!(
            "tests/data/ps06_combined_validation_properties_source_order.p21"
        ))
        .unwrap(),
    );
    let reordered = decode_source(
        std::str::from_utf8(include_bytes!(
            "tests/data/ps06_combined_validation_properties_reordered.p21"
        ))
        .unwrap(),
    );

    let expected = vec![
        DeclaredQuantity::SolidArea {
            value: scalar(50.0),
        },
        DeclaredQuantity::SolidCentroid {
            value: point(1.0, 2.0, 3.0),
        },
        DeclaredQuantity::Volume { value: scalar(8.0) },
    ];
    assert_eq!(quantities(&of_property(source_order.ir(), 41)), expected);
    assert_eq!(quantities(&of_property(reordered.ir(), 41)), expected);
    assert_eq!(
        unsupported_items(&source_order),
        ["geometric validation property #41 has unsupported item #60"]
    );
    assert_eq!(
        unsupported_items(&source_order),
        unsupported_items(&reordered)
    );
}

#[test]
fn validation_shape_representation_with_parameters_uses_inherited_items() {
    let source = tessellation_fixture().replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=SHAPE_REPRESENTATION_WITH_PARAMETERS('surface area',(#43),#2);",
    );
    let result = decode_source(&source);

    assert_eq!(
        quantities(&of_property(result.ir(), 41)),
        [DeclaredQuantity::SolidArea {
            value: scalar(50.0)
        }]
    );
    assert!(unsupported_items(&result).is_empty());
}

#[test]
fn a_product_definition_shape_property_targets_the_definition() {
    let part = part(&[CUBE]);
    let validation = validation_property(
        &part,
        1000,
        part.shape,
        &[
            Item::Volume("volume measure", 1000.0),
            Item::Area("surface area measure", 600.0),
            Item::Point("centre point", [5.0, 5.0, 5.0]),
        ],
    );
    let result = decode_inline(&format!("{}{validation}", part.records));
    let ir = result.ir();
    let [definition] = ir.model.product_definitions.as_slice() else {
        panic!("one product definition");
    };
    let measures = of_property(ir, 1000);
    assert_eq!(
        quantities(&measures),
        [
            DeclaredQuantity::Volume {
                value: scalar(1000.0)
            },
            DeclaredQuantity::SolidArea {
                value: scalar(600.0)
            },
            DeclaredQuantity::SolidCentroid {
                value: point(5.0, 5.0, 5.0)
            },
        ]
    );
    for measure in &measures {
        assert_eq!(
            measure.target,
            DeclaredTarget::Definition {
                definition: definition.id.clone()
            }
        );
        assert_eq!(measure.label.as_deref(), Some("validated"));
    }
    assert!(ir
        .model
        .points
        .iter()
        .all(|point| { !point.id.as_str().ends_with(&format!("#{}", 1009)) }));
}

/// A part of two solid boxes whose first box carries a shape aspect, with
/// `association` relating the aspect `#900` to its geometry.
fn aspect_part(
    association: impl Fn(&crate::test_support::validated_part::Part) -> String,
) -> (CadIr, Vec<cadmpeg_ir::ids::BodyId>) {
    let part = part(&[
        CUBE,
        PartBody::Solid {
            min: [20.0, 0.0, 0.0],
            max: [30.0, 10.0, 10.0],
        },
    ]);
    let validation =
        validation_property(&part, 1000, 900, &[Item::Volume("volume measure", 1000.0)]);
    let records = format!(
        "{}#900=SHAPE_ASPECT('','first box',#{},.F.);\n{}{validation}",
        part.records,
        part.shape,
        association(&part)
    );
    let result = decode_inline(&records);
    let first = format!("step:data:body#{}", part.items[0]);
    let bodies = result
        .ir()
        .model
        .bodies
        .iter()
        .filter(|body| body.id.as_str() == first)
        .map(|body| body.id.clone())
        .collect();
    (result.ir().clone(), bodies)
}

#[test]
fn a_shape_aspect_resolves_through_a_shape_definition_representation() {
    let (ir, first) = aspect_part(|part| {
        format!(
            "#901=PROPERTY_DEFINITION('','shape for validation properties',#900);\n\
             #902=SHAPE_REPRESENTATION('',(#{}),#{});\n\
             #903=SHAPE_DEFINITION_REPRESENTATION(#901,#902);\n",
            part.items[0], part.context
        )
    });
    assert_eq!(first.len(), 1);
    let [measure] = of_property(&ir, 1000).try_into().unwrap();
    assert_eq!(measure.target, DeclaredTarget::Bodies { bodies: first });
}

#[test]
fn a_shape_aspect_resolves_through_a_geometric_item_specific_usage() {
    let (ir, first) = aspect_part(|part| {
        format!(
            "#901=GEOMETRIC_ITEM_SPECIFIC_USAGE('','shape for validation properties',#900,#{},#{});\n",
            part.representation, part.items[0]
        )
    });
    let [measure] = of_property(&ir, 1000).try_into().unwrap();
    assert_eq!(measure.target, DeclaredTarget::Bodies { bodies: first });
}

#[test]
fn a_shape_aspect_of_a_face_has_no_body_target() {
    let (ir, _) = aspect_part(|part| {
        format!(
            "#901=GEOMETRIC_ITEM_SPECIFIC_USAGE('','face',#900,#{},#{});\n",
            part.representation,
            part.items[0] - 2
        )
    });
    let [measure] = of_property(&ir, 1000).try_into().unwrap();
    assert_eq!(measure.target, DeclaredTarget::Unresolved {});
}

#[test]
fn a_shape_aspect_without_geometry_has_no_body_target() {
    let (ir, _) = aspect_part(|_| String::new());
    let [measure] = of_property(&ir, 1000).try_into().unwrap();
    assert_eq!(measure.target, DeclaredTarget::Unresolved {});
}

#[test]
fn item_names_select_the_class_of_each_quantity() {
    let part = part(&[
        CUBE,
        PartBody::Sheet {
            min: [20.0, 0.0, 0.0],
            max: [30.0, 10.0, 10.0],
        },
    ]);
    let validation = validation_property(
        &part,
        1000,
        part.shape,
        &[
            Item::Area("wetted area measure", 600.0),
            Item::Area("independent surface area measure", 500.0),
            Item::Point("surface centre point", [25.0, 5.0, 4.0]),
            Item::Length("curve length measure", 0.0),
            Item::Point("curve centre point", [0.0, 0.0, 0.0]),
            Item::Point("bounding box corner point", [0.0, 0.0, 0.0]),
            Item::Volume("surface area measure", 1.0),
        ],
    );
    let result = decode_inline(&format!("{}{validation}", part.records));
    assert_eq!(
        quantities(&of_property(result.ir(), 1000)),
        [
            DeclaredQuantity::WettedArea {
                value: scalar(600.0)
            },
            DeclaredQuantity::SheetArea {
                value: scalar(500.0)
            },
            DeclaredQuantity::SheetCentroid {
                value: point(25.0, 5.0, 4.0)
            },
        ]
    );
    let first = 1007;
    assert_eq!(
        unsupported_items(&result),
        (first + 3..first + 7)
            .map(|item| format!("geometric validation property #1000 has unsupported item #{item}"))
            .collect::<Vec<_>>()
    );
    for id in first..first + 7 {
        assert!(
            !result
                .ir()
                .model
                .points
                .iter()
                .any(|point| point.id.as_str().ends_with(&format!("#{id}"))),
            "validation point #{id} is a model point"
        );
    }
}

#[test]
fn unresolved_or_dimensionally_incompatible_units_are_not_compared() {
    for unit in [
        "#44=(AREA_UNIT() NAMED_UNIT(*));",
        "#44=DERIVED_UNIT((#56));",
    ] {
        let source = tessellation_fixture().replace("#44=DERIVED_UNIT((#55));", unit);
        let result = decode_source(&source);
        assert!(of_property(result.ir(), 41).is_empty());
        assert!(!unsupported_items(&result)
            .iter()
            .any(|message| message.ends_with("item #43")));
        assert!(
            result.report().losses.iter().any(|loss| {
                loss.message
                    .contains("unit scale did not resolve; the quantity is not compared")
            }),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn validation_unit_conversion_refuses_nonfinite_quantities() {
    let source = tessellation_fixture()
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)")
        .replace("AREA_MEASURE(50.)", "AREA_MEASURE(1.E308)");
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect_err("area overflows cubic document scaling");
    assert!(
        error
            .to_string()
            .contains("not finite after unit conversion"),
        "{error}"
    );
}
