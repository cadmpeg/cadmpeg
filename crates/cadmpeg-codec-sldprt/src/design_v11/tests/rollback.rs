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
    assert_eq!(timeline_rollback(&marked.ir), Some(&Rollback::End));
}
