// SPDX-License-Identifier: Apache-2.0
//! Admission of native carriers with their stored evaluation.

use super::*;

#[test]
fn a_late_native_conflict_discards_its_staged_carriers_and_keeps_the_accepted_prefix() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let unknown = |name: &str, byte| {
        UnknownRecord::retained(
            UnknownId::mint(format!("f3d:brep:spline#{name}")).expect("identity grammar"),
            0,
            vec![byte],
            Vec::new(),
        )
    };
    let prefix = unknown("0", 0);
    let staged = unknown(&format!("1{}", "x".repeat(256)), 1);
    let conflicting = unknown("2", 2);
    let state = |unknowns: Vec<UnknownRecord>| {
        let surfaces = unknowns
            .iter()
            .enumerate()
            .map(|(index, unknown)| Surface {
                id: SurfaceId::mint(format!("f3d:brep:entity#s{index}")).expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(unknown.id().clone()),
                }),
                source_object: None,
                native_ref: None,
            })
            .collect();
        StateRecords {
            records: Topology {
                surfaces,
                ..Topology::default()
            },
            unknowns,
            notes: Vec::new(),
        }
    };
    let staged_text = u64::try_from(
        cadmpeg_core::json!({"id": staged.id().as_str(), "links": staged.links()})
            .to_string()
            .len(),
    )
    .expect("text length");
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = staged_text - 1;
    assert!(attach_bound(std::slice::from_ref(&prefix)).expect("prefix bound") < staged_text);
    let mut fidelity = no_fidelity();
    crate::test_support::with_decode_context(|ctx| {
        fidelity
            .attach_native_unknown_records(
                &mut CadIr::empty(),
                "f3d",
                vec![UnknownRecord::retained(
                    conflicting.id().clone(),
                    0,
                    vec![9],
                    Vec::new(),
                )],
                ctx,
            )
            .expect("the document's conflicting carrier");
    });
    let outcome = run_under(
        &policy,
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3, 4],
        }],
        &[Ok(read(
            4,
            Topology::default(),
            vec![
                (want(3, &[]), Ok(state(vec![prefix.clone()]))),
                (
                    want(2, &[]),
                    Ok(state(vec![prefix.clone(), staged, conflicting.clone()])),
                ),
                (want(1, &[]), Ok(records(Topology::default()))),
            ],
        ))],
        &[Consumed::default()],
        &fidelity,
    );
    assert!(!outcome.refused);
    let [c] = outcome.evaluations.as_slice() else {
        panic!("one accepted evaluation: {:#?}", outcome.evaluations);
    };
    assert_eq!(c.of, after("c"));
    assert_eq!(content(c).0, &StateBase::Current(design.scope.at.clone()));
    assert_eq!(outcome.carried, [prefix]);
    let [loss] = outcome.losses.as_slice() else {
        panic!("one suffix omission: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(loss.message.contains(&format!(
        "the states after features {}, {} are not stated: its native record {} differs from the document's record",
        feature_id("a"),
        feature_id("b"),
        conflicting.id()
    )));
    assert!(!loss.message.contains(feature_id("c").as_str()));
    crate::test_support::with_decode_policy(&policy, |ctx| {
        fidelity
            .attach_native_unknown_records(&mut CadIr::empty(), "f3d", outcome.carried, ctx)
            .expect("the accepted carrier prefix fits its actual attachment cost");
        assert_eq!(ctx.resource_refusal(), None);
    });
}
