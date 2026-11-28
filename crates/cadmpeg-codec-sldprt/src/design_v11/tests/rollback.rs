// SPDX-License-Identifier: Apache-2.0
//! Rollback-mark decode tests of the v11 design of a part.
#![allow(clippy::unwrap_used)]

use super::*;

fn marked_feature(name: &str, suppressed: bool) -> cadmpeg_ir::design::feature::Feature {
    cadmpeg_ir::design::feature::Feature {
        id: crate::history::project::neutral_feature_id(name).unwrap(),
        name: Some(name.to_owned()),
        suppressed: Some(Dimension::Literal(suppressed)),
        definition: cadmpeg_ir::design::feature::FeatureDefinition {
            operation: super::super::operation::origin(),
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

/// A rolled-back reading whose records the features `after` and `later`
/// hold.
fn rolled_back(after: &str, later: &[&str]) -> crate::rollback::Reading {
    crate::rollback::Reading::RolledBack {
        after: after.to_owned(),
        later: later.iter().map(|name| (*name).to_owned()).collect(),
        recorded: std::iter::once(&after)
            .chain(later)
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

/// An end reading whose last record `last` holds and whose other records
/// `earlier` hold.
fn end(last: &str, earlier: &[&str]) -> crate::rollback::Reading {
    crate::rollback::Reading::End {
        last: last.to_owned(),
        recorded: std::iter::once(&last)
            .chain(earlier)
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

#[test]
fn stored_marks_state_how_many_members_are_built() {
    use crate::rollback::{Reading, Unread};
    let features = [
        marked_feature("Sketch1", false),
        marked_feature("Boss-Extrude1", false),
        marked_feature("Sketch2", false),
        marked_feature("Cut-Extrude1", false),
        marked_feature("Fillet1", true),
    ];
    assert_eq!(
        super::super::stated(
            &features,
            end("Fillet1", &["Boss-Extrude1", "Cut-Extrude1"])
        ),
        Ok((5, None))
    );
    assert_eq!(
        super::super::stated(
            &features,
            rolled_back("Boss-Extrude1", &["Cut-Extrude1", "Fillet1"])
        ),
        Ok((
            2,
            Some(Unread::UnrecordedFeature {
                name: "Sketch2".to_owned()
            })
        )),
        "Sketch2 sets no mark, so the bar can stand before or after it"
    );
    assert_eq!(
        super::super::stated(&features[1..], rolled_back("Boss-Extrude1", &["Fillet1"])),
        Ok((
            4,
            Some(Unread::UnrecordedFeature {
                name: "Sketch2".to_owned()
            })
        )),
        "a suppressed later feature leaves the tree built through its end"
    );
    assert_eq!(
        super::super::stated(
            &features[1..],
            rolled_back("Sketch2", &["Cut-Extrude1", "Fillet1"])
        ),
        Ok((2, None))
    );
    assert_eq!(
        super::super::stated(&features, rolled_back("Cut-Extrude1", &["Fillet1"])),
        Ok((5, None)),
        "suppressed later features leave the tree built through its end"
    );
    assert_eq!(
        super::super::stated(&features[..3], end("Boss-Extrude1", &[])),
        Ok((
            3,
            Some(Unread::UnrecordedFeature {
                name: "Sketch2".to_owned()
            })
        )),
        "an unbuilt Sketch2 after the last record sets no mark either"
    );
    assert_eq!(
        super::super::stated(&features, end("Boss-Extrude2", &[])),
        Err(Unread::FeatureUnresolved {
            name: "Boss-Extrude2".to_owned()
        })
    );
    assert_eq!(
        super::super::stated(&features, rolled_back("Boss-Extrude2", &["Fillet1"])),
        Err(Unread::FeatureUnresolved {
            name: "Boss-Extrude2".to_owned()
        })
    );
    assert_eq!(
        super::super::stated(&features, rolled_back("Cut-Extrude1", &["Boss-Extrude1"])),
        Err(Unread::FeatureOutOfOrder {
            name: "Boss-Extrude1".to_owned()
        })
    );
    assert_eq!(
        super::super::stated(&features, Reading::Unread(Unread::NoMarkClass)),
        Err(Unread::NoMarkClass)
    );
}

#[test]
fn a_rolled_back_timeline_states_the_member_after_its_bar() {
    let id = super::super::container_id(cadmpeg_ir::identity_key!("timeline"));
    let members = ["Sketch1", "Boss-Extrude1", "Sketch2"]
        .map(|name| Member::Feature(marked_feature(name, false).id));
    assert_eq!(
        super::super::built_through(&id, &members, 2),
        (
            Rollback::Before(members[2].clone()),
            Position::After(members[1].clone())
        )
    );
    assert_eq!(
        super::super::built_through(&id, &members, 3),
        (Rollback::End, Position::After(members[2].clone()))
    );
    assert_eq!(
        super::super::built_through(&id, &members, 0),
        (
            Rollback::Before(members[0].clone()),
            Position::Start(id.clone())
        )
    );
}

const MARKED_KEYWORDS: &[u8] = br#"<Keywords Name="Marked">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
    </Extrusion>
</Keywords>"#;

/// [`MARKED_KEYWORDS`] with a sketch after the extrusion.
const TRAILING_SKETCH_KEYWORDS: &[u8] = br#"<Keywords Name="Marked">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
    </Extrusion>
    <Sketch Name="Sketch2" Type="ProfileFeature" id="9"/>
</Keywords>"#;

/// The current mark `test_pmarks::prefix` writes.
const CURRENT_MARK: u32 = 0x1234_5678;

/// The rollback losses of a decoded part.
struct Marked {
    ir: CadIr,
    unread: Vec<String>,
    inferred: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// A part whose partition section holds a deltas transmit at
/// [`CURRENT_MARK`], whose `Config-0` declares `class` at the record's class
/// index, and whose one feature record `Boss` holds, naming `record_mark`.
fn part_with_marks(keywords: &[u8], class: &str, record_mark: u32) -> Marked {
    use cadmpeg_parasolid::test_support::test_pmarks;

    use crate::loss::SldprtLossCode;
    use crate::test_support::carchive::{config_object_stream_declaring, held_mark_record_stream};
    use crate::test_support::container::outer_header;
    use crate::test_support::parasolid::parasolid_with_body;

    let mut partition =
        parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    partition.extend(test_pmarks::complete(test_pmarks::prefix(1)));
    let mut source = outer_header();
    source.extend(make_block(0x20, "Contents/Config-0-Partition", &partition));
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    source.extend(make_block(
        0x20,
        "Contents/Config-0",
        &config_object_stream_declaring(class),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-0-ResolvedFeatures",
        &held_mark_record_stream(record_mark, "Boss"),
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = &result.report().losses;
    let unread = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::HistoryRollbackUnread.kind())
        .map(|loss| loss.message.clone())
        .collect();
    let inferred = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::HistoryRollbackMarkClassInferred.kind())
        .cloned()
        .collect();
    Marked {
        ir: result.ir().clone(),
        unread,
        inferred,
    }
}

fn timeline_rollback(ir: &CadIr) -> Option<&Rollback> {
    let [Container::Ordered { rollback, .. }] = ir.design.containers.as_slice() else {
        panic!("one ordered container: {:#?}", ir.design.containers);
    };
    rollback.as_ref()
}

#[test]
fn a_current_mark_at_the_last_feature_mark_reads_the_end() {
    let marked = part_with_marks(MARKED_KEYWORDS, "moPMarkRecord_c", CURRENT_MARK);
    assert_eq!(marked.unread, Vec::<String>::new());
    assert!(marked.inferred.is_empty(), "{:#?}", marked.inferred);
    assert_eq!(timeline_rollback(&marked.ir), Some(&Rollback::End));
}

#[test]
fn a_feature_with_no_mark_record_after_the_last_record_is_reported_unread() {
    let marked = part_with_marks(TRAILING_SKETCH_KEYWORDS, "moPMarkRecord_c", CURRENT_MARK);
    assert_eq!(
        marked.unread,
        [
            "The rollback position of the feature tree is not read (feature Sketch2 after the \
          feature at the current mark holds no mark record); the timeline's rollback is not \
          stated."
        ]
    );
    assert_eq!(timeline_rollback(&marked.ir), None);
    let [scope] = marked.ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    assert_eq!(
        scope.current[0].at,
        Position::After(Member::Container(scope.roots[0].clone()))
    );
}

#[test]
fn a_current_mark_no_feature_names_is_reported_unread() {
    let marked = part_with_marks(MARKED_KEYWORDS, "moPMarkRecord_c", 5);
    assert_eq!(
        marked.unread,
        [format!(
            "The rollback position of the feature tree is not read (current mark \
             {CURRENT_MARK} is the mark of no feature); the timeline's rollback is not \
             stated."
        )]
    );
    assert_eq!(timeline_rollback(&marked.ir), None);
}

#[test]
fn a_mark_class_the_class_map_does_not_name_is_noted_as_inferred() {
    let marked = part_with_marks(MARKED_KEYWORDS, "moOtherRecord_c", CURRENT_MARK);
    let [note] = marked.inferred.as_slice() else {
        panic!("one inferred-class note: {:#?}", marked.inferred);
    };
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Info);
    assert!(note.message.contains("Contents/Config-0"), "{note:#?}");
    assert_eq!(
        marked.unread.len(),
        1,
        "no feature holds the record at the current mark: {:#?}",
        marked.unread
    );
    assert_eq!(timeline_rollback(&marked.ir), None);
}

#[derive(Clone, Copy)]
enum SphereTip {
    Changed,
    Deleted,
    Alternate,
    AlternateSplit,
    AlternateWithoutFeatureLane,
    LocalBody,
}

/// The partition before the last feature contains a loopless sphere. The
/// current partition either changes its radius or removes its nodes.
fn part_with_earlier_sphere(tip_kind: SphereTip) -> cadmpeg_ir::codec::DecodeResult {
    SldprtCodec
        .decode(
            &mut Cursor::new(earlier_sphere_fixture(tip_kind)),
            &DecodeOptions::default(),
        )
        .unwrap()
}

fn earlier_sphere_fixture(tip_kind: SphereTip) -> Vec<u8> {
    use cadmpeg_parasolid::test_support::replay::{DeltasStream, Mark};

    use crate::test_support::carchive::config_object_stream_declaring;
    use crate::test_support::container::outer_header;
    use crate::test_support::parasolid::sequence::{loopless_nodes, sequence_of, status_node, F};

    let alternate = matches!(
        tip_kind,
        SphereTip::Alternate | SphereTip::AlternateSplit | SphereTip::AlternateWithoutFeatureLane
    );
    let delete_body = matches!(tip_kind, SphereTip::Deleted | SphereTip::LocalBody);
    let tip = Mark {
        xmt: 100,
        identifier: 43,
        preceding: 101,
    };
    let earlier = Mark {
        xmt: 101,
        identifier: 17,
        preceding: 1,
    };
    let mut nodes = loopless_nodes(53);
    let mut prefix = DeltasStream::embedded(tip).finish();
    let mut deltas = DeltasStream::embedded(tip);
    let prefix_len = prefix.len();
    let (mut partition, _) = if delete_body {
        for (kind, index, fields) in &nodes {
            status_node(&mut prefix, *kind, *index, fields);
        }
        sequence_of(&[])
    } else {
        let (kind, index, fields) = &mut nodes[4];
        let original = fields.clone();
        fields[8] = F::R(0.011);
        deltas = deltas.terminator().map(&[]);
        prefix = deltas.finish();
        status_node(&mut prefix, *kind, *index, &original);
        deltas = DeltasStream::embedded(tip);
        sequence_of(&nodes)
    };
    let mut tail = if delete_body {
        deltas.terminator().map(&[]).terminator()
    } else {
        deltas.terminator()
    }
    .marks(&[earlier])
    .terminator()
    .finish();
    prefix.extend(tail.drain(prefix_len..));
    partition.extend(prefix);

    let mut resolved = 9_u32.to_le_bytes().to_vec();
    resolved.extend([0, 0]);
    for (name, id, mark) in [("Boss", 7, earlier), ("Later", 9, tip)] {
        resolved.extend(resolved_feature_classes_with_ids(&[(
            "moExtrusion_c",
            name,
            id,
        )]));
        resolved.extend(0x8007_u16.to_le_bytes());
        for word in [mark.identifier, 1, 0] {
            resolved.extend(word.to_le_bytes());
        }
    }
    let mut source = if alternate {
        sldprt_with_body(&triangle_body())
    } else {
        outer_header()
    };
    if alternate {
        source.extend(make_block(
            0x43, "Contents/SolidWorks",
            br#"<swSolidWorks><swModel swConfigurationName="Default"/><swConfigurationList><swConfiguration swName="Default" swID="0"/><swConfiguration swName="Alternate" swID="1"/></swConfigurationList></swSolidWorks>"#,
        ));
    }
    if matches!(tip_kind, SphereTip::LocalBody) {
        let local = crate::test_support::parasolid::parasolid_with_body(
            ": TRANSMIT FILE created by modeller",
            "SCH_SW_33103_11000",
            &triangle_body(),
        );
        source.extend(make_block(
            0x22,
            "Contents/Config-0-FeatureBodies/LocalBodies",
            &local,
        ));
    }
    let configuration = format!("Contents/Config-{}", usize::from(alternate));
    source.extend(make_block(
        0x20,
        &format!("{configuration}-Partition"),
        &partition,
    ));
    if matches!(tip_kind, SphereTip::AlternateSplit) {
        let (other, _) = sequence_of(&loopless_nodes(53));
        source.extend(make_block(
            0x20,
            &format!("{configuration}-Partition"),
            &other,
        ));
    }
    let rows = if alternate {
        r#"<Configuration Name="Default" SourceIndex="0"/><Configuration Name="Alternate" SourceIndex="1"/>"#
    } else {
        ""
    };
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(
            r#"<Keywords Name="Marked">{rows}
            <Extrusion Name="Later" Type="BossExtrude" id="9"/>
            <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
        </Keywords>"#
        )
        .as_bytes(),
    ));
    source.extend(make_block(
        0x20,
        &configuration,
        &config_object_stream_declaring("moPMarkRecord_c"),
    ));
    if !matches!(tip_kind, SphereTip::AlternateWithoutFeatureLane) {
        source.extend(make_block(
            0x21,
            &format!("{configuration}-ResolvedFeatures"),
            &resolved,
        ));
    }
    native_envelope_fixture(&source)
}

#[test]
fn serialized_marks_order_the_timeline_and_replay_a_nonempty_earlier_brep() {
    use cadmpeg_ir::design::evaluation::StateBase;
    use cadmpeg_ir::design::refs::Evaluated;

    let result = part_with_earlier_sphere(SphereTip::Changed);
    let ir = result.ir();
    let boss = feature_named(ir, "Boss").id.clone();
    let later = feature_named(ir, "Later").id.clone();
    let [Container::Ordered { members, .. }] = ir.design.containers.as_slice() else {
        panic!("expected the part timeline");
    };
    assert_eq!(
        *members,
        [Member::Feature(boss.clone()), Member::Feature(later)]
    );
    let [current] = ir.design.scopes[0].current.as_slice() else {
        panic!("expected Current to hold the newer sphere");
    };
    assert_eq!(current.native_ref.as_deref(), Some("PMARK 43"));
    let [earlier] = ir.design.evaluations.as_slice() else {
        panic!(
            "expected the boss evaluation: {:#?}",
            result.report().losses
        );
    };
    assert_eq!(
        earlier.of,
        Evaluated::State(Position::After(Member::Feature(boss)))
    );
    assert_eq!(earlier.native_ref.as_deref(), Some("PMARK 17"));
    let content = earlier.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Current(current.at.clone()));
    assert_eq!(current.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(content.delta.records.surfaces.len(), 1);
    assert_ne!(content.delta.records.surfaces, ir.geometry.surfaces);
}

#[test]
fn an_empty_current_partition_replays_the_deleted_body_from_empty() {
    use cadmpeg_ir::design::evaluation::StateBase;
    use cadmpeg_ir::design::refs::Evaluated;

    let result = part_with_earlier_sphere(SphereTip::Deleted);
    let ir = result.ir();
    assert!(ir.design.scopes[0].current.is_empty());
    let [earlier] = ir.design.evaluations.as_slice() else {
        panic!("expected the deleted body: {:#?}", result.report().losses);
    };
    let boss = feature_named(ir, "Boss").id.clone();
    assert_eq!(
        earlier.of,
        Evaluated::State(Position::After(Member::Feature(boss)))
    );
    let content = earlier.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Empty);
    assert_eq!(content.delta.records.bodies.len(), 1);
    assert_eq!(content.delta.records.faces.len(), 1);
}

#[test]
fn a_nonactive_configuration_tip_keeps_its_mark_and_bases_its_earlier_state() {
    use cadmpeg_ir::design::evaluation::StateBase;

    let result = part_with_earlier_sphere(SphereTip::Alternate);
    let [tip, earlier] = result.ir().design.evaluations.as_slice() else {
        panic!(
            "expected the alternate partition's tip and earlier state: {:#?}",
            result.report().losses
        );
    };
    assert_eq!(tip.native_ref.as_deref(), Some("PMARK 43"));
    assert_eq!(earlier.native_ref.as_deref(), Some("PMARK 17"));
    assert_eq!(tip.configuration, earlier.configuration);
    let content = earlier.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Evaluation(tip.id.clone()));
    assert_eq!(content.delta.records.surfaces.len(), 1);
}

#[test]
fn the_state_chain_ceiling_refuses_the_decode_session() {
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::DecodeFailure;

    for (tip, depth) in [(SphereTip::Changed, 1), (SphereTip::Alternate, 2)] {
        let bytes = earlier_sphere_fixture(tip);
        let decode =
            |options: &DecodeOptions| SldprtCodec.decode(&mut Cursor::new(&bytes), options);
        let result = decode(&DecodeOptions::default()).unwrap();
        assert_eq!(result.ir().design.evaluations.len(), depth);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code != SldprtLossCode::HistoryStatesUnread.kind()
                && loss.code != SldprtLossCode::ConfigStateUnstated.kind()
        }));

        let mut options = DecodeOptions::default();
        options.policy.limits.max_state_chain_depth = u64::try_from(depth).unwrap();
        assert_eq!(decode(&options).unwrap(), result);
        for limit in 0..options.policy.limits.max_state_chain_depth {
            options.policy.limits.max_state_chain_depth = limit;
            let Err(failure) = decode(&options) else {
                panic!("the decode returned a document at state chain limit {limit}");
            };
            assert!(
                matches!(
                    failure,
                    DecodeFailure::Codec(CodecError::ResourceLimit(ResourceLimit {
                        scope: LimitScope::Session,
                        dimension: ResourceDimension::RecursionDepth,
                        reason: ResourceFailure::BudgetExceeded,
                        limit: maximum,
                        used,
                        additional: 1,
                        ..
                    })) if maximum == limit && used == limit
                ),
                "{failure:?}"
            );
        }
    }
}

fn assert_alternate_states_unread(tip: SphereTip, reason: &str) {
    let result = part_with_earlier_sphere(tip);
    let [evaluation] = result.ir().design.evaluations.as_slice() else {
        panic!("expected the alternate configuration tip");
    };
    assert!(evaluation.content.is_some());
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::HistoryStatesUnread.kind())
        .collect::<Vec<_>>();
    let [loss] = losses.as_slice() else {
        panic!(
            "expected a history.states-unread loss: {:#?}",
            result.report().losses
        );
    };
    assert!(loss.message.contains("Alternate"), "{loss:?}");
    assert!(loss.message.contains(reason), "{loss:?}");
}

#[test]
fn an_alternate_configuration_without_a_unique_partition_site_is_reported() {
    assert_alternate_states_unread(SphereTip::AlternateSplit, "partition site");
}

#[test]
fn an_alternate_configuration_without_a_feature_lane_is_reported() {
    assert_alternate_states_unread(SphereTip::AlternateWithoutFeatureLane, "feature records");
}

#[test]
fn an_unclaimed_empty_bookkeeping_mark_is_not_a_missing_feature_state() {
    use crate::test_support::container::outer_header;
    use crate::test_support::parasolid::sequence::{loopless_nodes, sequence_of};
    use cadmpeg_parasolid::test_support::replay::{DeltasStream, Mark};

    let nodes = loopless_nodes(53);
    let (mut partition, _) = sequence_of(&nodes);
    let created = nodes
        .iter()
        .map(|(kind, index, _)| (u32::from(*index), *kind))
        .collect::<Vec<_>>();
    let tip = Mark {
        xmt: 100,
        identifier: 43,
        preceding: 101,
    };
    let bookkeeping = Mark {
        xmt: 101,
        identifier: 17,
        preceding: 102,
    };
    let initial = Mark {
        xmt: 102,
        identifier: 3,
        preceding: 1,
    };
    partition.extend(
        DeltasStream::embedded(tip)
            .delta(&[], &created, &[])
            .marks(&[bookkeeping])
            .delta(&[], &[], &[])
            .marks(&[initial])
            .terminator()
            .finish(),
    );
    let mut source = outer_header();
    source.extend(make_block(0x20, "Contents/Config-0-Partition", &partition));
    source.extend(make_block(0x42, "Contents/Keywords", MARKED_KEYWORDS));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.ir().design.evaluations.is_empty());
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != SldprtLossCode::HistoryStatesUnread.kind()),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn an_imported_current_body_is_outside_the_empty_partitions_index_space() {
    use cadmpeg_ir::design::evaluation::StateBase;
    let result = part_with_earlier_sphere(SphereTip::LocalBody);
    let ir = result.ir();
    let [current] = ir.design.scopes[0].current.as_slice() else {
        panic!(
            "expected the local body in Current: {:#?}",
            result.report().losses
        );
    };
    assert_eq!(current.bodies.len(), 1);
    let [earlier] = ir.design.evaluations.as_slice() else {
        panic!(
            "expected the earlier partition body: {:#?}",
            result.report().losses
        );
    };
    let content = earlier.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Empty);
    assert_eq!(content.delta.records.bodies.len(), 1);
    assert_ne!(current.bodies[0], content.delta.records.bodies[0].id);
}
