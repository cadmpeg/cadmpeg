// SPDX-License-Identifier: Apache-2.0

use crate::geometry::curve_payloads::{
    SilhouetteCurveConstruction, SurfaceOffsetCurveConstruction,
    ThreeSurfaceIntersectionCurvePayload, TwoSidedOffsetCurveConstruction,
};
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::geometry::{
    CacheContract, CacheFirstCurveForm, CacheFirstCurveParameterization, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurveDefinition, RevisionCacheForm, SilhouetteKind,
    SupportPcurve,
};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point2, Vector3};
use crate::scalar::{FiniteReal, PositiveI64};

fn line_side() -> IntcurveSupportSide {
    IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.5, 1.0), Point2::new(1.0, -0.5)).expect("line"),
            ),
            None,
        )),
    }
}

fn context() -> IntcurveSupportContext {
    IntcurveSupportContext::try_new(
        [line_side(), line_side()],
        [1.0, 4.0],
        [vec![2.0], vec![], vec![]],
    )
    .expect("context")
}

fn definitions() -> Vec<ProceduralCurveDefinition> {
    let form = CacheFirstCurveForm {
        revision: PositiveI64::new(22_800).expect("revision"),
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(1.0), Some(4.0)],
            closed_form: 0,
        }),
        support_bounds: [[None; 4]; 2],
        solved_range: [Some(1.5), None],
        extension: 0,
    };
    vec![
        ProceduralCurveDefinition::ThreeSurfaceIntersection(
            ThreeSurfaceIntersectionCurvePayload::try_new(context(), 1, line_side())
                .expect("three-surface"),
        ),
        ProceduralCurveDefinition::TwoSidedOffset(
            TwoSidedOffsetCurveConstruction::try_new(context(), false, [1.0, -1.0], None)
                .expect("two-sided offset"),
        ),
        ProceduralCurveDefinition::Silhouette(
            SilhouetteCurveConstruction::try_new(
                context(),
                SilhouetteKind::Standard {},
                SurfaceId::mint("test:model:surface#cast").expect("surface"),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("silhouette"),
        ),
        ProceduralCurveDefinition::SurfaceOffset(
            SurfaceOffsetCurveConstruction::try_new(
                context(),
                false,
                [[0.0, 1.0], [0.0, 2.0]],
                (
                    CurveId::mint("test:model:curve#base").expect("curve"),
                    [0.0, 3.0],
                    [None, None],
                ),
                CacheContract::Revision { form },
                0.25,
                [0.5, 2.0],
            )
            .expect("surface offset"),
        ),
    ]
}

/// Each family restates its context on `-t`, a second reversal restores it,
/// and a surface offset keeps its stored base ranges, shift, and scale.
#[test]
fn context_families_reverse_about_zero_and_back() {
    for original in definitions() {
        let mut reversed = original.clone();
        reversed.reverse_support_contexts().expect("reversible");
        assert_ne!(reversed, original);
        let expected = context().reversed_about_zero().expect("reversible context");
        match &reversed {
            ProceduralCurveDefinition::ThreeSurfaceIntersection(payload) => {
                assert_eq!(payload.context(), &expected);
                assert_eq!(payload.third(), &expected.sides()[0]);
            }
            ProceduralCurveDefinition::TwoSidedOffset(payload) => {
                assert_eq!(payload.context(), &expected);
            }
            ProceduralCurveDefinition::Silhouette(payload) => {
                assert_eq!(payload.context(), &expected);
            }
            ProceduralCurveDefinition::SurfaceOffset(payload) => {
                assert_eq!(payload.context(), &expected);
                let form = payload.cache_first().expect("cache-first form");
                assert_eq!(form.solved_range, [None, FiniteReal::new(-1.5)]);
                assert!(matches!(
                    form.cache,
                    RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
                        interval,
                        ..
                    }) if interval == [FiniteReal::new(-4.0), FiniteReal::new(-1.0)]
                ));
                assert_eq!(payload.base_range().endpoints(), [0.0, 3.0]);
                assert_eq!([payload.shift().get(), payload.scale().get()], [0.5, 2.0]);
            }
            _ => unreachable!("only context families are built"),
        }
        reversed.reverse_support_contexts().expect("reversible");
        assert_eq!(reversed, original);
    }
}
