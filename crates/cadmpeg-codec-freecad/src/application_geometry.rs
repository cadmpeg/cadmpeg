// SPDX-License-Identifier: Apache-2.0
//! Transfer of application-owned mesh and point payloads.

use cadmpeg_core::decode::tree::AdmittedXml;
use cadmpeg_core::decode::{BoundedCount, DecodeContext, View};
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

const MAX_ELEMENTS: usize = 1_000_000;
const MESH_MAGIC: u32 = mesh_hdr::MAGIC_VALUE;
const MESH_VERSION: u32 = mesh_hdr::VERSION_VALUE;

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    admitted_entities: &mut u64,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut entry_index = None;
    let mut transferred = false;
    let mut property_sources = properties.iter();
    while property_sources.len() != 0 {
        let Some(property) = ctx.next_charged(&mut property_sources, "FreeCAD geometry properties")? else {
            break;
        };
        let geometry_kind = match property.type_name.as_str() {
            "Mesh::PropertyMeshKernel" => GeometryKind::Mesh,
            "Points::PropertyPointKernel" => GeometryKind::Points,
            _ => continue,
        };
        if property.side_entries().len() > 1 {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!(
                    "geometry property {} references more than one side entry",
                    property.id
                ),
                "FreeCAD geometry side-entry error",
            )?));
        }
        let admitted_document = ctx
            .parse_xml(property.xml.text(), "FreeCAD XML tree")
            .or_else(|error| {
                let CodecError::Malformed(error) = error else {
                    return Err(error);
                };
                Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("invalid geometry property XML {}: {error}", property.id),
                    "FreeCAD geometry XML error",
                )?))
            })?;
        let root_entry =
            ctx.with_scoped_storage("FreeCAD geometry side-entry lookup", || {
                validate_value_root(
                    ctx,
                    property,
                    geometry_kind.value_tag(),
                    Some(admitted_document.document()),
                )
            })?;
        let _root_storage = root_entry.1;
        let root_entry = root_entry.0;
        let side_entry_matches_root = property.side_entries().len()
            == usize::from(root_entry.is_some())
            && match (property.side_entries().first(), root_entry.as_ref()) {
                (Some(side), Some(root)) => {
                    ctx.equal(side, root, "FreeCAD geometry side-entry equality")?
                }
                (None, None) => true,
                _ => false,
            };
        if !side_entry_matches_root {
            return Err(CodecError::Malformed(
                "geometry property has an unowned side-entry reference".into(),
            ));
        }
        let Some(entry_name) = root_entry else {
            continue;
        };
        if entry_index.is_none() {
            entry_index = Some(ctx.collect_scoped_btree_map(
                entries.iter().map(|entry| (entry.name(), entry)),
                "FreeCAD geometry entry index",
            )?);
        }
        let entry = entry_index
            .as_ref()
            .map(|(index, _storage)| {
                ctx.get_btree_map(index, entry_name.as_str(), "FreeCAD geometry entry lookup")
            })
            .transpose()?
            .flatten();
        let Some(entry) = entry else {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!(
                    "geometry property {} references missing side entry {entry_name}",
                    property.id
                ),
                "FreeCAD geometry missing side-entry error",
            )?));
        };
        if geometry_kind == GeometryKind::Mesh {
            drop(admitted_document);
            ctx.reserve_vec(&mut ir.model.tessellations, 1, "FreeCAD mesh tessellations")?;
            ir.model
                .tessellations
                .push(parse_mesh(ctx, property, entry.data())?);
            transferred = true;
        } else if geometry_kind == GeometryKind::Points {
            parse_points(
                ctx,
                property,
                entry.data(),
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
                admitted_entities,
                Some(admitted_document),
                &mut ir.model.points,
            )?;
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
    document: Option<&roxmltree::Document<'_>>,
) -> Result<Option<String>, CodecError> {
    let admitted_document;
    let document = if let Some(document) = document {
        document
    } else {
        admitted_document = ctx
            .parse_xml(property.xml.text(), "FreeCAD XML tree")
            .or_else(|error| {
                let CodecError::Malformed(error) = error else {
                    return Err(error);
                };
                Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("invalid geometry property XML {}: {error}", property.id),
                    "FreeCAD geometry XML error",
                )?))
            })?;
        admitted_document.document()
    };
    let property_root = ctx.xml_root_element(document, "FreeCAD geometry property root")?;
    let mut roots = property_root.children();
    let mut root = None;
    let mut count = 0;
    while let Some(node) = ctx.next_charged(&mut roots, "FreeCAD geometry value roots")? {
        if ctx.xml_has_tag_name(node, expected_tag, "FreeCAD geometry value tag")? {
            root = Some(node);
            count += 1;
        }
    }
    let Some(root) = root.filter(|_| count == 1) else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "geometry property {} must contain exactly one {expected_tag} value root, found {}",
                property.id, count
            ),
            "FreeCAD geometry root error",
        )?));
    };
    ctx.xml_attribute(root, "file", "FreeCAD geometry file attribute")?
        .filter(|value| !value.is_empty())
        .map(|value| ctx.copy_retained_text(value, "FreeCAD geometry side-entry name"))
        .transpose()
}

fn association(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<SourceObjectAssociation, CodecError> {
    Ok(SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(&property.owner, "FreeCAD geometry object identity")?,
            "validate nonblank text",
        )?
        .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
        name: Some(ctx.copy_retained_text(&property.name, "FreeCAD geometry property name")?),
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
    let vertex_bytes = point_count
        .checked_mul(12)
        .and_then(|len| {
            facet_count
                .checked_mul(mesh_facet::LEN)
                .and_then(|facets| len.checked_add(facets))
        })
        .and_then(|len| len.checked_add(24))
        .ok_or_else(|| CodecError::malformed("mesh population extent overflows"))?;
    if reader.remaining() < vertex_bytes {
        return Err(CodecError::malformed(
            "mesh population exceeds remaining payload",
        ));
    }
    let mut vertices = ctx.collection_vec(point_count, "FreeCAD mesh vertices")?;
    let mut vertex_sources = 0..point_count;
    while vertex_sources.len() != 0 {
        let Some(_) = ctx.next_charged(&mut vertex_sources, "FreeCAD mesh vertex visits")? else {
            break;
        };
        vertices.push(reader.point3(byte_order, "mesh point")?);
    }
    // Each facet consumes three point indices and three neighbour indices (24 bytes),
    // so the declared count cannot exceed the unread payload.
    let facet_capacity = reader
        .counted(
            cadmpeg_core::decode::u64_from_index(facet_count),
            mesh_facet::LEN,
        )
        .ok_or_else(|| {
            CodecError::Malformed("mesh facet count exceeds remaining payload".into())
        })?;
    let mut triangles = ctx.collection_vec(facet_capacity, "FreeCAD mesh facets")?;
    let mut facet_sources = 0..facet_count;
    while facet_sources.len() != 0 {
        let Some(_) = ctx.next_charged(&mut facet_sources, "FreeCAD mesh facet visits")? else {
            break;
        };
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
        cadmpeg_ir::tessellation::TessellationId::mint(ctx.retained_suffix(
            &property.id,
            ":mesh",
            "FreeCAD mesh identity",
        )?)
        .map_err(|error| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("{error}"),
                "FreeCAD mesh diagnostic",
            )
        })?,
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices,
            triangles,
        },
        Vec::new(),
    )
    .map_err(|error| {
        crate::resource::malformed_charged(ctx, format_args!("{error}"), "FreeCAD mesh diagnostic")
    })?
    .with_source_object(Some(association(ctx, property)?)))
}

fn parse_points(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    bytes: &[u8],
    current_entities: u64,
    admitted_entities: &mut u64,
    document: Option<AdmittedXml<'_, '_>>,
    points: &mut Vec<Point>,
) -> Result<(), CodecError> {
    let mut reader = Reader::new(bytes);
    let count = reader.count(ByteOrder::Little, "point-cloud point count")?;
    reader
        .counted(cadmpeg_core::decode::u64_from_index(count), 12)
        .ok_or_else(|| CodecError::malformed("point-cloud count exceeds remaining payload"))?;
    let population = current_entities
        .checked_add(cadmpeg_core::decode::u64_from_index(count))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("FreeCAD point-cloud entities", u64::MAX, u64::MAX)
        })?;
    ctx.admit_entities(
        population,
        admitted_entities,
        "FreeCAD point-cloud entities",
    )?;
    let transform = point_transform(ctx, property, document.as_ref().map(AdmittedXml::document))?;
    drop(document);
    ctx.reserve_vec(points, count, "FreeCAD point-cloud points")?;
    let mut point_sources = 0..count;
    while point_sources.len() != 0 {
        let Some(index) = ctx.next_charged(&mut point_sources, "FreeCAD point-cloud point visits")? else {
            break;
        };
        let ordinal =
            ctx.format_scoped(format_args!("{index}"), "FreeCAD point ordinal")?;
        let _ordinal_storage = ordinal.1;
        let ordinal = ordinal.0;
        let position = reader.point3(ByteOrder::Little, "point-cloud point")?;
        points.push(Point::new(
            PointId::mint(crate::native::model_id_charged(
                ctx,
                "point",
                &property.id,
                &ordinal,
            )?)
            .map_err(CodecError::malformed)?,
            transform_point(transform, position)?,
            Some(association(ctx, property)?),
        ));
    }
    reader.finish("point-cloud payload")?;
    Ok(())
}

fn point_transform(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    document: Option<&roxmltree::Document<'_>>,
) -> Result<[[FiniteReal; 4]; 4], CodecError> {
    let admitted_document;
    let document = if let Some(document) = document {
        document
    } else {
        admitted_document = ctx
            .parse_xml(property.xml.text(), "FreeCAD XML tree")
            .or_else(|error| {
                let CodecError::Malformed(error) = error else {
                    return Err(error);
                };
                Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("invalid point property XML {}: {error}", property.id),
                    "FreeCAD point XML error",
                )?))
            })?;
        admitted_document.document()
    };
    let property_root = ctx.xml_root_element(document, "FreeCAD point property root")?;
    let point_root = ctx.find_by(
        property_root.children(),
        |node| ctx.xml_has_tag_name(*node, "Points", "FreeCAD point value tag"),
        "FreeCAD point value root",
    )?;
    let text = point_root
        .map(|node| ctx.xml_attribute(node, "mtrx", "FreeCAD point matrix attribute"))
        .transpose()?
        .flatten();
    let Some(text) = text else {
        return Ok(identity());
    };
    let mut values = [0.0_f64; 16];
    let mut count = 0_usize;
    // Splitting reads each byte once, whitespace runs included.
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(text.len()),
        "FreeCAD point matrix token visits",
    )?;
    for token in text.split_whitespace() {
        let value = ctx
            .parse_text::<f64>(token, "FreeCAD point matrix scalar")?
            .map_err(|_| CodecError::Malformed("invalid point-cloud transform scalar".into()))?;
        if count < values.len() {
            values[count] = value;
        }
        count = count.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("point-cloud transform must contain 16 finite scalars".into())
        })?;
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

    fn mesh_byte_order(
        &mut self,
        ctx: &DecodeContext<'_>,
        property_id: &str,
    ) -> Result<ByteOrder, CodecError> {
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
        Err(CodecError::NotImplemented(ctx.format_retained(
            format_args!("FCStd mesh payload {property_id} has an unsupported header or version"),
            "FreeCAD unsupported mesh header",
        )?))
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
    fn empty_geometry_transfer_is_free_and_preserves_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut ir = cadmpeg_ir::CadIr::empty();
        assert!(!super::transfer(&ctx, &mut ir, &[], &[], &mut 0).expect("empty geometry"));
        assert!(ir.model.points.is_empty());
        assert!(ir.model.tessellations.is_empty());
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior geometry refusal")
            .expect_err("work limit") else { panic!("resource refusal") };
        assert!(matches!(super::transfer(&ctx, &mut ir, &[], &[], &mut 0),
            Err(CodecError::ResourceLimit(repeated)) if repeated == original));
    }

    #[test]
    fn malformed_first_geometry_property_skips_unvisited_suffix() {
        let mut property = resource_test_property();
        let budget = crate::test_support::with_service_context(&[], |ctx| {
            assert!(!super::transfer(ctx, &mut cadmpeg_ir::CadIr::empty(),
                std::slice::from_ref(&property), &[], &mut 0).expect("short geometry"));
            ctx.format_retained(format_args!(
                "geometry property {} references more than one side entry", property.id,
            ), "FreeCAD geometry side-entry error").expect("diagnostic oracle");
            let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "geometry work oracle")
                .expect_err("work overflow") else { panic!("resource refusal") };
            limit.used
        });
        property.body = PropertyBody::Persisted {
            values: Vec::new(), links: Vec::new(), side_entries: vec!["A".into(), "B".into()],
            dynamic: None,
        };
        let count = usize::try_from(budget).expect("finite budget").checked_add(1)
            .expect("finite suffix count");
        let properties: Vec<_> = (0..count).map(|_| property.clone()).collect();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let error = super::transfer(&ctx, &mut cadmpeg_ir::CadIr::empty(), &properties, &[], &mut 0)
            .expect_err("first invalid side-entry cardinality");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "geometry property fcstd:native:property#Geometry references more than one side entry"));
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn nonfinite_first_point_skips_unvisited_population_suffix() {
        let property = resource_test_property();
        let short = [1_u32.to_le_bytes().as_slice(), &[0; 12]].concat();
        let budget = crate::test_support::with_service_context(&[], |ctx| {
            let mut points = Vec::new();
            parse_points(ctx, &property, &short, 0, &mut 0, None, &mut points)
                .expect("short point population");
            assert_eq!(points.len(), 1);
            let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "point work oracle")
                .expect_err("work overflow") else { panic!("resource refusal") };
            limit.used
        });
        let count = budget.checked_add(1).expect("finite point count");
        assert!(count <= u64::try_from(super::MAX_ELEMENTS).expect("codec population bound"));
        let mut bytes = u32::try_from(count).expect("bounded u32 count").to_le_bytes().to_vec();
        bytes.extend_from_slice(&f32::NAN.to_le_bytes());
        bytes.resize(4 + usize::try_from(count).expect("bounded count") * 12, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut points = Vec::new();
        let error = parse_points(&ctx, &property, &bytes, 0, &mut 0, None, &mut points)
            .expect_err("first non-finite point");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "point-cloud point contains a non-finite coordinate"));
        assert!(points.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn geometry_without_payload_skips_entry_index() {
        let property = resource_test_property();
        let entry = crate::test_support::entry_record(
            "fcstd:native:entry#unused".into(),
            "unused".into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            Vec::new(),
            vec![1],
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "FreeCAD geometry entry index",
            None,
        );
        let mut ir = cadmpeg_ir::CadIr::empty();
        assert!(
            !super::transfer(&ctx, &mut ir, &[property], &[entry], &mut 0).expect("empty geometry")
        );
        assert!(ir.model.points.is_empty());
        assert!(ir.model.tessellations.is_empty());
    }

    #[test]
    fn geometry_population_work_refuses_at_caller_limit() {
        let mut mesh = Vec::new();
        mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; 12 + 24 + 24]);
        for operation in ["FreeCAD mesh vertex visits", "FreeCAD mesh facet visits"] {
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &mesh,
                operation,
                |ctx| parse_mesh(ctx, &resource_test_property(), &mesh),
            );
        }
        let mut points = 1_u32.to_le_bytes().to_vec();
        points.extend_from_slice(&[0; 12]);
        crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &points,
            "FreeCAD point-cloud point visits",
            |ctx| {
                parse_points(
                    ctx,
                    &resource_test_property(),
                    &points,
                    0,
                    &mut 0,
                    None,
                    &mut Vec::new(),
                )
            },
        );
    }

    #[test]
    fn geometry_xml_queries_refuse_at_caller_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text(r#"<Property><Points file="payload" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property>"#.into(), 0).expect("XML");
        for operation in [
            "FreeCAD geometry property root",
            "FreeCAD geometry value roots",
            "FreeCAD geometry file attribute",
        ] {
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &[],
                operation,
                |ctx| super::validate_value_root(ctx, &property, "Points", None),
            );
        }
        for operation in [
            "FreeCAD point value root",
            "FreeCAD point matrix attribute",
            "FreeCAD point matrix token visits",
            "FreeCAD point matrix scalar",
        ] {
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &[],
                operation,
                |ctx| super::point_transform(ctx, &property, None),
            );
        }
    }

    #[test]
    fn geometry_reuses_the_admitted_property_xml() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text(r#"<Property><Points file="payload" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property>"#.into(), 0).expect("XML");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let admitted = ctx
            .parse_xml(property.xml.text(), "FreeCAD XML tree")
            .expect("XML");
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "FreeCAD XML tree",
            None,
        );
        assert_eq!(
            super::validate_value_root(&ctx, &property, "Points", Some(admitted.document()))
                .expect("root"),
            Some("payload".into())
        );
        let transform = super::point_transform(&ctx, &property, Some(admitted.document()))
            .expect("transform without reparsing");
        assert_eq!(
            [
                transform[0][3].get(),
                transform[1][3].get(),
                transform[2][3].get()
            ],
            [10.0, 20.0, 30.0]
        );
    }

    #[test]
    fn point_records_release_admitted_xml_before_population() {
        let mut bytes = 256_u32.to_le_bytes().to_vec();
        bytes.resize(4 + 256 * 12, 0);
        let property = resource_test_property();
        let error = crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            &bytes,
            "FreeCAD point-cloud points",
            |ctx| {
                let document = ctx.parse_xml(property.xml.text(), "FreeCAD XML tree")?;
                ctx.with_scoped_storage("FreeCAD point test records", || {
                    parse_points(
                        ctx,
                        &property,
                        &bytes,
                        0,
                        &mut 0,
                        Some(document),
                        &mut Vec::new(),
                    )
                })
                .map(|_| ())
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.used == 0));
    }

    #[test]
    fn point_cloud_entities_refuse_before_records_and_are_not_admitted_twice() {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 24]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        let mut admitted = 0;
        assert!(
            matches!(parse_points(&ctx, &resource_test_property(), &bytes, 0, &mut admitted, None, &mut Vec::new()),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities && limit.additional == 2)
        );
        assert_eq!(admitted, 0);
        policy.limits.max_entities = 2;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        let mut points = Vec::new();
        parse_points(
            &ctx,
            &resource_test_property(),
            &bytes,
            0,
            &mut admitted,
            None,
            &mut points,
        )
        .expect("admitted points");
        assert_eq!(points.len(), 2);
        ctx.admit_entities(2, &mut admitted, "aggregate")
            .expect("already admitted");
        assert!(matches!(
            ctx.charge_entities(1, "next entity"),
            Err(CodecError::ResourceLimit(_))
        ));
    }

    #[test]
    fn unsupported_mesh_header_refuses_diagnostic_at_retained_limit() {
        let property = resource_test_property();
        crate::test_support::assert_retained_refusal_at(
            &[0; 8],
            "FreeCAD unsupported mesh header",
            |ctx| parse_mesh(ctx, &property, &[0; 8]),
        );
    }

    #[test]
    fn geometry_side_entry_error_refuses_at_retained_limit() {
        let mut property = resource_test_property();
        property.body = PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["one".into(), "two".into()],
            dynamic: None,
        };
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FreeCAD geometry side-entry error",
            |ctx| {
                super::transfer(
                    ctx,
                    &mut cadmpeg_ir::CadIr::empty(),
                    std::slice::from_ref(&property),
                    &[],
                    &mut 0,
                )
            },
        );
    }

    #[test]
    fn geometry_missing_side_entry_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text(
            "<Property><Points file=\"missing.pts\"/></Property>".into(),
            0,
        )
        .expect("valid XML span");
        property.body = PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["missing.pts".into()],
            dynamic: None,
        };
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FreeCAD geometry missing side-entry error",
            |ctx| {
                super::transfer(
                    ctx,
                    &mut cadmpeg_ir::CadIr::empty(),
                    std::slice::from_ref(&property),
                    &[],
                    &mut 0,
                )
            },
        );
    }

    #[test]
    fn geometry_value_root_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml = RetainedXml::from_text("<Property><Points/><Points/></Property>".into(), 0)
            .expect("valid XML span");
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FreeCAD geometry root error",
            |ctx| super::validate_value_root(ctx, &property, "Points", None),
        );
    }

    #[test]
    fn invalid_point_xml_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml =
            RetainedXml::from_text("<Property><Points>".into(), 0).expect("retained XML span");
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD point XML error", |ctx| {
            super::point_transform(ctx, &property, None)
        });
    }

    #[test]
    fn invalid_geometry_xml_refuses_diagnostic_at_retained_limit() {
        let mut property = resource_test_property();
        property.xml =
            RetainedXml::from_text("<Property><Points>".into(), 0).expect("retained XML span");
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD geometry XML error", |ctx| {
            super::validate_value_root(ctx, &property, "Points", None)
        });
    }

    #[test]
    fn truncated_mesh_is_malformed_before_vertex_admission() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        bytes.extend_from_slice(&1_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        crate::test_support::with_service_context(&bytes, |ctx| {
            assert!(matches!(parse_mesh(ctx, &resource_test_property(), &bytes),
                Err(CodecError::Malformed(message)) if message == "mesh population exceeds remaining payload"));
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        assert!(matches!(
            parse_mesh(&ctx, &resource_test_property(), &bytes),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_vertices_refuse_retained_storage_before_reading() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 36]);
        crate::test_support::assert_retained_refusal_at(&bytes, "FreeCAD mesh vertices", |ctx| {
            parse_mesh(ctx, &resource_test_property(), &bytes)
        });
    }

    #[test]
    fn mesh_vertex_collection_limit_refuses_before_allocation() {
        let mut mesh = Vec::new();
        mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        mesh.extend_from_slice(&1_u32.to_le_bytes());
        mesh.extend_from_slice(&0_u32.to_le_bytes());
        mesh.extend_from_slice(&[0; 36]);
        crate::test_support::assert_collection_refusal_at(&mesh, "FreeCAD mesh vertices", |ctx| {
            parse_mesh(ctx, &resource_test_property(), &mesh)
        });
    }

    #[test]
    fn point_cloud_collection_limit_refuses_before_allocation() {
        let mut points = 1_u32.to_le_bytes().to_vec();
        points.extend_from_slice(&[0; 12]);
        crate::test_support::assert_collection_refusal_at(
            &points,
            "FreeCAD point-cloud points",
            |ctx| {
                parse_points(
                    ctx,
                    &resource_test_property(),
                    &points,
                    0,
                    &mut 0,
                    None,
                    &mut Vec::new(),
                )
            },
        );
    }

    #[test]
    fn truncated_point_cloud_is_malformed_before_collection_admission() {
        let bytes = 1_000_000_u32.to_le_bytes();
        crate::test_support::with_service_context(&bytes, |ctx| {
            assert!(
                matches!(parse_points(ctx, &resource_test_property(), &bytes, 0, &mut 0, None, &mut Vec::new()),
                Err(CodecError::Malformed(message))
                    if message == "point-cloud count exceeds remaining payload")
            );
            assert_eq!(
                ctx.policy().limits.max_collection_items,
                DecodePolicy::service().limits.max_collection_items
            );
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        assert!(matches!(
            parse_points(
                &ctx,
                &resource_test_property(),
                &bytes,
                0,
                &mut 0,
                None,
                &mut Vec::new()
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn point_cloud_storage_refuses_before_record_construction() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 12]);
        crate::test_support::assert_retained_refusal_at(
            &bytes,
            "FreeCAD point-cloud points",
            |ctx| {
                parse_points(
                    ctx,
                    &resource_test_property(),
                    &bytes,
                    0,
                    &mut 0,
                    None,
                    &mut Vec::new(),
                )
            },
        );
    }

    #[test]
    fn point_cloud_identity_refuses_at_retained_limit() {
        let property = resource_test_property();
        let mut points = Vec::new();
        points.extend_from_slice(&1_u32.to_le_bytes());
        points.extend_from_slice(&[0; 12]);
        crate::test_support::assert_retained_refusal_at(&points, "FreeCAD model identity", |ctx| {
            parse_points(ctx, &property, &points, 0, &mut 0, None, &mut Vec::new())
        });
    }

    #[test]
    fn geometry_association_refuses_on_retained_limit() {
        let property = resource_test_property();
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FreeCAD geometry object identity",
            |ctx| association(ctx, &property),
        );
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
        crate::test_support::assert_collection_refusal_at(&mesh, "FreeCAD mesh facets", |ctx| {
            parse_mesh(ctx, &property, &mesh)
        });
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
