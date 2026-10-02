// SPDX-License-Identifier: Apache-2.0
//! Structural and numeric validation for [`CadIr`].
//!
//! Validation checks identity and arena order, references, topology rings,
//! carrier reachability, annotations, native links, parameter
//! domains, payload integrity, tessellation, numeric bounds, and geometric
//! consistency (edge-curve endpoints and pcurve surface images against vertex
//! positions). It does not evaluate interior surface membership or solid
//! closure.

use std::collections::BTreeMap;

use crate::document::{CadIr, CensusKey};
use crate::report::{
    check::{Check, Finding, ValidationReport},
    loss::LossNote,
    Severity,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

/// Narrow admissibility predicates as documented `Check` subsets.
pub mod admit;
mod annotations_native;
mod carriers_parameterization;
mod drawings;
pub(crate) mod evaluation_cycles;
mod geometry_consistency;
mod geometry_payloads;
mod identity_order;
mod identities;
mod pmi;
mod presentation;
mod products;
mod referential_integrity;
mod semantic_annotations;
mod sketches;
mod spreadsheets;
mod topology;

use annotations_native::{check_annotations, check_native_links};
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use drawings::check_drawings;
use evaluation_cycles::check_evaluation_cycles;
use geometry_consistency::{
    check_edge_endpoint_consistency, check_pcurve_surface_consistency,
    check_procedural_support_consistency,
};
use geometry_payloads::check_tessellations;
use identity_order::check_identity_and_order;
use pmi::check_pmi;
use presentation::check_presentation;
use products::check_products;
use referential_integrity::check_typed_references;
use semantic_annotations::check_semantic_annotations;
use sketches::check_sketches;
use spreadsheets::check_spreadsheets;
use topology::{
    check_coedge_pairing, check_references, check_shell_connectivity, check_tolerances,
    check_topology_tolerances, check_wire_topology,
};

/// The parameter interval a pcurve carrier is defined on, when the carrier
/// states one.
///
/// A nesting carrier states its own interval where it has one and otherwise
/// reports its basis's: a trim is the interval the trimmed carrier exists on,
/// and a placement and an offset both keep their basis's parameterization. An
/// analytic carrier has no bounded domain. The match is exhaustive, so a new
/// pcurve variant states its answer here rather than inheriting one.
///
/// Both the carrier-parameterization pass and the geometric-consistency pass
/// ask this question of the same carrier, so they ask it of one function.
///
/// The recursion is bounded by the carrier: each pcurve nesting constructor
/// refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](crate::geometry::MAX_GEOMETRY_NESTING).
fn pcurve_parameter_domain(
    geometry: &crate::geometry::pcurve::PcurveGeometry,
) -> Option<crate::topology::IncreasingParameterInterval> {
    use crate::geometry::pcurve::PcurveGeometry;

    match geometry {
        PcurveGeometry::Nurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.control_points().len(),
        ),
        PcurveGeometry::PolarNurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.poles().len(),
        ),
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let [start, end] = trimmed_pcurve.parameter_range().finite_endpoints();
            crate::topology::IncreasingParameterInterval::between(start, end)
                .or_else(|| pcurve_parameter_domain(trimmed_pcurve.basis()))
        }
        PcurveGeometry::Offset(offset_pcurve) => pcurve_parameter_domain(offset_pcurve.basis()),
        PcurveGeometry::Transformed(placed) => pcurve_parameter_domain(placed.basis()),
        PcurveGeometry::Line(_)
        | PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::SphericalGreatCircle(_) => None,
    }
}

/// Record an error finding of `check` against one entity.
fn error_finding(findings: &mut Vec<Finding>, check: Check, entity: &str, message: &str) {
    findings.push(Finding {
        check,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}

fn record_finding(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    check: Check,
    severity: Severity,
    entity: &str,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_retained_vec(findings, 1, "validation finding storage")?;
    let message = ctx.format_retained(message, "validation finding message")?;
    let entity = ctx.copy_retained_text(entity, "validation finding identity")?;
    findings.push(Finding { check, severity, message, entity: Some(entity) });
    Ok(())
}

/// Count the records represented by the IR arenas without running validation.
///
/// Prefer [`CadIr::census`](crate::CadIr::census); this alias remains for
/// existing `cadmpeg_ir::entity_census` call sites.
pub fn entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
    crate::document::entity_census(ir)
}

macro_rules! define_validation_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn validation_census(ctx: &DecodeContext<'_>, view: crate::native::view::NativeView<'_>) -> Result<BTreeMap<CensusKey, usize>, CodecError> {
            let mut counts = BTreeMap::new();
            $(ctx.insert_btree_map(&mut counts, CensusKey::model(crate::document::ArenaName::registered(stringify!($field))), view.ir.model.$field.len(), "validation model census slots")?;)*
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(view.ir.model.surfaces.len()), "validation surface census scan")?;
            let unknown_surfaces = view.ir.model.surfaces.iter().filter(|surface| matches!(surface.geometry,
                crate::geometry::SurfaceGeometry::Solved(crate::geometry::SolvedSurfaceGeometry::Unknown { .. }))).count();
            ctx.insert_btree_map(&mut counts, CensusKey::surfaces_unknown_geometry(), unknown_surfaces, "validation surface census slot")?;
            view.visit(|work| ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "validation native census scan"), |format, arena, records| {
                if records.len() == 0 { return Ok(()); }
                let key = ctx.format_retained(format_args!("native.{format}.{arena}"), "validation native census key")?;
                let work = key.len().checked_add(1).and_then(|bytes| counts.len().checked_add(1).and_then(|count| bytes.checked_mul(count)))
                    .ok_or_else(|| ctx.refuse_codec_limit("validation census key comparisons", u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "validation census key comparisons")?;
                ctx.insert_btree_map(&mut counts, CensusKey::from_wire(key), records.len(), "validation native census slots")?;
                Ok(())
            })?;
            Ok(counts)
        }
    };
}
crate::document::arena_registry!(define_validation_census);

/// Validate `ir` and copy `losses` into the returned report unchanged.
fn validate_model(ctx: &DecodeContext<'_>, ir: &CadIr, losses: Vec<LossNote>) -> Result<ValidationReport, CodecError> {
    let index = crate::index::ModelIndex::new_for_decode(ir, ctx)?;
    validate_model_with_index(ctx, ir, losses, &index)
}

fn validate_model_with_index(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    losses: Vec<LossNote>,
    ids: &crate::index::ModelIndex<'_>,
) -> Result<ValidationReport, CodecError> {
    let _depth = ctx.enter_nested("IR neutral validation")?;
    ctx.charge_work(1, "IR neutral validation")?;
    let mut findings = Vec::new();

    // The identity walk enumerates every entity id in the product document;
    // native links resolve against that set.
    check_identity_and_order(ctx, ids.native_view(), &mut findings)?;
    check_tolerances(ir, &mut findings);
    check_references(ctx, ir, ids, &mut findings)?;
    check_evaluation_cycles(ctx, ir, ids, &mut findings)?;
    check_pmi(ir, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_shell_connectivity(ir, &mut findings);
    check_wire_topology(ir, &mut findings);
    check_carrier_reachability(ctx, ids.native_view(), &mut findings)?;
    check_native_links(ctx, ids.native_view(), ids, &mut findings)?;
    check_parameter_domains(ir, &mut findings);
    check_edge_endpoint_consistency(ir, &mut findings)?;
    check_pcurve_surface_consistency(ctx, ir, &mut findings)?;
    check_procedural_support_consistency(ir, &mut findings)?;
    check_topology_tolerances(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_sketches(ir, &mut findings)?;
    check_spreadsheets(ir, &mut findings);
    check_products(ir, &mut findings);
    check_presentation(ir, ids, &mut findings);
    check_drawings(ir, ids, &mut findings);
    check_semantic_annotations(ir, ids, &mut findings);
    check_typed_references(ir, ids, &mut findings);

    Ok(ValidationReport {
        entity_counts: validation_census(ctx, ids.native_view())?,
        findings,
        losses,
    })
}

/// Validate an application-owned document under the explicit standalone policy.
fn standalone_validation(
    validate: impl FnOnce(&DecodeContext<'_>) -> Result<ValidationReport, CodecError>,
) -> Result<ValidationReport, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let report = validate(&ctx)?;
    ctx.finish_session()?;
    Ok(report)
}

fn validate_annotations<'a>(
    ctx: &DecodeContext<'_>,
    ids: &crate::index::ModelIndex<'a>,
    annotations: &crate::annotations::Annotations,
    additional: impl IntoIterator<Item = &'a str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let all_ids = identities::BorrowedIdentities::build(ctx, |add| {
        for id in ids.identities().chain(additional) { add(id)?; }
        Ok(())
    })?;
    check_annotations(ctx, ids.native_view(), annotations, &all_ids, findings)

}

fn validate_model_with_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &crate::annotations::Annotations,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    let index = crate::index::ModelIndex::new_for_decode(ir, ctx)?;
    let mut report = validate_model_with_index(ctx, ir, losses, &index)?;
    validate_annotations(ctx, &index, annotations, std::iter::empty(), &mut report.findings)?;
    Ok(report)
}

/// Validate while treating staged retained-record identities as native entities.
pub fn validate_neutral_with_additional_native_identities<'a>(
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    standalone_validation(|ctx| {
        let index = crate::index::ModelIndex::with_additional_native_identities(ir, additional, ctx)?;
        validate_model_with_index(ctx, ir, losses, &index)
    })
}

/// Validate one neutral product model under the standalone application policy.
pub fn validate_neutral(ir: &CadIr, losses: Vec<LossNote>) -> Result<ValidationReport, CodecError> {
    standalone_validation(|ctx| validate_model(ctx, ir, losses))
}

/// Validate an application-owned model together with borrowed annotations.
pub fn validate_neutral_with_annotations(
    ir: &CadIr,
    annotations: &crate::annotations::Annotations,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    standalone_validation(|ctx| validate_model_with_annotations(ctx, ir, annotations, losses))
}

/// Validate an application-owned model together with its decode-time source sidecar.
pub fn validate_neutral_with_source_fidelity(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    standalone_validation(|ctx| {
        let index = crate::index::ModelIndex::new_for_decode(ir, ctx)?;
        let mut report = validate_model_with_index(ctx, ir, losses, &index)?;
        validate_annotations(ctx, &index, &source_fidelity.annotations,
            source_fidelity.retained_records().keys().map(|id| id.as_str()),
            &mut report.findings)?;
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use super::{pcurve_parameter_domain, validate_neutral};
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, FaceSelection, Feature,
        FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane, SplitFaceTool,
    };
    use crate::geometry::pcurve::PcurveGeometry;
    use crate::math::{Point3, Vector3};
    use crate::sketches::{Sketch, SketchId};
    use crate::CadIr;
    use std::collections::BTreeMap;

    fn nurbs_pcurve_leaf() -> PcurveGeometry {
        PcurveGeometry::Nurbs {
            nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(1.0, 1.0),
                ],
                None,
                false,
            )
            .unwrap(),
        }
    }

    fn placed_pcurve(placements: usize) -> Result<PcurveGeometry, &'static str> {
        let mut geometry = nurbs_pcurve_leaf();
        for _ in 0..placements {
            geometry = PcurveGeometry::Transformed(crate::geometry::pcurve::PlacedPcurve::try_new(
                Box::new(geometry),
                crate::transform::Transform2::identity(),
            )?);
        }
        Ok(geometry)
    }

    #[test]
    fn pcurve_parameter_domain_stops_at_the_admitted_nesting_depth() {
        let accepted =
            placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING).expect("admitted nesting");
        assert!(pcurve_parameter_domain(&accepted).is_some());

        // The walk has no depth gate because the carrier one placement deeper
        // cannot be built: `PlacedPcurve::try_new` refuses it.
        assert_eq!(
            placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING + 1),
            Err("PlacedPcurve.basis nests past the admitted inline basis depth")
        );
    }

    #[test]
    fn configuration_feature_sketch_resolves_against_model_sketches() {
        let mut ir = CadIr::empty();
        let feature_id = FeatureId::mint("test:model:feature#sketch").expect("identity grammar");
        let sketch_id = SketchId::mint("test:model:sketch#sketch").unwrap();
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: crate::features::SketchFeatureBinding::Unresolved,
                }),
            ),
            native_ref: None,
        });
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: crate::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: crate::sketches::SketchProfiles::default(),
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint("test:model:configuration#default")
                .expect("identity grammar"),
            ordinal: 0,
            active: true,
            source_index: None,
            name: Some("Default".to_string()),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: Some(crate::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id,
                ConfigurationFeatureState {
                    evaluation: crate::features::ConfigurationEvaluation::Active {
                        outputs: crate::features::DistinctMembers::default(),
                    },
                    dependencies: crate::features::DistinctMembers::default(),
                    definition: FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: crate::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }),
                },
            )]),
            native_ref: None,
        });

        let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn split_face_plane_sets_require_two_unique_plane_dependencies() {
        let feature = |id: FeatureId,
                       ordinal,
                       dependencies: Vec<FeatureId>,
                       definition: FeatureDefinition| Feature {
            id,
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: crate::features::DistinctMembers::try_from(dependencies, &cadmpeg_test_support::service_decode_context()).unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        };
        let first = FeatureId::mint("test:model:feature#plane-a").expect("identity grammar");
        let second = FeatureId::mint("test:model:feature#plane-b").expect("identity grammar");
        let split = FeatureId::mint("test:model:feature#split").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.features = vec![
            feature(
                first.clone(),
                0,
                Vec::new(),
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                    plane: PrincipalPlane::Front,
                }),
            ),
            feature(
                second.clone(),
                1,
                Vec::new(),
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                    plane: PrincipalPlane::Right,
                }),
            ),
            feature(
                split,
                2,
                vec![first.clone(), second.clone()],
                FeatureDefinition::Operation(FeatureOperation::SplitFace {
                    targets: FaceSelection::Unresolved,
                    tool: SplitFaceTool::Planes {
                        planes: vec![first.clone(), second.clone()].try_into().unwrap(),
                    },
                }),
            ),
        ];

        let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }
}
