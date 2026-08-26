// SPDX-License-Identifier: Apache-2.0
//! Transfer of application-owned mesh and point payloads.

use cadmpeg_core::decode::{BoundedCount, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::SourceObjectAssociation;

use crate::layout::mesh_facet;
use crate::layout::mesh_kernel_side_entry_header as mesh_hdr;
use crate::native::{EntryRecord, PropertyRecord};

const MAX_ELEMENTS: usize = 1_000_000;
const MESH_MAGIC: u32 = mesh_hdr::MAGIC_VALUE;
const MESH_VERSION: u32 = mesh_hdr::VERSION_VALUE;

pub(crate) fn transfer(
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
        if property.side_entries.len() > 1 {
            return Err(CodecError::malformed(format_args!(
                "geometry property {} references more than one side entry",
                property.id
            )));
        }
        let root_entry = validate_value_root(property, geometry_kind.value_tag())?;
        let side_entry_matches_root = property.side_entries.len()
            == usize::from(root_entry.is_some())
            && property.side_entries.first() == root_entry.as_ref();
        if !side_entry_matches_root {
            return Err(CodecError::Malformed(
                "geometry property has an unowned side-entry reference".into(),
            ));
        }
        let Some(entry_name) = root_entry else {
            continue;
        };
        let entry = entries
            .iter()
            .find(|entry| entry.name == *entry_name)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "geometry property {} references missing side entry {entry_name}",
                    property.id
                ))
            })?;
        if geometry_kind == GeometryKind::Mesh {
            ir.model
                .tessellations
                .push(parse_mesh(property, &entry.data)?);
            transferred = true;
        } else if geometry_kind == GeometryKind::Points {
            ir.model.points.extend(parse_points(property, &entry.data)?);
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
    property: &PropertyRecord,
    expected_tag: &str,
) -> Result<Option<String>, CodecError> {
    let document = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "invalid geometry property XML {}: {error}",
            property.id
        ))
    })?;
    let roots = document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.has_tag_name(expected_tag))
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "geometry property {} must contain exactly one {expected_tag} value root, found {}",
            property.id,
            roots.len()
        )));
    }
    let root = roots[0];
    Ok(root
        .attribute("file")
        .filter(|value| !value.is_empty())
        .map(str::to_owned))
}

fn association(property: &PropertyRecord) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "fcstd".into(),
        object_id: property.owner.clone(),
        name: Some(property.name.clone()),
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

fn parse_mesh(property: &PropertyRecord, bytes: &[u8]) -> Result<Tessellation, CodecError> {
    let mut reader = Reader::new(bytes);
    let byte_order = reader.mesh_byte_order(&property.id)?;
    reader.skip(mesh_hdr::LEN - mesh_hdr::INFORMATION)?;
    let point_count = reader.count(byte_order, "mesh point count")?;
    let facet_count = reader.count(byte_order, "mesh facet count")?;
    let vertices = (0..point_count)
        .map(|_| reader.point3(byte_order, "mesh point"))
        .collect::<Result<Vec<_>, _>>()?;
    // Each facet consumes three point indices and three neighbour indices (24 bytes),
    // so the declared count cannot exceed the unread payload.
    let facet_capacity = reader
        .counted(facet_count as u64, mesh_facet::LEN)
        .ok_or_else(|| {
            CodecError::Malformed("mesh facet count exceeds remaining payload".into())
        })?;
    let mut triangles = Vec::with_capacity(facet_capacity);
    for _ in 0..facet_count {
        let triangle = [
            reader.index(byte_order, point_count, "mesh facet point")?,
            reader.index(byte_order, point_count, "mesh facet point")?,
            reader.index(byte_order, point_count, "mesh facet point")?,
        ];
        for _ in 0..3 {
            let _ = reader.u32(byte_order)?;
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
    Ok(Tessellation {
        id: format!("{}:mesh", property.id),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: Some(association(property)),
        vertices,
        triangles,
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    })
}

fn parse_points(property: &PropertyRecord, bytes: &[u8]) -> Result<Vec<Point>, CodecError> {
    let mut reader = Reader::new(bytes);
    let count = reader.count(ByteOrder::Little, "point-cloud point count")?;
    let transform = point_transform(property)?;
    let source_object = association(property);
    let points = (0..count)
        .map(|index| {
            let position = reader.point3(ByteOrder::Little, "point-cloud point")?;
            Ok(Point {
                id: PointId(crate::native::model_id(
                    "point",
                    &property.id,
                    index.to_string(),
                )),
                position: transform_point(transform, position),
                source_object: Some(source_object.clone()),
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    reader.finish("point-cloud payload")?;
    Ok(points)
}

fn point_transform(property: &PropertyRecord) -> Result<[[f64; 4]; 4], CodecError> {
    let document = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "invalid point property XML {}: {error}",
            property.id
        ))
    })?;
    let Some(text) = document
        .root_element()
        .children()
        .find(|node| node.is_element() && node.has_tag_name("Points"))
        .and_then(|node| node.attribute("mtrx"))
    else {
        return Ok(identity());
    };
    let values = text
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CodecError::Malformed("invalid point-cloud transform scalar".into()))?;
    if values.len() != 16 || values.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::Malformed(
            "point-cloud transform must contain 16 finite scalars".into(),
        ));
    }
    Ok(std::array::from_fn(|row| {
        std::array::from_fn(|column| values[row * 4 + column])
    }))
}

fn identity() -> [[f64; 4]; 4] {
    std::array::from_fn(|row| std::array::from_fn(|column| f64::from(row == column)))
}

fn transform_point(transform: [[f64; 4]; 4], point: Point3) -> Point3 {
    Point3::new(
        transform[0][0] * point.x
            + transform[0][1] * point.y
            + transform[0][2] * point.z
            + transform[0][3],
        transform[1][0] * point.x
            + transform[1][1] * point.y
            + transform[1][2] * point.z
            + transform[1][3],
        transform[2][0] * point.x
            + transform[2][1] * point.y
            + transform[2][2] * point.z
            + transform[2][3],
    )
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

    fn mesh_byte_order(&mut self, property_id: &str) -> Result<ByteOrder, CodecError> {
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
        Err(CodecError::NotImplemented(format!(
            "FCStd mesh payload {property_id} has an unsupported header or version"
        )))
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

    fn point3(&mut self, order: ByteOrder, label: &str) -> Result<Point3, CodecError> {
        let values = [self.f32(order)?, self.f32(order)?, self.f32(order)?];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(CodecError::malformed(format_args!(
                "{label} contains a non-finite coordinate"
            )));
        }
        Ok(Point3::new(
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
        ))
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
    use super::*;
    use crate::test_support::*;
    use crate::FcstdCodec;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

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
        assert_eq!(mesh.triangles, [[0, 1, 2]]);
        assert_eq!(
            mesh.source_object
                .as_ref()
                .map(|source| source.object_id.as_str()),
            Some("fcstd:native:object#Mesh")
        );
        assert_eq!(result.ir().model.points.len(), 2);
        assert_eq!(
            result.ir().model.points[0].position,
            cadmpeg_ir::math::Point3::new(11.0, 22.0, 33.0)
        );
        assert_eq!(
            result.ir().model.points[1].position,
            cadmpeg_ir::math::Point3::new(9.0, 18.0, 27.0)
        );
        assert!(result.report().geometry_transferred);
        assert!(result.report().losses.is_empty());
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
                cadmpeg_core::CodecError::Malformed(message)
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
                cadmpeg_core::CodecError::Malformed(message)
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
                cadmpeg_core::CodecError::Malformed(message)
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
