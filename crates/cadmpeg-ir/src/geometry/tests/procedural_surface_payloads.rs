// SPDX-License-Identifier: Apache-2.0
use crate::geometry::laws::{LawExpression, LawFormula, LawSurfaceConstruction, LawSurfaceTail};
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use crate::geometry::ProceduralSurface;
use crate::ids::{ProceduralSurfaceId, SurfaceId};
use cadmpeg_core::serde_erase::json;

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:procedural_surface#payload").unwrap()
}

fn subset(
    ranges: [[f64; 2]; 2],
) -> Result<ProceduralSurfaceDefinition, crate::geometry::legacy_cache::ProceduralGeometryError> {
    Ok(ProceduralSurfaceDefinition::Subset(
        crate::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
            SurfaceId::mint("synthetic:test:surface#support").unwrap(),
            ranges,
            None,
            None,
            None,
        )?,
    ))
}

#[test]
fn surface_payload_admission_enforces_directed_nonzero_subset_ranges() {
    let definition = subset([[2.0, -1.0], [0.0, 1.0]]).unwrap();
    let surface = ProceduralSurface::new(id(), definition.clone(), None);
    let wire = json::to_value(&definition).unwrap();
    assert_eq!(
        wire["parameter_ranges"],
        cadmpeg_core::json!([[2.0, -1.0], [0.0, 1.0]])
    );
    assert_eq!(
        json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    let before = surface.clone();
    for range in [[0.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(subset([range, [0.0, 1.0]]).is_err());
        let mut invalid = json::to_value(&before).unwrap();
        invalid["definition"]["parameter_ranges"][0] = cadmpeg_core::json!(range);
        assert!(
            json::from_value::<ProceduralSurfaceDefinition>(invalid["definition"].clone()).is_err()
        );
        assert!(json::from_value::<ProceduralSurface>(invalid).is_err());
    }
    let mut wire = json::to_value(&before).unwrap();
    wire["definition"]["parameter_ranges"][0] = cadmpeg_core::json!([1.0, 1.0]);
    assert!(json::from_value::<ProceduralSurfaceDefinition>(wire["definition"].clone()).is_err());
    assert!(json::from_value::<ProceduralSurface>(wire).is_err());
}

#[test]
fn surface_law_admission_preserves_the_depth_boundary() {
    let mut expression = LawExpression::Null {};
    for _ in 0..64 {
        expression = LawExpression::Algebraic {
            operator: "+".into(),
            operands: vec![expression],
        };
    }
    let construction = |expression| LawSurfaceConstruction {
        parameter_ranges: None,
        primary: LawFormula::Named {
            name: cadmpeg_core::nonblank_literal!("test"),
            variables: vec![expression],
        },
        additional: Vec::new(),
        tail: LawSurfaceTail::Historical {},
        discontinuities: std::array::from_fn(|_| Vec::new()),
    };
    let law = |expression| {
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(construction(
            expression,
        )))
        .map(ProceduralSurfaceDefinition::Law)
    };
    let _surface: ProceduralSurface =
        ProceduralSurface::new(id(), law(expression.clone()).unwrap(), None);
    expression = LawExpression::Algebraic {
        operator: "+".into(),
        operands: vec![expression],
    };
    assert!(law(expression).is_err());
    // A blank law text has no spelling: the member type refuses it on the wire,
    // so no construction can carry one. Whitespace alone is blank too.
    for blank in ["", " ", "\t"] {
        assert!(json::from_value::<LawExpression>(
            cadmpeg_core::json!({"kind": "text", "value": blank})
        )
        .is_err());
    }
    let _surface: ProceduralSurface = ProceduralSurface::new(
        id(),
        law(LawExpression::Text {
            value: cadmpeg_core::nonblank_literal!("x "),
        })
        .unwrap(),
        None,
    );
}

#[test]
fn linear_sweep_admission_requires_a_finite_nondegenerate_direction() {
    use crate::geometry::surface_payloads::LinearSweepSurfaceConstruction;
    use crate::ids::CurveId;
    use crate::math::Vector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let sweep = |direction| {
        LinearSweepSurfaceConstruction::try_new(directrix.clone(), direction)
            .map(ProceduralSurfaceDefinition::LinearSweep)
    };
    let valid = sweep(Vector3::new(0.0, 0.0, 2.0)).unwrap();
    let surface = ProceduralSurface::new(id(), valid.clone(), None);
    let wire = json::to_value(&surface).unwrap();
    assert_eq!(
        wire["definition"]["direction"],
        cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 2.0})
    );
    assert_eq!(
        json::from_value::<ProceduralSurface>(wire.clone()).unwrap(),
        surface
    );
    assert!(sweep(Vector3::new(f64::EPSILON, 0.0, 0.0)).is_err());
    assert!(sweep(Vector3::new(2.0 * f64::EPSILON, 0.0, 0.0)).is_ok());
    for direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 0.0, 1.0),
        Vector3::new(0.0, f64::INFINITY, 0.0),
    ] {
        assert!(sweep(direction).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["direction"] = json::to_value(direction).unwrap();
        assert!(
            json::from_value::<ProceduralSurfaceDefinition>(invalid["definition"].clone()).is_err()
        );
        assert!(json::from_value::<ProceduralSurface>(invalid).is_err());
    }
}

#[test]
fn rejected_surface_definition_changes_preserve_serialized_owner() {
    let mut definition = subset([[0.0, 1.0], [0.0, 1.0]]).unwrap();
    definition
        .set_legacy_cache(Some(
            crate::geometry::legacy_cache::LegacyCache::try_new(0.5).unwrap(),
        ))
        .unwrap();
    let surface = ProceduralSurface::new(id(), definition, None);
    let before = json::to_vec(&surface).unwrap();
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        let mut replacement = subset([[2.0, 3.0], [4.0, 5.0]]).unwrap();
        assert!(crate::geometry::legacy_cache::LegacyCache::try_new(tolerance).is_err());
        assert!(replacement.set_cache_fit_tolerance(None).is_ok());
        assert_eq!(json::to_vec(&surface).unwrap(), before);
    }
    let mut incompatible = ProceduralSurfaceDefinition::Law(
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(
            LawSurfaceConstruction {
                parameter_ranges: None,
                primary: LawFormula::Null {},
                additional: Vec::new(),
                tail: LawSurfaceTail::Historical {},
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        ))
        .unwrap(),
    );
    assert!(incompatible
        .set_legacy_cache(Some(
            crate::geometry::legacy_cache::LegacyCache::try_new(0.5).unwrap()
        ))
        .is_err());
    assert_eq!(json::to_vec(&surface).unwrap(), before);
}
