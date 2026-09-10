// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    CacheFitToleranceError, FitTolerance, LawFormula, LawSurfaceConstruction, LawSurfaceTail,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralGeometryError, ProceduralSurface,
    ProceduralSurfaceDefinition,
};
use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
use serde::Deserialize;

fn surface_id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:procedural_surface#tolerance").unwrap()
}

fn law(tail: LawSurfaceTail) -> ProceduralSurfaceDefinition {
    ProceduralSurfaceDefinition::Law(
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(
            LawSurfaceConstruction {
                parameter_ranges: None,
                primary: LawFormula::Null,
                additional: Vec::new(),
                tail,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        ))
        .unwrap(),
    )
}

#[test]
fn fit_tolerance_rejects_negative_and_nonfinite_values_at_admission() {
    for value in [-1.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        assert!(FitTolerance::try_new(value).is_err());
        assert!(
            FitTolerance::deserialize(
                serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value)
            )
            .is_err()
        );
    }
    for value in [-0.0, 0.0, 0.25, f64::MAX] {
        let tolerance = FitTolerance::try_new(value).unwrap();
        assert_eq!(tolerance.get().to_bits(), value.to_bits());
        let wire = serde_json::to_string(&tolerance).unwrap();
        assert_eq!(
            serde_json::from_str::<FitTolerance>(&wire).unwrap(),
            tolerance
        );
    }
}

#[test]
fn law_tail_requires_exactly_its_cache_contract() {
    assert!(matches!(
        ProceduralSurface::new(surface_id(), law(LawSurfaceTail::Full {}), None),
        Err(ProceduralGeometryError::Cache(
            CacheFitToleranceError::MissingLawFull
        ))
    ));
    let full =
        ProceduralSurface::try_new(surface_id(), law(LawSurfaceTail::Full {}), Some(0.25), None)
            .unwrap();
    let mut wire = serde_json::to_value(&full).unwrap();
    assert_eq!(wire["cache_fit_tolerance"], 0.25);
    assert_eq!(
        wire["definition"]["construction"]["tail"],
        serde_json::json!({"kind":"full"})
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurface>(wire.clone()).unwrap(),
        full
    );
    wire.as_object_mut().unwrap().remove("cache_fit_tolerance");
    assert!(serde_json::from_value::<ProceduralSurface>(wire).is_err());
    for tail in [
        LawSurfaceTail::Historical {},
        LawSurfaceTail::Optimal {},
        LawSurfaceTail::None {
            parameter_ranges: [[0.0, 1.0]; 2],
            closures: [0; 2],
            singularities: [0; 2],
        },
        LawSurfaceTail::Summary {
            parameters: [vec![0.0], vec![1.0]],
            fit_tolerance: FitTolerance::try_new(0.5).unwrap(),
            closures: [0; 2],
            singularities: [0; 2],
        },
    ] {
        let definition = law(tail);
        assert!(matches!(
            ProceduralSurface::try_new(surface_id(), definition.clone(), Some(0.25), None),
            Err(ProceduralGeometryError::Cache(
                CacheFitToleranceError::NonFullLaw
            ))
        ));
        let mut surface = ProceduralSurface::new(surface_id(), definition, None).unwrap();
        let before = surface.clone();
        assert!(surface.set_cache_fit_tolerance(Some(0.25)).is_err());
        assert_eq!(surface, before);
        let mut wire = serde_json::to_value(surface).unwrap();
        wire["cache_fit_tolerance"] = serde_json::json!(0.25);
        assert!(serde_json::from_value::<ProceduralSurface>(wire).is_err());
    }
}

#[test]
fn rejected_surface_tolerance_edits_preserve_the_owner() {
    let mut surface = ProceduralSurface::try_new(
        surface_id(),
        law(LawSurfaceTail::Full {}),
        Some(1.0e300),
        None,
    )
    .unwrap();
    let before = surface.clone();
    for value in [f64::INFINITY, f64::NAN, -1.0] {
        assert!(surface.set_cache_fit_tolerance(Some(value)).is_err());
        assert_eq!(surface, before);
    }
    for scale in [1.0e300, f64::INFINITY, f64::NAN, -1.0] {
        assert!(surface.scale_cache_fit_tolerance(scale).is_err());
        assert_eq!(surface, before);
    }
    assert!(surface.set_cache_fit_tolerance(None).is_err());
    assert_eq!(surface, before);
    assert!(surface
        .replace_definition(law(LawSurfaceTail::Historical {}))
        .is_err());
    assert_eq!(surface, before);
    assert!(surface
        .edit_definition(|definition| *definition = law(LawSurfaceTail::Optimal {}))
        .is_err());
    assert_eq!(surface, before);
    surface
        .try_replace_definition(law(LawSurfaceTail::Optimal {}), None)
        .unwrap();
    assert_eq!(surface.cache_fit_tolerance(), None);
    let before = surface.clone();
    assert!(surface
        .edit_definition(|definition| *definition = law(LawSurfaceTail::Full {}))
        .is_err());
    assert_eq!(surface, before);
}

#[test]
fn rejected_curve_tolerance_scaling_preserves_the_owner() {
    let mut curve = ProceduralCurve::try_new(
        ProceduralCurveId::mint("synthetic:test:procedural_curve#tolerance").unwrap(),
        ProceduralCurveDefinition::Exact,
        Some(1.0e300),
    )
    .unwrap();
    let before = curve.clone();
    for scale in [1.0e300, f64::INFINITY, f64::NAN, -1.0] {
        assert!(curve.scale_cache_fit_tolerance(scale).is_err());
        assert_eq!(curve, before);
    }
    curve.raise_cache_fit_tolerance(FitTolerance::try_new(f64::MAX).unwrap());
    assert_eq!(curve.cache_fit_tolerance(), Some(f64::MAX));
}
