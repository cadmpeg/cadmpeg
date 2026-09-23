// SPDX-License-Identifier: Apache-2.0
//! Geometry records owned by the legacy ASCII persistence object graph.

use crate::legacy::value_index;
use cadmpeg_core::decode::index_from_u32;
use std::collections::BTreeMap;

use crate::curve::{CurveTopologyRow, PcurveEndpoints};
use crate::legacy::{self, NumericPayload, ObjectPayload, ObjectRecord, Persistence, RealRecord};
use crate::surface::{self, SurfaceKind, SurfaceRow};

/// Acceptance gate on the handedness of a legacy `local_sys` matrix.
const EPS_LOCAL_SYSTEM_HANDEDNESS: f64 = 1.0e-9;

/// A complete model-space carrier from one legacy analytic surface prototype.
///
/// Every stored coordinate and dimension is finite: [`crate::legacy::Real`] is
/// admitted from its stored IEEE-754 bits only when those bits state a finite
/// value, and a legacy record holding one non-finite real is withheld whole.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LegacySurfaceGeometry {
    /// A plane from a complete row-major local system.
    ///
    /// The frame origin is the plane origin, the frame axis is the surface normal and
    /// the frame ref direction is the parameter-space u axis.
    Plane {
        /// Origin with the normal and u-axis pair.
        frame: surface::PositionalFrame,
    },
    /// A cylinder from a complete row-major local system and radius.
    Cylinder {
        /// A point on the cylinder axis with the axis and ref direction.
        frame: surface::PositionalFrame,
        /// Positive cylinder radius in the stored model coordinate system.
        radius: f64,
    },
    /// A circular cone from a complete legacy local system and signed angle.
    Cone {
        /// The apex with the axis directed toward increasing radius.
        frame: surface::PositionalFrame,
        /// Cone half-angle in radians, admitted by
        /// [`crate::surface::valid_apex_cone_half_angle`].
        half_angle: f64,
        /// Sign that maps the source `v` parameter to this positive-angle frame.
        parameter_v_sign: f64,
    },
    /// A torus from a complete legacy local system and two radii.
    Torus {
        /// The torus center with the axis and ref direction.
        frame: surface::PositionalFrame,
        /// Torus major radius.
        major_radius: f64,
        /// Torus minor radius.
        minor_radius: f64,
    },
    /// A sphere represented by the torus family with a zero major radius.
    Sphere {
        /// The sphere center with the stored local-system directions, which the
        /// sphere keeps for parameter provenance.
        frame: surface::PositionalFrame,
        /// Sphere radius.
        radius: f64,
    },
    /// A complete bicubic interpolation surface carrier.
    Spline(crate::interpolation_grid::InterpolationGrid),
}

/// The legacy namespace that owns one analytic surface carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacySurfaceNamespace {
    /// The active model geometry namespace.
    Visible,
    /// The inactive or construction geometry namespace.
    NonVisible,
}

impl LegacySurfaceNamespace {
    pub(crate) const fn source_prefix(self) -> &'static str {
        match self {
            Self::Visible => "VisibGeom:",
            Self::NonVisible => "NovisGeom:",
        }
    }

    pub(crate) const fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
    }
}

/// One complete legacy surface carrier associated with a namespace surface row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LegacySurfaceCarrier {
    /// The namespace that owns the `srf_array` surface identifier.
    pub(crate) namespace: LegacySurfaceNamespace,
    /// Native `srf_array` surface identifier.
    pub(crate) surface_id: u32,
    /// Complete surface geometry or interpolation data.
    pub(crate) geometry: LegacySurfaceGeometry,
    /// Byte offset of the `srf_prim_ptr` object.
    pub(crate) offset: usize,
}

/// Legacy geometry rows and complete analytic carriers from both namespaces.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LegacyGeometryScan {
    /// Rows under `Sld_VisGeom.active_geom.srf_array`.
    pub(crate) rows: Vec<SurfaceRow>,
    /// Rows under `Sld_NonVisGeom.inactive_geom.srf_array`.
    pub(crate) nonvisible_rows: Vec<SurfaceRow>,
    /// Complete surface carriers from both surface namespaces.
    pub(crate) carriers: Vec<LegacySurfaceCarrier>,
    /// Complete visible curve topology rows from the legacy `crv_array`
    /// namespace.
    pub(crate) topology_rows: Vec<CurveTopologyRow>,
    /// Complete endpoint witnesses from legacy `crv_pnt_arr` samples.
    pub(crate) pcurves: Vec<PcurveEndpoints>,
}

type ObjectIdIndex<'a> = BTreeMap<String, &'a ObjectRecord>;
type ChildIndex<'a> = BTreeMap<usize, Vec<&'a ObjectRecord>>;
type IntegerFieldIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealFieldIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a RealRecord>>;

/// Decode the surface portions of one legacy persistence object graph.
pub(crate) fn scan(persistence: &Persistence) -> LegacyGeometryScan {
    let object_ids = object_id_index(&persistence.objects);
    let children = child_index(&persistence.objects);
    let integer_fields = value_index(&persistence.integer_values.rows);
    let real_fields = value_index(&persistence.real_values.rows);
    let (rows, mut carriers) = namespace(
        &persistence.objects,
        &object_ids,
        &children,
        &integer_fields,
        &real_fields,
        "Sld_VisGeom",
        "active_geom",
        LegacySurfaceNamespace::Visible,
    );
    let (nonvisible_rows, mut nonvisible_carriers) = namespace(
        &persistence.objects,
        &object_ids,
        &children,
        &integer_fields,
        &real_fields,
        "Sld_NonVisGeom",
        "inactive_geom",
        LegacySurfaceNamespace::NonVisible,
    );
    carriers.append(&mut nonvisible_carriers);
    carriers.sort_by_key(|carrier| carrier.offset);
    let (topology_rows, pcurves) = curve_namespace(
        &persistence.objects,
        &object_ids,
        &integer_fields,
        &real_fields,
    );
    LegacyGeometryScan {
        rows,
        nonvisible_rows,
        carriers,
        topology_rows,
        pcurves,
    }
}

fn curve_namespace(
    objects: &[ObjectRecord],
    object_ids: &ObjectIdIndex<'_>,
    integer_fields: &IntegerFieldIndex<'_>,
    real_fields: &RealFieldIndex<'_>,
) -> (Vec<CurveTopologyRow>, Vec<PcurveEndpoints>) {
    let Some(elements) = geometry_array_elements(
        objects,
        object_ids,
        "Sld_VisGeom",
        "active_geom",
        "crv_array",
    ) else {
        return (Vec::new(), Vec::new());
    };
    let mut topology_rows = Vec::new();
    let mut pcurves = Vec::new();
    for curve_object in elements {
        let Some(row) = curve_topology_row(curve_object, integer_fields) else {
            continue;
        };
        if let Some(pcurve) = curve_pcurve(curve_object, &row, real_fields) {
            pcurves.push(pcurve);
        }
        topology_rows.push(row);
    }
    topology_rows.sort_by_key(|row| row.offset);
    topology_rows.dedup_by_key(|row| row.offset);
    pcurves.sort_by_key(|pcurve| pcurve.offset);
    pcurves.dedup_by_key(|pcurve| pcurve.offset);
    (topology_rows, pcurves)
}

fn geometry_array_elements<'a>(
    objects: &'a [ObjectRecord],
    object_ids: &ObjectIdIndex<'a>,
    root_name: &str,
    branch_name: &str,
    array_name: &str,
) -> Option<Vec<&'a ObjectRecord>> {
    let mut roots = objects
        .iter()
        .filter(|object| object.name == root_name && object.parent.is_none());
    let root = roots.next()?;
    roots.next().is_none().then_some(())?;

    let mut branches = objects
        .iter()
        .filter(|object| object.parent == Some(root.offset) && object.name == branch_name);
    let branch = branches.next()?;
    branches.next().is_none().then_some(())?;

    let mut arrays = objects.iter().filter_map(|object| {
        let ObjectPayload::Array { elements, .. } = &object.payload else {
            return None;
        };
        (object.parent == Some(branch.offset)
            && object.name == array_name
            && object.payload.is_complete())
        .then_some((object, elements))
    });
    let (array, elements) = arrays.next()?;
    arrays.next().is_none().then_some(())?;

    elements
        .iter()
        .map(|element_id| {
            let element = object_ids.get(element_id.as_str()).copied()?;
            (element.parent == Some(array.offset) && element.name == array_name).then_some(())?;
            Some(element)
        })
        .collect()
}

fn curve_topology_row(
    curve_object: &ObjectRecord,
    integers: &IntegerFieldIndex<'_>,
) -> Option<CurveTopologyRow> {
    let id = u32::try_from(integer_field(integers, curve_object.offset, "crv_id")?).ok()?;
    let type_byte = u8::try_from(integer_field(integers, curve_object.offset, "type")?).ok()?;
    let feature_id =
        u32::try_from(integer_field(integers, curve_object.offset, "feat_id")?).ok()?;
    let directions = integer_array(integers, curve_object.offset, "crv_pnt_dir")?
        .into_iter()
        .map(legacy_direction)
        .collect::<Option<Vec<_>>>()?;
    let [first_direction, second_direction] = directions.as_slice() else {
        return None;
    };
    let faces = [
        u32::try_from(integer_field(
            integers,
            curve_object.offset,
            "crv_hdr_geom_ptr[0]",
        )?)
        .ok()?,
        u32::try_from(integer_field(
            integers,
            curve_object.offset,
            "crv_hdr_geom_ptr[1]",
        )?)
        .ok()?,
    ];
    let next_edges = [
        u32::try_from(integer_field(
            integers,
            curve_object.offset,
            "next_crv_hdr_ptr[0]",
        )?)
        .ok()?,
        u32::try_from(integer_field(
            integers,
            curve_object.offset,
            "next_crv_hdr_ptr[1]",
        )?)
        .ok()?,
    ];
    Some(CurveTopologyRow {
        id,
        type_byte,
        feature_id,
        directions: [*first_direction, *second_direction],
        faces: faces.map(std::num::NonZeroU32::new),
        next_edges,
        offset: integer_record(integers, curve_object.offset, "crv_id")?.offset,
    })
}

fn curve_pcurve(
    curve_object: &ObjectRecord,
    topology: &CurveTopologyRow,
    reals: &RealFieldIndex<'_>,
) -> Option<PcurveEndpoints> {
    let record = real_record(reals, curve_object.offset, "crv_pnt_arr")?;
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    let [sample_count, lane_width] = array.dimensions() else {
        return None;
    };
    if *lane_width != 4 || *sample_count < 2 {
        return None;
    }
    // One element per declared element, so the four-element window at each end
    // of the expansion is the first and the last of the `sample_count` samples.
    let values = real_array_values(record)?;
    let first = *values.first_chunk::<4>()?;
    let last = *values.last_chunk::<4>()?;
    Some(PcurveEndpoints {
        curve_id: topology.id,
        faces: topology.faces,
        face_0_endpoints: [[first[0], first[1]], [last[0], last[1]]],
        face_1_endpoints: [[first[2], first[3]], [last[2], last[3]]],
        offset: record.offset,
    })
}

fn legacy_direction(value: i32) -> Option<u8> {
    match value {
        1 => Some(0x01),
        -1 => Some(0xf6),
        _ => None,
    }
}

#[expect(clippy::too_many_arguments)]
fn namespace(
    objects: &[ObjectRecord],
    object_ids: &ObjectIdIndex<'_>,
    children: &ChildIndex<'_>,
    integer_fields: &IntegerFieldIndex<'_>,
    real_fields: &RealFieldIndex<'_>,
    root_name: &str,
    branch_name: &str,
    namespace: LegacySurfaceNamespace,
) -> (Vec<SurfaceRow>, Vec<LegacySurfaceCarrier>) {
    let Some(elements) =
        geometry_array_elements(objects, object_ids, root_name, branch_name, "srf_array")
    else {
        return (Vec::new(), Vec::new());
    };

    let mut rows = Vec::new();
    let mut carriers = Vec::new();
    for row_object in elements {
        let Some(row) = surface_row(row_object, integer_fields) else {
            continue;
        };
        if let Some(carrier) = surface_carrier(row_object, &row, children, real_fields, namespace) {
            carriers.push(carrier);
        }
        rows.push(row);
    }
    rows.sort_by_key(|row| row.offset);
    carriers.sort_by_key(|carrier| carrier.offset);
    (rows, carriers)
}

fn surface_row(row_object: &ObjectRecord, integers: &IntegerFieldIndex<'_>) -> Option<SurfaceRow> {
    let type_byte = u8::try_from(integer_field(integers, row_object.offset, "geom_type")?).ok()?;
    let kind = SurfaceKind::from_byte(type_byte)?;
    let feature_id = u32::try_from(integer_field(integers, row_object.offset, "feat_id")?).ok()?;
    let id = u32::try_from(integer_field(integers, row_object.offset, "geom_id")?).ok()?;
    let boundary_type =
        u8::try_from(integer_field(integers, row_object.offset, "boundary_type")?).ok()?;
    let boundary_type = surface::BoundaryType::from_byte(boundary_type)?;
    let orientation = integer_field(integers, row_object.offset, "orient")?;
    let reversed = match orientation {
        1 => false,
        -1 => true,
        _ => return None,
    };
    let next_surface =
        u32::try_from(integer_field(integers, row_object.offset, "next_geom_ptr")?).ok()?;
    Some(SurfaceRow {
        id,
        kind,
        feature_id,
        reversed,
        boundary_type,
        next_surface,
        offset: integer_record(integers, row_object.offset, "geom_id")?.offset,
    })
}

/// Whether the three columns of a `local_sys` matrix are a right-handed orthonormal basis.
///
/// A carrier keeps column two as its axis and column zero as its ref direction. Column one is
/// read only here, where it proves the stored matrix is the right-handed completion of that
/// pair; a record whose middle column disagrees states no carrier. Each column pair is admitted
/// by the IR orthonormal-frame measurement, the one the carrier's frame holds. `handedness` is
/// `NaN` or `+-inf` only for inputs the three orthonormality conjuncts have already refused.
fn valid_right_handed_local_system(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> bool {
    let cross = [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ];
    let handedness = cross
        .into_iter()
        .zip(third)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let orthonormal = |axis: [f64; 3], reference: [f64; 3]| {
        cadmpeg_ir::units::OrthonormalFrame3::new(
            cadmpeg_ir::math::Vector3::from(axis),
            cadmpeg_ir::math::Vector3::from(reference),
        )
        .is_some()
    };
    orthonormal(third, first)
        && orthonormal(third, second)
        && orthonormal(first, second)
        && (handedness - 1.0).abs() <= EPS_LOCAL_SYSTEM_HANDEDNESS
}

fn surface_carrier(
    row_object: &ObjectRecord,
    row: &SurfaceRow,
    children: &ChildIndex<'_>,
    reals: &RealFieldIndex<'_>,
    namespace: LegacySurfaceNamespace,
) -> Option<LegacySurfaceCarrier> {
    enum AnalyticFamily {
        Plane,
        Cylinder,
        Cone,
        TorusOrSphere,
    }

    let mut primitives = children
        .get(&row_object.offset)?
        .iter()
        .copied()
        .filter(|object| object.name.starts_with("srf_prim_ptr("));
    let primitive = primitives.next()?;
    primitives.next().is_none().then_some(())?;
    let (family, expected_name) = match row.kind {
        SurfaceKind::Plane => (AnalyticFamily::Plane, "srf_prim_ptr(plane)"),
        SurfaceKind::Cylinder => (AnalyticFamily::Cylinder, "srf_prim_ptr(cylinder)"),
        SurfaceKind::Cone => (AnalyticFamily::Cone, "srf_prim_ptr(cone)"),
        SurfaceKind::TorusOrSphere => (AnalyticFamily::TorusOrSphere, "srf_prim_ptr(torus)"),
        SurfaceKind::Spline => {
            (primitive.name == "srf_prim_ptr(splsrf)").then_some(())?;
            let points = real_vector_array(reals, primitive.offset, "i_points")?;
            let u_parameters = real_scalar_array(reals, primitive.offset, "u_params")?;
            let v_parameters = real_scalar_array(reals, primitive.offset, "v_params")?;
            let u_tangents = real_vector_array(reals, primitive.offset, "u_tangts")?;
            let v_tangents = real_vector_array(reals, primitive.offset, "v_tangts")?;
            let mixed_derivatives = real_vector_array(reals, primitive.offset, "uv_deriv")?;
            let spline = crate::interpolation_grid::InterpolationGrid::from_full_tangent_grid(
                points,
                u_parameters,
                v_parameters,
                &u_tangents,
                &v_tangents,
                &mixed_derivatives,
            )?;
            return Some(LegacySurfaceCarrier {
                namespace,
                surface_id: row.id,
                geometry: LegacySurfaceGeometry::Spline(spline),
                offset: primitive.offset,
            });
        }
        SurfaceKind::Fillet | SurfaceKind::Extrusion(_) => return None,
    };
    (primitive.name == expected_name).then_some(())?;

    let local_system = real_record(reals, primitive.offset, "local_sys")?;
    let slots = local_system_slots(local_system)?;
    let first = [slots[0], slots[3], slots[6]];
    let second = [slots[1], slots[4], slots[7]];
    let third = [slots[2], slots[5], slots[8]];
    valid_right_handed_local_system(first, second, third).then_some(())?;
    let origin = [slots[9], slots[10], slots[11]];
    // The matrix admission above states the finite origin and the unit-length orthogonal
    // pair the frame holds, so this construction is the one the carrier keeps.
    let frame = surface::PositionalFrame::new(origin, third, first)?;
    let geometry = match family {
        AnalyticFamily::Plane => LegacySurfaceGeometry::Plane { frame },
        AnalyticFamily::Cylinder => LegacySurfaceGeometry::Cylinder {
            frame,
            radius: real_scalar(reals, primitive.offset, "radius")
                .filter(|radius| *radius > 0.0)?,
        },
        AnalyticFamily::Cone => {
            // The legacy record signs the half angle: the magnitude is the half angle and the
            // sign gives the axis direction. The magnitude is the apex cone half angle that
            // `surface::valid_apex_cone_half_angle` owns, because
            // `decode::surfaces::prototypes` builds the same `radius = 0.0`, `ratio = 1.0`
            // cone from this carrier as the positional cone rows do.
            let signed_half_angle = real_scalar(reals, primitive.offset, "half_angle")?;
            let half_angle = signed_half_angle.abs();
            surface::valid_apex_cone_half_angle(half_angle).then_some(())?;
            LegacySurfaceGeometry::Cone {
                frame: if signed_half_angle.is_sign_positive() {
                    frame
                } else {
                    frame.with_reversed_axis()
                },
                half_angle,
                parameter_v_sign: signed_half_angle.signum(),
            }
        }
        AnalyticFamily::TorusOrSphere => {
            let major_radius =
                real_scalar(reals, primitive.offset, "radius1").filter(|radius| *radius >= 0.0)?;
            let minor_radius =
                real_scalar(reals, primitive.offset, "radius2").filter(|radius| *radius > 0.0)?;
            if major_radius == 0.0 {
                LegacySurfaceGeometry::Sphere {
                    frame,
                    radius: minor_radius,
                }
            } else {
                LegacySurfaceGeometry::Torus {
                    frame,
                    major_radius,
                    minor_radius,
                }
            }
        }
    };
    Some(LegacySurfaceCarrier {
        namespace,
        surface_id: row.id,
        geometry,
        offset: primitive.offset,
    })
}

/// Map legacy pcurve `v` coordinates into the positive-angle frame emitted by
/// [`LegacySurfaceGeometry::Cone`].
pub(crate) fn canonicalize_legacy_cone_pcurve_endpoints(
    carriers: &[LegacySurfaceCarrier],
    face_id: u32,
    endpoints: [[f64; 2]; 2],
) -> [[f64; 2]; 2] {
    let sign = carriers
        .iter()
        .find_map(|carrier| {
            (carrier.surface_id == face_id).then_some(match carrier.geometry {
                LegacySurfaceGeometry::Cone {
                    parameter_v_sign, ..
                } => parameter_v_sign,
                _ => 1.0,
            })
        })
        .unwrap_or(1.0);
    endpoints.map(|[u, v]| [u, v * sign])
}

fn real_vector_array(
    records: &RealFieldIndex<'_>,
    parent: usize,
    name: &str,
) -> Option<Vec<[f64; 3]>> {
    let record = real_record(records, parent, name)?;
    let values = real_array_values(record)?;
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    let dimensions = array.dimensions();
    let [_, width] = dimensions else {
        return None;
    };
    (*width == 3).then_some(())?;
    Some(values.as_chunks::<3>().0.to_vec())
}

fn real_scalar_array(records: &RealFieldIndex<'_>, parent: usize, name: &str) -> Option<Vec<f64>> {
    let record = real_record(records, parent, name)?;
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    let dimensions = array.dimensions();
    (dimensions.len() == 1).then_some(())?;
    real_array_values(record)
}

/// Expand one real array's runs into its elements, in element order.
///
/// The array states a run-count sum equal to its extent product, so the result
/// holds one element per declared array element.
fn real_array_values(record: &RealRecord) -> Option<Vec<f64>> {
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    Some(
        array
            .runs()
            .iter()
            .flat_map(|run| std::iter::repeat_n(run.value.value(), index_from_u32(run.count)))
            .collect(),
    )
}

fn object_id_index(objects: &[ObjectRecord]) -> ObjectIdIndex<'_> {
    objects.iter().map(|object| (object.id(), object)).collect()
}

fn child_index(objects: &[ObjectRecord]) -> ChildIndex<'_> {
    let mut index = BTreeMap::new();
    for object in objects {
        if let Some(parent) = object.parent {
            index.entry(parent).or_insert_with(Vec::new).push(object);
        }
    }
    index
}

fn integer_record<'a>(
    records: &'a IntegerFieldIndex<'a>,
    parent: usize,
    name: &str,
) -> Option<&'a legacy::IntegerRecord> {
    let matches = records.get(&(parent, name))?;
    (matches.len() == 1).then_some(matches[0])
}

fn integer_field(records: &IntegerFieldIndex<'_>, parent: usize, name: &str) -> Option<i32> {
    let record = integer_record(records, parent, name)?;
    match &record.payload {
        NumericPayload::Scalar { value } => Some(*value),
        NumericPayload::Array(_) => None,
    }
}

/// Expand one integer array's runs into its elements, in element order.
///
/// The array states a run-count sum equal to its extent product, so the result
/// holds one element per declared array element.
fn integer_array(records: &IntegerFieldIndex<'_>, parent: usize, name: &str) -> Option<Vec<i32>> {
    let record = integer_record(records, parent, name)?;
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    Some(
        array
            .runs()
            .iter()
            .flat_map(|run| std::iter::repeat_n(run.value, index_from_u32(run.count)))
            .collect(),
    )
}

fn real_record<'a>(
    records: &'a RealFieldIndex<'a>,
    parent: usize,
    name: &str,
) -> Option<&'a RealRecord> {
    let matches = records.get(&(parent, name))?;
    (matches.len() == 1).then_some(matches[0])
}

fn real_scalar(records: &RealFieldIndex<'_>, parent: usize, name: &str) -> Option<f64> {
    let record = real_record(records, parent, name)?;
    match &record.payload {
        NumericPayload::Scalar { value } => Some(value.value()),
        NumericPayload::Array(_) => None,
    }
}

/// The twelve row-major slots of a `[4][3]` local-system real array.
fn local_system_slots(record: &RealRecord) -> Option<[f64; 12]> {
    let NumericPayload::Array(array) = &record.payload else {
        return None;
    };
    (array.dimensions() == [4, 3]).then_some(())?;
    real_array_values(record)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_legacy_cone_pcurve_endpoints, scan, LegacySurfaceCarrier,
        LegacySurfaceGeometry, LegacySurfaceNamespace,
    };
    use crate::legacy::{
        IntegerPayload, IntegerRun, ObjectPayload, Persistence, Real, RealPayload, RealRecord,
        RealRun, ValueRecord,
    };
    use crate::test_support::{fixture_offset, object};

    fn real(value: f64) -> String {
        format!("{:016X}", value.to_bits())
    }

    /// The frame a carrier holds, from the origin, axis and ref direction a test states.
    fn frame(
        origin: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
    ) -> crate::surface::PositionalFrame {
        let Some(frame) = crate::surface::PositionalFrame::new(origin, axis, ref_direction) else {
            panic!("the test states a finite origin and an orthonormal direction pair");
        };
        frame
    }

    fn fixture(radius: f64, conflicting: bool) -> Vec<u8> {
        let mut data = format!(
            r"#UGC:2 PART 1
#-END_OF_UGC_HEADER
#P_OBJECT 6
@Sld_VisGeom 1 0
@active_geom 2 0
@srf_array 3 0
@geom_type 4 1
@geom_id 5 1
@feat_id 6 1
@boundary_type 7 1
@next_geom_ptr 8 1
@orient 9 1
@srf_prim_ptr(cylinder) 10 0
@local_sys 11 2
@radius 12 2
@principal_sys_units 13 10
0 13 millimeter Newton Second (mmNs)
0 1 ->
1 2 ->
2 3 [1]
3 3 ->
4 4 36
4 5 42
4 6 7
4 7 0
4 8 0
4 9 1
4 10 ->
5 11 [4][3]
$3FF,0,0,0,3FF,0,0,0,3FF,0,0,0
5 12 {}
",
            real(radius)
        );
        if conflicting {
            data.push_str("5 12 ");
            data.push_str(&real(radius + 1.0));
            data.push('\n');
        }
        data.push_str("#END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n");
        data.into_bytes()
    }

    fn spline_real_array(
        parent: &str,
        name: &str,
        dimensions: Vec<u32>,
        values: impl IntoIterator<Item = f64>,
        offset: usize,
    ) -> crate::legacy::RealRecord {
        let runs = values
            .into_iter()
            .map(|value| RealRun {
                count: 1,
                value: Real::from_bits(value.to_bits()),
            })
            .collect();
        ValueRecord {
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: RealPayload::array(dimensions, runs).expect("complete numeric array"),
            offset,
        }
    }

    fn spline_persistence(with_all_fields: bool) -> Persistence {
        let root = "spline_root";
        let branch = "spline_branch";
        let array = "spline_array";
        let row = "spline_row";
        let primitive = "spline_primitive";
        let objects = vec![
            object(root, "Sld_VisGeom", None, ObjectPayload::Arrow),
            object(branch, "active_geom", Some(root), ObjectPayload::Arrow),
            object(
                array,
                "srf_array",
                Some(branch),
                ObjectPayload::Array {
                    dimensions: vec![1],
                    elements: vec![row.to_string()],
                },
            ),
            object(row, "srf_array", Some(array), ObjectPayload::Arrow),
            object(
                primitive,
                "srf_prim_ptr(splsrf)",
                Some(row),
                ObjectPayload::Arrow,
            ),
        ];
        let integer_values = vec![
            integer(row, "geom_type", IntegerPayload::Scalar { value: 40 }, 10),
            integer(row, "geom_id", IntegerPayload::Scalar { value: 42 }, 11),
            integer(row, "feat_id", IntegerPayload::Scalar { value: 7 }, 12),
            integer(
                row,
                "boundary_type",
                IntegerPayload::Scalar { value: 0 },
                13,
            ),
            integer(
                row,
                "next_geom_ptr",
                IntegerPayload::Scalar { value: 0 },
                14,
            ),
            integer(row, "orient", IntegerPayload::Scalar { value: 1 }, 15),
        ];
        let mut real_values = vec![
            spline_real_array(
                primitive,
                "i_points",
                vec![4, 3],
                [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
                20,
            ),
            spline_real_array(primitive, "u_params", vec![2], [0.0, 1.0], 21),
            spline_real_array(primitive, "v_params", vec![2], [0.0, 1.0], 22),
            spline_real_array(
                primitive,
                "u_tangts",
                vec![4, 3],
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                23,
            ),
            spline_real_array(
                primitive,
                "v_tangts",
                vec![4, 3],
                [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
                24,
            ),
        ];
        if with_all_fields {
            real_values.push(spline_real_array(
                primitive,
                "uv_deriv",
                vec![4, 3],
                [0.0; 12],
                25,
            ));
        }
        Persistence {
            real_values: crate::legacy::TypedValues {
                rows: real_values,
                unresolved_count: 0,
            },
            integer_values: crate::legacy::TypedValues {
                rows: integer_values,
                unresolved_count: 0,
            },
            objects,
            ..Persistence::default()
        }
    }

    fn cone_persistence(signed_half_angle: Option<f64>) -> Persistence {
        let root = "cone_root";
        let branch = "cone_branch";
        let array = "cone_array";
        let row = "cone_row";
        let primitive = "cone_primitive";
        let objects = vec![
            object(root, "Sld_VisGeom", None, ObjectPayload::Arrow),
            object(branch, "active_geom", Some(root), ObjectPayload::Arrow),
            object(
                array,
                "srf_array",
                Some(branch),
                ObjectPayload::Array {
                    dimensions: vec![1],
                    elements: vec![row.to_string()],
                },
            ),
            object(row, "srf_array", Some(array), ObjectPayload::Arrow),
            object(
                primitive,
                "srf_prim_ptr(cone)",
                Some(row),
                ObjectPayload::Arrow,
            ),
        ];
        let integer_values = vec![
            integer(row, "geom_type", IntegerPayload::Scalar { value: 37 }, 10),
            integer(row, "geom_id", IntegerPayload::Scalar { value: 42 }, 11),
            integer(row, "feat_id", IntegerPayload::Scalar { value: 7 }, 12),
            integer(
                row,
                "boundary_type",
                IntegerPayload::Scalar { value: 0 },
                13,
            ),
            integer(
                row,
                "next_geom_ptr",
                IntegerPayload::Scalar { value: 0 },
                14,
            ),
            integer(row, "orient", IntegerPayload::Scalar { value: 1 }, 15),
        ];
        let mut real_values = vec![spline_real_array(
            primitive,
            "local_sys",
            vec![4, 3],
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0],
            20,
        )];
        if let Some(signed_half_angle) = signed_half_angle {
            real_values.push(ValueRecord {
                name: "half_angle".to_string(),
                attribute_id: 0,
                scope_offset: 0,
                parent: Some(fixture_offset(primitive)),
                depth: 0,
                payload: RealPayload::Scalar {
                    value: Real::from_bits(signed_half_angle.to_bits()),
                },
                offset: 21,
            });
        }
        Persistence {
            real_values: crate::legacy::TypedValues {
                rows: real_values,
                unresolved_count: 0,
            },
            integer_values: crate::legacy::TypedValues {
                rows: integer_values,
                unresolved_count: 0,
            },
            objects,
            ..Persistence::default()
        }
    }

    fn real_scalar(parent: &str, name: &str, value: f64, offset: usize) -> RealRecord {
        ValueRecord {
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: RealPayload::Scalar {
                value: Real::from_bits(value.to_bits()),
            },
            offset,
        }
    }

    fn torus_persistence(major_radius: f64, minor_radius: f64) -> Persistence {
        let root = "torus_root";
        let branch = "torus_branch";
        let array = "torus_array";
        let row = "torus_row";
        let primitive = "torus_primitive";
        let objects = vec![
            object(root, "Sld_VisGeom", None, ObjectPayload::Arrow),
            object(branch, "active_geom", Some(root), ObjectPayload::Arrow),
            object(
                array,
                "srf_array",
                Some(branch),
                ObjectPayload::Array {
                    dimensions: vec![1],
                    elements: vec![row.to_string()],
                },
            ),
            object(row, "srf_array", Some(array), ObjectPayload::Arrow),
            object(
                primitive,
                "srf_prim_ptr(torus)",
                Some(row),
                ObjectPayload::Arrow,
            ),
        ];
        let integer_values = vec![
            integer(row, "geom_type", IntegerPayload::Scalar { value: 38 }, 10),
            integer(row, "geom_id", IntegerPayload::Scalar { value: 42 }, 11),
            integer(row, "feat_id", IntegerPayload::Scalar { value: 7 }, 12),
            integer(
                row,
                "boundary_type",
                IntegerPayload::Scalar { value: 0 },
                13,
            ),
            integer(
                row,
                "next_geom_ptr",
                IntegerPayload::Scalar { value: 0 },
                14,
            ),
            integer(row, "orient", IntegerPayload::Scalar { value: 1 }, 15),
        ];
        let real_values = vec![
            spline_real_array(
                primitive,
                "local_sys",
                vec![4, 3],
                [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0],
                20,
            ),
            real_scalar(primitive, "radius1", major_radius, 21),
            real_scalar(primitive, "radius2", minor_radius, 22),
        ];
        Persistence {
            real_values: crate::legacy::TypedValues {
                rows: real_values,
                unresolved_count: 0,
            },
            integer_values: crate::legacy::TypedValues {
                rows: integer_values,
                unresolved_count: 0,
            },
            objects,
            ..Persistence::default()
        }
    }

    #[test]
    fn extracts_row_major_cylinder_carrier_from_active_namespace() {
        let data = fixture(2.0, false);
        let Ok(persistence) = crate::legacy::scan(&data, std::iter::once(0..data.len())) else {
            panic!("the fixture states a persistence scope past its own end");
        };
        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(
            result.carriers[0].namespace,
            LegacySurfaceNamespace::Visible
        );
        assert_eq!(result.rows[0].id, 42);
        assert_eq!(
            result.carriers[0].geometry,
            LegacySurfaceGeometry::Cylinder {
                frame: frame([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                radius: 2.0,
            }
        );
    }

    #[test]
    fn conflicting_complete_scalar_fields_withhold_legacy_carrier() {
        let data = fixture(2.0, true);
        let Ok(persistence) = crate::legacy::scan(&data, std::iter::once(0..data.len())) else {
            panic!("the fixture states a persistence scope past its own end");
        };
        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn a_nonfinite_local_system_slot_withholds_the_real_record_and_the_carrier() {
        let data = String::from_utf8(fixture(2.0, false))
            .expect("ASCII fixture")
            .replace(
                "$3FF,0,0,0,3FF,0,0,0,3FF,0,0,0",
                "$3FF,0,0,0,3FF,0,0,0,3FF,7FF,0,0",
            )
            .into_bytes();
        let Ok(persistence) = crate::legacy::scan(&data, std::iter::once(0..data.len())) else {
            panic!("the fixture states a persistence scope past its own end");
        };

        assert!(persistence
            .real_values
            .rows
            .iter()
            .all(|record| record.name != "local_sys"));
        assert_eq!(persistence.real_values.unresolved_count, 1);

        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn a_left_handed_local_system_withholds_the_legacy_carrier() {
        let data = String::from_utf8(fixture(2.0, false))
            .expect("ASCII fixture")
            .replace(
                "$3FF,0,0,0,3FF,0,0,0,3FF,0,0,0",
                "$3FF,0,0,0,BFF,0,0,0,3FF,0,0,0",
            )
            .into_bytes();
        let Ok(persistence) = crate::legacy::scan(&data, std::iter::once(0..data.len())) else {
            panic!("the fixture states a persistence scope past its own end");
        };

        assert!(persistence
            .real_values
            .rows
            .iter()
            .any(|record| record.name == "local_sys"));
        assert_eq!(persistence.real_values.unresolved_count, 0);

        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn extracts_complete_legacy_carrier_from_nonvisible_namespace() {
        let data = String::from_utf8(fixture(2.0, false))
            .expect("ASCII fixture")
            .replace("Sld_VisGeom", "Sld_NonVisGeom")
            .replace("active_geom", "inactive_geom")
            .into_bytes();
        let Ok(persistence) = crate::legacy::scan(&data, std::iter::once(0..data.len())) else {
            panic!("the fixture states a persistence scope past its own end");
        };
        let result = scan(&persistence);

        assert!(result.rows.is_empty());
        assert_eq!(result.nonvisible_rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(
            result.carriers[0].namespace,
            LegacySurfaceNamespace::NonVisible
        );
        assert_eq!(result.carriers[0].surface_id, 42);
    }

    #[test]
    fn extracts_complete_legacy_spline_surface_carrier() {
        let result = scan(&spline_persistence(true));

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(result.carriers[0].surface_id, 42);
        let LegacySurfaceGeometry::Spline(spline) = &result.carriers[0].geometry else {
            panic!("expected spline carrier");
        };
        assert_eq!(spline.points().len(), 4);
        assert_eq!(spline.u_parameters(), &[0.0, 1.0]);
        assert_eq!(spline.v_parameters(), &[0.0, 1.0]);
        assert_eq!(spline.u_derivatives().len(), 4);
        assert_eq!(spline.v_derivatives().len(), 4);
        assert_eq!(spline.mixed_derivatives().len(), 4);
    }

    #[test]
    fn incomplete_legacy_spline_surface_fields_withhold_carrier() {
        let result = scan(&spline_persistence(false));

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn converts_full_spline_derivative_grid_to_boundary_order() {
        let points = vec![[0.0; 3]; 3 * 2];
        let u_parameters = [0.0, 0.5, 1.0];
        let v_parameters = [0.0, 1.0];
        let vector = |value: f64| [value, value, value];
        let u_tangents = (0..6)
            .map(|value| vector(f64::from(value)))
            .collect::<Vec<_>>();
        let v_tangents = (10..16)
            .map(|value| vector(f64::from(value)))
            .collect::<Vec<_>>();
        let mixed_derivatives = (20..26)
            .map(|value| vector(f64::from(value)))
            .collect::<Vec<_>>();

        let spline = crate::interpolation_grid::InterpolationGrid::from_full_tangent_grid(
            points,
            u_parameters.to_vec(),
            v_parameters.to_vec(),
            &u_tangents,
            &v_tangents,
            &mixed_derivatives,
        )
        .expect("complete full derivative grid");
        let u_derivatives = spline.u_derivatives();
        let v_derivatives = spline.v_derivatives();
        let mixed_derivatives = spline.mixed_derivatives();

        assert_eq!(u_derivatives, [0.0, 1.0, 4.0, 5.0].map(vector));
        assert_eq!(
            v_derivatives,
            [10.0, 12.0, 14.0, 11.0, 13.0, 15.0].map(vector)
        );
        assert_eq!(*mixed_derivatives, [20.0, 21.0, 24.0, 25.0].map(vector));
    }

    #[test]
    fn extracts_signed_legacy_cone_carrier_from_active_namespace() {
        let result = scan(&cone_persistence(Some(-std::f64::consts::FRAC_PI_4)));

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(
            result.carriers[0].geometry,
            LegacySurfaceGeometry::Cone {
                frame: frame([1.0, 2.0, 3.0], [-0.0, -0.0, -1.0], [1.0, 0.0, 0.0]),
                half_angle: std::f64::consts::FRAC_PI_4,
                parameter_v_sign: -1.0,
            }
        );
    }

    #[test]
    fn incomplete_legacy_cone_angle_withholds_carrier() {
        let result = scan(&cone_persistence(None));

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn legacy_cone_half_angle_magnitude_takes_the_apex_cone_interval() {
        let refused = [
            0.0,
            -0.0,
            std::f64::consts::FRAC_PI_2,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        for signed_half_angle in refused {
            let result = scan(&cone_persistence(Some(signed_half_angle)));
            assert_eq!(result.rows.len(), 1);
            assert!(
                result.carriers.is_empty(),
                "{signed_half_angle} is outside the apex cone interval"
            );
        }

        let largest_admitted = f64::from_bits(std::f64::consts::FRAC_PI_2.to_bits() - 1);
        let smallest_admitted = f64::MIN_POSITIVE;
        for (signed_half_angle, half_angle, parameter_v_sign) in [
            (largest_admitted, largest_admitted, 1.0),
            (-largest_admitted, largest_admitted, -1.0),
            (smallest_admitted, smallest_admitted, 1.0),
            (-smallest_admitted, smallest_admitted, -1.0),
        ] {
            let result = scan(&cone_persistence(Some(signed_half_angle)));
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.carriers.len(), 1);
            assert_eq!(
                result.carriers[0].geometry,
                LegacySurfaceGeometry::Cone {
                    frame: frame(
                        [1.0, 2.0, 3.0],
                        if parameter_v_sign > 0.0 {
                            [0.0, 0.0, 1.0]
                        } else {
                            [-0.0, -0.0, -1.0]
                        },
                        [1.0, 0.0, 0.0],
                    ),
                    half_angle,
                    parameter_v_sign,
                }
            );
        }
    }

    #[test]
    fn extracts_legacy_torus_and_zero_major_radius_sphere_carriers() {
        let torus = scan(&torus_persistence(4.0, 0.5));
        assert_eq!(torus.rows.len(), 1);
        assert_eq!(torus.carriers.len(), 1);
        assert_eq!(
            torus.carriers[0].geometry,
            LegacySurfaceGeometry::Torus {
                frame: frame([1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                major_radius: 4.0,
                minor_radius: 0.5,
            }
        );

        let sphere = scan(&torus_persistence(0.0, 2.0));
        assert_eq!(sphere.rows.len(), 1);
        assert_eq!(sphere.carriers.len(), 1);
        assert_eq!(
            sphere.carriers[0].geometry,
            LegacySurfaceGeometry::Sphere {
                frame: frame([1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                radius: 2.0,
            }
        );
    }

    #[test]
    fn canonicalizes_negative_legacy_cone_v_parameters() {
        let carriers = [LegacySurfaceCarrier {
            namespace: LegacySurfaceNamespace::Visible,
            surface_id: 42,
            geometry: LegacySurfaceGeometry::Cone {
                frame: frame([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                half_angle: std::f64::consts::FRAC_PI_4,
                parameter_v_sign: -1.0,
            },
            offset: 0,
        }];

        assert_eq!(
            canonicalize_legacy_cone_pcurve_endpoints(
                &carriers,
                42,
                [[0.0, 2.0], [std::f64::consts::PI, -3.0]],
            ),
            [[0.0, -2.0], [std::f64::consts::PI, 3.0]],
        );
    }

    fn integer(
        parent: &str,
        name: &str,
        payload: IntegerPayload,
        offset: usize,
    ) -> crate::legacy::IntegerRecord {
        ValueRecord {
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload,
            offset,
        }
    }

    fn real_array(parent: &str, values: [[f64; 4]; 2], offset: usize) -> crate::legacy::RealRecord {
        let values = values
            .into_iter()
            .flatten()
            .map(|value| Real::from_bits(value.to_bits()));
        let runs = values.map(|value| RealRun { count: 1, value }).collect();
        ValueRecord {
            name: "crv_pnt_arr".to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: RealPayload::array(vec![2, 4], runs).expect("complete numeric array"),
            offset,
        }
    }

    fn curve_field_records(
        curve: &str,
        id: i32,
        faces: [i32; 2],
        next: [i32; 2],
    ) -> Vec<crate::legacy::IntegerRecord> {
        vec![
            integer(
                curve,
                "crv_id",
                IntegerPayload::Scalar { value: id },
                id as usize,
            ),
            integer(
                curve,
                "type",
                IntegerPayload::Scalar { value: 0 },
                100 + id as usize,
            ),
            integer(
                curve,
                "feat_id",
                IntegerPayload::Scalar { value: 7 },
                200 + id as usize,
            ),
            integer(
                curve,
                "crv_pnt_dir",
                IntegerPayload::array(
                    vec![2],
                    vec![
                        IntegerRun { count: 1, value: 1 },
                        IntegerRun {
                            count: 1,
                            value: -1,
                        },
                    ],
                )
                .expect("complete numeric array"),
                300 + id as usize,
            ),
            integer(
                curve,
                "crv_hdr_geom_ptr[0]",
                IntegerPayload::Scalar { value: faces[0] },
                400 + id as usize,
            ),
            integer(
                curve,
                "crv_hdr_geom_ptr[1]",
                IntegerPayload::Scalar { value: faces[1] },
                500 + id as usize,
            ),
            integer(
                curve,
                "next_crv_hdr_ptr[0]",
                IntegerPayload::Scalar { value: next[0] },
                600 + id as usize,
            ),
            integer(
                curve,
                "next_crv_hdr_ptr[1]",
                IntegerPayload::Scalar { value: next[1] },
                700 + id as usize,
            ),
        ]
    }

    fn topology_persistence() -> Persistence {
        let root = "root";
        let branch = "branch";
        let array = "curve_array";
        let first = "curve_10";
        let second = "curve_11";
        let objects = vec![
            object(root, "Sld_VisGeom", None, ObjectPayload::Arrow),
            object(branch, "active_geom", Some(root), ObjectPayload::Arrow),
            object(
                array,
                "crv_array",
                Some(branch),
                ObjectPayload::Array {
                    dimensions: vec![2],
                    elements: vec![first.to_string(), second.to_string()],
                },
            ),
            object(first, "crv_array", Some(array), ObjectPayload::Arrow),
            object(second, "crv_array", Some(array), ObjectPayload::Arrow),
        ];
        let mut integer_values = curve_field_records(first, 10, [100, 200], [11, 11]);
        integer_values.extend(curve_field_records(second, 11, [100, 200], [10, 10]));
        let real_values = vec![real_array(
            first,
            [[0.0, 1.0, 2.0, 3.0], [4.0, 5.0, 6.0, 7.0]],
            810,
        )];
        Persistence {
            real_values: crate::legacy::TypedValues {
                rows: real_values,
                unresolved_count: 0,
            },
            integer_values: crate::legacy::TypedValues {
                rows: integer_values,
                unresolved_count: 0,
            },
            objects,
            ..Persistence::default()
        }
    }

    #[test]
    fn extracts_legacy_curve_topology_and_endpoint_witnesses() {
        let result = scan(&topology_persistence());

        assert_eq!(result.topology_rows.len(), 2);
        assert_eq!(result.topology_rows[0].id, 10);
        assert_eq!(result.topology_rows[0].directions, [0x01, 0xf6]);
        assert_eq!(
            result.topology_rows[0].faces,
            [
                std::num::NonZeroU32::new(100),
                std::num::NonZeroU32::new(200)
            ]
        );
        assert_eq!(result.topology_rows[0].next_edges, [11, 11]);
        assert_eq!(result.pcurves.len(), 1);
        assert_eq!(result.pcurves[0].curve_id, 10);
        assert_eq!(result.pcurves[0].face_0_endpoints, [[0.0, 1.0], [4.0, 5.0]]);
        assert_eq!(result.pcurves[0].face_1_endpoints, [[2.0, 3.0], [6.0, 7.0]]);
    }

    #[test]
    fn incomplete_legacy_curve_fields_withhold_topology() {
        let mut persistence = topology_persistence();
        persistence.integer_values.rows.retain(|record| {
            !(record.parent == Some(fixture_offset("curve_11"))
                && record.name == "next_crv_hdr_ptr[1]")
        });

        let result = scan(&persistence);

        assert_eq!(result.topology_rows.len(), 1);
        assert_eq!(result.topology_rows[0].id, 10);
    }
}
