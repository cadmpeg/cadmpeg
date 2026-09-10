// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{CurveOffsetRange, OffsetSide, ProceduralCurve, ProceduralCurveDefinition};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::Vector3;

fn id() -> ProceduralCurveId {
    ProceduralCurveId::mint("synthetic:test:procedural_curve#payload").unwrap()
}

fn source() -> CurveId {
    CurveId::mint("synthetic:test:curve#source").unwrap()
}

fn subset(range: [f64; 2]) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Subset(
        crate::geometry::curve_payloads::SubsetCurveConstruction::try_new(source(), range, false)
            .unwrap(),
    )
}

#[test]
fn curve_payload_admission_requires_finite_ordered_subset_ranges() {
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    let curve = ProceduralCurve::try_new(id(), subset([1.0, 1.0]), Some(0.5)).unwrap();
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(
        wire["definition"]["parameter_range"],
        serde_json::json!([1.0, 1.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurve>(wire.clone()).unwrap(),
        curve
    );
    for range in [[1.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(SubsetCurveConstruction::try_new(source(), range, false).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["parameter_range"] = serde_json::json!(range);
        assert!(
            serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone())
                .is_err()
        );
        assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
    }
}

#[test]
fn a_distance_law_without_its_parameter_range_has_no_encoding() {
    use crate::geometry::curve_payloads::OffsetCurveConstruction;
    let side = OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    };
    let uniform = ProceduralCurveDefinition::Offset(
        OffsetCurveConstruction::try_new(
            source(),
            -2.0,
            side.clone(),
            Some(CurveOffsetRange::Uniform {
                parameter_range: [0.0, 1.0],
            }),
        )
        .unwrap(),
    );
    let wire = serde_json::to_value(&uniform).unwrap();
    assert_eq!(wire["range"]["kind"], "uniform");
    assert!(wire["range"].get("distance_law").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        uniform
    );

    let absent = ProceduralCurveDefinition::Offset(
        OffsetCurveConstruction::try_new(source(), -2.0, side, None).unwrap(),
    );
    let absent_wire = serde_json::to_value(&absent).unwrap();
    assert!(absent_wire.get("range").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(absent_wire).unwrap(),
        absent
    );

    let mut law_without_range = wire.clone();
    law_without_range["range"] = serde_json::json!({"kind": "variable"});
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(law_without_range).is_err());

    let mut bogus = wire;
    bogus["range"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<ProceduralCurveDefinition>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn offset_payload_preserves_direction_magnitude_and_requires_strict_ranges() {
    use crate::geometry::curve_payloads::OffsetCurveConstruction;
    let definition = |side, range| {
        OffsetCurveConstruction::try_new(source(), -2.0, side, range)
            .map(ProceduralCurveDefinition::Offset)
    };
    let direction = OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    };
    let valid = definition(direction.clone(), None).unwrap();
    assert!(ProceduralCurve::new(id(), valid.clone()).is_ok());
    for range in [[1.0, 0.0], [0.0, 0.0]] {
        assert!(definition(
            direction.clone(),
            Some(CurveOffsetRange::Uniform {
                parameter_range: range
            })
        )
        .is_err());
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire["range"] = serde_json::json!({"kind": "uniform", "parameter_range": range});
        assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
    }
    assert!(definition(OffsetSide::PlaneNormal(Vector3::new(0.0, 0.0, 2.0)), None).is_err());
    assert!(ProceduralCurve::new(
        id(),
        definition(OffsetSide::PlaneNormal(Vector3::new(0.0, 0.0, 1.0)), None).unwrap()
    )
    .is_ok());
    assert!(definition(
        OffsetSide::Direction {
            direction: Vector3::new(0.0, 0.0, 0.0),
            support: None
        },
        None
    )
    .is_err());
}

#[test]
fn intersection_context_mutation_keeps_checked_ranges_and_cache_tolerance() {
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide};
    use crate::ids::SurfaceId;

    let mut curve = ProceduralCurve::try_new(
        id(),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext::try_new(
                std::array::from_fn(|_| IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                }),
                [0.0, 1.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap(),
            discontinuity_flag: false,
        },
        Some(0.5),
    )
    .unwrap();
    let support = SurfaceId::mint("synthetic:test:surface#support").unwrap();
    let context = curve.intersection_context_mut().unwrap();
    context.set_surface(0, Some(support.clone()));
    assert!(context.edit(|_, range, _| *range = [1.0, 0.0]).is_err());
    assert_eq!(context.parameter_range(), [0.0, 1.0]);
    assert_eq!(context.sides()[0].surface.as_ref(), Some(&support));
    assert_eq!(curve.cache_fit_tolerance(), Some(0.5));
    assert!(ProceduralCurve::new(id(), subset([0.0, 1.0]))
        .unwrap()
        .intersection_context_mut()
        .is_none());
}

#[test]
fn silhouette_admission_requires_a_nondegenerate_light_direction_and_finite_draft() {
    use crate::geometry::curve_payloads::SilhouetteCurveConstruction;
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide, SilhouetteKind};
    use crate::ids::SurfaceId;
    use crate::scalar::FiniteReal;

    let context = || {
        IntcurveSupportContext::try_new(
            std::array::from_fn(|_| IntcurveSupportSide {
                surface: None,
                pcurve: None,
            }),
            [0.0, 1.0],
            std::array::from_fn(|_| Vec::new()),
        )
        .unwrap()
    };
    let cast_surface = SurfaceId::mint("synthetic:test:surface#cast").unwrap();
    let silhouette = |kind, light_direction| {
        SilhouetteCurveConstruction::try_new(context(), kind, cast_surface.clone(), light_direction)
            .map(ProceduralCurveDefinition::Silhouette)
    };
    let valid = silhouette(
        SilhouetteKind::Taper {
            draft_factor: FiniteReal::new(0.5).unwrap(),
        },
        Vector3::new(0.0, 0.0, 2.0),
    )
    .unwrap();
    let curve = ProceduralCurve::new(id(), valid).unwrap();
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(
        wire["definition"]["silhouette"]["draft_factor"],
        serde_json::json!(0.5)
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurve>(wire.clone()).unwrap(),
        curve
    );
    for light_direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 0.0, 1.0),
        Vector3::new(0.0, f64::INFINITY, 0.0),
    ] {
        assert!(silhouette(SilhouetteKind::Standard, light_direction).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["light_direction"] = serde_json::to_value(light_direction).unwrap();
        assert!(
            serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone())
                .is_err()
        );
        assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
    }
    assert!(FiniteReal::new(f64::NAN).is_none());
    let mut invalid = wire;
    invalid["definition"]["silhouette"]["draft_factor"] = serde_json::json!("nan");
    assert!(
        serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone()).is_err()
    );
    assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
}

#[test]
fn rejected_curve_definition_replacements_preserve_serialized_owner() {
    let mut curve = ProceduralCurve::try_new(id(), subset([0.0, 1.0]), Some(0.5)).unwrap();
    let before = serde_json::to_vec(&curve).unwrap();
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(curve
            .try_replace_definition(subset([2.0, 3.0]), Some(tolerance))
            .is_err());
        assert_eq!(serde_json::to_vec(&curve).unwrap(), before);
    }
}
