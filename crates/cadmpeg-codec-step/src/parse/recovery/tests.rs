// SPDX-License-Identifier: Apache-2.0
//! Record admission, dependency closure, and exact omitted-source retention.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::record_unreadable_kind;

use crate::loss::StepLossCode;
use crate::parse::{parse_inner, ParseDiagnosticKind, Value};
use crate::test_support::{with_policy_context, with_service_context};
use crate::StepCodec;

fn exchange(data: &str) -> String {
    format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;")
}

#[test]
fn bounded_defects_preserve_independent_instances_and_exact_bytes() {
    for defective in [
        "#2=ITEM(#0);",
        "#0=ITEM();",
        "#2=ITEM(1.E999);",
        "#2=ITEM(%,3);",
        "#2=ITEM('literal;semicolon', /* ; */ ,3);",
        "#2=ITEM('literal'\n';semicolon',,3);",
        "#2=ITEM_ENDSEC;",
        "#2=ENDSEC\n_NAME;",
        "#2=(A()A());",
        "#2=ITEM(#12BROKEN(3));",
    ] {
        let source = exchange(&format!(
            "#1=CARTESIAN_POINT('',(1.,2.,3.));{defective}#3=CARTESIAN_POINT('',(4.,5.,6.));"
        ));
        let (parsed, diagnostics) = with_service_context(source.as_bytes(), parse_inner)
            .expect("explicit statement boundaries permit recovery");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [1, 3]);
        assert!(parsed.omitted_spans().is_empty());
        let [statement] = parsed.given_up() else {
            panic!("one given-up statement for {defective}");
        };
        assert_eq!(statement.kind, "data_record");
        assert_eq!(&source[statement.span.clone()], defective);
        assert!(statement.dropped.is_empty());
        assert!(!diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::RecordOmitted));
        let decoded_source = source.replace(
            "ENDSEC;END-ISO",
            "#4=GEOMETRIC_SET('',(#1,#3));ENDSEC;END-ISO",
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(&decoded_source), &DecodeOptions::default())
            .expect("independent points decode");
        assert_eq!(decoded.ir().geometry.points.len(), 2);
        let losses = given_up_losses(decoded.report());
        let [loss] = losses.as_slice() else {
            panic!("one record.unreadable loss for {defective}");
        };
        assert!(loss.message.starts_with(&format!(
            "part data_record stream=\"\" offset={} length={} record=step:source:part#",
            statement.span.start,
            defective.len()
        )));
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::ParseRecordOmitted.kind()));
        let (ir, _, fidelity) = decoded.into_parts();
        let unknowns = ir.native_unknowns("step").expect("retained source");
        let stub = unknowns
            .iter()
            .find(|record| record.id.as_str().starts_with("step:source:part#"))
            .expect("given-up statement stub");
        assert!(stub.links.is_empty());
        assert_eq!(
            fidelity
                .retained_record(stub.id.as_str())
                .expect("fidelity payload")
                .data(),
            Some(defective.as_bytes())
        );
        assert_eq!(
            ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
            "0"
        );
    }
}

fn given_up_losses(report: &DecodeReport) -> Vec<&LossNote> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .collect()
}

#[test]
fn a_given_up_statement_names_the_records_that_need_it() {
    let source = exchange(
        "#1=CARTESIAN_POINT('',(1.,2.,3.));#2=ITEM(%,3);#3=ITEM(#2);#4=ITEM(#3);#5=ITEM(#9);#6=GEOMETRIC_SET('',(#1,#3));",
    );
    let dependent = source.find("#3=").expect("first dependent");
    let transitive = source.find("#4=").expect("transitive dependent");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("the set keeps its readable entry");
    assert_eq!(decoded.ir().geometry.points.len(), 1);
    let losses = given_up_losses(decoded.report());
    let [loss] = losses.as_slice() else {
        panic!("one record.unreadable loss");
    };
    assert!(loss.message.ends_with(&format!(
        "; dropped=[step:file:omitted#{dependent}, step:file:omitted#{transitive}]"
    )));
    let omissions = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ParseRecordOmitted.kind())
        .collect::<Vec<_>>();
    let [omission] = omissions.as_slice() else {
        panic!("only the record with an unrelated missing reference is omitted");
    };
    assert!(omission.message.contains("instance #5"));
    let (_, _, fidelity) = decoded.into_parts();
    for (offset, text) in [(dependent, "#3=ITEM(#2);"), (transitive, "#4=ITEM(#3);")] {
        assert_eq!(
            fidelity
                .retained_record(&format!("step:file:omitted#{offset}"))
                .expect("dropped record keeps its source")
                .data(),
            Some(text.as_bytes())
        );
    }
}

#[test]
fn a_record_that_lists_a_given_up_statement_loses_only_those_entries() {
    let source = exchange(
        "#1=ITEM();#2=ITEM(%,3);#3=ITEM(#2);#4=ITEM((#1,#3,LABEL(#2)),((#2),#1));#5=ITEM((#4));",
    );
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("bounded graph");
    assert_eq!(
        parsed.records().keys().copied().collect::<Vec<_>>(),
        [1, 4, 5]
    );
    assert_eq!(
        parsed.records()[&4].partials[0]
            .parameters
            .iter()
            .collect::<Vec<_>>(),
        [
            &Value::List(vec![Value::Reference(1)]),
            &Value::List(vec![Value::List(Vec::new()), Value::Reference(1)]),
        ]
    );
    let [statement] = parsed.given_up() else {
        panic!("one given-up statement");
    };
    assert_eq!(
        statement.dropped,
        [source.find("#3=").expect("direct user")]
    );
    assert_eq!(parsed.omitted_spans().len(), 1);
    assert!(!diagnostics
        .iter()
        .any(|item| item.kind == ParseDiagnosticKind::RecordOmitted));
}

#[test]
fn a_statement_past_the_nesting_ceiling_is_given_up_alone() {
    let nested = format!("#2=ITEM({}1{});", "(".repeat(40), ")".repeat(40));
    let source = exchange(&format!(
        "#1=CARTESIAN_POINT('',(1.,2.,3.));{nested}#3=GEOMETRIC_SET('',(#1));"
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_recursion_depth = 32;
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&source), &options)
        .expect("a Part refusal gives up its statement only");
    assert_eq!(decoded.ir().geometry.points.len(), 1);
    let losses = given_up_losses(decoded.report());
    let [loss] = losses.as_slice() else {
        panic!("one record.unreadable loss");
    };
    assert!(loss.message.contains("scope Part"));
    assert!(loss.message.contains(&format!("length={}", nested.len())));
}

#[test]
fn dangling_references_remove_transitive_dependents_without_removing_cycles() {
    let source =
        exchange("#1=ITEM(#90);#2=ITEM(#1);#3=ITEM((#2));#4=ITEM(#5);#5=ITEM(#4);#6=ITEM();");
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("bounded graph");
    assert_eq!(
        parsed.records().keys().copied().collect::<Vec<_>>(),
        [4, 5, 6]
    );
    assert_eq!(
        parsed
            .records()
            .iter()
            .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        [4, 5, 6]
    );
    assert_eq!(parsed.omitted_spans().len(), 3);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|item| item.kind == ParseDiagnosticKind::RecordOmitted)
            .count(),
        3
    );
}

#[test]
fn duplicate_id_removes_every_definition_and_its_users() {
    let source = exchange("#1=ITEM();#2=ITEM(#1);#1=OTHER();#1=THIRD();#3=ITEM();");
    let (parsed, _) =
        with_service_context(source.as_bytes(), parse_inner).expect("bounded duplicates");
    assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
    assert_eq!(
        parsed
            .records()
            .iter()
            .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(parsed.omitted_spans().len(), 4);
}

#[test]
fn malformed_definition_keeps_a_readable_instance_name_ambiguous() {
    for definitions in [
        "#1=ITEM();#1=ITEM(%,3);",
        "#1=ITEM(%,3);#1=ITEM();",
        "#1=ITEM(%,3);#1=ITEM();#1=OTHER();",
    ] {
        let source = exchange(&format!("{definitions}#2=ITEM(#1);#3=ITEM();"));
        let (parsed, diagnostics) =
            with_service_context(source.as_bytes(), parse_inner).expect("bounded definitions");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
        assert_eq!(
            parsed
                .records()
                .iter()
                .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
                .map(|(&id, _)| id)
                .collect::<Vec<_>>(),
            [3]
        );
        let [statement] = parsed.given_up() else {
            panic!("one given-up definition in {definitions}");
        };
        // Every readable definition of #1 and its user #2 need the given-up one.
        let dropped = definitions.matches(';').count();
        assert_eq!(parsed.omitted_spans().len(), dropped);
        assert_eq!(statement.dropped.len(), dropped);
        assert!(!diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::RecordOmitted));
    }
}

#[test]
fn unterminated_literals_and_comments_remain_structural_refusals() {
    for data in ["#1=ITEM('open;#2=ITEM();", "#1=ITEM(/* open;#2=ITEM();"] {
        let source = exchange(data);
        let error = with_service_context(source.as_bytes(), parse_inner)
            .expect_err("a literal has no known continuation boundary");
        assert!(error.to_string().contains("cannot establish a boundary"));
    }
}

#[test]
fn eof_after_complete_records_keeps_the_population() {
    for ending in ["", "ENDSEC", "ENDSEC;", "ENDSEC;END-ISO-10303-21"] {
        let source =
            format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();{ending}");
        let (parsed, diagnostics) =
            with_service_context(source.as_bytes(), parse_inner).expect("record closes before EOF");
        assert_eq!(parsed.records().len(), 1);
        assert!(diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::EnvelopeIncomplete));
    }
}

#[test]
fn a_missing_semicolon_does_not_consume_the_next_assignment_or_section_end() {
    for defective in ["#2=ITEM(3)", "#2=ITEM(3"] {
        let source = exchange(&format!("{defective} #3=ITEM();"));
        let (parsed, _) = with_service_context(source.as_bytes(), parse_inner)
            .expect("next assignment supplies an explicit boundary");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
        assert_eq!(source[parsed.given_up()[0].span.clone()].trim(), defective);
        let source = exchange(&format!("{defective} "));
        let (parsed, _) = with_service_context(source.as_bytes(), parse_inner)
            .expect("section end supplies an explicit boundary");
        assert!(parsed.records().is_empty());
        assert_eq!(source[parsed.given_up()[0].span.clone()].trim(), defective);
        let source = source.replace("ENDSEC;END-ISO", "E\nNdSeC;END-ISO");
        let (parsed, diagnostics) = with_service_context(source.as_bytes(), parse_inner)
            .expect("ignored controls preserve the section boundary");
        assert!(parsed.records().is_empty());
        assert_eq!(source[parsed.given_up()[0].span.clone()].trim(), defective);
        assert!(!diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::EnvelopeIncomplete));
    }
}

#[test]
fn draft_envelope_uses_declared_recovery_grammar_and_retains_header() {
    let source = "!* leading comment *!STEP;HEADER;FILE_IDENTIFICATION('synthetic');IMP_LEVEL('1.0');ENDSEC;DATA;!* data comment *!@1=CARTESIAN_POINT('',(1.,2.,3.));@2=ITEM(#1);@3=GEOMETRIC_SET('',(#1));ENDSEC;ENDSTEP;";
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("draft grammar");
    assert_eq!(parsed.records().len(), 3);
    assert!(parsed.schema_identifiers().next().is_none());
    assert!(diagnostics
        .iter()
        .any(|item| item.kind == ParseDiagnosticKind::DraftExchangeGrammar));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("draft point decoded");
    assert_eq!(decoded.ir().geometry.points.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseDraftGrammar.kind()));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("retained header")
        .iter()
        .any(|record| record.id.as_str().starts_with("step:file:header#")));
}

#[test]
fn usable_schema_identifiers_survive_invalid_entries_and_duplicates() {
    let source = exchange("#1=ITEM();").replace(
        "('AP242')",
        "(17,'bad name','AUTOMOTIVE_DESIGN','automotive_design')",
    );
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("usable schema");
    assert_eq!(
        parsed.schema_identifiers().collect::<Vec<_>>(),
        ["AUTOMOTIVE_DESIGN"]
    );
    assert!(diagnostics
        .iter()
        .any(|item| item.message.contains("duplicate excluded")));
}

#[test]
fn recovery_never_turns_a_resource_refusal_into_an_omission() {
    let source = exchange("#1=ITEM();#2=ITEM();");
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_entities = 1;
    with_policy_context(source.as_bytes(), &policy, |source, ctx| {
        assert!(matches!(crate::parse::parse(source, ctx),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities));
    });
}

#[test]
fn strict_decode_rejects_the_reported_given_up_record() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=ITEM(#0);#2=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let mut options = DecodeOptions::default();
    options.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a completed decode with a given-up record");
    assert!(
        matches!(error, cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }
        if rejection.loss().code == record_unreadable_kind())
    );
}

#[test]
fn reference_walk_allocates_only_reference_outputs() {
    let value = crate::parse::Value::List(vec![crate::parse::Value::Integer(1); 32]);
    let policy = DecodePolicy::service();
    with_policy_context(b"", &policy, |_, ctx| {
        super::visit_references(&value, ctx, &mut |_, _| panic!("scalar has no reference"))
            .expect("scalar traversal visits no reference");
    });
}

#[test]
fn bounded_draft_header_defects_retain_source_and_independent_geometry() {
    for defective in [
        ".\n.\nFILE_DESCRIPTION('synthetic');",
        "FILE_DESCRIPTION('a'b);",
    ] {
        let source = format!("STEP;HEADER;{defective}IMP_LEVEL('1.0');ENDSEC;DATA;@1=CARTESIAN_POINT('',(1.,2.,3.));@2=GEOMETRIC_SET('',(#1));ENDSEC;ENDSTEP;");
        let (parsed, _) = with_service_context(source.as_bytes(), parse_inner)
            .expect("bounded HEADER statement admits independent DATA");
        let [statement] = parsed.given_up() else {
            panic!("one given-up HEADER statement");
        };
        assert_eq!(statement.kind, "header_record");
        assert_eq!(&source[statement.span.clone()], defective);
        assert_eq!(parsed.records().len(), 2);
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("independent point");
        assert_eq!(decoded.ir().geometry.points.len(), 1);
        assert_eq!(given_up_losses(decoded.report()).len(), 1);
        let (ir, _, fidelity) = decoded.into_parts();
        let unknowns = ir.native_unknowns("step").expect("retained statements");
        let stub = unknowns
            .iter()
            .find(|record| record.id.as_str().starts_with("step:source:part#"))
            .expect("given-up HEADER statement");
        assert!(unknowns
            .iter()
            .any(|record| record.id.as_str().starts_with("step:file:header#")));
        assert_eq!(
            fidelity
                .retained_record(stub.id.as_str())
                .expect("exact source")
                .data(),
            Some(defective.as_bytes())
        );
        assert_eq!(
            ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
            "0"
        );
    }
}

#[test]
fn bounded_draft_preamble_is_retained_with_an_explicit_loss() {
    let source =
        "EXTRA!*synthetic*!STEP;HEADER;IMP_LEVEL('1.0');ENDSEC;DATA;@1=ITEM();ENDSEC;ENDSTEP;";
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("bounded draft preamble");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParsePreambleOmitted.kind()));
    let (ir, _, fidelity) = decoded.into_parts();
    assert_eq!(
        fidelity
            .retained_record("step:file:omitted#0")
            .expect("exact prefix")
            .data(),
        Some(b"EXTRA".as_slice())
    );
    assert_eq!(
        ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
        "0"
    );
}
