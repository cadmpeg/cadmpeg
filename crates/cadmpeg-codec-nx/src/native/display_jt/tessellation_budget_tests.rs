// SPDX-License-Identifier: Apache-2.0
//! Candidate storage and visited-prefix work for scene paths and tessellations.

use super::candidate_tests::{assert_released, coordinate_header, coordinates, vertex_header};
use super::*;

struct Fixture {
    meshes: Vec<DisplayJtPolygonMesh>,
    coordinates: Vec<DisplayJtVertexCoordinates>,
    coordinate_headers: Vec<DisplayJtVertexCoordinateArrayHeader>,
    vertex_headers: Vec<DisplayJtCompressedVertexRecordsHeader>,
    elements: Vec<DisplayJtShapeLodElement>,
    bindings: Vec<DisplayJtShapeLodBinding>,
    shapes: Vec<DisplayJtTriStripShapeNode>,
    bases: Vec<DisplayJtBaseNodeData>,
    instances: Vec<DisplayJtInstanceNode>,
    groups: Vec<DisplayJtGroupNodeData>,
    compressed: Vec<DisplayJtCompressedElement>,
    transforms: Vec<DisplayJtGeometricTransformAttribute>,
}

impl Fixture {
    fn new() -> Self {
        let mesh = DisplayJtPolygonMesh {
            id: "mesh".into(),
            topology: "topology".into(),
            coordinate_header: "coordinate-header".into(),
            polygons: vec![Polygon {
                corners: vec![(0, None), (0, None), (0, None)],
                group: 0,
                flags: 0,
            }],
            source_offset: 0,
        };
        let element = DisplayJtShapeLodElement {
            id: "shape".into(),
            segment: "lod".into(),
            ordinal: 0,
            object_type_id: [0; 16],
            object_id: 7,
            body_byte_len: 0,
            body_sha256: Sha256Digest::digest(&[]),
            source_offset: 0,
        };
        let binding = DisplayJtShapeLodBinding {
            id: "binding".into(),
            scene_segment: "scene".into(),
            table_version: 1,
            shape_node_object_id: 1,
            key_object_id: 1,
            key: "JT_LLPROP_SHAPEIMPL".into(),
            value_object_id: 2,
            state_flags: 0,
            property_version: 1,
            shape_segment: "lod".into(),
            payload_object_id: 7,
            reserved_value: 1,
            source_offset: 0,
        };
        let bounds = JtBounds::try_from([[0_f32; 3]; 2]).unwrap();
        let shape = DisplayJtTriStripShapeNode {
            id: "nx:display-jt:shape#1".into(),
            base_node: "base".into(),
            object_id: 1,
            reserved_bounds: bounds,
            untransformed_bounds: bounds,
            area: JtArea::new(0.).unwrap(),
            vertex_count_range: [0, 1],
            node_count_range: [0, 1],
            polygon_count_range: [0, 1],
            memory_byte_len: 0,
            compression_level: UnitBinary32::try_from(0.).unwrap(),
            vertex_version: JtVertexVersion::One,
            vertex_bindings: 0,
            vertex_quantization_bits: 0,
            normal_quantization_factor: 0,
            texture_quantization_bits: 0,
            color_quantization_bits: 0,
            source_offset: 0,
        };
        let base = DisplayJtBaseNodeData {
            id: "base".into(),
            element: "scene-element".into(),
            object_type_id: [0; 16],
            object_id: 1,
            version: 1,
            flags: 0,
            attribute_object_ids: vec![],
            family_data_byte_len: 0,
            family_data_sha256: Sha256Digest::digest(&[]),
            source_offset: 0,
        };
        let compressed = DisplayJtCompressedElement {
            id: "scene-element".into(),
            segment: "scene".into(),
            segment_type: 1,
            ordinal: 0,
            object_type_id: [0; 16],
            object_base_type: 2,
            object_id: 1,
            body_byte_len: 0,
            body_sha256: Sha256Digest::digest(&[]),
            inflated_offset: 0,
            source_offset: 0,
        };
        Self {
            meshes: vec![mesh],
            coordinates: vec![coordinates()],
            coordinate_headers: vec![coordinate_header()],
            vertex_headers: vec![vertex_header(0)],
            elements: vec![element],
            bindings: vec![binding],
            shapes: vec![shape],
            bases: vec![base],
            instances: vec![],
            groups: vec![],
            compressed: vec![compressed],
            transforms: vec![],
        }
    }

    fn inputs(&self) -> DisplayJtTessellationInputs<'_> {
        DisplayJtTessellationInputs {
            meshes: &self.meshes,
            coordinates: &self.coordinates,
            normals: &[],
            colors: &[],
            texture_coordinates: &[],
            vertex_flags: &[],
            vertex_headers: &self.vertex_headers,
            coordinate_headers: &self.coordinate_headers,
            shape_elements: &self.elements,
            bindings: &self.bindings,
            shape_nodes: &self.shapes,
            base_nodes: &self.bases,
            group_nodes: &self.groups,
            instance_nodes: &self.instances,
            transforms: &self.transforms,
            materials: &[],
            compressed_elements: &self.compressed,
        }
    }

    fn add_instance(&mut self, object_id: u32, child: u32) {
        let mut base = self.bases[0].clone();
        base.id = format!("instance-base-{object_id}");
        base.object_id = object_id;
        base.element = format!("instance-element-{object_id}");
        let mut element = self.compressed[0].clone();
        element.id.clone_from(&base.element);
        element.object_id = object_id;
        self.instances.push(DisplayJtInstanceNode {
            id: format!("nx:display-jt:instance#{object_id}"),
            base_node: base.id.clone(),
            object_id,
            version: 1,
            child_object_id: child,
            source_offset: 0,
        });
        self.bases.push(base);
        self.compressed.push(element);
    }
}

fn work_used(decode: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>) -> u64 {
    crate::test_support::with_decode_context(|ctx| {
        decode(ctx).unwrap();
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "measure JT work").unwrap_err()
        else {
            panic!("work measurement");
        };
        assert_eq!(limit.operation, "measure JT work");
        limit.used
    })
}

#[test]
fn tessellation_candidate_releases_valid_mesh_before_missing_second_owner() {
    let mut fixture = Fixture::new();
    fixture.add_instance(2, 1);
    crate::test_support::with_decode_context(|ctx| {
        let tessellations = display_jt_tessellations(ctx, &fixture.inputs()).unwrap();
        assert_eq!(tessellations.len(), 1);
        assert_eq!(
            tessellations[0]
                .0
                .source_object
                .as_ref()
                .unwrap()
                .instance_path,
            ["nx:display-jt:instance#2"]
        );
    });
    let mut missing = fixture.meshes[0].clone();
    missing.coordinate_header = "missing".into();
    fixture.meshes.push(missing);
    assert_released(|ctx| {
        assert!(display_jt_tessellations(ctx, &fixture.inputs())?.is_empty());
        Ok(())
    });
}

#[test]
fn instance_path_candidate_releases_id_when_later_attribute_is_ambiguous() {
    let mut fixture = Fixture::new();
    fixture.add_instance(2, 1);
    let attribute = DisplayJtGeometricTransformAttribute {
        id: "transform".into(),
        element: "scene-element".into(),
        object_id: 10,
        state_flags: 0,
        field_inhibit_flags: 0,
        stored_values_mask: 0,
        matrix: JtTransformMatrix::from_finite([
            [
                FiniteBinary32::ONE,
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
            ],
            [
                FiniteBinary32::ZERO,
                FiniteBinary32::ONE,
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
            ],
            [
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
                FiniteBinary32::ONE,
                FiniteBinary32::ZERO,
            ],
            [
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
                FiniteBinary32::ONE,
            ],
        ])
        .unwrap(),
        source_offset: 0,
    };
    fixture.transforms = vec![attribute.clone(), attribute];
    fixture.bases[0].attribute_object_ids = vec![10];
    assert_released(|ctx| {
        let inputs = fixture.inputs();
        let index = JtTessellationIndex::new(ctx, &inputs)?;
        let graph = JtSceneGraph::new(ctx, "scene", &inputs, &index)?.unwrap();
        assert!(graph.node_paths(ctx, 1)?.is_none());
        Ok(())
    });
}

#[test]
fn tessellation_mesh_rejection_does_not_visit_a_missing_owner_suffix() {
    let mut fixture = Fixture::new();
    fixture.meshes[0].coordinate_header = "missing".into();
    let prefix = work_used(|ctx| {
        assert!(display_jt_tessellations(ctx, &fixture.inputs())?.is_empty());
        Ok(())
    });
    fixture.meshes.extend(vec![fixture.meshes[0].clone(); 128]);
    let extended = work_used(|ctx| {
        assert!(display_jt_tessellations(ctx, &fixture.inputs())?.is_empty());
        Ok(())
    });
    assert_eq!(prefix, extended);
}

#[test]
fn tessellation_polygon_rejection_does_not_visit_a_nontriangle_suffix() {
    let mut fixture = Fixture::new();
    fixture.meshes[0].polygons[0].corners.pop();
    let prefix = work_used(|ctx| {
        assert!(display_jt_tessellations(ctx, &fixture.inputs())?.is_empty());
        Ok(())
    });
    let suffix = vec![fixture.meshes[0].polygons[0].clone(); 128];
    fixture.meshes[0].polygons.extend(suffix);
    let extended = work_used(|ctx| {
        assert!(display_jt_tessellations(ctx, &fixture.inputs())?.is_empty());
        Ok(())
    });
    assert_eq!(prefix, extended);
}

#[test]
fn tessellation_triangle_rejection_does_not_visit_a_missing_vertex_suffix() {
    let mut fixture = Fixture::new();
    fixture.vertex_headers[0].vertex_bindings = 0x40;
    let flags = [DisplayJtVertexFlags {
        id: "flags".into(),
        vertex_records_header: "vertex-header".into(),
        values: vec![0],
        byte_len: 0,
        source_offset: 0,
    }];
    fixture.meshes[0].polygons[0].corners[0].0 = 1;
    // Admission of polygons must occur even though the first rendered triangle fails.
    let prefix = work_used(|ctx| {
        let mut inputs = fixture.inputs();
        inputs.vertex_flags = &flags;
        assert!(display_jt_tessellations(ctx, &inputs)?.is_empty());
        Ok(())
    });
    let suffix = vec![fixture.meshes[0].polygons[0].clone(); 128];
    fixture.meshes[0].polygons.extend(suffix);
    // Building the rendered list visits all polygons; only the final triangle traversal stops early.
    let extended = work_used(|ctx| {
        let mut inputs = fixture.inputs();
        inputs.vertex_flags = &flags;
        assert!(display_jt_tessellations(ctx, &inputs)?.is_empty());
        Ok(())
    });
    // Each appended polygon has one visit and one scoped append. Amortized growth can add
    // movement work, so the direct boundary test below is the exact traversal proof.
    assert!(extended > prefix);
    let operation = "nx JT tessellation triangles";
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |ctx| {
            let mut inputs = fixture.inputs();
            inputs.vertex_flags = &flags;
            display_jt_tessellations(ctx, &inputs)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("triangle visit refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn duplicate_instance_rejection_does_not_visit_the_remaining_nodes() {
    let mut fixture = Fixture::new();
    fixture.add_instance(2, 1);
    fixture.instances.push(fixture.instances[0].clone());
    fixture
        .instances
        .extend(vec![fixture.instances[0].clone(); 128]);
    crate::test_support::with_decode_context(|setup| {
        let inputs = fixture.inputs();
        let index = JtTessellationIndex::new(setup, &inputs).unwrap();
        let prefix = work_used(|ctx| {
            let mut inputs = inputs;
            inputs.instance_nodes = &fixture.instances[..2];
            assert!(JtSceneGraph::new(ctx, "scene", &inputs, &index)?.is_none());
            Ok(())
        });
        let extended = work_used(|ctx| {
            assert!(JtSceneGraph::new(ctx, "scene", &inputs, &index)?.is_none());
            Ok(())
        });
        assert_eq!(prefix, extended);
    });
}

#[test]
fn unsupported_parent_rejection_admits_one_node_before_lookup() {
    let mut fixture = Fixture::new();
    fixture.groups.push(DisplayJtGroupNodeData {
        id: "group".into(),
        base_node: "base".into(),
        object_id: 1,
        version: 1,
        child_object_ids: vec![],
        family_data_byte_len: 0,
        family_data_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    });
    fixture.instances.push(DisplayJtInstanceNode {
        id: "nx:display-jt:instance#1".into(),
        base_node: "base".into(),
        object_id: 1,
        version: 1,
        child_object_id: 1,
        source_offset: 0,
    });
    for id in 2..130 {
        let mut base = fixture.bases[0].clone();
        base.object_id = id;
        base.id = format!("base-{id}");
        fixture.bases.push(base);
    }
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "nx JT parent index",
        |ctx| {
            let inputs = fixture.inputs();
            let index = JtTessellationIndex::new(ctx, &inputs)?;
            JtSceneGraph::new(ctx, "scene", &inputs, &index).map(|value| {
                assert!(value.is_none());
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("parent visit refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn cyclic_parent_rejection_does_not_visit_the_remaining_paths() {
    let fixture = Fixture::new();
    crate::test_support::with_decode_context(|setup| {
        let inputs = fixture.inputs();
        let index = JtTessellationIndex::new(setup, &inputs).unwrap();
        let mut graph = JtSceneGraph::new(setup, "scene", &inputs, &index)
            .unwrap()
            .unwrap();
        graph.parents.insert(1, vec![1]);
        let prefix = work_used(|ctx| {
            assert!(graph.node_paths(ctx, 1)?.is_none());
            Ok(())
        });
        graph.parents.insert(1, vec![1; 129]);
        let extended = work_used(|ctx| {
            assert!(graph.node_paths(ctx, 1)?.is_none());
            Ok(())
        });
        assert_eq!(prefix, extended);
    });
}

#[test]
fn ambiguous_attribute_rejection_does_not_visit_the_remaining_attributes() {
    let mut fixture = Fixture::new();
    fixture.bases[0].attribute_object_ids = vec![10];
    crate::test_support::with_decode_context(|setup| {
        let inputs = fixture.inputs();
        let index = JtTessellationIndex::new(setup, &inputs).unwrap();
        let mut graph = JtSceneGraph::new(setup, "scene", &inputs, &index)
            .unwrap()
            .unwrap();
        graph.transforms.insert(10, JtKeyed::Repeated);
        let prefix = work_used(|ctx| {
            assert!(graph.node_paths(ctx, 1)?.is_none());
            Ok(())
        });
        let mut base = fixture.bases[0].clone();
        base.attribute_object_ids.extend([10; 128]);
        graph.by_object.insert(1, &base);
        let extended = work_used(|ctx| {
            assert!(graph.node_paths(ctx, 1)?.is_none());
            Ok(())
        });
        assert_eq!(prefix, extended);
    });
}

#[test]
fn tessellation_path_rejection_admits_one_path_before_missing_vertex() {
    let mut fixture = Fixture::new();
    fixture.add_instance(2, 1);
    fixture.add_instance(3, 1);
    fixture.vertex_headers[0].vertex_bindings = 0x40;
    fixture.meshes[0].polygons[0].corners[0].0 = 1;
    let flags = [DisplayJtVertexFlags {
        id: "flags".into(),
        vertex_records_header: "vertex-header".into(),
        values: vec![0],
        byte_len: 0,
        source_offset: 0,
    }];
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "nx JT tessellation paths",
        |ctx| {
            let mut inputs = fixture.inputs();
            inputs.vertex_flags = &flags;
            display_jt_tessellations(ctx, &inputs).map(|value| {
                assert!(value.is_empty());
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("path visit refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn missing_child_rejection_admits_one_reference_before_lookup() {
    let mut fixture = Fixture::new();
    fixture.groups.push(DisplayJtGroupNodeData {
        id: "group".into(),
        base_node: "base".into(),
        object_id: 1,
        version: 1,
        child_object_ids: vec![99; 129],
        family_data_byte_len: 0,
        family_data_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    });
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "nx JT parent references",
        |ctx| {
            let inputs = fixture.inputs();
            let index = JtTessellationIndex::new(ctx, &inputs)?;
            JtSceneGraph::new(ctx, "scene", &inputs, &index).map(|value| {
                assert!(value.is_none());
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("child visit refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn tessellation_texture_component_width_does_not_charge_bounded_work() {
    let mut fixture = Fixture::new();
    fixture.vertex_headers[0].vertex_bindings = 0x100;
    for corner in &mut fixture.meshes[0].polygons[0].corners {
        corner.1 = Some(0);
    }
    let measure = |width| {
        let textures = [DisplayJtVertexTextureCoordinates {
            id: "texture".into(),
            vertex_records_header: "vertex-header".into(),
            channel: 0,
            values: vec![vec![FiniteBinary32::ZERO; width]],
            texture_coordinate_hash: 0,
            byte_len: 0,
            source_offset: 0,
        }];
        work_used(|ctx| {
            let mut inputs = fixture.inputs();
            inputs.texture_coordinates = &textures;
            let tessellations = display_jt_tessellations(ctx, &inputs)?;
            assert_eq!(tessellations.len(), 1);
            let tessellation = &tessellations[0].0;
            assert_eq!(tessellation.vertices().len(), 3);
            assert_eq!(tessellation.channels().len(), 1);
            let channel = &tessellation.channels()[0];
            assert_eq!(channel.item_size(), u32::try_from(width * 4).unwrap());
            assert_eq!(channel.data(), vec![0; width * 12]);
            Ok(())
        })
    };
    // Each channel has at most four scalar components. Both widths visit the
    // same input-sized records; only bounded scalar work and output size differ.
    assert_eq!(measure(1), measure(4));
}

#[test]
fn tessellation_fixed_identity_retains_exact_storage_without_work() {
    for (offset, object_id) in [(0, 0), (9, 10), (u64::MAX, u32::MAX)] {
        let expected = format!("nx:display-jt:tessellation#{offset}-{object_id}");
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
                policy.limits.max_retained_bytes = expected.len() as u64;
            },
            |ctx| {
                let id = retain_jt_tessellation_id(ctx, offset, object_id, None).unwrap();
                assert_eq!(id.as_str(), expected);
                assert!(ctx.resource_refusal().is_none());
            },
        );
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "nx JT tessellation identity",
            |ctx| retain_jt_tessellation_id(ctx, offset, object_id, None),
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("identity storage refusal");
        };
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, expected.len() as u64);
    }
}

#[test]
fn tessellation_shaded_rows_are_retained_and_admitted_directly() {
    use cadmpeg_core::decode::ResourceDimension;
    let mut fixture = Fixture::new();
    fixture.vertex_headers[0].vertex_bindings = 0x8;
    for corner in &mut fixture.meshes[0].polygons[0].corners {
        corner.1 = Some(0);
    }
    let normals = [DisplayJtVertexNormals {
        id: "normals".into(),
        vertex_records_header: "vertex-header".into(),
        normals: vec![[
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
            FiniteBinary32::ONE,
        ]],
        normal_hash: 0,
        byte_len: 0,
        source_offset: 0,
    }];
    let decode = |ctx: &DecodeContext<'_>| {
        let mut inputs = fixture.inputs();
        inputs.normals = &normals;
        display_jt_tessellations(ctx, &inputs)
    };
    crate::test_support::with_decode_context(|ctx| {
        let tessellations = decode(ctx).unwrap();
        assert_eq!(tessellations.len(), 1);
        let TessellationMesh::ShadedList {
            vertices,
            triangles,
        } = tessellations[0].0.mesh()
        else {
            panic!("shaded triangle-list mesh");
        };
        assert_eq!(triangles, &[[0, 1, 2]]);
        assert_eq!(vertices.len(), 3);
        for vertex in vertices {
            assert_eq!(vertex.position.get(), Point3::new(0., 0., 0.));
            assert_eq!(vertex.normal.get(), Vector3::new(0., 0., 1.));
        }
        assert!(tessellations[0].0.channels().is_empty());
    });
    for (dimension, additional) in [
        (
            ResourceDimension::RetainedBytes,
            (3 * std::mem::size_of::<ShadedVertex<FinitePoint3, FiniteVector3>>()) as u64,
        ),
        (ResourceDimension::CollectionItems, 1),
    ] {
        let error = crate::test_support::resource_refusal_at(
            &[],
            dimension,
            "nx JT tessellation shaded vertices",
            decode,
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("shaded vertex refusal");
        };
        assert_eq!(limit.additional, additional);
    }
}

#[test]
fn tessellation_shaded_candidate_releases_rows_before_invalid_normal() {
    let mut fixture = Fixture::new();
    fixture.vertex_headers[0].vertex_bindings = 0x8;
    fixture.vertex_headers[0].vertex_attribute_count = 2;
    fixture.meshes[0].polygons[0].corners = vec![(0, Some(0)), (0, Some(1)), (0, Some(0))];
    let normals = [DisplayJtVertexNormals {
        id: "normals".into(),
        vertex_records_header: "vertex-header".into(),
        normals: vec![
            [
                FiniteBinary32::ZERO,
                FiniteBinary32::ZERO,
                FiniteBinary32::ONE,
            ],
            [FiniteBinary32::ZERO; 3],
        ],
        normal_hash: 0,
        byte_len: 0,
        source_offset: 0,
    }];
    assert_released(|ctx| {
        let mut inputs = fixture.inputs();
        inputs.normals = &normals;
        assert!(display_jt_tessellations(ctx, &inputs)?.is_empty());
        Ok(())
    });
}
