// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{self as expr, Expression, ExpressionToken};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

use super::line::{self, Line, Parsed, Segment};
use super::translate::{self, Typing};
use super::{walk, Names, Walk, MANAGER, RELATION, SIDE};
use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

/// The repeat token of the relation class in the synthesized managers.
const RELATION_TOKEN: u16 = 0x8105;
/// Bytes a relation owns after its side tag: a u16 token count and a stand-in
/// for its side objects.
const OWNED: &[u8] = b"\x01\x00\x00\x00\x00\x00owned";

fn counted_utf16(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut bytes = b"\xff\xfe\xff".to_vec();
    match u8::try_from(units.len()) {
        Ok(count) if count != 0xff => bytes.push(count),
        _ => {
            bytes.push(0xff);
            bytes.extend(u16::try_from(units.len()).unwrap().to_le_bytes());
        }
    }
    bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    bytes
}

/// A configuration stream holding a manager of `lines`, each relation
/// followed by the six bytes `after_text`.
fn manager_with(lines: &[&str], after_text: [u8; 6]) -> Vec<u8> {
    let mut bytes = b"moPart_c settings before the manager".to_vec();
    bytes.extend(MANAGER);
    bytes.extend(u16::try_from(lines.len()).unwrap().to_le_bytes());
    for (index, text) in lines.iter().enumerate() {
        if index == 0 {
            bytes.extend(RELATION);
        } else {
            bytes.extend(RELATION_TOKEN.to_le_bytes());
        }
        bytes.extend(counted_utf16(text));
        bytes.extend(after_text);
        if index == 0 {
            bytes.extend(SIDE);
        } else {
            bytes.extend((RELATION_TOKEN + 2).to_le_bytes());
        }
        bytes.extend(OWNED);
    }
    bytes.extend(b"\x00\x00detailing settings");
    bytes
}

fn manager(lines: &[&str]) -> Vec<u8> {
    manager_with(lines, [0, 2, 0, 0, 0, 0])
}

#[test]
fn a_manager_lists_its_relation_texts_in_stored_order() {
    let lines = [
        "\"D1@Sketch1\" = \"width\" * 2",
        "'a comment",
        "\"width\" = 4",
    ];
    assert_eq!(
        walk(&manager(&lines)),
        Some(Walk::Lines(
            lines.iter().map(|line| (*line).to_owned()).collect()
        ))
    );
    let older = manager_with(&lines, [0; 6]);
    assert_eq!(walk(&older).unwrap().lines().unwrap().len(), 3);
    let long = "\"D1@Sketch1\" = ".to_owned() + &"1 + ".repeat(80) + "1";
    assert_eq!(
        walk(&manager(&[&long])),
        Some(Walk::Lines(vec![long.clone()]))
    );
}

#[test]
fn a_manager_that_stops_before_its_count_is_unread() {
    let mut bytes = manager(&["\"A\" = 1"]);
    let count = bytes
        .windows(MANAGER.len())
        .position(|w| w == MANAGER)
        .unwrap()
        + MANAGER.len();
    bytes[count] = 2;
    assert_eq!(walk(&bytes), Some(Walk::Stopped));
    let mut wrong_side = manager(&["\"A\" = 1", "\"B\" = 2"]);
    let side = wrong_side
        .windows(2)
        .rposition(|w| w == (RELATION_TOKEN + 2).to_le_bytes())
        .unwrap();
    wrong_side[side] ^= 1;
    assert_eq!(walk(&wrong_side), Some(Walk::Stopped));
    assert_eq!(walk(b"no manager here"), None);
}

#[test]
fn a_line_is_a_quoted_name_an_expression_and_a_trailing_comment() {
    assert_eq!(line::parse("  'note"), Parsed::Comment);
    assert_eq!(
        line::parse("\"D1@Sketch1\" = \"a \"\"b\"\"\" * 2 + \"it's\" 'why"),
        Parsed::Line(Line {
            name: "D1@Sketch1".into(),
            expression: "\"a \"\"b\"\"\" * 2 + \"it's\"".into(),
            segments: vec![
                Segment::Name("a \"b\"".into()),
                Segment::Text(" * 2 + ".into()),
                Segment::Name("it's".into()),
            ],
            comment: Some("why".into()),
        })
    );
    assert_eq!(line::parse("D1 = 2"), Parsed::Malformed);
    assert_eq!(line::parse("\"D1\" 2"), Parsed::Malformed);
    assert_eq!(line::parse("\"D1\" = \"open"), Parsed::Malformed);
    assert_eq!(line::parse("\"D1\" = 'comment only"), Parsed::Malformed);
    assert_eq!(line::quoted("a \"b\""), "\"a \"\"b\"\"\"");
}

fn id(name: &str) -> ParameterId {
    ParameterId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "parameter"),
        cadmpeg_ir::ids::IdentityKey::try_new(name.to_owned()).unwrap(),
    )
}

fn global(name: &str, kind: ValueKind) -> DesignParameter {
    DesignParameter {
        id: id(name),
        scope: super::super::root_definition(),
        owner: None,
        name: cadmpeg_core::text::NonBlankString::new(name.to_owned()),
        kind,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: Some(ParameterValue::Number(FiniteReal::new(1.0).unwrap())),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

/// The canonical text of `expression` read as the expression of a
/// parameter of `target`, over the parameters `L` (a length), `B` (a
/// length), `ang` (an angle), and the global variable `W`.
fn translated(
    expression: &str,
    target: Quantity,
    length_unit: Option<UnitSymbol>,
) -> Option<String> {
    let parameters = [
        global("L", ValueKind::Quantity(Quantity::LENGTH)),
        global("B", ValueKind::Quantity(Quantity::LENGTH)),
        global("ang", ValueKind::Quantity(Quantity::ANGLE)),
        global("W", ValueKind::Quantity(Quantity::REAL)),
    ];
    let names = Names::new(&parameters, &[]);
    let tree = translate::tree(
        expression,
        &Typing {
            parameters: &parameters,
            names: &names,
            target: &ValueKind::Quantity(target),
            length_unit,
        },
    )?;
    Some(expr::print(&tree).unwrap().text)
}

#[test]
fn a_bare_number_or_global_takes_the_quantity_of_its_sum_or_parameter() {
    let mm = Some(UnitSymbol::Mm);
    let length = Quantity::LENGTH;
    assert_eq!(
        translated("\"L\" * .125", length, mm).as_deref(),
        Some("$0 * 0.125")
    );
    assert_eq!(
        translated("(\"L\" - \"B\") / 2", length, mm).as_deref(),
        Some("($0 - $1) / 2")
    );
    assert_eq!(
        translated("\"L\" + 2", length, mm).as_deref(),
        Some("$0 + 2[mm]")
    );
    assert_eq!(
        translated("\"W\" + 2", length, mm).as_deref(),
        Some("$0 * 1[mm] + 2[mm]")
    );
    assert_eq!(
        translated("-3", length, Some(UnitSymbol::In)).as_deref(),
        Some("-3[in]")
    );
    assert_eq!(
        translated("2.5*4.4mm", length, None).as_deref(),
        Some("2.5 * 4.4[mm]")
    );
    assert_eq!(
        translated("\"ang\" + 15", Quantity::ANGLE, None).as_deref(),
        Some("$0 + 15[deg]")
    );
    assert_eq!(
        translated("\"W\" * 2", Quantity::REAL, None).as_deref(),
        Some("$0 * 2")
    );
}

#[test]
fn an_expression_outside_the_grammar_or_its_kind_has_no_tree() {
    let mm = Some(UnitSymbol::Mm);
    let length = Quantity::LENGTH;
    assert_eq!(
        translated("\"L\" + 5", length, None),
        None,
        "no length unit"
    );
    assert_eq!(
        translated("\"W\" * 2", length, mm),
        None,
        "a dimensionless product"
    );
    assert_eq!(translated("sin(\"ang\")", Quantity::REAL, mm), None);
    assert_eq!(translated("\"L\" ^ 2", length, mm), None);
    assert_eq!(translated("\"L\" + \"ang\"", length, mm), None);
    assert_eq!(translated("\"missing\" * 2", length, mm), None);
    assert_eq!(translated("2furlong", length, mm), None);
    assert_eq!(translated("(\"L\"", length, mm), None);
    let deep = "(".repeat(100) + "\"L\"" + &")".repeat(100);
    assert_eq!(translated(&deep, length, mm), None);
    let long = "\"L\"".to_owned() + &" + \"B\"".repeat(200);
    assert_eq!(translated(&long, length, mm), None);
}

const KEYWORDS: &[u8] = br#"<Keywords Name="Bracket">
    <Feature Name="Equations" Type="EquationDriven" id="16">
        <Dimension Name="wall">5</Dimension>
    </Feature>
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Dimension Name="D1">12.5mm</Dimension>
        <Dimension Name="D2">25mm</Dimension>
        <Dimension Name="D3">12.5mm</Dimension>
        <Dimension Name="D4">17.5mm</Dimension>
        <Dimension Name="D5">30deg</Dimension>
        <Dimension Name="D6">40mm</Dimension>
    </Extrusion>
</Keywords>"#;

fn decode_with(streams: &[(&str, Vec<u8>)]) -> (CadIr, Vec<LossNote>) {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", KEYWORDS));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[]),
    ));
    for (name, payload) in streams {
        source.extend(make_block(0x42, name, payload));
    }
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    (result.ir().clone(), result.report().losses.clone())
}

fn parameter<'a>(ir: &'a CadIr, name: &str) -> &'a DesignParameter {
    ir.design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some(name)
        })
        .unwrap()
}

fn reference_to(parameter: &DesignParameter) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(parameter.id.clone()),
        through: None,
    }
}

fn neutral_text(parameter: &DesignParameter) -> (String, Vec<ParameterRef>) {
    let ParameterDefinition::Expression(Expression::Neutral(neutral)) = &parameter.definition
    else {
        panic!("{:?} is not neutral", parameter.definition);
    };
    let wire = expr::print(&neutral.0).unwrap();
    (wire.text, wire.refs)
}

fn source_tokens(parameter: &DesignParameter) -> &[ExpressionToken<ParameterRef>] {
    let ParameterDefinition::Expression(Expression::Source { tokens, .. }) = &parameter.definition
    else {
        panic!("{:?} is not source", parameter.definition);
    };
    tokens
}

fn messages(losses: &[LossNote], code: SldprtLossCode) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

fn assert_no_design_findings(ir: &CadIr) {
    let report = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
    let design = report
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Design)
        .collect::<Vec<_>>();
    assert!(design.is_empty(), "{design:#?}");
}

#[test]
fn equation_manager_lines_link_the_parameters_they_assign() {
    let (ir, losses) = decode_with(&[(
        "Contents/Config-0",
        manager(&[
            "'the boss follows its first depth",
            "\"D2@Boss\" = \"D1@Boss\" * 2 'twice",
            "\"D3@Boss\" = \"d1@BOSS\"",
            "\"D4@Boss\" = \"D1@Boss\" + 5",
            "\"D5@Boss\" = \"missing@Boss\" * 2",
            "\"D6@Boss\" = \"D1@Boss\" * 3",
            "\"nowhere@Boss\" = \"D1@Boss\"",
            "\"thickness\" = 2.5 'minted",
            "\"wall\" = \"thickness\" * 2",
            "\"label\" = \"missing\"",
        ]),
    )]);
    let d1 = parameter(&ir, "D1");

    let d2 = parameter(&ir, "D2");
    assert_eq!(
        neutral_text(d2),
        ("$0 * 2".to_owned(), vec![reference_to(d1)])
    );
    assert_eq!(d2.comment.as_deref(), Some("twice"));
    assert_eq!(
        d2.value,
        Some(ParameterValue::Number(FiniteReal::new(25.0).unwrap()))
    );

    assert_eq!(
        neutral_text(parameter(&ir, "D3")),
        ("$0".to_owned(), vec![reference_to(d1)])
    );

    assert_eq!(
        source_tokens(parameter(&ir, "D4")),
        [
            ExpressionToken::Reference(reference_to(d1)),
            ExpressionToken::Text(" + 5".into())
        ]
    );
    assert_eq!(
        source_tokens(parameter(&ir, "D5")),
        [ExpressionToken::Text("\"missing@Boss\" * 2".into())]
    );
    assert_eq!(
        source_tokens(parameter(&ir, "D6")),
        [
            ExpressionToken::Reference(reference_to(d1)),
            ExpressionToken::Text(" * 3".into())
        ],
        "a reading whose evaluation disagrees with the stored value stays source"
    );
    let thickness = parameter(&ir, "thickness");
    assert_eq!(thickness.owner, None);
    assert_eq!(thickness.kind, ValueKind::Quantity(Quantity::REAL));
    assert_eq!(neutral_text(thickness).0, "2.5");
    assert_eq!(
        thickness.value,
        Some(ParameterValue::Number(FiniteReal::new(2.5).unwrap()))
    );
    assert_eq!(thickness.comment.as_deref(), Some("minted"));
    assert_eq!(
        neutral_text(parameter(&ir, "wall")),
        ("$0 * 2".to_owned(), vec![reference_to(thickness)])
    );
    let label = parameter(&ir, "label");
    assert_eq!(label.value, None);
    assert_eq!(
        source_tokens(label),
        [ExpressionToken::Text("\"missing\"".into())]
    );

    let unread = messages(&losses, SldprtLossCode::DesignEquationUnread);
    assert!(
        unread.len() == 1 && unread[0].contains("nowhere@Boss"),
        "{unread:?}"
    );
    let kept = messages(&losses, SldprtLossCode::DesignEquationSource);
    for name in ["D4", "D5", "D6", "label"] {
        let id = parameter(&ir, name).id.as_str().to_owned();
        assert!(
            kept.iter().any(|message| message.contains(&id)),
            "{name}: {kept:?}"
        );
    }
    assert_no_design_findings(&ir);
}

#[test]
fn lines_that_form_a_cycle_or_assign_twice_are_not_read() {
    let (ir, losses) = decode_with(&[(
        "Contents/Config-0",
        manager(&[
            "\"D2@Boss\" = \"D3@Boss\" * 2",
            "\"D3@Boss\" = \"D2@Boss\" / 2",
            "\"D4@Boss\" = \"D4@Boss\" * 1",
            "\"D6@Boss\" = \"D1@Boss\" * 2",
            "\"D6@Boss\" = \"D1@Boss\" * 3",
        ]),
    )]);
    for name in ["D2", "D3", "D4", "D6"] {
        assert!(
            matches!(
                &parameter(&ir, name).definition,
                ParameterDefinition::Expression(Expression::Neutral(neutral))
                    if expr::print(&neutral.0).unwrap().refs.is_empty()
            ),
            "{name}: {:?}",
            parameter(&ir, name).definition
        );
    }
    let unread = messages(&losses, SldprtLossCode::DesignEquationUnread);
    assert_eq!(unread.len(), 2, "{unread:?}");
    assert_no_design_findings(&ir);
}

#[test]
fn several_managers_and_no_active_configuration_read_no_line() {
    let lines = ["\"D2@Boss\" = \"D1@Boss\" * 2"];
    let (ir, losses) = decode_with(&[
        ("Contents/Config-0", manager(&lines)),
        ("Contents/Config-1", manager(&lines)),
    ]);
    assert_eq!(neutral_text(parameter(&ir, "D2")).0, "25[mm]");
    assert_eq!(
        messages(&losses, SldprtLossCode::DesignEquationUnread).len(),
        1
    );
}
