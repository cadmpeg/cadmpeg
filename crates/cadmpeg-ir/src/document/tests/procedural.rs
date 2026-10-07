// SPDX-License-Identifier: Apache-2.0
//! A batch of procedural attachments decides exactly as one attachment at a
//! time does.

use proptest::prelude::*;

use crate::document::admission::StandardAdmission;
use crate::document::Model;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId};

fn curve_id(index: u8) -> CurveId {
    CurveId::mint(format!("test:model:curve#{index}")).unwrap()
}

fn construction_id(index: u8) -> ProceduralCurveId {
    ProceduralCurveId::mint(format!("test:model:construction#{index}")).unwrap()
}

fn procedural(index: u8) -> ProceduralCurve {
    ProceduralCurve::new(
        construction_id(index),
        ProceduralCurveDefinition::Exact { cache: None },
    )
}

/// A carrier's starting geometry: solved, or naming a construction with or
/// without a solved cache.
fn geometry(state: (u8, u8)) -> CurveGeometry {
    let solved = || SolvedCurveGeometry::Unknown { record: None };
    match state.0 % 3 {
        0 => CurveGeometry::Solved(solved()),
        1 => CurveGeometry::Procedural {
            construction: construction_id(state.1),
            cache: None,
        },
        _ => CurveGeometry::Procedural {
            construction: construction_id(state.1),
            cache: Some(solved()),
        },
    }
}

fn model(carriers: &[(u8, (u8, u8))], stored: &[u8]) -> Model {
    let mut model = Model::default();
    for (id, state) in carriers {
        model.curves.push(Curve {
            id: curve_id(*id),
            geometry: geometry(*state),
            source_object: None,
        });
    }
    for id in stored {
        model.procedural_curves.push(procedural(*id));
    }
    model
}

fn outcome(result: Result<(), crate::document::ProceduralCarrierError>) -> Result<(), String> {
    result.map_err(|error| error.to_string())
}

proptest! {
    #[test]
    fn batch_attachment_matches_one_at_a_time(
        carriers in prop::collection::vec((0_u8..6, (0_u8..3, 0_u8..6)), 0..8),
        stored in prop::collection::vec(0_u8..6, 0..4),
        attachments in prop::collection::vec((0_u8..7, 0_u8..6), 0..10),
    ) {
        let base = model(&carriers, &stored);

        let mut single = base.clone();
        let single_outcomes: Vec<_> = attachments
            .iter()
            .map(|(owner, id)| {
                let attached = single
                    .add_procedural_curve(&StandardAdmission, &curve_id(*owner), procedural(*id));
                match attached {
                    Ok(result) => outcome(result),
                    Err(never) => match never {},
                }
            })
            .collect();

        let batch_input: Vec<_> = attachments
            .iter()
            .map(|(owner, id)| (curve_id(*owner), procedural(*id)))
            .collect();
        let mut batch = base.clone();
        let batch_outcomes: Vec<_> = match batch.add_procedural_curves(&StandardAdmission, batch_input.clone()) {
            Ok(outcomes) => outcomes.into_iter().map(outcome).collect(),
            Err(never) => match never {},
        };
        prop_assert_eq!(&batch_outcomes, &single_outcomes);
        prop_assert_eq!(&batch, &single);

        let ctx = cadmpeg_test_support::service_decode_context();
        let mut charged = base;
        let charged_outcomes: Vec<_> = charged
            .add_procedural_curves(&ctx, batch_input)
            .unwrap()
            .into_iter()
            .map(outcome)
            .collect();
        prop_assert_eq!(&charged_outcomes, &single_outcomes);
        prop_assert_eq!(&charged, &single);
    }
}

#[test]
fn batch_attachment_pays_for_its_index_and_preserves_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let base = model(&[(0, (0, 0)), (1, (0, 0))], &[]);
    let attachments = || vec![(curve_id(0), procedural(0)), (curve_id(1), procedural(1))];
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut refused = base.clone();
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            refused.add_procedural_curves(&ctx, attachments())
        else {
            panic!("the batch must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(refused, base);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
