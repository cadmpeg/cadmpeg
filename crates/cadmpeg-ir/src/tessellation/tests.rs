// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    ChannelAddressing, Strip, Strips, Tessellation, TessellationChannel, TessellationError,
    TessellationId, TessellationMesh, TessellationTextureAssignment, TessellationTriangleGroup,
    TessellationWire, TextureCoordinateAddressing, TextureCoordinates,
};
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::scalar::NonNegativeReal;
use crate::units::{FinitePoint2, UnitVector3};

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
        TessellationId::mint("test:mesh:tessellation#0").expect("valid identity"),
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
        TessellationId::mint("test:mesh:tessellation#corners").expect("valid identity"),
        TessellationMesh::from_corner_lanes(
            base.mesh().clone().into_raw().vertices(),
            base.triangles(),
            Some(normals.clone()),
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(value.per_corner_normals().len(), 3 * value.triangle_count());
    assert_eq!(value.per_corner_normals(), normals);
    assert!(value.vertex_normals().is_empty());
}

#[test]
fn admitted_corner_mesh_parts_preserve_normals_and_check_indices() {
    let vertices = square()
        .into_iter()
        .map(|point| FinitePoint3::new(point).unwrap())
        .collect::<Vec<_>>();
    let normals = vec![FiniteVector3::from(UnitVector3::Z_AXIS); 3];
    let admitted = TessellationMesh::from_corner_lanes(
        vertices.clone(),
        vec![[0, 1, 2]],
        Some(normals.clone()),
    )
    .unwrap();
    let value = Tessellation::from_parts(
        TessellationId::mint("test:mesh:tessellation#admitted").expect("valid identity"),
        admitted,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        value.per_corner_normals(),
        vec![Vector3::new(0.0, 0.0, 1.0); 3]
    );

    let invalid =
        TessellationMesh::from_corner_lanes(vertices, vec![[0, 1, 4]], Some(normals)).unwrap();
    assert!(Tessellation::from_parts(
        TessellationId::mint("test:mesh:tessellation#invalid").expect("valid identity"),
        invalid,
        Vec::new()
    )
    .is_err());
}

#[test]
fn per_vertex_mesh_exposes_its_normals() {
    let base = mesh();
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); base.vertex_count()];
    let value = Tessellation::new(
        TessellationId::mint("test:mesh:tessellation#vertices").expect("valid identity"),
        TessellationMesh::from_list_lanes(
            base.mesh().clone().into_raw().vertices(),
            base.triangles(),
            Some(normals.clone()),
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(value.vertex_normals().len(), value.vertex_count());
    assert_eq!(value.vertex_normals(), normals);
    assert!(value.per_corner_normals().is_empty());
}

#[test]
fn checked_vertex_lanes_pair_without_retesting_coordinates() {
    let positions = square()
        .into_iter()
        .map(|point| FinitePoint3::new(point).unwrap())
        .collect::<Vec<_>>();
    let normals = vec![FiniteVector3::from(UnitVector3::Z_AXIS); positions.len()];
    let mesh = TessellationMesh::from_checked_list_lanes(
        positions.clone(),
        vec![[0, 1, 2]],
        Some(normals.clone()),
    )
    .unwrap();
    let tessellation = Tessellation::from_parts(
        TessellationId::mint("test:mesh:tessellation#checked-lanes").expect("valid identity"),
        mesh,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(tessellation.vertex_count(), positions.len());
    assert_eq!(
        tessellation.vertex_normals(),
        vec![Vector3::new(0.0, 0.0, 1.0); 4]
    );
    let refusal = TessellationMesh::from_checked_list_lanes(
        positions,
        vec![[0, 1, 2]],
        Some(normals[..3].to_vec()),
    );
    assert_eq!(
        refusal.unwrap_err().to_string(),
        "4 vertex/vertices against 3 vertex normal(s)"
    );
}

fn group(source_id: Option<&str>, triangles: Vec<u32>) -> TessellationTriangleGroup {
    TessellationTriangleGroup::new(source_id.map(str::to_owned), triangles)
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
        TessellationId::mint("test:mesh:tessellation#auxiliary").expect("valid identity"),
        TessellationMesh::List {
            vertices: base.mesh().clone().into_raw().vertices(),
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
        let mut vertices = base.mesh().clone().into_raw().vertices();
        vertices[0].x = invalid;
        assert!(Tessellation::new(
            TessellationId::mint("test:mesh:tessellation#numeric").expect("valid identity"),
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
                    base.mesh().clone().into_raw().vertices(),
                    base.triangles(),
                    Some(normals.clone()),
                )
            } else {
                TessellationMesh::from_list_lanes(
                    base.mesh().clone().into_raw().vertices(),
                    base.triangles(),
                    Some(normals.clone()),
                )
            }
            .unwrap();
            assert!(Tessellation::new(
                TessellationId::mint("test:mesh:tessellation#numeric").expect("valid identity"),
                rows.clone(),
                Vec::new()
            )
            .is_err());
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
            TessellationMesh::from_corner_lanes(
                base.mesh().clone().into_raw().vertices(),
                base.triangles(),
                Some(normals),
            )
        } else {
            TessellationMesh::from_list_lanes(
                base.mesh().clone().into_raw().vertices(),
                base.triangles(),
                Some(normals),
            )
        }
        .unwrap();
        let mut value = Tessellation::new(
            TessellationId::mint("test:mesh:tessellation#numeric").expect("valid identity"),
            rows,
            Vec::new(),
        )
        .unwrap();
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
                    Ok(())
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
                    Ok(())
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
                Ok(())
            })
            .unwrap();
        let mut first = true;
        value
            .edit_normals(|normal| {
                if std::mem::take(&mut first) {
                    normal.z = -2.0;
                }
                Ok(())
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
        assert_eq!(
            value.chordal_deflection().map(NonNegativeReal::get),
            Some(0.5)
        );
        let mut wire = TessellationWire::from(value.clone());
        wire.chordal_deflection = Some(invalid);
        assert!(Tessellation::try_from(wire).is_err());
    }
    rejects_wire_field("chordal_deflection", -1.0);
    for deflection in [Some(0.0), Some(f64::MAX), None] {
        value.set_chordal_deflection(deflection).unwrap();
        assert_eq!(
            value.chordal_deflection().map(NonNegativeReal::get),
            deflection
        );
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(serde_json::from_value::<Tessellation>(wire).unwrap(), value);
    }
}

#[test]
fn admitted_tessellation_deflection_keeps_the_same_wire_value() {
    let deflection = NonNegativeReal::new(0.5).unwrap();
    let direct = mesh().with_admitted_chordal_deflection(Some(deflection));
    let checked = mesh().with_chordal_deflection(Some(0.5)).unwrap();
    assert_eq!(direct.chordal_deflection(), checked.chordal_deflection());
    assert_eq!(
        serde_json::to_value(direct).unwrap(),
        serde_json::to_value(checked).unwrap()
    );
}

#[test]
fn absent_normals_reject_edit_without_calling_the_editor() {
    let mut value = mesh();
    let original = value.clone();
    let mut called = false;
    assert!(value
        .edit_normals(|_| {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(!called);
    assert_eq!(value, original);
}

#[test]
fn a_refused_vertex_edit_keeps_the_prior_vertices() {
    let mut value = mesh();
    let original = value.clone();
    let mut seen = 0;
    assert!(value
        .edit_vertices(|vertex| {
            seen += 1;
            if seen == 1 {
                vertex.x = 9.0;
                Ok(())
            } else {
                Err(TessellationError::EditRefused(
                    "the caller refused this vertex".to_string(),
                ))
            }
        })
        .is_err());
    assert_eq!(seen, 2);
    assert_eq!(value, original);
}

#[test]
fn a_refused_normal_edit_keeps_the_prior_normals() {
    let base = mesh();
    let rows = TessellationMesh::from_list_lanes(
        base.mesh().clone().into_raw().vertices(),
        base.triangles(),
        Some(vec![Vector3::new(0.0, 0.0, 1.0); 4]),
    )
    .unwrap();
    let mut value = Tessellation::new(
        TessellationId::mint("test:mesh:tessellation#refused").expect("valid identity"),
        rows,
        Vec::new(),
    )
    .unwrap();
    let original = value.clone();
    let mut seen = 0;
    assert!(value
        .edit_normals(|normal| {
            seen += 1;
            if seen == 1 {
                normal.z = -1.0;
                Ok(())
            } else {
                Err(TessellationError::EditRefused(
                    "the caller refused this normal".to_string(),
                ))
            }
        })
        .is_err());
    assert_eq!(seen, 2);
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
        TessellationId::mint("test:mesh:tessellation#shaded").expect("valid identity"),
        TessellationMesh::from_list_lanes(
            base.mesh().clone().into_raw().vertices(),
            base.triangles(),
            Some(vec![Vector3::new(0.0, 0.0, 1.0); 4]),
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    let wire = serde_json::to_value(&shaded).unwrap();
    assert_eq!(wire["mesh"]["kind"], "shaded_list");
    assert_eq!(wire["mesh"]["vertices"][0]["normal"]["z"], 1.0);
    assert_eq!(
        serde_json::from_value::<Tessellation>(wire).unwrap(),
        shaded
    );

    // A lane of normals that does not cover the mesh has no rows to become, so
    // the pairing refuses it before a mesh exists.
    assert!(TessellationMesh::from_list_lanes(
        base.mesh().clone().into_raw().vertices(),
        base.triangles(),
        Some(vec![Vector3::new(0.0, 0.0, 1.0); 3]),
    )
    .is_err());
    assert!(TessellationMesh::from_corner_lanes(
        base.mesh().clone().into_raw().vertices(),
        base.triangles(),
        Some(vec![Vector3::new(0.0, 0.0, 1.0); 5]),
    )
    .is_err());

    // An unshaded mesh is stated by absence. An empty lane against a non-empty
    // vertex lane is a length mismatch, not a shading form.
    assert!(TessellationMesh::from_list_lanes(
        base.mesh().clone().into_raw().vertices(),
        base.triangles(),
        Some(Vec::new()),
    )
    .is_err());
    assert!(matches!(
        TessellationMesh::from_list_lanes(
            base.mesh().clone().into_raw().vertices(),
            base.triangles(),
            None
        ),
        Ok(TessellationMesh::List { .. })
    ));
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
        TessellationId::mint("test:mesh:tessellation#strips").expect("valid identity"),
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

// The domain is the wire's `kind` tag, so the selector table exists only on the two
// domains that address one. A vertex channel has no `indices` key to carry,
// and the payload length is the data itself, not a restated `count`.
#[test]
fn a_channel_states_its_addressing_by_name() {
    let value = TessellationChannel::new(ChannelAddressing::Vertex {}, 1, 0, 0, vec![7]).unwrap();
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["addressing"], serde_json::json!({"kind": "vertex"}));
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
        serde_json::json!({"kind": "corner", "indices": [0, 0, 0]})
    );
    assert_eq!(
        serde_json::from_value::<TessellationChannel>(corner_wire).unwrap(),
        corner
    );

    for orphan in [
        serde_json::json!({"kind": "vertex", "indices": [0]}),
        serde_json::json!({"kind": "corner"}),
        serde_json::json!({"kind": "triangle"}),
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

#[test]
fn a_tessellation_holds_its_admitted_mesh_and_takes_an_admitted_replacement() {
    use crate::features::geometry::{FinitePoint3, FiniteVector3};

    let base = mesh();
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); base.vertex_count()];
    let shaded = Tessellation::new(
        TessellationId::mint("test:mesh:tessellation#admitted").expect("valid identity"),
        TessellationMesh::from_list_lanes(square(), base.triangles(), Some(normals.clone()))
            .unwrap(),
        Vec::new(),
    )
    .unwrap()
    .with_chordal_deflection(Some(0.25))
    .unwrap();
    assert_eq!(
        shaded.vertices(),
        square()
            .into_iter()
            .map(|point| FinitePoint3::new(point).unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        shaded.vertex_normals(),
        vec![FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(); 4]
    );
    assert_eq!(shaded.chordal_deflection(), NonNegativeReal::new(0.25));
    assert_eq!(
        shaded.mesh().clone().into_raw(),
        TessellationMesh::from_list_lanes(square(), base.triangles(), Some(normals)).unwrap()
    );

    let reduced = TessellationMesh::<FinitePoint3, FiniteVector3>::List {
        vertices: shaded.vertices()[..3].to_vec(),
        triangles: vec![[0, 1, 2]],
    };
    let replaced = shaded
        .clone()
        .with_mesh(reduced.clone(), Vec::new())
        .unwrap();
    assert_eq!(replaced.id, shaded.id);
    assert_eq!(replaced.chordal_deflection(), shaded.chordal_deflection());
    assert_eq!(replaced.mesh(), &reduced);
    assert!(shaded
        .with_mesh(
            TessellationMesh::List {
                vertices: reduced.vertices(),
                triangles: vec![[0, 1, 3]],
            },
            Vec::new(),
        )
        .is_err());
}

/// A positive scale keeps a zero deflection at zero and an absent one absent;
/// only a deflection that overflows is refused, and it keeps its prior value.
#[test]
fn a_scaled_deflection_is_refused_only_when_it_overflows() {
    use crate::scalar::PositiveReal;
    let scale = |value: f64| PositiveReal::new(value).expect("a positive scale");
    for (deflection, factor, scaled) in [
        (Some(0.0), 1.0e-300, Some(0.0)),
        (Some(0.5), 25.4, Some(0.5 * 25.4)),
        (None, 25.4, None),
    ] {
        let mut value = mesh().with_chordal_deflection(deflection).unwrap();
        value.scale_chordal_deflection(scale(factor)).unwrap();
        assert_eq!(value.chordal_deflection().map(NonNegativeReal::get), scaled);
    }
    let mut value = mesh().with_chordal_deflection(Some(2.0)).unwrap();
    assert!(value.scale_chordal_deflection(scale(f64::MAX)).is_err());
    assert_eq!(
        value.chordal_deflection().map(NonNegativeReal::get),
        Some(2.0)
    );
}

fn coordinates(
    name: &str,
    addressing: TextureCoordinateAddressing,
    count: usize,
) -> TextureCoordinates {
    TextureCoordinates {
        name: cadmpeg_core::text::NonBlankString::new(name.to_owned()).unwrap(),
        addressing,
        values: (0..count)
            .map(|index| {
                FinitePoint2::new(crate::math::Point2::new(
                    f64::from(u32::try_from(index).unwrap()),
                    0.5,
                ))
                .unwrap()
            })
            .collect(),
    }
}

#[test]
fn texture_coordinates_hold_one_value_per_vertex_or_corner_under_unique_names() {
    use TextureCoordinateAddressing::{Corner, Vertex};
    let sets = vec![coordinates("st", Vertex, 4), coordinates("st1", Corner, 6)];
    let value = mesh().with_texture_coordinates(sets.clone()).unwrap();
    assert_eq!(value.texture_coordinates(), sets.as_slice());
    let round: Tessellation =
        serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
    assert_eq!(round, value);
    for invalid in [
        vec![coordinates("st", Vertex, 3)],
        vec![coordinates("st", Corner, 4)],
        vec![coordinates("st", Vertex, 4), coordinates("st", Corner, 6)],
    ] {
        assert!(mesh().with_texture_coordinates(invalid).is_err());
    }
    rejects_wire_field("texture_coordinates", vec![coordinates("st", Vertex, 3)]);
}

#[test]
fn a_tessellation_writes_the_state_it_names_and_refuses_a_null_one() {
    let mut value = mesh();
    assert!(serde_json::to_value(&value).unwrap().get("state").is_none());
    value.state = Some(crate::design::refs::StateRef {
        configuration: None,
        of: crate::design::refs::Evaluated::Tool(
            crate::features::FeatureId::mint("test:design:feature#3").unwrap(),
        ),
    });
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire["state"],
        serde_json::json!({"of": {"kind": "tool", "value": "test:design:feature#3"}})
    );
    assert_eq!(serde_json::from_value::<Tessellation>(wire).unwrap(), value);
    rejects_wire_field("state", serde_json::Value::Null);
}

#[test]
fn triangle_groups_name_each_face_at_most_once() {
    let face = crate::ids::FaceId::mint("test:mesh:face#0").unwrap();
    let groups = vec![
        group(None, vec![0]).with_face(face.clone()),
        group(None, vec![1]).with_face(face),
    ];
    assert!(mesh().with_triangle_groups(groups.clone()).is_err());
    rejects_wire_field("triangle_groups", groups);
}

#[test]
fn a_triangle_group_states_its_face_only_when_it_has_one() {
    let face = crate::ids::FaceId::mint("test:mesh:face#0").unwrap();
    let plain = serde_json::to_value(group(Some("a"), vec![0])).unwrap();
    assert_eq!(
        plain,
        serde_json::json!({ "source_id": "a", "triangles": [0] })
    );
    let faced = group(Some("a"), vec![0]).with_face(face.clone());
    let value = serde_json::to_value(&faced).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "source_id": "a", "face": "test:mesh:face#0", "triangles": [0] })
    );
    let back: TessellationTriangleGroup = serde_json::from_value(value).unwrap();
    assert_eq!(back.face, Some(face));
}

#[test]
fn a_tessellation_states_what_it_is_under_of() {
    let unresolved = serde_json::to_value(mesh()).unwrap();
    assert_eq!(unresolved["of"], serde_json::json!({"kind": "unresolved"}));
    assert!(unresolved.get("body").is_none() && unresolved.get("faces").is_none());

    let body = mesh().with_of(super::Meshed::Body {
        body: crate::ids::BodyId::mint("test:mesh:body#0").unwrap(),
        faces: vec![crate::ids::FaceId::mint("test:mesh:face#0").unwrap()],
    });
    let wire = serde_json::to_value(&body).unwrap();
    assert_eq!(
        wire["of"],
        serde_json::json!({"kind": "body", "body": "test:mesh:body#0", "faces": ["test:mesh:face#0"]})
    );
    assert_eq!(serde_json::from_value::<Tessellation>(wire).unwrap(), body);

    let definition = mesh().with_of(super::Meshed::Definition {
        definition: crate::ids::ProductDefinitionId::mint("test:mesh:product#0").unwrap(),
        name: cadmpeg_core::text::NonBlankString::new("Mesh".to_owned()),
        color: crate::topology::Color::new(1.0, 0.0, 0.0, 1.0),
        visible: Some(false),
    });
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        wire["of"],
        serde_json::json!({
            "kind": "definition",
            "definition": "test:mesh:product#0",
            "name": "Mesh",
            "color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0},
            "visible": false
        })
    );
    assert_eq!(
        serde_json::from_value::<Tessellation>(wire).unwrap(),
        definition
    );

    rejects_wire_field("of", serde_json::json!({"kind": "display"}));
    rejects_wire_field(
        "of",
        serde_json::json!({"kind": "definition", "definition": "test:mesh:product#0", "name": null}),
    );
    let mut legacy = serde_json::to_value(mesh()).unwrap();
    legacy["body"] = serde_json::json!("test:mesh:body#0");
    assert!(serde_json::from_value::<Tessellation>(legacy).is_err());
}
