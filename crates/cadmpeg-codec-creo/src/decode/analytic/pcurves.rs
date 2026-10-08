// SPDX-License-Identifier: Apache-2.0
//! Analytic pcurve carrier transfer and native pcurve helpers.

use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::num::NonZeroU32;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveNurbs},
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::units::OrthonormalFrame3;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::native::annotate;
use super::super::surfaces::intersection_resolve::curve_contains_points;

use super::carriers::placed_carriers;
use super::edges::{
    nurbs_control_extent, nurbs_intrinsic_parameter_range, periodic_conic_edge_parameter_range,
    point_pair_alignments,
};
use super::equations::{CarrierEquation, PlaneEquation};
use super::model_index::{CurveIndex, SurfaceIndex};
use super::planes::point_on_carrier;
use super::vertices::{finite_model_point, model_points_agree};
use crate::vecmath::{cross, dot};

macro_rules! require_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_NEAR_ZERO: f64 = 1.0e-12;
const PCURVE_MISMATCH_SAMPLE_LIMIT: usize = 4;
const PCURVE_CARRIER_PARALLEL_EPS_SQUARED: f64 = 1e-18;
const PCURVE_CARRIER_SAMPLE_PARAMETERS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn unique_model_surface<'a>(
    surfaces: &'a [Surface],
    face_id: u32,
    index: &SurfaceIndex<'_>,
) -> Option<&'a Surface> {
    index.preferred(face_id).map(|position| &surfaces[position])
}

fn topology_ignored_surface_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    layout: &crate::container::Layout,
    rows: &[crate::surface::SurfaceRow],
) -> Result<HashSet<u32>, cadmpeg_core::CodecError> {
    // The interpolation carrier makes a legacy spline surface evaluable, but
    // its trim/intersection join is still unresolved. Keep that surface from
    // vetoing endpoint evidence supplied by a proven adjacent analytic face.
    if !matches!(layout, crate::container::Layout::LegacyAscii(_)) {
        return Ok(HashSet::new());
    }
    let mut ignored = HashSet::new();
    for row in ctx.admit_iter(rows, "creo topology ignored surface rows")? {
        if row.kind == crate::surface::SurfaceKind::Spline {
            ctx.insert_hash_set(&mut ignored, row.id, "creo ignored spline surface IDs")?;
        }
    }
    Ok(ignored)
}

pub(in crate::decode) fn canonicalized_pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    faces: [Option<NonZeroU32>; 2],
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
) -> Result<[[[f64; 2]; 2]; 2], cadmpeg_core::CodecError> {
    let mut endpoints = [face_0_endpoints, face_1_endpoints];
    for (side, face) in faces.into_iter().enumerate() {
        let Some(face) = face else {
            continue;
        };
        if let Some(carrier) = ctx.find_by(
            &scan.surfaces.legacy_carriers,
            |carrier| Ok(carrier.surface_id == face.get()),
            "creo legacy cone pcurve carrier search",
        )? {
            endpoints[side] = crate::legacy_geometry::canonicalize_legacy_cone_pcurve_endpoints(
                std::slice::from_ref(carrier),
                face.get(),
                endpoints[side],
            );
        } else {
            endpoints[side] = crate::legacy_geometry::canonicalize_legacy_cone_pcurve_endpoints(
                &[],
                face.get(),
                endpoints[side],
            );
        }
    }
    Ok(endpoints)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::decode) enum TwoChartEndpointSets {
    Both([[[f64; 2]; 2]; 2]),
    First([[f64; 2]; 2]),
    Second([[f64; 2]; 2]),
}

impl TwoChartEndpointSets {
    /// The endpoint path for each face.
    pub(in crate::decode) fn paths(self) -> [Option<[[f64; 2]; 2]>; 2] {
        match self {
            Self::Both(paths) => paths.map(Some),
            Self::First(path) => [Some(path), None],
            Self::Second(path) => [None, Some(path)],
        }
    }

    fn complete(self) -> bool {
        matches!(self, Self::Both(_))
    }
}

#[derive(Debug)]
enum TwoChartMapping {
    NoSamples,
    Mapped {
        endpoint_sets: Option<TwoChartEndpointSets>,
        missing_surface_paths: usize,
        unevaluable_paths: usize,
        surface_mismatch: bool,
    },
}

fn map_two_chart_endpoint_sets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    pcurve: &crate::curve::TwoChartPcurveSamples,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_index: &SurfaceIndex<'_>,
) -> Result<TwoChartMapping, cadmpeg_core::CodecError> {
    let (Some(first), Some(last)) = (pcurve.samples.first(), pcurve.samples.last()) else {
        return Ok(TwoChartMapping::NoSamples);
    };
    let surfaces = pcurve
        .faces
        .map(|face_id| unique_model_surface(&ir.model.surfaces, face_id, surface_index));
    let mut mapped = surfaces.map(|surface| surface.is_some());
    let missing_surface_paths = usize::from(!mapped[0]) + usize::from(!mapped[1]);
    let mut unevaluable_paths = 0;
    let mut mismatch = false;
    // A sample that leaves the finite range is a mapped sample; it agrees
    // with no sample on the other chart.
    for sample in ctx.admit_iter(
        &pcurve.samples,
        "creo map two chart endpoint sets samples traversal",
    )? {
        let mut points: [Option<Result<FinitePoint3, Point3>>; 2] = [None, None];
        for face_index in 0..2 {
            if !mapped[face_index] {
                continue;
            }
            let Some(surface) = surfaces[face_index] else {
                continue;
            };
            points[face_index] = match cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(
                    ctx,
                    source_carriers.surface_geometry(surface),
                    sample[face_index][0],
                    sample[face_index][1],
                ),
            )? {
                Ok(point) => Some(Ok(point)),
                Err(failure) => failure.non_finite()?.map(Err),
            };
            if points[face_index].is_none() {
                mapped[face_index] = false;
                unevaluable_paths += 1;
            }
        }
        if let [Some(first), Some(second)] = &points {
            mismatch |= match (first, second) {
                (Ok(first), Ok(second)) => !model_points_agree(*first, *second),
                _ => true,
            };
        }
    }
    let canonical = canonicalized_pcurve_endpoints(
        ctx,
        scan,
        pcurve.faces.map(NonZeroU32::new),
        [first[0], last[0]],
        [first[1], last[1]],
    )?;
    let endpoint_sets = std::array::from_fn(|index| mapped[index].then_some(canonical[index]));
    let endpoint_sets = match endpoint_sets {
        [Some(first), Some(second)] => Some(TwoChartEndpointSets::Both([first, second])),
        [Some(first), None] => Some(TwoChartEndpointSets::First(first)),
        [None, Some(second)] => Some(TwoChartEndpointSets::Second(second)),
        [None, None] => None,
    };
    let surface_mismatch = mapped[0] && mapped[1] && mismatch;
    Ok(TwoChartMapping::Mapped {
        endpoint_sets,
        missing_surface_paths,
        unevaluable_paths,
        surface_mismatch,
    })
}

pub(in crate::decode) fn mapped_two_chart_endpoint_sets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    pcurve: &crate::curve::TwoChartPcurveSamples,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<Option<TwoChartEndpointSets>, cadmpeg_core::CodecError> {
    if pcurve.samples.is_empty() {
        return Ok(None);
    }
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    Ok(
        match map_two_chart_endpoint_sets(ctx, scan, ir, pcurve, source_carriers, &surface_index)? {
            TwoChartMapping::Mapped {
                endpoint_sets,
                surface_mismatch: false,
                ..
            } => endpoint_sets,
            TwoChartMapping::NoSamples
            | TwoChartMapping::Mapped {
                surface_mismatch: true,
                ..
            } => None,
        },
    )
}

#[cfg(test)]
pub(in crate::decode) fn mapped_pcurve_endpoints(
    ir: &CadIr,
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> Result<Option<[[f64; 3]; 2]>, cadmpeg_core::CodecError> {
    let mapped = crate::decode::with_test_decode_ctx(|ctx| {
        map_pcurve_paths(
            ctx,
            ir,
            faces.into_iter().map(NonZeroU32::new).zip(endpoint_sets),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &SurfaceIndex::new(ctx, &ir.model.surfaces)?,
        )
    })?;
    crate::decode::with_test_decode_ctx(|ctx| {
        Ok(
            pcurve_endpoint_evidence_from_mapped(ctx, &mapped.mapped, false)?
                .map(|evidence| evidence.points),
        )
    })
}

#[derive(Debug, Clone, Copy)]
pub(in crate::decode) struct PcurveEndpointEvidence {
    pub(super) points: [[f64; 3]; 2],
    pub(in crate::decode) complete: bool,
    /// The endpoints came from a complete native grammar that is allowed to
    /// override an inconsistent inferred analytic intersection at a vertex.
    pub(super) authoritative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::decode) struct PcurveMismatchDetail {
    pub(in crate::decode) curve_id: u32,
    pub(in crate::decode) faces: [u32; 2],
    pub(in crate::decode) same_order_error: f64,
    pub(in crate::decode) reverse_order_error: f64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(in crate::decode) struct PcurveEndpointDiagnostics {
    pub(in crate::decode) records: usize,
    pub(in crate::decode) inactive_paths: usize,
    pub(in crate::decode) inactive_records: usize,
    pub(in crate::decode) partial_records: usize,
    pub(in crate::decode) topology_mismatch_records: usize,
    pub(in crate::decode) missing_surfaces: usize,
    pub(in crate::decode) unevaluable_paths: usize,
    pub(in crate::decode) mapped_paths: usize,
    pub(in crate::decode) unmapped_records: usize,
    pub(in crate::decode) inconsistent_records: usize,
    pub(in crate::decode) accepted_records: usize,
    pub(in crate::decode) complete_records: usize,
    pub(in crate::decode) conflicting_curves: usize,
    pub(in crate::decode) evidence: usize,
    pub(in crate::decode) complete_evidence: usize,
    pub(in crate::decode) two_chart_records: usize,
    pub(in crate::decode) two_chart_complete_records: usize,
    pub(in crate::decode) two_chart_partial_records: usize,
    pub(in crate::decode) two_chart_missing_surface_paths: usize,
    pub(in crate::decode) two_chart_unevaluable_paths: usize,
    pub(in crate::decode) two_chart_surface_mismatch_records: usize,
    pub(in crate::decode) two_chart_no_sample_records: usize,
    pub(in crate::decode) two_chart_unmapped_records: usize,
    pub(in crate::decode) carrier_validated_paths: usize,
    pub(in crate::decode) carrier_rejected_paths: usize,
    pub(in crate::decode) carrier_unknown_missing_surface_paths: usize,
    pub(in crate::decode) carrier_unknown_missing_carrier_paths: usize,
    pub(in crate::decode) carrier_unknown_unsupported_pair_paths: usize,
    pub(in crate::decode) carrier_unknown_parallel_plane_paths: usize,
    pub(in crate::decode) carrier_unknown_unsupported_path_paths: usize,
    pub(in crate::decode) carrier_rejected_records: usize,
    pub(in crate::decode) mismatch_samples: Vec<PcurveMismatchDetail>,
}

impl PcurveEndpointDiagnostics {
    /// Returns the total number of path outcomes.
    pub(in crate::decode) fn paths(&self) -> usize {
        self.mapped_paths + self.missing_surfaces + self.unevaluable_paths
    }

    /// Returns the number of records with at least one mapped chart.
    pub(in crate::decode) fn two_chart_mapped_records(&self) -> usize {
        self.two_chart_complete_records + self.two_chart_partial_records
    }

    /// Returns the number of paths without a carrier decision.
    pub(in crate::decode) fn carrier_unknown_paths(&self) -> usize {
        self.carrier_unknown_missing_surface_paths
            + self.carrier_unknown_missing_carrier_paths
            + self.carrier_unknown_unsupported_pair_paths
            + self.carrier_unknown_parallel_plane_paths
            + self.carrier_unknown_unsupported_path_paths
    }
}

#[derive(Debug, Default)]
struct PcurvePathActivity {
    active_paths: BTreeSet<(Option<std::num::NonZeroU32>, u32)>,
    topology_faces: BTreeMap<u32, Option<[Option<std::num::NonZeroU32>; 2]>>,
    prototype_faces: BTreeMap<u32, Option<[Option<std::num::NonZeroU32>; 2]>>,
}

impl PcurvePathActivity {
    fn from_scan(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scan: &ContainerScan,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut active_paths = BTreeSet::new();
        for loop_ in ctx.admit_iter(&scan.topology.loops, "creo from scan loops traversal")? {
            for half_edge in
                ctx.admit_iter(loop_.half_edges(), "creo pcurve activity half-edges")?
            {
                let key = (loop_.face_id(), half_edge.curve_id);
                ctx.insert_btree_set(&mut active_paths, key, "creo active pcurve path nodes")?;
            }
        }
        let mut topology_faces = BTreeMap::new();
        for row in ctx.admit_iter(&scan.curves.topology_rows, "creo pcurve topology face rows")? {
            match ctx.entry_btree_map(
                &mut topology_faces,
                row.id,
                "creo pcurve topology face nodes",
            )? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(row.faces));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        let mut prototype_faces = BTreeMap::new();
        for row in ctx.admit_iter(
            &scan.curves.prototype_topology,
            "creo from scan prototype topology traversal",
        )? {
            match ctx.entry_btree_map(
                &mut prototype_faces,
                row.curve_id,
                "creo pcurve prototype face nodes",
            )? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(row.faces));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        Ok(Self {
            active_paths,
            topology_faces,
            prototype_faces,
        })
    }

    fn selected_paths(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        curve_id: u32,
        faces: [Option<NonZeroU32>; 2],
        prototype: bool,
    ) -> Result<Option<[bool; 2]>, cadmpeg_core::CodecError> {
        let topology_faces = if prototype {
            &self.prototype_faces
        } else {
            &self.topology_faces
        };
        let recorded_faces = ctx
            .get_btree_map(
                topology_faces,
                &curve_id,
                "creo analytic topology faces lookup",
            )?
            .and_then(Option::as_ref);
        let faces_match = recorded_faces == Some(&faces);
        let active_paths = [
            ctx.contains_btree_set(
                &self.active_paths,
                &(faces[0], curve_id),
                "creo active pcurve path membership",
            )?,
            ctx.contains_btree_set(
                &self.active_paths,
                &(faces[1], curve_id),
                "creo active pcurve path membership",
            )?,
        ];
        Ok(faces_match.then_some(active_paths))
    }
}

#[derive(Debug, Clone, Copy)]
struct MappedPcurvePath {
    face_id: u32,
    endpoints: [[f64; 3]; 2],
}

type IndexedPcurvePath = (usize, (Option<NonZeroU32>, [[f64; 2]; 2]));
type SupportConePlaneWitness = ([[f64; 2]; 2], PlaneEquation);

struct MappedPcurvePaths {
    mapped: Vec<MappedPcurvePath>,
    missing_surfaces: usize,
    unevaluable_paths: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcurveCarrierUnknownReason {
    MissingSurface,
    MissingCarrier,
    UnsupportedPair,
    ParallelPlanePair,
    UnsupportedPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcurveCarrierStatus {
    Validated,
    Rejected,
    Unknown(PcurveCarrierUnknownReason),
}

fn pcurve_plane_carrier_status(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    face_carrier: CarrierEquation,
    other_carrier: CarrierEquation,
    endpoints: [[f64; 2]; 2],
) -> Result<PcurveCarrierStatus, cadmpeg_core::CodecError> {
    let (CarrierEquation::Plane(face_plane), CarrierEquation::Plane(other_plane)) =
        (face_carrier, other_carrier)
    else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::UnsupportedPair,
        ));
    };
    if dot(
        cross(face_plane.normal, other_plane.normal),
        cross(face_plane.normal, other_plane.normal),
    ) <= PCURVE_CARRIER_PARALLEL_EPS_SQUARED
    {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::ParallelPlanePair,
        ));
    }
    if !matches!(
        surface,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ) || linear_pcurve_carrier(ctx, surface, endpoints)?.is_none()
    {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::UnsupportedPath,
        ));
    }
    for fraction in PCURVE_CARRIER_SAMPLE_PARAMETERS {
        let uv = [
            endpoints[0][0].mul_add(1.0 - fraction, endpoints[1][0] * fraction),
            endpoints[0][1].mul_add(1.0 - fraction, endpoints[1][1] * fraction),
        ];
        let Some(point) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv[0], uv[1]),
            )?)?
        else {
            return Ok(PcurveCarrierStatus::Rejected);
        };
        let point = [point.x, point.y, point.z];
        if !point_on_carrier(point, face_carrier) || !point_on_carrier(point, other_carrier) {
            return Ok(PcurveCarrierStatus::Rejected);
        }
    }
    Ok(PcurveCarrierStatus::Validated)
}

fn pcurve_path_carrier_status(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    incidence: ([Option<NonZeroU32>; 2], usize),
    endpoints: [[f64; 2]; 2],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_index: &SurfaceIndex<'_>,
) -> Result<PcurveCarrierStatus, cadmpeg_core::CodecError> {
    let (faces, face_index) = incidence;
    let face_id = faces[face_index];
    let other_id = faces[1 - face_index];
    let Some(surface) =
        face_id.and_then(|id| unique_model_surface(&ir.model.surfaces, id.get(), surface_index))
    else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingSurface,
        ));
    };
    let face_carrier = match face_id {
        Some(id) => ctx
            .get_btree_map(carriers, &id.get(), "creo pcurve face carrier lookup")?
            .copied(),
        None => None,
    };
    let Some(face_carrier) = face_carrier else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingCarrier,
        ));
    };
    let other_carrier = match other_id {
        Some(id) => ctx
            .get_btree_map(carriers, &id.get(), "creo pcurve other carrier lookup")?
            .copied(),
        None => None,
    };
    let Some(other_carrier) = other_carrier else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingCarrier,
        ));
    };
    pcurve_plane_carrier_status(
        ctx,
        source_carriers.surface_geometry(surface),
        face_carrier,
        other_carrier,
        endpoints,
    )
}

fn pcurve_endpoint_carrier_status(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    incidence: ([Option<NonZeroU32>; 2], usize),
    endpoints: [[f64; 2]; 2],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_index: &SurfaceIndex<'_>,
) -> Result<PcurveCarrierStatus, cadmpeg_core::CodecError> {
    let (faces, face_index) = incidence;
    let face_id = faces[face_index];
    let other_id = faces[1 - face_index];
    let Some(surface) =
        face_id.and_then(|id| unique_model_surface(&ir.model.surfaces, id.get(), surface_index))
    else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingSurface,
        ));
    };
    let face_carrier = match face_id {
        Some(id) => ctx
            .get_btree_map(carriers, &id.get(), "creo pcurve face carrier lookup")?
            .copied(),
        None => None,
    };
    let Some(face_carrier) = face_carrier else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingCarrier,
        ));
    };
    let other_carrier = match other_id {
        Some(id) => ctx
            .get_btree_map(carriers, &id.get(), "creo pcurve other carrier lookup")?
            .copied(),
        None => None,
    };
    let Some(other_carrier) = other_carrier else {
        return Ok(PcurveCarrierStatus::Unknown(
            PcurveCarrierUnknownReason::MissingCarrier,
        ));
    };
    let mut valid = true;
    for uv in endpoints {
        if !valid {
            break;
        }
        let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
                ctx,
                source_carriers.surface_geometry(surface),
                uv[0],
                uv[1],
            ))?,
        )?
        else {
            valid = false;
            break;
        };
        let point = [point.x, point.y, point.z];
        valid = point_on_carrier(point, face_carrier) && point_on_carrier(point, other_carrier);
    }
    Ok(if valid {
        PcurveCarrierStatus::Validated
    } else {
        PcurveCarrierStatus::Rejected
    })
}

/// Mirrors an apex cone through its apex: the same shape a cone surface record carries, so it
/// takes its half angle as a [`crate::surface::ApexConeHalfAngle`].
///
/// The frame is not examined. A `ConeSurface` holds an
/// [`cadmpeg_ir::units::OrthonormalFrame3`], whose only admission is
/// [`cadmpeg_ir::units::OrthonormalFrame3::new`]: both directions are unit length and
/// perpendicular within the analytic frame tolerance, so every component is finite.
/// [`cadmpeg_ir::units::OrthonormalFrame3::reverse_axis`] negates components and keeps that
/// guarantee, and `ConeSurface::new` moves the frame across without a new admission.
fn mirrored_support_apex_cone(geometry: &SurfaceGeometry) -> Option<SurfaceGeometry> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = geometry else {
        return None;
    };
    let radius = cone_surface.radius().get();
    let ratio = cone_surface.ratio().get();
    let half_angle = cone_surface.half_angle().get();
    if radius != 0.0
        || (ratio - 1.0).abs() > EPS_NEAR_ZERO
        || crate::surface::ApexConeHalfAngle::new(half_angle).is_none()
    {
        return None;
    }
    let origin = cone_surface.origin().negated();
    let mut frame = *cone_surface.frame();
    frame.reverse_axis();
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::new(
            origin,
            frame,
            cone_surface.radius(),
            cone_surface.ratio(),
            cone_surface.half_angle(),
        ),
    )))
}

fn support_cone_witness_matches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    plane: PlaneEquation,
) -> Result<bool, cadmpeg_core::CodecError> {
    for uv in endpoints {
        let Some(point) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, geometry, uv[0], uv[1]),
            )?)?
        else {
            return Ok(false);
        };
        if !point_on_carrier([point.x, point.y, point.z], CarrierEquation::Plane(plane)) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn collect_support_cone_plane_witness(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    witnesses: &mut BTreeMap<u32, Vec<SupportConePlaneWitness>>,
    planes: &BTreeMap<u32, PlaneEquation>,
    faces: [Option<NonZeroU32>; 2],
    endpoint_sets: [Option<[[f64; 2]; 2]>; 2],
) -> Result<(), cadmpeg_core::CodecError> {
    let [Some(first), Some(second)] = faces else {
        return Ok(());
    };
    let faces = [first.get(), second.get()];
    for face_index in 0..2 {
        let Some(endpoints) = endpoint_sets[face_index] else {
            continue;
        };
        let Some(plane) = ctx
            .get_btree_map(
                planes,
                &faces[1 - face_index],
                "creo analytic planes lookup",
            )?
            .copied()
        else {
            continue;
        };
        match ctx.entry_btree_map(
            witnesses,
            faces[face_index],
            "creo support cone witness nodes",
        )? {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                ctx.reserve_vec(entry.get_mut(), 1, "creo support cone plane witnesses")?;
                entry.get_mut().push((endpoints, plane));
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut values = Vec::new();
                ctx.reserve_vec(&mut values, 1, "creo support cone plane witnesses")?;
                values.push((endpoints, plane));
                entry.insert(values);
            }
        }
    }
    Ok(())
}

/// Reconcile the signed frame of a radius-zero support cone from a pcurve
/// endpoint and an independently placed adjacent plane.
pub(in crate::decode) fn reconcile_support_apex_cone_parameter_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo reconcile support apex cone parameter branches scratch",
    )?;
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    let planes = scratch.with_storage(|| super::planes::placed_planes(ctx, scan))?;
    if planes.is_empty() {
        return Ok(0);
    }
    let mut witnesses = BTreeMap::<u32, Vec<SupportConePlaneWitness>>::new();
    for pcurve in ctx.admit_iter(
        &scan.curves.pcurves,
        "creo transfer analytic pcurve carriers pcurves traversal",
    )? {
        let endpoint_sets = canonicalized_pcurve_endpoints(
            ctx,
            scan,
            pcurve.faces,
            pcurve.face_0_endpoints,
            pcurve.face_1_endpoints,
        )?;
        scratch.with_storage(|| {
            collect_support_cone_plane_witness(
                ctx,
                &mut witnesses,
                &planes,
                pcurve.faces,
                endpoint_sets.map(Some),
            )
        })?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.bound_prototype_pcurves,
        "creo transfer analytic pcurve carriers bound prototype pcurves traversal",
    )? {
        let endpoint_sets = canonicalized_pcurve_endpoints(
            ctx,
            scan,
            pcurve.faces,
            pcurve.face_0_endpoints,
            pcurve.face_1_endpoints,
        )?;
        scratch.with_storage(|| {
            collect_support_cone_plane_witness(
                ctx,
                &mut witnesses,
                &planes,
                pcurve.faces,
                endpoint_sets.map(Some),
            )
        })?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.two_chart_pcurves,
        "creo pcurve edge endpoint evidence with carriers two chart pcurves traversal",
    )? {
        let faces = pcurve.faces.map(NonZeroU32::new);
        let mapping =
            map_two_chart_endpoint_sets(ctx, scan, ir, pcurve, source_carriers, &surface_index)?;
        let TwoChartMapping::Mapped {
            endpoint_sets: Some(endpoint_sets),
            ..
        } = mapping
        else {
            continue;
        };
        scratch.with_storage(|| {
            collect_support_cone_plane_witness(
                ctx,
                &mut witnesses,
                &planes,
                faces,
                endpoint_sets.paths(),
            )
        })?;
    }

    let mut reconciled = 0;
    let mut traversal = witnesses.iter();
    while let Some((face_id, face_witnesses)) =
        ctx.next_charged(&mut traversal, "creo support cone face witnesses")?
    {
        let Some(surface) = surface_index
            .preferred(*face_id)
            .and_then(|position| ir.model.surfaces.get_mut(position))
        else {
            continue;
        };
        let source_geometry = source_carriers.surface_geometry(surface);
        let Some(mirrored) = mirrored_support_apex_cone(source_geometry) else {
            continue;
        };
        let mut current_matches = true;
        let mut traversal = (face_witnesses).iter();
        while let Some((endpoints, plane)) = ctx.next_charged(
            &mut traversal,
            "creo reconcile support apex cone parameter branches face witnesses traversal",
        )? {
            if !support_cone_witness_matches(ctx, source_geometry, *endpoints, *plane)? {
                current_matches = false;
                break;
            }
        }
        if current_matches {
            continue;
        }
        let mut mirrored_matches = true;
        let mut traversal = (face_witnesses).iter();
        while let Some((endpoints, plane)) = ctx.next_charged(
            &mut traversal,
            "creo reconcile support apex cone parameter branches face witnesses traversal",
        )? {
            if !support_cone_witness_matches(ctx, &mirrored, *endpoints, *plane)? {
                mirrored_matches = false;
                break;
            }
        }
        if !mirrored_matches {
            continue;
        }
        source_carriers.replace_surface_geometry(ctx, surface, mirrored)?;
        reconciled += 1;
        if let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id) {
            annotate(
                ctx,
                annotations,
                &surface.id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(row.offset),
                "support_apex_cone_pcurve_branch",
                Exactness::Derived,
            )?;
        }
    }
    Ok(reconciled)
}

fn map_pcurve_paths(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    paths: impl IntoIterator<Item = (Option<NonZeroU32>, [[f64; 2]; 2])>,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_index: &SurfaceIndex<'_>,
) -> Result<MappedPcurvePaths, cadmpeg_core::CodecError> {
    let mut result = MappedPcurvePaths {
        mapped: Vec::new(),
        missing_surfaces: 0,
        unevaluable_paths: 0,
    };
    let mut paths = paths.into_iter();
    while let Some((face_id, endpoints)) =
        ctx.next_charged(&mut paths, "creo mapped pcurve path input")?
    {
        let Some(face_id) = face_id else {
            result.missing_surfaces += 1;
            continue;
        };
        let face_id = face_id.get();
        let Some(surface) = unique_model_surface(&ir.model.surfaces, face_id, surface_index) else {
            result.missing_surfaces += 1;
            continue;
        };
        // A non-finite endpoint is a mapped endpoint; the path comparisons
        // read it as a mismatch.
        let [first, second] = endpoints.map(|uv| -> Result<_, cadmpeg_core::CodecError> {
            let point = match cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(
                    ctx,
                    source_carriers.surface_geometry(surface),
                    uv[0],
                    uv[1],
                ),
            )? {
                Ok(point) => point.get(),
                Err(failure) => match failure.non_finite()? {
                    Some(point) => point,
                    None => return Ok(None),
                },
            };
            Ok(Some([point.x, point.y, point.z]))
        });
        let [Some(first), Some(second)] = [first?, second?] else {
            result.unevaluable_paths += 1;
            continue;
        };
        ctx.reserve_vec(&mut result.mapped, 1, "creo mapped pcurve paths")?;
        result.mapped.push(MappedPcurvePath {
            face_id,
            endpoints: [first, second],
        });
    }
    Ok(result)
}

fn pcurve_endpoint_evidence_from_mapped(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mapped: &[MappedPcurvePath],
    authoritative: bool,
) -> Result<Option<PcurveEndpointEvidence>, cadmpeg_core::CodecError> {
    let Some(first) = mapped.first().map(|path| path.endpoints) else {
        return Ok(None);
    };
    // An endpoint outside the finite range agrees with no endpoint, so a
    // path that reaches one forms no evidence.
    let admitted = first.map(finite_model_point);
    Ok(ctx
        .all_by(
            mapped,
            |candidate| {
                Ok(admitted
                    .into_iter()
                    .zip(candidate.endpoints)
                    .all(|(first, candidate)| {
                        first
                            .zip(finite_model_point(candidate))
                            .is_some_and(|(first, candidate)| model_points_agree(first, candidate))
                    }))
            },
            "creo mapped pcurve endpoint agreement",
        )?
        .then_some(PcurveEndpointEvidence {
            points: first,
            complete: mapped.len() == 2,
            authoritative,
        }))
}

fn point_coordinate_error(first: [f64; 3], second: [f64; 3]) -> f64 {
    first
        .into_iter()
        .zip(second)
        .map(|(first, second)| {
            let error = (first - second).abs();
            if error.is_finite() {
                error
            } else {
                f64::INFINITY
            }
        })
        .fold(0.0, f64::max)
}

fn endpoint_pair_error(first: [[f64; 3]; 2], second: [[f64; 3]; 2], reverse: bool) -> f64 {
    if reverse {
        point_coordinate_error(first[0], second[1]).max(point_coordinate_error(first[1], second[0]))
    } else {
        point_coordinate_error(first[0], second[0]).max(point_coordinate_error(first[1], second[1]))
    }
}

fn pcurve_mismatch_detail(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve_id: u32,
    mapped: &[MappedPcurvePath],
) -> Result<Option<PcurveMismatchDetail>, cadmpeg_core::CodecError> {
    let Some(first) = mapped.first().map(|path| path.endpoints) else {
        return Ok(None);
    };
    let Some(second) = mapped.get(1) else {
        return Ok(None);
    };
    let mut same_order_error = 0.0_f64;
    let mut reverse_order_error = 0.0_f64;
    for candidate in ctx.admit_iter(&mapped[1..], "creo pcurve mismatch comparisons")? {
        same_order_error =
            same_order_error.max(endpoint_pair_error(first, candidate.endpoints, false));
        reverse_order_error =
            reverse_order_error.max(endpoint_pair_error(first, candidate.endpoints, true));
    }
    Ok(Some(PcurveMismatchDetail {
        curve_id,
        faces: [mapped[0].face_id, second.face_id],
        same_order_error,
        reverse_order_error,
    }))
}

pub(in crate::decode) fn pcurve_edge_endpoint_evidence(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<BTreeMap<u32, PcurveEndpointEvidence>, cadmpeg_core::CodecError> {
    Ok(pcurve_edge_endpoint_evidence_with_diagnostics(ctx, scan, ir, source_carriers)?.0)
}

fn pcurve_edge_endpoint_evidence_with_diagnostics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<
    (
        BTreeMap<u32, PcurveEndpointEvidence>,
        PcurveEndpointDiagnostics,
    ),
    cadmpeg_core::CodecError,
> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo pcurve edge endpoint evidence with diagnostics scratch",
    )?;
    let carriers = scratch.with_storage(|| placed_carriers(ctx, scan, ir, source_carriers))?;
    pcurve_edge_endpoint_evidence_with_carriers(ctx, scan, ir, &carriers, source_carriers, None)
}

/// Use caller-owned scratch for evidence that is consumed in this route.
/// Diagnostic samples keep their output storage.
pub(super) fn pcurve_edge_endpoint_evidence_with_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    mut evidence_storage: Option<&mut cadmpeg_core::decode::ScopedReservation<'_>>,
) -> Result<
    (
        BTreeMap<u32, PcurveEndpointEvidence>,
        PcurveEndpointDiagnostics,
    ),
    cadmpeg_core::CodecError,
> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo pcurve edge endpoint evidence with carriers scratch",
    )?;
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    let ignored_surface_ids = scratch.with_storage(|| {
        topology_ignored_surface_ids(ctx, &scan.framing.layout, &scan.surfaces.rows)
    })?;
    let path_activity = scratch.with_storage(|| PcurvePathActivity::from_scan(ctx, scan))?;
    let mut candidates = BTreeMap::<u32, Vec<PcurveEndpointEvidence>>::new();
    let mut diagnostics = PcurveEndpointDiagnostics::default();
    let mut group_storage = ctx.reserve_scoped(0, "creo endpoint evidence group storage")?;
    let mut process_paths = |curve_id: u32,
                             faces: [Option<NonZeroU32>; 2],
                             paths: Vec<IndexedPcurvePath>,
                             authoritative: bool,
                             endpoint_carrier_proof: bool|
     -> Result<(), cadmpeg_core::CodecError> {
        let mut path_storage = ctx.reserve_scoped(0, "creo mapped endpoint path scratch")?;
        let mut mapped_paths = Vec::new();
        let mut carrier_mapped_paths = Vec::new();
        let mut carrier_proof_available = false;
        for (face_index, (face_id, endpoints)) in
            ctx.admit_iter(&paths, "creo pcurve endpoint paths")?
        {
            let mapped = path_storage.with_storage(|| {
                map_pcurve_paths(
                    ctx,
                    ir,
                    [(*face_id, *endpoints)],
                    source_carriers,
                    &surface_index,
                )
            })?;
            diagnostics.missing_surfaces += mapped.missing_surfaces;
            diagnostics.unevaluable_paths += mapped.unevaluable_paths;
            diagnostics.mapped_paths += mapped.mapped.len();
            path_storage.with_storage(|| {
                ctx.extend_from_slice(
                    &mut mapped_paths,
                    &mapped.mapped,
                    "creo selected mapped pcurve paths",
                )
            })?;
            let carrier_status = if endpoint_carrier_proof {
                pcurve_endpoint_carrier_status(
                    ctx,
                    ir,
                    carriers,
                    (faces, *face_index),
                    *endpoints,
                    source_carriers,
                    &surface_index,
                )?
            } else {
                pcurve_path_carrier_status(
                    ctx,
                    ir,
                    carriers,
                    (faces, *face_index),
                    *endpoints,
                    source_carriers,
                    &surface_index,
                )?
            };
            match carrier_status {
                PcurveCarrierStatus::Validated => {
                    carrier_proof_available = true;
                    diagnostics.carrier_validated_paths += 1;
                    path_storage.with_storage(|| {
                        ctx.extend_vec(
                            &mut carrier_mapped_paths,
                            mapped.mapped,
                            "creo carrier mapped pcurve paths",
                        )
                    })?;
                }
                PcurveCarrierStatus::Rejected => {
                    carrier_proof_available = true;
                    diagnostics.carrier_rejected_paths += 1;
                }
                PcurveCarrierStatus::Unknown(reason) => match reason {
                    PcurveCarrierUnknownReason::MissingSurface => {
                        diagnostics.carrier_unknown_missing_surface_paths += 1;
                    }
                    PcurveCarrierUnknownReason::MissingCarrier => {
                        diagnostics.carrier_unknown_missing_carrier_paths += 1;
                    }
                    PcurveCarrierUnknownReason::UnsupportedPair => {
                        diagnostics.carrier_unknown_unsupported_pair_paths += 1;
                    }
                    PcurveCarrierUnknownReason::ParallelPlanePair => {
                        diagnostics.carrier_unknown_parallel_plane_paths += 1;
                    }
                    PcurveCarrierUnknownReason::UnsupportedPath => {
                        diagnostics.carrier_unknown_unsupported_path_paths += 1;
                    }
                },
            }
        }
        let selected_paths = if carrier_proof_available {
            carrier_mapped_paths
        } else {
            mapped_paths
        };
        if carrier_proof_available && selected_paths.is_empty() {
            diagnostics.carrier_rejected_records += 1;
        }
        match pcurve_endpoint_evidence_from_mapped(ctx, &selected_paths, authoritative)? {
            Some(evidence) => {
                diagnostics.accepted_records += 1;
                diagnostics.complete_records += usize::from(evidence.complete);
                match group_storage.with_storage(|| {
                    ctx.entry_btree_map(
                        &mut candidates,
                        curve_id,
                        "creo pcurve evidence candidate nodes",
                    )
                })? {
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        group_storage.with_storage(|| {
                            ctx.reserve_vec(entry.get_mut(), 1, "creo pcurve evidence candidates")
                        })?;
                        entry.get_mut().push(evidence);
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let mut entries = Vec::new();
                        group_storage.with_storage(|| {
                            ctx.reserve_vec(&mut entries, 1, "creo pcurve evidence candidates")
                        })?;
                        entries.push(evidence);
                        entry.insert(entries);
                    }
                }
            }
            None if selected_paths.is_empty() && !carrier_proof_available => {
                diagnostics.unmapped_records += 1;
            }
            None => {
                diagnostics.inconsistent_records += 1;
                if diagnostics.mismatch_samples.len() < PCURVE_MISMATCH_SAMPLE_LIMIT {
                    if let Some(detail) = pcurve_mismatch_detail(ctx, curve_id, &selected_paths)? {
                        ctx.reserve_vec(
                            &mut diagnostics.mismatch_samples,
                            1,
                            "creo pcurve mismatch samples",
                        )?;
                        diagnostics.mismatch_samples.push(detail);
                    }
                }
            }
        }
        Ok(())
    };
    {
        let mut process_face_paths = |curve_id: u32,
                                      faces: [Option<NonZeroU32>; 2],
                                      first: [[f64; 2]; 2],
                                      second: [[f64; 2]; 2],
                                      prototype: bool|
         -> Result<(), cadmpeg_core::CodecError> {
            diagnostics.records += 1;
            if let Some(active) = path_activity.selected_paths(ctx, curve_id, faces, prototype)? {
                let active_count = active.iter().filter(|is_active| **is_active).count();
                diagnostics.inactive_paths += 2 - active_count;
                match active_count {
                    0 => diagnostics.inactive_records += 1,
                    1 => diagnostics.partial_records += 1,
                    _ => {}
                }
            } else {
                diagnostics.topology_mismatch_records += 1;
            }
            let mut candidate_storage =
                ctx.reserve_scoped(0, "creo endpoint candidate path storage")?;
            let mut paths = Vec::new();
            for path in faces
                .iter()
                .zip([first, second])
                .enumerate()
                .filter(|(_, (face_id, _))| {
                    face_id.is_none_or(|id| !ignored_surface_ids.contains(&id.get()))
                })
                .map(|(index, (face_id, endpoints))| (index, (*face_id, endpoints)))
            {
                candidate_storage.with_storage(|| {
                    ctx.reserve_vec(&mut paths, 1, "creo pcurve candidate paths")
                })?;
                paths.push(path);
            }
            process_paths(curve_id, faces, paths, false, false)
        };
        for pcurve in ctx.admit_iter(&scan.curves.pcurves, "creo visible pcurve records")? {
            let [first, second] = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )?;
            process_face_paths(pcurve.curve_id, pcurve.faces, first, second, false)?;
        }
        for pcurve in ctx.admit_iter(
            &scan.curves.bound_prototype_pcurves,
            "creo prototype pcurve records",
        )? {
            let [first, second] = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )?;
            process_face_paths(pcurve.curve_id, pcurve.faces, first, second, true)?;
        }
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.two_chart_pcurves,
        "creo transfer analytic pcurve carriers two chart pcurves traversal",
    )? {
        let faces = pcurve.faces.map(NonZeroU32::new);
        diagnostics.records += 1;
        diagnostics.two_chart_records += 1;
        let mapping =
            map_two_chart_endpoint_sets(ctx, scan, ir, pcurve, source_carriers, &surface_index)?;
        let (endpoint_sets, surface_mismatch) = match mapping {
            TwoChartMapping::NoSamples => {
                diagnostics.two_chart_no_sample_records += 1;
                (None, false)
            }
            TwoChartMapping::Mapped {
                endpoint_sets,
                missing_surface_paths,
                unevaluable_paths,
                surface_mismatch,
            } => {
                diagnostics.two_chart_missing_surface_paths += missing_surface_paths;
                diagnostics.two_chart_unevaluable_paths += unevaluable_paths;
                diagnostics.two_chart_surface_mismatch_records += usize::from(surface_mismatch);
                (endpoint_sets, surface_mismatch)
            }
        };
        let Some(endpoint_sets) = endpoint_sets else {
            diagnostics.two_chart_unmapped_records += 1;
            process_paths(pcurve.curve_id, faces, Vec::new(), true, false)?;
            continue;
        };
        if endpoint_sets.complete() {
            diagnostics.two_chart_complete_records += 1;
        } else {
            diagnostics.two_chart_partial_records += 1;
        }
        if let Some(active) = path_activity.selected_paths(ctx, pcurve.curve_id, faces, false)? {
            let active_count = active.iter().filter(|is_active| **is_active).count();
            diagnostics.inactive_paths += 2 - active_count;
            match active_count {
                0 => diagnostics.inactive_records += 1,
                1 => diagnostics.partial_records += 1,
                _ => {}
            }
        } else {
            diagnostics.topology_mismatch_records += 1;
        }
        let mut candidate_storage =
            ctx.reserve_scoped(0, "creo endpoint candidate path storage")?;
        let mut paths = Vec::new();
        for path in faces
            .iter()
            .zip(endpoint_sets.paths())
            .enumerate()
            .filter_map(|(index, (face_id, endpoints))| {
                face_id
                    .is_none_or(|id| !ignored_surface_ids.contains(&id.get()))
                    .then_some((index, (*face_id, endpoints?)))
            })
        {
            candidate_storage
                .with_storage(|| ctx.reserve_vec(&mut paths, 1, "creo pcurve candidate paths"))?;
            paths.push(path);
        }
        process_paths(
            pcurve.curve_id,
            faces,
            paths,
            endpoint_sets.complete(),
            surface_mismatch,
        )?;
    }
    let short_pcurves = scratch.with_storage(|| {
        crate::curve::fc02_short_pcurve_endpoints(
            ctx,
            &scan.curves.parameters,
            &scan.curves.topology_rows,
        )
    })?;
    for pcurve in ctx.admit_iter(&short_pcurves, "creo short pcurve endpoint records")? {
        let faces = pcurve.faces.map(NonZeroU32::new);
        diagnostics.records += 1;
        if let Some(active) = path_activity.selected_paths(ctx, pcurve.curve_id, faces, false)? {
            diagnostics.inactive_paths += usize::from(!active[0]);
            diagnostics.inactive_records += usize::from(!active[0]);
        } else {
            diagnostics.topology_mismatch_records += 1;
        }
        let [face_0_endpoints, _] = canonicalized_pcurve_endpoints(
            ctx,
            scan,
            faces,
            pcurve.face_0_endpoints,
            pcurve.face_0_endpoints,
        )?;
        let mut candidate_storage =
            ctx.reserve_scoped(0, "creo endpoint candidate path storage")?;
        let mut paths = Vec::new();
        if faces[0].is_none_or(|id| !ignored_surface_ids.contains(&id.get())) {
            candidate_storage
                .with_storage(|| ctx.reserve_vec(&mut paths, 1, "creo pcurve candidate paths"))?;
            paths.push((0, (faces[0], face_0_endpoints)));
        }
        process_paths(pcurve.curve_id, faces, paths, true, false)?;
    }
    let mut evidence = BTreeMap::new();
    for (curve_id, candidates) in
        ctx.admit_iter(&candidates, "creo pcurve endpoint candidate groups")?
    {
        let Some(first) = candidates.first().copied() else {
            continue;
        };
        // An endpoint outside the finite range agrees with no endpoint.
        let admitted = first.points.map(finite_model_point);
        let mut complete = false;
        let mut authoritative = true;
        let mut agrees = true;
        let mut candidates = candidates.iter();
        while let Some(candidate) =
            ctx.next_charged(&mut candidates, "creo pcurve endpoint candidate agreement")?
        {
            if !admitted
                .into_iter()
                .zip(candidate.points)
                .all(|(first, candidate)| {
                    first
                        .zip(finite_model_point(candidate))
                        .is_some_and(|(first, candidate)| model_points_agree(first, candidate))
                })
            {
                agrees = false;
                break;
            }
            complete |= candidate.complete;
            authoritative &= candidate.authoritative;
        }
        if agrees {
            let candidate = PcurveEndpointEvidence {
                points: first.points,
                complete,
                authoritative,
            };
            if let Some(storage) = evidence_storage.as_deref_mut() {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut evidence,
                        *curve_id,
                        candidate,
                        "creo pcurve endpoint evidence nodes",
                    )
                })?;
            } else {
                ctx.insert_btree_map(
                    &mut evidence,
                    *curve_id,
                    candidate,
                    "creo pcurve endpoint evidence nodes",
                )?;
            }
            diagnostics.complete_evidence += usize::from(complete);
        } else {
            diagnostics.conflicting_curves += 1;
        }
    }
    diagnostics.evidence = evidence.len();
    Ok((evidence, diagnostics))
}

/// Keep the surface frame, with the reference reversed when `sign` is
/// negative: a negative ring or local radius places the circle start on the
/// opposite side of the axis.
fn signed_reference_frame(mut frame: OrthonormalFrame3, sign: f64) -> OrthonormalFrame3 {
    if sign.is_sign_negative() {
        frame.reverse_reference();
    }
    frame
}

fn linear_pcurve_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
) -> Result<Option<CurveGeometry>, cadmpeg_core::CodecError> {
    let scaled_vector = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    let offset_point = |point: Point3, vector: Vector3, scale: f64| {
        Point3::new(
            point.x + vector.x * scale,
            point.y + vector.y * scale,
            point.z + vector.z * scale,
        )
    };
    let [start, end] = endpoints;
    if start == end {
        return Ok(None);
    }
    Ok(match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv[0], uv[1]),
                )?)
                .map(|point| point.map(|point| [point.x, point.y, point.z]))
            });
            let [Some(first), Some(second)] = [first?, second?] else {
                return Ok(None);
            };
            let direction = require_some!(normalize(std::array::from_fn(
                |axis| second[axis] - first[axis]
            )));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                require_some!(cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::from(first),
                    Vector3::from(direction),
                )
                .ok()),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
            if { start[0] == end[0] } =>
        {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let reference = [ref_direction.x, ref_direction.y, ref_direction.z];
            let radial: [f64; 3] = std::array::from_fn(|coordinate| {
                start[0].cos() * reference[coordinate] + start[0].sin() * transverse[coordinate]
            });
            let point = [
                origin.x + radius * radial[0] + start[1] * axis.x,
                origin.y + radius * radial[1] + start[1] * axis.y,
                origin.z + radius * radial[2] + start[1] * axis.z,
            ];
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    require_some!(cadmpeg_ir::features::FinitePoint3::new(Point3::from(point))),
                    cylinder_surface.frame().axis().to_unit_length(),
                ),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
            if start[1] == end[1] =>
        {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let center = require_some!(FinitePoint3::new(offset_point(origin, *axis, start[1])));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    center,
                    *cylinder_surface.frame(),
                    cylinder_surface.radius(),
                ),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) if { start[0] == end[0] } => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv[0], uv[1]),
                )?)
                .map(|point| point.map(|point| [point.x, point.y, point.z]))
            });
            let [Some(first), Some(second)] = [first?, second?] else {
                return Ok(None);
            };
            let direction = require_some!(normalize(std::array::from_fn(
                |axis| second[axis] - first[axis]
            )));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                require_some!(cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::from(first),
                    Vector3::from(direction),
                )
                .ok()),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
            if { start[1] == end[1] } =>
        {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let local_radius = radius + start[1] * half_angle.tan();
            let first_radius = local_radius.abs();
            let second_radius = (local_radius * ratio).abs();
            if !first_radius.is_finite() || !second_radius.is_finite() {
                return Ok(None);
            }
            let center = offset_point(origin, *axis, start[1]);
            if (first_radius - second_radius).abs()
                <= EPS_NEAR_ZERO * first_radius.max(second_radius).max(1.0)
            {
                (first_radius > 0.0).then_some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::new(
                        require_some!(FinitePoint3::new(center)),
                        signed_reference_frame(*cone_surface.frame(), local_radius),
                        require_some!(PositiveLength::new(first_radius)),
                    ),
                )))
            } else {
                let transverse = cross(
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                let transverse = Vector3::from(transverse);
                let (major_direction, major_radius, minor_radius) = if first_radius > second_radius
                {
                    (
                        scaled_vector(*ref_direction, local_radius.signum()),
                        first_radius,
                        second_radius,
                    )
                } else {
                    (
                        scaled_vector(transverse, (local_radius * ratio).signum()),
                        second_radius,
                        first_radius,
                    )
                };
                (minor_radius > 0.0).then_some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    require_some!(cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                        center,
                        *axis,
                        major_direction,
                        major_radius,
                        minor_radius,
                    )
                    .ok()),
                )))
            }
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))
            if start[1] == end[1] =>
        {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let radius = sphere_surface.radius().get();
            require_some!((radius > 0.0).then_some(()));
            let ring = radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    require_some!(FinitePoint3::new(offset_point(
                        center,
                        *axis,
                        radius * start[1].sin()
                    ))),
                    signed_reference_frame(*sphere_surface.frame(), ring),
                    require_some!(PositiveLength::new(ring.abs())),
                ),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))
            if start[0] == end[0] =>
        {
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = require_some!(PositiveLength::try_from(sphere_surface.radius()).ok());
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            let frame = require_some!(OrthonormalFrame3::new(Vector3::from(normal), radial));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    sphere_surface.center(),
                    frame,
                    radius,
                ),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))
            if start[1] == end[1] =>
        {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            require_some!((minor_radius > 0.0).then_some(()));
            let ring = major_radius + minor_radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    require_some!(FinitePoint3::new(offset_point(
                        center,
                        *axis,
                        minor_radius * start[1].sin()
                    ))),
                    signed_reference_frame(*torus_surface.frame(), ring),
                    require_some!(PositiveLength::new(ring.abs())),
                ),
            )))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))
            if start[0] == end[0] =>
        {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius =
                require_some!(PositiveLength::try_from(torus_surface.minor_radius()).ok());
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            let center = require_some!(FinitePoint3::new(offset_point(
                center,
                radial,
                major_radius
            )));
            let frame = require_some!(OrthonormalFrame3::new(Vector3::from(normal), radial));
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(center, frame, minor_radius),
            )))
        }
        _ => None,
    })
}

pub(in crate::decode) fn transfer_analytic_pcurve_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<BTreeSet<CurveId>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo transfer analytic pcurve carriers scratch")?;
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    let curve_index = CurveIndex::new(ctx, &ir.model.curves)?;
    let reconciled_endpoints =
        scratch.with_storage(|| pcurve_edge_endpoint_evidence(ctx, scan, ir, source_carriers))?;
    let ignored_surface_ids = scratch.with_storage(|| {
        topology_ignored_surface_ids(ctx, &scan.framing.layout, &scan.surfaces.rows)
    })?;
    let mut candidates = BTreeMap::<u32, Vec<(CurveGeometry, usize)>>::new();
    let mut evaluable_path_counts = BTreeMap::<u32, usize>::new();
    {
        let mut retain_path = |curve_id: u32,
                               face_id: Option<NonZeroU32>,
                               endpoints: [[f64; 2]; 2],
                               offset: usize|
         -> Result<(), cadmpeg_core::CodecError> {
            let Some(face_id) = face_id.map(NonZeroU32::get) else {
                return Ok(());
            };
            if ignored_surface_ids.contains(&face_id) {
                return Ok(());
            }
            let Some(surface) = unique_model_surface(&ir.model.surfaces, face_id, &surface_index)
            else {
                return Ok(());
            };
            let geometry = source_carriers.surface_geometry(surface);
            // A path whose endpoint evaluates to a non-finite point is
            // evaluable; only an endpoint with no value is not.
            let mut evaluable = true;
            for uv in endpoints {
                match cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, geometry, uv[0], uv[1]),
                )? {
                    Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                        return Err(limit.into())
                    }
                    Err(cadmpeg_ir::eval::EvaluationFailure::NoValue) => {
                        evaluable = false;
                        break;
                    }
                    Ok(_) | Err(cadmpeg_ir::eval::EvaluationFailure::NonFinite(_)) => {}
                }
            }
            if evaluable {
                match scratch.with_storage(|| {
                    ctx.entry_btree_map(
                        &mut evaluable_path_counts,
                        curve_id,
                        "creo evaluable pcurve path count nodes",
                    )
                })? {
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        *entry.get_mut() += 1;
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(1);
                    }
                }
            }
            if let Some(carrier) = linear_pcurve_carrier(ctx, geometry, endpoints)? {
                match scratch.with_storage(|| {
                    ctx.entry_btree_map(
                        &mut candidates,
                        curve_id,
                        "creo analytic pcurve candidate nodes",
                    )
                })? {
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        scratch.with_storage(|| {
                            ctx.reserve_vec(entry.get_mut(), 1, "creo analytic pcurve candidates")
                        })?;
                        entry.get_mut().push((carrier, offset));
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let mut values = Vec::new();
                        scratch.with_storage(|| {
                            ctx.reserve_vec(&mut values, 1, "creo analytic pcurve candidates")
                        })?;
                        values.push((carrier, offset));
                        entry.insert(values);
                    }
                }
            }
            Ok(())
        };
        for pcurve in ctx.admit_iter(
            &scan.curves.pcurves,
            "creo reconcile support apex cone parameter branches pcurves traversal",
        )? {
            let endpoint_sets = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )?;
            for (face_id, endpoints) in pcurve.faces.into_iter().zip(endpoint_sets) {
                retain_path(pcurve.curve_id, face_id, endpoints, pcurve.offset)?;
            }
        }
        for pcurve in ctx.admit_iter(
            &scan.curves.bound_prototype_pcurves,
            "creo reconcile support apex cone parameter branches bound prototype pcurves traversal",
        )? {
            let endpoint_sets = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )?;
            for (face_id, endpoints) in pcurve.faces.into_iter().zip(endpoint_sets) {
                retain_path(pcurve.curve_id, face_id, endpoints, pcurve.offset)?;
            }
        }
        for pcurve in ctx.admit_iter(
            &scan.curves.two_chart_pcurves,
            "creo reconcile support apex cone parameter branches two chart pcurves traversal",
        )? {
            let faces = pcurve.faces.map(NonZeroU32::new);
            let Some(endpoint_sets) = (match map_two_chart_endpoint_sets(
                ctx,
                scan,
                ir,
                pcurve,
                source_carriers,
                &surface_index,
            )? {
                TwoChartMapping::Mapped {
                    endpoint_sets,
                    surface_mismatch: false,
                    ..
                } => endpoint_sets,
                _ => None,
            }) else {
                continue;
            };
            for (face_id, endpoints) in faces.into_iter().zip(endpoint_sets.paths()) {
                if let Some(endpoints) = endpoints {
                    retain_path(pcurve.curve_id, face_id, endpoints, pcurve.offset)?;
                }
            }
        }
        let mut short_storage = ctx.reserve_scoped(0, "creo analytic short pcurve storage")?;
        let short_pcurves = short_storage.with_storage(|| {
            crate::curve::fc02_short_pcurve_endpoints(
                ctx,
                &scan.curves.parameters,
                &scan.curves.topology_rows,
            )
        })?;
        for pcurve in ctx.admit_iter(&short_pcurves, "creo short pcurve carrier records")? {
            let faces = pcurve.faces.map(NonZeroU32::new);
            let [face_0_endpoints, _] = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                faces,
                pcurve.face_0_endpoints,
                pcurve.face_0_endpoints,
            )?;
            retain_path(pcurve.curve_id, faces[0], face_0_endpoints, pcurve.offset)?;
        }
    }
    let mut transferred = BTreeSet::new();
    let mut traversal = candidates.iter();
    while let Some((curve_id, candidates)) =
        ctx.next_charged(&mut traversal, "creo analytic pcurve candidate groups")?
    {
        if ctx
            .get_btree_map(
                &evaluable_path_counts,
                curve_id,
                "creo analytic evaluable path counts lookup",
            )?
            .copied()
            != Some(candidates.len())
        {
            continue;
        }
        let Some(points) = ctx
            .get_btree_map(
                &reconciled_endpoints,
                curve_id,
                "creo analytic reconciled endpoints lookup",
            )?
            .map(|evidence| evidence.points)
        else {
            continue;
        };
        let Some((geometry, offset)) = candidates.first() else {
            continue;
        };
        if !curve_contains_points(geometry, points) {
            continue;
        }
        let mut compatible = true;
        let mut traversal = (candidates).iter();
        while let Some((candidate, _)) = ctx.next_charged(
            &mut traversal,
            "creo transfer analytic pcurve carriers candidates traversal",
        )? {
            if !curve_contains_points(candidate, points) {
                compatible = false;
                break;
            }
            for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let point =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::curve_point(ctx, candidate, parameter),
                    )?)?;
                if !point.is_some_and(|point| {
                    curve_contains_points(geometry, [[point.x, point.y, point.z]; 2])
                }) {
                    compatible = false;
                    break;
                }
            }
            if !compatible {
                break;
            }
        }
        if !compatible {
            continue;
        }
        let offset = ctx
            .admit_iter(
                candidates,
                "creo analytic pcurve candidate minimum candidates",
            )?
            .map(|(_, offset)| *offset)
            .min()
            .unwrap_or(*offset);
        let id = crate::identity::compose_checked::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            *curve_id,
            "creo decoded model identity",
        )?;
        let curve_exists = curve_index.contains(*curve_id);
        if curve_exists {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(offset),
            "analytic_pcurve_carrier",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id: id.try_clone_for_decode(ctx, "creo analytic pcurve curve identity copy")?,
                geometry: geometry
                    .try_clone_for_decode(ctx, "creo analytic pcurve geometry copy")?,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{curve_id}"),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        ctx.insert_btree_set(
            &mut transferred,
            id,
            "creo transferred analytic pcurve nodes",
        )?;
    }
    Ok(transferred)
}

type PcurveVertexConstraint = ([u32; 2], [[f64; 3]; 2]);

pub(super) fn directed_pcurve_points(
    directions: [u8; 2],
    points: [[f64; 3]; 2],
) -> Option<[[f64; 3]; 2]> {
    match directions {
        [0x01, 0xf6] => Some(points),
        [0xf6, 0x01] => Some([points[1], points[0]]),
        _ => None,
    }
}

#[cfg(test)]
pub(in crate::decode) fn solve_pcurve_vertex_domains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    constraints: &[PcurveVertexConstraint],
    fixed_points: &BTreeMap<u32, [f64; 3]>,
    analytic_domains: &BTreeMap<u32, Vec<[f64; 3]>>,
    incident_curves: &BTreeMap<u32, Vec<&CurveGeometry>>,
) -> Result<BTreeMap<u32, [f64; 3]>, cadmpeg_core::CodecError> {
    solve_pcurve_vertex_domains_with_authoritative_points(
        ctx,
        constraints,
        fixed_points,
        analytic_domains,
        incident_curves,
        &BTreeMap::new(),
    )
}

/// Solve endpoint domains while preserving exact native one-sided witnesses.
///
/// An authoritative point is emitted only by a parser for a complete native
/// grammar. Such a point is still checked against fixed carrier solutions and
/// other pcurve constraints by the caller, but an inferred analytic
/// intersection or carrier curve must not erase it merely because that
/// inferred geometry is inconsistent.
pub(super) fn solve_pcurve_vertex_domains_with_authoritative_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    constraints: &[PcurveVertexConstraint],
    fixed_points: &BTreeMap<u32, [f64; 3]>,
    analytic_domains: &BTreeMap<u32, Vec<[f64; 3]>>,
    incident_curves: &BTreeMap<u32, Vec<&CurveGeometry>>,
    authoritative_points: &BTreeMap<u32, [f64; 3]>,
) -> Result<BTreeMap<u32, [f64; 3]>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo solve pcurve vertex domains with authoritative points scratch",
    )?;
    // A point outside the finite range agrees with no point.
    let agree = |first: [f64; 3], second: [f64; 3]| {
        finite_model_point(first)
            .zip(finite_model_point(second))
            .is_some_and(|(first, second)| model_points_agree(first, second))
    };
    let mut domains = BTreeMap::<u32, Vec<[f64; 3]>>::new();
    for (vertices, points) in ctx.admit_iter(constraints, "creo pcurve vertex constraints")? {
        if vertices[0] == vertices[1] {
            match scratch.with_storage(|| {
                ctx.entry_btree_map(&mut domains, vertices[0], "creo pcurve domain nodes")
            })? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let mut domain = Vec::new();
                    if agree(points[0], points[1]) {
                        scratch.with_storage(|| {
                            ctx.reserve_vec(&mut domain, 1, "creo pcurve domain points")
                        })?;
                        domain.push(points[0]);
                    }
                    entry.insert(domain);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let domain = entry.get_mut();
                    if agree(points[0], points[1]) {
                        ctx.retain_vec(
                            domain,
                            |candidate| Ok(agree(*candidate, points[0])),
                            "creo same-vertex pcurve domain retention",
                        )?;
                    } else {
                        ctx.truncate_vec(domain, 0, "creo conflicting pcurve domain removal")?;
                    }
                }
            }
            continue;
        }
        for vertex in vertices {
            let domain = match scratch.with_storage(|| {
                ctx.entry_btree_map(&mut domains, *vertex, "creo pcurve domain nodes")
            })? {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let mut domain = Vec::new();
                    scratch.with_storage(|| {
                        ctx.reserve_vec(&mut domain, 2, "creo pcurve domain points")
                    })?;
                    domain.extend_from_slice(points);
                    entry.insert(domain)
                }
            };
            ctx.retain_vec(
                domain,
                |candidate| Ok(points.iter().any(|point| agree(*candidate, *point))),
                "creo pcurve vertex domain retention",
            )?;
        }
    }
    for (vertex, candidates) in ctx.admit_iter(analytic_domains, "creo analytic vertex domains")? {
        if ctx.contains_key_btree_map(
            authoritative_points,
            vertex,
            "creo analytic authoritative points lookup",
        )? {
            continue;
        }
        match scratch.with_storage(|| {
            ctx.entry_btree_map(&mut domains, *vertex, "creo pcurve domain nodes")
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut domain = Vec::new();
                scratch.with_storage(|| {
                    ctx.extend_from_slice(&mut domain, candidates, "creo analytic domain points")
                })?;
                entry.insert(domain);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                ctx.retain_vec(
                    entry.get_mut(),
                    |point| {
                        ctx.any_by(
                            candidates,
                            |candidate| Ok(agree(*point, *candidate)),
                            "creo analytic vertex candidate agreement",
                        )
                    },
                    "creo analytic domain retention",
                )?;
            }
        }
    }
    for (vertex, point) in ctx.admit_iter(fixed_points, "creo fixed vertex points")? {
        match scratch.with_storage(|| {
            ctx.entry_btree_map(&mut domains, *vertex, "creo pcurve domain nodes")
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut domain = Vec::new();
                scratch
                    .with_storage(|| ctx.reserve_vec(&mut domain, 1, "creo fixed domain points"))?;
                domain.push(*point);
                entry.insert(domain);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                ctx.retain_vec(
                    entry.get_mut(),
                    |candidate| Ok(agree(*candidate, *point)),
                    "creo fixed pcurve domain retention",
                )?;
            }
        }
    }
    for (vertex, curves) in ctx.admit_iter(incident_curves, "creo incident curves by vertex")? {
        if ctx.contains_key_btree_map(
            authoritative_points,
            vertex,
            "creo analytic authoritative points lookup",
        )? {
            continue;
        }
        if let Some(domain) =
            ctx.get_mut_btree_map(&mut domains, vertex, "creo analytic domains lookup")?
        {
            ctx.retain_vec(
                domain,
                |candidate| {
                    ctx.all_by(
                        curves,
                        |curve| Ok(curve_contains_points(curve, [*candidate, *candidate])),
                        "creo incident curve vertex checks",
                    )
                },
                "creo incident analytic domain retention",
            )?;
        }
    }
    let compatible = |first: [f64; 3], second: [f64; 3], points: [[f64; 3]; 2]| {
        (agree(first, points[0]) && agree(second, points[1]))
            || (agree(first, points[1]) && agree(second, points[0]))
    };
    loop {
        ctx.charge_work(1, "creo pcurve vertex propagation rounds")?;
        let mut changed = false;
        for (vertices, points) in ctx.admit_iter(constraints, "creo pcurve vertex propagation")? {
            if vertices[0] == vertices[1] {
                continue;
            }
            let (retained_first, retained_second, first_len, second_len) = {
                let first = ctx
                    .get_btree_map(&domains, &vertices[0], "creo analytic domains lookup")?
                    .map_or(&[][..], Vec::as_slice);
                let second = ctx
                    .get_btree_map(&domains, &vertices[1], "creo analytic domains lookup")?
                    .map_or(&[][..], Vec::as_slice);
                let mut retained_first = Vec::new();
                for candidate in ctx.admit_iter(first, "creo first pcurve domain candidates")? {
                    if ctx.any_by(
                        second,
                        |other| Ok(compatible(*candidate, *other, *points)),
                        "creo second pcurve domain compatibility",
                    )? {
                        scratch.with_storage(|| {
                            ctx.reserve_vec(
                                &mut retained_first,
                                1,
                                "creo retained first pcurve domain",
                            )
                        })?;
                        retained_first.push(*candidate);
                    }
                }
                let mut retained_second = Vec::new();
                for candidate in ctx.admit_iter(second, "creo second pcurve domain candidates")? {
                    if ctx.any_by(
                        first,
                        |other| Ok(compatible(*other, *candidate, *points)),
                        "creo first pcurve domain compatibility",
                    )? {
                        scratch.with_storage(|| {
                            ctx.reserve_vec(
                                &mut retained_second,
                                1,
                                "creo retained second pcurve domain",
                            )
                        })?;
                        retained_second.push(*candidate);
                    }
                }
                (retained_first, retained_second, first.len(), second.len())
            };
            changed |= retained_first.len() != first_len || retained_second.len() != second_len;
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut domains,
                    vertices[0],
                    retained_first,
                    "creo pcurve domain nodes",
                )
            })?;
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut domains,
                    vertices[1],
                    retained_second,
                    "creo pcurve domain nodes",
                )
            })?;
        }
        if !changed {
            break;
        }
    }
    let mut solved = BTreeMap::new();
    let mut traversal = domains.iter();
    while let Some((vertex, domain)) =
        ctx.next_charged(&mut traversal, "creo solved pcurve domain groups")?
    {
        let mut previous_kept = None;
        let mut point = None;
        let mut unique = true;
        let mut traversal = (domain).iter();
        while let Some(candidate) =
            ctx.next_charged(&mut traversal, "creo solved pcurve domain points")?
        {
            if let Some(previous) = previous_kept {
                if agree(previous, *candidate) {
                    continue;
                }
                if point.is_some() {
                    unique = false;
                    break;
                }
            }
            point.get_or_insert(*candidate);
            previous_kept = Some(*candidate);
        }
        let Some(point) = point.filter(|_| unique) else {
            continue;
        };
        ctx.insert_btree_map(
            &mut solved,
            *vertex,
            point,
            "creo solved pcurve vertex nodes",
        )?;
    }
    Ok(solved)
}

pub(super) fn native_pcurve_midpoint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    edge_points: [[f64; 3]; 2],
) -> Result<Option<[f64; 3]>, cadmpeg_core::CodecError> {
    // A point outside the finite range, mapped or on the edge, aligns with
    // no point.
    let [first, second] = endpoints.map(|uv| {
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx, surface, uv[0], uv[1],
        ))
    });
    let ([Some(first), Some(second)], [Some(start), Some(end)]) = (
        [
            cadmpeg_ir::eval::finite_or_refusal(first?)?,
            cadmpeg_ir::eval::finite_or_refusal(second?)?,
        ],
        edge_points.map(finite_model_point),
    ) else {
        return Ok(None);
    };
    require_some!(point_pair_alignments([first, second], [start, end])
        .into_iter()
        .any(|matches| matches)
        .then_some(()));
    // A midpoint outside the finite range is returned as the evaluation
    // reached it.
    let point =
        match cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx,
            surface,
            f64::midpoint(endpoints[0][0], endpoints[1][0]),
            f64::midpoint(endpoints[0][1], endpoints[1][1]),
        ))? {
            Ok(point) => point.get(),
            Err(failure) => require_some!(failure.non_finite()?),
        };
    Ok(Some(<[f64; 3]>::from(point)))
}

pub(in crate::decode) type NativePcurveCandidates =
    BTreeMap<(u32, u32), Vec<([[f64; 2]; 2], usize)>>;

pub(in crate::decode) fn pcurve_backed_periodic_conic_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    incidence: (u32, [u32; 2]),
    candidates: &NativePcurveCandidates,
    surfaces: &[Surface],
    points: [[f64; 3]; 2],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let surface_index = SurfaceIndex::new(ctx, surfaces)?;
    let (curve_id, faces) = incidence;

    let mut selected = None;
    for face_id in faces {
        let Some(surface) = unique_model_surface(surfaces, face_id, &surface_index)
            .map(|surface| source_carriers.surface_geometry(surface))
        else {
            continue;
        };
        if let Some(face_candidates) = ctx.get_btree_map(
            candidates,
            &(curve_id, face_id),
            "creo analytic candidates lookup",
        )? {
            let mut traversal = (face_candidates).iter();
            while let Some((endpoints, _)) =
                ctx.next_charged(&mut traversal, "creo periodic conic pcurve candidates")?
            {
                let Some(interior) = native_pcurve_midpoint(ctx, surface, *endpoints, points)?
                else {
                    continue;
                };
                let candidate = require_some!(periodic_conic_edge_parameter_range(
                    ctx, geometry, points, interior
                )?);
                if selected.is_some_and(|selected: [f64; 2]| {
                    candidate
                        .into_iter()
                        .zip(selected)
                        .any(|(candidate, selected)| (candidate - selected).abs() > EPS_AGREE)
                }) {
                    return Ok(None);
                }
                selected = Some(candidate);
            }
        }
    }
    Ok(selected)
}

fn oriented_native_pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    traversal: [[f64; 3]; 2],
) -> Result<Option<[[f64; 2]; 2]>, cadmpeg_core::CodecError> {
    // A point outside the finite range, mapped or traversed, aligns with no
    // point.
    let [first, second] = endpoints.map(|uv| {
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx, surface, uv[0], uv[1],
        ))
    });
    let mapped = [
        cadmpeg_ir::eval::finite_or_refusal(first?)?,
        cadmpeg_ir::eval::finite_or_refusal(second?)?,
    ];
    let ([Some(first), Some(second)], [Some(start), Some(end)]) =
        (mapped, traversal.map(finite_model_point))
    else {
        return Ok(None);
    };
    Ok(match point_pair_alignments([first, second], [start, end]) {
        [true, false] => Some(endpoints),
        [false, true] => Some([endpoints[1], endpoints[0]]),
        _ => None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::decode) struct OrientedNativePcurve {
    pub(in crate::decode) endpoints: [[f64; 2]; 2],
    pub(in crate::decode) offset: usize,
}

pub(in crate::decode) fn unique_oriented_native_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    candidates: &[([[f64; 2]; 2], usize)],
    traversal: [[f64; 3]; 2],
) -> Result<Option<OrientedNativePcurve>, cadmpeg_core::CodecError> {
    let mut selected: Option<OrientedNativePcurve> = None;
    let mut candidate_rows = candidates.iter();
    while let Some((endpoints, offset)) = ctx.next_charged(
        &mut candidate_rows,
        "creo oriented native pcurve candidates",
    )? {
        let Some(oriented) = oriented_native_pcurve_endpoints(ctx, surface, *endpoints, traversal)?
        else {
            continue;
        };
        if let Some(previous) = selected.as_mut() {
            if oriented != previous.endpoints {
                return Ok(None);
            }
            previous.offset = previous.offset.min(*offset);
        } else {
            selected = Some(OrientedNativePcurve {
                endpoints: oriented,
                offset: *offset,
            });
        }
    }
    Ok(selected)
}

pub(in crate::decode) fn planar_curve_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<PcurveGeometry>, cadmpeg_core::CodecError> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
        return Ok(planar_primitive_pcurve(surface, geometry));
    };
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = surface else {
        return Ok(None);
    };
    let origin = <[f64; 3]>::from(plane_surface.origin().get());
    let normal = unit_length(*plane_surface.frame().axis());
    let u_axis = unit_length(*plane_surface.frame().reference());
    if !matches!(
        dot(normal, u_axis).abs().partial_cmp(&EPS_ORTHO),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) {
        return Ok(None);
    }
    let Some(v_axis) = normalize(cross(normal, u_axis)) else {
        return Ok(None);
    };
    if nurbs_intrinsic_parameter_range(nurbs).is_none() {
        return Ok(None);
    }
    let tolerance = EPS_AGREE * nurbs_control_extent(ctx, nurbs)?;
    let project_point = |point: Point3| {
        let point: [f64; 3] = point.into();
        let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
        (dot(relative, normal).abs() <= tolerance)
            .then_some(Point2::new(dot(relative, u_axis), dot(relative, v_axis)))
    };
    let poles = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            let mut projected = Vec::new();
            ctx.reserve_vec(
                &mut projected,
                points.len(),
                "creo planar projected NURBS poles",
            )?;
            let mut points = points.iter();
            while let Some(point) =
                ctx.next_charged(&mut points, "creo planar polynomial NURBS poles")?
            {
                let Some(projected_point) = project_point(point.get()) else {
                    return Ok(None);
                };
                let Some(projected_point) = cadmpeg_ir::units::FinitePoint2::new(projected_point)
                else {
                    let reason = ctx.copy_retained_text(
                        "control_points contains a non-finite point",
                        "creo planar projected NURBS refusal text",
                    )?;
                    refusal.note_checked(
                        ctx,
                        format_args!("creo planar-curve pcurve record for {record}"),
                        &cadmpeg_ir::geometry::nurbs::NurbsError::Structure(reason),
                    );
                    return Ok(None);
                };
                projected.push(projected_point);
            }
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points: projected }
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let mut projected = Vec::new();
            ctx.reserve_vec(
                &mut projected,
                points.len(),
                "creo planar projected NURBS poles",
            )?;
            let mut points = points.iter();
            while let Some(pole) =
                ctx.next_charged(&mut points, "creo planar rational NURBS poles")?
            {
                let Some(projected_point) = project_point(pole.point.get()) else {
                    return Ok(None);
                };
                let Some(projected_point) = cadmpeg_ir::units::FinitePoint2::new(projected_point)
                else {
                    let reason = ctx.copy_retained_text(
                        "control_points contains a non-finite point",
                        "creo planar projected NURBS refusal text",
                    )?;
                    refusal.note_checked(
                        ctx,
                        format_args!("creo planar-curve pcurve record for {record}"),
                        &cadmpeg_ir::geometry::nurbs::NurbsError::Structure(reason),
                    );
                    return Ok(None);
                };
                projected.push(cadmpeg_ir::geometry::pcurve::WeightedPole2 {
                    point: projected_point,
                    weight: pole.weight,
                });
            }
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Rational { points: projected }
        }
    };
    let knots = nurbs
        .knots()
        .try_clone_for_decode(ctx, "creo planar projected NURBS knots")?;
    match PcurveNurbs::new(ctx, nurbs.degree(), knots, poles, nurbs.periodic())? {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo planar-curve pcurve record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

fn planar_primitive_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = surface else {
        return None;
    };
    let origin = plane_surface.origin().get();
    let origin = [origin.x, origin.y, origin.z];
    let normal = unit_length(*plane_surface.frame().axis());
    let u_axis = unit_length(*plane_surface.frame().reference());
    (dot(normal, u_axis).abs() <= EPS_ORTHO).then_some(())?;
    let v_axis = normalize(cross(normal, u_axis))?;
    let project_point = |point: [f64; 3], tolerance: f64| {
        let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
        (dot(relative, normal).abs() <= tolerance)
            .then_some(Point2::new(dot(relative, u_axis), dot(relative, v_axis)))
    };
    let project_direction = |direction: [f64; 3]| {
        let length = dot(direction, direction).sqrt();
        (length.is_finite() && length > 0.0 && dot(direction, normal).abs() <= EPS_ORTHO * length)
            .then_some(Point2::new(dot(direction, u_axis), dot(direction, v_axis)))
    };
    let conic_frame = |center: [f64; 3], frame: &OrthonormalFrame3, scale: f64| {
        let axis = unit_length(*frame.axis());
        let x_axis = unit_length(*frame.reference());
        ((dot(axis, normal).abs() - 1.0).abs() <= EPS_ORTHO
            && dot(axis, x_axis).abs() <= EPS_ORTHO)
            .then_some(())?;
        let y_axis = normalize(cross(axis, x_axis))?;
        Some((
            project_point(center, EPS_AGREE * scale.max(1.0))?,
            project_direction(x_axis)?,
            project_direction(y_axis)?,
        ))
    };

    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            let direction = [direction.x, direction.y, direction.z];
            Some(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    project_point([origin.x, origin.y, origin.z], EPS_AGREE)?,
                    project_direction(direction)?,
                )
                .ok()?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let radius = circle_curve.radius().get();
            let (center, x_axis, y_axis) =
                conic_frame([center.x, center.y, center.z], circle_curve.frame(), radius)?;
            Some(PcurveGeometry::Circle(
                cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(center, x_axis, y_axis, radius)
                    .ok()?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                ellipse_curve.frame(),
                major_radius.max(minor_radius),
            )?;
            Some(PcurveGeometry::Ellipse(
                cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                )
                .ok()?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = parabola_curve.vertex().get();
            let focal_distance = parabola_curve.focal_distance().get();
            let (vertex, x_axis, y_axis) = conic_frame(
                [vertex.x, vertex.y, vertex.z],
                parabola_curve.frame(),
                focal_distance,
            )?;
            Some(PcurveGeometry::Parabola(
                cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
                    vertex,
                    x_axis,
                    y_axis,
                    focal_distance,
                )
                .ok()?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = hyperbola_curve.center().get();
            let major_radius = hyperbola_curve.major_radius().get();
            let minor_radius = hyperbola_curve.minor_radius().get();
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                hyperbola_curve.frame(),
                major_radius.max(minor_radius),
            )?;
            Some(PcurveGeometry::Hyperbola(
                cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::try_new(
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                )
                .ok()?,
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::super::equations::{CarrierEquation, PlaneEquation};
    use super::SurfaceIndex;
    use super::{
        map_two_chart_endpoint_sets, mapped_pcurve_endpoints, mapped_two_chart_endpoint_sets,
        mirrored_support_apex_cone, pcurve_edge_endpoint_evidence_with_carriers,
        pcurve_edge_endpoint_evidence_with_diagnostics, pcurve_mismatch_detail,
        pcurve_plane_carrier_status, point_coordinate_error, support_cone_witness_matches,
        topology_ignored_surface_ids, transfer_analytic_pcurve_carriers, MappedPcurvePath,
        PcurveCarrierStatus, PcurveCarrierUnknownReason, TwoChartEndpointSets, TwoChartMapping,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::nurbs::NurbsSurface;
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::BTreeMap;
    use std::collections::{BTreeSet, HashSet};

    #[test]
    fn active_pcurve_path_membership_refuses_work_and_preserves_service_result() {
        let face_zero = std::num::NonZeroU32::new(10).expect("one-based fixture face");
        let face_one = std::num::NonZeroU32::new(11).expect("one-based fixture face");
        let faces = [Some(face_zero), Some(face_one)];
        let activity = super::PcurvePathActivity {
            active_paths: BTreeSet::from([(Some(face_zero), 7)]),
            topology_faces: BTreeMap::from([(7, Some(faces))]),
            prototype_faces: BTreeMap::new(),
        };
        let selected = crate::test_support::assert_work_boundaries(
            &["creo active pcurve path membership"],
            |ctx| activity.selected_paths(ctx, 7, faces, false),
        );
        assert_eq!(selected, Some([true, false]));
    }

    #[test]
    fn pcurve_vertex_propagation_round_refuses_work() {
        let constraints: [super::PcurveVertexConstraint; 1] =
            [([1, 2], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])];
        let fixed_points = BTreeMap::from([(1, [0.0, 0.0, 0.0]), (2, [1.0, 0.0, 0.0])]);
        let solved = crate::test_support::assert_work_boundaries(
            &["creo pcurve vertex propagation rounds"],
            |ctx| {
                super::solve_pcurve_vertex_domains(
                    ctx,
                    &constraints,
                    &fixed_points,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                )
            },
        );
        assert_eq!(
            solved,
            BTreeMap::from([(1, [0.0, 0.0, 0.0]), (2, [1.0, 0.0, 0.0])]),
        );
    }

    #[test]
    fn legacy_spline_topology_filter_is_layout_scoped() {
        let rows = vec![
            crate::surface::SurfaceRow {
                id: 7,
                kind: crate::surface::SurfaceKind::Spline,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
            crate::surface::SurfaceRow {
                id: 8,
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
        ];

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| topology_ignored_surface_ids(
                ctx,
                &crate::test_support::legacy_layout(),
                &rows
            ))
            .expect("service spline filter"),
            HashSet::from([7]),
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| topology_ignored_surface_ids(
                ctx,
                &crate::container::Layout::Nd,
                &rows
            ))
            .expect("service spline filter")
            .is_empty()
        );
    }

    fn one_plane_pcurve_fixture() -> (crate::container::ContainerScan<'static>, CadIr) {
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [std::num::NonZeroU32::new(10), None],
            face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            face_1_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            offset: 0,
        });
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#10".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        (scan, ir)
    }

    fn pcurve_evidence_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
        let (scan, ir) = one_plane_pcurve_fixture();
        crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                pcurve_edge_endpoint_evidence_with_carriers(
                    ctx,
                    &scan,
                    &ir,
                    &BTreeMap::new(),
                    &crate::decode::source_carriers::SourceUnitCarriers::default(),
                    None,
                )
            },
        )
    }

    fn analytic_pcurve_transfer_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
        crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                let (scan, mut ir) = one_plane_pcurve_fixture();
                transfer_analytic_pcurve_carriers(
                    ctx,
                    &scan,
                    &mut ir,
                    &mut cadmpeg_ir::AnnotationBuilder::new(),
                    &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
                )
            },
        )
    }

    fn assert_pcurve_collection_refusal(error: &cadmpeg_core::CodecError, operation: &str) {
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && resource.operation == operation)
        );
    }

    fn support_cone_witness_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
        crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                let planes = BTreeMap::from([(
                    2,
                    PlaneEquation {
                        origin: [0.0, 0.0, 0.0],
                        normal: [0.0, 0.0, 1.0],
                    },
                )]);
                super::collect_support_cone_plane_witness(
                    ctx,
                    &mut BTreeMap::new(),
                    &planes,
                    [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
                    [Some([[1.0, 2.0], [3.0, 4.0]]), None],
                )
            },
        )
    }

    #[test]
    fn support_cone_witness_refuses_map_node() {
        assert_pcurve_collection_refusal(
            &support_cone_witness_limit_error("creo support cone witness nodes"),
            "creo support cone witness nodes",
        );
    }

    #[test]
    fn support_cone_witness_refuses_nested_member() {
        assert_pcurve_collection_refusal(
            &support_cone_witness_limit_error("creo support cone plane witnesses"),
            "creo support cone plane witnesses",
        );
    }

    #[test]
    fn support_cone_witness_preserves_service_result() {
        let planes = BTreeMap::from([(
            2,
            PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
        )]);
        let mut witnesses = BTreeMap::new();
        crate::decode::with_test_decode_ctx(|ctx| {
            super::collect_support_cone_plane_witness(
                ctx,
                &mut witnesses,
                &planes,
                [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
                [Some([[1.0, 2.0], [3.0, 4.0]]), None],
            )
        })
        .expect("service support cone witness");
        let witness = witnesses.get(&1).expect("face witness");
        assert_eq!(witness.len(), 1);
        assert_eq!(witness[0].0, [[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(witness[0].1.origin, planes[&2].origin);
        assert_eq!(witness[0].1.normal, planes[&2].normal);
    }

    #[test]
    fn legacy_spline_filter_refuses_ignored_surface_node() {
        let row = crate::surface::SurfaceRow {
            id: 7,
            kind: crate::surface::SurfaceKind::Spline,
            feature_id: 0,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root");
        let error =
            topology_ignored_surface_ids(&ctx, &crate::test_support::legacy_layout(), &[row])
                .expect_err("spline set exceeds limit");
        assert_pcurve_collection_refusal(&error, "creo ignored spline surface IDs");
    }

    #[test]
    fn pcurve_evidence_refuses_candidate_path_vector() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo pcurve candidate paths"),
            "creo pcurve candidate paths",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_mapped_path_vector() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo mapped pcurve paths"),
            "creo mapped pcurve paths",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_selected_path_vector() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo selected mapped pcurve paths"),
            "creo selected mapped pcurve paths",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_candidate_node() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo pcurve evidence candidate nodes"),
            "creo pcurve evidence candidate nodes",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_candidate_member() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo pcurve evidence candidates"),
            "creo pcurve evidence candidates",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_endpoint_node() {
        assert_pcurve_collection_refusal(
            &pcurve_evidence_limit_error("creo pcurve endpoint evidence nodes"),
            "creo pcurve endpoint evidence nodes",
        );
    }

    #[test]
    fn pcurve_evidence_refuses_carrier_mapped_path_vector() {
        let (mut scan, mut ir) = one_plane_pcurve_fixture();
        scan.curves.pcurves[0].faces[1] = std::num::NonZeroU32::new(11);
        scan.curves.pcurves[0].face_0_endpoints = [[1.0, 0.0], [3.0, 0.0]];
        scan.curves.pcurves[0].face_1_endpoints = [[1.0, 0.0], [3.0, 0.0]];
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#11".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        let carriers = BTreeMap::from([
            (
                10,
                CarrierEquation::Plane(PlaneEquation {
                    origin: [0.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                }),
            ),
            (
                11,
                CarrierEquation::Plane(PlaneEquation {
                    origin: [0.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                }),
            ),
        ]);
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo carrier mapped pcurve paths",
            |ctx| {
                pcurve_edge_endpoint_evidence_with_carriers(
                    ctx,
                    &scan,
                    &ir,
                    &carriers,
                    &crate::decode::source_carriers::SourceUnitCarriers::default(),
                    None,
                )
            },
        );
        assert_pcurve_collection_refusal(&error, "creo carrier mapped pcurve paths");
    }

    #[test]
    fn pcurve_evidence_refuses_mismatch_sample_vector() {
        let (mut scan, mut ir) = one_plane_pcurve_fixture();
        scan.curves.pcurves[0].faces[1] = std::num::NonZeroU32::new(11);
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#11".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo pcurve mismatch samples",
            |ctx| {
                pcurve_edge_endpoint_evidence_with_carriers(
                    ctx,
                    &scan,
                    &ir,
                    &BTreeMap::new(),
                    &crate::decode::source_carriers::SourceUnitCarriers::default(),
                    None,
                )
            },
        );
        assert_pcurve_collection_refusal(&error, "creo pcurve mismatch samples");
    }

    #[test]
    fn pcurve_evidence_preserves_one_plane_path_at_service_limit() {
        let (scan, ir) = one_plane_pcurve_fixture();
        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_carriers(
                ctx,
                &scan,
                &ir,
                &BTreeMap::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                None,
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(
            evidence.get(&7).map(|entry| entry.points),
            Some([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]])
        );
        assert_eq!(diagnostics.accepted_records, 1);
    }

    #[test]
    fn analytic_pcurve_transfer_refuses_evaluable_count_node() {
        assert_pcurve_collection_refusal(
            &analytic_pcurve_transfer_limit_error("creo evaluable pcurve path count nodes"),
            "creo evaluable pcurve path count nodes",
        );
    }

    #[test]
    fn analytic_pcurve_transfer_refuses_candidate_node() {
        assert_pcurve_collection_refusal(
            &analytic_pcurve_transfer_limit_error("creo analytic pcurve candidate nodes"),
            "creo analytic pcurve candidate nodes",
        );
    }

    #[test]
    fn analytic_pcurve_transfer_refuses_candidate_member() {
        assert_pcurve_collection_refusal(
            &analytic_pcurve_transfer_limit_error("creo analytic pcurve candidates"),
            "creo analytic pcurve candidates",
        );
    }

    #[test]
    fn analytic_pcurve_transfer_refuses_transferred_node() {
        assert_pcurve_collection_refusal(
            &analytic_pcurve_transfer_limit_error("creo transferred analytic pcurve nodes"),
            "creo transferred analytic pcurve nodes",
        );
    }

    #[test]
    fn analytic_pcurve_transfer_refuses_retained_model_identity_copy() {
        use cadmpeg_core::decode::ResourceDimension;
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::RetainedBytes,
            "creo analytic pcurve curve identity copy",
            |ctx| {
                let (scan, mut ir) = one_plane_pcurve_fixture();
                transfer_analytic_pcurve_carriers(
                    ctx,
                    &scan,
                    &mut ir,
                    &mut cadmpeg_ir::AnnotationBuilder::new(),
                    &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo analytic pcurve curve identity copy")
        );
    }

    #[test]
    fn analytic_pcurve_transfer_minimum_comparison_refuses_work() {
        let transferred = crate::test_support::assert_work_boundaries(
            &["creo analytic pcurve candidate minimum candidates"],
            |ctx| {
                let (mut scan, mut ir) = one_plane_pcurve_fixture();
                let duplicate = scan.curves.pcurves[0].clone();
                scan.curves.pcurves.push(duplicate);
                transfer_analytic_pcurve_carriers(
                    ctx,
                    &scan,
                    &mut ir,
                    &mut cadmpeg_ir::AnnotationBuilder::new(),
                    &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
                )
            },
        );
        assert_eq!(transferred.len(), 1);
    }

    #[test]
    fn analytic_pcurve_transfer_selects_minimum_source_offset() {
        let (mut scan, mut ir) = one_plane_pcurve_fixture();
        scan.curves.pcurves[0].offset = 41;
        let mut later_minimum = scan.curves.pcurves[0].clone();
        later_minimum.offset = 17;
        scan.curves.pcurves.push(later_minimum);

        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut source_carriers,
            )
        })
        .expect("service analytic pcurve transfer");

        assert_eq!(
            transferred.iter().map(CurveId::as_str).collect::<Vec<_>>(),
            ["creo:visibgeom:curve#7"]
        );
        assert_eq!(ir.model.curves.len(), 1);
        let curve = &ir.model.curves[0];
        assert_eq!(curve.id.as_str(), "creo:visibgeom:curve#7");
        assert_eq!(
            curve
                .source_object
                .as_ref()
                .expect("source object association")
                .object_id
                .as_str(),
            "VisibGeom:7"
        );
        let provenance = &annotations.annotations().provenance["creo:visibgeom:curve#7"];
        assert_eq!(provenance.stream(), "creo:VisibGeom");
        assert_eq!(provenance.offset, 17);
        assert_eq!(provenance.tag.as_deref(), Some("analytic_pcurve_carrier"));
    }

    #[test]
    fn analytic_pcurve_transfer_preserves_service_curve() {
        let (scan, mut ir) = one_plane_pcurve_fixture();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut source_carriers,
            )
        })
        .expect("service analytic pcurve transfer");
        assert_eq!(transferred.len(), 1);
        assert_eq!(ir.model.curves.len(), 1);
    }

    #[test]
    fn mapped_pcurve_endpoints_reject_duplicate_face_surfaces() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#7".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#7".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 1.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
        ]);

        assert!(mapped_pcurve_endpoints(
            &ir,
            [7, 7],
            [[[0.0, 0.0], [1.0, 0.0]], [[0.0, 0.0], [1.0, 0.0]]],
        )
        .expect("evaluator allocation succeeds")
        .is_none());
    }

    #[test]
    fn placed_plane_pcurve_mapping_uses_source_origin() {
        let id = SurfaceId::mint("creo:visibgeom:surface#7").expect("identity grammar");
        let plane = |x| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(x, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            ))
        };
        let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        source_carriers.record_surface(&Surface {
            id: id.clone(),
            geometry: plane(1.0),
            source_object: None,
        });
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id,
            geometry: plane(25.4),
            source_object: None,
        });

        let mapped = crate::decode::with_test_decode_ctx(|ctx| {
            super::map_pcurve_paths(
                ctx,
                &ir,
                [(std::num::NonZeroU32::new(7), [[0.0, 0.0], [1.0, 0.0]])],
                &source_carriers,
                &SurfaceIndex::new(
                    &cadmpeg_test_support::service_decode_context(),
                    &ir.model.surfaces,
                )
                .expect("surface index fixture"),
            )
        })
        .expect("service pcurve paths");
        assert_eq!(mapped.missing_surfaces, 0);
        assert_eq!(mapped.unevaluable_paths, 0);
        assert_eq!(mapped.mapped.len(), 1);
        assert_eq!(
            mapped.mapped[0].endpoints,
            [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]
        );
    }

    #[test]
    fn two_chart_samples_validate_every_point_and_extend_a_nurbs_boundary_span() {
        let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
        let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &evaluation_arena,
            &evaluation_policy,
        )
        .expect("evaluation root");

        let scan = crate::test_support::empty_container_scan();
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#7".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                    NurbsSurface::from_lanes(
                        &cadmpeg_test_support::service_decode_context(),
                        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                            1,
                            vec![0.0, 0.0, 1.0, 1.0],
                            false,
                        ),
                        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                            1,
                            vec![0.0, 0.0, 1.0, 1.0],
                            false,
                        ),
                        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                            vec![
                                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                            ],
                            None,
                        ),
                        false,
                    )
                    .expect("fixture constructor admission")
                    .expect("valid test surface"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#8".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
        ]);
        let mut pcurve = crate::curve::TwoChartPcurveSamples {
            curve_id: 9,
            faces: [7, 8],
            samples: vec![
                [[-0.01, 0.25], [-0.01, 0.25]],
                [[0.5, 0.5], [0.5, 0.5]],
                [[1.01, 0.75], [1.01, 0.75]],
            ],
            offset: 0,
        };

        assert_eq!(
            mapped_two_chart_endpoint_sets(
                &evaluation_ctx,
                &scan,
                &ir,
                &pcurve,
                &crate::decode::source_carriers::SourceUnitCarriers::default()
            )
            .expect("evaluation resources"),
            Some(TwoChartEndpointSets::Both([
                [[-0.01, 0.25], [1.01, 0.75]],
                [[-0.01, 0.25], [1.01, 0.75]],
            ]))
        );

        pcurve.samples[1][1][0] = 0.6;
        let mapping = map_two_chart_endpoint_sets(
            &evaluation_ctx,
            &scan,
            &ir,
            &pcurve,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &SurfaceIndex::new(
                &cadmpeg_test_support::service_decode_context(),
                &ir.model.surfaces,
            )
            .expect("surface index fixture"),
        )
        .expect("evaluation resources");
        assert!(matches!(
            mapping,
            TwoChartMapping::Mapped {
                surface_mismatch: true,
                endpoint_sets: Some(_),
                ..
            }
        ));
        assert!(mapped_two_chart_endpoint_sets(
            &evaluation_ctx,
            &scan,
            &ir,
            &pcurve,
            &crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("evaluation resources")
        .is_none());

        pcurve.samples[1][1][0] = 0.5;
    }

    #[test]
    fn two_chart_endpoint_carrier_proof_ignores_interior_disagreement() {
        const EPS_EXPECTED_POINT: f64 = 1.0e-12;
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves
            .two_chart_pcurves
            .push(crate::curve::TwoChartPcurveSamples {
                curve_id: 17,
                faces: [1, 2],
                samples: vec![
                    [[0.0, 0.5], [0.0, -0.5]],
                    [[std::f64::consts::FRAC_PI_2, 0.5], [1.0, 0.0]],
                    [[std::f64::consts::PI, 0.5], [0.0, 0.5]],
                ],
                offset: 0,
            });
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#1".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                    cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                        Point3::new(-1.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, -1.0),
                        0.0,
                        1.0,
                        std::f64::consts::FRAC_PI_4,
                    )
                    .expect("valid ConeSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#2".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(-0.5, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
        ]);
        let carriers = BTreeMap::from([
            (
                1,
                CarrierEquation::Cone(
                    super::super::equations::ConeEquation::new(
                        [-1.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [0.0, 0.0, -1.0],
                        0.0,
                        1.0,
                        std::f64::consts::FRAC_PI_4,
                    )
                    .expect("valid test cone"),
                ),
            ),
            (
                2,
                CarrierEquation::Plane(PlaneEquation {
                    origin: [-0.5, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                }),
            ),
        ]);
        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_carriers(
                ctx,
                &scan,
                &ir,
                &carriers,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                None,
            )
        })
        .expect("service pcurve evidence");

        assert_eq!(diagnostics.two_chart_surface_mismatch_records, 1);
        assert_eq!(diagnostics.carrier_validated_paths, 2);
        assert_eq!(diagnostics.accepted_records, 1);
        let evidence = evidence.get(&17).expect("two-chart endpoint evidence");
        assert!(evidence.complete);
        assert!(
            point_coordinate_error(evidence.points[0], [-0.5, 0.0, -0.5]) <= EPS_EXPECTED_POINT
        );
        assert!(point_coordinate_error(evidence.points[1], [-0.5, 0.0, 0.5]) <= EPS_EXPECTED_POINT);
    }

    #[test]
    fn pcurve_mismatch_diagnostics_measure_both_endpoint_orders() {
        const EPS_TEST_ERROR: f64 = 1.0e-12;
        let mapped = vec![
            MappedPcurvePath {
                face_id: 7,
                endpoints: [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            },
            MappedPcurvePath {
                face_id: 8,
                endpoints: [[1.0, 0.0, 0.0], [3.0, 0.0, 0.0]],
            },
        ];

        let detail = crate::test_support::assert_work_boundaries(
            &["creo pcurve mismatch comparisons"],
            |ctx| pcurve_mismatch_detail(ctx, 11, &mapped),
        )
        .expect("two mapped paths");
        assert_eq!(detail.curve_id, 11);
        assert_eq!(detail.faces, [7, 8]);
        assert!((detail.same_order_error - 1.0).abs() < EPS_TEST_ERROR);
        assert!((detail.reverse_order_error - 3.0).abs() < EPS_TEST_ERROR);
    }

    #[test]
    fn pcurve_diagnostics_count_inactive_face_paths() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 7,
                type_byte: 0,
                feature_id: 0,
                directions: [0x01, 0xf6],
                faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
                next_edges: [7, 7],
                offset: 0,
            });
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [10, 11].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            face_1_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            offset: 0,
        });
        scan.topology.loops.push(crate::test_support::closed_loop(
            std::num::NonZeroU32::new(10),
            vec![crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            }],
        ));
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#10".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#11".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
        ]);

        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_diagnostics(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(
            evidence.get(&7).map(|value| (value.points, value.complete)),
            Some(([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]], true))
        );
        assert_eq!(diagnostics.records, 1);
        assert_eq!(diagnostics.paths(), 2);
        assert_eq!(diagnostics.inactive_paths, 1);
        assert_eq!(diagnostics.inactive_records, 0);
        assert_eq!(diagnostics.partial_records, 1);
        assert_eq!(diagnostics.topology_mismatch_records, 0);
        assert_eq!(diagnostics.accepted_records, 1);
        assert_eq!(diagnostics.complete_records, 1);
    }

    #[test]
    fn short_fc02_path_supplies_one_sided_endpoint_evidence() {
        let token_specs = [
            (-14.5, vec![0x48, 0x45, 0x00]),
            (0.75, vec![0x2a, 0xe8, 0x00]),
            (0.0, vec![0x18]),
            (1.0, vec![0xe4]),
            (-12.5, vec![0x48, 0x41, 0x00]),
            (0.75, vec![0x2a, 0xe8, 0x00]),
            (
                f64::from_bits(0x3fff_ffff_ffff_ffff),
                vec![0x29, 0xff, 0xff],
            ),
        ];
        let mut body = vec![0xfc, 0x02];
        let mut scalar_tokens = Vec::new();
        for (value, raw) in token_specs {
            let offset = body.len();
            body.extend_from_slice(&raw);
            scalar_tokens.push(crate::curve::CurveParameterScalar { value, raw, offset });
        }
        body.extend_from_slice(&[0x34, 0xb0, 0x00]);
        let record = crate::curve::CurveParameterRecord {
            curve_id: 846,
            type_byte: 0,
            scalar_tokens,
            reference_geometry: [0, 0],
            opaque_spans: vec![
                crate::curve::CurveParameterOpaqueSpan {
                    raw: vec![0xfc, 0x02],
                    offset: 0,
                },
                crate::curve::CurveParameterOpaqueSpan {
                    raw: vec![0x34, 0xb0, 0x00],
                    offset: body.len() - 3,
                },
            ],
            body,
            references: Vec::new(),
            offset: 100,
            body_offset: 100,
            suffix_offset: 122,
        };
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.parameters.push(record);
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 846,
                type_byte: 0,
                feature_id: 57,
                directions: [0x01, 0xf6],
                faces: [
                    std::num::NonZeroU32::new(43),
                    std::num::NonZeroU32::new(163),
                ],
                next_edges: [841, 164],
                offset: 100,
            });
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#43".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });

        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_diagnostics(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(
            evidence
                .get(&846)
                .map(|value| (value.points, value.complete)),
            Some(([[-14.5, 0.0, -0.75], [-12.5, 0.0, -0.75]], false,))
        );
        assert_eq!(diagnostics.records, 1);
        assert_eq!(diagnostics.paths(), 1);
        assert_eq!(diagnostics.mapped_paths, 1);
        assert_eq!(diagnostics.accepted_records, 1);
        assert_eq!(diagnostics.evidence, 1);
        assert_eq!(diagnostics.complete_records, 0);

        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert_eq!(
            transferred,
            BTreeSet::from([
                CurveId::mint("creo:visibgeom:curve#846".to_string()).expect("identity grammar")
            ])
        );
        assert!(ir.model.curves.iter().any(|curve| {
            curve.id
                == CurveId::mint("creo:visibgeom:curve#846".to_string()).expect("identity grammar")
                && matches!(
                    curve.geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                )
        }));
    }

    #[test]
    fn pcurve_plane_carrier_status_requires_a_unique_join() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ));
        let face_carrier = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        let crossing_carrier = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            ({
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &arena,
                    &cadmpeg_core::decode::DecodePolicy::service(),
                )
                .expect("root");
                pcurve_plane_carrier_status(
                    &ctx,
                    &surface,
                    face_carrier,
                    crossing_carrier,
                    [[0.0, 0.0], [0.0, 1.0]],
                )
            })
            .expect("evaluation resources"),
            PcurveCarrierStatus::Validated,
        );
        assert_eq!(
            ({
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &arena,
                    &cadmpeg_core::decode::DecodePolicy::service(),
                )
                .expect("root");
                pcurve_plane_carrier_status(
                    &ctx,
                    &surface,
                    face_carrier,
                    crossing_carrier,
                    [[0.0, 0.0], [1.0, 0.0]],
                )
            })
            .expect("evaluation resources"),
            PcurveCarrierStatus::Rejected,
        );
        assert_eq!(
            ({
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &arena,
                    &cadmpeg_core::decode::DecodePolicy::service(),
                )
                .expect("root");
                pcurve_plane_carrier_status(
                    &ctx,
                    &surface,
                    face_carrier,
                    CarrierEquation::Plane(PlaneEquation {
                        origin: [0.0, 0.0, 1.0],
                        normal: [0.0, 0.0, 1.0],
                    }),
                    [[0.0, 0.0], [0.0, 1.0]],
                )
            })
            .expect("evaluation resources"),
            PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::ParallelPlanePair),
        );
    }

    #[test]
    fn pcurve_carrier_join_keeps_one_valid_face_path() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 7,
                type_byte: 0,
                feature_id: 0,
                directions: [0x01, 0xf6],
                faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
                next_edges: [7, 7],
                offset: 0,
            });
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [10, 11].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[0.0, 0.0], [0.0, 1.0]],
            face_1_endpoints: [[0.0, 0.0], [0.0, 1.0]],
            offset: 0,
        });
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#10".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#11".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
        ]);

        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_diagnostics(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(
            evidence.get(&7).map(|value| (value.points, value.complete)),
            Some(([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]], false)),
        );
        assert_eq!(diagnostics.carrier_validated_paths, 1);
        assert_eq!(diagnostics.carrier_rejected_paths, 1);
        assert_eq!(diagnostics.carrier_rejected_records, 0);
        assert_eq!(diagnostics.accepted_records, 1);
    }

    #[test]
    fn support_apex_cone_mirror_is_selected_by_plane_endpoint_witness() {
        let current = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(1.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                0.0,
                1.0,
                std::f64::consts::FRAC_PI_4,
            )
            .expect("valid ConeSurface fixture"),
        ));
        let mirrored = mirrored_support_apex_cone(&current).expect("support cone mirror");
        let plane = PlaneEquation {
            origin: [-0.5, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        };
        let perpendicular_plane = PlaneEquation {
            origin: [0.0, 0.0, 0.5],
            normal: [0.0, 0.0, 1.0],
        };
        let endpoints = [[0.0, 0.5], [std::f64::consts::PI, 0.5]];

        assert!(!({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            support_cone_witness_matches(&ctx, &current, endpoints, plane)
        })
        .expect("evaluation resources"));
        assert!(({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            support_cone_witness_matches(&ctx, &mirrored, endpoints, plane)
        })
        .expect("evaluation resources"));
        let perpendicular_endpoints = [[std::f64::consts::PI, 0.5]; 2];
        assert!(({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            support_cone_witness_matches(
                &ctx,
                &current,
                perpendicular_endpoints,
                perpendicular_plane,
            )
        })
        .expect("evaluation resources"));
        assert!(({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            support_cone_witness_matches(
                &ctx,
                &mirrored,
                perpendicular_endpoints,
                perpendicular_plane,
            )
        })
        .expect("evaluation resources"));
    }

    #[test]
    fn a_zero_half_angle_apex_cone_is_not_a_support_apex_cone() {
        let degenerate = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(1.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                0.0,
                1.0,
                0.0,
            )
            .expect("valid ConeSurface fixture"),
        ));

        assert!(mirrored_support_apex_cone(&degenerate).is_none());
    }

    #[test]
    fn an_apex_cone_mirrors_on_every_frame_the_ir_admits() {
        // `OrthonormalFrame3` admits a unit length and an orthogonality within 1e-9. This axis
        // is 5e-10 long of unit and this reference is 5e-10 out of square with it, so both sit
        // inside that admission and outside the 1e-10 the module applies to native record
        // directions.
        let slack = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(1.0, 0.0, 0.0),
                Vector3::new(-1.0 - 5.0e-10, 0.0, 0.0),
                Vector3::new(5.0e-10, 0.0, -1.0 - 5.0e-10),
                0.0,
                1.0,
                std::f64::consts::FRAC_PI_4,
            )
            .expect("valid ConeSurface fixture"),
        ));
        let expected = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(-1.0, 0.0, 0.0),
                Vector3::new(1.0 + 5.0e-10, 0.0, 0.0),
                Vector3::new(5.0e-10, 0.0, -1.0 - 5.0e-10),
                0.0,
                1.0,
                std::f64::consts::FRAC_PI_4,
            )
            .expect("valid ConeSurface fixture"),
        ));

        assert_eq!(mirrored_support_apex_cone(&slack), Some(expected));
    }

    /// A plane whose origin is the largest finite x coordinate: the pcurve
    /// point u = MAX lifts to a point without a finite x, and u = -MAX lifts
    /// onto the plane x = 0.
    fn overflowing_plane_surface(face_id: u32) -> Surface {
        Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{face_id}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        }
    }

    fn unit_plane_surface(face_id: u32) -> Surface {
        Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{face_id}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        }
    }

    #[test]
    fn a_two_chart_path_with_an_overflowing_sample_is_mapped_and_mismatched() {
        let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
        let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &evaluation_arena,
            &evaluation_policy,
        )
        .expect("evaluation root");

        let scan = crate::test_support::empty_container_scan();
        let mut ir = CadIr::empty();
        ir.model
            .surfaces
            .extend([overflowing_plane_surface(7), unit_plane_surface(8)]);
        let pcurve = crate::curve::TwoChartPcurveSamples {
            curve_id: 9,
            faces: [7, 8],
            samples: vec![
                [[f64::MAX, 0.0], [0.0, 0.0]],
                [[-f64::MAX, 1.0], [0.0, 1.0]],
            ],
            offset: 0,
        };
        assert!(matches!(
            map_two_chart_endpoint_sets(
                &evaluation_ctx,
                &scan,
                &ir,
                &pcurve,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &SurfaceIndex::new(
                    &cadmpeg_test_support::service_decode_context(),
                    &ir.model.surfaces
                )
                .expect("surface index fixture"),
            )
            .expect("evaluation resources"),
            TwoChartMapping::Mapped {
                endpoint_sets: Some(TwoChartEndpointSets::Both(_)),
                missing_surface_paths: 0,
                unevaluable_paths: 0,
                surface_mismatch: true,
            }
        ));
    }

    #[test]
    fn a_pcurve_path_with_an_overflowing_endpoint_is_mapped() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(overflowing_plane_surface(7));
        let mapped = crate::decode::with_test_decode_ctx(|ctx| {
            super::map_pcurve_paths(
                ctx,
                &ir,
                [(
                    std::num::NonZeroU32::new(7),
                    [[f64::MAX, 0.0], [-f64::MAX, 5.0]],
                )],
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &SurfaceIndex::new(
                    &cadmpeg_test_support::service_decode_context(),
                    &ir.model.surfaces,
                )
                .expect("surface index fixture"),
            )
        })
        .expect("service pcurve paths");
        assert_eq!(mapped.unevaluable_paths, 0);
        assert_eq!(mapped.mapped.len(), 1);
        assert!(mapped.mapped[0].endpoints[0][0].is_nan());
        assert_eq!(mapped.mapped[0].endpoints[1], [0.0, 5.0, 0.0]);
    }

    #[test]
    fn a_pcurve_path_with_an_overflowing_endpoint_withholds_the_curve_carrier() {
        // Face 11's path lifts one endpoint to a point without a finite x. It
        // is a mapped path that no endpoint evidence agrees with, and an
        // evaluable path without a line carrier, so the curve keeps no carrier.
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 7,
                type_byte: 0,
                feature_id: 0,
                directions: [0x01, 0xf6],
                faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
                next_edges: [7, 7],
                offset: 0,
            });
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [10, 11].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            face_1_endpoints: [[f64::MAX, 2.0], [-f64::MAX, 4.0]],
            offset: 0,
        });
        scan.topology.loops.push(crate::test_support::closed_loop(
            std::num::NonZeroU32::new(10),
            vec![crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            }],
        ));
        let mut ir = CadIr::empty();
        ir.model
            .surfaces
            .extend([unit_plane_surface(10), overflowing_plane_surface(11)]);
        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_diagnostics(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(diagnostics.mapped_paths, 2);
        assert_eq!(diagnostics.unevaluable_paths, 0);
        assert!(!evidence.contains_key(&7));
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert!(transferred.is_empty(), "{transferred:?}");
        assert!(ir.model.curves.is_empty());

        let mut finite = CadIr::empty();
        finite
            .model
            .surfaces
            .extend([unit_plane_surface(10), unit_plane_surface(11)]);
        scan.curves.pcurves[0].face_1_endpoints = [[1.0, 2.0], [3.0, 4.0]];
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut finite,
                &mut annotations,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert_eq!(
            transferred,
            BTreeSet::from([
                CurveId::mint("creo:visibgeom:curve#7".to_string()).expect("identity grammar")
            ])
        );
    }

    #[test]
    fn a_native_pcurve_with_an_overflowing_endpoint_has_no_midpoint_and_no_orientation() {
        let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
        let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &evaluation_arena,
            &evaluation_policy,
        )
        .expect("evaluation root");

        // The endpoint u = MAX reaches x = +inf on the plane through
        // (MAX, 0, 0). A point outside the finite range aligns with no edge
        // point, so neither pairing aligns.
        let surface = overflowing_plane_surface(7).geometry;
        let endpoints = [[f64::MAX, 0.0], [-f64::MAX, 5.0]];
        assert_eq!(
            super::native_pcurve_midpoint(
                &evaluation_ctx,
                &surface,
                endpoints,
                [[9.0, 9.0, 9.0], [0.0, 5.0, 0.0]]
            )
            .expect("evaluation resources"),
            None
        );
        assert_eq!(
            super::oriented_native_pcurve_endpoints(
                &evaluation_ctx,
                &surface,
                endpoints,
                [[9.0, 9.0, 9.0], [0.0, 5.0, 0.0]],
            )
            .expect("evaluation resources"),
            None
        );
    }

    /// A plane through the origin placed by a transform that adds the largest
    /// finite x coordinate: the pcurve point u = MAX lifts to a point without
    /// a finite x, and u = -MAX lifts onto the plane x = 0.
    fn overflowing_placed_plane_surface(face_id: u32) -> Surface {
        let SurfaceGeometry::Solved(plane) = unit_plane_surface(face_id).geometry else {
            panic!("the plane fixture is solved");
        };
        Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{face_id}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(plane),
                    cadmpeg_ir::transform::Transform::affine([
                        [1.0, 0.0, 0.0, f64::MAX],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                    ])
                    .expect("affine transform"),
                )
                .expect("valid PlacedSurface fixture"),
            )),
            source_object: None,
        }
    }

    #[test]
    fn a_two_chart_path_with_an_overflowing_placed_sample_is_a_surface_mismatch() {
        let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
        let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &evaluation_arena,
            &evaluation_policy,
        )
        .expect("evaluation root");

        // The placed sample reaches x = +inf. It is a mapped sample on both
        // charts, and a point outside the finite range agrees with no point,
        // so the charts do not agree.
        let scan = crate::test_support::empty_container_scan();
        let mut ir = CadIr::empty();
        ir.model
            .surfaces
            .extend([overflowing_placed_plane_surface(7), unit_plane_surface(8)]);
        let pcurve = crate::curve::TwoChartPcurveSamples {
            curve_id: 9,
            faces: [7, 8],
            samples: vec![
                [[f64::MAX, 0.0], [0.0, 0.0]],
                [[-f64::MAX, 1.0], [0.0, 1.0]],
            ],
            offset: 0,
        };
        assert!(matches!(
            map_two_chart_endpoint_sets(
                &evaluation_ctx,
                &scan,
                &ir,
                &pcurve,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &SurfaceIndex::new(
                    &cadmpeg_test_support::service_decode_context(),
                    &ir.model.surfaces
                )
                .expect("surface index fixture"),
            )
            .expect("evaluation resources"),
            TwoChartMapping::Mapped {
                endpoint_sets: Some(TwoChartEndpointSets::Both(_)),
                missing_surface_paths: 0,
                unevaluable_paths: 0,
                surface_mismatch: true,
            }
        ));
    }

    #[test]
    fn a_pcurve_path_with_an_overflowing_placed_endpoint_is_mapped() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(overflowing_placed_plane_surface(7));
        let mapped = crate::decode::with_test_decode_ctx(|ctx| {
            super::map_pcurve_paths(
                ctx,
                &ir,
                [(
                    std::num::NonZeroU32::new(7),
                    [[f64::MAX, 0.0], [-f64::MAX, 5.0]],
                )],
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &SurfaceIndex::new(
                    &cadmpeg_test_support::service_decode_context(),
                    &ir.model.surfaces,
                )
                .expect("surface index fixture"),
            )
        })
        .expect("service pcurve paths");
        assert_eq!(mapped.unevaluable_paths, 0);
        assert_eq!(mapped.mapped.len(), 1);
        assert_eq!(mapped.mapped[0].endpoints[0], [f64::INFINITY, 0.0, 0.0]);
        assert_eq!(mapped.mapped[0].endpoints[1], [0.0, 5.0, 0.0]);
    }

    #[test]
    fn a_pcurve_path_with_an_overflowing_placed_endpoint_withholds_the_curve_carrier() {
        // Face 11's path lifts one endpoint to a point without a finite x. It
        // is a mapped path that no endpoint evidence agrees with, and an
        // evaluable path without a line carrier, so the curve keeps no carrier.
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 7,
                type_byte: 0,
                feature_id: 0,
                directions: [0x01, 0xf6],
                faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
                next_edges: [7, 7],
                offset: 0,
            });
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [10, 11].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            face_1_endpoints: [[f64::MAX, 2.0], [-f64::MAX, 4.0]],
            offset: 0,
        });
        scan.topology.loops.push(crate::test_support::closed_loop(
            std::num::NonZeroU32::new(10),
            vec![crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            }],
        ));
        let mut ir = CadIr::empty();
        ir.model
            .surfaces
            .extend([unit_plane_surface(10), overflowing_placed_plane_surface(11)]);
        let (evidence, diagnostics) = crate::decode::with_test_decode_ctx(|ctx| {
            pcurve_edge_endpoint_evidence_with_diagnostics(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service pcurve evidence");
        assert_eq!(diagnostics.mapped_paths, 2);
        assert_eq!(diagnostics.unevaluable_paths, 0);
        assert!(!evidence.contains_key(&7));
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut annotations,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert!(transferred.is_empty(), "{transferred:?}");
        assert!(ir.model.curves.is_empty());
    }

    #[test]
    fn a_native_pcurve_with_an_overflowing_placed_endpoint_has_no_midpoint_and_no_orientation() {
        let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
        let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &evaluation_arena,
            &evaluation_policy,
        )
        .expect("evaluation root");

        // The placed endpoint reaches x = +inf. A point outside the finite
        // range aligns with no edge point, so neither pairing aligns: no
        // midpoint is read, and no orientation is found.
        let surface = overflowing_placed_plane_surface(7).geometry;
        let endpoints = [[f64::MAX, 0.0], [-f64::MAX, 5.0]];
        assert_eq!(
            super::native_pcurve_midpoint(
                &evaluation_ctx,
                &surface,
                endpoints,
                [[9.0, 9.0, 9.0], [0.0, 5.0, 0.0]]
            )
            .expect("evaluation resources"),
            None
        );
        assert_eq!(
            super::oriented_native_pcurve_endpoints(
                &evaluation_ctx,
                &surface,
                endpoints,
                [[9.0, 9.0, 9.0], [0.0, 5.0, 0.0]],
            )
            .expect("evaluation resources"),
            None
        );
    }
}

#[cfg(test)]
mod evaluation_tests;
