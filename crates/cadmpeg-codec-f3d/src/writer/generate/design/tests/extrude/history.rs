// SPDX-License-Identifier: Apache-2.0
//! The history-true form of a sketch-and-extrude design (cadir-v11.md §3.4,
//! Features without their state): the run of features written with their
//! states, the timeline marker after it, and the later features rolled back
//! (f3d.md §3.1, "Feature item tail").

use cadmpeg_ir::codec::write::MissingStateForm;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;

use crate::loss::F3dLossCode;
use crate::test_support::native_test::f3d_native;

use super::super::sketch::{feature, feature_id};
use super::super::write_bytes_as;
use super::{blind, extrude, kinds, loss, sketch_and_extrude, EVALUATION_MISSING, EXTRUDE};

/// The decoded rollback of the one decoded timeline, and its members.
fn decoded_rollback(decoded: &CadIr) -> (Option<Rollback>, Vec<Member>) {
    let [Container::Ordered {
        members, rollback, ..
    }] = decoded.design.containers.as_slice()
    else {
        panic!("one decoded timeline: {:#?}", decoded.design.containers);
    };
    (rollback.clone(), members.clone())
}

/// `ir` with an unresolved fillet inserted in its timeline before the datum
/// `after`, its last member. Returns the fillet's id.
fn with_fillet(ir: &mut CadIr) -> FeatureId {
    let fillet = feature(
        "fillet",
        FeatureOperation::Unresolved {
            family: OperationFamily::Fillet,
        },
    );
    let id = fillet.id.clone();
    ir.design.features.push(fillet);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    let at = members.len() - 1;
    members.insert(at, Member::Feature(id.clone()));
    id
}

/// The unit cube as `Sketch`, `Extrude`, a later `Sketch` of one line, an
/// unresolved fillet and a datum, rolled back before the later sketch, with
/// the current state after the extrude. Returns the design, the later
/// sketch and the fillet.
fn rolled_back_after_the_extrude() -> (CadIr, FeatureId, FeatureId) {
    let mut ir = sketch_and_extrude(blind(), true);
    let fillet = with_fillet(&mut ir);
    let later = feature_id("later");
    let Some(Container::Ordered { rollback, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    *rollback = Some(Rollback::Before(Member::Feature(later.clone())));
    ir.design.scopes[0].current[0].at = Position::After(Member::Feature(
        FeatureId::mint(EXTRUDE).expect("identity grammar"),
    ));
    (ir, later, fillet)
}

#[test]
fn the_run_is_written_with_its_state_and_the_later_features_follow_the_marker() {
    let (ir, later, fillet) = rolled_back_after_the_extrude();
    let (geometry_true, decoded, _) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    assert!(
        kinds(&decoded).contains(&"base"),
        "{:#?}",
        geometry_true.losses
    );

    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(members.len(), 3);
    assert_eq!(rollback, Some(Rollback::Before(members[2].clone())));
    assert_eq!(decoded.geometry.bodies.len(), 1);

    let missing = loss(&report, EVALUATION_MISSING);
    let rolled = missing
        .iter()
        .find(|message| message.contains("after the timeline marker, rolled back"))
        .unwrap_or_else(|| panic!("{missing:#?}"));
    assert!(rolled.contains(later.as_str()), "{rolled}");
    let unwritten = missing
        .iter()
        .find(|message| message.contains(fillet.as_str()))
        .unwrap_or_else(|| panic!("{missing:#?}"));
    assert!(
        unwritten.contains(&format!("{fillet} (it follows the timeline marker")),
        "{unwritten}"
    );
    assert!(
        missing.iter().all(|message| !message.contains(EXTRUDE)),
        "{missing:#?}"
    );
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(!omitted.contains("rollback (before"), "{omitted}");
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(
        residual.contains("(Sketch scope: its item tail)") && residual.contains("DR-1032"),
        "{residual}"
    );
}

/// The export note of a history-true request written in the geometry-true
/// form.
const GEOMETRY_TRUE: &str = "in the geometry-true form and not the history-true form";

#[test]
fn a_history_true_write_notes_the_geometry_true_form_only_where_it_writes_it() {
    let (ir, _, _) = rolled_back_after_the_extrude();
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(kinds(&decoded), ["sketch", "extrude", "sketch"]);
    assert!(
        report
            .notes
            .iter()
            .all(|note| !note.contains(GEOMETRY_TRUE)),
        "{:#?}",
        report.notes
    );

    super::super::super::FAIL_NEXT_PLAN.with(|fail| fail.set(u32::MAX));
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    super::super::super::FAIL_NEXT_PLAN.with(|fail| fail.set(0));
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert!(
        loss(&report, EVALUATION_MISSING)
            .iter()
            .any(|message| message.contains("a test failure of the design module")),
        "{:#?}",
        report.losses
    );
    assert!(
        report.notes.iter().any(|note| note.contains(GEOMETRY_TRUE)),
        "{:#?}",
        report.notes
    );
}

#[test]
fn a_rollback_before_a_later_sketch_is_written_where_the_source_states_it() {
    let mut ir = sketch_and_extrude(blind(), true);
    let second = feature(
        "second",
        FeatureOperation::Extrude(Box::new(extrude(blind()))),
    );
    let second_id = second.id.clone();
    ir.design.features.push(second);
    let later = feature_id("later");
    let Some(Container::Ordered {
        members, rollback, ..
    }) = ir.design.containers.first_mut()
    else {
        panic!("the cube's timeline");
    };
    let at = members
        .iter()
        .position(|member| *member == Member::Feature(later.clone()))
        .expect("the later sketch");
    members.insert(at + 1, Member::Feature(second_id));
    *rollback = Some(Rollback::Before(Member::Feature(later)));
    ir.design.scopes[0].current[0].at = Position::After(Member::Feature(
        FeatureId::mint(EXTRUDE).expect("identity grammar"),
    ));

    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[2].clone())));
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(!omitted.contains("rollback (before"), "{omitted}");
}

#[test]
fn a_timeline_whose_every_feature_has_its_state_writes_one_form() {
    let ir = sketch_and_extrude(blind(), true);
    let (geometry_true, _, geometry_bytes) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    let (history_true, decoded, history_bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(kinds(&decoded), ["sketch", "extrude", "sketch"]);
    assert_eq!(decoded_rollback(&decoded).0, Some(Rollback::End));
    assert!(geometry_bytes == history_bytes, "the two forms differ");
    assert_eq!(
        format!("{:?}", geometry_true.losses),
        format!("{:?}", history_true.losses)
    );
    assert_eq!(geometry_true.notes, history_true.notes);
}

#[test]
fn a_run_whose_feature_has_no_state_stands_after_the_marker_with_no_body() {
    let mut ir = sketch_and_extrude(blind(), false);
    let fillet = with_fillet(&mut ir);
    let (_, _, geometry_bytes) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    let (report, decoded, history_bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert!(geometry_bytes != history_bytes, "the two forms agree");
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[1].clone())));
    assert!(decoded.geometry.bodies.is_empty());
    assert!(
        !history_bytes.windows(4).any(|name| name == b".smb"),
        "a B-rep stream member"
    );
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the writer writes it without its state: the current state is not its result)"
        )),
        "{missing}"
    );
    assert!(missing.contains(fillet.as_str()), "{missing}");
    let omitted = loss(
        &report,
        &F3dLossCode::WriterGeometryOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(
        omitted.contains("holds no body at its timeline marker"),
        "{omitted}"
    );
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    let reference = residual
        .split("body-reference record ")
        .skip(1)
        .find_map(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|digits| digits.parse::<u64>().ok())
        })
        .unwrap_or_else(|| panic!("{residual}"));
    let native = f3d_native(&decoded);
    let listed = native
        .design_body_members
        .iter()
        .map(|member| member.entity_suffix)
        .collect::<Vec<_>>();
    assert_eq!(listed, [reference]);
    assert!(native.design_body_bindings.is_empty());
}

/// The unit cube as `Sketch` and `Extrude`, then a second `Extrude` of the
/// same profile, rolled back before the second extrude. Returns the design
/// and the second extrude.
fn with_a_rolled_back_extrude() -> (CadIr, FeatureId) {
    let mut ir = sketch_and_extrude(blind(), false);
    let second = feature(
        "second",
        FeatureOperation::Extrude(Box::new(extrude(blind()))),
    );
    let second_id = second.id.clone();
    ir.design.features.push(second);
    let Some(Container::Ordered {
        members, rollback, ..
    }) = ir.design.containers.first_mut()
    else {
        panic!("the cube's timeline");
    };
    members.insert(2, Member::Feature(second_id.clone()));
    *rollback = Some(Rollback::Before(Member::Feature(second_id.clone())));
    ir.design.scopes[0].current[0].at = Position::After(Member::Feature(
        FeatureId::mint(EXTRUDE).expect("identity grammar"),
    ));
    (ir, second_id)
}

#[test]
fn a_rolled_back_extrude_follows_the_marker_with_its_definition() {
    let (ir, second_id) = with_a_rolled_back_extrude();
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "extrude"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[2].clone())));
    assert_eq!(decoded.geometry.bodies.len(), 1);
    let mut features = decoded.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let names = features
        .iter()
        .filter_map(|feature| feature.name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Sketch 1", "Extrude 1", "Extrude 2"]);
    let missing = loss(&report, EVALUATION_MISSING);
    let rolled = missing
        .iter()
        .find(|message| message.contains("after the timeline marker, rolled back"))
        .unwrap_or_else(|| panic!("{missing:#?}"));
    assert!(rolled.contains(second_id.as_str()), "{rolled}");
    assert!(
        missing
            .iter()
            .all(|message| !message.contains("writes no rolled-back item")),
        "{missing:#?}"
    );
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(residual.contains("DR-1032"), "{residual}");
    let native = f3d_native(&decoded);
    let rolled = native
        .design_parameter_scopes
        .iter()
        .find(|scope| scope.kind_name() == "Extrude" && scope.feature_ordinal.get() == 2)
        .expect("the rolled-back Extrude scope");
    assert_eq!(
        (
            rolled.history_state_id(),
            rolled.previous_history_state_id()
        ),
        (None, None)
    );
    let flags = rolled.tail_flags.expect("the item tail flags");
    assert!(flags.no_result && flags.rolled_back);
    assert!(!flags.suppressed() && !flags.unevaluated);
}

/// `ir` with an `f3d` namespace whose one native Design record is a Fusion
/// Design type row: the writer writes the Design stream from its native
/// records.
fn with_a_native_design(mut ir: CadIr) -> CadIr {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    crate::test_support::native_test::f3d_native_mut(&mut ir).design_types =
        vec![SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#0".into(),
            SegmentTypeData {
                byte_offset: 0,
                module: "Fusion".to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![1]),
                type_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Absent,
                version: 7,
                version_offset: 0,
            },
        )
        .expect("a Design type row")];
    ir
}

#[test]
fn a_native_design_rolled_back_writes_the_chosen_form() {
    let (ir, second) = with_a_rolled_back_extrude();
    let ir = with_a_native_design(ir);
    let dropped = |report: &cadmpeg_ir::report::export::ExportReport, form: &str| {
        let omitted = loss(report, "export/native.records-omitted").join("\n");
        assert!(
            omitted.contains(&format!(
                "of the Design stream, because the generator writes the design's timeline, rolled back before {second}, from its neutral records in the {form} form"
            )),
            "{omitted}"
        );
    };

    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    dropped(&report, "history-true");
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "extrude"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[2].clone())));
    let missing = loss(&report, EVALUATION_MISSING);
    let rolled = missing
        .iter()
        .find(|message| message.contains("after the timeline marker, rolled back"))
        .unwrap_or_else(|| panic!("{missing:#?}"));
    assert!(rolled.contains(second.as_str()), "{rolled}");

    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    dropped(&report, "geometry-true");
    assert!(kinds(&decoded).contains(&"base"), "{:#?}", report.losses);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(missing.contains(second.as_str()), "{missing}");
    assert!(
        !missing.contains("rolled back with their definitions"),
        "{missing}"
    );
}
