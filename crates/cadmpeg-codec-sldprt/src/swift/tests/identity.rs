use super::*;

#[test]
fn empty_swift_pattern_uses_one_native_hole_join() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureId, PatternKind, PatternSeed};

    let seed = FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar");
    let pattern_definition = FeatureDefinition::Pattern {
        seeds: vec![PatternSeed::Feature(seed.clone())],
        pattern: PatternKind::UNRESOLVED,
    };
    let features = vec![
        neutral_feature(
            "seed",
            "Sketch20",
            1,
            Vec::new(),
            FeatureDefinition::Native {
                kind: "Sketch".into(),
                parameters: BTreeMap::new(),
            },
        ),
        neutral_feature("pattern", "LPattern6", 2, Vec::new(), pattern_definition),
        neutral_feature(
            "hole",
            "Hole5",
            3,
            vec![seed],
            simple_hole_definition(6.1468),
        ),
    ];
    let context = pattern_hole_nominal_context(&features);
    assert_eq!(context.get("Hole Pattern6"), Some(&6.1468));

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
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &projected.first().expect("diameter annotation").definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(6.1468).expect("finite length"));

    let mut ambiguous = features.clone();
    ambiguous.push(neutral_feature(
        "hole2",
        "Hole6",
        4,
        vec![FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar")],
        simple_hole_definition(6.1468),
    ));
    assert!(pattern_hole_nominal_context(&ambiguous).is_empty());

    let mut unresolved = features;
    let mut unresolved_hole = neutral_feature(
        "hole2",
        "Hole6",
        4,
        vec![FeatureId::mint("sldprt:model:feature#seed").expect("identity grammar")],
        simple_hole_definition(6.1468),
    );
    unresolved_hole
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::Hole { shape, .. } = definition else {
                panic!("expected hole definition");
            };
            shape.try_edit(|_, _, diameter| *diameter = None).unwrap();
        })
        .unwrap();
    unresolved.push(unresolved_hole);
    assert!(pattern_hole_nominal_context(&unresolved).is_empty());
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
    let projected = project_with_topology(&root, Some(&index), &[], None);
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
    let projected = project_with_topology(&root, Some(&index), &[], None);
    let first = projected.first().expect("projected datum");
    assert_eq!(
        first.targets,
        [PmiTarget::ShapeAspect {
            source_id: cadmpeg_ir::products::NonEmptyString::new("F10")
                .expect("nonempty source identity")
        }]
    );
}

#[test]
fn cad_identifier_resolves_each_primary_topology_kind_and_rejects_collisions() {
    use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::topology::{Body, BodyKind, Edge, Face, Sense, Vertex};

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
        loops: Vec::new().into(),
        name: None,
        color: None,
        tolerance: None,
    };
    let edge = Edge {
        id: EdgeId::mint("sldprt:brep:edge#33").expect("identity grammar"),
        curve: None,
        start: VertexId::mint("sldprt:brep:vertex#1").expect("identity grammar"),
        end: VertexId::mint("sldprt:brep:vertex#2").expect("identity grammar"),
        param_range: None,
        tolerance: None,
    };
    let vertex = Vertex {
        id: VertexId::mint("sldprt:brep:vertex#44").expect("identity grammar"),
        point: PointId::mint("sldprt:brep:point#44").expect("identity grammar"),
        tolerance: None,
    };
    let index = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        std::slice::from_ref(&face),
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[(222, 22)],
        &[(333, 33)],
        &[(444, 44)],
    );

    assert_eq!(
        index.resolve("schema-a:11"),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-b:222"),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(index.resolve("schema-b:22").is_none());
    assert_eq!(
        index.resolve("schema-c:33"),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-d:44"),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-e:333"),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );
    assert_eq!(
        index.resolve("schema-f:444"),
        Some(PmiTarget::Vertex {
            vertex: vertex.id.clone(),
        })
    );

    let qualified_alternate = Face {
        id: FaceId::mint("sldprt:brep:face#22@alternate").expect("identity grammar"),
        ..face.clone()
    };
    let active_with_alternate = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        &[face.clone(), qualified_alternate],
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[(222, 22)],
        &[],
        &[],
    );
    assert_eq!(
        active_with_alternate.resolve("schema-g:222"),
        Some(PmiTarget::Face {
            face: face.id.clone(),
        })
    );
    assert!(index.resolve("schema-e:not-a-number").is_none());
    assert!(index.resolve("11").is_none());

    let collision = Face {
        id: FaceId::mint("sldprt:brep:face#11").expect("identity grammar"),
        ..face.clone()
    };
    let index = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        &[collision],
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[],
        &[],
        &[],
    );
    assert_eq!(
        index.resolve("schema-g:11"),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );

    let sequence_wins = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        std::slice::from_ref(&face),
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[],
        &[(11, 33)],
        &[],
    );
    assert_eq!(
        sequence_wins.resolve("schema-h:11"),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );

    let unresolved = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        std::slice::from_ref(&face),
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[(11, 999)],
        &[],
        &[],
    );
    assert!(unresolved.resolve("schema-i:11").is_none());

    let conflicting = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        &[
            face.clone(),
            Face {
                id: FaceId::mint("sldprt:brep:face#23").expect("identity grammar"),
                ..face.clone()
            },
        ],
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[(77, 22), (77, 23)],
        &[],
        &[],
    );
    assert!(conflicting.resolve("schema-j:77").is_none());

    let conflicting_families = TopologyIdentityIndex::from_model(
        std::slice::from_ref(&body),
        std::slice::from_ref(&face),
        std::slice::from_ref(&edge),
        std::slice::from_ref(&vertex),
        &[(88, 22)],
        &[(88, 33)],
        &[],
    );
    assert!(conflicting_families.resolve("schema-k:88").is_none());
}
