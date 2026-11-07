use super::{entity, reference};
use crate::swift::{
    project_with_topology, rendered_projections, spherical_diameter_display_literals, Entity,
    PrimaryTopology, TopologyIdentityIndex,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiTarget};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};
use std::collections::BTreeSet;

const DIMENSION_VALUE_TOLERANCE: f64 = 1.0e-12;

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

fn fixture() -> (Entity, TopologyIdentityIndex) {
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
    (root, topology)
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

fn project(
    root: &Entity,
    topology: &TopologyIdentityIndex,
    displays: &[crate::swift::SphericalDiameterDisplay],
) -> crate::swift::AnnotationProjection {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty test root fits policy");
    project_with_topology(&ctx, root, Some(topology), &[], displays)
        .expect("synthetic sphere projection fits policy")
}

#[test]
fn counted_display_scan_preserves_multiplicity_and_missing_nominal() {
    let displays = spherical_diameter_display_literals("2X <MOD-SPHDIA>");
    let [display] = displays.as_slice() else {
        panic!("one counted spherical-diameter display")
    };
    assert_eq!(display.multiplicity, Some(2));

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
}

#[test]
fn counted_display_keeps_direct_sphere_owner_and_reports_unresolved_peers() {
    let (root, topology) = fixture();
    let displays = spherical_diameter_display_literals("2X <MOD-SPHDIA>");
    let result = project(&root, &topology, &displays);

    assert_eq!(result.unresolved_spherical_diameter_targets, 1);
    let annotation = result
        .annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter1"))
        .expect("projected spherical diameter");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("spherical diameter dimension")
    };
    assert_eq!(dimension.kind(), &DimensionKind::SphericalDiameter);
    assert!((dimension.nominal().unwrap().value.get() - 31.75).abs() < DIMENSION_VALUE_TOLERANCE);
    assert_eq!(annotation.targets.len(), 1);
    assert!(matches!(
        annotation.targets.first(),
        Some(PmiTarget::Face { face }) if face.as_str() == "sldprt:brep:face#11"
    ));

    let mut losses = Vec::new();
    result.append_losses(&mut losses);
    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::PmiSwiftSphericalDiameterTargetUnresolved.kind()
    }));
}

#[test]
fn explicit_counted_sphere_owners_are_kept_with_unresolved_display_association() {
    let (mut root, topology) = fixture();
    root.annotations
        .entities
        .first_mut()
        .expect("one diameter annotation")
        .features
        .references = vec![reference("F1", "GdtSphere"), reference("F2", "GdtSphere")];
    let displays = spherical_diameter_display_literals("2X <MOD-SPHDIA>");
    let result = project(&root, &topology, &displays);

    assert_eq!(result.unresolved_spherical_diameter_targets, 1);
    let annotation = result
        .annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter1"))
        .expect("projected spherical diameter");
    let target_faces = annotation
        .targets
        .iter()
        .filter_map(|target| match target {
            PmiTarget::Face { face } => Some(face.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        target_faces,
        BTreeSet::from(["sldprt:brep:face#11", "sldprt:brep:face#12"])
    );

    let mut losses = Vec::new();
    result.append_losses(&mut losses);
    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::PmiSwiftSphericalDiameterTargetUnresolved.kind()
    }));
}

#[test]
fn single_spherical_diameter_display_does_not_report_unresolved_peers() {
    let (root, topology) = fixture();
    let displays = spherical_diameter_display_literals("1X <MOD-SPHDIA>");
    let result = project(&root, &topology, &displays);
    assert_eq!(result.unresolved_spherical_diameter_targets, 0);
    assert_eq!(
        result
            .annotations
            .first()
            .expect("one dimension annotation")
            .targets
            .len(),
        1
    );
}
