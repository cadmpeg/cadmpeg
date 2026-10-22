// SPDX-License-Identifier: Apache-2.0
use crate::ids::Identity;
use crate::provenance::CodecFormat;
use crate::source_fidelity::parts::{
    DecodeParts, PartDependencyPruning, PartLevel, PartOutput, PartSpec,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Record {
    id: Identity,
    required: Option<Identity>,
    listed: Vec<Identity>,
    label: String,
    value: u64,
}
rewrite_record!(Record, []; {id, required, listed, label, value});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Object {
    target: Identity,
    label: String,
}
rewrite_record!(Object, []; {target, label});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Objects {
    id: Identity,
    objects: Vec<Object>,
}
rewrite_record!(Objects, []; {id, objects});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
fn record(number: u8, required: Option<u8>, listed: &[u8]) -> Record {
    Record {
        id: id(number),
        required: required.map(id),
        listed: listed.iter().copied().map(id).collect(),
        label: "step:native:record#3".into(),
        value: 42,
    }
}
fn failure(ctx: &DecodeContext<'_>, output: &mut PartOutput, extent: View<'_>) {
    let start = cadmpeg_core::decode::u64_from_index(extent.start());
    let end = cadmpeg_core::decode::u64_from_index(extent.end());
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
        vec![start..start + 3, end - 1..end],
    );
    ctx.decode_part(output, extent, spec, |staged| {
        staged.insert_native(ctx, "dependency_fixture", record(3, None, &[]))?;
        Err::<(), _>(CodecError::NotImplemented("UNKNOWN entity family".into()))
    })
    .expect("only the unsupported entity gives up");
}

#[test]
fn prior_siblings_and_required_cycles_drop_transitively_but_lists_and_labels_survive() {
    // #1 -> #7 -> #3; #7 also needs #1. #2 needs #1. #4 lists #2 and #3.
    let bytes = b"#1=USE(#7);#2=USE(#1);#3=UNKNOWN(7);#4=LIST((#2,#5,#3,#5),'step:native:record#3');#5=KNOWN(42);#7=OBJECTS((#3,#1));";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(&ctx, "dependency_fixture", record(1, Some(7), &[]))
        .expect("prior #1");
    output
        .insert_native(&ctx, "dependency_fixture", record(2, Some(1), &[]))
        .expect("prior #2");
    output
        .insert_native(&ctx, "dependency_fixture", record(4, None, &[2, 5, 3, 5]))
        .expect("list owner");
    output
        .insert_native(&ctx, "dependency_fixture", record(5, None, &[]))
        .expect("unrelated");
    output
        .insert_native(
            &ctx,
            "dependency_objects",
            Objects {
                id: id(7),
                objects: vec![
                    Object {
                        target: id(3),
                        label: "preserve ordinary labels".into(),
                    },
                    Object {
                        target: id(1),
                        label: "cycle".into(),
                    },
                ],
            },
        )
        .expect("objects have required fields inside Vec");
    let unrelated = output
        .native()
        .namespace("step")
        .expect("namespace")
        .arenas()["dependency_fixture"]
        .iter()
        .find(|value| value.id() == id(5).as_str())
        .expect("unrelated record")
        .clone();
    let start = bytes
        .windows(3)
        .position(|window| window == b"#3=")
        .expect("failed entity offset");
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == b';')
        .expect("failed entity end")
        + start
        + 1;
    let extent = root.child(start, end).expect("terminated entity");
    failure(&ctx, &mut output, extent);
    let expected = vec![
        id(1).to_string(),
        id(2).to_string(),
        id(3).to_string(),
        id(7).to_string(),
    ];
    assert_eq!(
        output.prune_dependencies(&ctx).expect("typed closure"),
        PartDependencyPruning::Complete {
            dropped_records: expected.clone()
        }
    );
    let namespace = output.native().namespace("step").expect("namespace");
    let records = namespace
        .arena_as::<Record>("dependency_fixture")
        .expect("real registered owner");
    assert_eq!(records, [record(4, None, &[5, 5]), record(5, None, &[])]);
    assert!(namespace.arenas()["dependency_objects"].is_empty());
    assert_eq!(namespace.arenas()["dependency_fixture"][1], unrelated);
    assert_eq!(output.losses().len(), 1);
    let dropped = format!(
        "dropped={}",
        serde_json::to_string(&expected).expect("sorted actual IDs")
    );
    assert!(output.losses()[0].message.ends_with(&dropped));
    assert_eq!(output.fidelity().retained_records().len(), 1);
    assert_eq!(
        output
            .fidelity()
            .retained_records()
            .values()
            .next()
            .expect("one source home")
            .data(),
        Some(extent.window())
    );
    assert_eq!(
        output.prune_dependencies(&ctx).expect("idempotent closure"),
        PartDependencyPruning::Complete {
            dropped_records: expected
        }
    );
}

#[test]
fn fixed_reference_list_owner_is_not_formed_and_drops_with_its_reference() {
    // #215 §4.5 rule 1: a record that cannot be formed without a dropped
    // record is not formed; rule 2 drops it too. Pair #8 holds #3 in a fixed
    // two-entry list.
    let bytes = b"#3=UNKNOWN(7);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "dependency_pairs",
            Pair {
                id: id(8),
                references: [id(3), id(5)],
            },
        )
        .expect("fixed pair");
    failure(&ctx, &mut output, root);
    let expected = vec![id(3).to_string(), id(8).to_string()];
    assert_eq!(
        output.prune_dependencies(&ctx).expect("typed closure"),
        PartDependencyPruning::Complete {
            dropped_records: expected.clone()
        }
    );
    let namespace = output.native().namespace("step").expect("namespace");
    assert!(namespace
        .arena_as::<Pair>("dependency_pairs")
        .expect("real registered owner")
        .is_empty());
    assert_eq!(output.losses().len(), 1);
    let dropped = format!(
        "dropped={}",
        serde_json::to_string(&expected).expect("sorted actual IDs")
    );
    assert!(output.losses()[0].message.ends_with(&dropped));
    assert_eq!(output.fidelity().retained_records().len(), 1);
}

#[test]
fn records_that_need_an_unformed_record_drop_and_lists_lose_only_its_entry() {
    // #215 §4.5 rules 1-2, applied until nothing changes: #3 gives up, Pair #8
    // cannot be formed without #3, #9 needs #8, and #10 lists #8.
    let bytes = b"#3=UNKNOWN(7);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "dependency_pairs",
            Pair {
                id: id(8),
                references: [id(3), id(5)],
            },
        )
        .expect("fixed pair");
    for value in [record(9, Some(8), &[]), record(10, None, &[8, 5])] {
        output
            .insert_native(&ctx, "dependency_fixture", value)
            .expect("pair users");
    }
    failure(&ctx, &mut output, root);
    let expected = vec![id(3).to_string(), id(8).to_string(), id(9).to_string()];
    assert_eq!(
        output.prune_dependencies(&ctx).expect("typed closure"),
        PartDependencyPruning::Complete {
            dropped_records: expected.clone()
        }
    );
    let namespace = output.native().namespace("step").expect("namespace");
    assert_eq!(
        namespace
            .arena_as::<Record>("dependency_fixture")
            .expect("real registered owner"),
        [record(10, None, &[5])]
    );
    let dropped = format!(
        "dropped={}",
        serde_json::to_string(&expected).expect("sorted actual IDs")
    );
    assert!(output.losses()[0].message.ends_with(&dropped));
}

#[test]
fn model_registry_closure_preserves_every_unaffected_cube_carrier_value() {
    use crate::schema::EntitySchema;
    let cube = crate::examples::unit_cube().expect("independent valid document");
    let body = cube.model.bodies[0].clone();
    let body_id = body.id.to_string();
    let source = b"#1=SOLID_BODY(BAD_EXTENSION);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).expect("root");
    let mut output = PartOutput::new(CodecFormat::Step);
    macro_rules! stage_cube {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(for value in cube.model.$field.clone() { if value.identity() != body_id { output.insert_model(&ctx, value).expect("unaffected independently admitted record"); } })* };
    }
    crate::document::arena_registry!(stage_cube);
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
        vec![
            0..3,
            cadmpeg_core::decode::u64_from_index(source.len() - 1)
                ..cadmpeg_core::decode::u64_from_index(source.len()),
        ],
    );
    ctx.decode_part(&mut output, root, spec, |staged| {
        staged.insert_model(&ctx, body)?;
        Err::<(), _>(CodecError::NotImplemented(
            "BAD_EXTENSION in body definition".into(),
        ))
    })
    .expect("body part recovery");
    assert!(matches!(
        output
            .prune_dependencies(&ctx)
            .expect("typed model closure"),
        PartDependencyPruning::Complete { .. }
    ));
    assert!(output.model().bodies.is_empty());
    assert!(output.model().regions.is_empty());
    assert!(output.model().shells.is_empty());
    assert!(output.model().faces.is_empty());
    assert!(output.model().loops.is_empty());
    assert!(output.model().coedges.is_empty());
    assert_eq!(output.model().points, cube.model.points);
    assert_eq!(output.model().vertices, cube.model.vertices);
    assert_eq!(output.model().edges, cube.model.edges);
    assert_eq!(output.model().curves, cube.model.curves);
    assert_eq!(output.model().surfaces, cube.model.surfaces);
    assert_eq!(output.losses().len(), 1);
    assert!(output.losses()[0].message.contains(&body_id));
}
