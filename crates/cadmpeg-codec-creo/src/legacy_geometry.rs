// SPDX-License-Identifier: Apache-2.0
//! Geometry records owned by the legacy ASCII persistence object graph.

use std::collections::BTreeMap;

use crate::legacy::{self, NumericPayload, ObjectPayload, ObjectRecord, Persistence, RealRecord};
use crate::surface::{self, SurfaceKind, SurfaceRow};

/// A complete model-space carrier from one legacy analytic surface prototype.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// One complete legacy analytic carrier associated with a visible surface row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LegacySurfaceCarrier {
    /// Visible `srf_array` surface identifier.
    pub(crate) surface_id: u32,
    /// Complete analytic geometry.
    pub(crate) geometry: LegacySurfaceGeometry,
    /// Byte offset of the `srf_prim_ptr` object.
    pub(crate) offset: usize,
}

/// Legacy surface rows and complete analytic carriers from both namespaces.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LegacySurfaceScan {
    /// Rows under `Sld_VisGeom.active_geom.srf_array`.
    pub(crate) rows: Vec<SurfaceRow>,
    /// Rows under `Sld_NonVisGeom.inactive_geom.srf_array`.
    pub(crate) nonvisible_rows: Vec<SurfaceRow>,
    /// Complete plane and cylinder carriers from visible rows.
    pub(crate) carriers: Vec<LegacySurfaceCarrier>,
}

type ObjectIdIndex<'a> = BTreeMap<&'a str, &'a ObjectRecord>;
type ChildIndex<'a> = BTreeMap<&'a str, Vec<&'a ObjectRecord>>;
type IntegerFieldIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealFieldIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a RealRecord>>;

/// Decode the surface portions of one legacy persistence object graph.
pub(crate) fn scan(persistence: &Persistence) -> LegacySurfaceScan {
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
    LegacySurfaceScan {
        rows,
        nonvisible_rows,
        carriers,
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
        if let Some(carrier) = analytic_carrier(row_object, &row, children, real_fields) {
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

fn analytic_carrier(
    row_object: &ObjectRecord,
    row: &SurfaceRow,
    children: &ChildIndex<'_>,
    reals: &RealFieldIndex<'_>,
) -> Option<LegacySurfaceCarrier> {
    if !matches!(row.kind, SurfaceKind::Plane | SurfaceKind::Cylinder) {
        return None;
    }
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
        _ => unreachable!("analytic carrier family was filtered above"),
    };
    (primitive.name == expected_name).then_some(())?;

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
        _ => unreachable!("analytic carrier family was filtered above"),
    };
    Some(LegacySurfaceCarrier {
        surface_id: row.id,
        geometry,
        offset: primitive.offset,
    })
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
    use super::{scan, LegacySurfaceGeometry};

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
}
