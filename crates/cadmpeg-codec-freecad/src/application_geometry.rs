// SPDX-License-Identifier: Apache-2.0
//! Transfer of application-owned mesh and point payloads.

pub(crate) mod parts;
mod sources;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};
use cadmpeg_ir::SourceObjectAssociation;

use crate::layout::mesh_facet;
use crate::layout::mesh_kernel_side_entry_header as mesh_hdr;
#[cfg(test)]
use crate::native::PropertyRecord;

const MAX_ELEMENTS: usize = 1_000_000;
const MESH_MAGIC: u32 = mesh_hdr::MAGIC_VALUE;
const MESH_VERSION: u32 = mesh_hdr::VERSION_VALUE;

/// Whether a kernel property has delivered its owning object's neutral geometry.
pub(crate) fn holds_property(ir: &CadIr, type_name: &str, owner: &str) -> bool {
    match type_name {
        "Mesh::PropertyMeshKernel" => ir.model.tessellations.iter().any(|mesh| {
            mesh.source_object
                .as_ref()
                .is_some_and(|source| source.object_id.as_str() == owner)
        }),
        "Points::PropertyPointKernel" => ir.model.points.iter().any(|point| {
            point
                .source_object
                .as_ref()
                .is_some_and(|source| source.object_id.as_str() == owner)
        }),
        _ => false,
    }
}

pub(crate) fn projects_object(object: &crate::native::ObjectRecord) -> bool {
    matches!(
        object.type_name.as_str(),
        "Mesh::Feature" | "Points::Feature"
    )
}

fn association(object: &str) -> Result<SourceObjectAssociation, CodecError> {
    Ok(SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new(object.to_owned())
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
        instance_path: Vec::new(),
    })
}

pub(crate) fn point_transform(
    values: &[crate::native::ValueRecord],
) -> Result<[[FiniteReal; 4]; 4], CodecError> {
    let Some(text) = values
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "Points")
        .and_then(|value| value.attributes.get("mtrx"))
    else {
        return Ok(identity());
    };
    let mut values = [0.0_f64; 16];
    let mut count = 0_usize;
    for token in text.split_whitespace() {
        let value = token
            .parse::<f64>()
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
pub(crate) fn transform_point(
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
    #[cfg(test)]
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
        }
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

    fn point3(&mut self, order: ByteOrder, label: &str) -> Result<FinitePoint3, CodecError> {
        let values = [self.f32(order)?, self.f32(order)?, self.f32(order)?];
        let [Some(x), Some(y), Some(z)] = values.map(FiniteBinary32::new) else {
            return Err(CodecError::malformed(format_args!(
                "{label} contains a non-finite coordinate"
            )));
        };
        Ok(FinitePoint3::from_coordinates(x.into(), y.into(), z.into()))
    }
}

#[cfg(test)]
fn parse_points(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    bytes: &[u8],
    current_entities: u64,
    admitted_entities: &mut u64,
) -> Result<Vec<cadmpeg_ir::topology::Point>, CodecError> {
    let view = ctx.test_input_view(bytes)?;
    let (_, owner) = ctx.record_owner("test:kernel#points", view, || Ok(()))?;
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let crate::native::entry_contents::EntryContents::Points { points } =
        parts::points_value(ctx, view, &owner, property.values(), &mut output)?
    else {
        return Err(CodecError::malformed("point parser produced a mesh kernel"));
    };
    let transform = point_transform(property.values())?;
    let mut output = Vec::new();
    parts::project_points(
        ctx,
        &points,
        current_entities,
        admitted_entities,
        |ordinal, point| {
            Ok(cadmpeg_ir::topology::Point::new(
                cadmpeg_ir::ids::PointId::mint(crate::native::model_id(
                    "point",
                    &property.id,
                    ordinal.to_string(),
                ))
                .map_err(CodecError::malformed)?,
                transform_point(transform, point)?,
                Some(association(&property.owner)?),
            ))
        },
        &mut output,
    )?;
    Ok(output)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{parse_points, ByteOrder, Reader};
    use crate::layout::mesh_kernel_side_entry_header as mesh_hdr;
    use crate::native::{PropertyBody, PropertyFamily, PropertyRecord};
    use crate::test_support::test_archive::archive_entries;
    use crate::FcstdCodec;
    use cadmpeg_core::decode::DecodePolicy;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    fn resource_test_property() -> crate::test_support::source_fidelity::PropertyFixture {
        crate::test_support::source_fidelity::PropertyFixture::new(
            PropertyRecord {
                dynamic: None,
                id: "fcstd:native:property#Geometry".to_owned(),
                owner: "fcstd:native:object#Geometry".to_owned(),
                name: "Geometry".to_owned(),
                type_name: "Points::PropertyPointKernel".to_owned(),
                family: PropertyFamily::Unknown,
                status: None,
                body: Some(PropertyBody::Transient),
                order: 0,
            },
            "<Property><Points/></Property>".to_owned(),
        )
    }

    #[test]
    fn point_cloud_entities_refuse_before_records_and_are_not_admitted_twice() {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 24]);
        let property = resource_test_property();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 1;
        let mut admitted = 0;
        crate::test_support::source_fidelity::with_property_reader(
            &bytes,
            &[&property],
            &policy,
            |ctx| {
                assert!(
                    matches!(parse_points(ctx, &property, &bytes, 0, &mut admitted),
                Err(CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities && limit.additional == 2)
                );
                assert_eq!(admitted, 0);
            },
        );
        policy.limits.max_entities = 2;
        crate::test_support::source_fidelity::with_property_reader(
            &bytes,
            &[&property],
            &policy,
            |ctx| {
                assert_eq!(
                    parse_points(ctx, &property, &bytes, 0, &mut admitted)
                        .expect("admitted points")
                        .len(),
                    2
                );
                ctx.admit_entities(2, &mut admitted, "aggregate")
                    .expect("already admitted");
                assert!(matches!(
                    ctx.charge_entities(1, "next entity"),
                    Err(CodecError::ResourceLimit(_))
                ));
            },
        );
    }

    fn assert_truncated_kernel(bytes: &[u8], mesh: bool) {
        use crate::native::EntryRecord;
        use cadmpeg_ir::source_fidelity::parts::PartState;
        let (object_type, runtime, tag, entry_level, operation) = if mesh {
            (
                "Mesh::Feature",
                "Mesh::PropertyMeshKernel",
                "Mesh",
                "freecad.mesh_kernel_entry",
                "FreeCAD mesh point slots",
            )
        } else {
            (
                "Points::Feature",
                "Points::PropertyPointKernel",
                "Points",
                "freecad.point_kernel_entry",
                "FreeCAD point-cloud slots",
            )
        };
        let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Geometry' type='{object_type}'/></Objects><ObjectData Count='1'><Object name='Geometry'><Properties Count='1'><Property name='Geometry' type='{runtime}'><{tag} file='Geometry.bin'/></Property></Properties></Object></ObjectData></Document>");
        let archive = archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("Geometry.bin", bytes),
        ]);
        let options = DecodeOptions::default();
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(&archive), &options)
            .unwrap();
        let entries = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<EntryRecord>("entries")
            .unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.name() == "Geometry.bin")
            .unwrap();
        assert!(entry.contents().is_empty());
        assert!(decoded.ir().model.tessellations.is_empty());
        assert!(decoded.ir().model.points.is_empty());
        let parts = decoded.source_fidelity().parts();
        let failed = parts
            .values()
            .filter(|part| part.state() == &(PartState::GivenUp {}))
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), entry_level);
        assert_eq!(failed[0].offset(), 0);
        assert_eq!(failed[0].byte_len(), u64::try_from(bytes.len()).unwrap());
        assert!(
            !parts.values().any(|part| matches!(
                part.level().as_str(),
                "freecad.mesh_point"
                    | "freecad.mesh_facet"
                    | "freecad.mesh_bounding_box"
                    | "freecad.point_kernel_point"
            )),
            "unadmitted slot storage creates no row parts"
        );
        let layout = &decoded.source_fidelity().record_layout()[entry.id()];
        let header = parts
            .values()
            .find(|part| {
                part.level().as_str() == "zip.local_header"
                    && part.stream() == layout.stream()
                    && part.offset() == layout.offset()
            })
            .unwrap();
        assert_eq!(header.state(), &(PartState::Decoded {}));
        assert_eq!(header.byte_len(), layout.byte_len());
        let errors = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
        let source = errors[0].provenance.as_ref().unwrap();
        assert_eq!(source.stream_name(), Some(failed[0].stream()));
        assert_eq!(source.offset, 0);
        assert_eq!(source.byte_len(), Some(u64::try_from(bytes.len()).unwrap()));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &options.policy)
                .unwrap();
        let count =
            cadmpeg_core::decode::View::u32_le_at(bytes, if mesh { mesh_hdr::LEN } else { 0 })
                .unwrap();
        assert_eq!(count, 1_000_000);
        let requested = u64::from(count)
            .checked_mul(cadmpeg_core::decode::u64_from_index(size_of::<
                Option<cadmpeg_ir::features::FinitePoint3>,
            >()))
            .unwrap();
        assert!(requested > ctx.per_expand_allowance());
        let resource = cadmpeg_core::decode::ResourceLimit {
            scope: cadmpeg_core::decode::LimitScope::Part,
            dimension: cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
            limit: ctx.per_expand_allowance(),
            used: 0,
            additional: requested,
            operation,
        };
        assert!(errors[0].message.contains(&resource.to_string()));
        let physical = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        assert_eq!(physical.open(&ctx, "Geometry.bin").unwrap().window(), bytes);
        assert_eq!(
            crate::source_archive::image(decoded.source_fidelity()).unwrap(),
            archive
        );
        crate::fidelity::b1::assert_classified_spans(&decoded);
    }

    #[test]
    fn truncated_mesh_is_malformed() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
        bytes.extend_from_slice(&1_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        assert_truncated_kernel(&bytes, true);
    }

    #[test]
    fn truncated_point_cloud_is_malformed() {
        let bytes = 1_000_000_u32.to_le_bytes();
        assert_truncated_kernel(&bytes, false);
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
        assert!(!result.report().losses.is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code.local_code() == "source.bytes-undecoded"
                && loss.severity == cadmpeg_ir::report::Severity::Warning
                && loss.provenance.is_some()));
        crate::fidelity::b1::assert_classified_spans(&result);
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
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("Cloud", &points),
                ])),
                &DecodeOptions::default(),
            )
            .expect("the actual overflowing point is a recoverable source row");

        // #215 §4.5 gives up the smallest actual source row and keeps its native kernel.
        assert!(result.ir().model.points.is_empty());
        let failed = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| {
                matches!(
                    part.state(),
                    cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), "freecad.point_kernel_point");
        assert_eq!(failed[0].byte_len(), 12);
        let kernels = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap();
        assert!(kernels.iter().flat_map(|entry| entry.contents()).any(|contents|
            matches!(contents,crate::native::entry_contents::EntryContents::Points{points} if points == &[None])
        ));
    }

    // #215 keeps a readable first value; only the extra direct value is bad.
    // The empty first payload in the first fixture independently fails its own
    // bounded binary carrier, which must have its own exact loss as well.
    fn assert_geometry_value_recovery(
        result: &cadmpeg_ir::codec::DecodeResult,
        document: &str,
        values: &str,
        binary: Option<&str>,
    ) {
        let first_end = values.find("/>").unwrap() + 2;
        let extra = &values[first_end..];
        let expected_offset = u64::try_from(document.find(values).unwrap() + first_end).unwrap();
        let failed = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| {
                matches!(
                    part.state(),
                    cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1 + usize::from(binary.is_some()));
        let value = failed
            .iter()
            .find(|part| part.level().as_str() == "freecad.property_value")
            .unwrap();
        assert_eq!(value.offset(), expected_offset);
        assert_eq!(value.byte_len(), u64::try_from(extra.len()).unwrap());
        if let Some(level) = binary {
            let payload = failed
                .iter()
                .find(|part| part.level().as_str() == level)
                .unwrap();
            assert_eq!(payload.offset(), 0);
            assert_eq!(payload.byte_len(), 0);
        }
        let errors = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), failed.len());
        for part in failed {
            let matching = errors
                .iter()
                .filter(|loss| {
                    loss.provenance.as_ref().is_some_and(|source| {
                        source.stream_name() == Some(part.stream())
                            && source.offset == part.offset()
                            && source.byte_len().unwrap_or(0) == part.byte_len()
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            assert_eq!(matching[0].severity, cadmpeg_ir::report::Severity::Error);
        }
        let namespace = result.ir().native.namespace("fcstd").unwrap();
        assert_eq!(
            namespace
                .arena_as::<crate::native::ObjectRecord>("objects")
                .unwrap()
                .len(),
            1
        );
        let properties = namespace
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].values().len(), 1);
        assert_eq!(properties[0].values()[0].order, 0);
        assert!(namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap()
            .iter()
            .flat_map(|entry| entry.contents())
            .all(|contents| !matches!(
                contents,
                crate::native::entry_contents::EntryContents::Mesh { .. }
                    | crate::native::entry_contents::EntryContents::Points { .. }
            )));
        assert!(result.ir().model.points.is_empty());
        assert!(result.ir().model.tessellations.is_empty());
        crate::fidelity::b1::assert_classified_spans(result);
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
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        ("first", b""),
                        ("second", b""),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect("#215 retains the readable geometry property and gives up the extra value");

            assert_geometry_value_recovery(
                &result,
                &document,
                values,
                Some(if property_type == "Mesh::PropertyMeshKernel" {
                    "freecad.mesh_kernel_header"
                } else {
                    "freecad.point_kernel_entry"
                }),
            );
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
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        (entry, b"payload"),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect("#215 gives up only the unowned extra value");

            assert_geometry_value_recovery(&result, &document, values, None);
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
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        (entry, b""),
                    ])),
                    &DecodeOptions::default(),
                )
                .expect("#215 retains the first value and gives up the extra value");

            assert_geometry_value_recovery(&result, &document, values, None);
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
