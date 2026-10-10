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

pub(super) fn pattern_hole_features() -> Vec<Feature> {
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

pub(super) fn cad_feature(class: &str, identifier: &str) -> Entity {
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

    let sequence_face = FaceId::mint("sldprt:brep:face#42").expect("identity grammar");
    let mut index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: &[],
            faces: &[],
            edges: &[],
            vertices: &[],
        },
        &[],
        &[],
        &[],
    )
    .expect("empty topology index carries its storage admission");
    let TopologyIdentityIndex {
        sequence_targets,
        _storage: storage,
        ..
    } = &mut index;
    storage
        .with_storage(|| {
            TopologyIdentityIndex::insert_sequence_target(
                &ctx,
                sequence_targets,
                42,
                Some(crate::swift::TopologyTarget::Face(&sequence_face)),
            )
        })
        .expect("sequence fixture uses the index storage admission");
    let projected = project_with_topology(&ctx, &root, Some(&index), &[], None, None)
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
    let projected = project_with_topology(&ctx, &root, Some(&index), &[], None, None)
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
    for operation in [
        "index SWIFT face attribute identities",
        "index SWIFT edge attribute identities",
        "index SWIFT vertex attribute identities",
    ] {
        let probe_arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (probe_ctx, _) = DecodeContext::from_root_bytes(&[], &probe_arena, &policy).unwrap();
        let topology = crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        };
        let skipped_probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let no_sequences = TopologyIdentityIndex::from_model(&probe_ctx, topology, &[], &[], &[])
            .expect("primary identities do not need attribute joins");
        assert!(
            crate::swift::ProjectionGeometry::new(&probe_ctx, Some(&no_sequences), None, None)
                .resolve("schema:22")
                .unwrap()
                .is_none()
        );
        assert!(
            crate::swift::ProjectionGeometry::new(&probe_ctx, Some(&no_sequences), None, None)
                .resolve("schema:11")
                .unwrap()
                .is_some()
        );
        drop((no_sequences, skipped_probe));
        assert!(probe_ctx.resource_refusal().is_none());

        let selected_probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let result = TopologyIdentityIndex::from_model(
            &probe_ctx,
            topology,
            &[(222, 22)],
            &[(333, 33)],
            &[(444, 44)],
        );
        drop(selected_probe);
        assert!(matches!(result, Err(CodecError::ResourceLimit(ref limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
        drop(result);
        assert!(probe_ctx.finish_session().is_err());
    }
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
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-a:11")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-b:222")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-b:22")
            .unwrap()
            .map(owned_topology_target)
            .is_none()
    );
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-c:33")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-d:44")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-e:333")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-f:444")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );

    let qualified_alternate = Face {
        id: FaceId::mint("sldprt:brep:face#22@alternate").expect("identity grammar"),
        ..face.clone()
    };
    let alternate_faces = [face.clone(), qualified_alternate];
    let active_with_alternate = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &alternate_faces,
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(222, 22)],
        &[],
        &[],
    )
    .expect("topology index");
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&active_with_alternate), None, None)
            .resolve("schema-g:222")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-e:not-a-number")
            .unwrap()
            .map(owned_topology_target)
            .is_none()
    );
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("11")
            .unwrap()
            .is_none()
    );

    let collision = Face {
        id: FaceId::mint("sldprt:brep:face#11").expect("identity grammar"),
        ..face.clone()
    };
    let index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&collision),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[],
        &[],
        &[],
    )
    .expect("topology index");
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema-g:11")
            .unwrap()
            .map(owned_topology_target),
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
        crate::swift::ProjectionGeometry::new(&ctx, Some(&sequence_wins), None, None)
            .resolve("schema-h:11")
            .unwrap()
            .map(owned_topology_target),
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
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&unresolved), None, None)
            .resolve("schema-i:11")
            .unwrap()
            .map(owned_topology_target)
            .is_none()
    );

    let conflicting_faces = [
        face.clone(),
        Face {
            id: FaceId::mint("sldprt:brep:face#23").expect("identity grammar"),
            ..face.clone()
        },
    ];
    let conflicting = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &conflicting_faces,
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(77, 22), (77, 23)],
        &[],
        &[],
    )
    .expect("topology index");
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&conflicting), None, None)
            .resolve("schema-j:77")
            .unwrap()
            .map(owned_topology_target)
            .is_none()
    );

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
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&conflicting_families), None, None)
            .resolve("schema-k:88")
            .unwrap()
            .map(owned_topology_target)
            .is_none()
    );
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
fn swift_topology_index_refuses_materialized_limit() {
    let CodecError::ResourceLimit(limit) = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "collect SWIFT primary topology targets",
        |cap| {
            Err::<(), _>(topology_index_limit_error(|limits| {
                limits.max_materialized_bytes = cap;
            }))
        },
    ) else {
        panic!("expected materialized refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SWIFT primary topology targets");
}

#[test]
fn swift_topology_index_scratch_does_not_consume_retained_budget() {
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
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[],
            edges: &[],
            vertices: &[],
        },
        &[],
        &[],
        &[],
    )
    .expect("scratch identity index uses the materialized budget");
    assert_eq!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema:11")
            .unwrap()
            .map(owned_topology_target),
        Some(PmiTarget::Body {
            body: body.id.clone()
        })
    );

    let error = crate::swift::TopologyTarget::Body(&body.id)
        .copy_for_decode(&ctx)
        .expect_err("final PMI output identity remains retained");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "copy SWIFT topology identity");
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

#[test]
fn pattern_hole_ambiguity_skips_unvisited_dependents() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for tail in [1, 128] {
        let mut features = pattern_hole_features();
        let hole = features.get(2).expect("hole fixture").clone();
        for ordinal in 4..=4 + tail {
            let mut additional = hole.clone();
            additional.id =
                FeatureId::mint(format!("sldprt:model:feature#hole-{ordinal}")).unwrap();
            additional.ordinal = ordinal;
            features.push(additional);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "swift pattern hole search",
            Some(tail + 2),
        );
        assert!(pattern_hole_nominal_context(&ctx, &features)
            .unwrap()
            .is_empty());
        ctx.finish_session().unwrap();
    }
}

fn owned_topology_target(target: &crate::swift::TopologyTarget<'_>) -> PmiTarget {
    match target {
        crate::swift::TopologyTarget::Body(id) => PmiTarget::Body {
            body: (*id).clone(),
        },
        crate::swift::TopologyTarget::Face(id) => PmiTarget::Face {
            face: (*id).clone(),
        },
        crate::swift::TopologyTarget::Edge(id) => PmiTarget::Edge {
            edge: (*id).clone(),
        },
        crate::swift::TopologyTarget::Vertex(id) => PmiTarget::Vertex {
            vertex: (*id).clone(),
        },
    }
}

#[test]
fn swift_topology_index_never_copies_rejected_or_duplicate_identities() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::Body;
    let body = Body {
        id: BodyId::mint("sldprt:brep:body#11").unwrap(),
        kind: cadmpeg_ir::topology::BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut bodies = vec![body.clone(); 64];
    bodies.push(Body {
        id: BodyId::mint(format!("sldprt:brep:body#77@{}", "qualified".repeat(64))).unwrap(),
        ..body
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "copy SWIFT topology identity",
        None,
    );
    let index = TopologyIdentityIndex::from_model(
        &ctx,
        crate::swift::PrimaryTopology {
            bodies: &bodies,
            faces: &[],
            edges: &[],
            vertices: &[],
        },
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries.get(&11).unwrap().len(), 1);
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema:11")
            .unwrap()
            .is_some()
    );
    assert!(
        crate::swift::ProjectionGeometry::new(&ctx, Some(&index), None, None)
            .resolve("schema:77")
            .unwrap()
            .is_none()
    );
    drop(index);
    ctx.finish_session().unwrap();
}
