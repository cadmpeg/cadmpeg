// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, LimitScope};
use cadmpeg_core::CodecError;

use crate::provenance::{CodecFormat, SourceProvenance};
use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec, PartState};
use crate::SourceFidelity;

fn source<'a>(ctx: &DecodeContext<'a>, root: cadmpeg_core::decode::View<'a>) -> SourceFidelity {
    let root = ctx.name_stream(root, "member").expect("member stream");
    let mut output = PartOutput::new(CodecFormat::F3d);
    let spec = |name, framing| PartSpec::new(PartLevel::parse(name).expect("Appendix C"), framing);
    ctx.decode_part(&mut output, root, spec("f3d.manifest", vec![]), |staged| {
        ctx.decode_part(staged, root, spec("f3d.manifest_field", vec![0..4]), |_| {
            Err::<(), _>(CodecError::malformed("invalid manifest version bad"))
        })?;
        Err::<(), _>(CodecError::malformed("manifest could not be decoded"))
    })
    .expect("bounded recovery");
    let crate::source_fidelity::parts::PartFinalization::Ready(output) =
        output.into_parts(ctx).expect("dependency finalization")
    else {
        panic!("shared stubs have registered typed owners");
    };
    output.into_parts().2
}

#[test]
fn part_tree_uses_the_new_stream_address_and_preserves_covering_ancestry() {
    let bytes = b"\x03\0\0\0bad";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("root");
    let mut fidelity = source(&ctx, root);
    let original = fidelity.clone();
    let ids = fidelity
        .rescope_part_tree(&ctx, CodecFormat::F3d, "f3d:xref/a/")
        .expect("composition");
    // Independent SHA-256 of the specified length-prefixed level, stream and offset.
    let parent = "f3d:source:part#c3de81cb0dc2403fb27d9b7538c8e5d8f6f7e7f1ac57c7ebfa8c8cd2477e1db1";
    let child = "f3d:source:part#377823785f130da6572420cf170a0fffb27125a8a581f78809e60959d1dc8019";
    assert_eq!(fidelity.parts().len(), 2);
    assert!(fidelity.parts().contains_key(parent));
    assert_eq!(fidelity.parts()[parent].stream(), "f3d:xref/a/member");
    assert_eq!(fidelity.parts()[parent].state(), &PartState::GivenUp {});
    assert_eq!(
        fidelity.parts()[child].parent().map(|id| id.as_str()),
        Some(parent)
    );
    assert_eq!(
        fidelity.parts()[child].state(),
        &PartState::Covered {
            by: parent.try_into().expect("canonical ID")
        }
    );
    assert_eq!(fidelity.parts()[child].framing_bytes(), [0..4]);
    assert_eq!(
        (
            fidelity.parts()[parent].offset(),
            fidelity.parts()[parent].byte_len()
        ),
        (0, 7)
    );
    assert_eq!(fidelity.retained_records(), original.retained_records());
    assert_eq!(fidelity.record_layout(), original.record_layout());
    ids.validate_rescoped_tree(fidelity.parts())
        .expect("complete target map");
    for (id, node) in original.parts() {
        let mapped = ids.get(id.as_str()).expect("every source node mapped");
        assert_eq!(fidelity.parts()[mapped].level(), node.level());
    }
    let nested = fidelity
        .rescope_part_tree(&ctx, CodecFormat::F3d, "f3d:xref/b/")
        .expect("nested composition");
    assert_eq!(
        nested.get(parent),
        Some("f3d:source:part#452056074b9d0a56e0c7bba54c5127e77ee48795e9ad953b9e85738e7e2eaf6a")
    );
    assert_eq!(
        fidelity.parts()
            ["f3d:source:part#452056074b9d0a56e0c7bba54c5127e77ee48795e9ad953b9e85738e7e2eaf6a"]
            .stream(),
        "f3d:xref/b/f3d:xref/a/member"
    );
}

#[test]
fn wrong_address_and_codec_refuse_without_changing_the_tree() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"\x03\0\0\0bad", &arena, &DecodePolicy::service())
            .expect("root");
    let fidelity = source(&ctx, root);
    let mut wrong_codec = fidelity.clone();
    assert!(wrong_codec
        .rescope_part_tree(&ctx, CodecFormat::Step, "prefix/")
        .is_err());
    assert_eq!(wrong_codec, fidelity);
    let mut wire = serde_json::to_value(&fidelity).expect("wire");
    let id = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.parent().is_none())
        .expect("root")
        .0
        .as_str();
    wire["parts"][id]["offset"] = serde_json::json!(1);
    let mut wrong_address: SourceFidelity =
        serde_json::from_value(wire).expect("structurally valid changed address");
    let before = wrong_address.clone();
    assert!(wrong_address
        .rescope_part_tree(&ctx, CodecFormat::F3d, "prefix/")
        .is_err());
    assert_eq!(wrong_address, before);
}

#[test]
fn part_tree_append_collision_and_session_refusal_are_atomic() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"\x03\0\0\0bad", &arena, &DecodePolicy::service())
            .expect("root");
    let fidelity = source(&ctx, root);
    let mut destination = SourceFidelity::default();
    destination
        .append_part_tree(fidelity.parts().clone())
        .expect("tree");
    let before = destination.clone();
    assert!(destination
        .append_part_tree(fidelity.parts().clone())
        .is_err());
    assert_eq!(destination, before);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let limited_arena = DecodeArena::new();
    let (limited, _) =
        DecodeContext::from_root_bytes(b"source", &limited_arena, &policy).expect("root");
    let error = destination
        .rescope_part_tree(&limited, CodecFormat::F3d, "prefix/")
        .expect_err("session ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
    );
    assert_eq!(destination, before);
}

#[test]
fn report_provenance_composition_preserves_format_offset_and_tag() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::service()).expect("root");
    let source =
        SourceProvenance::in_stream("f3d", "member".to_owned().try_into().expect("stream"), 4)
            .with_tag("field");
    let mapped = source
        .with_stream_prefix(&ctx, "f3d:xref/a/")
        .expect("mapped provenance");
    assert_eq!(
        serde_json::to_value(mapped).expect("wire"),
        serde_json::json!({"format":"f3d", "stream":"f3d:xref/a/member", "offset":4, "tag":"field"})
    );
    let mapped_root = SourceProvenance::root("f3d", 2)
        .with_stream_prefix(&ctx, "f3d:xref/a/")
        .expect("member root");
    assert_eq!(
        serde_json::to_value(mapped_root).expect("wire"),
        serde_json::json!({"format":"f3d", "stream":"f3d:xref/a/", "offset":2})
    );
}
