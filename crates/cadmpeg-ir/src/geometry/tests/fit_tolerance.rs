// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    CacheContractError, ExactSpline, FitTolerance, LawFormula, LawSurfaceConstruction,
    LawSurfaceTail, LegacyCache, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, RevisionCacheForm, RevisionSurfaceForm,
};
use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};

fn surface_id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:construction#0").expect("valid identity")
}

fn curve_id() -> ProceduralCurveId {
    ProceduralCurveId::mint("synthetic:test:construction#1").expect("valid identity")
}

fn law(tail: LawSurfaceTail) -> ProceduralSurfaceDefinition {
    ProceduralSurfaceDefinition::Law(
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(
            LawSurfaceConstruction {
                parameter_ranges: None,
                primary: LawFormula::Null {},
                additional: Vec::new(),
                tail,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        ))
        .expect("law surface payload"),
    )
}

fn full_tail(fit_tolerance: f64) -> LawSurfaceTail {
    LawSurfaceTail::Full {
        cache: LegacyCache::try_new(fit_tolerance).expect("admissible fit tolerance"),
    }
}

#[test]
fn a_full_law_surface_tail_states_its_fit_tolerance() {
    let definition = law(full_tail(0.25));
    assert_eq!(
        definition.cache_fit_tolerance(),
        Some(FitTolerance::try_new(0.25).expect("admissible fit tolerance"))
    );
    let surface = ProceduralSurface::new(surface_id(), definition, None).expect("procedural");
    let wire = serde_json::to_value(&surface).expect("serialize");
    assert_eq!(
        wire["definition"]["construction"]["tail"]["cache"]["fit_tolerance"],
        0.25
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurface>(wire).expect("read back"),
        surface
    );
}

#[test]
fn a_non_full_law_surface_tail_states_no_fit_tolerance() {
    for tail in [
        LawSurfaceTail::Historical {},
        LawSurfaceTail::Optimal {},
        LawSurfaceTail::None {
            parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
            closures: [0, 0],
            singularities: [0, 0],
        },
    ] {
        let mut definition = law(tail);
        assert_eq!(definition.cache_fit_tolerance(), None);
        assert_eq!(
            definition.set_legacy_cache(Some(
                LegacyCache::try_new(0.25).expect("admissible fit tolerance")
            )),
            Err(CacheContractError::Layout(
                "this construction states no solved-cache fit tolerance"
            ))
        );
        assert_eq!(definition.cache_fit_tolerance(), None);
    }
}

#[test]
fn a_fit_tolerance_is_finite_and_non_negative() {
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            FitTolerance::try_new(value),
            Err(CacheContractError::InvalidValue { .. })
        ));
        assert!(LegacyCache::try_new(value).is_err());
    }
    let mut definition = law(full_tail(0.25));
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        let mut surface =
            ProceduralSurface::new(surface_id(), definition.clone(), None).expect("procedural");
        assert!(surface.set_cache_fit_tolerance(Some(value)).is_err());
        assert!(surface.scale_cache_fit_tolerance(value).is_err());
    }
    definition
        .set_legacy_cache(Some(
            LegacyCache::try_new(0.5).expect("admissible fit tolerance"),
        ))
        .expect("full law tail states a tolerance");
    assert_eq!(
        definition.cache_fit_tolerance(),
        Some(FitTolerance::try_new(0.5).expect("admissible fit tolerance"))
    );
}

#[test]
fn a_construction_with_no_cache_slot_refuses_a_fit_tolerance() {
    let mut definition = ProceduralCurveDefinition::Exact { cache: None };
    assert_eq!(definition.cache_fit_tolerance(), None);
    definition
        .set_legacy_cache(LegacyCache::try_new(0.5).expect("admissible fit tolerance"))
        .expect("an exact curve states its own cache tolerance");
    let mut curve = ProceduralCurve::new(curve_id(), definition).expect("procedural");
    assert_eq!(curve.cache_fit_tolerance(), Some(0.5));
    curve.raise_cache_fit_tolerance(FitTolerance::try_new(f64::MAX).expect("admissible"));
    assert_eq!(curve.cache_fit_tolerance(), Some(f64::MAX));

    let mut replica = replica_definition();
    assert!(replica
        .set_legacy_cache(LegacyCache::try_new(0.5).expect("admissible fit tolerance"))
        .is_err());
    assert_eq!(replica.cache_fit_tolerance(), None);
}

fn replica_definition() -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Replica {
        source: crate::ids::CurveId::mint("synthetic:test:curve#0").expect("valid identity"),
        transform: crate::transform::Transform::identity(),
    }
}

#[test]
fn raising_the_fit_tolerance_of_a_slotless_construction_leaves_it_unchanged() {
    let untouched = ProceduralCurve::new(curve_id(), replica_definition()).expect("procedural");
    let mut raised = ProceduralCurve::new(curve_id(), replica_definition()).expect("procedural");

    raised.raise_cache_fit_tolerance(FitTolerance::try_new(0.5).expect("admissible"));

    assert_eq!(raised, untouched);
    assert_eq!(raised.cache_fit_tolerance(), None);
}

#[test]
fn raising_the_fit_tolerance_of_a_legacy_slot_keeps_the_higher_of_the_two() {
    let legacy = |fit_tolerance: f64| {
        ProceduralCurve::new(
            curve_id(),
            ProceduralCurveDefinition::Exact {
                cache: Some(LegacyCache::try_new(fit_tolerance).expect("admissible")),
            },
        )
        .expect("procedural")
    };

    let mut curve = legacy(0.25);
    curve.raise_cache_fit_tolerance(FitTolerance::try_new(0.75).expect("admissible"));
    assert_eq!(curve.cache_fit_tolerance(), Some(0.75));

    let mut curve = legacy(0.75);
    curve.raise_cache_fit_tolerance(FitTolerance::try_new(0.25).expect("admissible"));
    assert_eq!(curve.cache_fit_tolerance(), Some(0.75));

    let mut empty =
        ProceduralCurve::new(curve_id(), ProceduralCurveDefinition::Exact { cache: None })
            .expect("procedural");
    empty.raise_cache_fit_tolerance(FitTolerance::try_new(0.25).expect("admissible"));
    assert_eq!(empty.cache_fit_tolerance(), Some(0.25));
}

fn revision_exact_definition() -> ProceduralSurfaceDefinition {
    ProceduralSurfaceDefinition::Exact(
        crate::geometry::surface_payloads::ExactSurfacePayload::try_new(ExactSpline::Revision {
            intervals: [[None, None], [None, None]],
            extension: 0,
            form: RevisionSurfaceForm {
                revision: 1,
                support_bounds: [None; 4],
                reference_endpoints: [None; 2],
                second_endpoints: [None; 2],
                flags: Vec::new(),
                cache: RevisionCacheForm::SolvedCache {
                    fit_tolerance: FitTolerance::try_new(0.25).expect("admissible fit tolerance"),
                },
                discontinuities: std::array::from_fn(|_| Vec::new()),
                tail_flag: false,
                trailing_flags: Vec::new(),
            },
        })
        .expect("exact spline surface payload"),
    )
}

#[test]
fn a_revision_exact_spline_refuses_a_legacy_cache_on_its_only_write_route() {
    let mut definition = revision_exact_definition();
    let before = definition.clone();
    assert_eq!(
        definition.set_legacy_cache(Some(
            LegacyCache::try_new(9.0).expect("admissible fit tolerance")
        )),
        Err(CacheContractError::Layout(
            "this construction states no solved-cache fit tolerance"
        ))
    );
    assert_eq!(definition, before);
    assert_eq!(
        definition.cache_fit_tolerance(),
        Some(FitTolerance::try_new(0.25).expect("admissible fit tolerance"))
    );
}

#[test]
fn clearing_a_curve_legacy_cache_is_total() {
    let mut stated = ProceduralCurveDefinition::Exact {
        cache: Some(LegacyCache::try_new(0.5).expect("admissible fit tolerance")),
    };
    stated.clear_legacy_cache();
    assert_eq!(stated, ProceduralCurveDefinition::Exact { cache: None });
    assert_eq!(stated.cache_fit_tolerance(), None);

    let mut slotless = replica_definition();
    let before = slotless.clone();
    slotless.clear_legacy_cache();
    assert_eq!(slotless, before);
    assert_eq!(slotless.cache_fit_tolerance(), None);
}
