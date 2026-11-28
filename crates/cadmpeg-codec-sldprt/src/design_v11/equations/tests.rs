// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{self as expr, Expression, ExpressionDialect, ExpressionToken};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::FiniteReal;

use super::line::{self, Line, Parsed, Segment};
use super::stored::Stored;
use super::translate::{self, Typing};
use super::{
    active_lines, link, walk, Linking, Manager, Managers, Names, Relation, Unread, Walk, MANAGER,
    RELATION, SIDE,
};
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
    let owning = lines
        .iter()
        .map(|line| (*line, OWNED.to_vec()))
        .collect::<Vec<_>>();
    manager_owning(&owning, after_text)
}

/// A configuration stream holding a manager of relations, each a text and
/// the bytes it owns after its side tag, followed by the six bytes
/// `after_text`.
fn manager_owning(lines: &[(&str, Vec<u8>)], after_text: [u8; 6]) -> Vec<u8> {
    let mut bytes = b"moPart_c settings before the manager".to_vec();
    bytes.extend(MANAGER);
    bytes.extend(u16::try_from(lines.len()).unwrap().to_le_bytes());
    for (index, (text, owned)) in lines.iter().enumerate() {
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
        bytes.extend(owned);
    }
    bytes.extend(b"\x00\x00detailing settings");
    bytes
}

fn manager(lines: &[&str]) -> Vec<u8> {
    manager_with(lines, [0, 2, 0, 0, 0, 0])
}

/// Relations of `lines` that store no values.
fn relations(lines: &[&str]) -> Vec<Relation> {
    lines
        .iter()
        .map(|line| Relation {
            text: (*line).to_owned(),
            stored: None,
        })
        .collect()
}

/// The texts of a walked manager's relations.
fn texts(walked: Option<Walk>) -> Option<Vec<String>> {
    match walked? {
        Walk::Lines(relations) => Some(relations.into_iter().map(|r| r.text).collect()),
        Walk::Stopped => None,
    }
}

#[test]
fn a_manager_lists_its_relation_texts_in_stored_order() {
    let lines = [
        "\"D1@Sketch1\" = \"width\" * 2",
        "'a comment",
        "\"width\" = 4",
    ];
    assert_eq!(
        texts(walk(&manager(&lines))),
        Some(lines.iter().map(|line| (*line).to_owned()).collect())
    );
    let older = manager_with(&lines, [0; 6]);
    assert_eq!(walk(&older).unwrap().lines().unwrap().len(), 3);
    let long = "\"D1@Sketch1\" = ".to_owned() + &"1 + ".repeat(80) + "1";
    assert_eq!(texts(walk(&manager(&[&long]))), Some(vec![long.clone()]));
}

/// The declarations of the classes a relation's sides hold, which the first
/// relation of a stream declares and later ones repeat by token.
const DIMENSION_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x10\x00moRelDimension_c";
const WRAPPER_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x11\x00moDimRefWrapper_c";
const OPERATOR_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x0f\x00moRelOperator_c";
const HANDLE_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x14\x00moSkDimHandleValG2_c";
const DIMENSION_TOKEN: u16 = 0x810d;
const OPERATOR_TOKEN: u16 = 0x8111;
const HANDLE_TOKEN: u16 = 0x8113;
/// The expression side's tag, a repeat of the side class.
const EXPRESSION_SIDE_TOKEN: u16 = 0x8107;

/// One token of a synthesized equation side.
enum Token<'a> {
    /// An operator or a number: its text and kind word.
    Plain(&'a str, u32),
    /// A dimension reference: its name and stored value.
    Dimension(&'a str, f64),
    /// A global-variable reference: its name and its evaluated number as
    /// text. It declares its classes, so a stream holds one.
    Global(&'a str, &'a str),
}

const GLOBAL_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x10\x00moRelGlobalVar_c";
const GLOBAL_WRAPPER_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x17\x00moGlobalVarRefWrapper_c";
const COMPONENT_GLOBAL_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x11\x00moCompGlobalVar_c";

/// A global-variable reference to `name`: its classes, its handle object,
/// the counted decimal text `number`, and its name token. It stores no f64.
fn global_reference(name: &str, number: &str) -> Vec<u8> {
    let mut bytes = GLOBAL_DECLARATION.to_vec();
    bytes.extend([0, 0]);
    bytes.extend(GLOBAL_WRAPPER_DECLARATION);
    bytes.extend(COMPONENT_GLOBAL_DECLARATION);
    bytes.extend([0x02, 0, 0, 0]);
    bytes.extend(counted_utf16(number));
    bytes.extend(counted_utf16(&format!("\"{name}\"")));
    bytes.extend(words(1, 0));
    bytes
}

/// Four token words: the lead word, a character range, and a kind.
fn words(lead: u32, kind: u32) -> Vec<u8> {
    [lead, 0, 1, kind]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect()
}

/// A dimension reference holding `value`, its handle objects, and its name
/// token. The first reference declares its classes; its handle carries a
/// counted string with words and a stray repeat token that are no name.
fn dimension_reference(first: &mut bool, name: &str, value: f64) -> Vec<u8> {
    let mut bytes = Vec::new();
    if std::mem::take(first) {
        bytes.extend(DIMENSION_DECLARATION);
        bytes.extend([0; 4]);
        bytes.extend(WRAPPER_DECLARATION);
        bytes.extend(value.to_le_bytes());
        bytes.extend(HANDLE_DECLARATION);
        bytes.extend([0x2d, 0x80, 0x02, 0, 0, 0, 0, 0]);
        bytes.extend(counted_utf16("Point1@Origin"));
        bytes.extend(words(4, 0));
        bytes.extend([0x01, 0x81, 0, 0, 0, 0, 0x03, 0x81]);
        bytes.extend(2.0e-307f64.to_le_bytes());
    } else {
        bytes.extend(DIMENSION_TOKEN.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend((DIMENSION_TOKEN + 2).to_le_bytes());
        bytes.extend(value.to_le_bytes());
        bytes.extend(HANDLE_TOKEN.to_le_bytes());
        bytes.extend([0x02, 0, 0, 0, 0, 0]);
    }
    bytes.extend(counted_utf16(&format!("\"{name}\"")));
    bytes.extend(words(1, 0));
    bytes
}

/// The bytes a relation owns after its first side's tag: the assigned side
/// with the dimension `assigned` and its value, the assigned name repeated,
/// and the expression side of `tokens` after an `=` token. `first` holds
/// whether the stream has declared the side classes yet.
fn sides(first: &mut bool, assigned: (&str, f64), tokens: &[Token<'_>]) -> Vec<u8> {
    let declare = *first;
    let mut bytes = 1u16.to_le_bytes().to_vec();
    bytes.extend(dimension_reference(first, assigned.0, assigned.1));
    bytes.extend(counted_utf16(&format!("\"{}\"", assigned.0)));
    bytes.extend([0, 0, 0, 0, 1, 0, 0, 0, 0x5a, 0xa5]);
    bytes.extend(EXPRESSION_SIDE_TOKEN.to_le_bytes());
    bytes.extend(u16::try_from(tokens.len() + 1).unwrap().to_le_bytes());
    let plain = |bytes: &mut Vec<u8>, declare: &mut bool, text: &str, kind: u32| {
        if std::mem::take(declare) {
            bytes.extend(OPERATOR_DECLARATION);
        } else {
            bytes.extend(OPERATOR_TOKEN.to_le_bytes());
        }
        bytes.extend(counted_utf16(text));
        bytes.extend(words(8, kind));
    };
    let mut declare_operator = declare;
    plain(&mut bytes, &mut declare_operator, "=", 3);
    for token in tokens {
        match token {
            Token::Plain(text, kind) => plain(&mut bytes, &mut declare_operator, text, *kind),
            Token::Dimension(name, value) => {
                bytes.extend(dimension_reference(first, name, *value));
            }
            Token::Global(name, number) => bytes.extend(global_reference(name, number)),
        }
    }
    bytes.extend(counted_utf16(" = expression text"));
    bytes
}

/// The stored values of each relation of a walked manager.
fn stored_of(walked: Option<Walk>) -> Vec<Option<Stored>> {
    match walked.unwrap() {
        Walk::Lines(relations) => relations.into_iter().map(|r| r.stored).collect(),
        Walk::Stopped => panic!("stopped"),
    }
}

/// A manager whose lines drive `D2@Boss` and `D3@Boss` from `D1@Boss`, a
/// line whose sides hold no token, and a line whose name token names another
/// dimension.
fn manager_with_sides(after_text: [u8; 6]) -> Vec<u8> {
    let mut first = true;
    let half = sides(
        &mut first,
        ("D2@Boss", 0.011),
        &[
            Token::Dimension("D1@Boss", 0.022),
            Token::Plain("/", 3),
            Token::Plain("2", 2),
        ],
    );
    let double = sides(
        &mut first,
        ("D3@Boss", 0.044),
        &[
            Token::Plain("2", 2),
            Token::Plain("*", 3),
            Token::Dimension("d1@BOSS", 0.022),
        ],
    );
    let other = sides(
        &mut first,
        ("D9@Boss", 0.03),
        &[Token::Dimension("D1@Boss", 0.022)],
    );
    manager_owning(
        &[
            ("\"D2@Boss\" = \"D1@Boss\" / 2", half),
            ("\"D3@Boss\" = 2 * \"d1@BOSS\" 'twice", double),
            ("\"D4@Boss\" = \"D1@Boss\"", vec![0, 0, 0xaa, 0xbb]),
            ("\"D5@Boss\" = \"D1@Boss\"", other),
        ],
        after_text,
    )
}

#[test]
fn a_relation_stores_the_value_beside_each_dimension_it_names() {
    assert_eq!(
        stored_of(walk(&manager_with_sides([0, 2, 0, 0, 0, 0]))),
        [
            Some(Stored {
                assigned: Some(0.011),
                reads: vec![Some(0.022)]
            }),
            Some(Stored {
                assigned: Some(0.044),
                reads: vec![Some(0.022)]
            }),
            None,
            None,
        ]
    );
    assert_eq!(
        stored_of(walk(&manager_with_sides([0, 0, 0, 0, 0, 0]))),
        [None, None, None, None],
        "the older layout stores no value"
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
        position: None,
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
        &names,
        &Typing {
            parameters: &parameters,
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
    decode_keywords(KEYWORDS, streams)
}

fn decode_keywords(keywords: &[u8], streams: &[(&str, Vec<u8>)]) -> (CadIr, Vec<LossNote>) {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
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

fn design_findings(ir: &CadIr) -> Vec<Finding> {
    cadmpeg_ir::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .collect()
}

fn assert_no_design_findings(ir: &CadIr) {
    let design = design_findings(ir);
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
        "a reading whose evaluation disagrees with the decoded value stays source"
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
fn lines_on_a_cycle_are_stated_and_lines_that_assign_twice_are_not_read() {
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
    let (d2, d3, d4) = (
        parameter(&ir, "D2"),
        parameter(&ir, "D3"),
        parameter(&ir, "D4"),
    );
    assert_eq!(
        neutral_text(d2),
        ("$0 * 2".to_owned(), vec![reference_to(d3)])
    );
    assert_eq!(
        neutral_text(d3),
        ("$0 / 2".to_owned(), vec![reference_to(d2)])
    );
    assert_eq!(
        neutral_text(d4),
        ("$0 * 1".to_owned(), vec![reference_to(d4)])
    );
    assert!(neutral_text(parameter(&ir, "D6")).1.is_empty());
    let unread = messages(&losses, SldprtLossCode::DesignEquationUnread);
    assert!(
        unread.len() == 1 && unread[0].contains("D6@Boss"),
        "{unread:?}"
    );
    let design = design_findings(&ir);
    assert!(
        design.len() == 2
            && design
                .iter()
                .all(|finding| finding.severity == Severity::Warning
                    && finding.message.contains("cycle")),
        "{design:#?}"
    );
}

#[test]
fn a_bare_link_whose_caches_disagree_is_neutral() {
    let (ir, losses) =
        decode_with(&[("Contents/Config-0", manager(&["\"D4@Boss\" = \"D1@Boss\""]))]);
    let d4 = parameter(&ir, "D4");
    assert_eq!(
        neutral_text(d4),
        ("$0".to_owned(), vec![reference_to(parameter(&ir, "D1"))])
    );
    assert!(losses
        .iter()
        .all(|loss| loss.code != SldprtLossCode::DesignEquationSource.kind()));
    let design = design_findings(&ir);
    assert!(
        !design.is_empty()
            && design
                .iter()
                .all(|finding| finding.severity == Severity::Warning
                    && finding.entity.as_deref() == Some(d4.id.as_str())),
        "{design:#?}"
    );
}

/// A parameter of the scope with a cache and an unread definition.
fn valued(name: &str, kind: ValueKind, value: ParameterValue) -> DesignParameter {
    DesignParameter {
        definition: ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::SolidWorks,
            tokens: Vec::new(),
        }),
        value: Some(value),
        ..global(name, kind)
    }
}

fn number(value: f64) -> ParameterValue {
    ParameterValue::Number(FiniteReal::new(value).unwrap())
}

/// `lines`, the one manager's, read into `parameters`.
fn linked(parameters: &mut Vec<DesignParameter>, lines: &[&str]) -> Vec<LossNote> {
    linked_relations(parameters, relations(lines))
}

/// The one manager's `relations` read into `parameters`.
fn linked_relations(
    parameters: &mut Vec<DesignParameter>,
    relations: Vec<Relation>,
) -> Vec<LossNote> {
    let managers = Managers(vec![Manager {
        slot: 0,
        lines: Walk::Lines(relations),
    }]);
    link(
        parameters,
        &Linking {
            managers: &managers,
            scope: &super::super::root_definition(),
            active: None,
            features: &[],
            length_unit: None,
            tolerances: &CadIr::empty().tolerances,
        },
    )
}

#[test]
fn an_integer_parameter_takes_the_rounded_line_unless_it_links_an_integer() {
    let real = ValueKind::Quantity(Quantity::REAL);
    let mut parameters = vec![
        valued("n", ValueKind::Integer, ParameterValue::Integer(8)),
        valued("k", ValueKind::Integer, ParameterValue::Integer(2)),
        valued("c", ValueKind::Integer, ParameterValue::Integer(5)),
        valued("m", ValueKind::Integer, ParameterValue::Integer(5)),
        valued("W", real.clone(), number(2.5)),
        valued("r", real, number(4.0)),
    ];
    let losses = linked(
        &mut parameters,
        &[
            "\"n\" = \"W\" * 3",
            "\"k\" = \"W\"",
            "\"c\" = \"m\"",
            "\"r\" = \"m\"",
        ],
    );
    let [rounded, disagreeing, count_link, count, width, real_link] = &parameters[..] else {
        panic!("{parameters:#?}");
    };
    assert_eq!(
        neutral_text(rounded),
        ("round($0 * 3)".to_owned(), vec![reference_to(width)])
    );
    assert_eq!(
        source_tokens(disagreeing),
        [ExpressionToken::Text("\"W\"".into())],
        "round(2.5) is 3, not the stored 2"
    );
    assert_eq!(
        neutral_text(count_link),
        ("$0".to_owned(), vec![reference_to(count)])
    );
    assert_eq!(
        neutral_text(real_link),
        ("$0".to_owned(), vec![reference_to(count)])
    );
    let kept = messages(&losses, SldprtLossCode::DesignEquationSource);
    assert!(
        kept.len() == 1 && kept[0].contains(disagreeing.id.as_str()),
        "{kept:?}"
    );
}

fn managers(stored: &[(usize, Option<&[&str]>)]) -> Managers {
    Managers(
        stored
            .iter()
            .map(|(slot, lines)| Manager {
                slot: *slot,
                lines: lines.map_or(Walk::Stopped, |lines| Walk::Lines(relations(lines))),
            })
            .collect(),
    )
}

fn outcome(
    managers: &Managers,
    active: Option<usize>,
) -> Result<Vec<Result<String, Unread>>, Unread> {
    active_lines(managers, active).map(|lines| {
        lines
            .into_iter()
            .map(|line| line.map(|(line, _)| line.name).map_err(|(why, _)| why))
            .collect()
    })
}

#[test]
fn the_active_manager_is_read_where_every_other_manager_with_lines_holds_its_line() {
    let active: &[&str] = &[
        "'a comment",
        "\"A\" = \"D1@Boss\" * 2",
        "\"B\" = 3",
        "\"C\" = 4",
    ];
    let other: &[&str] = &["\"C\" = 4", "\"A\" = \"D1@Boss\" * 2", "\"B\" = 5"];
    let empty: &[&str] = &[];
    let stored = managers(&[(0, Some(other)), (2, Some(active)), (3, Some(empty))]);
    assert_eq!(
        outcome(&stored, Some(2)),
        Ok(vec![
            Ok("A".to_owned()),
            Err(Unread::ConfigurationsDiffer),
            Ok("C".to_owned()),
        ])
    );
    let only_a: &[&str] = &["\"A\" = \"D1@Boss\" * 2"];
    let lacking = managers(&[(0, Some(only_a)), (2, Some(active))]);
    assert_eq!(
        outcome(&lacking, Some(2)),
        Ok(vec![
            Ok("A".to_owned()),
            Err(Unread::ConfigurationsDiffer),
            Err(Unread::ConfigurationsDiffer),
        ])
    );
}

#[test]
fn several_managers_read_no_line_where_one_of_them_is_not_read_or_none_is_active() {
    let lines: &[&str] = &["\"A\" = 1", "\"B\" = 2"];
    assert_eq!(
        outcome(&managers(&[(0, None), (1, Some(lines))]), Some(1)),
        Ok(vec![
            Err(Unread::OtherManagerUnwalked),
            Err(Unread::OtherManagerUnwalked),
        ])
    );
    assert_eq!(
        outcome(&managers(&[(0, Some(lines)), (1, None)]), Some(1)),
        Err(Unread::ManagerUnwalked)
    );
    let both = managers(&[(0, Some(lines)), (1, Some(lines))]);
    assert_eq!(outcome(&both, Some(4)), Err(Unread::NoActiveStream));
    assert_eq!(outcome(&both, None), Err(Unread::NoActiveStream));
    let empty: &[&str] = &[];
    assert_eq!(
        outcome(&managers(&[(0, Some(empty)), (1, Some(empty))]), None),
        Ok(Vec::new())
    );
}

#[test]
fn a_part_that_states_no_design_names_its_equation_lines() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Config-0",
        &manager(&["'a comment", "\"D1@Boss\" = 2", "\"width\" = 4"]),
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().design.parameters.is_empty());
    let unread = messages(
        &result.report().losses,
        SldprtLossCode::DesignEquationUnread,
    );
    assert!(
        unread.len() == 1
            && unread[0].starts_with("2 equation-manager lines")
            && unread[0].contains("the part states no design")
            && unread[0].contains("D1@Boss")
            && unread[0].contains("width"),
        "{unread:?}"
    );
}

#[test]
fn a_part_with_sketches_and_no_history_reads_its_equation_lines() {
    let (mut ir, _) = crate::design_v11::tests::sketch_and_extrude();
    let losses = crate::design_v11::tests::outside_history::project_with(
        &mut ir,
        &[],
        &[],
        managers(&[(0, Some(&["\"width\" = 4"]))]),
    );
    let width = ir
        .design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .is_some_and(|name| name.as_str() == "width")
        })
        .unwrap_or_else(|| panic!("a parameter width: {:#?}", ir.design.parameters));
    assert!(matches!(
        width.value,
        Some(ParameterValue::Number(value)) if value.get() == 4.0
    ));
    let unread = messages(&losses, SldprtLossCode::DesignEquationUnread);
    assert!(
        unread
            .iter()
            .all(|message| !message.contains("the part states no design")),
        "{unread:?}"
    );
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

#[test]
fn a_tree_over_values_the_manager_stores_otherwise_is_source() {
    let length = ValueKind::Quantity(Quantity::LENGTH);
    let rounded = || {
        vec![
            valued("D1", length.clone(), number(0.43)),
            valued("D2", length.clone(), number(0.86)),
        ]
    };
    let line = "\"D2\" = \"D1\" * 2";
    let with = |read: f64| {
        vec![Relation {
            text: line.to_owned(),
            stored: Some(Stored {
                assigned: Some(0.000_857_2),
                reads: vec![Some(read)],
            }),
        }]
    };

    let mut parameters = rounded();
    assert!(linked(&mut parameters, &[line]).is_empty());
    assert_eq!(
        neutral_text(&parameters[1]),
        ("$0 * 2".to_owned(), vec![reference_to(&parameters[0])]),
        "with no stored value the rounded displays agree"
    );

    let mut parameters = rounded();
    let losses = linked_relations(&mut parameters, with(0.000_428_6));
    assert_eq!(
        source_tokens(&parameters[1]),
        [
            ExpressionToken::Reference(reference_to(&parameters[0])),
            ExpressionToken::Text(" * 2".into())
        ]
    );
    let kept = messages(&losses, SldprtLossCode::DesignEquationSource);
    assert!(
        kept.len() == 1
            && kept[0].contains("stores beside it")
            && kept[0].contains("writes their cached value")
            && kept[0].contains(parameters[1].id.as_str()),
        "{kept:?}"
    );

    let mut parameters = rounded();
    assert!(linked_relations(&mut parameters, with(0.000_43)).is_empty());
    assert_eq!(neutral_text(&parameters[1]).0, "$0 * 2");

    let mut parameters = rounded();
    parameters[1].value = None;
    let losses = linked_relations(&mut parameters, with(0.000_43));
    let kept = messages(&losses, SldprtLossCode::DesignEquationSource);
    assert!(
        kept.len() == 1
            && kept[0].contains("hold no cached value")
            && !kept[0].contains("writes their cached value"),
        "a parameter with no value has none for a writer: {kept:?}"
    );
}

const UNREAD_KIND_KEYWORDS: &[u8] = br#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Dimension Name="D1">12.5mm</Dimension>
        <Dimension Name="Width">44</Dimension>
        <Dimension Name="Half">22</Dimension>
        <Dimension Name="Count">3</Dimension>
    </Extrusion>
</Keywords>"#;

/// The length unit codes of a units table.
const MILLIMETRE: u16 = 0;
const INCH: u16 = 3;

/// The length precision of a part whose displays the tests take as exact.
const EXACT_PLACES: u32 = 8;

/// A configuration stream's units table stating the length unit `code` and
/// the length precision `places`, in the block container's layout: the
/// length unit, then a number unit whose object is one base long, then the
/// next class declaration.
fn units_table(code: u16, places: u32) -> Vec<u8> {
    let mut base = vec![0xaa; 62];
    base[..4].copy_from_slice(&places.to_le_bytes());
    let mut bytes = b"\xff\xff\x01\x00\x0e\x00moUnitsTable_c".to_vec();
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"\xff\xff\x01\x00\x13\x00moLengthUserUnits_c");
    bytes.extend(&base);
    bytes.extend(code.to_le_bytes());
    bytes.extend([0, 0]);
    bytes.extend(b"\xff\xff\x01\x00\x13\x00moNumberUserUnits_c");
    bytes.extend(&base);
    bytes.extend(b"\xff\xff\x01\x00\x10\x00moDensityUnits_c");
    bytes
}

/// The part of [`UNREAD_KIND_KEYWORDS`] with a manager whose lines drive
/// `Half` from `Width` and `Count` from both, each relation followed by the
/// six bytes `after_text`.
fn decode_unread_kinds(after_text: [u8; 6]) -> (CadIr, Vec<LossNote>) {
    let mut first = true;
    let half = sides(
        &mut first,
        ("Half@Boss", 0.022),
        &[
            Token::Dimension("Width@Boss", 0.044),
            Token::Plain("/", 3),
            Token::Plain("2", 2),
        ],
    );
    let count = sides(
        &mut first,
        ("Count@Boss", 3.0),
        &[
            Token::Dimension("Width@Boss", 0.044),
            Token::Plain("/", 3),
            Token::Dimension("Half@Boss", 0.022),
            Token::Plain("+", 3),
            Token::Plain("1", 2),
        ],
    );
    let mut stream = units_table(MILLIMETRE, EXACT_PLACES);
    stream.extend(manager_owning(
        &[
            ("\"Half@Boss\" = \"Width@Boss\" / 2", half),
            ("\"Count@Boss\" = \"Width@Boss\" / \"Half@Boss\" + 1", count),
        ],
        after_text,
    ));
    decode_keywords(UNREAD_KIND_KEYWORDS, &[("Contents/Config-0", stream)])
}

#[test]
fn the_manager_types_a_dimension_of_unread_kind_as_the_length_it_stores() {
    let (ir, losses) = decode_unread_kinds([0, 2, 0, 0, 0, 0]);
    let length = ValueKind::Quantity(Quantity::LENGTH);
    let width = parameter(&ir, "Width");
    assert_eq!(width.kind, length);
    assert_eq!(width.value, Some(number(44.0)));
    assert_eq!(neutral_text(width).0, "44[mm]");
    let half = parameter(&ir, "Half");
    assert_eq!(half.kind, length);
    assert_eq!(half.value, Some(number(22.0)));
    assert_eq!(
        neutral_text(half),
        ("$0 / 2".to_owned(), vec![reference_to(width)])
    );
    let count = parameter(&ir, "Count");
    assert!(
        matches!(count.kind, ValueKind::Native(_)),
        "a count stores its own number: {:?}",
        count.kind
    );
    let kept = messages(&losses, SldprtLossCode::DesignEquationSource);
    assert!(
        kept.len() == 1 && kept[0].contains(count.id.as_str()),
        "{kept:?}"
    );
    let unvalued = messages(&losses, SldprtLossCode::DesignParameterValueUnread);
    for name in ["Width", "Half"] {
        let id = parameter(&ir, name).id.as_str().to_owned();
        assert!(
            unvalued.iter().all(|message| !message.contains(&id)),
            "{name}: {unvalued:?}"
        );
    }
    assert_no_design_findings(&ir);

    let (ir, _) = decode_unread_kinds([0; 6]);
    assert!(
        matches!(parameter(&ir, "Width").kind, ValueKind::Native(_)),
        "the older layout stores no value to type it"
    );
    assert!(matches!(
        parameter(&ir, "Half").definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn a_global_variable_in_the_expression_stores_no_value_beside_it() {
    let mut first = true;
    let owned = sides(
        &mut first,
        ("D2@Boss", 0.047),
        &[
            Token::Global("width", "12.5"),
            Token::Plain("*", 3),
            Token::Plain("2", 2),
            Token::Plain("+", 3),
            Token::Dimension("D1@Boss", 0.022),
        ],
    );
    let walked = walk(&manager_owning(
        &[("\"D2@Boss\" = \"width\" * 2 + \"D1@Boss\"", owned)],
        [0, 2, 0, 0, 0, 0],
    ));
    assert_eq!(
        stored_of(walked),
        [Some(Stored {
            assigned: Some(0.047),
            reads: vec![None, Some(0.022)]
        })]
    );
}

/// A part of `keywords` whose length unit is `unit` and whose length
/// precision is `places`, with a manager of `relations` in the layout that
/// stores values.
fn decode_relations(
    keywords: &[u8],
    unit: u16,
    places: u32,
    relations: &[(&str, Vec<u8>)],
) -> (CadIr, Vec<LossNote>) {
    let mut stream = units_table(unit, places);
    stream.extend(manager_owning(relations, [0, 2, 0, 0, 0, 0]));
    decode_keywords(keywords, &[("Contents/Config-0", stream)])
}

/// Keywords of a boss whose dimensions `Lead` and `Half` show the bare
/// numbers `lead` and `half`, and whose `Quarter` shows `quarter`.
fn boss_keywords(lead: &str, half: &str, quarter: &str) -> Vec<u8> {
    format!(
        r#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Dimension Name="Lead">{lead}</Dimension>
        <Dimension Name="Half">{half}</Dimension>
        <Dimension Name="Quarter">{quarter}</Dimension>
    </Extrusion>
</Keywords>"#
    )
    .into_bytes()
}

/// The relation `"Half@Boss" = "Lead@Boss" / 2` storing `lead` and `half`.
fn half_of_lead(first: &mut bool, lead: f64, half: f64) -> (&'static str, Vec<u8>) {
    (
        "\"Half@Boss\" = \"Lead@Boss\" / 2",
        sides(
            first,
            ("Half@Boss", half),
            &[
                Token::Dimension("Lead@Boss", lead),
                Token::Plain("/", 3),
                Token::Plain("2", 2),
            ],
        ),
    )
}

fn is_untyped(ir: &CadIr, name: &str) -> bool {
    matches!(parameter(ir, name).kind, ValueKind::Native(_))
}

#[test]
fn an_angle_the_manager_stores_in_radians_stays_untyped() {
    let degree = std::f64::consts::PI / 180.0;
    let mut first = true;
    let (ir, _) = decode_relations(
        &boss_keywords("30", "15", "1"),
        MILLIMETRE,
        EXACT_PLACES,
        &[half_of_lead(&mut first, 30.0 * degree, 15.0 * degree)],
    );
    for name in ["Lead", "Half"] {
        assert!(is_untyped(&ir, name), "{name}: {:?}", parameter(&ir, name));
    }
    assert!(matches!(
        parameter(&ir, "Half").definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn the_manager_types_a_length_in_a_part_in_inches() {
    let mut first = true;
    let (ir, _) = decode_relations(
        &boss_keywords("1.75", "0.875", "1"),
        INCH,
        EXACT_PLACES,
        &[half_of_lead(&mut first, 0.044_45, 0.022_225)],
    );
    let length = ValueKind::Quantity(Quantity::LENGTH);
    let millimetres = |name: &str| {
        let found = parameter(&ir, name);
        assert_eq!(found.kind, length, "{name}");
        match &found.value {
            Some(ParameterValue::Number(value)) => value.get(),
            other => panic!("{name}: {other:?}"),
        }
    };
    assert!((millimetres("Lead") - 44.45).abs() < 1e-9);
    assert!((millimetres("Half") - 22.225).abs() < 1e-9);
    let lead = parameter(&ir, "Lead");
    assert_eq!(
        neutral_text(parameter(&ir, "Half")),
        ("$0 / 2".to_owned(), vec![reference_to(lead)])
    );
}

#[test]
fn reading_lines_that_store_different_values_give_none() {
    let mut first = true;
    let half = half_of_lead(&mut first, 0.044, 0.022);
    let quarter = sides(
        &mut first,
        ("Quarter@Boss", 0.011),
        &[
            Token::Dimension("Lead@Boss", 0.045),
            Token::Plain("/", 3),
            Token::Plain("4", 2),
        ],
    );
    let eighth = sides(
        &mut first,
        ("Eighth@Boss", 0.0055),
        &[
            Token::Dimension("Lead@Boss", 0.044),
            Token::Plain("/", 3),
            Token::Plain("8", 2),
        ],
    );
    let keywords = br#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Dimension Name="Lead">44</Dimension>
        <Dimension Name="Half">22</Dimension>
        <Dimension Name="Quarter">11</Dimension>
        <Dimension Name="Eighth">5.5</Dimension>
    </Extrusion>
</Keywords>"#;
    let (ir, _) = decode_relations(
        keywords,
        MILLIMETRE,
        EXACT_PLACES,
        &[
            half,
            ("\"Quarter@Boss\" = \"Lead@Boss\" / 4", quarter),
            ("\"Eighth@Boss\" = \"Lead@Boss\" / 8", eighth),
        ],
    );
    assert!(
        is_untyped(&ir, "Lead"),
        "lines that store different values beside Lead type it with none of them, the first \
         or the last: {:?}",
        parameter(&ir, "Lead")
    );
    for name in ["Half", "Quarter", "Eighth"] {
        assert!(
            matches!(
                parameter(&ir, name).definition,
                ParameterDefinition::Expression(Expression::Source { .. })
            ),
            "{name}"
        );
    }
}

/// A part in the length unit `unit` at the length precision `places`, whose
/// boss shows `lead.0` as `Lead`,
/// `target.1` as the dimension named `target.0`, and `quarter` as `Quarter`,
/// the target driven by `"<target>@Boss" = "Lead@Boss" / 3`, the relation
/// storing `lead.1` metres beside `Lead` and `third` metres beside the target.
fn decode_third(
    unit: u16,
    places: u32,
    lead: (&str, f64),
    target: (&str, &str),
    quarter: &str,
    third: f64,
) -> (CadIr, Vec<LossNote>) {
    let mut first = true;
    let assigned = format!("{}@Boss", target.0);
    let owned = sides(
        &mut first,
        (&assigned, third),
        &[
            Token::Dimension("Lead@Boss", lead.1),
            Token::Plain("/", 3),
            Token::Plain("3", 2),
        ],
    );
    let keywords = format!(
        r#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Dimension Name="Lead">{}</Dimension>
        <Dimension Name="{}">{}</Dimension>
        <Dimension Name="Quarter">{quarter}</Dimension>
    </Extrusion>
</Keywords>"#,
        lead.0, target.0, target.1
    );
    let line = format!("\"{assigned}\" = \"Lead@Boss\" / 3");
    decode_relations(keywords.as_bytes(), unit, places, &[(&line, owned)])
}

/// Asserts that `name` is the length `millimetres` with the formula
/// `Lead / 3`, and that no note names it.
fn assert_takes_its_line(ir: &CadIr, losses: &[LossNote], name: &str, millimetres: f64) {
    let target = parameter(ir, name);
    assert_eq!(target.kind, ValueKind::Quantity(Quantity::LENGTH));
    match &target.value {
        Some(ParameterValue::Number(value)) => {
            assert!((value.get() - millimetres).abs() < 1e-9, "{value:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        neutral_text(target),
        (
            "$0 / 3".to_owned(),
            vec![reference_to(parameter(ir, "Lead"))]
        )
    );
    assert!(messages(losses, SldprtLossCode::DesignEquationSource).is_empty());
    for code in [
        SldprtLossCode::DesignParameterValueUnread,
        SldprtLossCode::DesignParameterDisplayRounded,
    ] {
        let named = messages(losses, code);
        assert!(
            named
                .iter()
                .all(|message| !message.contains(target.id.as_str())),
            "{named:?}"
        );
    }
    assert_no_design_findings(ir);
}

#[test]
fn a_display_that_rounds_the_length_its_line_gives_takes_the_stored_length() {
    let lead = ("1.3", 0.0013);
    let third = 0.0013 / 3.0;
    let (ir, losses) = decode_third(MILLIMETRE, 2, lead, ("Half", "0.43"), "2.5", third);
    assert_takes_its_line(&ir, &losses, "Half", 1.3 / 3.0);

    let (ir, losses) = decode_third(
        MILLIMETRE,
        3,
        ("1.315", 0.001_315),
        ("Half", "0.44"),
        "2.5",
        0.001_315 / 3.0,
    );
    assert_eq!(
        parameter(&ir, "Lead").kind,
        ValueKind::Quantity(Quantity::LENGTH)
    );
    let half = parameter(&ir, "Half");
    assert!(
        is_untyped(&ir, "Half"),
        "a display coarser than the length precision: {half:?}"
    );
    assert!(matches!(
        half.definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
    let unvalued = messages(&losses, SldprtLossCode::DesignParameterValueUnread);
    assert!(
        unvalued.len() == 1
            && unvalued[0].contains("than the document's length precision")
            && unvalued[0].contains(half.id.as_str()),
        "{unvalued:?}"
    );

    let (ir, losses) = decode_third(MILLIMETRE, 2, lead, ("Half", "0.43"), "2.5", 0.000_432);
    let half = parameter(&ir, "Half");
    assert!(
        is_untyped(&ir, "Half"),
        "a stored value the line does not give: {half:?}"
    );
    let unvalued = messages(&losses, SldprtLossCode::DesignParameterValueUnread);
    assert!(
        unvalued
            .iter()
            .all(|message| !message.contains("length precision")),
        "{unvalued:?}"
    );
}

#[test]
fn a_length_that_holds_its_rounded_display_takes_the_length_its_line_gives() {
    let lead = ("1.3", 0.0013);
    let (ir, losses) = decode_third(MILLIMETRE, 2, lead, ("D1", "0.43"), "2.5", 0.0013 / 3.0);
    assert_takes_its_line(&ir, &losses, "D1", 1.3 / 3.0);

    let (ir, losses) = decode_third(MILLIMETRE, 2, lead, ("D1", "0.43"), "2.5", 0.000_432);
    let depth = parameter(&ir, "D1");
    assert_eq!(depth.kind, ValueKind::Quantity(Quantity::LENGTH));
    match &depth.value {
        Some(ParameterValue::Number(value)) => assert!((value.get() - 0.43).abs() < 1e-12),
        other => panic!("a stored value the line does not give: {other:?}"),
    }
    let rounded = messages(&losses, SldprtLossCode::DesignParameterDisplayRounded);
    assert!(
        rounded
            .iter()
            .any(|message| message.contains(depth.id.as_str())),
        "{rounded:?}"
    );
}

#[test]
fn a_part_in_inches_takes_the_length_its_line_gives_where_the_display_rounds_inches() {
    let lead = 1.3 * 0.0254;
    let (ir, losses) = decode_third(INCH, 2, ("1.3", lead), ("Half", "0.43"), "2.5", lead / 3.0);
    assert_takes_its_line(&ir, &losses, "Half", 1.3 * 25.4 / 3.0);

    let (ir, _) = decode_third(INCH, 2, ("1.3", lead), ("Half", "11.01"), "2.5", lead / 3.0);
    let half = parameter(&ir, "Half");
    assert!(
        is_untyped(&ir, "Half"),
        "a display of the millimetres in a part in inches: {half:?}"
    );
}
