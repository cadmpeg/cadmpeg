// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::report::Severity;

use crate::loss::F3dLossCode;

/// A report carrying the BREP-less geometry losses that `build_container_report`
/// states before the design segment is classified.
fn brep_less_geometry_report() -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: vec![
            F3dLossCode::GeometryNotTransferred.note("stated before classification"),
            F3dLossCode::TopologyNotTransferred.note("stated before classification"),
            F3dLossCode::MissingGeometryStream.note("stated before classification"),
        ],
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    }
}

/// A design whose content is sketch curves declares no body, so it has no
/// B-rep to lose: the sketch entities are its complete geometry.
#[test]
fn sketch_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 13, 0, false);
    assert!(report.transfer.geometry_transferred());
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "sketch-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("13 sketch entity(s)")));
}

/// A reference-image timeline object is presentation content. A bodyless
/// document that contains one does not require a BREP geometry carrier.
#[test]
fn presentation_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 1, false);
    assert!(report.transfer.geometry_transferred());
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "presentation-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("1 reference-image timeline object(s)")));
}

/// A declared body whose BREP stream is absent is a real missing carrier. Its
/// sketches do not stand in for the solid the document says it has.
#[test]
fn a_declared_body_without_a_brep_stream_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 1, 13, 0, false);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A document with no sketch entities transferred nothing, so nothing settles
/// the loss. An imported drawing whose only entity the importer did not author
/// produces exactly this: a document with no body and no sketch.
#[test]
fn a_document_without_sketch_entities_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 0, false);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A present BREP stream that produced no geometry is a decode failure, not a
/// sketch-only design, however many sketches the document also carries.
#[test]
fn a_present_brep_stream_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 1, 0, 0, 13, 0, false);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A text-encoded B-rep carrier is not sketch-only, regardless of sketch count.
#[test]
fn a_text_brep_carrier_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 2, 0, 13, 0, false);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

#[test]
fn empty_model_body_ownership_is_final_without_sketch_wire_ownership() {
    let wire = crate::test_support::smbh_geometry_test::synthetic_wire_body_smbh();
    let archive = crate::test_support::zip_test::f3d_with_smbh(&wire);
    let decoded = cadmpeg_ir::codec::Codec::decode(
        &crate::F3dCodec,
        &mut std::io::Cursor::new(archive),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .unwrap();
    assert!(!decoded.ir().geometry.bodies.is_empty());
    for model_body_maps_empty in [true, false] {
        let path = super::super::FinalizePath::Bodyless(super::super::DeferredBodylessInputs {
            model_body_maps_empty,
            model_body_maps_explicitly_empty: model_body_maps_empty,
            has_appearance: false,
            mesh_projection: super::super::MeshProjection {
                count: 0,
                unjoined: 0,
                tessellations_by_scope: std::collections::HashMap::new(),
                scopeless_tessellations: Vec::new(),
            },
        });
        let bodies = path.component_bodies(&decoded.ir().geometry.bodies, false);
        if model_body_maps_empty {
            assert!(bodies.unwrap().is_empty());
        } else {
            assert!(bodies.is_none());
        }
        assert!(path
            .component_bodies(&decoded.ir().geometry.bodies, true)
            .is_none());
    }
}

#[test]
fn component_body_coverage_uses_saved_edge_origin_and_keeps_ordinary_bodies() {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};
    let bodies = [
        ("f3d:brep:entity#1", BodyKind::Solid),
        ("f3d:brep:entity#2", BodyKind::Sheet),
        ("f3d:brep:entity#3", BodyKind::Wire),
        ("f3d:brep:saved-edge-body#4", BodyKind::Wire),
        ("f3d:brep/part.smbh/brep:saved-edge-body#5", BodyKind::Wire),
    ]
    .into_iter()
    .map(|(id, kind)| Body {
        id: BodyId::mint(id).unwrap(),
        kind,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    })
    .collect::<Vec<_>>();
    let path = super::super::FinalizePath::Geometry(super::super::GeometryIndex {
        primary_model_brep_name: "BREP.source.smbh".to_owned(),
        annotation_records: Vec::new(),
        mesh_projection: super::super::MeshProjection {
            count: 0,
            unjoined: 0,
            tessellations_by_scope: std::collections::HashMap::new(),
            scopeless_tessellations: Vec::new(),
        },
    });
    let required = path.component_bodies(&bodies, true).unwrap();
    let expected = bodies[..3]
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(required, expected);
    assert_eq!(bodies.len(), 5);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.bodies.extend(bodies);
    let loss = super::super::saved_free_edge_bodies_loss(&ir).unwrap();
    assert!(loss.message.starts_with("2 saved free-edge bodies"));
    let definition: cadmpeg_ir::products::ProductDefinition =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:design:product-definition#1", "kind": "part", "bodies": [], "bom_properties": {}, "storage": {"kind": "embedded"}, "tessellations": [], "occurrences": [], "instance_bodies": [], "representations": [], "sketches": []
        }))
        .unwrap();
    ir.model.product_definitions.push(definition);
    let loss = super::super::saved_free_edge_bodies_loss(&ir).unwrap();
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::SavedFreeEdgeBodyUnowned.kind()
    );
    assert!(loss.message.starts_with("2 saved free-edge bodies"));
    ir.model.product_definitions[0].bodies = ir.geometry.bodies[3..]
        .iter()
        .map(|body| body.id.clone())
        .collect();
    assert!(super::super::saved_free_edge_bodies_loss(&ir).is_none());
}

/// Carriers of a Design asset with one `.paramesh` entry and nothing else.
fn mesh_only_carriers() -> crate::decode::DesignCarriers {
    crate::decode::DesignCarriers {
        decoded_meshes: 0,
        unjoined_mesh_containers: 0,
        paramesh_entries: 1,
        brep_entries: 0,
        brep_streams: 0,
        selected_brep_streams: 0,
        model_body_maps_explicitly_empty: false,
        text_brep_streams: 0,
        declared_bodies: 0,
        sketch_entities: 0,
        reference_images: 0,
    }
}

fn has_code(report: &cadmpeg_ir::codec::DecodeBody, code: F3dLossCode) -> bool {
    report.losses.iter().any(|loss| loss.code == code.kind())
}

fn mesh_only_loss_names(report: &cadmpeg_ir::codec::DecodeBody, containers: &str) -> bool {
    report.losses.iter().any(|loss| {
        loss.code == F3dLossCode::MeshOnlyDesignUntransferred.kind()
            && loss.severity == Severity::Error
            && loss.message.contains(containers)
    })
}

fn classify(
    carriers: &crate::decode::DesignCarriers,
    assembly: bool,
) -> cadmpeg_ir::codec::DecodeBody {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_carrier_classification(&mut report, carriers, assembly);
    report
}

/// A mesh-only design whose mesh does not decode keeps its geometry losses and
/// names its mesh containers, whatever sketches it also carries.
#[test]
fn an_undecoded_mesh_only_design_with_a_sketch_names_its_mesh_containers() {
    let mut report = brep_less_geometry_report();
    report
        .losses
        .push(F3dLossCode::MeshContainerUndecoded.note("charged by the mesh decode"));
    let carriers = crate::decode::DesignCarriers {
        sketch_entities: 25,
        ..mesh_only_carriers()
    };
    crate::decode::apply_carrier_classification(&mut report, &carriers, false);
    assert!(!report.transfer.geometry_transferred());
    assert!(!has_code(&report, F3dLossCode::BodylessDesignCarrier));
    assert!(has_code(&report, F3dLossCode::MeshContainerUndecoded));
    assert!(has_code(&report, F3dLossCode::GeometryNotTransferred));
    assert!(mesh_only_loss_names(
        &report,
        "1 of its mesh geometry container(s)"
    ));
    assert_eq!(report.losses.len(), 5);
}

#[test]
fn an_undecoded_mesh_only_design_reports_every_mesh_container() {
    let report = classify(
        &crate::decode::DesignCarriers {
            paramesh_entries: 2,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 4);
    assert!(mesh_only_loss_names(
        &report,
        "2 of its mesh geometry container(s)"
    ));
}

/// A decoded container that no Design mesh body joins holds no current mesh
/// body, so it does not count toward a mesh-only design.
#[test]
fn unjoined_mesh_containers_keep_the_sketch_only_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            unjoined_mesh_containers: 1,
            sketch_entities: 25,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(report.transfer.geometry_transferred());
    assert!(has_code(&report, F3dLossCode::BodylessDesignCarrier));
    assert!(!has_code(&report, F3dLossCode::MeshOnlyDesignUntransferred));
    let report = classify(
        &crate::decode::DesignCarriers {
            paramesh_entries: 2,
            unjoined_mesh_containers: 1,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(mesh_only_loss_names(
        &report,
        "1 of its mesh geometry container(s)"
    ));
}

#[test]
fn a_design_without_mesh_containers_keeps_its_sketch_only_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            paramesh_entries: 0,
            sketch_entities: 25,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(report.transfer.geometry_transferred());
    assert!(has_code(&report, F3dLossCode::BodylessDesignCarrier));
    assert!(!has_code(&report, F3dLossCode::MeshOnlyDesignUntransferred));
}

#[test]
fn a_mesh_container_beside_a_brep_stream_keeps_the_brep_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            brep_entries: 1,
            brep_streams: 1,
            selected_brep_streams: 1,
            sketch_entities: 25,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

#[test]
fn a_mesh_container_beside_a_text_brep_stream_keeps_the_brep_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            brep_entries: 1,
            text_brep_streams: 1,
            sketch_entities: 25,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A BREP entry whose payload did not open is still a stored BREP entry.
#[test]
fn a_mesh_container_beside_an_unopened_brep_entry_keeps_the_bodyless_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            brep_entries: 1,
            sketch_entities: 25,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(has_code(&report, F3dLossCode::BodylessDesignCarrier));
    assert!(!has_code(&report, F3dLossCode::MeshOnlyDesignUntransferred));
}

/// An assembly's geometry is its external references; its classification
/// owns the report.
#[test]
fn an_assembly_with_a_mesh_container_keeps_its_report() {
    let report = classify(&mesh_only_carriers(), true);
    assert_eq!(report.losses.len(), 3);
    assert!(!has_code(&report, F3dLossCode::MeshOnlyDesignUntransferred));
}

/// A mesh-only design whose mesh decodes transfers its geometry as
/// tessellation.
#[test]
fn a_decoded_mesh_only_design_transfers_its_geometry() {
    let report = classify(
        &crate::decode::DesignCarriers {
            decoded_meshes: 1,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 1);
    assert!(has_code(&report, F3dLossCode::MeshVertexPrecisionReduced));
}

/// Carriers of a part whose modern model-body maps are explicitly empty and
/// whose snapshot BREP stream holds no model body.
fn empty_design_carriers() -> crate::decode::DesignCarriers {
    crate::decode::DesignCarriers {
        paramesh_entries: 0,
        brep_entries: 1,
        brep_streams: 1,
        model_body_maps_explicitly_empty: true,
        ..mesh_only_carriers()
    }
}

#[test]
fn explicitly_empty_model_maps_without_carriers_make_an_empty_design() {
    let report = classify(&empty_design_carriers(), false);
    assert!(report.transfer.geometry_transferred());
    assert!(has_code(&report, F3dLossCode::EmptyDesign));
    assert!(!has_code(&report, F3dLossCode::GeometryNotTransferred));
    assert!(!has_code(&report, F3dLossCode::TopologyNotTransferred));
    assert!(!has_code(&report, F3dLossCode::BodylessDesignCarrier));
}

#[test]
fn a_sketch_under_empty_model_maps_keeps_the_bodyless_classification() {
    let report = classify(
        &crate::decode::DesignCarriers {
            sketch_entities: 4,
            ..empty_design_carriers()
        },
        false,
    );
    assert!(has_code(&report, F3dLossCode::BodylessDesignCarrier));
    assert!(!has_code(&report, F3dLossCode::EmptyDesign));
}

#[test]
fn an_unselected_model_without_explicit_empty_maps_keeps_its_geometry_losses() {
    for carriers in [
        crate::decode::DesignCarriers {
            model_body_maps_explicitly_empty: false,
            ..empty_design_carriers()
        },
        crate::decode::DesignCarriers {
            selected_brep_streams: 1,
            ..empty_design_carriers()
        },
        crate::decode::DesignCarriers {
            declared_bodies: 1,
            ..empty_design_carriers()
        },
    ] {
        let report = classify(&carriers, false);
        assert!(!report.transfer.geometry_transferred());
        assert!(has_code(&report, F3dLossCode::GeometryNotTransferred));
        assert!(!has_code(&report, F3dLossCode::EmptyDesign));
    }
}

#[test]
fn an_assembly_with_empty_model_maps_is_not_an_empty_design() {
    let report = classify(&empty_design_carriers(), true);
    assert!(!has_code(&report, F3dLossCode::EmptyDesign));
}

#[test]
fn empty_model_maps_beside_an_undecoded_mesh_container_keep_the_geometry_losses() {
    let report = classify(
        &crate::decode::DesignCarriers {
            brep_entries: 1,
            brep_streams: 1,
            model_body_maps_explicitly_empty: true,
            ..mesh_only_carriers()
        },
        false,
    );
    assert!(!report.transfer.geometry_transferred());
    assert!(has_code(&report, F3dLossCode::GeometryNotTransferred));
    assert!(!has_code(&report, F3dLossCode::EmptyDesign));
}
