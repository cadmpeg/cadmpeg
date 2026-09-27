// SPDX-License-Identifier: Apache-2.0
//! Transfer of application-owned mesh and point payloads.

use cadmpeg_core::decode::{
    BoundedCount, DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::SourceObjectAssociation;

use crate::layout::mesh_facet;
use crate::layout::mesh_kernel_side_entry_header as mesh_hdr;
use crate::native::{EntryRecord, PropertyRecord};
use crate::resource::{collection_vec, reserve_vec_items, retained_format, retained_string, retained_suffix};

const MAX_ELEMENTS: usize = 1_000_000;
const MESH_MAGIC: u32 = mesh_hdr::MAGIC_VALUE;
const MESH_VERSION: u32 = mesh_hdr::VERSION_VALUE;

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<bool, CodecError> {
    let mut transferred = false;
    for property in properties {
        let geometry_kind = match property.type_name.as_str() {
            "Mesh::PropertyMeshKernel" => GeometryKind::Mesh,
            "Points::PropertyPointKernel" => GeometryKind::Points,
            _ => continue,
        };
        if property.side_entries().len() > 1 {
            return Err(CodecError::Malformed(retained_format(ctx, format_args!(
                "geometry property {} references more than one side entry",
                property.id
            ), "FreeCAD geometry side-entry error")?));
        }
        let root_entry = validate_value_root(ctx, property, geometry_kind.value_tag())?;
        let side_entry_matches_root = property.side_entries().len()
            == usize::from(root_entry.is_some())
            && property.side_entries().first() == root_entry.as_ref();
        if !side_entry_matches_root {
            return Err(CodecError::Malformed(
                "geometry property has an unowned side-entry reference".into(),
            ));
        }
        let Some(entry_name) = root_entry else {
            continue;
        };
        let Some(entry) = entries.iter().find(|entry| entry.name == *entry_name) else {
            return Err(CodecError::Malformed(retained_format(ctx, format_args!(
                "geometry property {} references missing side entry {entry_name}", property.id
            ), "FreeCAD geometry missing side-entry error")?));
        };
        if geometry_kind == GeometryKind::Mesh {
            reserve_vec_items(ctx, &mut ir.model.tessellations, 1, "FreeCAD mesh tessellations")?;
            ir.model
                .tessellations
                .push(parse_mesh(ctx, property, &entry.data)?);
            transferred = true;
        } else if geometry_kind == GeometryKind::Points {
            let points = parse_points(ctx, property, &entry.data)?;
            reserve_vec_items(ctx, &mut ir.model.points, points.len(), "FreeCAD point records")?;
            ir.model.points.extend(points);
            transferred = true;
        }
    }
    Ok(transferred)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GeometryKind {
    Mesh,
    Points,
}

impl GeometryKind {
    fn value_tag(self) -> &'static str {
        match self {
            Self::Mesh => "Mesh",
            Self::Points => "Points",
        }
    }
}

fn validate_value_root(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_tag: &str,
) -> Result<Option<String>, CodecError> {
    let document = roxmltree::Document::parse(property.xml.text()).or_else(|error| {
        Err(CodecError::Malformed(retained_format(ctx, format_args!(
            "invalid geometry property XML {}: {error}",
            property.id
        ), "FreeCAD geometry XML error")?))
    })?;
    let mut roots = document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.has_tag_name(expected_tag));
    let root = roots.next();
    let extra = roots.next().is_some();
    let Some(root) = root.filter(|_| !extra) else {
        return Err(CodecError::Malformed(retained_format(ctx, format_args!(
            "geometry property {} must contain exactly one {expected_tag} value root, found {}",
            property.id,
            document.root_element().children().filter(|node| node.is_element() && node.has_tag_name(expected_tag)).count()
        ), "FreeCAD geometry root error")?));
    };
    Ok(root
        .attribute("file")
        .filter(|value| !value.is_empty())
        .map(|value| retained_string(ctx, value, "FreeCAD geometry side-entry name"))
        .transpose()?)
}

fn association(ctx: &DecodeContext<'_>, property: &PropertyRecord) -> Result<SourceObjectAssociation, CodecError> {
    Ok(SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new(retained_string(ctx, &property.owner, "FreeCAD geometry object identity")?)
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
        name: Some(retained_string(ctx, &property.name, "FreeCAD geometry property name")?),
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    })
}

fn parse_mesh(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    bytes: &[u8],
) -> Result<Tessellation, CodecError> {
    let mut reader = Reader::new(bytes);
    let byte_order = reader.mesh_byte_order(ctx, &property.id)?;
    reader.skip(mesh_hdr::LEN - mesh_hdr::INFORMATION)?;
    let point_count = reader.count(byte_order, "mesh point count")?;
    let facet_count = reader.count(byte_order, "mesh facet count")?;
    let mut vertices = collection_vec(ctx, point_count, "FreeCAD mesh vertices")?;
    for _ in 0..point_count {
        vertices.push(reader.point3(byte_order, "mesh point")?);
    }
    // Each facet consumes three point indices and three neighbour indices (24 bytes),
    // so the declared count cannot exceed the unread payload.
    let facet_capacity = reader
        .counted(facet_count as u64, mesh_facet::LEN)
        .ok_or_else(|| {
            CodecError::Malformed("mesh facet count exceeds remaining payload".into())
        })?;
    ctx.charge_collection_items(facet_capacity as u64, "FreeCAD mesh facets")?;
    let mut triangles = Vec::new();
    triangles.try_reserve_exact(facet_capacity).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_collection_items,
            used: 0,
            additional: facet_capacity as u64,
            operation: "FreeCAD mesh facets",
        })
    })?;
    for _ in 0..facet_count {
        let triangle = [
            reader.index(byte_order, point_count, "mesh facet point")?,
            reader.index(byte_order, point_count, "mesh facet point")?,
            reader.index(byte_order, point_count, "mesh facet point")?,
        ];
        // The three facet padding words are skipped, not read.
        for _ in 0..3 {
            reader.skip(4)?;
        }
        triangles.push(triangle);
    }
    for _ in 0..6 {
        let value = reader.f32(byte_order)?;
        if !value.is_finite() {
            return Err(CodecError::Malformed(
                "FCStd mesh bounding box contains a non-finite value".into(),
            ));
        }
    }
    reader.finish("mesh payload")?;
    Ok(Tessellation::from_parts(
        retained_suffix(ctx, &property.id, ":mesh", "FreeCAD mesh identity")?,
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices,
            triangles,
        },
        Vec::new(),
    )
    .map_err(|err| CodecError::Malformed(err.to_string()))?
    .with_source_object(Some(association(ctx, property)?)))
}

fn parse_points(ctx: &DecodeContext<'_>, property: &PropertyRecord, bytes: &[u8]) -> Result<Vec<Point>, CodecError> {
    let mut reader = Reader::new(bytes);
    let count = reader.count(ByteOrder::Little, "point-cloud point count")?;
    let transform = point_transform(ctx, property)?;
    let mut points = collection_vec(ctx, count, "FreeCAD point-cloud points")?;
    for index in 0..count {
        let position = reader.point3(ByteOrder::Little, "point-cloud point")?;
        points.push(Point::new(
                PointId::mint(crate::native::model_id_charged(
                    ctx, "point", &property.id, &index.to_string(),
                )?).map_err(CodecError::malformed)?,
                transform_point(transform, position)?,
                Some(association(ctx, property)?),
            ));
    }
    reader.finish("point-cloud payload")?;
    Ok(points)
}

fn point_transform(ctx: &DecodeContext<'_>, property: &PropertyRecord) -> Result<[[FiniteReal; 4]; 4], CodecError> {
    let document = roxmltree::Document::parse(property.xml.text()).or_else(|error| {
        Err(CodecError::Malformed(retained_format(ctx, format_args!(
            "invalid point property XML {}: {error}",
            property.id
        ), "FreeCAD point XML error")?))
    })?;
    let Some(text) = document
        .root_element()
        .children()
        .find(|node| node.is_element() && node.has_tag_name("Points"))
        .and_then(|node| node.attribute("mtrx"))
    else {
        return Ok(identity());
    };
    let mut values = [0.0_f64; 16];
    let mut count = 0_usize;
    for token in text.split_whitespace() {
        let value = token.parse::<f64>()
            .map_err(|_| CodecError::Malformed("invalid point-cloud transform scalar".into()))?;
        if count < values.len() {
            values[count] = value;
        }
        count = count.checked_add(1)
            .ok_or_else(|| CodecError::Malformed("point-cloud transform must contain 16 finite scalars".into()))?;
    }
    if count != values.len() {
        return Err(CodecError::Malformed(
            "point-cloud transform must contain 16 finite scalars".into(),
        ));
    }
    let mut finite = [FiniteReal::ZERO; 16];
    for (index, value) in values.into_iter().enumerate() {
        finite[index] = FiniteReal::new(value).ok_or_else(|| {
            CodecError::Malformed("point-cloud transform must contain 16 finite scalars".into())
        })?;
    }
    Ok(std::array::from_fn(|row| {
        std::array::from_fn(|column| finite[row * 4 + column])
    }))
}

fn identity() -> [[FiniteReal; 4]; 4] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if row == column {
                FiniteReal::ONE
            } else {
                FiniteReal::ZERO
            }
        })
    })
}

/// Places a finite point with a finite transform.
///
/// Finite operands still multiply and add to a non-finite coordinate, which
/// states no position, so the transformed position carries its own test.
fn transform_point(
    transform: [[FiniteReal; 4]; 4],
    point: FinitePoint3,
) -> Result<FinitePoint3, CodecError> {
    let transform = transform.map(|row| row.map(FiniteReal::get));
    let point = point.get();
    let values: [f64; 3] = std::array::from_fn(|row| {
        transform[row][0] * point.x
            + transform[row][1] * point.y
            + transform[row][2] * point.z
            + transform[row][3]
    });
    FinitePoint3::new(Point3::new(values[0], values[1], values[2])).ok_or_else(|| {
        CodecError::Malformed(
            "transformed point-cloud point contains a non-finite coordinate".into(),
        )
    })
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

struct Reader<'a> {
    view: View<'a>,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
        }
    }

    fn remaining(&self) -> usize {
        self.view.remaining()
    }

    fn counted(&self, count: u64, min_element_size: usize) -> Option<usize> {
        self.view
            .counted(count, min_element_size)
            .map(BoundedCount::get)
    }

    fn skip(&mut self, count: usize) -> Result<(), CodecError> {
        self.view.req_take(count)?;
        Ok(())
    }

    fn mesh_byte_order(&mut self, ctx: &DecodeContext<'_>, property_id: &str) -> Result<ByteOrder, CodecError> {
        let start = self.view.position();
        self.view.req_take(mesh_hdr::INFORMATION)?;
        self.view
            .seek(start)
            .ok_or_else(|| CodecError::Malformed("mesh header window is inconsistent".into()))?;
        if self.view.u32_le() == Some(MESH_MAGIC) && self.view.u32_le() == Some(MESH_VERSION) {
            return Ok(ByteOrder::Little);
        }
        self.view
            .seek(start)
            .ok_or_else(|| CodecError::Malformed("mesh header window is inconsistent".into()))?;
        if self.view.u32_be() == Some(MESH_MAGIC) && self.view.u32_be() == Some(MESH_VERSION) {
            return Ok(ByteOrder::Big);
        }
        Err(CodecError::NotImplemented(retained_format(ctx, format_args!(
            "FCStd mesh payload {property_id} has an unsupported header or version"
        ), "FreeCAD unsupported mesh header")?))
    }

    fn u32(&mut self, order: ByteOrder) -> Result<u32, CodecError> {
        Ok(match order {
            ByteOrder::Little => self.view.req_u32_le()?,
            ByteOrder::Big => self.view.req_u32_be()?,
        })
    }

    fn f32(&mut self, order: ByteOrder) -> Result<f32, CodecError> {
        Ok(match order {
            ByteOrder::Little => self.view.req_f32_le()?,
            ByteOrder::Big => self.view.req_f32_be()?,
        })
    }

    fn count(&mut self, order: ByteOrder, label: &str) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(order)?)
            .map_err(|_| CodecError::malformed(format_args!("{label} does not fit usize")))?;
        if count > MAX_ELEMENTS {
            return Err(CodecError::malformed(format_args!("{label} exceeds limit")));
        }
        Ok(count)
    }

    fn index(
        &mut self,
        order: ByteOrder,
        point_count: usize,
        label: &str,
    ) -> Result<u32, CodecError> {
        let index = self.u32(order)?;
        if usize::try_from(index).map_or(true, |index| index >= point_count) {
            return Err(CodecError::malformed(format_args!(
                "{label} is out of bounds"
            )));
        }
        Ok(index)
    }

    fn point3(&mut self, order: ByteOrder, label: &str) -> Result<FinitePoint3, CodecError> {
        let values = [self.f32(order)?, self.f32(order)?, self.f32(order)?];
        let [Some(x), Some(y), Some(z)] = values.map(FiniteBinary32::new) else {
            return Err(CodecError::malformed(format_args!(
                "{label} contains a non-finite coordinate"
            )));
        };
        Ok(FinitePoint3::from_coordinates(x.into(), y.into(), z.into()))
    }

    fn finish(&self, label: &str) -> Result<(), CodecError> {
        if !self.view.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "{label} has {} trailing bytes",
                self.remaining()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{association, parse_mesh, parse_points, ByteOrder, Reader};
    use crate::layout::mesh_kernel_side_entry_header as mesh_hdr;
    use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
    use crate::test_support::test_archive::archive_entries;
    use crate::FcstdCodec;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    fn resource_test_property() -> PropertyRecord {
        PropertyRecord {
            id: "fcstd:native:property#Geometry".to_owned(),
            owner: "fcstd:native:object#Geometry".to_owned(),
            name: "Geometry".to_owned(),
            type_name: "Points::PropertyPointKernel".to_owned(),
            family: PropertyFamily::Unknown,
            status: None,
            body: PropertyBody::Transient,
            order: 0,
            xml: RetainedXml::from_text("<Property><Points/></Property>".to_owned(), 0)
                .expect("valid XML span"),
        }
    }

    #[test]
    fn unsupported_mesh_header_refuses_diagnostic_at_retained_limit() {
        let property = resource_test_property();
        crate::test_support::assert_retained_refusal_at(&[0; 8], "FreeCAD unsupported mesh header",
            |ctx| parse_mesh(ctx, &property, &[0; 8]));
    }

    #[test]
    fn geometry_side_entry_error_refuses_at_retained_limit() {
        let mut property = resource_test_property();
        property.body = PropertyBody::Persisted { values: Vec::new(), links: Vec::new(),
            side_entries: vec!["one".into(), "two".into()], dynamic: None };
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD geometry side-entry error",
            |ctx| super::transfer(ctx, &mut cadmpeg_ir::CadIr::empty(),
                std::slice::from_ref(&property), &[]));
    }

    #[test]
    fn geometry_missing_side_entry_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text("<Property><Points file=\"missing.pts\"/></Property>".into(), 0)
            .expect("valid XML span");
        property.body = PropertyBody::Persisted { values: Vec::new(), links: Vec::new(),
            side_entries: vec!["missing.pts".into()], dynamic: None };
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD geometry missing side-entry error",
            |ctx| super::transfer(ctx, &mut cadmpeg_ir::CadIr::empty(),
                std::slice::from_ref(&property), &[]));
    }

    #[test]
    fn geometry_value_root_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text("<Property><Points/><Points/></Property>".into(), 0)
            .expect("valid XML span");
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD geometry root error",
            |ctx| super::validate_value_root(ctx, &property, "Points"));
    }

    #[test]
    fn invalid_point_xml_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text("<Property><Points>".into(), 0)
            .expect("retained XML span");
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD point XML error",
            |ctx| super::point_transform(ctx, &property));
    }

    #[test]
    fn invalid_geometry_xml_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text("<Property><Points>".into(), 0)
            .expect("retained XML span");
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD geometry XML error",
            |ctx| super::validate_value_root(ctx, &property, "Points"));
    }

    #[test]
    fn mesh_vertex_collection_limit_refuses_before_allocation() {
        let mut mesh = Vec::new();
        mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&0_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&mesh, &arena, &policy)
            .expect("root mesh is within the input limit");
        assert!(matches!(parse_mesh(&ctx, &resource_test_property(), &mesh),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD mesh vertices"));
    }

    #[test]
    fn point_cloud_collection_limit_refuses_before_allocation() {
        let points = 1_u32.to_le_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&points, &arena, &policy)
            .expect("root points are within the input limit");
        assert!(matches!(parse_points(&ctx, &resource_test_property(), &points),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD point-cloud points"));
    }

    #[test]
    fn point_cloud_identity_refuses_at_retained_limit() {
        let property = resource_test_property();
        let mut points = Vec::new();
        points.extend_from_slice(&1_u32.to_le_bytes());
        points.extend_from_slice(&[0; 12]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::native::model_id(
            "point", &property.id, "0").len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&points, &arena, &policy)
            .expect("root points are within the input limit");
        assert!(matches!(parse_points(&ctx, &property, &points),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD model identity"));
    }

    #[test]
    fn geometry_association_refuses_on_retained_limit() {
        let property = resource_test_property();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = property.owner.len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within the input limit");
        assert!(matches!(association(&ctx, &property), Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD geometry object identity"));
    }

    #[test]
    fn mesh_facet_collection_limit_refuses_before_triangle_allocation() {
        let mut mesh = Vec::new();
        mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        mesh.extend_from_slice(&0_u32.to_le_bytes());
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; 24]);
        let property = PropertyRecord {
            id: "fcstd:native:property#Mesh".to_owned(),
            owner: "fcstd:native:object#Mesh".to_owned(),
            name: "Mesh".to_owned(),
            type_name: "Mesh::PropertyMeshKernel".to_owned(),
            family: PropertyFamily::Unknown,
            status: None,
            body: PropertyBody::Transient,
            order: 0,
            xml: RetainedXml::from_text("<Mesh/>".to_owned(), 0).expect("valid XML span"),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&mesh, &arena, &policy)
            .expect("root mesh is within the input limit");
        assert!(matches!(
            parse_mesh(&ctx, &property, &mesh),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD mesh facets"
        ));
    }

    #[test]
    fn bounded_application_geometry_reader_rejects_counts_indices_and_truncation() {
        let excessive_bytes = u32::MAX.to_le_bytes();
        let mut excessive = Reader::new(&excessive_bytes);
        assert!(excessive
            .count(ByteOrder::Little, "application count")
            .is_err());

        let invalid_index_bytes = 3_u32.to_le_bytes();
        let mut invalid_index = Reader::new(&invalid_index_bytes);
        assert!(invalid_index
            .index(ByteOrder::Little, 3, "application index")
            .is_err());

        let mut truncated = Reader::new(&[0; 11]);
        assert!(truncated
            .point3(ByteOrder::Little, "application point")
            .is_err());
    }

    #[test]
    pub(crate) fn transfers_application_mesh_and_transformed_point_cloud_payloads() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Cloud" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Mesh"><Properties Count="1"><Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="MeshKernel.bms"/></Property></Properties></Object>
 <Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property></Properties></Object>
</ObjectData></Document>"#;
        let mut mesh = Vec::new();
        mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        mesh.extend_from_slice(&3_u32.to_le_bytes());
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        let mut points = 2_u32.to_le_bytes().to_vec();
        for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
            points.extend_from_slice(&value.to_le_bytes());
        }
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("MeshKernel.bms", &mesh),
                    ("Cloud", &points),
                ])),
                &DecodeOptions::default(),
            )
            .expect("application geometry");
        assert_eq!(result.ir().model.tessellations.len(), 1);
        let mesh = &result.ir().model.tessellations[0];
        assert_eq!(mesh.triangles(), [[0, 1, 2]]);
        assert_eq!(
            mesh.source_object
                .as_ref()
                .map(|source| source.object_id.as_str()),
            Some("fcstd:native:object#Mesh")
        );
        assert_eq!(result.ir().model.points.len(), 2);
        assert_eq!(
            result.ir().model.points[0].position().get(),
            cadmpeg_ir::math::Point3::new(11.0, 22.0, 33.0)
        );
        assert_eq!(
            result.ir().model.points[1].position().get(),
            cadmpeg_ir::math::Point3::new(9.0, 18.0, 27.0)
        );
        assert!(result.report().geometry_transferred());
        assert!(result.report().losses.is_empty());
    }

    #[test]
    fn refuses_a_transformed_point_cloud_position_that_overflows_to_non_finite() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Points::Feature" name="Cloud" id="1"/></Objects>
<ObjectData Count="1">
 <Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud" mtrx="1e300 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/></Property></Properties></Object>
</ObjectData></Document>"#;
        let mut points = 1_u32.to_le_bytes().to_vec();
        for value in [1e30_f32, 0.0, 0.0] {
            points.extend_from_slice(&value.to_le_bytes());
        }
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("Cloud", &points),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("transformed point-cloud position");

        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                if message == "transformed point-cloud point contains a non-finite coordinate"
        ));
    }

    #[test]
    fn rejects_multiple_side_entries_for_one_typed_geometry_property() {
        for (object_type, property_type, values) in [
            (
                "Mesh::Feature",
                "Mesh::PropertyMeshKernel",
                r#"<Mesh file="first"/><Mesh file="second"/>"#,
            ),
            (
                "Points::Feature",
                "Points::PropertyPointKernel",
                r#"<Points file="first"/><Points file="second"/>"#,
            ),
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="{object_type}" name="Geometry"/></Objects>
<ObjectData Count="1"><Object name="Geometry"><Properties Count="1"><Property name="Geometry" type="{property_type}">{values}</Property></Properties></Object></ObjectData>
</Document>"#
            );
            let error = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        ("first", b""),
                        ("second", b""),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect_err("multiple typed geometry entries");

            assert!(matches!(
                error,
                cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                    if message.contains("references more than one side entry")
            ));
        }
    }

    #[test]
    fn rejects_unowned_side_entry_attributes() {
        for (object_type, property_type, values, entry) in [
            (
                "Mesh::Feature",
                "Mesh::PropertyMeshKernel",
                r#"<Mesh/><Extra file="payload"/>"#,
                "payload",
            ),
            (
                "Points::Feature",
                "Points::PropertyPointKernel",
                r#"<Points/><Extra file="payload"/>"#,
                "payload",
            ),
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="{object_type}" name="Geometry"/></Objects>
<ObjectData Count="1"><Object name="Geometry"><Properties Count="1"><Property name="Geometry" type="{property_type}">{values}</Property></Properties></Object></ObjectData>
</Document>"#
            );
            let error = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        (entry, b"payload"),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect_err("unowned geometry side entry");

            assert!(matches!(
                error,
                cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                    if message.contains("unowned side-entry reference")
            ));
        }
    }

    #[test]
    fn rejects_multiple_value_roots_with_one_side_entry() {
        for (object_type, property_type, values, entry) in [
            (
                "Mesh::Feature",
                "Mesh::PropertyMeshKernel",
                r#"<Mesh/><Mesh file="payload"/>"#,
                "payload",
            ),
            (
                "Points::Feature",
                "Points::PropertyPointKernel",
                r#"<Points/><Points file="payload"/>"#,
                "payload",
            ),
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="{object_type}" name="Geometry"/></Objects>
<ObjectData Count="1"><Object name="Geometry"><Properties Count="1"><Property name="Geometry" type="{property_type}">{values}</Property></Properties></Object></ObjectData>
</Document>"#
            );
            let error = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        (entry, b""),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect_err("multiple typed geometry value roots");

            assert!(matches!(
                error,
                cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                    if message.contains("must contain exactly one")
            ));
        }
    }

    #[test]
    fn does_not_decode_custom_runtime_names_as_application_geometry() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Geometry"/></Objects>
<ObjectData Count="1"><Object name="Geometry"><Properties Count="1"><Property name="Payload" type="Vendor::PropertyMeshKernelAndPropertyPointKernel"><Mesh file="payload"/></Property></Properties></Object></ObjectData>
</Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("payload", b"not a geometry payload"),
                ])),
                &DecodeOptions::default(),
            )
            .expect("custom application property is retained");
        assert!(result.ir().model.tessellations.is_empty());
        assert!(result.ir().model.points.is_empty());
        let property = result
            .ir()
            .native
            .namespace("fcstd")
            .expect("namespace")
            .arena_as::<crate::native::PropertyRecord>("properties")
            .expect("properties")
            .into_iter()
            .find(|property| property.name == "Payload")
            .expect("property");
        assert_eq!(property.family, crate::native::PropertyFamily::Unknown);
    }
}
