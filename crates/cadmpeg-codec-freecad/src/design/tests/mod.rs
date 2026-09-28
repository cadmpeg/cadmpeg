// SPDX-License-Identifier: Apache-2.0
//! Design-history unit tests over synthesized `FCStd` archives.

pub(crate) mod booleans_patterns;
pub(crate) mod construction;
mod history;
pub(crate) mod holes_extrude;
pub(crate) mod primitives;
pub(crate) mod sketches;
mod taper;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn sweep_profiles_and_paths_refuse_at_matching_limits() {
    let profile = linked_property("sweep", "Profile", "sweep-profile");
    let section = linked_property_count_to("sweep", "Sections", "sweep-section", 1, "other");
    let path = linked_property("sweep", "Spine", "sweep-path");
    let properties = [&profile, &section, &path];
    for operation in ["fcstd sweep profiles", "fcstd solid sweep sections"] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::sweep_definition(ctx, "Part::Sweep", &properties, &std::collections::HashMap::new())
        });
    }
    for operation in ["fcstd sweep native profile identity", "fcstd sweep path identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::sweep_definition(ctx, "Part::Sweep", &properties, &std::collections::HashMap::new())
        });
    }
    let mut sketches = std::collections::HashMap::new();
    sketches.insert("base", cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#sweep").expect("valid sketch id"));
    crate::test_support::assert_retained_refusal_at(&[], "fcstd sweep sketch identity", |ctx| {
        super::sweep_definition(ctx, "Part::Sweep", &properties, &sketches)
    });
    let sheet = bool_property("sweep", "Solid", false);
    crate::test_support::assert_collection_refusal_at(&[], "fcstd sheet sweep sections", |ctx| {
        super::sweep_definition(ctx, "Part::Sweep", &[&profile, &section, &path, &sheet], &std::collections::HashMap::new())
    });
    let mode = integer_property("pipe", "Mode", 3);
    let auxiliary = linked_property("pipe", "AuxiliarySpine", "auxiliary-path");
    crate::test_support::assert_retained_refusal_at(&[], "fcstd sweep auxiliary spine identity", |ctx| {
        super::sweep_definition(ctx, "PartDesign::AdditivePipe", &[&profile, &section, &path, &mode, &auxiliary], &std::collections::HashMap::new())
    });
}

#[test]
fn loft_profiles_refuse_at_matching_limits() {
    let profiles = linked_property_count("loft", "Sections", "loft-sections", 2);
    let sketches = std::collections::HashMap::new();
    for operation in ["fcstd loft profiles", "fcstd loft sections"] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::loft_definition(ctx, "Part::Loft", &[&profiles], &sketches)
        });
    }
    crate::test_support::assert_retained_refusal_at(&[], "fcstd loft native profile identity", |ctx| {
        super::loft_definition(ctx, "Part::Loft", &[&profiles], &sketches)
    });
    let mut sketches = std::collections::HashMap::new();
    sketches.insert("base", cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#loft").expect("valid sketch id"));
    crate::test_support::assert_retained_refusal_at(&[], "fcstd loft sketch identity", |ctx| {
        super::loft_definition(ctx, "Part::Loft", &[&profiles], &sketches)
    });
}

#[test]
fn boolean_selection_identities_refuse_at_retained_limits() {
    let base_feature = linked_property("boolean", "BaseFeature", "base-feature");
    let group = linked_property("boolean", "Group", "boolean-group");
    for operation in ["fcstd boolean base feature identity", "fcstd boolean group identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::boolean_definition(ctx, "PartDesign::Boolean", &[&base_feature, &group])
        });
    }
    for operation in ["fcstd boolean final group link", "fcstd boolean preceding group links"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::boolean_definition(ctx, "PartDesign::Boolean", &[&group])
        });
    }
    let base = linked_property("cut", "Base", "cut-base");
    let tool = linked_property("cut", "Tool", "cut-tool");
    for operation in ["fcstd boolean base identity", "fcstd boolean tool identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::boolean_definition(ctx, "Part::Cut", &[&base, &tool])
        });
    }
    let shapes = linked_property_count("fuse", "Shapes", "fuse-shapes", 2);
    for operation in ["fcstd boolean first shape link", "fcstd boolean remaining shape links"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::boolean_definition(ctx, "Part::MultiFuse", &[&shapes])
        });
    }
}

#[test]
fn thickness_faces_identity_refuses_at_retained_limit() {
    let faces = linked_property("wall", "Base", "faces-property");
    let value = scalar_property("wall", "Value", "2.5");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd thickness faces identity", |ctx| {
            super::thickness_definition(ctx, "PartDesign::Thickness", &[&faces, &value])
        },
    );
}

#[test]
fn offset_source_identity_refuses_at_retained_limit() {
    let source = linked_property("offset", "Source", "source-property");
    let value = scalar_property("offset", "Value", "1.5");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd offset source identity", |ctx| {
            super::offset_shape_definition(ctx, "Part::Offset", &[&source, &value])
        },
    );
}

#[test]
fn derived_shape_identities_refuse_at_retained_limits() {
    let links = linked_property("compound", "Links", "compound-links");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd compound members identity", |ctx| {
            super::derived_shape_definition(ctx, "Part::Compound", &[&links])
        },
    );
    let source = linked_property("refine", "Source", "refine-source");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd derived source identity", |ctx| {
            super::derived_shape_definition(ctx, "Part::Refine", &[&source])
        },
    );
}

#[test]
fn ruled_curve_identities_refuse_at_retained_limits() {
    let first = linked_property("ruled", "Curve1", "first-curve");
    let second = linked_property("ruled", "Curve2", "second-curve");
    for operation in ["fcstd ruled first curve identity", "fcstd ruled second curve identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::ruled_surface_definition(ctx, &[&first, &second])
        });
    }
}

#[test]
fn section_operand_identities_refuse_at_retained_limits() {
    let base = linked_property("section", "Base", "section-base");
    let tool = linked_property("section", "Tool", "section-tool");
    for operation in ["fcstd section base identity", "fcstd section tool identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::section_shape_definition(ctx, &[&base, &tool])
        });
    }
}

#[test]
fn mirror_shape_identities_refuse_at_retained_limits() {
    let source = linked_property("mirror", "Source", "mirror-source");
    let plane = linked_property("mirror", "MirrorPlane", "mirror-plane");
    let base = vector_property("mirror", "Base", 1.0, 0.0, 0.0);
    let normal = vector_property("mirror", "Normal", 0.0, 0.0, 1.0);
    for operation in ["fcstd mirror plane identity", "fcstd mirror source identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::mirror_shape_definition(ctx, &[&source, &plane, &base, &normal])
        });
    }
}

#[test]
fn projected_surface_identities_refuse_at_retained_limits() {
    let sources = linked_property("project", "Projection", "projection-sources");
    let support = linked_property("project", "SupportFace", "projection-support");
    let direction = vector_property("project", "Direction", 0.0, 0.0, 1.0);
    for operation in ["fcstd projection sources identity", "fcstd projection support identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::project_on_surface_definition(ctx, &[&sources, &support, &direction])
        });
    }
}

#[test]
fn draft_face_identities_refuse_at_retained_limits() {
    let faces = linked_property("draft", "Base", "draft-faces");
    let neutral = linked_property("draft", "NeutralPlane", "draft-neutral");
    let angle = scalar_property("draft", "Angle", "4.5");
    for operation in ["fcstd draft faces identity", "fcstd draft neutral plane identity"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::draft_definition(ctx, &[&faces, &neutral, &angle], &[], &std::collections::HashMap::new())
        });
    }
}

fn linked_property(owner: &str, name: &str, id: &str) -> crate::native::PropertyRecord {
    linked_property_count(owner, name, id, 1)
}

fn linked_property_count(owner: &str, name: &str, id: &str, count: usize) -> crate::native::PropertyRecord {
    linked_property_count_to(owner, name, id, count, "base")
}

fn linked_property_count_to(owner: &str, name: &str, id: &str, count: usize, target: &str) -> crate::native::PropertyRecord {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None, document_attribute: None, object: Some(target.into()),
        subelements: Vec::new(),
    }).expect("valid link");
    crate::native::PropertyRecord {
        id: id.into(), owner: owner.into(), name: name.into(),
        type_name: "App::PropertyLink".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(), links: vec![link; count], side_entries: Vec::new(), dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    }
}

fn bool_property(owner: &str, name: &str, value: bool) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:{name}"), owner: owner.into(), name: name.into(),
        type_name: "App::PropertyBool".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 0,
        xml: crate::native::RetainedXml::from_text(
            format!("<Property><Bool value=\"{value}\"/></Property>"), 0,
        ).expect("valid XML span"),
    }
}

fn integer_property(owner: &str, name: &str, value: i64) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:{name}"), owner: owner.into(), name: name.into(),
        type_name: "App::PropertyInteger".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 0,
        xml: crate::native::RetainedXml::from_text(
            format!("<Property><Integer value=\"{value}\"/></Property>"), 0,
        ).expect("valid XML span"),
    }
}

fn scalar_property(owner: &str, name: &str, value: &str) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:{name}"), owner: owner.into(), name: name.into(),
        type_name: "App::PropertyLength".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 0,
        xml: crate::native::RetainedXml::from_text(
            format!("<Property><Float value=\"{value}\"/></Property>"), 0,
        ).expect("valid XML span"),
    }
}

fn vector_property(owner: &str, name: &str, x: f64, y: f64, z: f64) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:{name}"), owner: owner.into(), name: name.into(),
        type_name: "App::PropertyVector".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 0,
        xml: crate::native::RetainedXml::from_text(
            format!("<Property><PropertyVector valueX=\"{x}\" valueY=\"{y}\" valueZ=\"{z}\"/></Property>"), 0,
        ).expect("valid XML span"),
    }
}

#[test]
fn dress_up_edge_identity_refuses_at_retained_limit() {
    let base = crate::native::PropertyRecord {
        id: "base-edge-property".into(), owner: "fillet".into(), name: "Base".into(),
        type_name: "App::PropertyLinkSub".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd dress-up edge selection", |ctx| {
            super::dress_up_edge_selection(ctx, "PartDesign::Fillet", &[&base])
        },
    );
}

#[test]
fn scale_base_identity_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None, document_attribute: None, object: Some("body".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let base = crate::native::PropertyRecord {
        id: "base-body-property".into(), owner: "scale".into(), name: "Base".into(),
        type_name: "App::PropertyLink".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(), links: vec![link], side_entries: Vec::new(), dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let factor = crate::native::PropertyRecord {
        id: "scale-factor".into(), owner: "scale".into(), name: "UniformScale".into(),
        type_name: "App::PropertyFloat".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Transient, order: 1,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Float value=\"2\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd scale base selection", |ctx| {
            super::scale_definition(ctx, &[&base, &factor])
        },
    );
}

#[test]
fn part_fillet_edge_values_refuse_at_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "edge-values-property".into(), owner: "fillet".into(), name: "Edges".into(),
        type_name: "Part::PropertyFilletEdges".into(),
        family: crate::native::PropertyFamily::Unknown, status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(), links: Vec::new(), side_entries: vec!["edges.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let mut data = 1_u32.to_le_bytes().to_vec();
    data.extend(0_u32.to_le_bytes());
    data.extend(2_f64.to_le_bytes());
    data.extend(2_f64.to_le_bytes());
    let entry = crate::native::EntryRecord {
        id: "entry".into(), name: "edges.bin".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(), data,
    };
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd fillet edge values", |ctx| {
            super::part_fillet_edge_values(ctx, &[&property], std::slice::from_ref(&entry))
        },
    );
}

#[test]
fn part_face_source_selection_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("source".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let sources = crate::native::PropertyRecord {
        id: "sources-property".into(),
        owner: "face".into(),
        name: "Sources".into(),
        type_name: "App::PropertyLinkList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(), links: vec![link], side_entries: Vec::new(), dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let maker = crate::native::PropertyRecord {
        id: "maker-property".into(),
        owner: "face".into(),
        name: "FaceMakerClass".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 1,
        xml: crate::native::RetainedXml::from_text(
            "<Property><String value=\"Part::FaceMakerUnified\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd face source selection", |ctx| {
            super::part_construction_geometry_definition(
                ctx, "Part::Face", &[&sources, &maker], &[],
            )
        },
    );
}

#[test]
fn singular_reference_link_keeps_one_selector_and_rejects_two() {
    let property = |subelements: Vec<String>| {
        let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
            document: None,
            document_attribute: None,
            object: Some("source".into()),
            subelements,
        }).expect("valid link");
        crate::native::PropertyRecord {
            id: "reference".into(),
            owner: "owner".into(),
            name: "ReferenceAxis".into(),
            type_name: "App::PropertyLinkSub".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: vec![link],
                side_entries: Vec::new(),
                dynamic: None,
            },
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
        }
    };
    let one = property(vec!["Edge1".into()]);
    assert!(matches!(super::singular_reference_link(&one), Some((_, Some("Edge1")))));
    let two = property(vec!["Edge1 Edge2".into()]);
    assert!(super::singular_reference_link(&two).is_none());
}

#[test]
fn design_revolution_reference_copies_refuse_at_retained_limits() {
    let property = |id: &str, name: &str, type_name: &str, xml: &str,
                    links: Vec<Option<crate::native::LinkTarget>>| crate::native::PropertyRecord {
        id: id.into(),
        owner: "revolution".into(),
        name: name.into(),
        type_name: type_name.into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(), links, side_entries: Vec::new(), dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(xml.into(), 0)
            .expect("valid XML span"),
    };
    let axis = property(
        "axis-property", "Axis", "App::PropertyVector",
        "<Property><PropertyVector valueX=\"0\" valueY=\"1\" valueZ=\"0\"/></Property>",
        Vec::new(),
    );
    let mode_face = property(
        "mode-face", "Type", "App::PropertyEnumeration",
        "<Property><Integer value=\"3\"/></Property>", Vec::new(),
    );
    let mode_first = property(
        "mode-first", "Type", "App::PropertyEnumeration",
        "<Property><Integer value=\"2\"/></Property>", Vec::new(),
    );
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None, document_attribute: None, object: Some("target".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let face = property(
        "terminal-face", "UpToFace", "App::PropertyLinkSub", "<Property/>",
        vec![link.clone()],
    );
    let reference = property(
        "axis-reference", "ReferenceAxis", "App::PropertyLinkSub", "<Property/>",
        vec![link],
    );
    let sketches = std::collections::HashMap::new();
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd revolution terminal face", |ctx| {
            super::revolution_definition(
                ctx, "PartDesign::Revolution", "revolution", &[&axis, &mode_face, &face],
                &sketches,
            )
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd revolution axis reference", |ctx| {
            super::revolution_definition(
                ctx, "PartDesign::Revolution", "revolution", &[&axis, &mode_first, &reference],
                &sketches,
            )
        },
    );
}

#[test]
fn design_grouped_and_native_constraints_refuse_at_matching_limits() {
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Sketch".into(),
        name: "Sketch".into(),
        type_name: "Sketcher::SketchObject".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let sketch = SketchId::mint("test:test:sketch#group").expect("valid sketch identity");
    let entities = [SketchEntity::new(
        SketchEntityId::mint("test:test:entity#line").expect("valid entity identity"),
        SketchId::mint("test:test:sketch#group").expect("valid sketch identity"),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        }).expect("valid line"),
    )];
    let property = |constraint: &str| crate::native::PropertyRecord {
        id: "constraint-property".into(),
        owner: object.id.clone(),
        name: "Constraints".into(),
        type_name: "Sketcher::PropertyConstraintList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            format!("<Property><ConstraintList count=\"1\">{constraint}</ConstraintList></Property>"), 0,
        ).expect("valid XML span"),
    };
    let text = property("<Constrain Type=\"21\" MetaData=\"{&quot;text&quot;:&quot;label&quot;,&quot;font&quot;:&quot;mono&quot;}\" ElementIds=\"0\" ElementPositions=\"0\"/>");
    for operation in ["fcstd constraint text", "fcstd constraint font"] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::parse_constraints(ctx, &object, &[&text], &sketch, &entities)
        });
    }
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd constraint locus copies", |ctx| {
            super::parse_constraints(ctx, &object, &[&text], &sketch, &entities)
        },
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::parse_constraints(&ctx, &object, &[&text], &sketch, &entities),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd constraint text metadata parse"));
    let native = property("<Constrain Type=\"99\" First=\"0\" FirstPos=\"0\"/>");
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd native constraint entities", |ctx| {
            super::parse_constraints(ctx, &object, &[&native], &sketch, &entities)
        },
    );
    let alignment = property("<Constrain Type=\"15\" InternalAlignmentType=\"1\" First=\"0\" FirstPos=\"0\" Second=\"0\" SecondPos=\"1\"/>");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd constraint entity identity", |ctx| {
            super::parse_constraints(ctx, &object, &[&alignment], &sketch, &entities)
        },
    );
}

#[test]
fn neutral_constraint_copies_refuse_at_matching_limits() {
    let entity = cadmpeg_ir::sketches::SketchEntityId::mint("test:test:entity#one")
        .expect("valid entity identity");
    let loci = [cadmpeg_ir::sketches::SketchLocus::Entity(entity)];
    let parameter = cadmpeg_ir::features::ParameterId::mint("test:test:parameter#one")
        .expect("valid parameter identity");
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd constraint locus copies", |ctx| {
            super::neutral_constraint(ctx, 1, &loci, None, true)
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd constraint entity identity", |ctx| {
            super::neutral_constraint(ctx, 2, &loci, None, true)
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd constraint parameter identity copy", |ctx| {
            super::neutral_constraint(ctx, 6, &loci, Some(&parameter), true)
        },
    );
}

#[test]
fn resolved_constraint_loci_refuse_at_retained_limits() {
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};
    let entity = |id, geometry| SketchEntity::new(
        SketchEntityId::mint(id).expect("valid entity identity"),
        SketchId::mint("test:test:sketch#constraints").expect("valid sketch identity"),
        geometry,
    );
    let entities = [
        entity(
            "test:test:entity#line",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                end: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            }).expect("valid line"),
        ),
        entity(
            "test:test:entity#point",
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            }).expect("valid point"),
        ),
    ];
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd resolved operand identity", |ctx| {
            super::resolve_operand(ctx, 0, 3, &entities)
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd midpoint line identity", |ctx| {
            super::midpoint_constraint(ctx, 1, &[(0, 3), (1, 0)], &entities)
        },
    );
}

#[test]
fn design_profile_references_refuse_at_matching_retained_limits() {
    let sketches = std::collections::HashMap::new();
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd unresolved profile reference", |ctx| {
            super::profile_ref(ctx, "source-owner", &[], &sketches)
        },
    );
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("target".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let property = crate::native::PropertyRecord {
        id: "profile-property".into(),
        owner: "source-owner".into(),
        name: "Profile".into(),
        type_name: "App::PropertyLink".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd native profile reference", |ctx| {
            super::profile_ref(ctx, "source-owner", &[&property], &sketches)
        },
    );
    let mut sketches = sketches;
    sketches.insert("target", cadmpeg_ir::sketches::SketchId::mint(
        "test:test:sketch#profile",
    ).expect("valid sketch identity"));
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd sketch profile reference", |ctx| {
            super::profile_ref(ctx, "source-owner", &[&property], &sketches)
        },
    );
}

#[test]
fn design_nurbs_lanes_refuse_at_each_collection_limit() {
    let xml = roxmltree::Document::parse(
        "<BSplineCurve PolesCount=\"3\" KnotsCount=\"2\" Degree=\"2\" IsPeriodic=\"0\"><Pole X=\"0\" Y=\"0\" Z=\"0\" Weight=\"1\"/><Pole X=\"1\" Y=\"2\" Z=\"0\" Weight=\"0.5\"/><Pole X=\"3\" Y=\"0\" Z=\"0\" Weight=\"1\"/><Knot Value=\"0\" Mult=\"3\"/><Knot Value=\"1\" Mult=\"3\"/></BSplineCurve>",
    ).expect("valid XML");
    for operation in [
        "fcstd sketch NURBS poles",
        "fcstd sketch NURBS knots",
        "fcstd sketch NURBS expanded knots",
        "fcstd sketch NURBS control points",
        "fcstd sketch NURBS weights",
        "fcstd sketch NURBS nonzero weights",
        "fcstd sketch NURBS knot conversion",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::sketch_nurbs_lanes(ctx, "Part::GeomBSplineCurve", xml.root_element())
        });
    }
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd sketch NURBS weighted pole pairs", |ctx| {
            super::sketch_nurbs(ctx, "Part::GeomBSplineCurve", xml.root_element())
        },
    );
}

#[test]
fn design_constraint_parameter_admissions_refuse_at_matching_limits() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Sketch".into(),
        name: "Sketch".into(),
        type_name: "Sketcher::SketchObject".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "constraint-property".into(),
        owner: object.id.clone(),
        name: "Constraints".into(),
        type_name: "Sketcher::PropertyConstraintList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><ConstraintList count=\"1\"><Constrain Type=\"6\" Value=\"4\" Name=\"Width\"/></ConstraintList></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#one")
        .expect("valid sketch identity");
    for operation in [
        "fcstd constraint expression path",
        "fcstd constraint parameter name",
    ] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            super::parse_constraints(ctx, &object, &[&property], &sketch, &[])
        });
    }
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd constraint parameter properties", |ctx| {
            super::parse_constraints(ctx, &object, &[&property], &sketch, &[])
        },
    );
}

#[test]
fn design_native_operand_position_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Sketch".into(),
        name: "Sketch".into(),
        type_name: "Sketcher::SketchObject".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "constraint-property".into(),
        owner: object.id.clone(),
        name: "Constraints".into(),
        type_name: "Sketcher::PropertyConstraintList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><ConstraintList count=\"1\"><Constrain Type=\"99\" First=\"-3\" FirstPos=\"1\"/></ConstraintList></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#one")
        .expect("valid sketch identity");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd native operand position kind", |ctx| {
            super::parse_constraints(ctx, &object, &[&property], &sketch, &[])
        },
    );
}

#[test]
fn design_native_parameter_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "native-property".into(),
        owner: "feature".into(),
        name: "ProxyState".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><String value=\"native-value\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd native parameter value",
        |ctx| super::native_definition(ctx, "Part::FeaturePython", &[&property]),
    );
}

#[test]
fn design_feature_state_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "state-property".into(),
        owner: "feature".into(),
        name: "Visibility".into(),
        type_name: "App::PropertyBool".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Bool value=\"true\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd feature state value",
        |ctx| super::feature_state(ctx, "feature", &[&property]),
    );
}

#[test]
fn design_operation_scalar_expression_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Box".into(),
        name: "Box".into(),
        type_name: "PartDesign::AdditiveBox".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "length-property".into(),
        owner: object.id.clone(),
        name: "Length".into(),
        type_name: "App::PropertyLength".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Float value=\"3.5\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd operation scalar expression",
        |ctx| super::append_operation_parameters(ctx, &mut Vec::new(), &object, &[&property]),
    );
}

#[test]
fn design_string_property_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "maker-property".into(),
        owner: "feature".into(),
        name: "FaceMakerClass".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><String value=\"Part::FaceMakerBullseye\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd string property value",
        |ctx| super::string_property_value(ctx, &property),
    );
}

#[test]
fn design_numeric_list_refuses_at_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "numeric-list".into(),
        owner: "pattern".into(),
        name: "Spacings".into(),
        type_name: "App::PropertyFloatList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["numbers.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><FloatList file=\"numbers.bin\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let mut data = 1_u32.to_le_bytes().to_vec();
    data.extend(2.5_f64.to_le_bytes());
    let entry = crate::native::EntryRecord {
        id: "entry".into(),
        name: "numbers.bin".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::numeric_list(&ctx, &property, &[entry]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd numeric-list values"));
}

#[test]
fn design_vector_list_refuses_at_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "vector-list".into(),
        owner: "polygon".into(),
        name: "Nodes".into(),
        type_name: "App::PropertyVectorList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["vectors.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><VectorList file=\"vectors.bin\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let mut data = 1_u32.to_le_bytes().to_vec();
    for component in [1.0_f64, 2.0, 3.0] {
        data.extend(component.to_le_bytes());
    }
    let entry = crate::native::EntryRecord {
        id: "entry".into(),
        name: "vectors.bin".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::vector_list_property(&ctx, &[&property], "Nodes", &[entry]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd vector-list points"));
}

#[test]
fn design_body_output_prefix_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Body".into(),
        name: "Body".into(),
        type_name: "PartDesign::Body".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Body:Shape".into(),
        owner: object.id.clone(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let payload = crate::brep::ShapePayloadRecord {
        id: "fcstd:native:shape-payload#Body:Shape".into(),
        property: property.id.clone(),
        entry: "shape.brp".into(),
        payload: crate::brep::ShapePayload::Empty,
    };
    crate::test_support::assert_retained_refusal_at(&[], "fcstd design body output prefix", |ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        super::transfer(ctx, &mut ir, &[object.clone()], &[property.clone()], &[payload.clone()], &[], None)
    });
}

#[test]
fn sketch_placement_error_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "placement-property".into(),
        owner: "sketch".into(),
        name: "Placement".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let expected = "sketch Placement placement carrier has runtime type App::PropertyString";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::validate_sketch_placement(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).expect("message length fits");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::validate_sketch_placement(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD sketch placement error"));
    assert!(matches!(super::sketch_frame(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD sketch placement error"));
}

#[test]
fn design_ordered_objects_refuse_at_caller_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Body".into(),
        name: "Body".into(),
        type_name: "PartDesign::Body".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::feature_ordinals(
        &ctx, &[object], &Default::default(), &Default::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd design ordered objects"));
}

#[test]
fn design_body_member_identity_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("child-object".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let property = crate::native::PropertyRecord {
        id: "group-property".into(),
        owner: "body".into(),
        name: "Group".into(),
        type_name: "App::PropertyLinkList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let mut feature_ids = std::collections::HashMap::new();
    feature_ids.insert("child-object",
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Child")
            .expect("valid feature identity"));
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd body member feature identity",
        |ctx| super::body_definition(ctx, &[&property], &feature_ids),
    );
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd distinct body children",
        |ctx| super::body_definition(ctx, &[&property], &feature_ids),
    );
}

#[test]
fn design_body_tip_identity_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("child-object".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let property = crate::native::PropertyRecord {
        id: "tip-property".into(),
        owner: "body".into(),
        name: "Tip".into(),
        type_name: "App::PropertyLink".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let mut feature_ids = std::collections::HashMap::new();
    feature_ids.insert("child-object",
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Child")
            .expect("valid feature identity"));
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd body tip feature identity",
        |ctx| super::body_definition(ctx, &[&property], &feature_ids),
    );
}

#[test]
fn design_unresolved_profile_identity_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Pad".into(),
        name: "Pad".into(),
        type_name: "PartDesign::Pad".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd unresolved profile identity",
        |ctx| super::transfer(
            ctx, &mut cadmpeg_ir::document::CadIr::empty(),
            &[object.clone()], &[], &[], &[], None,
        ),
    );
}

#[test]
fn design_distinct_feature_dependencies_refuse_at_collection_limit() {
    let source = crate::native::ObjectRecord {
        id: "fcstd:native:object#Source".into(),
        name: "Source".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let dependent = crate::native::ObjectRecord {
        id: "fcstd:native:object#Dependent".into(),
        name: "Dependent".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: vec![source.id.clone()],
        dependency_allow_partial: None,
        order: 1,
        data: None,
    };
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd distinct feature dependencies",
        |ctx| super::transfer(
            ctx, &mut cadmpeg_ir::document::CadIr::empty(),
            &[source.clone(), dependent.clone()], &[], &[], &[], None,
        ),
    );
}

#[test]
fn design_feature_identity_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Feature".into(),
        name: "Feature".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design feature identity",
        |ctx| super::feature_id(ctx, &object),
    );
}

#[test]
fn design_composed_identities_refuse_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Shape%20A".into(),
        name: "Shape A".into(),
        type_name: "Sketcher::SketchObject".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    for (kind, tail, expected) in [
        ("sketch", "", "fcstd:design:sketch#Shape%20A"),
        ("sketch-entity", ":external:2", "fcstd:design:sketch-entity#Shape%20A:external:2"),
        ("sketch-constraint", ":3", "fcstd:design:sketch-constraint#Shape%20A:3"),
        ("parameter", ":cell:A1", "fcstd:design:parameter#Shape%20A:cell:A1"),
        ("spreadsheet", "", "fcstd:design:spreadsheet#Shape%20A"),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root");
        assert_eq!(super::design_identity_text(
            &ctx, kind, &object, format_args!("{tail}"), "fcstd composed identity",
        ).expect("identity fits policy"), expected);
        crate::test_support::assert_retained_refusal_at(
            &[], "fcstd composed identity",
            |ctx| super::design_identity_text(
                ctx, kind, &object, format_args!("{tail}"), "fcstd composed identity",
            ),
        );
    }
}

#[test]
fn design_parameter_object_name_index_refuses_at_collection_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Feature".into(),
        name: "Feature".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::bind_parameter_dependencies(
        &ctx, &mut Vec::new(), &[object], &Default::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd parameter dependency object names"));
}

#[test]
fn design_parameter_candidates_refuse_at_collection_limit() {
    let parameter = cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#Feature:Length")
            .expect("valid parameter identity"),
        owner: None,
        ordinal: 0,
        name: "Length".into(),
        expression: "1".into(),
        display: None,
        value: None,
        dependencies: Default::default(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::bind_parameter_dependencies(
        &ctx, &mut vec![parameter.clone()], &[], &Default::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd parameter dependency candidates"));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let result = super::order_parameters_by_dependencies(
        &ctx, &mut vec![parameter],
    );
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(ref limit))
        if limit.operation == "fcstd known parameter identities"), "{result:?}");
}

#[test]
fn design_qualified_parameter_name_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Feature".into(),
        name: "Feature".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let parameter = cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("fcstd:design:parameter#Feature:Length")
            .expect("valid parameter identity"),
        owner: Some(cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Feature")
            .expect("valid feature identity")),
        ordinal: 0,
        name: "Length".into(),
        expression: "1".into(),
        display: None,
        value: None,
        dependencies: Default::default(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd qualified candidate name",
        |ctx| super::bind_parameter_dependencies(
            ctx, &mut vec![parameter.clone()], &[object.clone()], &Default::default(),
        ),
    );
}

fn parameter_dependency_fixture(cycle: bool) -> (crate::native::ObjectRecord, Vec<cadmpeg_ir::features::DesignParameter>) {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Feature".into(),
        name: "Feature".into(),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let parameters = [("Length", "Width"), ("Width", if cycle { "Length" } else { "1" })]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (name, expression))| cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId::mint(
                format!("fcstd:design:parameter#Feature:{name}"),
            ).expect("valid parameter identity"),
            owner: Some(cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Feature")
                .expect("valid feature identity")),
            ordinal: ordinal as u32,
            name: name.into(),
            expression: expression.into(),
            display: None,
            value: None,
            dependencies: Default::default(),
            properties: Default::default(),
            pmi: None,
            native_ref: None,
        })
        .collect();
    (object, parameters)
}

#[test]
fn design_parameter_dependency_stages_refuse_at_collection_limits() {
    let (object, parameters) = parameter_dependency_fixture(false);
    for operation in [
        "fcstd parameter dependency candidates",
        "fcstd parameter candidate names",
        "fcstd local candidate keys",
        "fcstd local candidate identities",
        "fcstd qualified candidate keys",
        "fcstd qualified candidate identities",
        "fcstd unique local candidates",
        "fcstd unique qualified candidates",
        "fcstd parameter dependencies",
        "fcstd parameter dependency members",
        "fcstd parameter distinct check",
        "fcstd ordinal owner groups",
        "fcstd owner ordinals",
        "fcstd known parameter identities",
        "fcstd emitted parameter identities",
        "fcstd reordered parameters",
        "fcstd next ordinal owners",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::bind_parameter_dependencies(
                ctx, &mut parameters.clone(), &[object.clone()], &Default::default(),
            )
        });
    }
}

#[test]
fn design_parameter_cycle_owners_refuse_at_collection_limit() {
    let (object, parameters) = parameter_dependency_fixture(true);
    crate::test_support::assert_collection_refusal_at(
        &[], "fcstd parameter cycle owners",
        |ctx| super::bind_parameter_dependencies(
            ctx, &mut parameters.clone(), &[object.clone()], &Default::default(),
        ),
    );
}

#[test]
fn design_spreadsheet_value_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse("<Property/>").expect("valid XML");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design diagnostic",
        |ctx| super::direct_spreadsheet_value(ctx, &xml, "Cells", "spreadsheet-property"),
    );
}

#[test]
fn design_spreadsheet_cell_properties_refuse_at_collection_limits() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Sheet".into(),
        name: "Sheet".into(),
        type_name: "Spreadsheet::Sheet".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "cells-property".into(),
        owner: object.id.clone(),
        name: "cells".into(),
        type_name: "Spreadsheet::PropertySheet".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Cells Count=\"1\"><Cell address=\"A1\" content=\"5\" alias=\"Length\"/></Cells></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    for operation in [
        "fcstd spreadsheet cell properties",
        "fcstd spreadsheet distinct parameter IDs",
        "fcstd spreadsheet distinct addresses",
    ] {
        crate::test_support::assert_collection_refusal_at(
            &[], operation,
            |ctx| super::append_spreadsheet(ctx, &mut Vec::new(), &object, &[&property]),
        );
    }
}

#[test]
fn design_sketch_carrier_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse("<Wrong/>").expect("valid XML");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design diagnostic",
        |ctx| super::validate_sketch_carrier(
            ctx, "Part::GeomLineSegment", &xml.root_element(), 1,
        ),
    );
}

#[test]
fn design_external_geometry_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse(
        "<Geometry><GeoExtensions><GeoExtension type=\"Sketcher::ExternalGeometryExtension\"/><GeoExtension type=\"Sketcher::ExternalGeometryExtension\"/></GeoExtensions></Geometry>",
    ).expect("valid XML");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design diagnostic",
        |ctx| super::external_geometry_metadata(ctx, xml.root_element(), 1),
    );
}

#[test]
fn design_counted_record_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse("<Property/>").expect("valid XML");
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design diagnostic",
        |ctx| super::direct_counted_records(
            ctx, &xml, "GeometryList", "Geometry", "geometry-property",
        ),
    );
}

#[test]
fn design_taper_diagnostic_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "taper-property".into(),
        owner: "feature".into(),
        name: "TaperAngle".into(),
        type_name: "App::PropertyAngle".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Float value=\"180\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd design diagnostic",
        |ctx| super::taper_angle(ctx, &[&property], "TaperAngle"),
    );
}

#[test]
fn design_expression_copy_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "expression-property".into(),
        owner: "owner".into(),
        name: "ExpressionEngine".into(),
        type_name: "App::PropertyExpressionEngine".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: vec![crate::native::ValueRecord {
                tag: "Expression".into(),
                order: 0,
                attributes: [
                    ("path".into(), "Length".into()),
                    ("expression".into(), "Sheet.A1".into()),
                ].into(),
                text: None,
                raw_xml: String::new(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::expression_binding(&ctx, &[&property], "Length"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd expression engine reference"));
}

/// Feature definition selected by exact feature name.
fn definition<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    name: &str,
) -> &'a FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name}"))
        .evaluation
        .definition()
}

/// Definition of the single `Extrusion` feature.
fn extrusion_definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .expect("extrusion feature")
        .evaluation
        .definition()
}
