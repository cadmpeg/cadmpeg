// SPDX-License-Identifier: Apache-2.0
use super::PartOutput;
use crate::ids::Identity;
use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartSpec};
use crate::{CodecFormat, NativeUnknownRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    id: Identity,
    required: Option<Identity>,
    listed: Vec<Identity>,
}
rewrite_record!(Record, []; {id, required, listed});
#[derive(Debug, Serialize, Deserialize)]
struct Pair {
    id: Identity,
    references: [Identity; 2],
}
rewrite_record!(Pair, []; {id, references});

fn id(number: u8) -> Identity {
    format!("step:native:record#{number}")
        .try_into()
        .expect("identity")
}
fn fail(ctx: &DecodeContext<'_>, output: &mut PartOutput, source: View<'_>, built: bool) {
    let start = u64::try_from(source.start()).expect("source start");
    let end = u64::try_from(source.end()).expect("source end");
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
        vec![start..start + 3, end - 1..end],
    );
    ctx.decode_part(output, source, spec, |staged| {
        if built {
            staged.insert_native(
                ctx,
                "records",
                Record {
                    id: id(2),
                    required: None,
                    listed: vec![],
                },
            )?;
        }
        Err::<(), _>(CodecError::NotImplemented(
            "UNKNOWN field in the source entity".into(),
        ))
    })
    .expect("bounded unsupported entity");
}

#[test]
fn owned_handoff_runs_required_closure_and_list_pruning_without_an_advisory_call() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=ITEM(2,UNKNOWN);", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(1),
                required: Some(id(2)),
                listed: vec![],
            },
        )
        .expect("prior required user");
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(3),
                required: None,
                listed: vec![id(2), id(4)],
            },
        )
        .expect("list user");
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(4),
                required: None,
                listed: vec![],
            },
        )
        .expect("independent sibling");
    fail(&ctx, &mut output, root, true);
    let finalized = output.into_parts(&ctx).expect("real resource checks");
    let (_, native, _, losses) = finalized.into_parts();
    let records = native
        .namespace("step")
        .expect("namespace")
        .arena_as::<Record>("records")
        .expect("actual record owner");
    assert_eq!(
        records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        [id(3).as_str(), id(4).as_str()]
    );
    assert_eq!(records[0].listed, [id(4)]);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .ends_with("dropped=[\"step:native:record#1\",\"step:native:record#2\"]"));
}

#[test]
fn fixed_list_owner_of_a_dropped_record_is_not_formed_and_drops_at_handoff() {
    // #215 §4.5 rule 1: "A record that cannot be formed without such a field
    // is not formed, and rule 2 drops the records that need it." Pair #1 holds
    // #2 in a fixed two-entry list, so it cannot be formed once #2 drops.
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=ITEM(2,UNKNOWN);", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "pairs",
            Pair {
                id: id(1),
                references: [id(2), id(4)],
            },
        )
        .expect("checked fixed list");
    fail(&ctx, &mut output, root, true);
    let finalized = output.into_parts(&ctx).expect("dropped, not deferred");
    let (_, native, fidelity, losses) = finalized.into_parts();
    let namespace = native.namespace("step").expect("namespace");
    assert!(namespace
        .arena_as::<Pair>("pairs")
        .expect("actual pair owner")
        .is_empty());
    assert_eq!(
        namespace
            .arena_as::<NativeUnknownRecord>("unknowns")
            .expect("part stub")
            .len(),
        1
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .ends_with("dropped=[\"step:native:record#1\",\"step:native:record#2\"]"));
}

#[test]
fn helper_only_unknown_stub_has_its_actual_typed_owner_at_handoff() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=UNKNOWN();", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    fail(&ctx, &mut output, root, false);
    let finalized = output.into_parts(&ctx).expect("owner visitation");
    let (_, native, fidelity, losses) = finalized.into_parts();
    let records = native
        .namespace("step")
        .expect("namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("actual typed stub");
    assert_eq!(records.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(records[0].id.as_str())
            .expect("single helper home")
            .data(),
        Some(root.window())
    );
    assert_eq!(losses.len(), 1);
}
