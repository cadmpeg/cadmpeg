use super::{datum_names, specimen, MM_PER_INCH, VALUE_TOLERANCE};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::pmi::{
    DimensionKind, GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiTarget,
};
use cadmpeg_ir::CadIr;

#[test]
fn ftc_08_row_54_emits_the_size_and_opposed_face_pair() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Width4"))
        .expect("Width4 annotation");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("Width4 dimension")
    };
    assert_eq!(dimension.kind(), &DimensionKind::Size);
    assert!(
        (dimension.nominal().expect("nominal").value.get() - 1.1 * MM_PER_INCH).abs()
            < VALUE_TOLERANCE
    );

    let faces = annotation
        .targets
        .iter()
        .filter_map(|target| match target {
            PmiTarget::Face { face } => ir.model.faces.iter().find(|item| &item.id == face),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(annotation.targets.len(), 2);
    assert_eq!(faces.len(), 2);
    let planes = faces
        .iter()
        .map(|face| {
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .expect("target face surface");
            let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
                panic!("target face plane")
            };
            plane
        })
        .collect::<Vec<_>>();
    let [first, second] = planes.as_slice() else {
        panic!("opposing dimension planes")
    };
    let first_normal = first.frame().axis().as_raw();
    let second_normal = second.frame().axis().as_raw();
    let normal_dot = first_normal.x * second_normal.x
        + first_normal.y * second_normal.y
        + first_normal.z * second_normal.z;
    assert!(normal_dot < -1.0 + VALUE_TOLERANCE);
    let first_origin = first.origin().get();
    let second_origin = second.origin().get();
    let separation = ((second_origin.x - first_origin.x) * first_normal.x
        + (second_origin.y - first_origin.y) * first_normal.y
        + (second_origin.z - first_origin.z) * first_normal.z)
        .abs();
    assert!((separation - 1.1 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
}

fn named_tolerance<'a>(
    ir: &'a CadIr,
    name: &str,
    magnitude: f64,
    datums: &[&str],
    modifiers: &[&str],
) -> &'a PmiAnnotation {
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name} PMI"));
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude: actual_magnitude,
        datum_system,
        modifiers: actual_modifiers,
        ..
    } = &annotation.definition
    else {
        panic!("{name} geometric tolerance")
    };
    assert_eq!(tolerance, &GeometricToleranceKind::Position);
    assert!(
        (actual_magnitude.get().value.get() - magnitude * MM_PER_INCH).abs() < VALUE_TOLERANCE,
        "{name} magnitude"
    );
    assert_eq!(
        datum_system
            .as_ref()
            .map(|system| datum_names(ir, system))
            .unwrap_or_default(),
        datums,
        "{name} datum references"
    );
    let actual_modifiers = actual_modifiers
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(actual_modifiers, modifiers.to_vec(), "{name} modifiers");
    annotation
}

#[test]
fn ftc_08_row_66_and_92_preserve_the_native_parallelism_modifier_subset() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let free_state = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Parallelism10"))
        .expect("0.030 parallelism");
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude,
        datum_system,
        modifiers,
        ..
    } = &free_state.definition
    else {
        panic!("0.030 parallelism definition")
    };
    assert_eq!(tolerance, &GeometricToleranceKind::Parallelism);
    assert!((magnitude.get().value.get() - 0.030 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert_eq!(
        datum_system
            .as_ref()
            .map(|system| datum_names(ir, system))
            .unwrap_or_default(),
        vec![String::from("A")]
    );
    assert_eq!(
        modifiers.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["free_state"]
    );
    assert!(free_state
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Face { .. })));

    let tangent_plane = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Parallelism9"))
        .expect("0.015 tangent-plane parallelism");
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude,
        datum_system,
        modifiers,
        ..
    } = &tangent_plane.definition
    else {
        panic!("0.015 tangent-plane parallelism definition")
    };
    assert_eq!(tolerance, &GeometricToleranceKind::Parallelism);
    assert!((magnitude.get().value.get() - 0.015 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert_eq!(
        datum_system
            .as_ref()
            .map(|system| datum_names(ir, system))
            .unwrap_or_default(),
        vec![String::from("A")]
    );
    assert_eq!(
        modifiers.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["tangent_plane"]
    );
    assert_eq!(free_state.targets, tangent_plane.targets);
}

#[test]
fn ftc_08_row_94_emits_the_profile_value_and_face_target_subset() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Surface Profile16"))
        .expect("0.015 surface profile");
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude,
        ..
    } = &annotation.definition
    else {
        panic!("surface profile definition")
    };
    assert_eq!(tolerance, &GeometricToleranceKind::SurfaceProfile);
    assert!((magnitude.get().value.get() - 0.015 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert_eq!(annotation.targets.len(), 10);
    assert!(annotation
        .targets
        .iter()
        .all(|target| matches!(target, PmiTarget::Face { .. })));
}

#[test]
fn ftc_08_row_99_emits_four_candidate_position_pairs_and_shared_targets() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let candidate_pairs = [
        (("Position8", 0.050), ("Position9", 0.020)),
        (("Position10", 0.050), ("Position11", 0.020)),
        (("Position12", 0.050), ("Position13", 0.020)),
        (("Position14", 0.050), ("Position15", 0.020)),
    ];
    let mut pair_targets = Vec::with_capacity(candidate_pairs.len());
    for ((first_name, first_magnitude), (second_name, second_magnitude)) in candidate_pairs {
        let first = named_tolerance(
            ir,
            first_name,
            first_magnitude,
            &["D", "B", "C"],
            &["maximum_material_requirement"],
        );
        let second = named_tolerance(
            ir,
            second_name,
            second_magnitude,
            &["D", "B", "C"],
            &["maximum_material_requirement"],
        );
        assert_eq!(
            first.targets, second.targets,
            "{first_name}/{second_name} targets"
        );
        assert_eq!(
            first.targets.len(),
            2,
            "{first_name}/{second_name} face pair"
        );
        assert!(first
            .targets
            .iter()
            .all(|target| matches!(target, PmiTarget::Face { .. })));
        pair_targets.push(first.targets.clone());
    }

    for left in 0..pair_targets.len() {
        for right in left + 1..pair_targets.len() {
            assert_ne!(pair_targets[left], pair_targets[right]);
        }
    }
    let shared_targets = pair_targets[0]
        .iter()
        .filter(|target| pair_targets[1..].iter().all(|pair| pair.contains(*target)))
        .count();
    assert_eq!(shared_targets, 1);
}
