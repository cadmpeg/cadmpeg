// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;

fn square() -> Vec<Point3> {
    vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]
}

fn mesh() -> Tessellation {
    Tessellation::new(
        "test:mesh:tessellation#0",
        TessellationMesh::List {
            vertices: square(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        },
        Vec::new(),
    )
    .unwrap()
}

fn rejects_wire_field(field: &str, value: impl serde::Serialize) {
    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire[field] = serde_json::to_value(value).unwrap();
    assert!(serde_json::from_value::<Tessellation>(wire).is_err());
}

#[test]
fn per_corner_mesh_exposes_its_normals() {
    let base = mesh();
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); 6];
    let value = Tessellation::new(
        "test:mesh:tessellation#corners",
        TessellationMesh::from_corner_lanes(base.vertices(), base.triangles(), normals.clone())
            .unwrap(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(value.per_corner_normals().len(), 3 * value.triangle_count());
    assert_eq!(value.per_corner_normals(), normals);
    assert!(value.vertex_normals().is_empty());
}

#[test]
fn per_vertex_mesh_exposes_its_normals() {
    let base = mesh();
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); base.vertex_count()];
    let value = Tessellation::new(
        "test:mesh:tessellation#vertices",
        TessellationMesh::from_list_lanes(base.vertices(), base.triangles(), normals.clone())
            .unwrap(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(value.vertex_normals().len(), value.vertex_count());
    assert_eq!(value.vertex_normals(), normals);
    assert!(value.per_corner_normals().is_empty());
}

fn group(source_id: Option<&str>, triangles: Vec<u32>) -> TessellationTriangleGroup {
    TessellationTriangleGroup {
        source_id: source_id.map(str::to_owned),
        triangles,
    }
}

fn assignment(source_id: Option<&str>, triangles: Vec<u32>) -> TessellationTextureAssignment {
    TessellationTextureAssignment {
        source_id: source_id.map(str::to_owned),
        texture: "test:mesh:asset#0".try_into().unwrap(),
        triangles,
    }
}

#[test]
fn feature_edge_admission_rejects_invalid_pairs_order_duplicates_and_bounds() {
    for edges in [
        vec![[0, 0]],
        vec![[1, 0]],
        vec![[0, 4]],
        vec![[1, 2], [0, 1]],
        vec![[0, 1], [0, 1]],
    ] {
        assert!(mesh().with_feature_edges(edges.clone()).is_err());
        rejects_wire_field("feature_edges", edges);
    }
}

#[test]
fn triangle_groups_require_a_complete_disjoint_partition_and_unique_source_ids() {
    for groups in [
        vec![group(None, Vec::new())],
        vec![group(None, vec![0, 2])],
        vec![group(None, vec![1, 0])],
        vec![group(None, vec![0, 0, 1])],
        vec![group(None, vec![0, 1]), group(None, vec![1])],
        vec![group(None, vec![0])],
        vec![group(Some("a"), vec![0]), group(Some("a"), vec![1])],
        vec![group(Some(""), vec![0, 1])],
    ] {
        assert!(mesh().with_triangle_groups(groups.clone()).is_err());
        rejects_wire_field("triangle_groups", groups);
    }
}

#[test]
fn texture_assignment_admission_rejects_overlaps_and_ambiguous_resources() {
    for assignments in [
        vec![assignment(None, Vec::new())],
        vec![assignment(None, vec![2])],
        vec![assignment(None, vec![1, 0])],
        vec![assignment(None, vec![0, 0])],
        vec![
            assignment(Some("a"), vec![0]),
            assignment(Some("b"), vec![0]),
        ],
        vec![
            assignment(Some("a"), vec![0]),
            assignment(Some("a"), vec![1]),
        ],
        vec![assignment(None, vec![0]), assignment(None, vec![1])],
        vec![assignment(Some(""), vec![0])],
    ] {
        assert!(mesh()
            .with_texture_assignments(assignments.clone())
            .is_err());
        rejects_wire_field("texture_assignments", assignments);
    }
}

#[test]
fn valid_metadata_retains_group_order_and_distinct_resources_for_one_asset() {
    let groups = vec![group(Some("b"), vec![1]), group(Some("a"), vec![0])];
    let assignments = vec![
        assignment(Some("a"), vec![0]),
        assignment(Some("b"), vec![1]),
    ];
    let value = mesh()
        .with_feature_edges(vec![[0, 1], [2, 3]])
        .unwrap()
        .with_triangle_groups(groups.clone())
        .unwrap()
        .with_texture_assignments(assignments.clone())
        .unwrap();
    assert_eq!(value.triangle_groups(), groups);
    assert_eq!(value.texture_assignments(), assignments);
    let wire = serde_json::to_value(&value).unwrap();
    let decoded: Tessellation = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}

#[test]
fn vertex_channels_may_retain_auxiliary_descriptors_with_a_different_count() {
    let base = mesh();
    let channel = TessellationChannel::new(ChannelAddressing::Vertex {}, 1, 0, 0, vec![7]).unwrap();
    let value = Tessellation::new(
        "test:mesh:tessellation#auxiliary",
        TessellationMesh::List {
            vertices: base.vertices(),
            triangles: base.triangles(),
        },
        vec![channel],
    )
    .unwrap();
    assert_eq!(value.channels()[0].count(), 1);
    assert_eq!(value.vertex_count(), 4);
    assert_eq!(
        serde_json::from_value::<Tessellation>(serde_json::to_value(&value).unwrap()).unwrap(),
        value
    );
}

#[test]
fn tessellation_identity_admission() {
    for id in ["", "mesh id", "mesh\nid"] {
        assert!(TessellationId::mint(id).is_err());
        rejects_wire_field("id", id);
        assert!(Tessellation::new(
            id,
            TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            Vec::new(),
        )
        .is_err());
    }
    let id = TessellationId::mint("test:mesh:tessellation#0").unwrap();
    assert_eq!(
        serde_json::to_value(&id).unwrap(),
        "test:mesh:tessellation#0"
    );
    assert_eq!(
        serde_json::from_value::<TessellationId>(serde_json::to_value(&id).unwrap()).unwrap(),
        id
    );
}

#[test]
fn numeric_admission_rejects_non_finite_vertices_and_normals() {
    let base = mesh();
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut vertices = base.vertices();
        vertices[0].x = invalid;
        assert!(Tessellation::new(
            "test:mesh:tessellation#numeric",
            TessellationMesh::List {
                vertices: vertices.clone(),
                triangles: base.triangles(),
            },
            Vec::new(),
        )
        .is_err());
        rejects_wire_field(
            "mesh",
            serde_json::json!({
                "kind": "list",
                "vertices": vertices,
                "triangles": base.triangles(),
            }),
        );
        for corner in [false, true] {
            let count = if corner {
                base.triangle_count() * 3
            } else {
                base.vertex_count()
            };
            let mut normals = vec![Vector3::new(0.0, 0.0, 1.0); count];
            normals[0].y = invalid;
            let rows = if corner {
                TessellationMesh::from_corner_lanes(
                    base.vertices(),
                    base.triangles(),
                    normals.clone(),
                )
            } else {
                TessellationMesh::from_list_lanes(
                    base.vertices(),
                    base.triangles(),
                    normals.clone(),
                )
            }
            .unwrap();
            assert!(
                Tessellation::new("test:mesh:tessellation#numeric", rows.clone(), Vec::new())
                    .is_err()
            );
            rejects_wire_field("mesh", rows);
        }
    }
}

#[test]
fn numeric_edits_reject_invalid_values_without_partial_changes() {
    for corner in [false, true] {
        let base = mesh();
        let normals = vec![Vector3::new(0.0, 0.0, 1.0); if corner { 6 } else { 4 }];
        let rows = if corner {
            TessellationMesh::from_corner_lanes(base.vertices(), base.triangles(), normals)
        } else {
            TessellationMesh::from_list_lanes(base.vertices(), base.triangles(), normals)
        }
        .unwrap();
        let mut value =
            Tessellation::new("test:mesh:tessellation#numeric", rows, Vec::new()).unwrap();
        let original = value.clone();
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut seen = 0;
            assert!(value
                .edit_vertices(|vertex| {
                    if seen == 0 {
                        vertex.x = 2.0;
                    } else if seen == 1 {
                        vertex.z = invalid;
                    }
                    seen += 1;
                })
                .is_err());
            assert_eq!(value, original);
            let mut seen = 0;
            assert!(value
                .edit_normals(|normal| {
                    if seen == 0 {
                        normal.z = -1.0;
                    } else if seen == 1 {
                        normal.x = invalid;
                    }
                    seen += 1;
                })
                .is_err());
            assert_eq!(value, original);
        }
        let mut first = true;
        value
            .edit_vertices(|vertex| {
                if std::mem::take(&mut first) {
                    vertex.x = f64::MAX;
                }
            })
            .unwrap();
        let mut first = true;
        value
            .edit_normals(|normal| {
                if std::mem::take(&mut first) {
                    normal.z = -2.0;
                }
            })
            .unwrap();
        assert_eq!(value.vertices()[0].x, f64::MAX);
        let normals = if corner {
            value.per_corner_normals()
        } else {
            value.vertex_normals()
        };
        assert_eq!(normals[0].z, -2.0);
    }
}

#[test]
fn deflection_admission_and_edits_require_finite_non_negative_values() {
    let mut value = mesh().with_chordal_deflection(Some(0.5)).unwrap();
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(value
            .clone()
            .with_chordal_deflection(Some(invalid))
            .is_err());
        assert!(value.set_chordal_deflection(Some(invalid)).is_err());
        assert_eq!(value.chordal_deflection(), Some(0.5));
        let mut wire = TessellationWire::from(value.clone());
        wire.chordal_deflection = Some(invalid);
        assert!(Tessellation::try_from(wire).is_err());
    }
    rejects_wire_field("chordal_deflection", -1.0);
    for deflection in [Some(0.0), Some(f64::MAX), None] {
        value.set_chordal_deflection(deflection).unwrap();
        assert_eq!(value.chordal_deflection(), deflection);
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(serde_json::from_value::<Tessellation>(wire).unwrap(), value);
    }
}

#[test]
fn absent_normals_reject_edit_without_calling_the_editor() {
    let mut value = mesh();
    let original = value.clone();
    let mut called = false;
    assert!(value.edit_normals(|_| called = true).is_err());
    assert!(!called);
    assert_eq!(value, original);
}

// A shading normal travels in the vertex row or the triangle row that carries
// it, so "no normals" has exactly one spelling: the unshaded variant of the
// mesh. A normal run that does not cover the mesh has no spelling at all.
#[test]
fn the_wire_spells_the_shading_by_name() {
    let unshaded = serde_json::to_value(mesh()).unwrap();
    assert_eq!(unshaded["mesh"]["kind"], "list");
    assert!(unshaded["mesh"].get("shading").is_none());

    let base = mesh();
    let shaded = Tessellation::new(
        "test:mesh:tessellation#shaded",
        TessellationMesh::from_list_lanes(
            base.vertices(),
            base.triangles(),
            vec![Vector3::new(0.0, 0.0, 1.0); 4],
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    let wire = serde_json::to_value(&shaded).unwrap();
    assert_eq!(wire["mesh"]["kind"], "shaded_list");
    assert_eq!(wire["mesh"]["vertices"][0]["normal"]["z"], 1.0);
    assert_eq!(serde_json::from_value::<Tessellation>(wire).unwrap(), shaded);

    // A lane of normals that does not cover the mesh has no rows to become, so
    // the pairing refuses it before a mesh exists.
    assert!(TessellationMesh::from_list_lanes(
        base.vertices(),
        base.triangles(),
        vec![Vector3::new(0.0, 0.0, 1.0); 3],
    )
    .is_none());
    assert!(TessellationMesh::from_corner_lanes(
        base.vertices(),
        base.triangles(),
        vec![Vector3::new(0.0, 0.0, 1.0); 5],
    )
    .is_none());
}

// A strip owns the vertices it spans, so the wire states no strip length, no
// vertex total and no triangle list beside the strips.
#[test]
fn the_wire_spells_the_topology_by_name() {
    // A strip of one or two vertices spans no triangle and is refused at the
    // mint; a mesh with no strip is not a strip mesh.
    assert!(Strip::new(vec![Point3::new(0.0, 0.0, 0.0); 2]).is_none());
    assert!(Strips::<Point3>::new(Vec::new()).is_none());
    assert!(Strips::from_spans(square(), &[4, 4]).is_none());
    assert!(Strips::from_spans(square(), &[2, 2]).is_none());

    let strips = Tessellation::new(
        "test:mesh:tessellation#strips",
        TessellationMesh::Strips {
            strips: Strips::from_spans(square(), &[4]).unwrap(),
        },
        Vec::new(),
    )
    .unwrap();
    assert_eq!(strips.strip_lengths(), vec![4]);
    assert_eq!(strips.triangles(), vec![[0, 1, 2], [1, 3, 2]]);
    let wire = serde_json::to_value(&strips).unwrap();
    assert_eq!(wire["mesh"]["kind"], "strips");
    assert_eq!(wire["mesh"]["strips"][0].as_array().unwrap().len(), 4);
    let read: Tessellation = serde_json::from_value(wire).unwrap();
    assert_eq!(read, strips);
}

#[test]
fn a_strip_run_outside_the_mesh_is_an_unknown_key() {
    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["strip_lengths"] = serde_json::json!([4]);
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("unknown field"), "{error}");
    assert!(message.contains("strip_lengths"), "{error}");

    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["mesh"]["strip_lengths"] = serde_json::json!([4]);
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    assert!(error.to_string().contains("strip_lengths"), "{error}");

    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["mesh"] = serde_json::json!({"kind": "strips", "strips": []});
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    assert!(error.to_string().contains("empty"), "{error}");

    // A strip of one or two vertices spans no triangle, and the document route
    // refuses it at the strip mint rather than silently emitting nothing.
    for short in [0usize, 1, 2] {
        let mut wire = serde_json::to_value(mesh()).unwrap();
        wire["mesh"] = serde_json::json!({
            "kind": "strips",
            "strips": [square()[..short]],
        });
        let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
        assert!(
            error.to_string().contains("spans no triangle"),
            "{short}: {error}"
        );
    }
}

// The domain is the wire's tag, so the selector table exists only on the two
// domains that address one. A vertex channel has no `indices` key to carry,
// and the payload length is the data itself, not a restated `count`.
#[test]
fn a_channel_states_its_addressing_by_name() {
    let value = TessellationChannel::new(ChannelAddressing::Vertex {}, 1, 0, 0, vec![7]).unwrap();
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["addressing"], serde_json::json!({"domain": "vertex"}));
    assert!(wire.get("indices").is_none());
    assert!(wire.get("count").is_none());
    assert_eq!(
        serde_json::from_value::<TessellationChannel>(wire.clone()).unwrap(),
        value
    );

    let corner = TessellationChannel::new(
        ChannelAddressing::Corner {
            indices: vec![0, 0, 0],
        },
        1,
        0,
        0,
        vec![7],
    )
    .unwrap();
    let corner_wire = serde_json::to_value(&corner).unwrap();
    assert_eq!(
        corner_wire["addressing"],
        serde_json::json!({"domain": "corner", "indices": [0, 0, 0]})
    );
    assert_eq!(
        serde_json::from_value::<TessellationChannel>(corner_wire).unwrap(),
        corner
    );

    for orphan in [
        serde_json::json!({"domain": "vertex", "indices": [0]}),
        serde_json::json!({"domain": "corner"}),
        serde_json::json!({"domain": "triangle"}),
    ] {
        let mut invalid = wire.clone();
        invalid["addressing"] = orphan;
        assert!(serde_json::from_value::<TessellationChannel>(invalid).is_err());
    }

    let mut restated = wire;
    restated["count"] = serde_json::json!(1);
    let error = serde_json::from_value::<TessellationChannel>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("count"), "{error}");
}
