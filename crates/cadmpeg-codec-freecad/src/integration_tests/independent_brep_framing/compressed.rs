// SPDX-License-Identifier: Apache-2.0
//! Every public golden's actual BRep levels receive bounded 00/FF controls.
use super::*;

pub(super) fn diagnose_changed_geometry(
    before: &DecodeResult,
    after: &DecodeResult,
    target: &Boundary,
    fill: u8,
) {
    let failures = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .map(|part| {
            (
                part.level().as_str(),
                part.stream(),
                part.offset(),
                part.byte_len(),
            )
        })
        .collect::<Vec<_>>();
    for edge in before
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| {
            !after
                .ir()
                .model
                .edges
                .iter()
                .any(|current| current.id == edge.id)
        })
        .take(1)
    {
        let layout = before
            .source_fidelity()
            .record_layout()
            .get(edge.id.as_str())
            .map(|layout| {
                (
                    layout.stream(),
                    layout.offset(),
                    layout.byte_len(),
                    layout.reads(),
                )
            });
        eprintln!("missing edge target={target:?} fill={fill:#x}; edge={edge:?}; ownR1={layout:?}; failures={failures:?}");
    }
    for edge in &before.ir().model.edges {
        if let Some(current) = after
            .ir()
            .model
            .edges
            .iter()
            .find(|current| current.id == edge.id)
        {
            if current != edge {
                eprintln!("changed edge target={target:?} fill={fill:#x}; before={edge:?}; after={current:?}; failures={failures:?}");
                break;
            }
        }
    }
    for mesh in &before.ir().model.tessellations {
        if let Some(current) = after
            .ir()
            .model
            .tessellations
            .iter()
            .find(|current| current.id == mesh.id)
        {
            let previous = serde_json::to_value(mesh).unwrap();
            let current = serde_json::to_value(current).unwrap();
            if previous != current {
                let summary = |value: &serde_json::Value| {
                    serde_json::json!({
                        "keys": value.as_object().unwrap().keys().collect::<Vec<_>>(),
                        "mesh_keys": value["mesh"].as_object().unwrap().keys().collect::<Vec<_>>(),
                        "kind": value["mesh"]["kind"],
                        "vertices": value["mesh"]["vertices"].as_array().map(Vec::len),
                        "triangles": value["mesh"]["triangles"].as_array().map(Vec::len),
                        "body": value["body"],
                        "faces": value["faces"],
                    })
                };
                eprintln!("changed tessellation target={target:?} fill={fill:#x}; id={}; before={}; after={}; failures={failures:?}", mesh.id, summary(&previous), summary(&current));
                break;
            }
        }
    }
}

#[test]
fn compressed_golden_brep_damage_keeps_real_member_compression_and_independent_parts() {
    use cadmpeg_test_support::golden::Harness;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures");
    let inputs = Harness::new(env!("CARGO_MANIFEST_DIR"), "FCStd", "")
        .with_fixture_dir(fixtures)
        .fixture_inputs();
    assert!(!inputs.is_empty());
    let mut compressed_members = BTreeSet::new();
    for (name, original) in inputs {
        eprintln!("public golden {name}: decode original BRep source");
        let before = decode(&original);
        let boundaries = assert_parts_agree(&original, &before);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&original, &arena, &DecodePolicy::service()).unwrap();
        let entries = zip::frame(&ctx, root, &mut Vec::new()).unwrap();
        for member in boundaries
            .iter()
            .map(|part| &part.member)
            .collect::<BTreeSet<_>>()
        {
            let entry = entries
                .iter()
                .find(|entry| entry.name().window() == member.as_bytes())
                .unwrap();
            if entry.method() == 8 {
                compressed_members.insert((name.clone(), member.clone()));
            }
        }
        // An absent level creates no fictitious operation; every independently
        // located level is exercised, including genuine empty member roots.
        for level in boundaries
            .iter()
            .map(|part| part.level.as_str())
            .collect::<BTreeSet<_>>()
        {
            eprintln!("public golden {name}: BRep level {level}");
            damage_candidates(&original, &before, &boundaries, level);
        }
    }
    assert!(
        !compressed_members.is_empty(),
        "real deflated corpus members were exercised"
    );
}
