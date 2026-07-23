// SPDX-License-Identifier: Apache-2.0
//! Transfer of application-owned mesh and point payloads.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::wire::cursor::bounded_len;
use cadmpeg_ir::SourceObjectAssociation;

use crate::native::{EntryRecord, PropertyRecord};

const MAX_ELEMENTS: usize = 1_000_000;
const MESH_MAGIC: u32 = 0xa0b0_c0d0;
const MESH_VERSION: u32 = 0x0001_0000;

pub(crate) fn transfer(
    ir: &mut CadIr,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<bool, CodecError> {
    let mut transferred = false;
    for property in properties {
        let Some(entry_name) = property.side_entries.first() else {
            continue;
        };
        let entry = entries
            .iter()
            .find(|entry| entry.name == *entry_name)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "geometry property {} references missing side entry {entry_name}",
                    property.id
                ))
            })?;
        if property.type_name.contains("PropertyMeshKernel") {
            ir.model
                .tessellations
                .push(parse_mesh(property, &entry.data)?);
            transferred = true;
        } else if property.type_name.contains("PropertyPointKernel") {
            ir.model.points.extend(parse_points(property, &entry.data)?);
            transferred = true;
        }
    }
    Ok(transferred)
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
    let raw_magic = reader.array::<4>("mesh magic")?;
    let raw_version = reader.array::<4>("mesh version")?;
    let byte_order = if u32::from_le_bytes(raw_magic) == MESH_MAGIC
        && u32::from_le_bytes(raw_version) == MESH_VERSION
    {
        ByteOrder::Little
    } else if u32::from_be_bytes(raw_magic) == MESH_MAGIC
        && u32::from_be_bytes(raw_version) == MESH_VERSION
    {
        ByteOrder::Big
    } else {
        return Err(CodecError::NotImplemented(format!(
            "FCStd mesh payload {} has an unsupported header or version",
            property.id
        )));
    };
    reader.skip(256, "mesh information header")?;
    let point_count = reader.count(byte_order, "mesh point count")?;
    let facet_count = reader.count(byte_order, "mesh facet count")?;
    let vertices = (0..point_count)
        .map(|_| reader.point3(byte_order, "mesh point"))
        .collect::<Result<Vec<_>, _>>()?;
    // Each facet consumes three point indices and three neighbour indices (24 bytes),
    // so the declared count cannot exceed the unread payload.
    let facet_capacity =
        bounded_len(facet_count as u64, 24, reader.remaining()).ok_or_else(|| {
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
            let _ = reader.u32(byte_order, "mesh facet neighbour")?;
        }
        triangles.push(triangle);
    }
    for _ in 0..6 {
        let value = reader.f32(byte_order, "mesh bounding box")?;
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
        strip_lengths: Vec::new(),
        normals: Vec::new(),
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
        CodecError::Malformed(format!(
            "invalid point property XML {}: {error}",
            property.id
        ))
    })?;
    let Some(text) = document
        .descendants()
        .find(|node| node.has_tag_name("Points"))
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
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn array<const N: usize>(&mut self, label: &str) -> Result<[u8; N], CodecError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or_else(|| CodecError::Malformed(format!("{label} offset overflow")))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated {label}")))?;
        self.offset = end;
        value
            .try_into()
            .map_err(|_| CodecError::Malformed(format!("invalid {label} width")))
    }

    fn skip(&mut self, count: usize, label: &str) -> Result<(), CodecError> {
        self.offset = self
            .offset
            .checked_add(count)
            .and_then(|end| self.bytes.get(self.offset..end).map(|_| end))
            .ok_or_else(|| CodecError::Malformed(format!("truncated {label}")))?;
        Ok(())
    }

    fn u32(&mut self, order: ByteOrder, label: &str) -> Result<u32, CodecError> {
        let bytes = self.array::<4>(label)?;
        Ok(match order {
            ByteOrder::Little => u32::from_le_bytes(bytes),
            ByteOrder::Big => u32::from_be_bytes(bytes),
        })
    }

    fn f32(&mut self, order: ByteOrder, label: &str) -> Result<f32, CodecError> {
        Ok(f32::from_bits(self.u32(order, label)?))
    }

    fn count(&mut self, order: ByteOrder, label: &str) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(order, label)?)
            .map_err(|_| CodecError::Malformed(format!("{label} does not fit usize")))?;
        if count > MAX_ELEMENTS {
            return Err(CodecError::Malformed(format!("{label} exceeds limit")));
        }
        Ok(count)
    }

    fn index(
        &mut self,
        order: ByteOrder,
        point_count: usize,
        label: &str,
    ) -> Result<u32, CodecError> {
        let index = self.u32(order, label)?;
        if usize::try_from(index).map_or(true, |index| index >= point_count) {
            return Err(CodecError::Malformed(format!("{label} is out of bounds")));
        }
        Ok(index)
    }

    fn point3(&mut self, order: ByteOrder, label: &str) -> Result<Point3, CodecError> {
        let values = [
            self.f32(order, label)?,
            self.f32(order, label)?,
            self.f32(order, label)?,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(CodecError::Malformed(format!(
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
        if self.offset != self.bytes.len() {
            return Err(CodecError::Malformed(format!(
                "{label} has {} trailing bytes",
                self.bytes.len() - self.offset
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
