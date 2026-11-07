use super::{entity, reference};
use crate::records::{Feature, FeatureHistory};
use crate::swift::{
    approximately_equal, display_value_normalizes_to, project_with_spherical_diameter_displays,
    rendered_projections, spherical_diameter_display_literals, Entity, PrimaryTopology,
    SphericalDiameterDisplay, TopologyIdentityIndex,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiTarget};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};
use std::collections::BTreeMap;

const MM_PER_INCH: f64 = 25.4;

fn sphere(radius: f64, cad_sequence: u32) -> Entity {
    let mut sphere = entity("GdtSphere");
    let mut geometry = Entity {
        class: "PrizMetrik.Geometry.GeoSphere".into(),
        ..Entity::default()
    };
    geometry.doubles.insert("R".into(), radius);
    sphere.related.push(crate::swift::RelatedObject {
        name: "NomSphere".into(),
        class: geometry.class.clone(),
        entity: geometry,
    });
    let mut references = entity("CadRefCollection");
    let mut cad_ref = entity("CadRef");
    cad_ref
        .strings
        .insert("CadIdentifier".into(), format!("125:{cad_sequence}"));
    references.related.push(crate::swift::RelatedObject {
        name: "CadRef0".into(),
        class: cad_ref.class.clone(),
        entity: cad_ref,
    });
    sphere.related.push(crate::swift::RelatedObject {
        name: "CadReferences".into(),
        class: references.class.clone(),
        entity: references,
    });
    sphere
}

fn diameter() -> Entity {
    let mut dimension = entity("GdtDiameter");
    dimension
        .strings
        .insert("ObjectName".into(), "Diameter1".into());
    dimension.doubles.insert("Nominal".into(), 31.75);
    dimension
        .features
        .references
        .push(reference("F1", "GdtSphere"));
    dimension
}

fn history_with_display(display: &str) -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "Annotations".into(),
            kind: "Annotations".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new("TXD10").unwrap(),
                display.into(),
            )]),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }
}

fn fixture() -> (Entity, TopologyIdentityIndex, FeatureHistory) {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.features.references = vec![reference("F1", "GdtSphere"), reference("F2", "GdtSphere")];
    root.features.entities = vec![sphere(15.875, 1), sphere(15.875, 2)];
    root.annotations.references = vec![reference("A1", "GdtDiameter")];
    root.annotations.entities = vec![diameter()];
    let faces = vec![face(11), face(12)];
    let topology = TopologyIdentityIndex::from_model(
        PrimaryTopology {
            bodies: &[],
            faces: &faces,
            edges: &[],
            vertices: &[],
        },
        &[(1, 11), (2, 12)],
        &[],
        &[],
    );
    (root, topology, history_with_display("2X <MOD-SPHDIA>1.250"))
}

fn face(attribute: u32) -> Face {
    Face {
        id: FaceId::mint(format!("sldprt:brep:face#{attribute}")).unwrap(),
        shell: ShellId::mint(format!("sldprt:brep:shell#{attribute}")).unwrap(),
        surface: SurfaceId::mint(format!("sldprt:brep:surface#{attribute}")).unwrap(),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    }
}

fn raw_counted_display() -> SphericalDiameterDisplay {
    let displays = spherical_diameter_display_literals("2X <MOD-SPHDIA>");
    let [display] = displays.as_slice() else {
        panic!("one raw counted spherical-diameter display")
    };
    *display
}

fn project(
    root: &Entity,
    topology: &TopologyIdentityIndex,
    history: &FeatureHistory,
    displays: &[SphericalDiameterDisplay],
) -> crate::swift::AnnotationProjection {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty test root fits policy");
    project_with_spherical_diameter_displays(
        &ctx,
        root,
        Some(topology),
        &[],
        displays,
        std::slice::from_ref(history),
    )
    .expect("synthetic sphere projection fits policy")
}

#[test]
fn counted_display_value_matches_one_unrounded_length_unit() {
    let inch_displays = spherical_diameter_display_literals("2X <MOD-SPHDIA>1.250");
    let [inch_display] = inch_displays.as_slice() else {
        panic!("one spherical-diameter display")
    };
    assert_eq!(inch_display.multiplicity, Some(2));
    assert_eq!(inch_display.decimal_places, Some(3));
    let value = inch_display.value.expect("displayed value");
    let semantic = cadmpeg_ir::scalar::PositiveReal::new(1.25 * MM_PER_INCH).unwrap();
    assert!(display_value_normalizes_to(
        value,
        inch_display.decimal_places.unwrap(),
        semantic
    ));
    assert!(!display_value_normalizes_to(
        cadmpeg_ir::scalar::PositiveReal::new(1.3).unwrap(),
        1,
        semantic,
    ));

    let metric_displays = spherical_diameter_display_literals("2X &lt;MOD-SPHDIA&gt;31.75");
    let [metric_display] = metric_displays.as_slice() else {
        panic!("one escaped spherical-diameter display")
    };
    assert_eq!(metric_display.decimal_places, Some(2));
    assert!(display_value_normalizes_to(
        metric_display.value.unwrap(),
        metric_display.decimal_places.unwrap(),
        semantic,
    ));
}

#[test]
fn raw_utf16_display_preserves_its_count_and_missing_nominal() {
    let text = "2X <MOD-SPHDIA>";
    let units = u8::try_from(text.encode_utf16().count()).expect("fixture fits SWIFT text length");
    let mut payload = vec![0xff, 0xfe, 0xff, units];
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    let displays = rendered_projections(&payload).1;
    let [display] = displays.as_slice() else {
        panic!("one rendered spherical-diameter display")
    };
    assert_eq!(display.multiplicity, Some(2));
    assert_eq!(display.value, None);
    assert_eq!(display.decimal_places, None);
}

#[test]
fn unique_counted_diameter_adds_the_second_sphere_targets() {
    let (root, topology, history) = fixture();
    let result = project(&root, &topology, &history, &[raw_counted_display()]);

    assert_eq!(result.inferred_spherical_diameter_targets, 1);
    assert_eq!(result.unresolved_spherical_diameter_targets, 0);
    let annotation = result
        .annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter1"))
        .expect("projected spherical diameter");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("spherical diameter dimension")
    };
    assert_eq!(dimension.kind(), &DimensionKind::SphericalDiameter);
    assert!(approximately_equal(
        dimension.nominal().unwrap().value.get(),
        1.25 * MM_PER_INCH
    ));
    let face_targets = annotation
        .targets
        .iter()
        .filter_map(|target| match target {
            PmiTarget::Face { face } => Some(face.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        face_targets,
        std::collections::BTreeSet::from(["sldprt:brep:face#11", "sldprt:brep:face#12"])
    );
}

#[test]
fn zero_nominal_uses_the_existing_implicit_sphere_diameter() {
    let (mut root, topology, history) = fixture();
    root.annotations
        .entities
        .first_mut()
        .expect("one diameter annotation")
        .doubles
        .insert("Nominal".into(), 0.0);
    let result = project(&root, &topology, &history, &[raw_counted_display()]);

    assert_eq!(result.inferred_spherical_diameter_targets, 1);
    assert_eq!(result.unresolved_spherical_diameter_targets, 0);
    let annotation = result
        .annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter1"))
        .expect("projected spherical diameter");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("spherical diameter dimension")
    };
    assert!(approximately_equal(
        dimension.nominal().unwrap().value.get(),
        1.25 * MM_PER_INCH
    ));
    assert_eq!(
        annotation
            .targets
            .iter()
            .filter(|target| matches!(target, PmiTarget::Face { .. }))
            .count(),
        2
    );
}

#[test]
fn ambiguous_count_dimension_or_sphere_sets_keep_the_gap_open() {
    let (root, topology, history) = fixture();
    let display = raw_counted_display();

    let duplicate_displays = project(&root, &topology, &history, &[display, display]);
    assert_eq!(duplicate_displays.inferred_spherical_diameter_targets, 0);
    assert_eq!(duplicate_displays.unresolved_spherical_diameter_targets, 2);
    let mut unresolved_losses = Vec::new();
    duplicate_displays.append_losses(&mut unresolved_losses);
    assert!(unresolved_losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::PmiSwiftSphericalDiameterTargetUnresolved.kind()
    }));

    let mut misaligned_annotations = root.clone();
    misaligned_annotations.annotations.references.pop();
    let misaligned_annotations = project(&misaligned_annotations, &topology, &history, &[display]);
    assert!(misaligned_annotations.annotations.is_empty());
    assert_eq!(
        misaligned_annotations.unresolved_spherical_diameter_targets,
        1
    );
    let mut unresolved_losses = Vec::new();
    misaligned_annotations.append_losses(&mut unresolved_losses);
    assert!(unresolved_losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::PmiSwiftSphericalDiameterTargetUnresolved.kind()
    }));

    let raw_nominal_conflict = SphericalDiameterDisplay {
        multiplicity: Some(2),
        value: Some(cadmpeg_ir::scalar::PositiveReal::new(99.0).unwrap()),
        decimal_places: Some(1),
    };
    let raw_nominal_conflict = project(&root, &topology, &history, &[raw_nominal_conflict]);
    assert_eq!(raw_nominal_conflict.inferred_spherical_diameter_targets, 0);
    assert_eq!(
        raw_nominal_conflict.unresolved_spherical_diameter_targets,
        1
    );

    let mut incompatible_nominal = root.clone();
    incompatible_nominal
        .annotations
        .entities
        .first_mut()
        .expect("one diameter annotation")
        .doubles
        .insert("Nominal".into(), 30.0);
    let incompatible_nominal = project(&incompatible_nominal, &topology, &history, &[display]);
    assert_eq!(incompatible_nominal.inferred_spherical_diameter_targets, 0);
    assert_eq!(
        incompatible_nominal.unresolved_spherical_diameter_targets,
        1
    );

    let mut duplicate_dimensions = root.clone();
    duplicate_dimensions
        .annotations
        .references
        .push(reference("A2", "GdtDiameter"));
    duplicate_dimensions.annotations.entities.push(diameter());
    let duplicate_dimensions = project(&duplicate_dimensions, &topology, &history, &[display]);
    assert_eq!(duplicate_dimensions.inferred_spherical_diameter_targets, 0);
    assert_eq!(
        duplicate_dimensions.unresolved_spherical_diameter_targets,
        1
    );

    let mut extra_direct_dimension = diameter();
    *extra_direct_dimension
        .features
        .references
        .first_mut()
        .expect("one direct sphere reference") = reference("F2", "GdtSphere");
    extra_direct_dimension.doubles.remove("Nominal");
    let mut extra_direct_dimension_root = root.clone();
    extra_direct_dimension_root
        .annotations
        .references
        .push(reference("A2", "GdtDiameter"));
    extra_direct_dimension_root
        .annotations
        .entities
        .push(extra_direct_dimension);
    let extra_direct_dimension = project(
        &extra_direct_dimension_root,
        &topology,
        &history,
        &[display],
    );
    assert_eq!(
        extra_direct_dimension.inferred_spherical_diameter_targets,
        0
    );
    assert_eq!(
        extra_direct_dimension.unresolved_spherical_diameter_targets,
        1
    );

    let mut mixed_competing_dimension = diameter();
    mixed_competing_dimension.features.references =
        vec![reference("F2", "GdtSphere"), reference("F3", "GdtCylinder")];
    let mut mixed_competing_root = root.clone();
    mixed_competing_root
        .features
        .references
        .push(reference("F3", "GdtCylinder"));
    mixed_competing_root
        .features
        .entities
        .push(entity("GdtCylinder"));
    mixed_competing_root
        .annotations
        .references
        .push(reference("A2", "GdtDiameter"));
    mixed_competing_root
        .annotations
        .entities
        .push(mixed_competing_dimension);
    let mixed_competing_dimension = project(&mixed_competing_root, &topology, &history, &[display]);
    assert_eq!(
        mixed_competing_dimension.inferred_spherical_diameter_targets,
        0
    );
    assert_eq!(
        mixed_competing_dimension.unresolved_spherical_diameter_targets,
        1
    );

    let mut third_sphere = root.clone();
    third_sphere
        .features
        .references
        .push(reference("F3", "GdtSphere"));
    third_sphere.features.entities.push(sphere(15.875, 3));
    let third_sphere = project(&third_sphere, &topology, &history, &[display]);
    assert_eq!(third_sphere.inferred_spherical_diameter_targets, 0);
    assert_eq!(third_sphere.unresolved_spherical_diameter_targets, 1);

    let mut duplicate_feature_id = root.clone();
    duplicate_feature_id
        .features
        .references
        .push(reference("F1", "GdtSphere"));
    duplicate_feature_id
        .features
        .entities
        .push(sphere(15.875, 3));
    let duplicate_feature_id = project(&duplicate_feature_id, &topology, &history, &[display]);
    assert_eq!(duplicate_feature_id.inferred_spherical_diameter_targets, 0);
    assert_eq!(
        duplicate_feature_id.unresolved_spherical_diameter_targets,
        1
    );

    let mut mismatched_radius = root.clone();
    mismatched_radius
        .features
        .entities
        .get_mut(1)
        .expect("peer sphere feature")
        .related
        .iter_mut()
        .find(|related| related.name == "NomSphere")
        .expect("peer nominal sphere")
        .entity
        .doubles
        .insert("R".into(), 15.0);
    let mismatched_radius = project(&mismatched_radius, &topology, &history, &[display]);
    assert_eq!(mismatched_radius.inferred_spherical_diameter_targets, 0);
    assert_eq!(mismatched_radius.unresolved_spherical_diameter_targets, 1);

    let rounded_history = history_with_display("2X <MOD-SPHDIA>1.3");
    let rounded_only = project(&root, &topology, &rounded_history, &[display]);
    assert_eq!(rounded_only.inferred_spherical_diameter_targets, 0);
    assert_eq!(rounded_only.unresolved_spherical_diameter_targets, 1);

    let wrong_count = history_with_display("3X <MOD-SPHDIA>1.250");
    let wrong_count = project(&root, &topology, &wrong_count, &[display]);
    assert_eq!(wrong_count.inferred_spherical_diameter_targets, 0);
    assert_eq!(wrong_count.unresolved_spherical_diameter_targets, 1);

    let three_x_display = spherical_diameter_display_literals("3X <MOD-SPHDIA>");
    let [three_x_display] = three_x_display.as_slice() else {
        panic!("one three-count raw display")
    };
    let three_x_history = history_with_display("3X <MOD-SPHDIA>1.250");
    let three_x = project(&root, &topology, &three_x_history, &[*three_x_display]);
    assert_eq!(three_x.inferred_spherical_diameter_targets, 0);
    assert_eq!(three_x.unresolved_spherical_diameter_targets, 1);

    let mut suppressed_peer = root.clone();
    suppressed_peer
        .features
        .entities
        .get_mut(1)
        .expect("peer sphere feature")
        .integers
        .insert("IsSuppressed".into(), 1);
    let suppressed_peer = project(&suppressed_peer, &topology, &history, &[display]);
    assert_eq!(suppressed_peer.inferred_spherical_diameter_targets, 0);
    assert_eq!(suppressed_peer.unresolved_spherical_diameter_targets, 1);

    let mut suppressed_history = history.clone();
    suppressed_history
        .features
        .first_mut()
        .expect("history display feature")
        .suppressed = true;
    let suppressed_history = project(&root, &topology, &suppressed_history, &[display]);
    assert_eq!(suppressed_history.inferred_spherical_diameter_targets, 0);
    assert_eq!(suppressed_history.unresolved_spherical_diameter_targets, 1);

    let mut unknown_peer_target = root.clone();
    let peer_references = unknown_peer_target
        .features
        .entities
        .get_mut(1)
        .expect("peer sphere feature")
        .related
        .iter_mut()
        .find(|related| related.name == "CadReferences")
        .expect("peer CAD references");
    peer_references
        .entity
        .related
        .first_mut()
        .expect("peer CAD reference")
        .entity
        .strings
        .insert("CadIdentifier".into(), "125:99".into());
    let unknown_peer_target = project(&unknown_peer_target, &topology, &history, &[display]);
    assert_eq!(unknown_peer_target.inferred_spherical_diameter_targets, 0);
    assert_eq!(unknown_peer_target.unresolved_spherical_diameter_targets, 1);

    let mut overlapping_peer_target = root.clone();
    let peer_references = overlapping_peer_target
        .features
        .entities
        .get_mut(1)
        .expect("peer sphere feature")
        .related
        .iter_mut()
        .find(|related| related.name == "CadReferences")
        .expect("peer CAD references");
    peer_references
        .entity
        .related
        .first_mut()
        .expect("peer CAD reference")
        .entity
        .strings
        .insert("CadIdentifier".into(), "125:1".into());
    let overlapping_peer_target =
        project(&overlapping_peer_target, &topology, &history, &[display]);
    assert_eq!(
        overlapping_peer_target.inferred_spherical_diameter_targets,
        0
    );
    assert_eq!(
        overlapping_peer_target.unresolved_spherical_diameter_targets,
        1
    );

    let mut mixed_owner = root.clone();
    mixed_owner
        .annotations
        .entities
        .first_mut()
        .expect("one diameter annotation")
        .features
        .references
        .push(reference("F2", "GdtSphere"));
    let mixed_owner = project(&mixed_owner, &topology, &history, &[display]);
    assert_eq!(mixed_owner.inferred_spherical_diameter_targets, 0);
    assert_eq!(mixed_owner.unresolved_spherical_diameter_targets, 1);

    let mut duplicate_history = history.clone();
    let duplicate_feature = duplicate_history
        .features
        .first()
        .expect("history display feature")
        .clone();
    duplicate_history.features.push(duplicate_feature);
    let duplicate_history = project(&root, &topology, &duplicate_history, &[display]);
    assert_eq!(duplicate_history.inferred_spherical_diameter_targets, 0);
    assert_eq!(duplicate_history.unresolved_spherical_diameter_targets, 1);
}
