use super::{datum_names, specimen, MM_PER_INCH, VALUE_TOLERANCE};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::pmi::{
    DimensionKind, GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiTarget,
};
use cadmpeg_ir::CadIr;

#[test]
fn ftc_08_internal_flatness_display_depicts_both_semantic_frames() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "pmi.swift-internal-flatness-symbol-assumed" }));
    let ir = result.ir();
    let frames = ["Flatness1", "Flatness2"].map(|name| {
        ir.model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some(name))
            .expect("native flatness annotation")
            .id
            .clone()
    });
    assert!(ir.model.pmi.iter().any(|annotation| {
        matches!(&annotation.definition, PmiDefinition::Presentation { semantics, .. }
            if semantics.len() == frames.len() && frames.iter().all(|id| semantics.contains(id)))
    }));
}

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

fn assert_single_cylinder_target(
    ir: &CadIr,
    annotation: &PmiAnnotation,
    radius_mm: f64,
    axis_point_mm: [f64; 3],
    axis_direction: [f64; 3],
) {
    let [PmiTarget::Face { face }] = annotation.targets.as_slice() else {
        panic!(
            "{} has one face target",
            annotation.name.as_deref().unwrap_or("PMI")
        );
    };
    let face = ir
        .model
        .faces
        .iter()
        .find(|candidate| &candidate.id == face)
        .expect("position target face");
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| candidate.id == face.surface)
        .expect("position target surface");
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
        panic!("position target is a cylinder");
    };
    assert!(
        (cylinder.radius().get() - radius_mm).abs() < VALUE_TOLERANCE,
        "{} cylinder radius",
        annotation.name.as_deref().unwrap_or("PMI")
    );
    let origin = cylinder.origin().get();
    let actual_axis_point_mm = [origin.x, origin.y, origin.z];
    let delta = [
        actual_axis_point_mm[0] - axis_point_mm[0],
        actual_axis_point_mm[1] - axis_point_mm[1],
        actual_axis_point_mm[2] - axis_point_mm[2],
    ];
    let axis_length = axis_direction
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let cross = [
        delta[1] * axis_direction[2] - delta[2] * axis_direction[1],
        delta[2] * axis_direction[0] - delta[0] * axis_direction[2],
        delta[0] * axis_direction[1] - delta[1] * axis_direction[0],
    ];
    let line_distance = cross.iter().map(|value| value * value).sum::<f64>().sqrt() / axis_length;
    assert!(
        line_distance < VALUE_TOLERANCE,
        "{} cylinder axis line",
        annotation.name.as_deref().unwrap_or("PMI")
    );
    let axis = cylinder.frame().axis().as_raw();
    let dot = axis.x * axis_direction[0] + axis.y * axis_direction[1] + axis.z * axis_direction[2];
    assert!(
        (dot.abs() - 1.0).abs() < VALUE_TOLERANCE,
        "{} cylinder axis direction",
        annotation.name.as_deref().unwrap_or("PMI")
    );
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
fn ftc_08_emits_surface_profile16_value_and_face_targets() {
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
fn ftc_08_emits_flatness4_value_and_face_target() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let annotation = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Flatness4"))
        .expect("0.015 flatness");
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude,
        ..
    } = &annotation.definition
    else {
        panic!("flatness definition")
    };
    assert_eq!(tolerance, &GeometricToleranceKind::Flatness);
    assert!((magnitude.get().value.get() - 0.015 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert!(matches!(
        annotation.targets.as_slice(),
        [PmiTarget::Face { .. }]
    ));
}

#[test]
fn ftc_08_row_99_emits_four_candidate_position_pairs_and_shared_targets() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let candidate_pairs = [
        (
            (("Position8", 0.050), ("Position9", 0.020)),
            3.0734,
            [69.85, -25.4, 48.26],
            [0.0, 0.0, 1.0],
        ),
        (
            (("Position10", 0.050), ("Position11", 0.020)),
            2.8956,
            [69.85, -67.31, 48.26],
            [0.0, 0.0, 1.0],
        ),
        (
            (("Position12", 0.050), ("Position13", 0.020)),
            2.8956,
            [69.85, 25.4, 48.26],
            [0.0, 0.0, 1.0],
        ),
        (
            (("Position14", 0.050), ("Position15", 0.020)),
            3.0734,
            [69.85, 67.31, 48.26],
            [0.0, 0.0, 1.0],
        ),
    ];
    let mut pair_targets = Vec::with_capacity(candidate_pairs.len());
    for (
        ((first_name, first_magnitude), (second_name, second_magnitude)),
        radius_mm,
        axis_point_mm,
        axis_direction,
    ) in candidate_pairs
    {
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
            "{first_name}/{second_name} pair target equality"
        );
        assert_eq!(
            first.targets.len(),
            1,
            "{first_name}/{second_name} cylinder target"
        );
        assert_single_cylinder_target(ir, first, radius_mm, axis_point_mm, axis_direction);
        assert_single_cylinder_target(ir, second, radius_mm, axis_point_mm, axis_direction);
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
    assert_eq!(shared_targets, 0);
}

#[test]
fn ftc_08_row_99_groups_each_position_pair_under_its_simultaneous_requirement() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let mut groups = super::depicted_names(ir)
        .into_iter()
        .filter_map(|(text, names)| Some((text?.trim().to_owned(), names)))
        .filter(|(text, _)| text.starts_with("SIM REQT"))
        .collect::<Vec<_>>();
    groups.sort();
    assert_eq!(
        groups,
        [
            ("SIM REQT 1", ["Position10", "Position11"]),
            ("SIM REQT 1", ["Position8", "Position9"]),
            ("SIM REQT 2", ["Position12", "Position13"]),
            ("SIM REQT 2", ["Position14", "Position15"]),
        ]
        .map(|(text, names)| (text.to_owned(), names.map(str::to_owned).to_vec()))
    );
}

#[test]
fn ftc_08_row_77_face_attached_display_depicts_the_profile_to_d_and_h_j() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    let depicts_row_77 = |semantics: &[cadmpeg_ir::ids::PmiId]| {
        let [profile] = semantics else {
            return false;
        };
        let Some(annotation) = ir
            .model
            .pmi
            .iter()
            .find(|annotation| &annotation.id == profile)
        else {
            return false;
        };
        let PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude,
            datum_system: Some(system),
            ..
        } = &annotation.definition
        else {
            return false;
        };
        (magnitude.get().value.get() - 0.015 * MM_PER_INCH).abs() < VALUE_TOLERANCE
            && datum_names(ir, system) == ["D", "H", "J"]
    };
    let displays = ir
        .model
        .pmi
        .iter()
        .filter(|annotation| {
            matches!(
                &annotation.definition,
                PmiDefinition::Presentation { semantics, .. } if depicts_row_77(semantics)
            )
        })
        .count();
    assert_eq!(displays, 1);
}
