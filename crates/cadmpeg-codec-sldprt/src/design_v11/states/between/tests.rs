// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::design::evaluation::{EvaluationStatus, HeldState, StateBase};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::names::NameEntry;
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_parasolid::deltas::replay::MainLine;
use cadmpeg_parasolid::records::node_type::{GROUP, POINT};
use cadmpeg_parasolid::records::{DeltasRecords, PrimaryRecords};
use cadmpeg_parasolid::test_support::replay::{partition, DeltasStream, Mark, TestNode};

use super::{
    evaluation_id, wanted, Between, End, Held, Read, Reader, Site, Sources, Timeline, Unread,
    Wanted,
};
use crate::container::{CompoundStream, ContainerScan, Section, SolidWorksEnvelopeScan};
use crate::design_v11::names::{Names, Persistent};
use crate::parasolid::{stream_header, ExtractedStream};
use crate::records::{FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use crate::rollback::{MarkRecord, SiteRecords};

const C: Mark = Mark {
    xmt: 100,
    preceding: 101,
    identifier: 9,
};
const B: Mark = Mark {
    xmt: 101,
    preceding: 102,
    identifier: 6,
};
const A: Mark = Mark {
    xmt: 102,
    preceding: 1,
    identifier: 3,
};

/// The state at the current mark C, which the main line C, B, A replays
/// back from.
fn current() -> [TestNode; 3] {
    [
        TestNode::point(10, 11, 3),
        TestNode::point(11, 1, 1),
        TestNode::group(12, 1),
    ]
}

/// The main line from C back to the initial mark A.
fn main_line() -> Vec<u8> {
    DeltasStream::new(C)
        .delta(
            &[TestNode::point(13, 1, 7)],
            &[(11, POINT)],
            &[TestNode::point(10, 1, 1)],
        )
        .marks(&[B])
        .delta(&[], &[(10, POINT), (12, GROUP), (13, POINT)], &[])
        .marks(&[A])
        .terminator()
        .finish()
}

/// The main line from C back to B, whose step removes a node the state does
/// not hold.
fn refused_line() -> Vec<u8> {
    DeltasStream::new(C)
        .delta(&[], &[(14, POINT)], &[])
        .marks(&[Mark { preceding: 1, ..B }])
        .terminator()
        .finish()
}

/// The timeline Boss, Cut, Fillet: three features that change the state.
fn timeline_features() -> Vec<Feature> {
    ["Boss", "Cut", "Fillet"]
        .into_iter()
        .map(|name| Feature {
            id: FeatureId::mint(format!("sldprt:model:feature#{name}")).unwrap(),
            name: Some(name.to_owned()),
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Unresolved {
                    family: OperationFamily::Extrude,
                },
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: Some(format!("record-{name}")),
        })
        .collect()
}

/// The history records of Boss, Cut and Fillet, with the object ids 100,
/// 101 and 102. The record of the feature at `imported` is an imported body.
fn history(imported: Option<usize>) -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: ["Boss", "Cut", "Fillet"]
            .into_iter()
            .zip(100..)
            .enumerate()
            .map(|(index, (name, object))| crate::records::Feature {
                id: format!("record-{name}"),
                parent: "history".into(),
                xml_tag: "Extrusion".into(),
                tree_parent: None,
                source_id: FeatureSource::from_value(object),
                ordinal: u32::try_from(index).unwrap(),
                name: name.into(),
                kind: "BossExtrude".into(),
                input_class: Some(
                    if imported == Some(index) {
                        crate::design_v11::operation::BASE_BODY_CLASS
                    } else {
                        "moExtrusion_c"
                    }
                    .into(),
                ),
                suppressed: false,
                parameters: BTreeMap::default(),
                dimension_properties: BTreeMap::default(),
                properties: BTreeMap::default(),
                text: None,
                content: Vec::new(),
            })
            .collect(),
    }
}

/// The serialized names of Boss, Cut and Fillet, each before its mark
/// record.
const NAMES: [(u64, u32); 3] = [(10, 100), (20, 101), (30, 102)];

/// The mark records of Boss at A, Cut at B and Fillet at the current mark C.
fn records() -> Vec<MarkRecord> {
    [(15, A), (25, B), (35, C)]
        .into_iter()
        .map(|(at, mark)| MarkRecord {
            at,
            mark: mark.identifier,
        })
        .collect()
}

/// The table that names the one body `body` of a state.
fn named(body: &str) -> BTreeMap<EntityRef, NameEntry> {
    BTreeMap::from([(
        EntityRef::Body(BodyId::mint(format!("sldprt:brep:body#{body}")).unwrap()),
        NameEntry {
            name: None,
            native: None,
        },
    )])
}

fn with_context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

fn definition() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}

fn after(feature: &Feature) -> Position {
    Position::After(Member::Feature(feature.id.clone()))
}

/// The states the replay of `stream` states for `wanted`, and the features
/// whose state it decodes, in order. The current state is after Fillet and
/// names the body `fillet`; the state after feature `k` names the body `k`.
fn replayed(
    stream: &[u8],
    features: &[Feature],
    histories: &[FeatureHistory],
    wanted: &mut Wanted,
) -> (Between, Vec<usize>) {
    with_context(|ctx| {
        let by_record = HashMap::new();
        let names = Names::new(
            Persistent {
                faces: &[],
                bodies: &[],
            },
            &[],
            &by_record,
        );
        let definition = definition();
        let reader = Reader {
            ctx,
            timeline: Timeline::new(features, histories),
            names: &names,
            definition: &definition,
        };
        let at = after(&features[2]);
        let latest = named("fillet");
        let geometry = Topology::default();
        let end = End {
            configuration: None,
            at: &at,
            base: StateBase::Current(at.clone()),
            status: EvaluationStatus::Current,
            bodies: &[],
            held: Held::Current {
                geometry: &geometry,
                names: &latest,
            },
        };
        let records = DeltasRecords::read(stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        let primary_bytes = partition(&current());
        let primary = PrimaryRecords::read(&primary_bytes).unwrap().unwrap();
        let mut read_features = Vec::new();
        let read = |transmit: &[u8], feature: usize| -> Result<Result<Read, String>, CodecError> {
            assert!(!transmit.is_empty());
            read_features.push(feature);
            Ok(Ok(Read {
                held: HeldState {
                    records: Topology::default(),
                    names: named(&feature.to_string()),
                },
                annotations: Annotations::default(),
                losses: vec![cadmpeg_ir::unknown::part::GivenUpRecord::new(
                    Identity::new("sldprt:brep:edge#4").unwrap(),
                    "unread edge use",
                )
                .loss()],
            }))
        };
        let mut between = Between::default();
        reader
            .replay(&end, &line, &primary, wanted, read, &mut between)
            .unwrap();
        (between, read_features)
    })
}

/// The states [`wanted`] joins in the timeline `features` with `histories`.
fn joined(features: &[Feature], histories: &[FeatureHistory], records: &[MarkRecord]) -> Wanted {
    let stream = main_line();
    let deltas = DeltasRecords::read(&stream).unwrap();
    let line = MainLine::read(&deltas).unwrap();
    wanted(
        &Timeline::new(features, histories),
        &line,
        records,
        &NAMES,
        Some(2),
    )
}

#[test]
fn each_earlier_feature_has_its_stored_state_based_on_the_next_later_state() {
    let features = timeline_features();
    let histories = [history(None)];
    let mut wanted = joined(&features, &histories, &records());
    assert_eq!(wanted.states, BTreeMap::from([(6, 1), (3, 0)]));
    let (between, read) = replayed(&main_line(), &features, &histories, &mut wanted);
    assert_eq!(read, [1, 0]);
    assert_eq!(wanted.unread, Unread::default());
    assert!(wanted.unread.note(None).is_none());
    let cut = evaluation_id(None, &features[1].id);
    let [after_cut, after_boss] = between.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", between.evaluations);
    };
    assert_eq!(after_cut.id, cut);
    assert_eq!(after_cut.of, Evaluated::State(after(&features[1])));
    assert_eq!(after_cut.status, EvaluationStatus::Current);
    assert_eq!(after_cut.native_ref.as_deref(), Some("PMARK 6"));
    let content = after_cut.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Current(after(&features[2])));
    let mut names = named("1")
        .into_iter()
        .map(|(entity, entry)| (entity, Some(entry)))
        .collect::<BTreeMap<_, _>>();
    names.extend(named("fillet").into_keys().map(|entity| (entity, None)));
    assert_eq!(content.delta.names, names);
    assert_eq!(after_boss.id, evaluation_id(None, &features[0].id));
    assert_eq!(after_boss.of, Evaluated::State(after(&features[0])));
    assert_eq!(after_boss.native_ref.as_deref(), Some("PMARK 3"));
    assert_eq!(
        after_boss.content.as_ref().unwrap().base,
        StateBase::Evaluation(cut)
    );
    assert_eq!(between.losses.len(), 2);
    assert!(between.losses[0].message.contains(after_cut.id.as_str()));
    assert!(between.losses[1].message.contains(after_boss.id.as_str()));
    let key = Identity::from(after_cut.id.clone()).key();
    assert!(between.losses[0]
        .message
        .contains(&format!("sldprt:brep:edge#4@state/{key}")));
}

#[test]
fn a_feature_without_a_main_line_mark_record_has_no_state_and_no_loss() {
    let features = timeline_features();
    let histories = [history(None)];
    let mut records = records();
    records[1].mark = 77;
    let mut wanted = joined(&features, &histories, &records);
    assert_eq!(wanted.states, BTreeMap::from([(3, 0)]));
    let (between, read) = replayed(&main_line(), &features, &histories, &mut wanted);
    assert_eq!(read, [0]);
    let [after_boss] = between.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", between.evaluations);
    };
    assert_eq!(after_boss.of, Evaluated::State(after(&features[0])));
    assert_eq!(
        after_boss.content.as_ref().unwrap().base,
        StateBase::Current(after(&features[2]))
    );
    assert!(wanted.unread.note(None).is_none());
}

#[test]
fn an_absent_record_does_not_assign_a_feature_to_a_mark() {
    let features = timeline_features();
    let histories = [history(None)];
    let mut records = records();
    records.remove(1);
    let wanted = joined(&features, &histories, &records);
    assert_eq!(wanted.states, BTreeMap::from([(3, 0)]));
    assert_eq!(wanted.unread, Unread::default());
}

#[test]
fn an_unjoined_current_mark_is_already_read() {
    let features = timeline_features();
    let mut histories = [history(None)];
    histories[0].features[2].source_id = FeatureSource::from_value(103);
    let wanted = joined(&features, &histories, &records());
    assert_eq!(wanted.states, BTreeMap::from([(6, 1), (3, 0)]));
    assert_eq!(wanted.unread, Unread::default());
}

#[test]
fn a_shared_mark_states_the_result_after_its_last_feature() {
    let features = timeline_features();
    let histories = [history(None)];
    let mut records = records();
    records[0].mark = B.identifier;
    let wanted = joined(&features, &histories, &records);
    assert_eq!(wanted.states, BTreeMap::from([(6, 1)]));
    assert_eq!(wanted.unread, Unread::default());
}

#[test]
fn a_record_can_join_the_only_earlier_mark_of_a_partition() {
    let features = timeline_features();
    let features = &features[1..];
    let histories = [history(None)];
    let deltas = DeltasStream::new(C)
        .delta(&[], &[(10, POINT), (11, POINT), (12, GROUP)], &[])
        .marks(&[Mark { preceding: 1, ..B }])
        .terminator()
        .finish();
    let stream = |payload: Vec<u8>| ExtractedStream {
        header: stream_header(&payload).unwrap().unwrap(),
        payload,
        offset: 0,
    };
    let partition_path = "Contents/Config-0-Partition";
    let scan = ContainerScan {
        source_image: &[],
        version: 0,
        blocks: Vec::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams: vec![CompoundStream {
            path: cadmpeg_ir::StreamName::try_from(partition_path.to_owned()).unwrap(),
            directory_id: 1,
            start_sector: 0,
            payload: Vec::new(),
            decoded_payload: None,
            ps_streams: vec![stream(partition(&current())), stream(deltas)],
            ps_probe_capped: false,
        }],
        solidworks: SolidWorksEnvelopeScan::default(),
        given_up: Vec::new(),
        noncanonical: Vec::new(),
    };
    let lane = "sldprt:feature-input:resolved-features#2";
    let mut lanes: [FeatureInputLane; 1] = [cadmpeg_core::serde_erase::json::from_value(
        cadmpeg_core::json!({"id": lane, "native_payload": ""}),
    )
    .unwrap()];
    lanes[0].names = NAMES[1..]
        .iter()
        .enumerate()
        .map(|(index, &(offset, object))| FeatureInputName {
            id: format!("{lane}/name/{index}"),
            parent: lane.to_owned(),
            ordinal: u32::try_from(index).unwrap(),
            offset,
            object_id: Some(ObjectId::try_from(object).unwrap()),
            value: features[index].name.clone().unwrap(),
        })
        .collect();
    let marks = BTreeMap::from([(
        partition_path.to_owned(),
        Ok(SiteRecords {
            configuration: "Contents/Config-0".to_owned(),
            records: records()[1..].to_vec(),
            inferred: false,
            lane: Some(lane.to_owned()),
        }),
    )]);

    with_context(|ctx| {
        let by_record = HashMap::new();
        let names = Names::new(
            Persistent {
                faces: &[],
                bodies: &[],
            },
            &[],
            &by_record,
        );
        let reader = Reader {
            ctx,
            timeline: Timeline::new(features, &histories),
            names: &names,
            definition: &definition(),
        };
        let at = after(&features[1]);
        let end = End {
            configuration: None,
            at: &at,
            base: StateBase::Current(at.clone()),
            status: EvaluationStatus::Current,
            bodies: &[],
            held: Held::Current {
                geometry: &Topology::default(),
                names: &BTreeMap::new(),
            },
        };
        let sources = Sources {
            scan: &scan,
            histories: &histories,
            lanes: &lanes,
            marks: &marks,
        };
        let site = Site {
            section: Section::Compound(&scan.compound_streams[0]),
            qualifier: None,
        };
        let mut between = Between::default();
        let unread = reader.site(&sources, &site, &end, &mut between).unwrap();
        assert_eq!(unread, Unread::default());
        let [evaluation] = between.evaluations.as_slice() else {
            panic!("one historical state");
        };
        assert_eq!(evaluation.of, Evaluated::State(after(&features[0])));
        let content = evaluation.content.as_ref().unwrap();
        assert_eq!(content.base, StateBase::Current(at.clone()));
        assert_eq!(content.delta.records, Topology::default());
        assert!(between.losses.is_empty());
    });
}

#[test]
fn an_imported_feature_has_no_state_and_no_loss() {
    let features = timeline_features();
    let histories = [history(Some(0))];
    let wanted = joined(&features, &histories, &records());
    assert_eq!(wanted.states, BTreeMap::from([(6, 1)]));
    assert_eq!(wanted.unread, Unread::default());
}

#[test]
fn a_refused_replay_leaves_the_states_unread_and_names_them_in_the_loss() {
    let features = timeline_features();
    let histories = [history(None)];
    let mut wanted = Wanted {
        states: BTreeMap::from([(6, 0)]),
        unread: Unread::default(),
    };
    let (between, read) = replayed(&refused_line(), &features, &histories, &mut wanted);
    assert!(read.is_empty());
    assert!(between.evaluations.is_empty());
    assert!(wanted.states.is_empty());
    let note = wanted.unread.note(None).unwrap();
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::HistoryStatesUnread.kind()
    );
    assert!(note.message.contains("Boss"), "{}", note.message);
    assert!(
        note.message.contains("the replay refuses"),
        "{}",
        note.message
    );
}
