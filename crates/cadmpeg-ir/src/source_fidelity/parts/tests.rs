// SPDX-License-Identifier: Apache-2.0
//! Literal STEP part framing and atomic shared recovery controls.

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension, View,
};
use cadmpeg_core::CodecError;

use super::{DecodeParts, PartFraming, PartLevel, PartOutput, PartSpec, PartState};
use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::provenance::CodecFormat;
use crate::topology::Point;
use crate::{NativeUnknownRecord, SourceFidelity};

fn entity_spec(view: View<'_>) -> PartSpec {
    // Appendix C step.simple_entity_instance: #, instance number, = and ;.
    let start = cadmpeg_core::decode::u64_from_index(view.start());
    let end = cadmpeg_core::decode::u64_from_index(view.end());
    PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("table level"),
        vec![start..start + 3, end - 1..end],
    )
}

fn insert_point(
    ctx: &DecodeContext<'_>,
    stage: &mut PartOutput,
    view: View<'_>,
    id: &str,
) -> Result<(), CodecError> {
    let start = view
        .window()
        .windows(5)
        .position(|bytes| bytes == b"1,2,3")
        .expect("literal point coordinates")
        + view.start();
    let coordinate = |offset| -> Result<f64, CodecError> {
        let value = view
            .read_at(start + offset, 1)
            .ok_or_else(|| CodecError::malformed("point coordinate missing"))?[0];
        Ok(f64::from(value - b'0'))
    };
    let position = FinitePoint3::new(Point3::new(coordinate(0)?, coordinate(2)?, coordinate(4)?))
        .expect("literal finite coordinates");
    stage.insert_model(
        ctx,
        Point::new(id.try_into().expect("point identity"), position, None),
    )
}

#[test]
fn generated_levels_match_the_unchanged_appendix_c_names_and_framing() {
    let table = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/part-framing.tsv"
    ));
    let rows = table
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), super::levels::LEVELS.len());
    for row in rows {
        let name = format!("{}.{}", row[0], row[1]);
        let level = PartLevel::parse(&name).expect("every actual table row");
        assert_eq!(level.as_str(), name);
        assert_eq!(
            level.framing(),
            match row[3] {
                "located" => PartFraming::Located,
                "terminated" => PartFraming::Terminated,
                "sequential" => PartFraming::Sequential,
                _ => panic!("Appendix C framing vocabulary"),
            }
        );
    }
    assert!(PartLevel::parse("step.invented_failure_record").is_none());
    assert!(serde_json::from_str::<PartLevel>("\"step.invented_failure_record\"").is_err());
}

#[test]
fn given_up_part_has_one_exact_source_home_and_successful_sibling_is_unchanged() {
    let source = b"#1=UNKNOWN(255);#2=CARTESIAN_POINT('',(1,2,3));";
    let split = source
        .iter()
        .position(|byte| *byte == b';')
        .expect("first terminator")
        + 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let bad = root.child(0, split).expect("failed part");
    let good = root.child(split, source.len()).expect("sibling");
    let mut output = PartOutput::new(CodecFormat::Step);
    assert!(ctx
        .decode_part(&mut output, bad, entity_spec(bad), |_| Err::<(), _>(
            CodecError::NotImplemented("UNKNOWN entity family".into())
        ))
        .expect("recoverable part")
        .is_none());
    assert!(ctx
        .decode_part(&mut output, good, entity_spec(good), |stage| {
            ctx.record("step:model:point#kept", good, || {
                insert_point(&ctx, stage, good, "step:model:point#kept")
            })
        })
        .expect("good sibling")
        .is_some());
    assert_eq!(output.model().points.len(), 1);
    assert_eq!(
        output.model().points[0].position().get(),
        Point3::new(1.0, 2.0, 3.0)
    );
    let unknowns = output
        .native()
        .namespace("step")
        .expect("namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("only identity and links");
    assert_eq!(unknowns.len(), 1);
    let id = unknowns[0].id.as_str();
    assert!(unknowns[0].links.is_empty());
    assert_eq!(
        output
            .fidelity()
            .retained_record(id)
            .expect("one home")
            .data(),
        Some(&source[..split])
    );
    assert_eq!(output.fidelity().retained_records().len(), 1);
    assert_eq!(output.fidelity().record_layout().len(), 1);
    assert!(output.fidelity().record_layout()[id].reads().is_empty());
    // No event-driven class marking: the B1 partition finalizer owns this.
    assert!(output.fidelity().unrepresented().is_empty());
    assert!(output
        .fidelity()
        .retained_record(id)
        .expect("unclassified source home")
        .untyped()
        .is_empty());
    assert_eq!(output.fidelity().parts().len(), 2);
    let failed = &output.fidelity().parts()[id];
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert_eq!(
        failed.content_ranges(),
        [3..cadmpeg_core::decode::u64_from_index(split - 1)]
    );
    assert_eq!(output.losses().len(), 1);
    assert_eq!(
        output.losses()[0].code.to_string(),
        "decode/record.unreadable"
    );
    assert_eq!(output.losses()[0].severity, crate::report::Severity::Error);
    assert!(output.losses()[0].message.contains("UNKNOWN entity family"));
    let captures = ctx.take_record_captures().expect("successful reads only");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "step:model:point#kept");
    let wire = serde_json::to_value(output.fidelity()).expect("wire");
    assert_eq!(
        serde_json::from_value::<SourceFidelity>(wire).expect("strict current wire"),
        *output.fidelity()
    );
}

#[test]
fn failed_parent_covers_reached_children_and_discards_child_records_stubs_and_losses() {
    let source = b"DATA;#1=CARTESIAN_POINT('',(1,2,3));#2=UNKNOWN(255);BAD;ENDSEC;";
    let first_end = source[5..]
        .iter()
        .position(|byte| *byte == b';')
        .expect("first entity terminator")
        + 6;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let good = root.child(5, first_end).expect("good child");
    let second_end = source[first_end..]
        .iter()
        .position(|byte| *byte == b';')
        .expect("second entity terminator")
        + first_end
        + 1;
    let bad = root.child(first_end, second_end).expect("bad child");
    let mut remaining = root
        .child(second_end, source.len() - 7)
        .expect("uncovered section content");
    let parent = PartSpec::new(
        PartLevel::parse("step.data_section").expect("table level"),
        vec![
            0..5,
            cadmpeg_core::decode::u64_from_index(source.len() - 7)
                ..cadmpeg_core::decode::u64_from_index(source.len()),
        ],
    );
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(&mut output, root, parent, |stage| {
        ctx.decode_part(stage, good, entity_spec(good), |child| {
            ctx.record("step:model:point#staged", good, || {
                insert_point(&ctx, child, good, "step:model:point#staged")
            })
        })?;
        ctx.decode_part(stage, bad, entity_spec(bad), |_| {
            Err::<(), _>(CodecError::NotImplemented("unknown child".into()))
        })?;
        assert_eq!(remaining.read_remaining(), Some(b"BAD;".as_slice()));
        Err::<(), _>(CodecError::malformed(
            "remaining section content is damaged",
        ))
    })
    .expect("parent recovery");
    assert!(output.model().points.is_empty());
    let unknowns = output
        .native()
        .namespace("step")
        .expect("namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stubs");
    assert_eq!(unknowns.len(), 1);
    let parent_id = &unknowns[0].id;
    assert_eq!(output.fidelity().retained_records().len(), 1);
    assert_eq!(
        output
            .fidelity()
            .retained_record(parent_id.as_str())
            .expect("parent home")
            .data(),
        Some(source.as_slice())
    );
    assert_eq!(
        output.fidelity().record_layout().keys().collect::<Vec<_>>(),
        [parent_id.as_str()]
    );
    assert_eq!(output.fidelity().parts().len(), 3);
    assert_eq!(
        output.fidelity().parts()[parent_id].state(),
        &PartState::GivenUp {}
    );
    for (id, child) in output.fidelity().parts() {
        if id != parent_id {
            assert_eq!(
                child.state(),
                &PartState::Covered {
                    by: parent_id.clone()
                }
            );
            assert_eq!(child.parent(), Some(parent_id));
        }
    }
    assert_eq!(output.losses().len(), 1);
    assert!(output.losses()[0]
        .message
        .contains("step:model:point#staged"));
    assert!(!output.losses()[0].message.contains("unknown child"));
    assert!(ctx
        .take_record_captures()
        .expect("failed-parent reads discarded")
        .is_empty());
    let wire = serde_json::to_value(output.fidelity()).expect("part tree wire");
    assert_eq!(
        serde_json::from_value::<SourceFidelity>(wire).expect("covered ancestor admitted"),
        *output.fidelity()
    );
}

#[test]
fn framing_only_part_retains_exact_bytes_and_has_no_content_ranges() {
    let source = b"#1=;";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(&mut output, root, entity_spec(root), |_| {
        Err::<(), _>(CodecError::malformed("missing entity content"))
    })
    .expect("part recovery");
    assert!(output
        .fidelity()
        .parts()
        .values()
        .next()
        .expect("part")
        .content_ranges()
        .is_empty());
    assert_eq!(
        output
            .fidelity()
            .retained_records()
            .values()
            .next()
            .expect("retained bytes")
            .data(),
        Some(source.as_slice())
    );
}

#[test]
fn part_identity_is_address_deterministic_and_independent_of_failure_text() {
    let source = b"#1=UNKNOWN(1);";
    let decode = |error: &str| {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
        let mut output = PartOutput::new(CodecFormat::Step);
        ctx.decode_part(&mut output, root, entity_spec(root), |_| {
            Err::<(), _>(CodecError::malformed(error))
        })
        .expect("recover");
        output
            .fidelity()
            .parts()
            .keys()
            .next()
            .expect("part")
            .clone()
    };
    assert_eq!(decode("first diagnostic"), decode("different diagnostic"));
}

#[test]
fn io_and_session_limits_escape_without_unknown_stubs_or_part_failures() {
    for session in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"#1=UNKNOWN(1);", &arena, &DecodePolicy::default())
                .expect("root");
        let mut output = PartOutput::new(CodecFormat::Step);
        let error = ctx
            .decode_part(&mut output, root, entity_spec(root), |_| {
                if session {
                    Err::<(), _>(ctx.refuse_input_limit(u64::MAX, "synthetic session refusal"))
                } else {
                    Err(CodecError::Io(std::io::Error::other("source unavailable")))
                }
            })
            .expect_err("fatal error escapes");
        assert!(
            matches!(error, CodecError::Io(_))
                || matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
        );
        assert!(output.fidelity().parts().is_empty());
        assert!(output.fidelity().retained_records().is_empty());
        assert!(output.native().0.is_empty());
        assert!(output.losses().is_empty());
    }
}

#[test]
fn a_part_limit_still_yields_one_home_and_allows_the_next_part() {
    let source = b"#1=UNKNOWN(1);";
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(&mut output, root, entity_spec(root), |_| {
        ctx.begin_expand(ExpandSpec::Exact(2)).map(drop)
    })
    .expect("part limit");
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(output.fidelity().retained_records().len(), 1);
    assert_eq!(output.losses().len(), 1);
    assert!(output.losses()[0].message.contains("resource limit"));
    assert!(output.losses()[0].message.contains("DecompressedBytes"));
    assert_eq!(
        ctx.charge_entities(1, "next independent entity")
            .map(|()| ResourceDimension::Entities)
            .expect("later work"),
        ResourceDimension::Entities
    );
}

#[test]
fn strict_part_tree_wire_refuses_unknown_keys_bad_ranges_missing_parents_and_cover_cycles() {
    let source = b"#1=UNKNOWN(1);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(&mut output, root, entity_spec(root), |_| {
        Err::<(), _>(CodecError::malformed("damaged"))
    })
    .expect("recover");
    let id = output
        .fidelity()
        .parts()
        .keys()
        .next()
        .expect("part")
        .as_str()
        .to_owned();
    let wire = serde_json::to_value(output.fidelity()).expect("wire");
    let mut extra = wire.clone();
    extra["parts"][&id]["unknown"] = serde_json::json!(1);
    assert!(serde_json::from_value::<SourceFidelity>(extra).is_err());
    let mut bad_range = wire.clone();
    bad_range["parts"][&id]["framing_bytes"][0]["end"] = serde_json::json!(1000);
    assert!(serde_json::from_value::<SourceFidelity>(bad_range).is_err());
    let mut missing = wire.clone();
    missing["parts"][&id]["parent"] = serde_json::json!("step:source:part#missing");
    assert!(serde_json::from_value::<SourceFidelity>(missing).is_err());
    let mut cycle = wire;
    cycle["parts"][&id]["parent"] = serde_json::json!(id);
    assert!(serde_json::from_value::<SourceFidelity>(cycle).is_err());
}

#[test]
fn part_state_arms_keep_their_tag_spelling_and_refuse_unknown_keys() {
    for (state, tag) in [
        (PartState::Decoded {}, "decoded"),
        (PartState::GivenUp {}, "given_up"),
    ] {
        let wire = serde_json::json!({ "state": tag });
        assert_eq!(serde_json::to_value(&state).expect("state wire"), wire);
        assert_eq!(
            serde_json::from_value::<PartState>(wire).expect("exact state arm"),
            state
        );
        assert!(
            serde_json::from_value::<PartState>(serde_json::json!({ "state": tag, "unknown": 1 }))
                .is_err(),
            "{tag} must refuse an undeclared key"
        );
    }
}

fn tree_node(parent: Option<usize>, state: PartState) -> super::PartRecord {
    super::PartRecord {
        level: PartLevel::parse("step.simple_entity_instance").expect("table level"),
        stream: "file".into(),
        offset: 0,
        byte_len: 1,
        parent: parent.map(tree_id),
        state,
        framing_bytes: Vec::new(),
    }
}

fn tree_id(index: usize) -> crate::ids::UnknownId {
    crate::ids::UnknownId::try_from(format!("step:source:part#{index:05}")).expect("part id")
}

#[test]
fn part_tree_check_is_linear_on_a_deep_chain_and_names_only_given_up_ancestors() {
    // A chain p0 <- p1 <- ... <- p19999, with p0 given up and every other node
    // covered by it. Each node's ancestry is the whole prefix of the chain.
    let depth = 20_000;
    let mut tree = std::collections::BTreeMap::new();
    tree.insert(tree_id(0), tree_node(None, PartState::GivenUp {}));
    for index in 1..depth {
        tree.insert(
            tree_id(index),
            tree_node(Some(index - 1), PartState::Covered { by: tree_id(0) }),
        );
    }
    super::validate_tree(&tree).expect("covered chain under one given-up root");

    // A covering id must be a proper ancestor: a sibling or a descendant fails.
    let mut sibling = std::collections::BTreeMap::new();
    sibling.insert(tree_id(0), tree_node(None, PartState::Decoded {}));
    sibling.insert(tree_id(1), tree_node(Some(0), PartState::GivenUp {}));
    sibling.insert(
        tree_id(2),
        tree_node(Some(0), PartState::Covered { by: tree_id(1) }),
    );
    assert!(super::validate_tree(&sibling).is_err());
    let mut descendant = std::collections::BTreeMap::new();
    descendant.insert(
        tree_id(0),
        tree_node(None, PartState::Covered { by: tree_id(1) }),
    );
    descendant.insert(tree_id(1), tree_node(Some(0), PartState::GivenUp {}));
    assert!(super::validate_tree(&descendant).is_err());
    let mut own = std::collections::BTreeMap::new();
    own.insert(
        tree_id(0),
        tree_node(None, PartState::Covered { by: tree_id(0) }),
    );
    assert!(super::validate_tree(&own).is_err());

    // A covering ancestor that is not given up fails.
    let mut decoded = std::collections::BTreeMap::new();
    decoded.insert(tree_id(0), tree_node(None, PartState::Decoded {}));
    decoded.insert(
        tree_id(1),
        tree_node(Some(0), PartState::Covered { by: tree_id(0) }),
    );
    assert!(super::validate_tree(&decoded).is_err());

    // A long cycle with no root fails.
    let mut cycle = std::collections::BTreeMap::new();
    for index in 0..depth {
        cycle.insert(
            tree_id(index),
            tree_node(Some((index + 1) % depth), PartState::Decoded {}),
        );
    }
    assert!(super::validate_tree(&cycle).is_err());
}
