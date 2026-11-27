// SPDX-License-Identifier: Apache-2.0
//! The history-true form of a sketch-and-extrude design (cadir-v11.md §3.4,
//! Features without their state): the run of features written with their
//! states, the timeline marker after it, and the later features rolled back
//! (f3d.md §3.1, "Feature item tail").

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::MissingStateForm;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{Evaluation, EvaluationStatus, StateBase, StateDelta};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::options::{Boolean, BooleanOp};
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::design::selection::{Members, PickTarget, Picked, Picks, PrincipalEntity};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;

use crate::loss::F3dLossCode;
use crate::test_support::native_test::f3d_native;

use super::super::sketch::{
    feature, feature_id, frame, picked, point2, principal, sketch_id, unread, Source,
};
use super::super::{neutral, number, parameter, parameter_id, tails, write_bytes_as};
use super::arcs::{prism, Side};
use super::{blind, extrude, kinds, loss, sketch_and_extrude, EVALUATION_MISSING, EXTRUDE, SIDE};

/// The decoded rollback of the one decoded timeline, and its members.
pub(super) fn decoded_rollback(decoded: &CadIr) -> (Option<Rollback>, Vec<Member>) {
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
pub(super) fn with_fillet(ir: &mut CadIr) -> FeatureId {
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
    assert!(!residual.contains("its item tail"), "{residual}");
    let sketches = tails(&decoded)
        .into_iter()
        .filter(|tail| tail.0 == "Sketch")
        .collect::<Vec<_>>();
    assert_eq!(
        sketches,
        [
            ("Sketch".to_owned(), [0, 0, 0], false, false, None, None),
            ("Sketch".to_owned(), [0, 0, 0], true, true, None, None),
        ]
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
            "{EXTRUDE} (the writer writes it without its state: the current state stands after the timeline marker, and the writer writes no evaluation of the document as the state at the marker)"
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

#[test]
fn a_design_whose_timeline_holds_no_body_decodes_with_no_body_loss() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut ir = sketch_and_extrude(blind(), false);
    with_fillet(&mut ir);
    let (report, _, bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert!(
        !loss(
            &report,
            &F3dLossCode::WriterGeometryOmitted.kind().to_string()
        )
        .is_empty(),
        "{:#?}",
        report.losses
    );
    let decoded = crate::F3dCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    let codes = decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect::<Vec<_>>();
    for absent in [
        F3dLossCode::GeometryNotTransferred,
        F3dLossCode::TopologyNotTransferred,
        F3dLossCode::MissingGeometryStream,
    ] {
        assert!(!codes.contains(&absent.kind()), "{codes:#?}");
    }
    assert!(
        codes.contains(&F3dLossCode::BodylessDesignCarrier.kind()),
        "{codes:#?}"
    );
}

/// The unit cube as `Sketch` and `Extrude`, then a second `Extrude` of the
/// same profile, rolled back before the second extrude. Returns the design
/// and the second extrude.
pub(super) fn with_a_rolled_back_extrude() -> (CadIr, FeatureId) {
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

#[test]
fn every_kind_with_a_definition_writer_follows_the_marker_with_the_no_state_tail() {
    let mut ir = sketch_and_extrude(blind(), false);
    let mut top = Source::new(
        "top",
        unread(),
        frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    top.entity(
        "line",
        PlanarCurve::Line {
            start: point2(1.0, 1.0),
            end: point2(3.0, 2.0),
        },
        &[],
    );
    let top = top.add_to(&mut ir);
    let second = feature(
        "second",
        FeatureOperation::Extrude(Box::new(extrude(blind()))),
    );
    let second_id = second.id.clone();
    ir.design.features.push(second);
    {
        let Some(Container::Ordered {
            members, rollback, ..
        }) = ir.design.containers.first_mut()
        else {
            panic!("the cube's timeline");
        };
        members.insert(2, Member::Feature(top.clone()));
        members.insert(3, Member::Feature(second_id.clone()));
        *rollback = Some(Rollback::Before(Member::Feature(top.clone())));
    }
    let fillet = with_fillet(&mut ir);
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
    let tails = tails(&decoded);
    let families = tails.iter().map(|tail| tail.0.as_str()).collect::<Vec<_>>();
    assert_eq!(
        families,
        ["Sketch", "Extrude", "WorkPlane", "Sketch", "Extrude"],
        "{tails:#?}"
    );
    let rolled = tails.iter().map(|tail| tail.3).collect::<Vec<_>>();
    assert_eq!(rolled, [false, false, true, true, true], "{tails:#?}");
    assert_eq!(
        tails[1],
        (
            "Extrude".to_owned(),
            [0, 0, 0],
            false,
            false,
            Some(3),
            Some(2)
        )
    );
    assert_eq!(
        tails[4],
        ("Extrude".to_owned(), [0, 0, 0], true, true, None, None)
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[2].clone())));

    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(missing.contains(second_id.as_str()), "{missing}");
    assert!(
        missing.contains(&format!("{fillet} (it follows the timeline marker")),
        "{missing}"
    );
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

/// The cube of [`sketch_and_extrude`], then a `Sketch` of the square from
/// (20, 0) to (30, 10) on `XY` and an `Extrude` of it by its own parameter
/// to 10 mm with `boolean`, before the datum. The current state holds the
/// cube and the prism of the second square. Returns the design and the
/// second extrude.
pub(super) fn with_a_second_extrude(boolean: Boolean) -> (CadIr, FeatureId) {
    let mut ir = sketch_and_extrude(blind(), false);
    let corners = [[20.0, 0.0], [30.0, 0.0], [30.0, SIDE], [20.0, SIDE]];
    let mut square = Source::new(
        "second",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    for (index, [x, y]) in corners.into_iter().enumerate() {
        let [u, v] = corners[(index + 1) % corners.len()];
        square.entity(
            &format!("l{}", index + 1),
            PlanarCurve::Line {
                start: point2(x, y),
                end: point2(u, v),
            },
            &[],
        );
    }
    let sketch = square.add_to(&mut ir);
    let mut operation = extrude(Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Parameter(parameter_id("d2")))),
    });
    operation.profile.picks = Picks::Named(
        Members::new(vec![Picked {
            target: PickTarget::Sketch(sketch_id("second")),
            context: None,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        }])
        .expect("one pick"),
    );
    operation.output.boolean = Some(boolean);
    let second = feature(
        "second-extrude",
        FeatureOperation::Extrude(Box::new(operation)),
    );
    let second_id = second.id.clone();
    ir.design.features.push(second);
    let mut depth = parameter(
        "d2",
        "d2",
        neutral(number(SIDE, Some(UnitSymbol::Mm))),
        Some(SIDE),
    );
    depth.owner = Some(ParameterOwner::Feature(second_id.clone()));
    ir.design.parameters.push(depth);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    let at = members.len() - 1;
    members.insert(at, Member::Feature(sketch));
    members.insert(at + 1, Member::Feature(second_id.clone()));
    let solid = prism(
        &[corners.map(|corner| (corner, Side::Line)).to_vec()],
        [0.0, SIDE],
    );
    let renamed = json::to_string(&solid.geometry)
        .expect("the prism serializes")
        .replace('#', "#second-");
    let serde_json::Value::Object(arenas) =
        serde_json::from_str::<serde_json::Value>(&renamed).expect("the prism reads")
    else {
        panic!("the prism's arenas");
    };
    let mut merged = json::to_value(&ir.geometry).expect("the cube serializes");
    let mut body = None;
    for (arena, records) in arenas {
        let serde_json::Value::Array(records) = records else {
            continue;
        };
        if arena == "bodies" {
            body = records.first().map(|record| record["id"].clone());
        }
        let slot = merged
            .as_object_mut()
            .expect("the cube's arenas")
            .entry(arena)
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        slot.as_array_mut().expect("an arena").extend(records);
    }
    ir.geometry = json::from_value(merged).expect("the merged arenas");
    let body = json::from_value(body.expect("the prism's body")).expect("a body id");
    ir.design.scopes[0].current[0].bodies.push(body);
    ir.finalize();
    (ir, second_id)
}

/// A `delta_state` record: identity, previous, next and node references, and
/// per board its owner, its number and whether every change inserts.
type State = (i64, i64, i64, i64, Vec<(i64, i64, bool)>);

/// The `delta_state` records of the history stream in `archive`, in stream
/// order.
fn history_chain(archive: &[u8]) -> Vec<State> {
    history_boards(archive)
        .into_iter()
        .map(|(state, boards)| {
            let boards = boards
                .into_iter()
                .map(|(owner, number, inserted)| (owner, number, inserted.is_some()))
                .collect();
            (state.0, state.1, state.2, state.3, boards)
        })
        .collect()
}

/// A board of a `delta_state` record: its owner, its number, and the slots
/// it inserts where every change inserts.
type Board = (i64, i64, Option<Vec<i64>>);

/// The header of a `delta_state` record: identity, previous, next and node
/// references.
type Header = (i64, i64, i64, i64);

/// The `delta_state` records of the history stream in `archive`, in stream
/// order: each header and its boards.
fn history_boards(archive: &[u8]) -> Vec<(Header, Vec<Board>)> {
    use cadmpeg_asm::history::{
        delta_state_offsets, read_delta_state_header, walk_bulletin_boards, BulletinBoardItem,
        DeltaStateHeaderRead, EntityChange,
    };
    use cadmpeg_asm::kernel_header::RefWidth;
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).expect("a ZIP archive");
    let index = (0..zip.len())
        .find(|&index| {
            zip.by_index(index)
                .is_ok_and(|member| member.name().is_ok_and(|name| name.ends_with(".smbh")))
        })
        .expect("a history stream");
    let mut stream = Vec::new();
    zip.by_index(index)
        .expect("the history stream")
        .read_to_end(&mut stream)
        .expect("the history stream reads");
    delta_state_offsets(&stream)
        .into_iter()
        .map(|offset| {
            let DeltaStateHeaderRead::Framed(header) =
                read_delta_state_header(&stream, offset, RefWidth::Eight)
            else {
                panic!("a framed delta_state at {offset}");
            };
            let mut boards = Vec::<Board>::new();
            walk_bulletin_boards(
                &stream,
                header.body_start,
                RefWidth::Eight,
                |item| match item {
                    BulletinBoardItem::Board {
                        owner_ref, number, ..
                    } => boards.push((owner_ref, number, Some(Vec::new()))),
                    BulletinBoardItem::Change { kind, .. } => {
                        let board = boards.last_mut().expect("a board");
                        match (kind, board.2.as_mut()) {
                            (EntityChange::Insert { new }, Some(inserted)) => inserted.push(new),
                            _ => board.2 = None,
                        }
                    }
                },
            )
            .expect("the bulletin boards walk");
            (
                (
                    header.state_id,
                    header.previous,
                    header.next,
                    header.node_index,
                ),
                boards,
            )
        })
        .collect()
}

#[test]
fn each_extrude_that_makes_a_new_body_commits_its_own_state() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    for form in [
        MissingStateForm::GeometryTrue,
        MissingStateForm::HistoryTrue,
    ] {
        let (report, decoded, bytes) = write_bytes_as(&ir, form);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude", "sketch", "extrude"],
            "{:#?}",
            report.losses
        );
        assert_eq!(decoded.geometry.bodies.len(), 2);
        let extrudes = tails(&decoded)
            .into_iter()
            .filter(|tail| tail.0 == "Extrude")
            .collect::<Vec<_>>();
        assert_eq!(
            extrudes,
            [
                (
                    "Extrude".to_owned(),
                    [0, 0, 0],
                    false,
                    false,
                    Some(3),
                    Some(2)
                ),
                (
                    "Extrude".to_owned(),
                    [0, 0, 0],
                    false,
                    false,
                    Some(4),
                    Some(3)
                ),
            ]
        );
        let mut features = decoded.model.features.iter().collect::<Vec<_>>();
        features.sort_by_key(|feature| feature.ordinal);
        let outputs = features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                    )
                )
            })
            .map(|feature| feature.evaluation.outputs().clone())
            .collect::<Vec<_>>();
        let bodies = decoded
            .geometry
            .bodies
            .iter()
            .map(|body| vec![body.id.clone()])
            .collect::<Vec<_>>();
        assert_eq!(outputs, bodies);
        assert_eq!(
            history_chain(&bytes),
            [
                (4, -1, 2, 0, vec![(0, 2, true)]),
                (2, 2, -1, 1, Vec::new()),
                (3, 0, 1, 2, vec![(2, 2, true)]),
            ]
        );
        let missing = loss(&report, EVALUATION_MISSING).join("\n");
        assert!(!missing.contains(second.as_str()), "{missing}");
        let residual = loss(&report, "export/record.residual-unavailable").join("\n");
        assert!(
            residual.contains("history_entry_count") && residual.contains("HS-02"),
            "{residual}"
        );
        assert!(!residual.contains("ASM record"), "{residual}");
        each_board_inserts_the_records_of_its_body(&decoded, &bytes);
    }
}

/// The boards of the two states after the root state insert every slot
/// but `0` once each, and the decoded state that the first commits holds
/// the first body and the slots of its board alone; the head state adds
/// the second body from the slots of its own board.
fn each_board_inserts_the_records_of_its_body(decoded: &CadIr, bytes: &[u8]) {
    use std::collections::BTreeSet;
    let boards = history_boards(bytes);
    let inserted = |state: i64| -> BTreeSet<i64> {
        let (_, boards) = boards
            .iter()
            .find(|(header, _)| header.0 == state)
            .unwrap_or_else(|| panic!("state {state}"));
        let [(_, _, Some(slots))] = boards.as_slice() else {
            panic!("one insertion board in state {state}: {boards:?}");
        };
        slots.iter().copied().collect()
    };
    let (first, head) = (inserted(3), inserted(4));
    assert!(first.is_disjoint(&head), "{first:?} {head:?}");
    let native = f3d_native(decoded);
    let [history] = native.asm_histories.as_slice() else {
        panic!("one decoded history");
    };
    let topology = |state: i64| {
        let state = history
            .states()
            .iter()
            .find(|candidate| candidate.state_id == state)
            .unwrap_or_else(|| panic!("decoded state {state}"));
        history
            .topology(state)
            .expect("the state materializes")
            .clone()
    };
    let slots = |topology: &crate::history_records::AsmHistoricalTopology| {
        [
            &topology.bodies,
            &topology.regions,
            &topology.shells,
            &topology.faces,
            &topology.loops,
            &topology.coedges,
            &topology.edges,
            &topology.vertices,
            &topology.points,
            &topology.surfaces,
            &topology.curves,
            &topology.pcurves,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
    };
    let (after_first, after_head) = (topology(3), topology(4));
    assert_eq!(after_first.bodies.len(), 1);
    assert_eq!(after_first.faces.len(), 6);
    assert_eq!(after_head.bodies.len(), 2);
    assert_eq!(after_head.faces.len(), 12);
    let (first_slots, head_slots) = (slots(&after_first), slots(&after_head));
    assert!(first_slots.is_subset(&first), "{first_slots:?} {first:?}");
    let added = head_slots
        .difference(&first_slots)
        .copied()
        .collect::<BTreeSet<_>>();
    assert!(added.is_subset(&head), "{added:?} {head:?}");
    assert!(head_slots.is_superset(&first_slots));
    let every = first.union(&head).copied().collect::<BTreeSet<_>>();
    assert_eq!(every.first(), Some(&1));
    assert_eq!(
        every.last().and_then(|last| usize::try_from(*last).ok()),
        Some(every.len())
    );
}

/// The extrudes of the decoded timeline before the member at the root
/// scope's current position, latest first.
fn built_extrudes(decoded: &CadIr) -> Vec<FeatureId> {
    let (_, members) = decoded_rollback(decoded);
    let Position::After(last) = &decoded.design.scopes[0].current[0].at else {
        panic!("a current state after a member");
    };
    let at = members
        .iter()
        .position(|member| member == last)
        .expect("the member at the current position");
    members[..at]
        .iter()
        .rev()
        .filter_map(|member| {
            let Member::Feature(id) = member else {
                return None;
            };
            let feature = decoded
                .design
                .features
                .iter()
                .find(|found| &found.id == id)?;
            matches!(
                feature.definition.operation,
                FeatureOperation::Extrude(_)
                    | FeatureOperation::Unresolved {
                        family: OperationFamily::Extrude
                    }
            )
            .then(|| id.clone())
        })
        .collect()
}

#[test]
fn a_history_true_write_decodes_each_stored_state_and_a_base_feature_rewrite_charges_it() {
    use cadmpeg_core::decode::ResourceLimits;
    let (ir, _) = with_a_second_extrude(Boolean::NewBody);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    let extrudes = built_extrudes(&decoded);
    let evaluations = &decoded.design.evaluations;
    assert_eq!(
        evaluations
            .iter()
            .map(|evaluation| evaluation.of.clone())
            .collect::<Vec<_>>(),
        extrudes
            .iter()
            .map(|id| Evaluated::State(Position::After(Member::Feature(id.clone()))))
            .collect::<Vec<_>>(),
        "{:#?}",
        report.losses
    );
    let mut base = StateBase::Current(decoded.design.scopes[0].current[0].at.clone());
    for evaluation in evaluations {
        assert_eq!(evaluation.status, EvaluationStatus::Current);
        assert_eq!(evaluation.scope, decoded.design.scopes[0].definition);
        assert!(evaluation.native_ref.is_some());
        let content = evaluation.content.as_ref().expect("the state's content");
        assert_eq!(content.base, base);
        base = StateBase::Evaluation(evaluation.id.clone());
    }

    let first = evaluations
        .last()
        .expect("the state after the first extrude");
    let state = decoded
        .resolved_evaluation_state(&first.id, &ResourceLimits::default())
        .expect("the state after the first extrude resolves");
    let records = state.records();
    assert_eq!(records.bodies.len(), 1);
    assert_eq!(records.faces.len(), 6);
    assert!(records
        .faces
        .iter()
        .all(|face| decoded.geometry.faces.contains(face)));
    let removed = decoded
        .geometry
        .bodies
        .iter()
        .filter(|body| !records.bodies.contains(body))
        .map(|body| RecordRef::Body(body.id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(removed.len(), 1);
    let content = first.content.as_ref().expect("the state's content");
    assert!(removed
        .iter()
        .all(|body| content.delta.removed.contains(body)));

    let (again, _, _) = write_bytes_as(&decoded, MissingStateForm::HistoryTrue);
    let omitted = loss(
        &again,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(omitted.contains(first.id.as_str()), "{omitted}");
}

#[test]
fn a_design_whose_stored_states_overrun_the_retained_allowance_decodes_without_them() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let (ir, _) = with_a_second_extrude(Boolean::NewBody);
    let (_, full, bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert!(!full.design.evaluations.is_empty());
    let decode = |limit: u64| {
        let mut options = DecodeOptions::default();
        options.policy.limits.max_retained_bytes = limit;
        crate::F3dCodec
            .decode(&mut std::io::Cursor::new(bytes.clone()), &options)
            .ok()
    };
    let mut refused = 0_u64;
    let mut decodes = DecodeOptions::default().policy.limits.max_retained_bytes;
    assert!(decode(refused).is_none() && decode(decodes).is_some());
    while decodes - refused > 1 {
        let limit = refused + (decodes - refused) / 2;
        if decode(limit).is_some() {
            decodes = limit;
        } else {
            refused = limit;
        }
    }
    let decoded = decode(decodes).expect("the design decodes at the least allowance");
    let losses = &decoded.report().losses;
    assert!(decoded.ir().design.evaluations.is_empty(), "{losses:#?}");
    assert!(
        losses.iter().any(|loss| {
            [
                F3dLossCode::HistoryStatesUnread.kind(),
                F3dLossCode::HistoryStateUnread.kind(),
            ]
            .contains(&loss.code)
                && (loss.message.contains("resource limits")
                    || loss.message.contains("retained-byte allowance"))
        }),
        "{losses:#?}"
    );
}

#[test]
fn a_history_true_write_charges_no_evaluation_that_its_history_restates() {
    let (mut ir, _) = with_a_second_extrude(Boolean::NewBody);
    let cube = cube_evaluation(&ir);
    let id = cube.id.clone();
    ir.design.evaluations.push(cube);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.message.contains(id.as_str())),
        "{:#?}",
        report.losses
    );
    assert_eq!(
        decoded
            .design
            .evaluations
            .iter()
            .map(|evaluation| evaluation.of.clone())
            .collect::<Vec<_>>(),
        built_extrudes(&decoded)
            .into_iter()
            .map(|id| Evaluated::State(Position::After(Member::Feature(id))))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_patched_write_keeps_the_stored_states_and_charges_no_evaluation() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::report::export::WritePath;
    use std::io::Cursor;

    let (ir, _) = with_a_second_extrude(Boolean::NewBody);
    let (_, _, bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    let decode = |bytes: Vec<u8>| {
        crate::F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("the file decodes")
    };
    let source = decode(bytes);
    let mut edited = source.ir().clone();
    assert!(!edited.design.evaluations.is_empty());
    let moved = edited.geometry.points[0].position().get();
    edited.geometry.points[0].set_position(
        FinitePoint3::new(Point3::new(moved.x + 1.0, moved.y, moved.z))
            .expect("a finite position is a point"),
    );

    let plan = crate::F3dCodec
        .plan(
            EncodeInput::new(&edited, Some(source.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("the edited document writes");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Patched { .. }
    ));
    assert!(
        !plan.report().losses.iter().any(|loss| edited
            .design
            .evaluations
            .iter()
            .any(|evaluation| loss.message.contains(evaluation.id.as_str()))),
        "{:#?}",
        plan.report().losses
    );
    let mut written = Vec::new();
    plan.write_to(&mut written)
        .expect("the patched file writes");
    let patched = decode(written);
    assert_eq!(
        patched
            .ir()
            .design
            .evaluations
            .iter()
            .map(|evaluation| (&evaluation.of, &evaluation.native_ref))
            .collect::<Vec<_>>(),
        edited
            .design
            .evaluations
            .iter()
            .map(|evaluation| (&evaluation.of, &evaluation.native_ref))
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_extrude_that_changes_an_earlier_body_stays_in_the_base_feature() {
    let (ir, second) = with_a_second_extrude(Boolean::Apply {
        op: BooleanOp::Join,
        targets: None,
    });
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "sketch", "base"],
        "{:#?}",
        report.losses
    );
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{second} (the Base Feature holds it: it changes a body of the state before it"
        )) && missing.contains("DR-1054"),
        "{missing}"
    );
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the Base Feature holds it: the writer cannot write {second} with its state;"
        )),
        "{missing}"
    );
}

/// [`with_a_second_extrude`] whose second extrude joins the cube, with the
/// state after the first extrude, the cube alone, held by an evaluation
/// where `held`.
fn joined_after_the_cube(held: bool) -> (CadIr, FeatureId) {
    let (mut ir, second) = with_a_second_extrude(Boolean::Apply {
        op: BooleanOp::Join,
        targets: None,
    });
    if held {
        let cube = cube_evaluation(&ir);
        ir.design.evaluations.push(cube);
    }
    (ir, second)
}

/// The evaluation of the state after the first extrude of `ir`: the cube
/// alone.
fn cube_evaluation(ir: &CadIr) -> Evaluation {
    use cadmpeg_ir::design::evaluation::StateContent;
    use cadmpeg_ir::design::ids::EvaluationId;
    Evaluation {
        id: EvaluationId::mint("test:design:evaluation#cube").expect("identity grammar"),
        scope: ir.design.scopes[0].definition.clone(),
        configuration: None,
        of: Evaluated::State(Position::After(Member::Feature(
            FeatureId::mint(EXTRUDE).expect("identity grammar"),
        ))),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Empty,
            delta: StateDelta {
                records: sketch_and_extrude(blind(), false).geometry,
                ..StateDelta::default()
            },
        }),
        native_ref: None,
    }
}

#[test]
fn a_history_true_join_names_its_reason_on_its_own_entry_with_no_held_state() {
    let (ir, second) = joined_after_the_cube(false);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert!(decoded.geometry.bodies.is_empty(), "{:#?}", report.losses);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the writer writes it without its state: the current state stands after the timeline marker"
        )),
        "{missing}"
    );
    assert!(
        missing.contains(&format!(
            "{second} (the writer writes it without its state: it changes a body of the state before it"
        )) && missing.contains("DR-1054"),
        "{missing}"
    );
}

#[test]
fn a_history_true_join_after_a_held_state_writes_that_state_and_its_body() {
    let (ir, second) = joined_after_the_cube(true);
    let (report, decoded, bytes) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (rollback, members) = decoded_rollback(&decoded);
    assert_eq!(rollback, Some(Rollback::Before(members[3].clone())));
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(decoded.geometry.faces.len(), 6);
    let extrudes = tails(&decoded)
        .into_iter()
        .filter(|tail| tail.0 == "Extrude")
        .map(|tail| (tail.2, tail.3, tail.4, tail.5))
        .collect::<Vec<_>>();
    assert_eq!(
        extrudes,
        [(false, false, Some(3), Some(2)), (true, true, None, None)]
    );
    assert_eq!(
        history_chain(&bytes),
        [(3, -1, 1, 0, vec![(0, 2, true)]), (2, 0, -1, 1, Vec::new()),]
    );
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(!missing.contains(EXTRUDE), "{missing}");
    assert!(
        missing.contains(&format!(
            "{second} (the writer writes it without its state: it changes a body of the state before it"
        )) && missing.contains("DR-1054"),
        "{missing}"
    );
    let omitted = loss(
        &report,
        &F3dLossCode::WriterGeometryOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(
        omitted.contains("test:design:evaluation#cube") && omitted.contains("#second-"),
        "{omitted}"
    );
}

/// [`with_a_second_extrude`] with a new body whose faces lie on the cube's
/// surfaces where the two have the same surface, so that the two bodies
/// share those carriers. Returns the design and the number of shared
/// surfaces.
fn sharing_carriers() -> (CadIr, usize) {
    use std::collections::BTreeMap;
    let (mut ir, _) = with_a_second_extrude(Boolean::NewBody);
    let content = |surface: &cadmpeg_ir::geometry::Surface| {
        let mut value = json::to_value(surface).expect("a surface serializes");
        value
            .as_object_mut()
            .expect("a surface object")
            .remove("id");
        value
    };
    let (cube, prism): (Vec<_>, Vec<_>) = ir
        .geometry
        .surfaces
        .iter()
        .partition(|surface| !surface.id.as_str().contains("#second-"));
    let shared = prism
        .iter()
        .filter_map(|surface| {
            let on = cube.iter().find(|cube| content(cube) == content(surface))?;
            Some((surface.id.clone(), on.id.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    for face in &mut ir.geometry.faces {
        if let Some(on) = shared.get(&face.surface) {
            face.surface = on.clone();
        }
    }
    ir.geometry
        .surfaces
        .retain(|surface| !shared.contains_key(&surface.id));
    ir.finalize();
    (ir, shared.len())
}

#[test]
fn a_carrier_that_two_bodies_share_is_in_the_state_of_the_first() {
    let (ir, shared) = sharing_carriers();
    assert!(shared > 0, "the prism shares no surface with the cube");
    for form in [
        MissingStateForm::GeometryTrue,
        MissingStateForm::HistoryTrue,
    ] {
        let (report, decoded, bytes) = write_bytes_as(&ir, form);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude", "sketch", "extrude"],
            "{:#?}",
            report.losses
        );
        assert_eq!(decoded.geometry.bodies.len(), 2);
        assert_eq!(decoded.geometry.surfaces.len(), 12 - shared);
        assert_eq!(
            history_chain(&bytes),
            [
                (4, -1, 2, 0, vec![(0, 2, true)]),
                (2, 2, -1, 1, Vec::new()),
                (3, 0, 1, 2, vec![(2, 2, true)]),
            ]
        );
        let residual = loss(&report, "export/record.residual-unavailable").join("\n");
        assert!(!residual.contains("ASM record"), "{residual}");
        each_board_inserts_the_records_of_its_body(&decoded, &bytes);
    }
}

#[test]
fn a_state_whose_result_body_is_not_written_names_its_feature_and_body() {
    use cadmpeg_ir::ids::BodyId;
    let body = |name: &str| BodyId::mint(format!("test:model:body#{name}")).expect("a body id");
    let feature =
        |name: &str| FeatureId::mint(format!("test:model:feature#{name}")).expect("a feature id");
    let (cube, prism) = (body("cube"), body("prism"));
    let (first, second) = (feature("first"), feature("second"));
    let written = [&prism, &cube];
    assert_eq!(
        crate::writer::generate::design::state_bodies(
            [(&first, &cube), (&second, &prism)],
            &written
        )
        .expect("both bodies are written"),
        [1, 0]
    );
    let error = crate::writer::generate::design::state_bodies(
        [(&first, &cube), (&second, &prism)],
        &[&cube],
    )
    .expect_err("the prism is not written");
    assert_eq!(error.feature, second);
    let error = error.error.to_string();
    assert!(
        error.contains(second.as_str()) && error.contains(prism.as_str()),
        "{error}"
    );
}

/// [`with_a_second_extrude`] with new bodies and the aligned distance `Gap`
/// of 10 mm between the lines `l1` and `l3` of the second square. Returns
/// the design, the second extrude and its result body.
fn dimensioned_second_extrude() -> (CadIr, FeatureId, String) {
    use cadmpeg_ir::design::ops::sketch::{
        DistanceAxis, Operand, SketchConstraint, SketchRelation,
    };
    use cadmpeg_ir::design::options::NativeOptions;
    let (mut ir, second) = with_a_second_extrude(Boolean::NewBody);
    let operand = |name: &str| Operand::Entity(super::super::sketch::entity_id("second", name));
    ir.design.sketch_constraints.push(SketchConstraint {
        id: super::super::sketch::constraint_id("second", "gap"),
        sketch: sketch_id("second"),
        relation: SketchRelation::Distance {
            a: operand("l1"),
            b: operand("l3"),
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: Dimension::Parameter(parameter_id("gap")),
        },
        name: None,
        active: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    });
    ir.design.parameters.push(parameter(
        "gap",
        "Gap",
        neutral(number(SIDE, Some(UnitSymbol::Mm))),
        Some(SIDE),
    ));
    ir.finalize();
    let body = ir.design.scopes[0].current[0]
        .bodies
        .last()
        .expect("the second body")
        .to_string();
    (ir, second, body)
}

/// Whether `decoded` holds a distance dimension bound to the parameter
/// `Gap`.
fn has_gap(decoded: &CadIr) -> bool {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Kind;
    decoded.model.sketch_constraints.iter().any(|constraint| {
        matches!(
            constraint.definition.kind(),
            Kind::Distance { parameter, .. }
                if decoded
                    .model
                    .parameters
                    .iter()
                    .any(|row| &row.id == parameter && row.name == "Gap")
        )
    })
}

/// A history failure drops the geometry items before the sketch
/// dimensions. Geometry-true writes the current bodies as the Base Feature.
/// History-true ends the run at the feature whose result body is not
/// written: the item before it keeps its state and its body, and the
/// feature follows the timeline marker. The feature whose result body is
/// not written takes the failure in its entry.
#[test]
fn a_history_failure_drops_the_items_first_and_keeps_the_sketch_dimensions() {
    let (ir, second, body) = dimensioned_second_extrude();
    let history = format!(
        "the writer cannot write its history state (not implemented yet: the history state of {second} holds its result body {body}, which the B-rep stream does not write)"
    );
    for form in [
        MissingStateForm::GeometryTrue,
        MissingStateForm::HistoryTrue,
    ] {
        let (report, decoded, _) = write_bytes_as(&ir, form);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude", "sketch", "extrude"],
            "{:#?}",
            report.losses
        );
        assert!(has_gap(&decoded), "{:#?}", report.losses);
        super::super::super::HIDE_NEXT_RESULT_BODY.with(|hide| hide.set(true));
        let (report, decoded, bytes) = write_bytes_as(&ir, form);
        assert!(has_gap(&decoded), "{form:?}: {:#?}", report.losses);
        let missing = loss(&report, EVALUATION_MISSING).join("\n");
        match form {
            MissingStateForm::GeometryTrue => {
                assert_eq!(
                    kinds(&decoded),
                    ["sketch", "sketch", "base"],
                    "{:#?}",
                    report.losses
                );
                assert_eq!(decoded.geometry.bodies.len(), 2);
                for entry in [
                    format!(
                        "{EXTRUDE} (the Base Feature holds it: the writer cannot write {second} with its state"
                    ),
                    format!("{second} (the Base Feature holds it: {history}"),
                ] {
                    assert!(missing.contains(&entry), "{entry}\n{missing}");
                }
            }
            MissingStateForm::HistoryTrue => {
                assert_eq!(
                    kinds(&decoded),
                    ["sketch", "extrude", "sketch", "extrude"],
                    "{:#?}",
                    report.losses
                );
                let (rollback, members) = decoded_rollback(&decoded);
                assert_eq!(rollback, Some(Rollback::Before(members[3].clone())));
                assert_eq!(decoded.geometry.bodies.len(), 1);
                assert_eq!(decoded.geometry.faces.len(), 6);
                assert_eq!(
                    history_chain(&bytes),
                    [(3, -1, 1, 0, vec![(0, 2, true)]), (2, 0, -1, 1, Vec::new())]
                );
                assert!(!missing.contains(&format!("{EXTRUDE} (")), "{missing}");
                assert!(
                    missing.contains(&format!(
                        "{second} (the writer writes it without its state: {history})"
                    )),
                    "{missing}"
                );
            }
        }
    }
}
