// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};
use crate::tessellation::{Meshed, Tessellation, TessellationMesh};
use crate::validate::validate_neutral;

#[test]
fn tessellation_counts_must_be_consistent() {
    use crate::ids::FaceId;
    use crate::math::Point3;

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.tessellations.push(
        Tessellation::new(
            crate::tessellation::TessellationId::mint("synthetic:test:tessellation#invalid-counts")
                .expect("valid identity"),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_of(Meshed::Body {
            body: ir.geometry.bodies[0].id.clone(),
            faces: vec![FaceId::mint("synthetic:test:face#missing").expect("valid identity")],
        }),
    );
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
}

#[test]
fn tessellation_triangle_groups_and_texture_assignments_validate() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::math::Point3;
    use crate::report::{check::Check, Severity};
    use crate::tessellation::{
        Tessellation, TessellationTextureAssignment, TessellationTriangleGroup,
    };

    let texture = AssetId::mint("synthetic:test:asset#mesh-texture").expect("identity grammar");
    let valid = Tessellation::new(
        crate::tessellation::TessellationId::mint("synthetic:test:tessellation#valid-groups")
            .expect("valid identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        },
        Vec::new(),
    )
    .expect("valid tessellation")
    .with_triangle_groups(vec![
        TessellationTriangleGroup::new(Some("group-a".into()), vec![0]),
        TessellationTriangleGroup::new(Some("group-b".into()), vec![1]),
    ])
    .expect("valid triangle group partition")
    .with_texture_assignments(vec![
        TessellationTextureAssignment {
            source_id: Some("texture-resource-a".into()),
            texture: texture.clone(),
            triangles: vec![0],
        },
        TessellationTextureAssignment {
            source_id: Some("texture-resource-b".into()),
            texture: texture.clone(),
            triangles: vec![1],
        },
    ])
    .expect("valid texture assignments");
    let mut invalid_texture = valid
        .clone()
        .with_texture_assignments(vec![TessellationTextureAssignment {
            source_id: Some("texture-resource-a".into()),
            texture: AssetId::mint("synthetic:test:asset#missing").expect("identity grammar"),
            triangles: vec![0],
        }])
        .expect("valid local texture assignment");
    invalid_texture.id = "synthetic:test:tessellation#missing-texture"
        .try_into()
        .unwrap();

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.assets.push(
        Asset::try_new(
            texture,
            None,
            None,
            AssetContent::Embedded {
                data: crate::assets::AssetData::new(vec![0]).expect("nonempty asset data"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model.tessellations.extend([valid, invalid_texture]);
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    let errors_for = |entity: &str| {
        report
            .findings
            .iter()
            .filter(|finding| {
                finding.check == Check::Tessellation
                    && finding.severity == Severity::Error
                    && finding.entity.as_deref() == Some(entity)
            })
            .count()
    };
    assert_eq!(errors_for("synthetic:test:tessellation#valid-groups"), 0);
    assert_eq!(errors_for("synthetic:test:tessellation#missing-texture"), 1);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding
                .message
                .contains("missing tessellation texture asset"))
            .count(),
        1
    );
}

#[test]
fn finite_nonzero_signed_sphere_radius_is_valid_without_a_size_floor() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        crate::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -1e-200,
        )
        .unwrap(),
    ));
    crate::examples::add_document_root(&mut ir);
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn tessellation_reference_validation_preserves_finding_order() {
    use crate::report::{check::Check, Severity};
    let owner = "test:model:tessellation#missing";
    let mut ir = unit_cube().unwrap();
    ir.model.tessellations.push(
        Tessellation::new(
            owner.try_into().unwrap(),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap()
        .with_of(Meshed::Body {
            body: "test:model:body#missing".try_into().unwrap(),
            faces: vec![
                "test:model:face#missing".try_into().unwrap(),
                "test:model:face#also-missing".try_into().unwrap(),
            ],
        })
        .with_texture_assignments(vec![crate::tessellation::TessellationTextureAssignment {
            source_id: None,
            texture: "test:model:asset#missing".try_into().unwrap(),
            triangles: vec![0],
        }])
        .unwrap(),
    );
    let mut findings = Vec::new();
    super::check_tessellations(
        &crate::index::ModelIndex::new_model_only(&ir),
        &mut findings,
    );
    assert_eq!(findings.len(), 2);
    for (finding, message) in findings.iter().zip([
        "references a missing tessellation body",
        "references a missing tessellation texture asset",
    ]) {
        assert_eq!(finding.check, Check::Tessellation);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner));
        assert_eq!(finding.message, message);
    }
}

/// A two-triangle mesh of the unit cube's body whose first group holds the
/// triangles of `face`.
fn face_grouped(ir: &crate::document::CadIr, face: crate::ids::FaceId, body: bool) -> Tessellation {
    use crate::tessellation::TessellationTriangleGroup;
    let mesh = Tessellation::new(
        crate::tessellation::TessellationId::mint("synthetic:test:tessellation#face-groups")
            .expect("valid identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        },
        Vec::new(),
    )
    .expect("valid tessellation")
    .with_triangle_groups(vec![
        TessellationTriangleGroup::new(None, vec![0]).with_face(face),
        TessellationTriangleGroup::new(None, vec![1]),
    ])
    .expect("valid triangle group partition");
    if body {
        mesh.with_of(Meshed::Body {
            body: ir.geometry.bodies[0].id.clone(),
            faces: Vec::new(),
        })
    } else {
        mesh
    }
}

/// `face_grouped` with `faces` as the faces its body arm lists.
fn listing(mesh: Tessellation, faces: Vec<crate::ids::FaceId>) -> Tessellation {
    let Meshed::Body { body, .. } = mesh.of.clone() else {
        unreachable!("a face-grouped mesh displays a body");
    };
    mesh.with_of(Meshed::Body { body, faces })
}

fn tessellation_errors(ir: &mut crate::document::CadIr) -> Vec<String> {
    use crate::report::{check::Check, Severity};
    ir.finalize();
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| {
            finding.check == Check::Tessellation && finding.severity == Severity::Error
        })
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_triangle_group_names_a_face_of_the_tessellation_body() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let face = ir.geometry.faces[0].id.clone();
    let mesh = listing(face_grouped(&ir, face.clone(), true), vec![face]);
    ir.model.tessellations.push(mesh);
    assert_eq!(tessellation_errors(&mut ir), Vec::<String>::new());
}

#[test]
fn a_triangle_group_face_needs_a_tessellation_body() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let face = ir.geometry.faces[0].id.clone();
    ir.model.tessellations.push(face_grouped(&ir, face, false));
    let errors = tessellation_errors(&mut ir);
    assert!(
        errors
            .iter()
            .any(|message| message.contains("triangle group face and no tessellation body")),
        "{errors:?}"
    );
}

#[test]
fn a_triangle_group_face_is_a_face_of_the_tessellation_body() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let face = crate::ids::FaceId::mint("synthetic:test:face#elsewhere").expect("valid identity");
    ir.model.tessellations.push(face_grouped(&ir, face, true));
    let errors = tessellation_errors(&mut ir);
    assert!(
        errors
            .iter()
            .any(|message| message.contains("triangle group face outside the tessellation body")),
        "{errors:?}"
    );
}

#[test]
fn a_triangle_group_face_is_one_of_the_tessellation_faces() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let grouped = ir.geometry.faces[0].id.clone();
    let listed = ir.geometry.faces[1].id.clone();
    let mesh = listing(face_grouped(&ir, grouped, true), vec![listed]);
    ir.model.tessellations.push(mesh);
    let errors = tessellation_errors(&mut ir);
    assert!(
        errors
            .iter()
            .any(|message| message.contains("triangle group face outside the tessellation faces")),
        "{errors:?}"
    );
}

/// A one-triangle mesh `id` that is `of`.
fn meshed(id: &str, of: Meshed) -> Tessellation {
    Tessellation::new(
        id.try_into().unwrap(),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap()
    .with_of(of)
}

#[test]
fn a_body_mesh_covers_faces_of_its_body() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let body = ir.geometry.bodies[0].id.clone();
    let face = ir.geometry.faces[0].id.clone();
    ir.model.tessellations.push(meshed(
        "test:model:tessellation#own",
        Meshed::Body {
            body: body.clone(),
            faces: vec![face],
        },
    ));
    assert_eq!(tessellation_errors(&mut ir), Vec::<String>::new());

    let mut other = crate::examples::unit_cube().unwrap();
    other.geometry.bodies[0].id = "test:model:body#other".try_into().unwrap();
    ir.geometry.bodies.push(other.geometry.bodies[0].clone());
    ir.model.tessellations[0].of = Meshed::Body {
        body: "test:model:body#other".try_into().unwrap(),
        faces: vec![ir.geometry.faces[0].id.clone()],
    };
    assert_eq!(
        tessellation_errors(&mut ir),
        ["covers a face outside the tessellation body"]
    );
}

#[test]
fn a_definition_mesh_names_a_stored_definition() {
    use crate::products::{ExternalDocument, ProductDefinition, Storage};
    let part: crate::ids::ProductDefinitionId = "test:model:product#part".try_into().unwrap();
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.tessellations.push(meshed(
        "test:model:tessellation#mesh",
        Meshed::definition(part.clone()),
    ));
    assert_eq!(
        tessellation_errors(&mut ir),
        ["references a missing tessellation product definition"]
    );

    ir.model.product_definitions.push(ProductDefinition::new(
        part.clone(),
        Storage::External(ExternalDocument::default()),
    ));
    assert_eq!(
        tessellation_errors(&mut ir),
        ["references an external tessellation product definition"]
    );

    ir.model.product_definitions[0].storage = Storage::Embedded;
    assert_eq!(tessellation_errors(&mut ir), Vec::<String>::new());
    ir.model.tessellations[0].of = Meshed::Display { definition: part };
    assert_eq!(tessellation_errors(&mut ir), Vec::<String>::new());
}

#[test]
fn a_pmi_mesh_names_an_annotation_and_an_unresolved_one_states_no_state() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.tessellations.push(meshed(
        "test:model:tessellation#pmi",
        Meshed::Pmi {
            pmi: "test:model:pmi#missing".try_into().unwrap(),
        },
    ));
    assert_eq!(
        tessellation_errors(&mut ir),
        ["references a missing tessellation PMI annotation"]
    );

    ir.model.tessellations[0].of = Meshed::Unresolved;
    assert_eq!(tessellation_errors(&mut ir), Vec::<String>::new());
    ir.model.tessellations[0].state = Some(crate::design::refs::StateRef {
        configuration: None,
        of: crate::design::refs::Evaluated::Tool(
            crate::features::FeatureId::mint("test:design:feature#3").unwrap(),
        ),
    });
    assert_eq!(
        tessellation_errors(&mut ir),
        ["states a state on an unresolved tessellation"]
    );
}
