use super::entity;
use super::reference;
use crate::swift::positions_by_id_prefix;
use crate::swift::project_with_topology;
use crate::swift::Entity;
use crate::swift::FeatureIndex;
use crate::swift::RelatedObject;
use crate::swift::TopologyIdentityIndex;
use crate::swift::ROOT_CLASS;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::pmi::PmiDefinition;
use cadmpeg_ir::pmi::PmiTarget;

#[test]
fn id_prefix_positions_match_the_prefix_filter() {
    fn text(next: &mut impl FnMut(u64) -> usize, length: u64) -> String {
        (0..next(length))
            .filter_map(|_| ['a', 'b', ':'].get(next(3)).copied())
            .collect()
    }

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((state >> 33) % bound).unwrap()
    };
    for _ in 0..256 {
        let ids = (0..next(12))
            .map(|_| text(&mut next, 6))
            .collect::<Vec<_>>();
        let index = positions_by_id_prefix(ids.iter().map(String::as_str));
        let prefixes = (0..8)
            .map(|_| text(&mut next, 4))
            .collect::<Vec<_>>()
            .into_iter()
            .chain(ids.iter().cloned());
        for prefix in prefixes {
            let expected = ids
                .iter()
                .enumerate()
                .filter(|(_, id)| {
                    **id == prefix
                        || id
                            .strip_prefix(&prefix)
                            .is_some_and(|suffix| suffix.starts_with(':'))
                })
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            assert_eq!(
                index.get(prefix.as_str()).cloned().unwrap_or_default(),
                expected,
                "{ids:?} {prefix:?}"
            );
        }
    }
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

fn cad_reference_collection(identifiers: &[&str]) -> RelatedObject {
    let mut collection = entity("CadRefCollection");
    for (ordinal, identifier) in identifiers.iter().enumerate() {
        let mut cad_ref = entity("CadRef");
        cad_ref
            .strings
            .insert("CadIdentifier".into(), (*identifier).into());
        collection.related.push(RelatedObject {
            name: format!("CadRef{ordinal}"),
            class: "PrizMetrik.GdtAnalysis.CadRef".into(),
            entity: cad_ref,
        });
    }
    RelatedObject {
        name: "CadReferences".into(),
        class: "PrizMetrik.GdtAnalysis.CadRefCollection".into(),
        entity: collection,
    }
}

fn applied_feature(ordinal: usize, id: &str, class: &str) -> RelatedObject {
    let mut applied = entity("GdtAppliedFeature");
    applied.features.references.push(reference(id, class));
    RelatedObject {
        name: format!("SubFeature{ordinal}"),
        class: applied.class.clone(),
        entity: applied,
    }
}

fn compound_hole_fixture(
    applied: &[(&str, &str)],
    direct: &[(&str, &str)],
    aggregate: &[&str],
) -> Entity {
    let mut compound = entity("GdtCompoundHole");
    compound.features.references = direct
        .iter()
        .map(|(id, class)| reference(id, class))
        .collect();
    let mut subfeatures = entity("GdtAppliedFeatureCollection");
    subfeatures.related = applied
        .iter()
        .enumerate()
        .map(|(ordinal, (id, class))| applied_feature(ordinal, id, class))
        .collect();
    compound.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: subfeatures.class.clone(),
        entity: subfeatures,
    });
    compound.related.push(cad_reference_collection(aggregate));
    compound
}

fn target_features(features: &[(String, Entity)]) -> FeatureIndex<'_> {
    FeatureIndex::new(
        features
            .iter()
            .map(|(id, entity)| (id.as_str(), entity))
            .collect(),
    )
}

fn face_target(suffix: &str) -> PmiTarget {
    PmiTarget::Face {
        face: FaceId::mint(format!("sldprt:brep:face#{suffix}")).expect("face identity grammar"),
    }
}

fn shape_aspect(source_id: &str) -> PmiTarget {
    crate::swift::shape_aspect_target(source_id).expect("shape aspect identity")
}

fn position_target(id: &str) -> Entity {
    let mut position = entity("GdtPosition");
    position
        .features
        .references
        .push(reference(id, "GdtCompoundHole"));
    position
}

fn target_selection(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &crate::swift::FeatureIndex<'_>,
    topology: Option<&TopologyIdentityIndex>,
) -> (Option<Vec<PmiTarget>>, bool) {
    if crate::swift::short_class(&annotation.class) == "GdtPosition" {
        let selection = crate::swift::position_targets(ctx, annotation, feature_index, topology)
            .expect("position target selection fits policy");
        selection.map_or((None, false), |selection| {
            (Some(selection.targets), selection.unresolved)
        })
    } else {
        let targets = crate::swift::targets(ctx, annotation, feature_index, topology)
            .expect("generic target selection fits policy");
        (targets, false)
    }
}

fn pattern_fixture(children: &[(&str, &str)]) -> Entity {
    let mut pattern = entity("GdtPattern");
    let mut subfeatures = entity("GdtAppliedFeatureCollection");
    subfeatures.related = children
        .iter()
        .enumerate()
        .map(|(ordinal, (id, class))| {
            let mut applied = applied_feature(ordinal, id, class);
            applied.class = "PrizMetrik.GdtAnalysis.GdtAppliedFeature".into();
            applied
        })
        .collect();
    pattern.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
        entity: subfeatures,
    });
    pattern
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
    let projected = project_with_topology(&ctx, &root, Some(&index), &[])
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
    let projected = project_with_topology(&ctx, &root, Some(&index), &[])
        .expect("test projection fits policy")
        .annotations;
    let first = projected.first().expect("projected datum");
    assert_eq!(
        first.targets,
        [PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("F10")
                .expect("nonempty source identity")
        }]
    );
}

#[test]
fn cad_identifier_resolves_each_primary_topology_kind_and_rejects_collisions() {
    use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::topology::{Body, BodyKind, Edge, Face, Sense, Vertex};

    let body = Body {
        removed_by: None,
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
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(222, 22)],
        &[(333, 33)],
        &[(444, 44)],
    );

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
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[face.clone(), qualified_alternate],
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(222, 22)],
        &[],
        &[],
    );
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
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: &[collision],
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[],
        &[],
        &[],
    );
    assert_eq!(
        index.resolve("schema-g:11").cloned(),
        Some(PmiTarget::Body {
            body: body.id.clone(),
        })
    );

    let sequence_wins = TopologyIdentityIndex::from_model(
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[],
        &[(11, 33)],
        &[],
    );
    assert_eq!(
        sequence_wins.resolve("schema-h:11").cloned(),
        Some(PmiTarget::Edge {
            edge: edge.id.clone(),
        })
    );

    let unresolved = TopologyIdentityIndex::from_model(
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(11, 999)],
        &[],
        &[],
    );
    assert!(unresolved.resolve("schema-i:11").cloned().is_none());

    let conflicting = TopologyIdentityIndex::from_model(
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
    );
    assert!(conflicting.resolve("schema-j:77").cloned().is_none());

    let conflicting_families = TopologyIdentityIndex::from_model(
        crate::swift::PrimaryTopology {
            bodies: std::slice::from_ref(&body),
            faces: std::slice::from_ref(&face),
            edges: std::slice::from_ref(&edge),
            vertices: std::slice::from_ref(&vertex),
        },
        &[(88, 22)],
        &[(88, 33)],
        &[],
    );
    assert!(conflicting_families
        .resolve("schema-k:88")
        .cloned()
        .is_none());
}

#[test]
fn position_compound_hole_targets_only_applied_cylinders() {
    let compound = compound_hole_fixture(
        &[
            ("cylinder-a", "GdtCylinder"),
            ("cylinder-b", "GdtCylinder"),
            ("floor", "GdtPlane"),
            ("cone", "GdtCone"),
            ("cylinder-a", "GdtCylinder"),
        ],
        &[("support", "GdtPlane"), ("cone", "GdtCone")],
        &[
            "schema:100",
            "schema:101",
            "schema:102",
            "schema:103",
            "schema:104",
        ],
    );
    let features = vec![
        ("hole".into(), compound),
        (
            "cylinder-a".into(),
            cad_feature("GdtCylinder", "schema:100"),
        ),
        (
            "cylinder-b".into(),
            cad_feature("GdtCylinder", "schema:101"),
        ),
        ("floor".into(), cad_feature("GdtPlane", "schema:102")),
        ("cone".into(), cad_feature("GdtCone", "schema:103")),
        ("support".into(), cad_feature("GdtPlane", "schema:104")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    for suffix in [100, 101, 102, 103, 104] {
        topology
            .entries
            .insert(suffix, vec![face_target(&suffix.to_string())]);
    }
    let position = position_target("hole");
    let ctx = cadmpeg_test_support::service_decode_context();

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (Some(vec![face_target("100"), face_target("101")]), false)
    );
}

#[test]
fn position_compound_hole_keeps_distinct_identifiers_from_one_cylinder() {
    let compound = compound_hole_fixture(
        &[("cylinder", "GdtCylinder"), ("cylinder", "GdtCylinder")],
        &[],
        &["schema:700", "schema:701", "schema:700"],
    );
    let mut cylinder = entity("GdtCylinder");
    cylinder.related.push(cad_reference_collection(&[
        "schema:700",
        "schema:701",
        "schema:700",
    ]));
    let features: Vec<(String, Entity)> =
        vec![("hole".into(), compound), ("cylinder".into(), cylinder)];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(700, vec![face_target("700")]);
    topology.entries.insert(701, vec![face_target("701")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let position = position_target("hole");

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (Some(vec![face_target("700"), face_target("701")]), false)
    );
}

#[test]
fn position_compound_hole_scales_with_identifier_work_and_keeps_target_order() {
    const IDENTIFIER_COUNT: usize = 4096;
    const FIRST_SUFFIX: u64 = 10_000;
    let suffix_for_index = |index: usize| {
        FIRST_SUFFIX
            .checked_add(u64::try_from(index).expect("identifier index fits u64"))
            .expect("identifier suffix fits u64")
    };

    let identifiers = (0..IDENTIFIER_COUNT)
        .map(|index| format!("schema:{}", suffix_for_index(index)))
        .collect::<Vec<_>>();
    let mut identifier_refs = identifiers.iter().map(String::as_str).collect::<Vec<_>>();
    let repeated_identifier = identifier_refs
        .first()
        .copied()
        .expect("identifier list is non-empty");
    identifier_refs.push(repeated_identifier);
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &identifier_refs);
    let mut cylinder = entity("GdtCylinder");
    cylinder
        .related
        .push(cad_reference_collection(&identifier_refs));
    let features: Vec<(String, Entity)> =
        vec![("hole".into(), compound), ("cylinder".into(), cylinder)];
    let feature_index = crate::swift::FeatureIndex::with_work_limit(
        features
            .iter()
            .map(|(id, feature)| (id.as_str(), feature))
            .collect(),
        7 + 5 * identifier_refs.len(),
    );
    let mut topology = TopologyIdentityIndex::default();
    for index in 0..IDENTIFIER_COUNT {
        let suffix = suffix_for_index(index);
        topology
            .entries
            .insert(suffix, vec![face_target(&suffix.to_string())]);
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let position = position_target("hole");
    let expected = (0..IDENTIFIER_COUNT)
        .map(|index| face_target(&suffix_for_index(index).to_string()))
        .collect::<Vec<_>>();

    assert_eq!(
        target_selection(&ctx, &position, &feature_index, Some(&topology)),
        (Some(expected), false)
    );
    assert_eq!(feature_index.caps().work, 0);

    let mapping_cap_index = crate::swift::FeatureIndex::with_work_limit(
        features
            .iter()
            .map(|(id, feature)| (id.as_str(), feature))
            .collect(),
        7 + 3 * identifier_refs.len(),
    );
    assert_eq!(
        target_selection(&ctx, &position, &mapping_cap_index, Some(&topology)),
        (Some(vec![shape_aspect("hole")]), true)
    );
    assert_eq!(mapping_cap_index.caps().work, 1);
}

#[test]
fn position_compound_hole_keeps_cylinder_targets_with_ambiguous_ownership() {
    let compound = compound_hole_fixture(
        &[("cylinder", "GdtCylinder"), ("plane", "GdtPlane")],
        &[],
        &["schema:501"],
    );
    let features = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:501")),
        ("plane".into(), cad_feature("GdtPlane", "schema:501")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(501, vec![face_target("501")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let position = position_target("hole");

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (
            Some(vec![face_target("501"), shape_aspect("schema:501")]),
            true
        )
    );
}

#[test]
fn position_compound_hole_preserves_missing_and_ambiguous_identifiers() {
    let missing = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &[]);
    let missing_features = vec![
        ("hole".into(), missing),
        ("cylinder".into(), cad_feature("GdtCylinder", "")),
    ];
    let missing_index = target_features(&missing_features);
    let position = position_target("hole");
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        target_selection(&ctx, &position, &missing_index, None),
        (Some(vec![shape_aspect("cylinder")]), true)
    );

    let ambiguous = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:502"]);
    let ambiguous_features = vec![
        ("hole".into(), ambiguous),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:502")),
    ];
    let ambiguous_index = target_features(&ambiguous_features);
    let mut topology = TopologyIdentityIndex::default();
    topology
        .entries
        .insert(502, vec![face_target("502a"), face_target("502b")]);
    assert_eq!(
        target_selection(&ctx, &position, &ambiguous_index, Some(&topology)),
        (Some(vec![shape_aspect("schema:502")]), true)
    );
}

#[test]
fn position_missing_compound_hole_reference_preserves_aspect_and_loss() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let feature_index = target_features(&[]);
    let mut position = position_target("missing-hole");
    assert_eq!(
        target_selection(&ctx, &position, &feature_index, None),
        (Some(vec![shape_aspect("missing-hole")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A90", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, None, &[])
        .expect("missing compound-hole feature stays a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("missing-hole")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn position_compound_hole_rejects_non_face_topology_targets() {
    use cadmpeg_ir::ids::BodyId;

    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:505"]);
    let features = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:505")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(
        505,
        vec![PmiTarget::Body {
            body: BodyId::mint("sldprt:brep:body#505").expect("body identity grammar"),
        }],
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("hole");

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (Some(vec![shape_aspect("schema:505")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A95", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, Some(&topology), &[])
        .expect("non-face topology target stays a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("schema:505")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn position_compound_hole_reference_class_mismatch_preserves_the_reference() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:506"]);
    let features = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:506")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(506, vec![face_target("506")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("hole");
    position
        .features
        .references
        .first_mut()
        .expect("position target reference")
        .class = reference("hole", "GdtPlane").class;

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (Some(vec![shape_aspect("hole")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A94", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, Some(&topology), &[])
        .expect("mismatched compound reference stays a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("hole")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn position_pattern_mismatched_subfeature_reference_preserves_pattern_aspect() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:507"]);
    let pattern = pattern_fixture(&[("hole", "GdtPlane")]);
    let features = vec![
        ("pattern".into(), pattern),
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:507")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(507, vec![face_target("507")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("pattern");
    position
        .features
        .references
        .first_mut()
        .expect("position target reference")
        .class = reference("pattern", "GdtPattern").class;

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (Some(vec![shape_aspect("pattern")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A92", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, Some(&topology), &[])
        .expect("mismatched pattern child reference stays a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("pattern")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn position_pattern_reference_class_mismatch_preserves_pattern_aspect() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:508"]);
    let pattern = pattern_fixture(&[("hole", "GdtCompoundHole")]);
    let features = vec![
        ("pattern".into(), pattern),
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:508")),
    ];
    let index = target_features(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("pattern");
    position
        .features
        .references
        .first_mut()
        .expect("position target reference")
        .class = reference("pattern", "GdtPlane").class;

    assert_eq!(
        target_selection(&ctx, &position, &index, None),
        (Some(vec![shape_aspect("pattern")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A91", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, None, &[])
        .expect("mismatched pattern reference stays a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("pattern")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn malformed_position_pattern_class_edges_preserve_pattern_aspect() {
    let mut wrong_collection_declared_class = pattern_fixture(&[("hole", "GdtCompoundHole")]);
    wrong_collection_declared_class
        .related
        .first_mut()
        .expect("SubFeatures relation")
        .class = "PrizMetrik.GdtAnalysis.GdtPlane".into();
    let mut wrong_collection_entity_class = pattern_fixture(&[("hole", "GdtCompoundHole")]);
    wrong_collection_entity_class
        .related
        .first_mut()
        .expect("SubFeatures relation")
        .entity
        .class = "GdtPlane".into();
    let mut wrong_applied_declared_class = pattern_fixture(&[("hole", "GdtCompoundHole")]);
    wrong_applied_declared_class
        .related
        .first_mut()
        .expect("SubFeatures relation")
        .entity
        .related
        .first_mut()
        .expect("applied feature relation")
        .class = "PrizMetrik.GdtAnalysis.GdtPlane".into();
    let mut wrong_applied_entity_class = pattern_fixture(&[("hole", "GdtCompoundHole")]);
    wrong_applied_entity_class
        .related
        .first_mut()
        .expect("SubFeatures relation")
        .entity
        .related
        .first_mut()
        .expect("applied feature relation")
        .entity
        .class = "GdtPlane".into();

    for (index, pattern) in [
        wrong_collection_declared_class,
        wrong_collection_entity_class,
        wrong_applied_declared_class,
        wrong_applied_entity_class,
    ]
    .into_iter()
    .enumerate()
    {
        let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:509"]);
        let features = vec![
            ("pattern".into(), pattern),
            ("hole".into(), compound),
            ("cylinder".into(), cad_feature("GdtCylinder", "schema:509")),
        ];
        let feature_index = target_features(&features);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut position = position_target("pattern");
        position
            .features
            .references
            .first_mut()
            .expect("position target reference")
            .class = reference("pattern", "GdtPattern").class;
        assert_eq!(
            target_selection(&ctx, &position, &feature_index, None),
            (Some(vec![shape_aspect("pattern")]), true)
        );

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        for (id, feature) in &features {
            root.features
                .references
                .push(reference(id, crate::swift::short_class(&feature.class)));
            root.features.entities.push(feature.clone());
        }
        position.doubles.insert("Tolerance".into(), 0.1);
        root.annotations
            .references
            .push(reference(&format!("A{index}"), "GdtPosition"));
        root.annotations.entities.push(position);
        let projection = project_with_topology(&ctx, &root, None, &[])
            .expect("malformed pattern classes stay a projection result");
        let [annotation] = projection.annotations.as_slice() else {
            panic!("position annotation is retained")
        };
        assert_eq!(annotation.targets, vec![shape_aspect("pattern")]);
        assert_eq!(projection.unresolved_position_targets, 1);
        let mut losses = Vec::new();
        projection.append_losses(&mut losses);
        assert!(losses
            .iter()
            .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
    }
}

#[test]
fn duplicate_position_pattern_subfeature_names_preserve_pattern_aspect() {
    let mut pattern =
        pattern_fixture(&[("hole-a", "GdtCompoundHole"), ("hole-b", "GdtCompoundHole")]);
    let subfeatures = pattern.related.first().expect("SubFeatures relation");
    let first_name = subfeatures
        .entity
        .related
        .first()
        .expect("first applied feature")
        .name
        .clone();
    pattern
        .related
        .first_mut()
        .expect("SubFeatures relation")
        .entity
        .related
        .get_mut(1)
        .expect("second applied feature")
        .name = first_name;
    let features = vec![
        ("pattern".into(), pattern),
        ("hole-a".into(), entity("GdtCompoundHole")),
        ("hole-b".into(), entity("GdtCompoundHole")),
    ];
    let feature_index = target_features(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("pattern");
    position
        .features
        .references
        .first_mut()
        .expect("position target reference")
        .class = reference("pattern", "GdtPattern").class;
    assert_eq!(
        target_selection(&ctx, &position, &feature_index, None),
        (Some(vec![shape_aspect("pattern")]), true)
    );

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A89", "GdtPosition"));
    root.annotations.entities.push(position);
    let projection = project_with_topology(&ctx, &root, None, &[])
        .expect("duplicate pattern relation names stay a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("pattern")]);
    assert_eq!(projection.unresolved_position_targets, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn position_compound_hole_uses_child_ids_and_preserves_unknown_aggregate_ids() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:999"]);
    let features = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:503")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(503, vec![face_target("503")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let position = position_target("hole");

    assert_eq!(
        target_selection(&ctx, &position, &index, Some(&topology)),
        (
            Some(vec![face_target("503"), shape_aspect("schema:999")]),
            true
        )
    );
}

#[test]
fn non_position_annotations_keep_generic_compound_hole_targets() {
    let compound = compound_hole_fixture(
        &[("cylinder", "GdtCylinder"), ("floor", "GdtPlane")],
        &[("support", "GdtPlane")],
        &["schema:601", "schema:602", "schema:603"],
    );
    let features = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:601")),
        ("floor".into(), cad_feature("GdtPlane", "schema:602")),
        ("support".into(), cad_feature("GdtPlane", "schema:603")),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    for suffix in [601, 602, 603] {
        topology
            .entries
            .insert(suffix, vec![face_target(&suffix.to_string())]);
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut annotation = position_target("hole");
    annotation.class = "PrizMetrik.GdtAnalysis.GdtFlatness".into();

    assert_eq!(
        target_selection(&ctx, &annotation, &index, Some(&topology)),
        (
            Some(vec![
                face_target("601"),
                face_target("602"),
                face_target("603")
            ]),
            false
        )
    );
}

#[test]
fn generic_target_walk_preserves_per_feature_duplicate_behavior() {
    let mut first = entity("GdtCylinder");
    first
        .related
        .push(cad_reference_collection(&["schema:610", "schema:610"]));
    let mut second = entity("GdtCylinder");
    second
        .related
        .push(cad_reference_collection(&["schema:610"]));
    let features = vec![("first".into(), first), ("second".into(), second)];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(610, vec![face_target("610")]);
    let mut annotation = entity("GdtFlatness");
    annotation
        .features
        .references
        .push(reference("first", "GdtCylinder"));
    annotation
        .features
        .references
        .push(reference("second", "GdtCylinder"));

    assert_eq!(
        target_selection(
            &cadmpeg_test_support::service_decode_context(),
            &annotation,
            &index,
            Some(&topology),
        ),
        (Some(vec![face_target("610"), face_target("610")]), false)
    );
}

#[test]
fn position_compound_and_generic_leaf_duplicates_are_order_independent() {
    let mut first = entity("GdtCylinder");
    first.related.push(cad_reference_collection(&[
        "schema:612",
        "schema:613",
        "schema:612",
    ]));
    let second = cad_feature("GdtCylinder", "schema:612");
    let compound = compound_hole_fixture(&[("hole-cylinder", "GdtCylinder")], &[], &["schema:612"]);
    let features = vec![
        ("first".into(), first),
        ("second".into(), second),
        ("hole".into(), compound),
        (
            "hole-cylinder".into(),
            cad_feature("GdtCylinder", "schema:612"),
        ),
    ];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(612, vec![face_target("612")]);
    topology.entries.insert(613, vec![face_target("613")]);
    let orders = [
        [
            ("first", "GdtCylinder"),
            ("second", "GdtCylinder"),
            ("hole", "GdtCompoundHole"),
        ],
        [
            ("hole", "GdtCompoundHole"),
            ("first", "GdtCylinder"),
            ("second", "GdtCylinder"),
        ],
    ];

    for order in orders {
        let mut position = entity("GdtPosition");
        position.features.references = order
            .into_iter()
            .map(|(id, class)| reference(id, class))
            .collect();
        let starts_with_hole = order.first().is_some_and(|(id, _)| *id == "hole");
        let expected = if starts_with_hole {
            vec![
                face_target("612"),
                face_target("612"),
                face_target("613"),
                face_target("612"),
            ]
        } else {
            vec![
                face_target("612"),
                face_target("613"),
                face_target("612"),
                face_target("612"),
            ]
        };
        assert_eq!(
            target_selection(
                &cadmpeg_test_support::service_decode_context(),
                &position,
                &index,
                Some(&topology),
            ),
            (Some(expected), false)
        );
    }
}

#[test]
fn position_non_compound_leaf_keeps_generic_targets() {
    let features = vec![("cylinder".into(), cad_feature("GdtCylinder", "schema:611"))];
    let index = target_features(&features);
    let mut topology = TopologyIdentityIndex::default();
    topology.entries.insert(611, vec![face_target("611")]);
    let mut position = position_target("cylinder");
    *position
        .features
        .references
        .first_mut()
        .expect("position target reference") = reference("cylinder", "GdtCylinder");

    assert_eq!(
        target_selection(
            &cadmpeg_test_support::service_decode_context(),
            &position,
            &index,
            Some(&topology),
        ),
        (Some(vec![face_target("611")]), false)
    );
}

#[test]
fn malformed_position_compound_hole_ownership_retains_semantic_pmi() {
    let valid = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:504"]);
    let subfeatures = valid
        .related
        .iter()
        .find(|related| related.name == "SubFeatures")
        .expect("SubFeatures collection")
        .clone();
    let mut missing_subfeatures = valid.clone();
    missing_subfeatures
        .related
        .retain(|related| related.name != "SubFeatures");
    let mut duplicate_subfeatures = valid.clone();
    duplicate_subfeatures.related.push(subfeatures);
    let mut wrongly_typed_subfeatures = valid.clone();
    wrongly_typed_subfeatures
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .expect("SubFeatures collection")
        .class = "PrizMetrik.GdtAnalysis.GdtPlane".into();
    let mut empty_subfeatures = valid.clone();
    empty_subfeatures
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .expect("SubFeatures collection")
        .entity
        .related
        .clear();
    let cad_references = valid
        .related
        .iter()
        .find(|related| related.name == "CadReferences")
        .expect("CadReferences collection")
        .clone();
    let mut duplicate_cad_references = valid.clone();
    duplicate_cad_references.related.push(cad_references);
    let mut duplicate_applied_name = valid.clone();
    let duplicate_applied = duplicate_applied_name
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .and_then(|collection| collection.entity.related.first().cloned())
        .expect("one applied record");
    duplicate_applied_name
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .expect("SubFeatures collection")
        .entity
        .related
        .push(duplicate_applied);
    let mut no_applied_reference = valid.clone();
    no_applied_reference
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .and_then(|collection| collection.entity.related.first_mut())
        .expect("one applied record")
        .entity
        .features
        .references
        .clear();
    let mut two_applied_references = valid.clone();
    two_applied_references
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .and_then(|collection| collection.entity.related.first_mut())
        .expect("one applied record")
        .entity
        .features
        .references
        .push(reference("cylinder", "GdtCylinder"));
    let mut mismatched_class = valid.clone();
    let mismatched_reference = mismatched_class
        .related
        .iter_mut()
        .find(|related| related.name == "SubFeatures")
        .and_then(|collection| collection.entity.related.first_mut())
        .expect("one applied record")
        .entity
        .features
        .references
        .first_mut()
        .expect("one applied feature reference");
    mismatched_reference.class = reference("cylinder", "GdtPlane").class;
    let unknown_child = compound_hole_fixture(&[("unknown", "GdtSphere")], &[], &[]);
    let unresolved_child = compound_hole_fixture(&[("absent", "GdtCylinder")], &[], &[]);

    for compound in [
        missing_subfeatures,
        duplicate_subfeatures,
        wrongly_typed_subfeatures,
        empty_subfeatures,
        duplicate_cad_references,
        duplicate_applied_name,
        no_applied_reference,
        two_applied_references,
        mismatched_class,
        unknown_child,
        unresolved_child,
    ] {
        let features = vec![
            ("hole".into(), compound),
            ("cylinder".into(), cad_feature("GdtCylinder", "schema:504")),
            ("unknown".into(), cad_feature("GdtSphere", "schema:505")),
        ];
        let index = target_features(&features);
        let ctx = cadmpeg_test_support::service_decode_context();
        let position = position_target("hole");
        assert_eq!(
            target_selection(&ctx, &position, &index, None),
            (Some(vec![shape_aspect("hole")]), true)
        );

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        for (id, feature) in &features {
            root.features
                .references
                .push(reference(id, crate::swift::short_class(&feature.class)));
            root.features.entities.push(feature.clone());
        }
        let mut position = position_target("hole");
        position.doubles.insert("Tolerance".into(), 0.1);
        root.annotations
            .references
            .push(reference("A99", "GdtPosition"));
        root.annotations.entities.push(position);
        let projection = project_with_topology(&ctx, &root, None, &[])
            .expect("malformed target ownership stays a projection result");
        let [annotation] = projection.annotations.as_slice() else {
            panic!("position annotation is retained")
        };
        assert!(matches!(
            &annotation.definition,
            PmiDefinition::GeometricTolerance {
                tolerance: cadmpeg_ir::pmi::GeometricToleranceKind::Position,
                ..
            }
        ));
        assert_eq!(annotation.targets, vec![shape_aspect("hole")]);
        assert!(projection.unsupported.is_empty());
        assert_eq!(projection.unresolved_position_targets, 1);
        let mut losses = Vec::new();
        projection.append_losses(&mut losses);
        assert_eq!(losses.len(), 1);
        let loss = losses.first().expect("one unresolved-position loss");
        assert_eq!(
            loss.code.local_code(),
            "pmi.swift-position-target-unresolved"
        );
        assert!(loss
            .message
            .contains("source ShapeAspect targets were retained"));
    }
}

#[test]
fn position_compound_hole_without_applied_cylinder_retains_semantic_pmi() {
    for (child_id, child_class, child_identifier, suffix) in [
        ("plane", "GdtPlane", "schema:800", 800),
        ("cone", "GdtCone", "schema:801", 801),
    ] {
        let compound = compound_hole_fixture(
            &[(child_id, child_class)],
            &[("support", "GdtPlane")],
            &[child_identifier, "schema:802"],
        );
        let features = vec![
            ("hole".into(), compound),
            (child_id.into(), cad_feature(child_class, child_identifier)),
            ("support".into(), cad_feature("GdtPlane", "schema:802")),
        ];
        let ctx = cadmpeg_test_support::service_decode_context();
        let position = position_target("hole");
        let mut topology = TopologyIdentityIndex::default();
        topology
            .entries
            .insert(suffix, vec![face_target(&suffix.to_string())]);
        assert_eq!(
            target_selection(
                &ctx,
                &position,
                &target_features(&features),
                Some(&topology),
            ),
            (Some(vec![shape_aspect("hole")]), true)
        );

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        for (id, feature) in &features {
            root.features
                .references
                .push(reference(id, crate::swift::short_class(&feature.class)));
            root.features.entities.push(feature.clone());
        }
        let mut position = position_target("hole");
        position.doubles.insert("Tolerance".into(), 0.1);
        root.annotations
            .references
            .push(reference("A98", "GdtPosition"));
        root.annotations.entities.push(position);
        let projection = project_with_topology(&ctx, &root, Some(&topology), &[])
            .expect("plane-only or cone-only target selection stays a projection result");
        let [annotation] = projection.annotations.as_slice() else {
            panic!("position annotation is retained")
        };
        assert_eq!(annotation.targets, vec![shape_aspect("hole")]);
        assert!(projection.unsupported.is_empty());
        assert_eq!(projection.unresolved_position_targets, 1);
        let mut losses = Vec::new();
        projection.append_losses(&mut losses);
        assert_eq!(
            losses
                .iter()
                .filter(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" })
                .count(),
            1
        );
    }
}

#[test]
fn compound_hole_applied_and_cad_reference_caps_retain_position_and_record_the_cap() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:900"]);
    let features: Vec<(String, Entity)> = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:900")),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut position = position_target("hole");
    position.doubles.insert("Tolerance".into(), 0.1);

    for work_limit in [6, 7, 8, 9, 10, 11] {
        let feature_index = crate::swift::FeatureIndex::with_work_limit(
            features
                .iter()
                .map(|(id, feature)| (id.as_str(), feature))
                .collect(),
            work_limit,
        );
        assert_eq!(
            target_selection(&ctx, &position, &feature_index, None),
            (Some(vec![shape_aspect("hole")]), true)
        );
        assert_eq!(feature_index.caps().work, 1, "work limit {work_limit}");

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        for (id, feature) in &features {
            root.features
                .references
                .push(reference(id, crate::swift::short_class(&feature.class)));
            root.features.entities.push(feature.clone());
        }
        root.annotations
            .references
            .push(reference("A97", "GdtPosition"));
        root.annotations.entities.push(position.clone());
        let projection_index = crate::swift::FeatureIndex::with_work_limit(
            root.features
                .references
                .iter()
                .zip(&root.features.entities)
                .map(|(reference, feature)| (reference.id.as_str(), feature))
                .collect(),
            work_limit,
        );
        let projection = crate::swift::project_with_feature_index_and_displays(
            &ctx,
            &root,
            None,
            &[],
            &projection_index,
            &[],
            &[],
        )
        .expect("relation cap preserves the position annotation");
        let [annotation] = projection.annotations.as_slice() else {
            panic!("position annotation is retained")
        };
        assert_eq!(annotation.targets, vec![shape_aspect("hole")]);
        assert!(projection.unsupported.is_empty());
        assert_eq!(projection.unresolved_position_targets, 1);
        assert_eq!(projection.walk_caps.work, 1);
        let mut losses = Vec::new();
        projection.append_losses(&mut losses);
        assert_eq!(losses.len(), 2, "work limit {work_limit}");
        assert!(losses
            .iter()
            .any(|loss| { loss.code.local_code() == "pmi.swift-feature-walk-capped" }));
        assert!(losses
            .iter()
            .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
    }
}

#[test]
fn position_counts_caps_for_independent_top_level_references() {
    let compound = compound_hole_fixture(&[("cylinder", "GdtCylinder")], &[], &["schema:900"]);
    let features: Vec<(String, Entity)> = vec![
        ("hole".into(), compound),
        ("cylinder".into(), cad_feature("GdtCylinder", "schema:900")),
    ];
    let feature_index = |work_limit| {
        crate::swift::FeatureIndex::with_work_limit(
            features
                .iter()
                .map(|(id, feature)| (id.as_str(), feature))
                .collect(),
            work_limit,
        )
    };
    let ctx = cadmpeg_test_support::service_decode_context();

    let single_reference_index = feature_index(11);
    let single_reference = position_target("hole");
    assert_eq!(
        target_selection(&ctx, &single_reference, &single_reference_index, None),
        (Some(vec![shape_aspect("hole")]), true)
    );
    assert_eq!(single_reference_index.caps().work, 1);

    let multiple_reference_index = feature_index(11);
    let mut multiple_references = position_target("hole");
    multiple_references
        .features
        .references
        .push(reference("cylinder", "GdtCylinder"));
    assert_eq!(
        target_selection(&ctx, &multiple_references, &multiple_reference_index, None,),
        (None, false)
    );
    assert_eq!(multiple_reference_index.caps().work, 2);
}

#[test]
fn compound_hole_applied_child_scan_cap_retains_position_and_records_the_cap() {
    let applied = [
        ("cylinder-a", "GdtCylinder"),
        ("cylinder-b", "GdtCylinder"),
        ("cylinder-c", "GdtCylinder"),
        ("cylinder-d", "GdtCylinder"),
    ];
    let identifiers = ["schema:920", "schema:921", "schema:922", "schema:923"];
    let compound = compound_hole_fixture(&applied, &[], &identifiers);
    let mut features: Vec<(String, Entity)> = vec![("hole".into(), compound)];
    features.extend(
        applied
            .iter()
            .zip(identifiers)
            .map(|((id, class), identifier)| ((*id).into(), cad_feature(class, identifier))),
    );
    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    let mut position = position_target("hole");
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A93", "GdtPosition"));
    root.annotations.entities.push(position);
    let ctx = cadmpeg_test_support::service_decode_context();
    let feature_index = crate::swift::FeatureIndex::with_work_limit(
        root.features
            .references
            .iter()
            .zip(&root.features.entities)
            .map(|(reference, feature)| (reference.id.as_str(), feature))
            .collect(),
        11,
    );

    let projection = crate::swift::project_with_feature_index_and_displays(
        &ctx,
        &root,
        None,
        &[],
        &feature_index,
        &[],
        &[],
    )
    .expect("applied-child work cap is a projection result");
    let [annotation] = projection.annotations.as_slice() else {
        panic!("position annotation is retained")
    };
    assert_eq!(annotation.targets, vec![shape_aspect("hole")]);
    assert!(projection.unsupported.is_empty());
    assert_eq!(projection.unresolved_position_targets, 1);
    assert_eq!(projection.walk_caps.work, 1);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-feature-walk-capped" }));
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-position-target-unresolved" }));
}

#[test]
fn multi_compound_pattern_global_cap_keeps_existing_unsupported_behavior() {
    let features: Vec<(String, Entity)> = vec![
        (
            "pattern".into(),
            pattern_fixture(&[("hole-a", "GdtCompoundHole"), ("hole-b", "GdtCompoundHole")]),
        ),
        (
            "hole-a".into(),
            compound_hole_fixture(&[("cylinder-a", "GdtCylinder")], &[], &["schema:910"]),
        ),
        (
            "hole-b".into(),
            compound_hole_fixture(&[("cylinder-b", "GdtCylinder")], &[], &["schema:911"]),
        ),
        (
            "cylinder-a".into(),
            cad_feature("GdtCylinder", "schema:910"),
        ),
        (
            "cylinder-b".into(),
            cad_feature("GdtCylinder", "schema:911"),
        ),
    ];
    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    for (id, feature) in &features {
        root.features
            .references
            .push(reference(id, crate::swift::short_class(&feature.class)));
        root.features.entities.push(feature.clone());
    }
    let mut position = position_target("pattern");
    *position
        .features
        .references
        .first_mut()
        .expect("position target reference") = reference("pattern", "GdtPattern");
    position.doubles.insert("Tolerance".into(), 0.1);
    root.annotations
        .references
        .push(reference("A96", "GdtPosition"));
    root.annotations.entities.push(position);
    let ctx = cadmpeg_test_support::service_decode_context();
    let feature_index = crate::swift::FeatureIndex::with_work_limit(
        root.features
            .references
            .iter()
            .zip(&root.features.entities)
            .map(|(reference, feature)| (reference.id.as_str(), feature))
            .collect(),
        15,
    );

    let projection = crate::swift::project_with_feature_index_and_displays(
        &ctx,
        &root,
        None,
        &[],
        &feature_index,
        &[],
        &[],
    )
    .expect("global pattern work cap is a projection result");
    assert!(projection.annotations.is_empty());
    assert_eq!(projection.unsupported.get("GdtPosition"), Some(&1));
    assert_eq!(projection.walk_caps.work, 1);
    assert_eq!(projection.unresolved_position_targets, 0);
    let mut losses = Vec::new();
    projection.append_losses(&mut losses);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses
            .first()
            .expect("single feature-walk cap loss")
            .code
            .local_code(),
        "pmi.swift-feature-walk-capped"
    );
}
