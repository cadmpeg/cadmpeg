// SPDX-License-Identifier: Apache-2.0
//! The stated wire forms of §2.5, checked against what the design types write
//! and read.

use std::collections::BTreeSet;
use std::num::NonZeroI8;

use serde_json::json;

use super::fixtures::{self, feature, id, length, parameter_ref, picked};
use super::{read_desktop, refuses, to_value};
use crate::design::construction::SourceMode;
use crate::design::dimension::Dimension;
use crate::design::expr::{self, BinaryOp, Expression, ExpressionDialect, Neutral, Node};
use crate::design::names::{NameTable, TopoName};
use crate::design::options::{Boolean, BooleanOp, Hand};
use crate::design::refs::{Evaluated, StateRef};
use crate::design::selection::{
    Keyed, Members, Ordered, PickDomain, PickKey, PickTarget, Picked, Picks, Unordered, Witness,
};
use crate::design::units::{Duration, Unit, UnitFactor, UnitSymbol};
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::{DegenerateCurve, LineCurve};
use crate::geometry::{InlineCurve, SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::scalar::Length;
use crate::transform::Transform;
use crate::units::UnitVector3;

#[test]
fn the_examples_of_section_2_5() {
    assert_eq!(to_value(&Hand::Left), json!("left"));
    let literal = Dimension::<Length>::Literal(length(2.5));
    assert_eq!(to_value(&literal), json!({"kind": "literal", "value": 2.5}));
    assert_eq!(
        to_value(&Picks::<Ordered>::Unread),
        json!({"kind": "unread"})
    );
    assert_eq!(
        to_value(&Boolean::Apply {
            op: BooleanOp::Cut,
            targets: None
        }),
        json!({"kind": "apply", "op": {"kind": "cut"}})
    );
}

#[test]
fn a_neutral_expression_is_text_with_side_lists() {
    let reference = parameter_ref(1);
    let tree = Node::Binary {
        op: BinaryOp::Add,
        left: Box::new(Node::Binary {
            op: BinaryOp::Multiply,
            left: Box::new(Node::Reference(reference.clone())),
            right: Box::new(fixtures::number(2.0, Unit::default())),
        }),
        right: Box::new(fixtures::number(10.0, fixtures::mm())),
    };
    let value = to_value(&Expression::Neutral(Neutral(tree)));
    assert_eq!(value["kind"], json!("neutral"));
    assert_eq!(value["value"]["text"], json!("$0 * 2 + 10[mm]"));
    assert_eq!(value["value"]["refs"], json!([to_value(&reference)]));
    assert_eq!(value["value"]["measures"], json!([]));
}

#[test]
fn maps_are_objects_or_sorted_pairs_and_refuse_a_repeated_key() {
    let mode = fixtures::source_mode();
    let written = serde_json::to_string(&mode).unwrap();
    assert!(
        written.contains("\"source_settings\":{}"),
        "string keys write an object: {written}"
    );
    let repeated = written.replace(
        "\"native_options\":{",
        "\"native_options\":{\"sldprt:x\":{\"kind\":\"bool\",\"value\":{\"kind\":\"literal\",\"value\":true}},\"sldprt:x\":{\"kind\":\"bool\",\"value\":{\"kind\":\"literal\",\"value\":true}},",
    );
    assert!(
        refuses::<SourceMode>(&repeated).contains("duplicate key sldprt:x"),
        "{repeated}"
    );

    let table = fixtures::name_tables().remove(1);
    assert!(
        to_value(&table).is_array(),
        "EntityRef keys write [key, value] pairs"
    );
    let key = json!({"kind": "body", "value": "test:design:body#1"});
    let entry = json!({"native": {"codec": "sldprt", "value": {"kind": "text", "value": "Body1"}}});
    let repeated = json!([[key, entry], [key, entry]]).to_string();
    assert!(refuses::<NameTable>(&repeated).contains("twice"));
    let later = json!({"kind": "body", "value": "test:design:body#2"});
    let unsorted = json!([[later, entry], [key, entry]]).to_string();
    assert!(refuses::<NameTable>(&unsorted).contains("out of key order"));
}

#[test]
fn an_omitted_optional_field_refuses_null_and_unknown_fields_are_refused() {
    let tool = json!({"kind": "tool", "value": "test:design:feature#1"});
    let state = StateRef {
        configuration: None,
        of: Evaluated::Tool(feature(1)),
    };
    assert_eq!(to_value(&state), json!({"of": tool}));
    refuses::<StateRef>(&json!({"configuration": null, "of": tool}).to_string());
    refuses::<StateRef>(&json!({"of": tool, "colour": 3}).to_string());
    // An internally tagged enum.
    refuses::<Boolean>(r#"{"kind": "apply", "op": {"kind": "cut"}, "targets": null}"#);
    refuses::<Boolean>(r#"{"kind": "apply", "op": {"kind": "cut"}, "extra": 1}"#);
    refuses::<Boolean>(r#"{"kind": "new_body", "extra": 1}"#);
    // An adjacently tagged enum.
    refuses::<TopoName>(
        r#"{"kind": "root", "value": {"container": "test:design:container#1"}, "extra": 1}"#,
    );
    assert_eq!(
        read_desktop::<Boolean>(r#"{"kind": "apply", "op": {"kind": "join"}}"#).unwrap(),
        Boolean::Apply {
            op: BooleanOp::Join,
            targets: None
        }
    );
}

#[test]
fn a_set_refuses_a_repeated_member_and_an_unsorted_array() {
    let leaf = to_value(&fixtures::generated(1));
    let merged = json!({"kind": "merged", "value": {"from": [leaf, leaf]}}).to_string();
    assert!(refuses::<TopoName>(&merged).contains("twice"), "{merged}");
    let mut merged = to_value(&TopoName::Merged {
        feature: None,
        from: BTreeSet::from([fixtures::generated(1), fixtures::generated(2)]),
    });
    merged["value"]["from"].as_array_mut().unwrap().reverse();
    let merged = merged.to_string();
    assert!(
        refuses::<TopoName>(&merged).contains("out of order"),
        "{merged}"
    );
}

/// Every `Option` field is omitted for `None` and refuses `null`, and every
/// list, map, and set field is always written and refuses omission.
#[test]
fn an_option_field_is_omitted_and_a_list_field_is_always_written() {
    let merged = TopoName::Merged {
        feature: None,
        from: BTreeSet::new(),
    };
    assert_eq!(
        to_value(&merged),
        json!({"kind": "merged", "value": {"from": []}})
    );
    refuses::<TopoName>(r#"{"kind": "merged", "value": {"from": [], "feature": null}}"#);
    refuses::<TopoName>(r#"{"kind": "merged", "value": {}}"#);
    let entity = to_value(&fixtures::generated(1));
    assert_eq!(
        to_value(&picked(1)),
        json!({"target": {"kind": "entity", "value": entity}})
    );
}

/// "A number has one zero: −0 equals 0, a float zero is written `0.0`, and a
/// writer never writes −0."
#[test]
fn negative_zero_is_never_written() {
    for zero in [0.0, -0.0] {
        assert_eq!(
            serde_json::to_string(&Duration::new(zero).unwrap()).unwrap(),
            "0.0"
        );
    }
    let read_back: Duration = read_desktop("-0.0").unwrap();
    assert_eq!(serde_json::to_string(&read_back).unwrap(), "0.0");
}

/// "A `symbol` is the wire name of a `UnitSymbol` variant". Every variant's
/// wire name has no underscore and is the symbol the text prints.
#[test]
fn a_unit_symbol_has_one_spelling() {
    assert_eq!(UnitSymbol::ALL.len(), 22);
    for symbol in UnitSymbol::ALL {
        let wire = to_value(&symbol).as_str().unwrap().to_owned();
        assert!(!wire.contains('_'), "{wire}");
        assert_eq!(wire, symbol.name());
        let unit = Unit(vec![UnitFactor {
            symbol,
            power: NonZeroI8::new(1).unwrap(),
        }]);
        let number = fixtures::number(30.0, unit);
        assert_eq!(expr::print(&number).unwrap().text, format!("30[{wire}]"));
    }
}

/// Members are distinct by identity, `(target, context, native, domain)`; two
/// picks that differ only in their witness are one member. Equality reads the
/// whole pick and order reads only its key, so a pick has no order of its own
/// and every sort and set of picks goes through the key.
#[test]
fn members_are_distinct_by_identity_not_by_witness() {
    let pick = fixtures::picks_full().remove(1);
    let mut other = pick.clone();
    other.witness = None;
    other.resolved = None;
    assert!(pick.witness.is_some() && pick != other);
    assert_eq!(pick.key(), other.key());
    assert!(Members::<Picked, Unordered>::new(vec![pick.clone(), other.clone()]).is_err());
    assert!(Members::<Picked, Ordered>::new(vec![other.clone(), pick.clone()]).is_err());

    let limited = |end: &Picked| Picked {
        domain: Some(PickDomain::Limits {
            start: None,
            end: Some(Box::new(end.clone())),
        }),
        ..picked(3)
    };
    assert_ne!(limited(&pick), limited(&other));
    assert_eq!(limited(&pick).key(), limited(&other).key());
    assert!(Members::<Picked, Unordered>::new(vec![limited(&pick), limited(&other)]).is_err());

    let picks = fixtures::picks_full();
    for left in &picks {
        for right in &picks {
            if left == right {
                assert_eq!(left.key(), right.key(), "{left:?}");
            }
            assert_eq!(
                left.key().cmp(&right.key()).is_eq(),
                left.key() == right.key()
            );
        }
    }
    let keys: BTreeSet<PickKey<'_>> = picks.iter().map(Picked::key).collect();
    let set = Members::<Picked, Unordered>::new(picks.clone()).unwrap();
    assert_eq!(keys.len(), picks.len());
    assert!(set
        .as_slice()
        .iter()
        .map(Picked::key)
        .eq(keys.iter().copied()));
}

/// An unordered member list sorts by `(target, context, native, domain)` and
/// reads only in that order; an ordered one keeps source order.
#[test]
fn unordered_members_sort_by_identity_and_ordered_members_keep_order() {
    let mut late = picked(1);
    late.native = Some(fixtures::native_name());
    let first = picked(1);
    let other = Picked {
        target: PickTarget::Feature(feature(1)),
        ..picked(1)
    };
    let given = vec![late.clone(), other.clone(), first.clone()];
    let set = Members::<Picked, Unordered>::new(given.clone()).unwrap();
    let mut sorted = given.clone();
    sorted.sort_by(|left, right| left.key().cmp(&right.key()));
    assert_eq!(set.as_slice(), sorted.as_slice());
    assert_eq!(set.as_slice()[0].target, first.target);
    assert!(set.as_slice()[0].native.is_none() && set.as_slice()[1].native.is_some());
    let list = Members::<Picked, Ordered>::new(given.clone()).unwrap();
    assert_eq!(list.as_slice(), given.as_slice());

    let written = serde_json::to_string(&Picks::<Unordered>::Named(set)).unwrap();
    let back: Picks<Unordered> = read_desktop(&written).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), written);
    let unsorted = serde_json::to_string(&given).unwrap();
    assert!(refuses::<Members<Picked, Unordered>>(&unsorted).contains("identity-key order"));
    let read_list: Members<Picked, Ordered> = read_desktop(&unsorted).unwrap();
    assert_eq!(read_list.as_slice(), given.as_slice());
    let doubled = serde_json::to_string(&vec![first.clone(), first]).unwrap();
    assert!(refuses::<Members<Picked, Unordered>>(&doubled).contains("twice"));
    assert!(refuses::<Members<Picked, Ordered>>(&doubled).contains("twice"));
}

/// A witness body is the document's `Topology`: it writes the document's
/// form, and reading it attaches each procedural carrier to its owner.
#[test]
fn a_witness_body_is_the_document_topology() {
    let body = fixtures::witness_body();
    let witness = Witness {
        body: Some(Box::new(body.clone())),
        ..fixtures::witnesses().remove(1)
    };
    let value = to_value(&witness);
    assert_eq!(value["body"], to_value(&body));
    assert_eq!(
        value["body"]["procedural_curves"].as_array().map(Vec::len),
        Some(1)
    );
    let back: Witness = read_desktop(&value.to_string()).unwrap();
    assert_eq!(back.body.as_deref(), Some(&body));
    assert!(back.body.unwrap().curves[0]
        .geometry
        .procedural_construction()
        .is_some());
}

/// A witness carrier names no record and has one spelling (§5.6): a leaf
/// carrier and a placement of one read, and a degenerate curve, a composite
/// and an opaque carrier do not.
#[test]
fn a_witness_carrier_is_an_inline_carrier() {
    let line = LineCurve::new(FinitePoint3::ZERO, UnitVector3::X_AXIS);
    let witness = Witness {
        curves: vec![InlineCurve::Transformed {
            basis: Box::new(InlineCurve::Line(line)),
            transform: Transform::default(),
        }],
        ..fixtures::witnesses().remove(1)
    };
    let value = to_value(&witness);
    assert_eq!(value["curves"][0]["kind"], json!("transformed"));
    assert_eq!(value["curves"][0]["basis"]["kind"], json!("line"));
    let back: Witness = read_desktop(&value.to_string()).unwrap();
    assert_eq!(back, witness);

    let degenerate = SolvedCurveGeometry::Degenerate(DegenerateCurve::new(FinitePoint3::ZERO));
    let mut refused = value.clone();
    refused["curves"] = json!([to_value(&degenerate)]);
    refuses::<Witness>(&refused.to_string());
    let mut refused = value;
    refused["surface"] = to_value(&SolvedSurfaceGeometry::Unknown { record: None });
    refuses::<Witness>(&refused.to_string());
}

#[test]
fn a_dimension_binds_a_parameter_or_its_negation() {
    let parameter = Dimension::<Length>::Parameter(id("parameter", 1));
    assert_eq!(
        to_value(&parameter),
        json!({"kind": "parameter", "value": "test:design:parameter#1"})
    );
    let negated = Dimension::<bool>::Negated(id("parameter", 1));
    assert_eq!(to_value(&negated)["kind"], json!("negated"));
    assert_eq!(
        to_value(&ExpressionDialect::SolidWorks),
        json!({"kind": "solid_works"})
    );
    assert_eq!(to_value(&crate::codec_id!("sldprt")), json!("sldprt"));
}

#[test]
fn a_codec_id_is_lower_case_letters_and_digits() {
    use crate::design::names::CodecId;
    for id in ["3dm", "fcstd", "sldprt"] {
        let read: CodecId = read_desktop(&format!("{id:?}")).unwrap();
        assert_eq!(read.as_str(), id);
    }
    for id in [r#""""#, r#""Step""#, r#""st-ep""#, r#""f3d ""#] {
        refuses::<CodecId>(id);
    }
}

#[test]
fn a_codec_name_is_a_codec_id_a_colon_and_a_name() {
    use crate::design::names::CodecName;
    for text in [
        "fcstd:Transition",
        "sldprt:auto_select",
        "rhino:loft:3",
        "f3d: a",
    ] {
        let read: CodecName = read_desktop(&format!("{text:?}")).unwrap();
        assert_eq!(to_value(&read), json!(text));
    }
    for text in [
        r#""thickness""#,
        r#"":thickness""#,
        r#""Sldprt:flag""#,
        r#""sldprt:""#,
        r#""sldprt: \u00a0""#,
        r#""sld prt:flag""#,
    ] {
        refuses::<CodecName>(text);
    }
    assert_eq!(
        CodecName::new(&crate::codec_id!("f3d"), "distance"),
        Some(crate::codec_name!("f3d:distance"))
    );
    assert_eq!(CodecName::new(&crate::codec_id!("f3d"), " "), None);
}

#[test]
fn a_native_arm_that_holds_a_codec_name_refuses_other_text() {
    use crate::design::names::GenerationRole;
    use crate::design::options::NativeOptions;
    use crate::design::selection::SelectionRule;
    refuses::<SelectionRule>(r#"{"kind": "native", "value": "tangent_curves"}"#);
    refuses::<GenerationRole>(r#"{"kind": "other", "value": "lip"}"#);
    refuses::<NativeOptions>(
        r#"{"thickness": {"kind": "bool", "value": {"kind": "literal", "value": true}}}"#,
    );
    let rule: SelectionRule =
        read_desktop(r#"{"kind": "native", "value": "nx:tangent_curves"}"#).unwrap();
    assert_eq!(
        rule,
        SelectionRule::Native(crate::codec_name!("nx:tangent_curves"))
    );
}
