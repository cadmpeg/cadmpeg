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
    // #215 rule 1: the given-up bytes are B1, so their classes enter
    // `unrepresented`. The stub links no record, so §3.1 gives both classes.
    let both = crate::document::ArenaClasses::from([
        crate::document::ArenaClass::Model,
        crate::document::ArenaClass::Design,
    ]);
    assert_eq!(output.fidelity().unrepresented(), both);
    assert_eq!(
        output
            .fidelity()
            .retained_record(id)
            .expect("classified source home")
            .untyped(),
        &both
    );
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

#[test]
fn damaged_framing_gives_up_its_part_and_the_next_part_still_decodes() {
    // #215 §4.5: a decoder gives up on the part it cannot read, decodes the
    // rest, and reports the give-up. A framing interval outside the extent is
    // damaged framing, not a whole-file refusal.
    let source = b"#1=UNKNOWN(1);#2=KNOWN(2);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let first = root.child(0, 14).expect("first instance");
    let second = root.child(14, source.len()).expect("second instance");
    let mut output = PartOutput::new(CodecFormat::Step);
    let damaged = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("table level"),
        vec![0..3, 13..20],
    );
    let mut ran = false;
    let result = ctx
        .decode_part(&mut output, first, damaged, |_| {
            ran = true;
            Ok(())
        })
        .expect("framing damage is a part give-up");
    assert_eq!(result, None);
    assert!(!ran, "content is not decoded under invalid framing");
    assert_eq!(
        ctx.decode_part(&mut output, second, entity_spec(second), |_| Ok(2))
            .expect("next part"),
        Some(2)
    );
    let given_up = output
        .fidelity()
        .parts()
        .values()
        .find(|part| part.offset() == 0)
        .expect("first part record");
    assert_eq!(given_up.state(), &PartState::GivenUp {});
    assert!(given_up.framing_bytes().is_empty());
    assert_eq!(given_up.content_ranges(), [0..14]);
    assert_eq!(output.losses().len(), 1);
    assert_eq!(output.losses()[0].code.local_code(), "record.unreadable");
    let wire = serde_json::to_value(output.fidelity()).expect("wire");
    assert_eq!(
        serde_json::from_value::<SourceFidelity>(wire).expect("admitted part tree"),
        *output.fidelity()
    );
}

#[test]
fn a_child_part_with_its_parents_identity_is_refused_not_replaced() {
    // The part id is minted from (codec, level, stream, offset). A child of the
    // same level at its parent's offset would replace the parent's tree node.
    let source = b"#1=UNKNOWN(1);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    for child_fails in [false, true] {
        let error = ctx
            .decode_part(&mut output, root, entity_spec(root), |staged| {
                ctx.decode_part(staged, root, entity_spec(root), |_| {
                    if child_fails {
                        Err(CodecError::malformed("damaged child"))
                    } else {
                        Ok(())
                    }
                })?;
                Ok(())
            })
            .expect_err("duplicate part identity");
        assert!(error
            .to_string()
            .contains("duplicate part invocation identity"));
    }
    assert!(output.fidelity().parts().is_empty());
}

/// Retained bytes charged to decode one point record `depth` parts deep, or
/// the same part nesting with no record.
fn nested_point_cost(depth: usize, with_point: bool) -> u64 {
    fn nest(
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        view: View<'_>,
        depth: usize,
        with_point: bool,
    ) -> Result<(), CodecError> {
        if depth == 0 {
            if with_point {
                let position = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .expect("literal finite coordinates");
                output.insert_model(
                    ctx,
                    Point::new(
                        "step:model:point#nested"
                            .try_into()
                            .expect("point identity"),
                        position,
                        None,
                    ),
                )?;
            }
            return Ok(());
        }
        let level = PartLevel::parse("step.simple_entity_instance").expect("table level");
        // Each level starts one byte later, so every part has its own identity.
        let inner = view
            .child(view.start() + 1, view.end())
            .expect("inner part");
        ctx.decode_part(output, view, PartSpec::new(level, Vec::new()), |stage| {
            nest(ctx, stage, inner, depth - 1, with_point)
        })?
        .ok_or_else(|| CodecError::malformed("nested part gave up"))
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"####", &arena, &DecodePolicy::default()).expect("root");
    let before = ctx.retained_bytes_available();
    let mut output = PartOutput::new(CodecFormat::Step);
    nest(&ctx, &mut output, root, depth, with_point).expect("nested parts decode");
    assert_eq!(output.model().points.len(), usize::from(with_point));
    before - ctx.retained_bytes_available()
}

#[test]
fn a_staged_record_is_charged_once_whatever_its_part_depth() {
    // The record's storage moves from each stage to its parent unchanged. It
    // is charged to the session retained budget once, at the root output.
    let at_root = nested_point_cost(0, true) - nested_point_cost(0, false);
    assert!(at_root > 0);
    for depth in [1, 3] {
        assert_eq!(
            nested_point_cost(depth, true) - nested_point_cost(depth, false),
            at_root,
            "depth {depth}"
        );
    }
}

#[test]
fn a_given_up_part_linked_to_a_neutral_owner_takes_the_owner_class() {
    // #215 rule 1 and §3.1: the classes of the given-up bytes are those of the
    // record the stub links; a neutral owner contributes its own arena class.
    let source = b"#2=CARTESIAN_POINT('',(1,2,3));#1=UNKNOWN(255);";
    let split = source
        .iter()
        .position(|byte| *byte == b';')
        .expect("first terminator")
        + 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::default()).expect("root");
    let owner = root.child(0, split).expect("owner record");
    let bad = root.child(split, source.len()).expect("failed part");
    let mut output = PartOutput::new(CodecFormat::Step);
    insert_point(&ctx, &mut output, owner, "step:model:point#owner").expect("owner");
    let given_up = ctx
        .record("step:model:point#owner", root, || {
            ctx.decode_part(&mut output, bad, entity_spec(bad), |_| {
                Err::<(), _>(CodecError::NotImplemented("UNKNOWN entity family".into()))
            })
        })
        .expect("recoverable part");
    assert!(given_up.is_none());
    let unknowns = output
        .native()
        .namespace("step")
        .expect("namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stub");
    assert_eq!(unknowns[0].links[0].as_str(), "step:model:point#owner");
    let model = crate::document::ArenaClasses::from([crate::document::ArenaClass::Model]);
    assert_eq!(output.fidelity().unrepresented(), model);
}

mod state;

mod attempted_owners;
