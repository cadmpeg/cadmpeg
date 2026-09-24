// SPDX-License-Identifier: Apache-2.0
use super::{
    DeformableCurveConstruction, ProceduralGeometryError, ProjectionCurvePayload,
    SpringCurvePayload, SurfaceOffsetCurveConstruction, ThreeSurfaceIntersectionCurvePayload,
};
use crate::geometry::{
    pcurve::{LinePcurve, PcurveGeometry},
    CacheContract, CacheFirstCurveForm, CacheFirstCurveParameterization, DeformableCurveData,
    DeformableCurveSource, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurveDefinition, ProjectionRole, ProjectionTail, RevisionCacheForm, SpringLayout,
    SpringPcurve, SpringSupport, SupportPcurve,
};
use crate::ids::CurveId;
use crate::math::Point2;
fn context(range: [f64; 2]) -> IntcurveSupportContext {
    IntcurveSupportContext::try_new(
        std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
        }),
        range,
        std::array::from_fn(|_| Vec::new()),
    )
    .unwrap()
}

#[test]
fn third_ranged_pcurve_requires_a_nonzero_context_interval() {
    let third = |ranged: bool| IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
            ),
            ranged.then(|| DirectedParameterRange::new([2.0, -1.0]).unwrap()),
        )),
    };
    assert!(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([1.0, 1.0]), 0, third(false)).is_ok()
    );
    assert!(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([1.0, 1.0]), 0, third(true)).is_err()
    );
    let valid = ProceduralCurveDefinition::ThreeSurfaceIntersection(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([0.0, 1.0]), 0, third(true)).unwrap(),
    );
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    wire["context"]["parameter_range"] = serde_json::json!([1.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}

#[test]
fn projection_payload_admits_finite_ordered_source_intervals() {
    let projection = |range| {
        ProjectionCurvePayload::try_new(
            context([0.0, 0.0]),
            false,
            CurveId::mint("synthetic:test:curve#source").unwrap(),
            ProjectionTail::Ranged {
                flag: false,
                parameter_range: range,
                role: ProjectionRole::Surf1,
            },
        )
    };
    let valid = ProceduralCurveDefinition::Projection(projection([1.0, 1.0]).unwrap());
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    for range in [[2.0, 1.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(projection(range).is_err());
    }
    wire["tail"]["parameter_range"] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}

#[test]
fn spring_payload_checks_inline_ranges_and_the_shared_context() {
    let spring = |range, shared| {
        SpringCurvePayload::try_new(
            SpringLayout::ContextFirst {
                supports: std::array::from_fn(|_| SpringSupport::Ranges([range, [0.0, 1.0]])),
                first_pcurve: SpringPcurve::Range(range),
                second_pcurve: None,
                parameter_range: shared,
                discontinuities: std::array::from_fn(|_| Vec::new()),
                discontinuity_flag: false,
                cache: None,
            },
            0,
        )
    };
    let valid = ProceduralCurveDefinition::Spring(spring([1.0, 1.0], [0.0, 0.0]).unwrap());
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    for range in [[2.0, 1.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(spring(range, [0.0, 1.0]).is_err());
        assert!(spring([0.0, 1.0], range).is_err());
    }
    wire["layout"]["first_pcurve"]["value"] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}

fn cache_first_form(degenerate: Option<(usize, f64)>) -> CacheFirstCurveForm {
    let mut support_bounds = [[None; 4]; 2];
    support_bounds[0][0] = Some(0.0);
    let mut solved_range = [Some(1.0), None];
    let mut interval = [Some(2.0), None];
    match degenerate {
        Some((0, value)) => support_bounds[0][0] = Some(value),
        Some((1, value)) => solved_range[0] = Some(value),
        Some((_, value)) => interval[0] = Some(value),
        None => {}
    }
    CacheFirstCurveForm {
        revision: crate::scalar::PositiveI64::new(23_100).unwrap(),
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval,
            closed_form: 0,
        }),
        support_bounds,
        solved_range,
        extension: 7,
    }
}

#[test]
fn cache_first_curve_form_admits_the_full_positive_revision_lane() {
    let form = cache_first_form(None);
    let wire = serde_json::to_value(form).unwrap();
    for revision in [0_i64, -1] {
        assert!(crate::scalar::PositiveI64::new(revision).is_none());
        let mut invalid = wire.clone();
        invalid["revision"] = serde_json::json!(revision);
        assert!(serde_json::from_value::<CacheFirstCurveForm>(invalid).is_err());
    }

    let maximum = CacheFirstCurveForm {
        revision: crate::scalar::PositiveI64::new(i64::MAX).expect("maximum is positive"),
        ..cache_first_form(None)
    };
    assert_eq!(maximum.revision.get(), i64::MAX);
    let mut maximum_wire = wire;
    maximum_wire["revision"] = serde_json::json!(i64::MAX);
    assert_eq!(
        serde_json::from_value::<CacheFirstCurveForm>(maximum_wire)
            .expect("maximum JSON revision is admitted")
            .revision
            .get(),
        i64::MAX
    );
}

fn surface_offset(
    form: CacheFirstCurveForm,
    base_endpoints: [Option<f64>; 2],
) -> Result<SurfaceOffsetCurveConstruction, ProceduralGeometryError> {
    SurfaceOffsetCurveConstruction::try_new(
        context([0.0, 1.0]),
        false,
        [[0.0, 1.0], [0.0, 1.0]],
        (
            CurveId::mint("synthetic:test:curve#base").unwrap(),
            [0.0, 1.0],
            base_endpoints,
        ),
        CacheContract::from_form(Some(form)),
        1.5,
        [0.25, 2.0],
    )
}

fn surface_offset_wire(
    form: CacheFirstCurveForm,
    base_endpoints: [Option<f64>; 2],
) -> Result<SurfaceOffsetCurveConstruction, ProceduralGeometryError> {
    SurfaceOffsetCurveConstruction::try_from(super::SurfaceOffsetCurveConstructionWire {
        context: context([0.0, 1.0]),
        discontinuity_flag: false,
        base_u_range: [0.0, 1.0],
        base_v_range: [0.0, 1.0],
        base: CurveId::mint("synthetic:test:curve#base").unwrap(),
        base_range: [0.0, 1.0],
        base_endpoints,
        cache: CacheContract::from_form(Some(form)),
        distance: 1.5,
        shift: 0.25,
        scale: 2.0,
    })
}

fn deformable_data() -> DeformableCurveData {
    DeformableCurveData::VectorField {
        vectors: std::array::from_fn(|_| crate::math::Vector3::new(0.0, 0.0, 1.0)),
        parameter_pairs: Vec::new(),
    }
}

fn deformable_source() -> DeformableCurveSource {
    DeformableCurveSource::Curve {
        curve: CurveId::mint("synthetic:test:curve#source").unwrap(),
    }
}

fn deformable(
    form: CacheFirstCurveForm,
) -> Result<DeformableCurveConstruction, ProceduralGeometryError> {
    DeformableCurveConstruction::try_new(
        context([0.0, 1.0]),
        form,
        deformable_source(),
        [None, None],
        deformable_data(),
    )
}

fn deformable_wire(
    form: CacheFirstCurveForm,
) -> Result<DeformableCurveConstruction, ProceduralGeometryError> {
    DeformableCurveConstruction::try_from(super::DeformableCurveConstructionWire {
        context: context([0.0, 1.0]),
        cache_first: form,
        source: deformable_source(),
        source_parameter_range: [None, None],
        data: deformable_data(),
    })
}

fn spring_layout(form: CacheFirstCurveForm) -> SpringLayout {
    SpringLayout::CacheFirst {
        context: context([0.0, 1.0]),
        form,
    }
}

fn spring_cache_first(
    form: CacheFirstCurveForm,
) -> Result<SpringCurvePayload, ProceduralGeometryError> {
    SpringCurvePayload::try_new(spring_layout(form), 0)
}

fn spring_cache_first_wire(
    form: CacheFirstCurveForm,
) -> Result<SpringCurvePayload, ProceduralGeometryError> {
    SpringCurvePayload::try_from(super::SpringCurvePayloadWire {
        layout: spring_layout(form),
        direction: 0,
    })
}

#[test]
fn the_cache_first_curve_admissions_refuse_every_non_finite_form_scalar() {
    let admitted = ProceduralCurveDefinition::SurfaceOffset(
        surface_offset(cache_first_form(None), [Some(0.5), None]).unwrap(),
    );
    let wire = serde_json::to_value(&admitted).unwrap();
    assert_eq!(wire["base_endpoints"], serde_json::json!([0.5, null]));
    assert_eq!(
        wire["cache"]["form"]["support_bounds"][0],
        serde_json::json!([0.0, null, null, null])
    );
    assert_eq!(
        wire["cache"]["form"]["solved_range"],
        serde_json::json!([1.0, null])
    );
    // `RevisionCacheForm` is internally tagged and its parameterization is a
    // newtype variant, so the interval is one level up from the variant name.
    assert_eq!(
        wire["cache"]["form"]["cache"]["interval"],
        serde_json::json!([2.0, null])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire).unwrap(),
        admitted
    );

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for family in 0..3 {
            let form = || cache_first_form(Some((family, value)));
            assert!(surface_offset(form(), [None, None]).is_err());
            assert!(surface_offset_wire(form(), [None, None]).is_err());
            assert!(deformable(form()).is_err());
            assert!(deformable_wire(form()).is_err());
            assert!(spring_cache_first(form()).is_err());
            assert!(spring_cache_first_wire(form()).is_err());
        }
        for endpoints in [[Some(value), None], [None, Some(value)]] {
            assert!(surface_offset(cache_first_form(None), endpoints).is_err());
            assert!(surface_offset_wire(cache_first_form(None), endpoints).is_err());
        }
    }
}

#[test]
fn a_direction_offset_from_admitted_parts_matches_its_raw_admission() {
    use super::OffsetCurveConstruction;
    use crate::geometry::{CurveOffsetRange, OffsetSide};
    use crate::ids::{CurveId, SurfaceId};
    use crate::math::Vector3;
    use crate::scalar::FiniteReal;
    use crate::topology::IncreasingParameterInterval;
    use crate::units::UnitVector3;

    let source = CurveId::mint("synthetic:test:curve#source").unwrap();
    let direction = UnitVector3::new(Vector3::new(0.0, 0.6, 0.8)).unwrap();
    let range = IncreasingParameterInterval::new([2.0, 5.0]).unwrap();
    for support in [
        None,
        Some(SurfaceId::mint("synthetic:test:surface#support").unwrap()),
    ] {
        assert_eq!(
            OffsetCurveConstruction::try_new(
                source.clone(),
                -1.25,
                OffsetSide::Direction {
                    direction: *direction.as_raw(),
                    support: support.clone(),
                },
                Some(CurveOffsetRange::uniform(range.endpoints()).unwrap()),
            ),
            Ok(OffsetCurveConstruction::along_direction(
                source.clone(),
                FiniteReal::new(-1.25).unwrap(),
                direction,
                support,
                range,
            ))
        );
    }
}
