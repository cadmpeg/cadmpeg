// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion saves that paste copies of 10 x 10 mm boxes into a new
//! component with one Copy/Paste Bodies.

use std::collections::HashMap;
use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::F3dCodec;

/// The decode of a corpus file, after checking that the neutral model
/// validates and that the paste resolves its sources and inserts its copies
/// with a construction.
fn decoded(bytes: &[u8]) -> cadmpeg_ir::DecodeResult {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    for code in [
        F3dLossCode::FeaturePasteSourcesUnresolved,
        F3dLossCode::FeatureStoredBodiesRecipeAbsent,
    ] {
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != code.kind()),
            "{:?}",
            code.kind()
        );
    }
    result
}

/// The one paste-owned scope of a decode.
fn paste_scope(native: &crate::native::F3dNative) -> &DesignParameterScope {
    let mut scopes = native.design_parameter_scopes.iter().filter(|scope| {
        scope
            .base_feature_construction()
            .is_some_and(|construction| construction.copy_source_suffixes().next().is_some())
    });
    let scope = scopes.next().expect("a paste-owned scope");
    assert!(scopes.next().is_none());
    scope
}

/// Relative allowance on a volume, beyond the two measurements' numeric
/// errors.
const VOLUME_RELATIVE_TOLERANCE: f64 = 1e-9;

/// The measured volume of each body that has one, by body id.
fn volumes(ir: &cadmpeg_ir::document::CadIr) -> HashMap<String, (f64, f64)> {
    let report = cadmpeg_measure::test_support::round_trip::measured("copy paste", ir)
        .expect("measure the bodies");
    report
        .bodies()
        .iter()
        .filter_map(|row| {
            let volume = row.volume().as_ref()?.measured()?;
            Some((
                row.body().to_owned(),
                (volume.value().value.get(), volume.numeric_error().get()),
            ))
        })
        .collect()
}

fn same_volume((a, a_error): (f64, f64), (b, b_error): (f64, f64)) -> bool {
    (a - b).abs() <= a_error + b_error + VOLUME_RELATIVE_TOLERANCE * a.abs().max(b.abs())
}

/// The current body that the one Design body binding of `suffix` in the
/// stream of `scope` names.
fn bound_body(
    native: &crate::native::F3dNative,
    scope: &DesignParameterScope,
    suffix: u64,
) -> String {
    let stream = crate::ids::native_stream(&scope.id);
    let mut bodies = native.design_body_bindings.iter().filter(|binding| {
        binding.entity_suffix == suffix && crate::ids::native_stream(binding.id()) == stream
    });
    let binding = bodies.next().expect("a binding of the suffix");
    assert!(bodies.next().is_none(), "suffix {suffix} binds twice");
    binding
        .body
        .clone()
        .expect("a current body")
        .as_str()
        .to_owned()
}

/// Each pasted copy has the volume of the source that the paste pairs it
/// with, and no two sources have one volume, so no other pairing holds.
fn assert_each_copy_has_its_source_volume(
    ir: &cadmpeg_ir::document::CadIr,
    native: &crate::native::F3dNative,
    pasted: usize,
) {
    let scope = paste_scope(native);
    let construction = scope.base_feature_construction().expect("a paste");
    let body = |suffix: u64| bound_body(native, scope, suffix);
    let volumes = volumes(ir);
    let volume = |suffix: u64| volumes[&body(suffix)];
    let copies = construction.body_entity_suffixes().collect::<Vec<_>>();
    let sources = construction.copy_source_suffixes().collect::<Vec<_>>();
    assert_eq!((copies.len(), sources.len()), (pasted, pasted));
    for (copy, source) in copies.iter().zip(&sources) {
        assert!(
            same_volume(volume(*copy), volume(*source)),
            "copy {copy} and source {source}"
        );
    }
    for (index, a) in sources.iter().enumerate() {
        for b in &sources[index + 1..] {
            assert!(!same_volume(volume(*a), volume(*b)), "sources {a} and {b}");
        }
    }
}

/// The paste's one feature inserts the bodies of its copies, and none of
/// its sources.
fn assert_paste_inserts_its_copies(
    ir: &cadmpeg_ir::document::CadIr,
    native: &crate::native::F3dNative,
    copies: usize,
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, InsertedBodies};
    let scope = paste_scope(native);
    let mut features = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()));
    let feature = features.next().expect("the paste's feature");
    assert!(features.next().is_none());
    assert!(
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertBodies {
                bodies: InsertedBodies::Resolved { .. },
            })
        ),
        "{:?}",
        feature.evaluation.definition()
    );
    let construction = scope.base_feature_construction().expect("a paste");
    let expected = construction
        .body_entity_suffixes()
        .map(|suffix| bound_body(native, scope, suffix))
        .collect::<Vec<_>>();
    let sources = construction
        .copy_source_suffixes()
        .map(|suffix| bound_body(native, scope, suffix))
        .collect::<Vec<_>>();
    let outputs = feature
        .evaluation
        .outputs()
        .iter()
        .map(|body| body.as_str().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(outputs.len(), copies);
    assert_eq!(outputs, expected);
    assert!(outputs.iter().all(|output| !sources.contains(output)));
    assert_eq!(feature.name.as_deref(), Some("CopyPasteBodies 1"));
}

/// Each corpus file decodes once for both checks.
#[test]
fn a_paste_inserts_its_copies_and_each_copy_names_its_source() {
    // The three-body and two-component scopes store their GUID pairs in the
    // reverse of body order. The one-body paste has no second source to
    // tell apart by volume.
    for (bytes, copies, volumes_pair_sources) in [
        (
            &include_bytes!("../../../../corpus/f3d/copy_paste_one_body.f3d")[..],
            1,
            false,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/copy_paste_two_components.f3d")[..],
            2,
            true,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/copy_paste_two_bodies.f3d")[..],
            2,
            true,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/copy_paste_three_bodies.f3d")[..],
            3,
            true,
        ),
    ] {
        let result = decoded(bytes);
        let native = crate::test_support::native_test::f3d_native(result.ir());
        assert_paste_inserts_its_copies(result.ir(), &native, copies);
        if volumes_pair_sources {
            assert_each_copy_has_its_source_volume(result.ir(), &native, copies);
        }
    }
}
