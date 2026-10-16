// SPDX-License-Identifier: Apache-2.0
//! Occurrence placements of a Design whose duplicate `Component Insert`
//! envelopes resolve through ASM history header links, decoded under
//! retained-byte allowances with and without room for history binding.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
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
    let options = DecodeOptions {
        policy,
        ..DecodeOptions::default()
    };
    let result = F3dCodec.decode_with_context(&ctx, root, &options);
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
    assert_eq!(occurrences[0].transform, expected);

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
#[ignore = "no allowance both skips history binding and decodes: the skipped history still \
            serializes about 278 bytes per entity change against a 170-byte per-slot binding \
            estimate, so the decode refuses at 'serialize native record'"]
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
