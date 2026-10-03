use super::entity;
use super::length;
use super::neutral_feature;
use super::reference;
use super::simple_hole_definition;
use crate::swift::enrich_implicit_nominals_with_context;
use crate::swift::pattern_hole_nominal_context;
use crate::swift::project;
use crate::swift::project_with_topology;
use crate::swift::Entity;
use crate::swift::ObjectSection;
use crate::swift::RelatedObject;
use crate::swift::TopologyIdentityIndex;
use crate::swift::ROOT_CLASS;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed},
    Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::pmi::PmiDefinition;
use cadmpeg_ir::pmi::PmiTarget;
use cadmpeg_ir::scalar::PositiveReal;
use std::collections::BTreeMap;

#[test]
fn empty_swift_pattern_uses_one_native_hole_join() {
    let features = pattern_hole_features();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root fits policy");
    let context = pattern_hole_nominal_context(&ctx, &features).expect("pattern context");
    assert_eq!(
        context.get("Hole Pattern6").copied().map(PositiveReal::get),
        Some(6.1468)
    );

    let mut pattern = cad_feature("GdtPattern", "");
    pattern
        .strings
        .insert("ObjectName".into(), "Hole Pattern6".into());
    pattern.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
        entity: entity("GdtAppliedFeatureCollection"),
    });
    let mut diameter = entity("GdtDiameter");
    diameter
        .strings
        .insert("ObjectName".into(), "Diameter 8".into());
    diameter.doubles.insert("Nominal".into(), 0.0);
    diameter.features.references = vec![reference("FP", "GdtPattern")];
    let root = Entity {
        class: ROOT_CLASS.into(),
        features: ObjectSection {
            references: vec![reference("FP", "GdtPattern")],
            entities: vec![pattern],
        },
        annotations: ObjectSection {
            references: vec![reference("A8", "GdtDiameter")],
            entities: vec![diameter],
        },
        ..Entity::default()
    };
    let mut projected = project(&root);
    enrich_implicit_nominals_with_context(&root, &[], &mut projected, Some(&context));
    let PmiDefinition::Dimension(relation) =
        &projected.first().expect("diameter annotation").definition
    else {
        panic!("dimension definition");
    };
    let nominal = relation.nominal().expect("dimension nominal");
    assert_eq!(*nominal, length(6.1468).expect("finite length"));

    let mut ambiguous = features.clone();
    ambiguous.push(neutral_feature(
        "hole2",
        "Hole6",
        4,
        vec![FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar")],
        simple_hole_definition(6.1468),
    ));
    assert!(pattern_hole_nominal_context(&ctx, &ambiguous)
        .expect("ambiguous pattern context")
        .is_empty());

    let mut unresolved = features;
    let mut unresolved_hole = neutral_feature(
        "hole2",
        "Hole6",
        4,
        vec![FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar")],
        simple_hole_definition(6.1468),
    );
    unresolved_hole.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) = definition else {
            panic!("expected hole definition");
        };
        shape.try_edit(|_, _, diameter| *diameter = None).unwrap();
    });
    unresolved.push(unresolved_hole);
    assert!(pattern_hole_nominal_context(&ctx, &unresolved)
        .expect("unresolved pattern context")
        .is_empty());
}

fn pattern_hole_features() -> Vec<Feature> {
    let seed = FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar");
    let pattern_definition = FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(seed.clone())],
        pattern: PatternKind::UNRESOLVED,
    });
    vec![
        neutral_feature(
            "seed",
            "Sketch20",
            1,
            Vec::new(),
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Sketch".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        neutral_feature("pattern", "LPattern6", 2, Vec::new(), pattern_definition),
        neutral_feature(
            "hole",
            "Hole5",
            3,
            vec![seed],
            simple_hole_definition(6.1468),
        ),
    ]
}

fn pattern_hole_limit_error(
    set_limit: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
) -> CodecError {
    let features = pattern_hole_features();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    set_limit(&mut policy.limits);
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    pattern_hole_nominal_context(&ctx, &features).expect_err("pattern context must refuse")
}

#[test]
fn pattern_hole_nominal_context_refuses_collection_limit() {
    let CodecError::ResourceLimit(limit) =
        pattern_hole_limit_error(|limits| limits.max_collection_items = 0)
    else {
        panic!("expected collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}

#[test]
fn pattern_hole_nominal_context_refuses_retained_limit() {
    let CodecError::ResourceLimit(limit) =
        pattern_hole_limit_error(|limits| limits.max_retained_bytes = 0)
    else {
        panic!("expected retained refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
}

#[test]
fn pattern_hole_nominal_context_refuses_work_limit() {
    let CodecError::ResourceLimit(limit) =
        pattern_hole_limit_error(|limits| limits.max_work_units = 0)
    else {
        panic!("expected work refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
}

fn cad_feature(class: &str, identifier: &str) -> Entity {
    let mut cad_ref = entity("CadRef");
    cad_ref
        .strings
        .insert("CadIdentifier".into(), identifier.into());
    let mut references = entity("CadRefCollection");
    references.related.push(RelatedObject {
        name: "CadRef0".into(),
        class: "PrizMetrik.GdtAnalysis.CadRef".into(),
        entity: cad_ref,
    });
    let mut feature = entity(class);
    feature.related.push(RelatedObject {
        name: "CadReferences".into(),
        class: "PrizMetrik.GdtAnalysis.CadRefCollection".into(),
        entity: references,
    });
    feature
}

#[test]
fn cad_identifier_binds_unique_primary_topology_and_preserves_fallback() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root fits policy");
    let mut datum = entity("GdtDatum");
    datum.strings.insert("DatumIdentifier".into(), "A".into());
    datum.features.references.push(reference("F10", "GdtPlane"));
    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.features.references.push(reference("F10", "GdtPlane"));
    root.features
        .entities
        .push(cad_feature("GdtPlane", "125:42"));
    root.annotations
        .references
        .push(reference("A10", "GdtDatum"));
    root.annotations.entities.push(datum);

    let mut index = TopologyIdentityIndex::default();
    index.sequence_targets.insert(
        42,
        Some(PmiTarget::Face {
            face: FaceId::mint("sldprt:brep:face#42").expect("identity grammar"),
        }),
    );
    let projected = project_with_topology(&ctx, &root, Some(&index), &[], None)
        .expect("test projection fits policy")
        .annotations;
    let first = projected.first().expect("projected datum");
    assert_eq!(
        first.targets,
        [PmiTarget::Face {
            face: "sldprt:brep:face#42".try_into().expect("valid identity")
        }]
    );

    root.features
        .entities
        .first_mut()
        .expect("GdtPlane")
        .related
        .first_mut()
        .expect("CadReferences")
        .entity
        .related
        .first_mut()
        .expect("CadRef0")
        .entity
        .strings
        .insert("CadIdentifier".into(), "125:99".into());
    let projected = project_with_topology(&ctx, &root, Some(&index), &[], None)
        .expect("test projection fits policy")
        .annotations;
    let first = projected.first().expect("projected datum");
    assert_eq!(
        first.targets,
        [PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::try_from("F10")
                .expect("nonempty source identity")
        }]
    );
}

#[test]
fn cad_identifier_resolves_each_primary_topology_kind_and_rejects_collisions() {
    use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::topology::{Body, BodyKind, Edge, Face, Sense, Vertex};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root fits policy");
    let body = Body {
        id: BodyId::mint("sldprt:brep:body#11").expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let face = Face {
        id: FaceId::mint("sldprt:brep:face#22").expect("identity grammar"),
        shell: ShellId::mint("sldprt:brep:shell#1").expect("identity grammar"),
        surface: SurfaceId::mint("sldprt:brep:surf#22").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let edge = Edge {
        id: EdgeId::mint("sldprt:brep:edge#33").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
        start: VertexId::mint("sldprt:brep:vertex#1").expect("identity grammar"),
        end: VertexId::mint("sldprt:brep:vertex#2").expect("identity grammar"),
        tolerance: None,
    };
    let vertex = Vertex {
        id: VertexId::mint("sldprt:brep:vertex#44").expect("identity grammar"),
        point: PointId::mint("sldprt:brep:point#44").expect("identity grammar"),
        tolerance: None,
    };
    let index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(222, 22)],
        &[(333, 33)],
        &[(444, 44)],
    )
    .expect("topology index");

    assert_eq!(
        index.resolve("schema-a:11").cloned(),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-b:222").cloned(),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(index.resolve("schema-b:22").cloned().is_none());
    assert_eq!(
        index.resolve("schema-c:33").cloned(),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-d:44").cloned(),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-e:333").cloned(),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-f:444").cloned(),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );

    let qualified_alternate = Face {
        id: FaceId::mint("sldprt:brep:face#22@alternate").expect("identity grammar"),
        ..face.clone()
    };
    let active_with_alternate = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[face.clone(), qualified_alternate],
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(222, 22)],
        &[],
        &[],
    )
    .expect("topology index");
    assert_eq!(
        active_with_alternate.resolve("schema-g:222").cloned(),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(index.resolve("schema-e:not-a-number").cloned().is_none());
    assert!(index.resolve("11").is_none());

    let collision = Face {
        id: FaceId::mint("sldprt:brep:face#11").expect("identity grammar"),
        ..face.clone()
    };
    let index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[collision],
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[],
        &[],
        &[],
    )
    .expect("topology index");
    assert_eq!(
        index.resolve("schema-g:11").cloned(),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );

    let sequence_wins = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[],
        &[(11, 33)],
        &[],
    )
    .expect("topology index");
    assert_eq!(
        sequence_wins.resolve("schema-h:11").cloned(),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );

    let unresolved = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(11, 999)],
        &[],
        &[],
    )
    .expect("topology index");
    assert!(unresolved.resolve("schema-i:11").cloned().is_none());

    let conflicting = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[
                face.clone(),
                Face {
                    id: FaceId::mint("sldprt:brep:face#23").expect("identity grammar"),
                    ..face.clone()
                },
            ],
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(77, 22), (77, 23)],
        &[],
        &[],
    )
    .expect("topology index");
    assert!(conflicting.resolve("schema-j:77").cloned().is_none());

    let conflicting_families = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(88, 22)],
        &[(88, 33)],
        &[],
    )
    .expect("topology index");
    assert!(conflicting_families
        .resolve("schema-k:88")
        .cloned()
        .is_none());
}

fn topology_index_limit_error(
    set_limit: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
) -> CodecError {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    let body = Body {
        id: BodyId::mint("sldprt:brep:body#11").expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    set_limit(&mut policy.limits);
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: &[body],
            faces: &[],
            edges: &[],
            vertices: &[],
        },
        &[],
        &[],
        &[],
    )
    .expect_err("topology index must refuse")
}

#[test]
fn swift_topology_index_refuses_work_limit() {
    let CodecError::ResourceLimit(limit) =
        topology_index_limit_error(|limits| limits.max_work_units = 0)
    else {
        panic!("expected work refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
}

#[test]
fn swift_topology_index_refuses_retained_limit() {
    let CodecError::ResourceLimit(limit) =
        topology_index_limit_error(|limits| limits.max_retained_bytes = 0)
    else {
        panic!("expected retained refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
}

#[test]
fn swift_topology_index_refuses_collection_limit() {
    let CodecError::ResourceLimit(limit) =
        topology_index_limit_error(|limits| limits.max_collection_items = 0)
    else {
        panic!("expected collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}
