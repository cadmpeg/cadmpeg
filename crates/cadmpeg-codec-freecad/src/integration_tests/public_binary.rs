// SPDX-License-Identifier: Apache-2.0
//! Public source carriers use independent framing and independently derived values.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};
use cadmpeg_test_support::framing::{
    freecad::binary::{frame_binary_member, BinaryKind},
    zip,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

mod material;
mod neutral;
mod oracle;
mod source;

fn decode(bytes: &[u8], depth: cadmpeg_core::decode::DecodeDepth) -> DecodeResult {
    crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
}

fn exact(expected: &cadmpeg_ir::CadIr, after: &cadmpeg_ir::CadIr) {
    let expected = serde_json::to_value(expected).unwrap();
    let actual = serde_json::to_value(after).unwrap();
    assert_decodes_agree(&expected, &actual, &Allowances::default(), Mode::Same);
    assert_decodes_agree(&actual, &expected, &Allowances::default(), Mode::Same);
}

fn positive(name: &str, bytes: &[u8], expected_members: usize, expected_levels: usize) {
    for depth in [
        cadmpeg_core::decode::DecodeDepth::Container,
        cadmpeg_core::decode::DecodeDepth::Geometry,
        cadmpeg_core::decode::DecodeDepth::Full,
    ] {
        let before = decode(bytes, depth);
        assert!(
            before
                .source_fidelity()
                .parts()
                .values()
                .all(|part| part.state() != &(PartState::GivenUp {})),
            "{name} {depth:?}: {:?}",
            before.report().losses
        );
        let (carriers, boundaries) = source::inventory(bytes, &before);
        assert_eq!(carriers.len(), expected_members, "{name}");
        assert_eq!(
            boundaries
                .iter()
                .map(|part| &part.level)
                .collect::<BTreeSet<_>>()
                .len(),
            expected_levels,
            "{name}"
        );
        assert!(
            crate::test_support::validate_native(before.ir()).is_empty(),
            "{name} {depth:?}: {:?}",
            crate::test_support::validate_native(before.ir())
        );
        crate::fidelity::b1::assert_classified_spans(&before);
        for carrier in &carriers {
            let expected =
                oracle::expected(&before, carrier, &source::member(bytes, &carrier.member));
            exact(&expected, before.ir());
        }
    }
}

fn compare_failure(original: &[u8], changed: &[u8], before: &DecodeResult, after: &DecodeResult) {
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(original, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let new_arena = DecodeArena::new();
    let (new_ctx, new_root) =
        DecodeContext::from_root_bytes(changed, &new_arena, &DecodePolicy::service()).unwrap();
    let new_root = new_ctx.name_stream(new_root, "file").unwrap();
    let allowances = Allowances::given_up_zip(
        DecodeSnapshot::new(before.ir(), before.source_fidelity()),
        DecodeSnapshot::new(after.ir(), after.source_fidelity()),
        after.report(),
        (&old_ctx, old_root),
        (&new_ctx, new_root),
    )
    .unwrap();
    assert_decodes_agree(
        &serde_json::to_value(before.ir()).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowances,
        Mode::Same,
    );
}

fn case(
    original: &[u8],
    before: &DecodeResult,
    carrier: &source::Carrier,
    target: &source::Boundary,
    fill: u8,
) {
    let changed = source::mutate(original, carrier, target, fill);
    let after = decode(&changed, cadmpeg_core::decode::DecodeDepth::Full);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&changed, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let member = archive.open(&ctx, &carrier.member).unwrap();
    let capture = ctx.capture_part_extent(member).unwrap();
    let failures = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}))
        .collect::<Vec<_>>();
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(
        failures.len(),
        errors.len(),
        "one actual Error for every failed helper"
    );
    for part in &failures {
        assert_eq!(part.stream(), capture.stream());
        assert!(
            part.offset() >= target.offset
                && part.offset().checked_add(part.byte_len()).unwrap()
                    <= target.offset + target.length,
            "failure outside actual overwritten part: {target:?} {part:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|loss| loss.severity == cadmpeg_ir::report::Severity::Error
                    && loss
                        .provenance
                        .as_ref()
                        .is_some_and(|p| p.stream_name() == Some(part.stream())
                            && p.offset == part.offset()
                            && p.byte_len() == Some(part.byte_len())))
                .count(),
            1,
            "one exact typed error home"
        );
    }
    crate::fidelity::b1::assert_classified_spans(&after);
    assert!(
        crate::test_support::validate_native(after.ir()).is_empty(),
        "{:?}",
        crate::test_support::validate_native(after.ir())
    );
    if failures.is_empty() {
        let expected = oracle::expected(before, carrier, member.window());
        exact(&expected, after.ir());
    } else {
        compare_failure(original, &changed, before, &after);
    }
    let baseline = before
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect::<BTreeSet<_>>();
    for loss in &after.report().losses {
        assert!(
            loss.code.local_code() == "record.unreadable"
                || baseline.contains(&loss.code.to_string()),
            "unexpected non-helper loss {loss:?}"
        );
    }
}

fn damage(name: &str, bytes: &[u8], expected_members: usize, expected_levels: usize) {
    let before = decode(bytes, cadmpeg_core::decode::DecodeDepth::Full);
    assert!(
        before
            .source_fidelity()
            .parts()
            .values()
            .all(|part| part.state() != &(PartState::GivenUp {})),
        "{name}: {:?}",
        before.report().losses
    );
    let (carriers, boundaries) = source::inventory(bytes, &before);
    assert_eq!(carriers.len(), expected_members, "{name}");
    let mut groups = BTreeMap::<&str, Vec<&source::Boundary>>::new();
    for boundary in &boundaries {
        groups.entry(&boundary.level).or_default().push(boundary);
    }
    assert_eq!(groups.len(), expected_levels, "{name}");
    let mut failures = Vec::new();
    for peers in groups.values() {
        for index in [0, peers.len() / 2, peers.len() - 1]
            .into_iter()
            .collect::<BTreeSet<_>>()
        {
            let target = peers[index];
            for fill in [0, 0xff] {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    case(bytes, &before, &carriers[target.carrier], target, fill)
                }));
                if result.is_err() {
                    failures.push(format!(
                        "{} {target:?} fill={fill:#x}",
                        carriers[target.carrier].member
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: binary controls failed:\n{}",
        failures.join("\n")
    );
}

macro_rules! controls {
    ($positive:ident,$damage:ident,$file:literal,$members:literal,$levels:literal) => {
        #[test]
        fn $positive() {
            positive(
                $file,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../corpus/freecad_fcstd/fixtures/",
                    $file
                )),
                $members,
                $levels,
            );
        }
        #[test]
        fn $damage() {
            damage(
                $file,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../corpus/freecad_fcstd/fixtures/",
                    $file
                )),
                $members,
                $levels,
            );
        }
    };
}
controls!(
    application_payloads_framing,
    application_payloads_damage,
    "application_payloads.FCStd",
    4,
    8
);
controls!(
    core_design_product_framing,
    core_design_product_damage,
    "core_design_product.FCStd",
    2,
    2
);
controls!(
    core_operations_framing,
    core_operations_damage,
    "core_operations.FCStd",
    5,
    4
);
controls!(
    gui_appearance_framing,
    gui_appearance_damage,
    "gui_appearance.FCStd",
    3,
    5
);
controls!(
    product_assembly_framing,
    product_assembly_damage,
    "product_assembly.FCStd",
    10,
    4
);
controls!(
    techdraw_annotations_framing,
    techdraw_annotations_damage,
    "techdraw_annotations.FCStd",
    1,
    2
);

#[test]
fn first_public_fillet_radius_keeps_source_feature_and_exact_ownership() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
    ));
    let before = decode(bytes, cadmpeg_core::decode::DecodeDepth::Full);
    let (carriers, boundaries) = source::inventory(bytes, &before);
    let target = boundaries
        .iter()
        .find(|part| part.level == "freecad.fillet_edge")
        .expect("actual first fillet row");
    for fill in [0, 0xff] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            case(bytes, &before, &carriers[target.carrier], target, fill)
        }));
        if let Err(error) = result {
            let changed = source::mutate(bytes, &carriers[target.carrier], target, fill);
            let after = decode(&changed, cadmpeg_core::decode::DecodeDepth::Full);
            let expected = if after
                .source_fidelity()
                .parts()
                .values()
                .any(|part| part.state() == &(PartState::GivenUp {}))
            {
                before.ir().clone()
            } else {
                oracle::expected(
                    &before,
                    &carriers[target.carrier],
                    &source::member(&changed, &carriers[target.carrier].member),
                )
            };
            let selected = |ir: &cadmpeg_ir::CadIr| {
                ir.model
                    .features
                    .iter()
                    .filter(|feature| {
                        feature.native_ref.as_deref() == Some("fcstd:native:object#PartFillet")
                    })
                    .map(|feature| serde_json::to_value(feature).unwrap())
                    .collect::<Vec<_>>()
            };
            eprintln!(
                "independent expected Fillet: {}\nactual Fillet: {}\nactual failures: {:?}",
                serde_json::to_string(&selected(&expected)).unwrap(),
                serde_json::to_string(&selected(after.ir())).unwrap(),
                after
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code.local_code() == "record.unreadable")
                    .collect::<Vec<_>>()
            );
            std::panic::resume_unwind(error);
        }
    }
}

#[test]
fn first_public_array_frame_and_scale_keep_exact_causal_omissions() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd"
    ));
    let before = decode(bytes, cadmpeg_core::decode::DecodeDepth::Full);
    let (carriers, boundaries) = source::inventory(bytes, &before);
    for (member, level) in [
        ("PlacementList3", "freecad.placement_list_placement"),
        ("ScaleList3", "freecad.vector_list_vector"),
    ] {
        let target = boundaries
            .iter()
            .find(|part| part.level == level && carriers[part.carrier].member == member)
            .expect("actual first array row");
        for fill in [0, 0xff] {
            case(bytes, &before, &carriers[target.carrier], target, fill);
        }
    }
}
