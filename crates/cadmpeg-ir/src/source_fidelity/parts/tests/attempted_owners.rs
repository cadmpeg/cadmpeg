// SPDX-License-Identifier: Apache-2.0
//! Failed genuine parser scopes seed loss accounting and dependency closure.

use super::*;

#[test]
fn failed_parser_owner_is_named_without_a_provisional_native_record() {
    let bytes = b"#1=CARTESIAN_POINT('',(1,X,3));";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let missing = "step:model:point#1";
    output
        .insert_model(
            &ctx,
            crate::topology::Vertex {
                id: "step:model:vertex#1".try_into().unwrap(),
                point: missing.try_into().unwrap(),
                tolerance: None,
            },
        )
        .unwrap();
    let value = ctx
        .decode_part(&mut output, root, entity_spec(root), |_stage| {
            ctx.record(missing, root, || {
                let offset = bytes.iter().position(|byte| *byte == b'X').unwrap();
                assert_eq!(root.read_at(offset, 1), Some(b"X".as_slice()));
                Err::<(), _>(CodecError::malformed("point coordinate is not numeric"))
            })
        })
        .unwrap();
    assert!(value.is_none());
    assert!(output.model().points.is_empty());
    assert!(output.losses()[0].message.contains("step:model:point#1"));
    let (model, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(model.vertices.is_empty());
    assert!(losses[0].message.contains("step:model:vertex#1"));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        native.namespace("step").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert_eq!(fidelity.parts().len(), 1);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn outer_part_rollback_collapses_inner_loss_and_names_all_actual_owner_attempts() {
    let bytes = b"#1=UNKNOWN();#2=UNKNOWN();";
    let split = bytes.iter().position(|byte| *byte == b';').unwrap() + 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let level = PartLevel::parse("step.data_section").unwrap();
    ctx.decode_part(
        &mut output,
        root,
        PartSpec::new(level, Vec::new()),
        |stage| {
            let child = root.child(0, split).unwrap();
            ctx.decode_part(stage, child, entity_spec(child), |_child| {
                ctx.record("step:native:record#1", child, || {
                    Err::<(), _>(CodecError::malformed("child value"))
                })
            })?;
            ctx.record(
                "step:native:record#2",
                root.child(split, bytes.len()).unwrap(),
                || Err::<(), _>(CodecError::malformed("parent value")),
            )
        },
    )
    .unwrap();
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("step:native:record#1"));
    assert!(losses[0].message.contains("step:native:record#2"));
    assert_eq!(
        native.namespace("step").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert_eq!(fidelity.parts().len(), 2);
    assert_eq!(fidelity.retained_records().len(), 1);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn successful_prior_owner_is_not_a_failed_siblings_dependency_seed() {
    let bytes = b"#1=CARTESIAN_POINT('',(1,2,3));#2=CARTESIAN_POINT('',(1,X,3));";
    let split = bytes.iter().position(|byte| *byte == b';').unwrap() + 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let good_id = "step:model:point#1";
    let bad_id = "step:model:point#2";
    let good = root.child(0, split).unwrap();
    ctx.decode_part(&mut output, good, entity_spec(good), |stage| {
        ctx.record(good_id, good, || insert_point(&ctx, stage, good, good_id))
    })
    .unwrap();
    let bad = root.child(split, bytes.len()).unwrap();
    ctx.decode_part(&mut output, bad, entity_spec(bad), |_stage| {
        ctx.record(bad_id, bad, || {
            Err::<(), _>(CodecError::malformed("invalid point"))
        })
    })
    .unwrap();
    let (model, _, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(model.points.len(), 1);
    assert_eq!(model.points[0].id.as_str(), good_id);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(bad_id));
    assert!(!losses[0].message.contains(good_id));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), good_id);
}
