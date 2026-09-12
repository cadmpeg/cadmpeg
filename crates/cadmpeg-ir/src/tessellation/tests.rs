// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;

fn mesh() -> Tessellation {
    Tessellation::new(
        "test:mesh:tessellation#0",
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        TessellationTopology::List {},
        TessellationNormals::None,
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
        base.vertices().to_vec(),
        base.triangles().to_vec(),
        TessellationTopology::List {},
        TessellationNormals::per_corner(normals.clone()),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        value.per_corner_normals().len(),
        3 * value.triangles().len()
    );
    assert_eq!(value.per_corner_normals(), normals);
    assert!(!matches!(value.shading(), TessellationNormals::None));
    assert!(value.vertex_normals().is_empty());
}

#[test]
fn per_vertex_mesh_exposes_its_normals() {
    let base = mesh();
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); base.vertices().len()];
    let value = Tessellation::new(
        "test:mesh:tessellation#vertices",
        base.vertices().to_vec(),
        base.triangles().to_vec(),
        TessellationTopology::List {},
        TessellationNormals::per_vertex(normals.clone()),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(value.vertex_normals().len(), value.vertices().len());
    assert_eq!(value.vertex_normals(), normals);
    assert!(value.per_corner_normals().is_empty());
    assert!(!matches!(value.shading(), TessellationNormals::None));
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
        base.vertices().to_vec(),
        base.triangles().to_vec(),
        TessellationTopology::List {},
        TessellationNormals::None,
        vec![channel],
    )
    .unwrap();
    assert_eq!(value.channels()[0].count(), 1);
    assert_eq!(value.vertices().len(), 4);
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
            Vec::new(),
            Vec::new(),
            TessellationTopology::List {},
            TessellationNormals::None,
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
        let mut vertices = base.vertices().to_vec();
        vertices[0].x = invalid;
        assert!(Tessellation::new(
            "test:mesh:tessellation#numeric",
            vertices.clone(),
            base.triangles().to_vec(),
            TessellationTopology::List {},
            TessellationNormals::None,
            Vec::new(),
        )
        .is_err());
        rejects_wire_field("vertices", vertices);
        for corner in [false, true] {
            let count = if corner {
                base.triangles().len() * 3
            } else {
                base.vertices().len()
            };
            let mut normals = vec![Vector3::new(0.0, 0.0, 1.0); count];
            normals[0].y = invalid;
            let shading = if corner {
                TessellationNormals::per_corner(normals.clone())
            } else {
                TessellationNormals::per_vertex(normals.clone())
            };
            assert!(Tessellation::new(
                "test:mesh:tessellation#numeric",
                base.vertices().to_vec(),
                base.triangles().to_vec(),
                TessellationTopology::List {},
                shading,
                Vec::new(),
            )
            .is_err());
            rejects_wire_field(
                "shading",
                serde_json::json!({
                    "kind": if corner { "per_corner" } else { "per_vertex" },
                    "values": normals,
                }),
            );
        }
    }
}

#[test]
fn numeric_edits_reject_invalid_values_without_partial_changes() {
    for corner in [false, true] {
        let base = mesh();
        let normals = vec![Vector3::new(0.0, 0.0, 1.0); if corner { 6 } else { 4 }];
        let mut value = Tessellation::new(
            "test:mesh:tessellation#numeric",
            base.vertices().to_vec(),
            base.triangles().to_vec(),
            TessellationTopology::List {},
            if corner {
                TessellationNormals::per_corner(normals)
            } else {
                TessellationNormals::per_vertex(normals)
            },
            Vec::new(),
        )
        .unwrap();
        let original = value.clone();
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(value
                .edit_vertices(|vertices| {
                    vertices[0].x = 2.0;
                    vertices[1].z = invalid;
                })
                .is_err());
            assert_eq!(value, original);
            assert!(value
                .edit_normals(|normals| {
                    normals[0].z = -1.0;
                    normals[1].x = invalid;
                })
                .is_err());
            assert_eq!(value, original);
        }
        value
            .edit_vertices(|vertices| vertices[0].x = f64::MAX)
            .unwrap();
        value.edit_normals(|normals| normals[0].z = -2.0).unwrap();
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

// The literal route to an empty sample run does not compile: `TessellationNormals`
// carries a `NormalSamples`, whose vector is private and whose only mint,
// `NormalSamples::new`, returns `None` for an empty vector. `PerVertex(vec![])`
// and `PerCorner(vec![])` are therefore type errors, not values this test could
// construct and reject at run time.
#[test]
fn empty_wire_shading_is_the_absent_shading() {
    assert!(NormalSamples::new(Vec::new()).is_none());
    let empty = Tessellation::new(
        "test:mesh:tessellation#empty",
        Vec::new(),
        Vec::new(),
        TessellationTopology::List {},
        TessellationNormals::None,
        Vec::new(),
    )
    .unwrap();
    for kind in ["per_vertex", "per_corner"] {
        let mut wire = serde_json::to_value(&empty).unwrap();
        wire["shading"] = serde_json::json!({"kind": kind, "values": []});
        let admitted: Tessellation = serde_json::from_value(wire).unwrap();
        assert_eq!(*admitted.shading(), TessellationNormals::None);
        assert_eq!(admitted, empty);
        assert_eq!(
            serde_json::to_value(&admitted).unwrap()["shading"],
            serde_json::Value::Null
        );
    }
}

// The literal route to an empty strip run does not compile: `TessellationTopology::Strips`
// carries a `StripLengths`, whose vector is private and whose only mint,
// `TryFrom<Vec<StripRun>>`, refuses an empty vector.
#[test]
fn the_wire_spells_the_topology_by_name() {
    assert!(StripLengths::try_from(Vec::<u32>::new()).is_err());
    // A run of one or two vertices spans no triangle and is refused at the mint.
    assert!(StripRun::new(2).is_none());
    assert!(StripLengths::try_from(vec![3, 2]).is_err());
    let list = serde_json::to_value(mesh()).unwrap();
    assert_eq!(list["topology"], serde_json::json!({"kind": "list"}));

    let strips = Tessellation::from_decoded(
        "test:mesh:tessellation#strips",
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [1, 3, 2]],
        vec![4],
        TessellationNormals::None,
        Vec::new(),
    )
    .unwrap();
    let wire = serde_json::to_value(&strips).unwrap();
    assert_eq!(
        wire["topology"],
        serde_json::json!({"kind": "strips", "strip_lengths": [4]})
    );
    let read: Tessellation = serde_json::from_value(wire).unwrap();
    assert_eq!(read, strips);
}

#[test]
fn a_strip_run_outside_the_topology_is_an_unknown_key() {
    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["strip_lengths"] = serde_json::json!([4]);
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("unknown field"), "{error}");
    assert!(message.contains("strip_lengths"), "{error}");

    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["topology"] = serde_json::json!({"kind": "list", "strip_lengths": [4]});
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    assert!(error.to_string().contains("strip_lengths"), "{error}");

    let mut wire = serde_json::to_value(mesh()).unwrap();
    wire["topology"] = serde_json::json!({"kind": "strips", "strip_lengths": []});
    let error = serde_json::from_value::<Tessellation>(wire).unwrap_err();
    assert!(error.to_string().contains("empty"), "{error}");

    // A run of one or two vertices spans no triangle, and the document route
    // refuses it at the run mint rather than silently emitting nothing.
    for short in [0, 1, 2] {
        let mut wire = serde_json::to_value(mesh()).unwrap();
        wire["topology"] = serde_json::json!({"kind": "strips", "strip_lengths": [short]});
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
