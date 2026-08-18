// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::eval::pcurve_uv;
use cadmpeg_ir::geometry::{PcurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, RegionId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Vertex};
use cadmpeg_ir::units::Units;
use std::collections::HashSet;
use std::io::Cursor;

use crate::loss::StepLossCode;

fn surface_draft(id: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft
        .insert(Surface {
            id: SurfaceId(id.into()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        })
        .expect("insert surface into draft");
    draft
}

#[test]
fn cross_root_surface_filter_tracks_successful_commits_only() {
    let committed_id = "step:data:surface#implicit-face-1";
    let rejected_id = "step:data:surface#implicit-face-2";
    let mut ir = CadIr::empty(Units::default());
    let mut session = CommitSession::new(&ir);

    session
        .commit_model(surface_draft(committed_id), &mut ir)
        .expect("first root commit");
    let mut second_root = surface_draft(committed_id);
    drop_committed_surfaces(&mut second_root, &session, &ir);
    assert!(second_root.model().surfaces.is_empty());

    let mut rejected_root = surface_draft(rejected_id);
    rejected_root
        .insert(Vertex {
            id: "step:data:vertex#rejected".into(),
            point: "step:data:point#missing".into(),
            tolerance: None,
        })
        .expect("insert invalid root reference");
    assert!(session.commit_model(rejected_root, &mut ir).is_err());

    let mut later_root = surface_draft(rejected_id);
    drop_committed_surfaces(&mut later_root, &session, &ir);
    assert_eq!(later_root.model().surfaces.len(), 1);
}

#[test]
fn trimmed_pcurve_fit_uses_declared_endpoints() {
    let surface_id = SurfaceId("step:data:surface#trimmed-endpoints".into());
    let surface_geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Trimmed {
        parameter_range: [
            std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2,
        ],
        same_sense: true,
        basis: Box::new(PcurveGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 1.0,
        }),
    };

    let fit = pcurve_declared_endpoint_fit(
        &ModelIndex::new(&ir),
        &surface_id,
        &pcurve,
        [
            std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2,
        ],
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    )
    .expect("declared pcurve endpoints should be evaluable");

    assert!(fit <= 2.0 * f64::EPSILON);
}

#[test]
fn bounded_pcurve_search_can_miss_an_unsampled_exact_point() {
    let surface_id = SurfaceId("step:data:surface#bounded-search-witness".into());
    let surface_geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });

    // The polar harmonic has a stationary chart tangent at seed 0. Its
    // exact point at pi is outside the default seed set, so the bounded
    // Newton loop cannot prove that the seed result is the global minimum.
    let pcurve = PcurveGeometry::PolarHarmonic {
        radial_center: Point2::new(2.0, -1.0),
        radial_cos: Point2::new(0.0, 1.0),
        radial_sin: Point2::new(1.0, 0.0),
        axial_origin: 0.0,
        axial_cos: 0.0,
        axial_sin: 0.0,
    };
    let exact_parameter = std::f64::consts::PI;
    let exact_uv = pcurve_uv(&pcurve, exact_parameter).expect("witness pcurve is evaluable");
    let target = Point3::new(exact_uv.u, exact_uv.v, 0.0);
    let index = ModelIndex::new(&ir);
    let seeds = pcurve_selection_seeds(&index, &surface_id, &pcurve, &surface_geometry);
    assert_eq!(seeds, vec![0.0]);
    let bounded = pcurve_surface_closest(&index, &surface_id, &pcurve, target, &seeds)
        .expect("bounded search returns an evaluated witness");
    assert!(bounded.0 > cadmpeg_ir::units::COINCIDENCE_TOLERANCE);
    let exact = pcurve_uv(&pcurve, exact_parameter).expect("exact point remains evaluable");
    assert!(Point3::new(exact.u, exact.v, 0.0).distance(target) <= f64::EPSILON);
}

#[test]
fn stale_trim_recovery_is_retained_above_step_tolerance() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
        "#70=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'distance_accuracy_value','');\n#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#70)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
    )
    .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);")
    .replace(
        "#54=LINE('',#51,#53);",
        "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.005)),.T.,.PARAMETER.);\n#71=LINE('',#51,#53);",
    );
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode stale trim with document tolerance");
    assert!((decoded.ir().tolerances.linear - 0.1).abs() <= f64::EPSILON);
    let use_ = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .find(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
        .expect("stale trimmed pcurve use");
    assert_eq!(use_.parameter_range, Some([0.0, 1.0]));
}

#[test]
fn finite_pcurve_admission_marks_unsampled_global_divergence() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_unsampled_divergence.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode unsampled-divergence pcurve witness");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#10")
        .expect("unsampled-divergence edge use");
    assert_eq!(edge_use.pcurves.len(), 1);
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#27")
        .expect("unsampled-divergence pcurve");
    let parameter_range = match &pcurve.geometry {
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => *parameter_range,
        other => panic!("expected trimmed pcurve, got {other:?}"),
    };
    let surface_id = decoded
        .ir()
        .model
        .surfaces
        .first()
        .expect("pcurve plane")
        .id
        .clone();
    let (curve_center, curve_radius) = decoded
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match curve.geometry {
            CurveGeometry::Circle { center, radius, .. } => Some((center, radius)),
            _ => None,
        })
        .expect("3D circle carrier");
    let index = ModelIndex::new(decoded.ir());
    let point_set_residual = |fraction: f64| {
        let parameter = parameter_range[0].mul_add(1.0 - fraction, parameter_range[1] * fraction);
        let uv = pcurve_uv(&pcurve.geometry, parameter).expect("evaluate pcurve");
        let mapped = model_surface_point_by_id(&index, &surface_id, uv.u, uv.v)
            .expect("map pcurve through plane");
        (mapped.distance(curve_center) - curve_radius).abs()
    };

    for sample in 0..PCURVE_LOCUS_SAMPLE_COUNT {
        let fraction = sample as f64 / (PCURVE_LOCUS_SAMPLE_COUNT - 1) as f64;
        assert!(point_set_residual(fraction) <= COINCIDENCE_TOLERANCE);
    }
    for gap in 0..(PCURVE_LOCUS_SAMPLE_COUNT - 1) {
        let fraction = (gap as f64 + 0.5) / (PCURVE_LOCUS_SAMPLE_COUNT - 1) as f64;
        assert!(point_set_residual(fraction) > 1.0);
    }

    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind())
        .expect("finite admission loss");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
}

/// Strict decode keeps a finitely admitted pcurve: the relation transfers the
/// source data with its verification status, and strict decode refuses only
/// CADIR-introduced substitution, salvage, and malformed structure.
#[test]
fn strict_decode_accepts_a_finitely_admitted_pcurve() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_unsampled_divergence.p21")),
            &options,
        )
        .expect("strict decode accepts an admitted pcurve");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#10")
        .expect("unsampled-divergence edge use");
    assert_eq!(edge_use.pcurves.len(), 1);
    let admissions = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind())
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), 1);
    assert_eq!(
        admissions[0].severity,
        cadmpeg_ir::report::Severity::Warning
    );
    assert_eq!(
        admissions[0].strict_consequence(),
        cadmpeg_ir::report::StrictConsequence::Tolerate
    );
}

#[test]
fn divergent_interior_pcurve_is_omitted_from_coedge() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_divergent_interior.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode divergent-interior pcurve witness");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("divergent pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveLocusDiscontinuous.kind()
            && loss.message.contains("bounded model-space locus")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#56"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn competing_same_surface_pcurves_remain_detached() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_competing_pcurves.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode competing pcurve witness");

    assert!(decoded.ir().model.pcurves.is_empty());
    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("competing pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("2 pcurves")
            && loss.message.contains("surface #28")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#56"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn assert_tp09_competing_pcurves_are_order_independent(source: &[u8]) {
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode competing pcurve witness");

    assert!(decoded.ir().model.pcurves.is_empty());
    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("competing pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("2 pcurves")
            && loss.message.contains("surface #28")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#56"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reordered_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_reordered.p21"
    ));
}

#[test]
fn near_tied_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_near_tied.p21"
    ));
}

#[test]
fn crossing_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_crossing.p21"
    ));
}

#[test]
fn shared_step_pcurve_mismatch_omits_optional_use() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc04_shared_pcurve.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode shared pcurve witness");

    let source = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#33")
        .expect("shared source pcurve");
    assert!(matches!(
        &source.geometry,
        PcurveGeometry::Trimmed {
            parameter_range,
            same_sense: true,
            ..
        } if *parameter_range == [0.0, 1.0]
    ));

    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| !pcurve.id.as_str().contains("-use-")));

    let first_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#42")
        .expect("first shared pcurve coedge");
    assert_eq!(first_use.pcurves.len(), 1);
    assert_eq!(first_use.pcurves[0].pcurve.as_str(), "step:data:pcurve#33");
    let second_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#45")
        .expect("second shared pcurve coedge");
    assert!(second_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #35")
            && loss.message.contains("surface #26")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reordered_shared_step_pcurve_mismatch_omits_optional_use() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc04_shared_pcurve_reordered.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode reordered shared pcurve witness");

    let source = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#33")
        .expect("reordered shared source pcurve");
    assert!(matches!(
        &source.geometry,
        PcurveGeometry::Trimmed {
            parameter_range,
            same_sense: true,
            ..
        } if *parameter_range == [0.0, 1.0]
    ));

    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| !pcurve.id.as_str().contains("-use-")));

    let uses = decoded
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge.as_str() == "step:data:edge#42")
        .map(|coedge| coedge.pcurves.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].len(), 1);
    assert_eq!(uses[0][0].pcurve.as_str(), "step:data:pcurve#33");
    let second_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#45")
        .expect("reordered second shared pcurve coedge");
    assert!(second_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #35")
            && loss.message.contains("surface #26")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_surface_carrier_is_staged_once() {
    let surface = Surface {
        id: SurfaceId("step:data:surface#shared".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let body_id = BodyId("step:data:body#shared-surface".into());
    let region_id = RegionId("step:data:region#shared-surface".into());
    let built = super::super::staged_topology(
        HashSet::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![surface.clone(), surface],
        Vec::new(),
        Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: Vec::new(),
        },
        Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )
    .expect("duplicate references to one source surface must stage");

    assert_eq!(built.draft.model().surfaces.len(), 1);
}
