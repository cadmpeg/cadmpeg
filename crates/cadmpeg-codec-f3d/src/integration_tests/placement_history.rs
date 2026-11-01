// SPDX-License-Identifier: Apache-2.0
//! Occurrence placements of a Design whose duplicate `Component Insert`
//! envelopes resolve through ASM history header links, decoded under
//! retained-byte allowances with and without room for history binding.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeResult};
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;
use crate::test_support::body_map_test::f3d_with_breps_and_design;
use crate::test_support::component_insert_test::{
    component_insert_design, ComponentInsertEnvelope,
};
use crate::test_support::history_stream::DesignScale;
use crate::F3dCodec;

const SCALE: DesignScale = DesignScale {
    states: 20,
    bodies: 20,
    faces_per_body: 1,
    updates_per_state: 2,
};

/// State ids count down from the head, so state 3 links to state 2.
const HISTORY_BOUND: ComponentInsertEnvelope = ComponentInsertEnvelope {
    translation_cm: [1.0, 2.0, 3.0],
    history_state_id: 3,
    previous_history_state_id: 2,
};

/// A later envelope whose state pair is absent from the history.
const UNBOUND: ComponentInsertEnvelope = ComponentInsertEnvelope {
    translation_cm: [4.0, 5.0, 6.0],
    history_state_id: 1_000,
    previous_history_state_id: 999,
};

struct Decoded {
    result: DecodeResult,
    retained_bytes: u64,
}

fn design() -> Vec<u8> {
    let (bulk, meta) = component_insert_design(&[HISTORY_BOUND, UNBOUND]);
    f3d_with_breps_and_design(
        &[("BREP.synthetic.smbh", &SCALE.history_smbh())],
        &bulk,
        &meta,
    )
}

fn decode(bytes: &[u8], max_retained_bytes: u64) -> Result<Decoded, String> {
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .map_err(|error| error.to_string())?;
    let available = ctx.retained_bytes_available();
    let result = F3dCodec.decode_with_context(&ctx, root);
    let retained_bytes = available - ctx.retained_bytes_available();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(format!("{refusal:?}"));
    }
    ctx.finish_session().map_err(|error| error.to_string())?;
    Ok(Decoded {
        result: result.map_err(|error| error.to_string())?,
        retained_bytes,
    })
}

/// Decode under the desktop allowance, where history binding fits.
fn decode_bound(bytes: &[u8]) -> Decoded {
    let bound = decode(bytes, DecodePolicy::desktop().limits.max_retained_bytes)
        .unwrap_or_else(|error| panic!("the design decodes under the desktop allowance: {error}"));
    assert!(!binding_skipped(&bound.result));
    bound
}

/// Decode under each `step / steps` fraction of `full_charge` below it.
fn below_full_charge(
    bytes: &[u8],
    full_charge: u64,
    steps: u64,
) -> Vec<(u64, Result<Decoded, String>)> {
    (1..steps)
        .map(|step| {
            let allowance = full_charge * step / steps;
            (allowance, decode(bytes, allowance))
        })
        .collect()
}

fn binding_skipped(result: &DecodeResult) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoryBindingBudgetExceeded.kind())
}

#[test]
fn duplicate_component_insert_placements_do_not_depend_on_the_retained_allowance() {
    let bytes = design();
    let bound = decode_bound(&bytes);
    let occurrences = &bound.result.ir().model.occurrences;
    assert_eq!(occurrences.len(), 1, "{occurrences:#?}");
    let expected = Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 30.0],
    ])
    .unwrap();
    assert_eq!(occurrences[0].transform, Some(expected));

    for (allowance, outcome) in below_full_charge(&bytes, bound.retained_bytes, 16) {
        if let Ok(decoded) = outcome {
            assert_eq!(
                &decoded.result.ir().model.occurrences,
                occurrences,
                "allowance {allowance}"
            );
        }
    }
}

#[test]
fn an_allowance_that_skips_history_binding_keeps_the_placements() {
    let bytes = design();
    let bound = decode_bound(&bytes);
    let mut outcomes = Vec::new();
    let mut skipped = None;
    for (allowance, outcome) in below_full_charge(&bytes, bound.retained_bytes, 64) {
        match outcome {
            Ok(decoded) if binding_skipped(&decoded.result) => skipped = Some(decoded),
            Ok(decoded) => outcomes.push(format!("{allowance}: bound, {}", decoded.retained_bytes)),
            Err(error) => outcomes.push(format!("{allowance}: {error}")),
        }
    }
    let skipped = skipped.unwrap_or_else(|| {
        panic!(
            "no allowance skips history binding and still decodes; full charge {}: {outcomes:#?}",
            bound.retained_bytes
        )
    });
    assert_eq!(
        skipped.result.ir().model.occurrences,
        bound.result.ir().model.occurrences
    );
}

#[test]
fn attributed_local_children_repeat_under_each_containing_instance() {
    const TOLERANCE: f64 = 1e-12;
    let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(&[300]);
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    let model = &result.ir().model;
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.product_definitions.len(), 3);
    assert_eq!(model.occurrences.len(), 7);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::LocalOccurrenceUnplaced.kind()));
    let graph = cadmpeg_ir::products::AssemblyGraph::new(&model.occurrences).unwrap();
    let leaves = model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.id.as_str().contains(':')
                && occurrence
                    .id
                    .as_str()
                    .split('#')
                    .next_back()
                    .unwrap()
                    .contains(':')
        })
        .collect::<Vec<_>>();
    assert_eq!(leaves.len(), 4);
    assert!(leaves
        .iter()
        .all(|leaf| leaf.prototype == leaves[0].prototype));
    for leaf in leaves {
        let pose = cadmpeg_test_support::assembly::resolved_transform(&graph, &leaf.id).unwrap();
        let child_b = leaf.id.as_str().ends_with("000000000403");
        let face = pose
            .apply_point(cadmpeg_ir::math::Point3::new(
                if child_b { 0.0 } else { 10.0 },
                0.0,
                0.0,
            ))
            .unwrap()
            .get();
        assert!((face.y - 50.0).abs() < TOLERANCE);
        assert!((face.x - 30.0).abs() < TOLERANCE);
    }
}

#[test]
fn only_absent_membership_uses_the_component_cluster() {
    for owners in [&[][..], &[200, 300][..], &[999][..]] {
        let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(owners);
        let bytes = f3d_with_breps_and_design(
            &[(
                "BREP.module.smbh",
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(
            result
                .ir()
                .model
                .product_definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            usize::from(owners.is_empty())
        );
        assert!(result.report().losses.iter().any(|loss| loss.code
            == if owners.is_empty() {
                F3dLossCode::ComponentBodyOwnerFallback.kind()
            } else {
                F3dLossCode::ComponentBodyOwnerUnresolved.kind()
            }));
    }
}

#[test]
fn admitted_component_membership_wins_over_an_unregistered_owner() {
    for owners in [&[999, 300][..], &[300, 999][..]] {
        let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(owners);
        let bytes = f3d_with_breps_and_design(
            &[(
                "BREP.module.smbh",
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(
            result
                .ir()
                .model
                .product_definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            1
        );
        assert!(!result.report().losses.iter().any(|loss| loss.code
            == F3dLossCode::ComponentBodyOwnerFallback.kind()
            || loss.code == F3dLossCode::ComponentBodyOwnerUnresolved.kind()));
    }
}

#[test]
fn empty_attribute_local_children_place_identity_matrix_and_nested_occurrences() {
    use cadmpeg_ir::products::OccurrenceParent;
    let (bulk, meta) =
        crate::test_support::empty_attribute_occurrences::empty_attribute_local_component_design(
            &[300],
        );
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    super::assert_valid(&result);
    let model = &result.ir().model;
    assert_eq!(model.product_definitions.len(), 3);
    assert_eq!(model.occurrences.len(), 7);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::LocalOccurrenceUnplaced.kind()));
    let root = model
        .occurrences
        .iter()
        .find(|o| o.parent == OccurrenceParent::Root {})
        .unwrap();
    let children = model
        .occurrences
        .iter()
        .filter(|o| {
            o.parent
                == OccurrenceParent::Occurrence {
                    occurrence: root.id.clone(),
                }
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(
        children
            .iter()
            .filter(|o| o.transform == Transform::identity())
            .count(),
        1
    );
    for child in children {
        assert_eq!(
            model
                .occurrences
                .iter()
                .filter(|o| o.parent
                    == OccurrenceParent::Occurrence {
                        occurrence: child.id.clone()
                    })
                .count(),
            2
        );
    }
}

#[test]
fn later_fixed_identity_occurrence_reports_loss_and_stays_out_of_neutral_structure() {
    let (bulk, meta) =
        crate::test_support::empty_attribute_occurrences::later_identity_local_component_design();
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    super::assert_valid(&result);
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("ordinal 2"));
    assert_eq!(result.ir().model.occurrences.len(), 4);
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .all(|o| !o.id.as_str().ends_with("000000000404")));
}
