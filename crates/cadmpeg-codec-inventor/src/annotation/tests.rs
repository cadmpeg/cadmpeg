// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{AttributeId, FaceId};
use cadmpeg_ir::pmi::{DimensionTolerance, PmiQuantity};

use super::{
    attribute_name, edge_name, fit, generated_name, naming_table, tolerance, BodyNames, EntityKind,
    NameFamily, Value,
};

const VALUE_TOLERANCE: f64 = 1.0e-12;

fn name_prefix(form: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(0x1234_u16.to_le_bytes());
    bytes.extend(form.to_le_bytes());
    bytes.extend(0x3000_0002_u32.to_le_bytes());
    bytes.extend([0; 24]);
    bytes.extend(0xb4_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

fn hole_name(pairs: &[(u32, i32)], first: u32, role: u16) -> Vec<u8> {
    let mut bytes = name_prefix(5);
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(0x3000_u16.to_le_bytes());
    bytes.extend(
        u32::try_from(pairs.len())
            .expect("pair count")
            .to_le_bytes(),
    );
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(0x1000_0000_u32.to_le_bytes());
    for (source, value) in pairs {
        bytes.extend(source.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(first.to_le_bytes());
    bytes.extend(role.to_le_bytes());
    bytes
}

fn hole_attribute(id: u32, face: &str, pairs: &[(i64, i64)], tail: &[i64]) -> SourceAttribute {
    let mut values = vec![
        AttributeValue::Reference("inventor:brep:entity#-1".to_owned()),
        AttributeValue::Integer(-1),
        AttributeValue::Reference("inventor:brep:entity#-1".to_owned()),
        AttributeValue::Reference("inventor:brep:entity#-1".to_owned()),
        AttributeValue::Reference(face.to_owned()),
        AttributeValue::Integer(43),
        AttributeValue::Integer(i64::try_from(pairs.len()).expect("pair count")),
    ];
    for (source, value) in pairs {
        values.push(AttributeValue::Integer(*source));
        values.push(AttributeValue::Integer(*value));
    }
    values.extend(tail.iter().map(|value| AttributeValue::Integer(*value)));
    SourceAttribute {
        id: AttributeId::mint(format!("inventor:brep:attribute#{id}")).expect("attribute id"),
        target: AttributeTarget::Face(FaceId::mint(face).expect("face id")),
        name: "NMx_Hole_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib".to_owned(),
        values,
    }
}

#[test]
fn hole_name_selects_the_face_with_equal_pairs_and_tail() {
    let record = hole_name(&[(1417, 5), (1344, -1)], 1, 7);
    let name = generated_name(NameFamily::Hole, View::over_retained(&record)).expect("name");
    assert_eq!(name.pairs, vec![(1417, 5), (1344, -1)]);
    let mut model = Model::default();
    model.attributes = vec![
        hole_attribute(
            1,
            "inventor:brep:entity#10",
            &[(1417, 5), (1344, -1)],
            &[1, 7],
        ),
        hole_attribute(
            2,
            "inventor:brep:entity#11",
            &[(1417, 5), (1344, -1)],
            &[1, 9],
        ),
        hole_attribute(
            3,
            "inventor:brep:entity#12",
            &[(1417, 2), (1344, -1)],
            &[1, 7],
        ),
    ];
    let names = BodyNames::new(&model);
    assert_eq!(
        names.select(&name, EntityKind::Face),
        Some("inventor:brep:entity#10")
    );
    assert_eq!(names.select(&name, EntityKind::Edge), None);
}

#[test]
fn equal_generated_names_on_two_faces_select_nothing() {
    let record = hole_name(&[(945, -1)], 1, 7);
    let name = generated_name(NameFamily::Hole, View::over_retained(&record)).expect("name");
    let mut model = Model::default();
    model.attributes = vec![
        hole_attribute(1, "inventor:brep:entity#10", &[(945, -1)], &[1, 7]),
        hole_attribute(2, "inventor:brep:entity#11", &[(945, -1)], &[1, 7]),
    ];
    assert_eq!(BodyNames::new(&model).select(&name, EntityKind::Face), None);
}

#[test]
fn name_without_terminal_value_is_rejected() {
    let record = hole_name(&[(945, 3)], 1, 7);
    assert!(generated_name(NameFamily::Hole, View::over_retained(&record)).is_err());
}

#[test]
fn custom_attribute_reads_schema_44_family_pairs() {
    let mut attribute = hole_attribute(1, "inventor:brep:entity#10", &[(761, -1)], &[120, 0, 18]);
    attribute.name = "ATTRIB_CUSTOM-attrib".to_owned();
    attribute.values.splice(
        5..6,
        [
            AttributeValue::String("INV_NMX_GEN_TAG".to_owned()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(44),
        ],
    );
    let (family, pairs, tail) =
        attribute_name(&attribute.name, &attribute.values).expect("generated name");
    assert_eq!(family, NameFamily::Generated);
    assert_eq!(pairs, vec![(761, -1)]);
    assert_eq!(
        NameFamily::Generated.attribute_tail(&tail),
        Some(vec![120, 18])
    );
}

#[test]
fn naming_table_maps_identifiers_to_one_based_references() {
    let mut bytes = Vec::new();
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(0x1e_u16.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(9_u32.to_le_bytes());
    bytes.extend(2_u32.to_le_bytes());
    for (id, reference) in [(1_u32, 0x8000_0667_u32), (9, 0x8000_0010)] {
        bytes.extend(id.to_le_bytes());
        bytes.extend(reference.to_le_bytes());
    }
    let table = naming_table(View::over_retained(&bytes)).expect("table");
    assert_eq!(table.get(&1), Some(&0x666));
    assert_eq!(table.get(&9), Some(&0x0f));
}

#[test]
fn edge_name_lists_its_two_face_names() {
    let mut bytes = name_prefix(1);
    for value in [0x8000_0004_u32, 1894, 0x8000_0004, 1894] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(0x3000_u16.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(2_u32.to_le_bytes());
    for face in [17_u32, 859] {
        bytes.extend(0x8000_0004_u32.to_le_bytes());
        bytes.extend(face.to_le_bytes());
    }
    assert_eq!(
        edge_name(View::over_retained(&bytes)).expect("edge name"),
        Some([17, 859])
    );
}

fn value(tolerance: u32, upper: f64, lower: f64, hole_fit: &str) -> Value {
    Value {
        measure: 1,
        tolerance,
        display: 0,
        upper,
        lower,
        hole_fit: hole_fit.to_owned(),
        shaft_fit: String::new(),
        nominal_parameter: 0,
    }
}

fn bounds(tolerance: Option<&DimensionTolerance>) -> Option<(f64, f64)> {
    match tolerance? {
        DimensionTolerance::PlusMinus { lower, upper }
        | DimensionTolerance::Limits { lower, upper }
        | DimensionTolerance::PlusMinusFit { lower, upper, .. } => {
            Some((lower.value.get(), upper.value.get()))
        }
        DimensionTolerance::Fit { .. } => None,
    }
}

fn close(actual: Option<(f64, f64)>, expected: (f64, f64)) -> bool {
    actual.is_some_and(|(lower, upper)| {
        (lower - expected.0).abs() < VALUE_TOLERANCE && (upper - expected.1).abs() < VALUE_TOLERANCE
    })
}

#[test]
fn tolerance_forms_scale_internal_centimetres() {
    let symmetric = tolerance(&value(6, 0.015, 0.015, ""), 2.5, 10.0, PmiQuantity::Length);
    assert!(close(bounds(symmetric.bounds.as_ref()), (-0.15, 0.15)));
    let deviation = tolerance(&value(7, 0.01, -0.005, ""), 2.0, 10.0, PmiQuantity::Length);
    assert!(close(bounds(deviation.bounds.as_ref()), (-0.05, 0.1)));
    let limits = tolerance(&value(3, 0.02, -0.02, ""), 3.5, 10.0, PmiQuantity::Length);
    assert!(matches!(
        limits.bounds,
        Some(DimensionTolerance::Limits { .. })
    ));
    assert!(close(bounds(limits.bounds.as_ref()), (34.8, 35.2)));
    let fitted = tolerance(
        &value(2, 0.0012, 0.0004, "G6"),
        0.35,
        10.0,
        PmiQuantity::Length,
    );
    assert!(matches!(
        fitted.bounds,
        Some(DimensionTolerance::PlusMinusFit { .. })
    ));
    assert!(close(bounds(fitted.bounds.as_ref()), (0.004, 0.012)));
    let maximum = tolerance(&value(4, 0.0, 0.0, ""), 0.02, 10.0, PmiQuantity::Length);
    assert!(maximum.resolved && maximum.bounds.is_none());
    assert_eq!(maximum.modifier, Some("maximum"));
    let none = tolerance(&value(0, 0.0, 0.0, ""), 1.0, 10.0, PmiQuantity::Length);
    assert!(none.resolved && none.bounds.is_none() && none.modifier.is_none());
    let unknown = tolerance(&value(5, 0.0, 0.0, ""), 1.0, 10.0, PmiQuantity::Length);
    assert!(!unknown.resolved && unknown.bounds.is_none());
}

#[test]
fn fit_classes_split_letters_and_grade() {
    let hole = fit("G6", "").expect("hole class");
    assert_eq!(
        (
            hole.form_variance.as_str(),
            hole.zone_variance.as_str(),
            hole.grade.as_str()
        ),
        ("G", "hole", "6")
    );
    let shaft = fit("", "h7").expect("shaft class");
    assert_eq!(shaft.zone_variance, "shaft");
    assert!(fit("H7", "h7").is_none());
    assert!(fit("g6", "").is_none());
}
