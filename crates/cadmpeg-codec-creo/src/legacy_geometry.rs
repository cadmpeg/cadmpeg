// SPDX-License-Identifier: Apache-2.0
//! Geometry records owned by the legacy ASCII persistence object graph.

use std::collections::BTreeMap;

use crate::curve::{CurveTopologyRow, PcurveEndpoints};
use crate::legacy::{self, NumericPayload, ObjectPayload, ObjectRecord, Persistence, RealRecord};
use crate::surface::{self, SurfaceKind, SurfaceRow};

/// A complete model-space carrier from one legacy analytic surface prototype.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LegacySurfaceGeometry {
    /// A plane from a complete row-major local system.
    Plane {
        /// Origin in the stored model coordinate system.
        origin: [f64; 3],
        /// Surface normal from local-system column two.
        normal: [f64; 3],
        /// Parameter-space reference direction from local-system column zero.
        u_axis: [f64; 3],
    },
    /// A cylinder from a complete row-major local system and radius.
    Cylinder {
        /// A point on the cylinder axis in the stored model coordinate system.
        origin: [f64; 3],
        /// Cylinder axis from local-system column two.
        axis: [f64; 3],
        /// Parameter-space reference direction from local-system column zero.
        ref_direction: [f64; 3],
        /// Positive cylinder radius in the stored model coordinate system.
        radius: f64,
    },
    /// A circular cone from a complete legacy local system and signed angle.
    Cone {
        /// The cone apex in the stored model coordinate system.
        apex: [f64; 3],
        /// Unit axis directed from the apex toward increasing radius.
        axis: [f64; 3],
        /// Unit parameter-space reference direction.
        ref_direction: [f64; 3],
        /// Positive cone half-angle in radians.
        half_angle: f64,
        /// Sign that maps the source `v` parameter to this positive-angle frame.
        parameter_v_sign: f64,
    },
    /// A complete bicubic interpolation surface carrier.
    Spline {
        /// Interpolation points in source order.
        points: Vec<[f64; 3]>,
        /// Ordered interpolation parameters in the first surface direction.
        u_parameters: Vec<f64>,
        /// Ordered interpolation parameters in the second surface direction.
        v_parameters: Vec<f64>,
        /// Boundary derivatives in the first surface direction.
        u_derivatives: Vec<[f64; 3]>,
        /// Boundary derivatives in the second surface direction.
        v_derivatives: Vec<[f64; 3]>,
        /// Mixed derivatives at the four parameter-domain corners.
        mixed_derivatives: Vec<[f64; 3]>,
    },
}

/// One complete legacy surface carrier associated with a visible surface row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LegacySurfaceCarrier {
    /// Visible `srf_array` surface identifier.
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
    /// Complete surface carriers from visible rows.
    pub(crate) carriers: Vec<LegacySurfaceCarrier>,
    /// Complete visible curve topology rows from the legacy `crv_array`
    /// namespace.
    pub(crate) topology_rows: Vec<CurveTopologyRow>,
    /// Complete endpoint witnesses from legacy `crv_pnt_arr` samples.
    pub(crate) pcurves: Vec<PcurveEndpoints>,
}

type ObjectIdIndex<'a> = BTreeMap<&'a str, &'a ObjectRecord>;
type ChildIndex<'a> = BTreeMap<&'a str, Vec<&'a ObjectRecord>>;
type IntegerFieldIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealFieldIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a RealRecord>>;

/// Decode the surface portions of one legacy persistence object graph.
pub(crate) fn scan(persistence: &Persistence) -> LegacyGeometryScan {
    let object_ids = object_id_index(&persistence.objects);
    let children = child_index(&persistence.objects);
    let integer_fields = integer_field_index(&persistence.integer_values);
    let real_fields = real_field_index(&persistence.real_values);
    let (rows, carriers) = namespace(
        &persistence.objects,
        &object_ids,
        &children,
        &integer_fields,
        &real_fields,
        "Sld_VisGeom",
        "active_geom",
    );
    let (nonvisible_rows, _) = namespace(
        &persistence.objects,
        &object_ids,
        &children,
        &integer_fields,
        &real_fields,
        "Sld_NonVisGeom",
        "inactive_geom",
    );
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
    let Some(elements) = curve_array_elements(objects, object_ids, "Sld_VisGeom", "active_geom")
    else {
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

fn curve_array_elements<'a>(
    objects: &'a [ObjectRecord],
    object_ids: &ObjectIdIndex<'a>,
    root_name: &str,
    branch_name: &str,
) -> Option<Vec<&'a ObjectRecord>> {
    let mut roots = objects
        .iter()
        .filter(|object| object.name == root_name && object.parent.is_none());
    let root = roots.next()?;
    roots.next().is_none().then_some(())?;

    let mut branches = objects.iter().filter(|object| {
        object.parent.as_deref() == Some(root.id.as_str()) && object.name == branch_name
    });
    let branch = branches.next()?;
    branches.next().is_none().then_some(())?;

    let mut arrays = objects.iter().filter(|object| {
        object.parent.as_deref() == Some(branch.id.as_str())
            && object.name == "crv_array"
            && matches!(object.payload, ObjectPayload::Array { complete: true, .. })
    });
    let array = arrays.next()?;
    arrays.next().is_none().then_some(())?;

    let ObjectPayload::Array { elements, .. } = &array.payload else {
        unreachable!("the curve namespace array was filtered above");
    };
    elements
        .iter()
        .map(|element_id| {
            let element = object_ids.get(element_id.as_str()).copied()?;
            (element.parent.as_deref() == Some(array.id.as_str()) && element.name == "crv_array")
                .then_some(())?;
            Some(element)
        })
        .collect()
}

fn curve_topology_row(
    curve_object: &ObjectRecord,
    integers: &IntegerFieldIndex<'_>,
) -> Option<CurveTopologyRow> {
    let id = u32::try_from(integer_field(integers, &curve_object.id, "crv_id")?).ok()?;
    let type_byte = u8::try_from(integer_field(integers, &curve_object.id, "type")?).ok()?;
    let feature_id = u32::try_from(integer_field(integers, &curve_object.id, "feat_id")?).ok()?;
    let directions = integer_array(integers, &curve_object.id, "crv_pnt_dir")?
        .into_iter()
        .map(legacy_direction)
        .collect::<Option<Vec<_>>>()?;
    let [first_direction, second_direction] = directions.as_slice() else {
        return None;
    };
    let faces = [
        u32::try_from(integer_field(
            integers,
            &curve_object.id,
            "crv_hdr_geom_ptr[0]",
        )?)
        .ok()?,
        u32::try_from(integer_field(
            integers,
            &curve_object.id,
            "crv_hdr_geom_ptr[1]",
        )?)
        .ok()?,
    ];
    let next_edges = [
        u32::try_from(integer_field(
            integers,
            &curve_object.id,
            "next_crv_hdr_ptr[0]",
        )?)
        .ok()?,
        u32::try_from(integer_field(
            integers,
            &curve_object.id,
            "next_crv_hdr_ptr[1]",
        )?)
        .ok()?,
    ];
    Some(CurveTopologyRow {
        id,
        type_byte,
        feature_id,
        directions: [*first_direction, *second_direction],
        faces,
        next_edges,
        offset: integer_record(integers, &curve_object.id, "crv_id")?.offset,
    })
}

fn curve_pcurve(
    curve_object: &ObjectRecord,
    topology: &CurveTopologyRow,
    reals: &RealFieldIndex<'_>,
) -> Option<PcurveEndpoints> {
    let record = real_record(reals, &curve_object.id, "crv_pnt_arr")?;
    let NumericPayload::Array { dimensions, runs } = &record.payload else {
        return None;
    };
    let [sample_count, lane_width] = dimensions.as_slice() else {
        return None;
    };
    if *lane_width != 4 || *sample_count < 2 {
        return None;
    }
    let sample_count = usize::try_from(*sample_count).ok()?;
    let expected_elements = sample_count.checked_mul(4)?;
    if record.payload.element_count() != u64::try_from(expected_elements).ok()? {
        return None;
    }
    let mut values = Vec::new();
    for run in runs {
        let count = usize::try_from(run.count).ok()?;
        let value = run.value.value();
        value.is_finite().then_some(())?;
        for _ in 0..count {
            values.push(value);
        }
    }
    let first: [f64; 4] = values.get(..4)?.try_into().ok()?;
    let last: [f64; 4] = values
        .get((sample_count - 1).checked_mul(4)?..expected_elements)?
        .try_into()
        .ok()?;
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

fn namespace(
    objects: &[ObjectRecord],
    object_ids: &ObjectIdIndex<'_>,
    children: &ChildIndex<'_>,
    integer_fields: &IntegerFieldIndex<'_>,
    real_fields: &RealFieldIndex<'_>,
    root_name: &str,
    branch_name: &str,
) -> (Vec<SurfaceRow>, Vec<LegacySurfaceCarrier>) {
    let Some(elements) = surface_array_elements(objects, object_ids, root_name, branch_name) else {
        return (Vec::new(), Vec::new());
    };

    let mut rows = Vec::new();
    let mut carriers = Vec::new();
    for row_object in elements {
        let Some(row) = surface_row(row_object, integer_fields) else {
            continue;
        };
        if let Some(carrier) = surface_carrier(row_object, &row, children, real_fields) {
            carriers.push(carrier);
        }
        rows.push(row);
    }
    rows.sort_by_key(|row| row.offset);
    carriers.sort_by_key(|carrier| carrier.offset);
    (rows, carriers)
}

fn surface_array_elements<'a>(
    objects: &'a [ObjectRecord],
    object_ids: &ObjectIdIndex<'a>,
    root_name: &str,
    branch_name: &str,
) -> Option<Vec<&'a ObjectRecord>> {
    let mut roots = objects
        .iter()
        .filter(|object| object.name == root_name && object.parent.is_none());
    let root = roots.next()?;
    roots.next().is_none().then_some(())?;

    let mut branches = objects.iter().filter(|object| {
        object.parent.as_deref() == Some(root.id.as_str()) && object.name == branch_name
    });
    let branch = branches.next()?;
    branches.next().is_none().then_some(())?;

    let mut arrays = objects.iter().filter(|object| {
        object.parent.as_deref() == Some(branch.id.as_str())
            && object.name == "srf_array"
            && matches!(object.payload, ObjectPayload::Array { complete: true, .. })
    });
    let array = arrays.next()?;
    arrays.next().is_none().then_some(())?;

    let ObjectPayload::Array { elements, .. } = &array.payload else {
        unreachable!("the namespace array was filtered above");
    };
    elements
        .iter()
        .map(|element_id| {
            let element = object_ids.get(element_id.as_str()).copied()?;
            (element.parent.as_deref() == Some(array.id.as_str()) && element.name == "srf_array")
                .then_some(())?;
            Some(element)
        })
        .collect()
}

fn surface_row(row_object: &ObjectRecord, integers: &IntegerFieldIndex<'_>) -> Option<SurfaceRow> {
    let type_byte = u8::try_from(integer_field(integers, &row_object.id, "geom_type")?).ok()?;
    let kind = SurfaceKind::from_byte(type_byte)?;
    let feature_id = u32::try_from(integer_field(integers, &row_object.id, "feat_id")?).ok()?;
    let id = u32::try_from(integer_field(integers, &row_object.id, "geom_id")?).ok()?;
    let boundary_type =
        u8::try_from(integer_field(integers, &row_object.id, "boundary_type")?).ok()?;
    if !surface::is_surface_boundary_type(boundary_type) {
        return None;
    }
    let orientation = integer_field(integers, &row_object.id, "orient")?;
    let reversed = match orientation {
        1 => false,
        -1 => true,
        _ => return None,
    };
    let next_surface =
        u32::try_from(integer_field(integers, &row_object.id, "next_geom_ptr")?).ok()?;
    Some(SurfaceRow {
        id,
        type_byte,
        kind,
        feature_id,
        reversed,
        boundary_type,
        next_surface,
        offset: integer_record(integers, &row_object.id, "geom_id")?.offset,
    })
}

fn surface_carrier(
    row_object: &ObjectRecord,
    row: &SurfaceRow,
    children: &ChildIndex<'_>,
    reals: &RealFieldIndex<'_>,
) -> Option<LegacySurfaceCarrier> {
    let mut primitives = children
        .get(row_object.id.as_str())?
        .iter()
        .copied()
        .filter(|object| object.name.starts_with("srf_prim_ptr("));
    let primitive = primitives.next()?;
    primitives.next().is_none().then_some(())?;
    let expected_name = match row.kind {
        SurfaceKind::Plane => "srf_prim_ptr(plane)",
        SurfaceKind::Cylinder => "srf_prim_ptr(cylinder)",
        SurfaceKind::Cone => "srf_prim_ptr(cone)",
        SurfaceKind::Spline => "srf_prim_ptr(splsrf)",
        _ => return None,
    };
    (primitive.name == expected_name).then_some(())?;

    if row.kind == SurfaceKind::Spline {
        return Some(LegacySurfaceCarrier {
            surface_id: row.id,
            geometry: LegacySurfaceGeometry::Spline {
                points: real_vector_array(reals, &primitive.id, "i_points")?,
                u_parameters: real_scalar_array(reals, &primitive.id, "u_params")?,
                v_parameters: real_scalar_array(reals, &primitive.id, "v_params")?,
                u_derivatives: real_vector_array(reals, &primitive.id, "u_tangts")?,
                v_derivatives: real_vector_array(reals, &primitive.id, "v_tangts")?,
                mixed_derivatives: real_vector_array(reals, &primitive.id, "uv_deriv")?,
            },
            offset: primitive.offset,
        });
    }

    let local_system = real_record(reals, &primitive.id, "local_sys")?;
    let slots = local_system_slots(local_system)?;
    let first = [slots[0], slots[3], slots[6]];
    let second = [slots[1], slots[4], slots[7]];
    let third = [slots[2], slots[5], slots[8]];
    surface::valid_right_handed_frame(first, second, third).then_some(())?;
    let origin = [slots[9], slots[10], slots[11]];
    let geometry = match row.kind {
        SurfaceKind::Plane => LegacySurfaceGeometry::Plane {
            origin,
            normal: third,
            u_axis: first,
        },
        SurfaceKind::Cylinder => LegacySurfaceGeometry::Cylinder {
            origin,
            axis: third,
            ref_direction: first,
            radius: real_scalar(reals, &primitive.id, "radius")
                .filter(|radius| radius.is_finite() && *radius > 0.0)?,
        },
        SurfaceKind::Cone => {
            let signed_half_angle = real_scalar(reals, &primitive.id, "half_angle")?;
            if !signed_half_angle.is_finite()
                || signed_half_angle == 0.0
                || signed_half_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return None;
            }
            LegacySurfaceGeometry::Cone {
                apex: origin,
                axis: third.map(|value| {
                    if signed_half_angle.is_sign_positive() {
                        value
                    } else {
                        -value
                    }
                }),
                ref_direction: first,
                half_angle: signed_half_angle.abs(),
                parameter_v_sign: signed_half_angle.signum(),
            }
        }
        _ => unreachable!("surface carrier family was filtered above"),
    };
    Some(LegacySurfaceCarrier {
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
    parent: &str,
    name: &str,
) -> Option<Vec<[f64; 3]>> {
    let record = real_record(records, parent, name)?;
    let values = real_array_values(record)?;
    let NumericPayload::Array { dimensions, .. } = &record.payload else {
        return None;
    };
    let [count, width] = dimensions.as_slice() else {
        return None;
    };
    (*width == 3 && values.len() == usize::try_from(*count).ok()?.checked_mul(3)?).then_some(())?;
    values
        .chunks_exact(3)
        .map(|vector| vector.try_into().ok())
        .collect()
}

fn real_scalar_array(records: &RealFieldIndex<'_>, parent: &str, name: &str) -> Option<Vec<f64>> {
    let record = real_record(records, parent, name)?;
    let NumericPayload::Array { dimensions, .. } = &record.payload else {
        return None;
    };
    (dimensions.len() == 1).then_some(())?;
    let values = real_array_values(record)?;
    (values.len() == usize::try_from(dimensions[0]).ok()?).then_some(values)
}

fn real_array_values(record: &RealRecord) -> Option<Vec<f64>> {
    let NumericPayload::Array { dimensions, runs } = &record.payload else {
        return None;
    };
    let expected = dimensions.iter().try_fold(1usize, |product, dimension| {
        product.checked_mul(usize::try_from(*dimension).ok()?)
    })?;
    let mut values = Vec::with_capacity(expected);
    for run in runs {
        let value = run.value.value();
        value.is_finite().then_some(())?;
        let count = usize::try_from(run.count).ok()?;
        values.extend(std::iter::repeat_n(value, count));
    }
    (values.len() == expected).then_some(values)
}

fn object_id_index(objects: &[ObjectRecord]) -> ObjectIdIndex<'_> {
    objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect()
}

fn child_index(objects: &[ObjectRecord]) -> ChildIndex<'_> {
    let mut index = BTreeMap::new();
    for object in objects {
        if let Some(parent) = object.parent.as_deref() {
            index.entry(parent).or_insert_with(Vec::new).push(object);
        }
    }
    index
}

fn integer_field_index(records: &[legacy::IntegerRecord]) -> IntegerFieldIndex<'_> {
    let mut index = BTreeMap::new();
    for record in records {
        if let Some(parent) = record.parent.as_deref() {
            index
                .entry((parent, record.name.as_str()))
                .or_insert_with(Vec::new)
                .push(record);
        }
    }
    index
}

fn real_field_index(records: &[RealRecord]) -> RealFieldIndex<'_> {
    let mut index = BTreeMap::new();
    for record in records {
        if let Some(parent) = record.parent.as_deref() {
            index
                .entry((parent, record.name.as_str()))
                .or_insert_with(Vec::new)
                .push(record);
        }
    }
    index
}

fn integer_record<'a>(
    records: &'a IntegerFieldIndex<'a>,
    parent: &str,
    name: &str,
) -> Option<&'a legacy::IntegerRecord> {
    let matches = records.get(&(parent, name))?;
    (matches.len() == 1).then_some(matches[0])
}

fn integer_field(records: &IntegerFieldIndex<'_>, parent: &str, name: &str) -> Option<i32> {
    let record = integer_record(records, parent, name)?;
    match &record.payload {
        NumericPayload::Scalar { value } => Some(*value),
        NumericPayload::Array { .. } => None,
    }
}

fn integer_array(records: &IntegerFieldIndex<'_>, parent: &str, name: &str) -> Option<Vec<i32>> {
    let record = integer_record(records, parent, name)?;
    let NumericPayload::Array { dimensions, runs } = &record.payload else {
        return None;
    };
    let expected = dimensions.iter().try_fold(1u64, |count, dimension| {
        count.checked_mul(u64::from(*dimension))
    })?;
    (record.payload.element_count() == expected).then_some(())?;
    let mut values = Vec::new();
    for run in runs {
        let count = usize::try_from(run.count).ok()?;
        for _ in 0..count {
            values.push(run.value);
        }
    }
    (u64::try_from(values.len()).ok()? == expected).then_some(values)
}

fn real_record<'a>(
    records: &'a RealFieldIndex<'a>,
    parent: &str,
    name: &str,
) -> Option<&'a RealRecord> {
    let matches = records.get(&(parent, name))?;
    (matches.len() == 1).then_some(matches[0])
}

fn real_scalar(records: &RealFieldIndex<'_>, parent: &str, name: &str) -> Option<f64> {
    let record = real_record(records, parent, name)?;
    match &record.payload {
        NumericPayload::Scalar { value } => Some(value.value()),
        NumericPayload::Array { .. } => None,
    }
}

fn local_system_slots(record: &RealRecord) -> Option<[f64; 12]> {
    let NumericPayload::Array { dimensions, runs } = &record.payload else {
        return None;
    };
    (dimensions.as_slice() == [4, 3] && record.payload.element_count() == 12).then_some(())?;
    let mut slots = Vec::with_capacity(12);
    for run in runs {
        let count = usize::try_from(run.count).ok()?;
        for _ in 0..count {
            slots.push(run.value.value());
        }
    }
    slots.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_legacy_cone_pcurve_endpoints, scan, LegacySurfaceCarrier,
        LegacySurfaceGeometry,
    };
    use crate::legacy::{
        IntegerPayload, IntegerRun, ObjectPayload, ObjectRecord, Persistence, Real, RealPayload,
        RealRun, ValueRecord,
    };

    fn real(value: f64) -> String {
        format!("{:016X}", value.to_bits())
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
            id: format!("{parent}:{name}"),
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(parent.to_string()),
            depth: 0,
            payload: RealPayload::Array { dimensions, runs },
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
                    complete: true,
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
            real_values,
            integer_values,
            objects,
            ..Persistence::default()
        }
    }

    fn cone_persistence(with_angle: bool) -> Persistence {
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
                    complete: true,
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
        if with_angle {
            real_values.push(ValueRecord {
                id: format!("{primitive}:half_angle"),
                name: "half_angle".to_string(),
                attribute_id: 0,
                scope_offset: 0,
                parent: Some(primitive.to_string()),
                depth: 0,
                payload: RealPayload::Scalar {
                    value: Real::from_bits((-std::f64::consts::FRAC_PI_4).to_bits()),
                },
                offset: 21,
            });
        }
        Persistence {
            real_values,
            integer_values,
            objects,
            ..Persistence::default()
        }
    }

    #[test]
    fn extracts_row_major_cylinder_carrier_from_active_namespace() {
        let data = fixture(2.0, false);
        let persistence = crate::legacy::scan(&data, std::iter::once(0..data.len()));
        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(result.rows[0].id, 42);
        assert_eq!(
            result.carriers[0].geometry,
            LegacySurfaceGeometry::Cylinder {
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 2.0,
            }
        );
    }

    #[test]
    fn conflicting_complete_scalar_fields_withhold_legacy_carrier() {
        let data = fixture(2.0, true);
        let persistence = crate::legacy::scan(&data, std::iter::once(0..data.len()));
        let result = scan(&persistence);

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn extracts_complete_legacy_spline_surface_carrier() {
        let result = scan(&spline_persistence(true));

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(result.carriers[0].surface_id, 42);
        let LegacySurfaceGeometry::Spline {
            points,
            u_parameters,
            v_parameters,
            u_derivatives,
            v_derivatives,
            mixed_derivatives,
        } = &result.carriers[0].geometry
        else {
            panic!("expected spline carrier");
        };
        assert_eq!(points.len(), 4);
        assert_eq!(u_parameters, &[0.0, 1.0]);
        assert_eq!(v_parameters, &[0.0, 1.0]);
        assert_eq!(u_derivatives.len(), 4);
        assert_eq!(v_derivatives.len(), 4);
        assert_eq!(mixed_derivatives.len(), 4);
    }

    #[test]
    fn incomplete_legacy_spline_surface_fields_withhold_carrier() {
        let result = scan(&spline_persistence(false));

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn extracts_signed_legacy_cone_carrier_from_active_namespace() {
        let result = scan(&cone_persistence(true));

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.carriers.len(), 1);
        assert_eq!(
            result.carriers[0].geometry,
            LegacySurfaceGeometry::Cone {
                apex: [1.0, 2.0, 3.0],
                axis: [-0.0, -0.0, -1.0],
                ref_direction: [1.0, 0.0, 0.0],
                half_angle: std::f64::consts::FRAC_PI_4,
                parameter_v_sign: -1.0,
            }
        );
    }

    #[test]
    fn incomplete_legacy_cone_angle_withholds_carrier() {
        let result = scan(&cone_persistence(false));

        assert_eq!(result.rows.len(), 1);
        assert!(result.carriers.is_empty());
    }

    #[test]
    fn canonicalizes_negative_legacy_cone_v_parameters() {
        let carriers = [LegacySurfaceCarrier {
            surface_id: 42,
            geometry: LegacySurfaceGeometry::Cone {
                apex: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
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

    fn object(id: &str, name: &str, parent: Option<&str>, payload: ObjectPayload) -> ObjectRecord {
        ObjectRecord {
            id: id.to_string(),
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: parent.map(str::to_string),
            depth: 0,
            payload,
            offset: 0,
        }
    }

    fn integer(
        parent: &str,
        name: &str,
        payload: IntegerPayload,
        offset: usize,
    ) -> crate::legacy::IntegerRecord {
        ValueRecord {
            id: format!("{parent}:{name}"),
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(parent.to_string()),
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
            id: format!("{parent}:crv_pnt_arr"),
            name: "crv_pnt_arr".to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(parent.to_string()),
            depth: 0,
            payload: RealPayload::Array {
                dimensions: vec![2, 4],
                runs,
            },
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
                IntegerPayload::Array {
                    dimensions: vec![2],
                    runs: vec![
                        IntegerRun { count: 1, value: 1 },
                        IntegerRun {
                            count: 1,
                            value: -1,
                        },
                    ],
                },
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
                    complete: true,
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
            real_values,
            integer_values,
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
        assert_eq!(result.topology_rows[0].faces, [100, 200]);
        assert_eq!(result.topology_rows[0].next_edges, [11, 11]);
        assert_eq!(result.pcurves.len(), 1);
        assert_eq!(result.pcurves[0].curve_id, 10);
        assert_eq!(result.pcurves[0].face_0_endpoints, [[0.0, 1.0], [4.0, 5.0]]);
        assert_eq!(result.pcurves[0].face_1_endpoints, [[2.0, 3.0], [6.0, 7.0]]);
    }

    #[test]
    fn incomplete_legacy_curve_fields_withhold_topology() {
        let mut persistence = topology_persistence();
        persistence.integer_values.retain(|record| {
            !(record.parent.as_deref() == Some("curve_11") && record.name == "next_crv_hdr_ptr[1]")
        });

        let result = scan(&persistence);

        assert_eq!(result.topology_rows.len(), 1);
        assert_eq!(result.topology_rows[0].id, 10);
    }
}
