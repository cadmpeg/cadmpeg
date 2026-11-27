// SPDX-License-Identifier: Apache-2.0
//! Extent-7 targets whose surface record has form 29 or 73.

use super::*;

fn parametric_surface(ordinal: u32, form: u32) -> PmDcFeatureProperty {
    parametric_surface_at(ordinal, form, 19, None)
}

fn parametric_surface_at(
    ordinal: u32,
    form: u32,
    version: u8,
    extension: Option<u32>,
) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: version,
            header: test_header(),
            kind: PmDcFeaturePropertyKind::ParametricSurface {
                references: [reference(0); 3],
                extension,
                state: 1,
                surface_form: form,
                value: 1,
            },
        },
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::SURFACE_TYPE),
        segment(),
        ordinal,
    )
}

fn changed_surface(case: usize) -> PmDcFeatureProperty {
    let mut surface = parametric_surface(21, 73);
    surface.edit(|payload| {
        let PmDcFeaturePropertyKind::ParametricSurface {
            references,
            extension,
            state,
            value,
            ..
        } = &mut payload.kind
        else {
            panic!("parametric surface fixture");
        };
        match case {
            0..=2 => references[case] = crate::pmdc::PmDcReference::from_packed(1),
            3 => *state = 0,
            4 => *value = 0,
            5 => *extension = Some(0),
            _ => panic!("control case"),
        }
    });
    surface
}

#[test]
fn a_parametric_face_target_is_a_native_face_termination() {
    for form in [29, 73] {
        let feature = extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                parametric_surface(21, form),
            ],
        )
        .expect("parametric face target");
        let (extent, start) = extrude_definition(&feature);
        assert!(matches!(
            start,
            cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {}
        ));
        let ExtrudeExtent::OneSided { side } = extent else {
            panic!("one-sided extrusion");
        };
        let LinearTermination::ToFace {
            face: cadmpeg_ir::features::selections::FaceSelection::Native(native),
            offset: None,
        } = &side.termination
        else {
            panic!("native face termination");
        };
        assert_eq!(*native, parametric_surface(21, form).id());
        let definition = feature.evaluation.definition();
        assert_eq!(
            crate::feature::extrusion::native_selected_face_targets(definition),
            1
        );
        assert_eq!(
            crate::feature::extrusion::native_face_targets(definition),
            0
        );
    }
}

#[test]
fn parametric_face_controls_keep_other_forms_native() {
    for case in 0..=5 {
        let feature = extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                changed_surface(case),
            ],
        );
        assert!(feature.is_none(), "control case {case}");
    }
}

#[test]
fn the_extension_control_depends_on_the_segment_major_version() {
    const WITH_EXTENSION: u8 = 22;
    const NONZERO_EXTENSION: u32 = 7;
    for (version, extension, projects) in [
        (WITH_EXTENSION, Some(0), true),
        (WITH_EXTENSION + 3, Some(0), true),
        (WITH_EXTENSION, None, false),
        (WITH_EXTENSION, Some(NONZERO_EXTENSION), false),
        (WITH_EXTENSION + 3, None, false),
        (WITH_EXTENSION + 3, Some(NONZERO_EXTENSION), false),
        (WITH_EXTENSION - 1, Some(0), false),
        (WITH_EXTENSION - 1, None, true),
    ] {
        let feature = extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                parametric_surface_at(21, 73, version, extension),
            ],
        );
        assert_eq!(
            feature.is_some(),
            projects,
            "version {version}, extension {extension:?}"
        );
    }
}

#[test]
fn a_target_face_projection_pushes_one_note_per_feature() {
    for (extra, wording) in [
        (
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                parametric_surface(21, 73),
            ],
            "Extrusion target retains its parametric surface record as a native face selection; the stored parameters do not identify the body face.",
        ),
        (
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface(21),
            ],
            "Extrusion target retains its stored planar face geometry; its body-face identity remains unresolved.",
        ),
    ] {
        configured_extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            extra,
            |_| {},
            |source, label, index, projected| {
                assert!(projected.is_some());
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                        .expect("projection context");
                let mut losses = Vec::new();
                assert!(
                    crate::feature::extrusion::project(&ctx, source, label, index, &mut losses)
                        .is_some()
                );
                let notes = losses
                    .iter()
                    .filter(|note| {
                        note.code.local_code() == "feature.extrusion-target-face-native"
                    })
                    .collect::<Vec<_>>();
                assert_eq!(notes.len(), 1);
                assert_eq!(notes[0].message, wording);
            },
        );
    }
}

#[test]
fn a_parametric_face_start_stays_native() {
    let feature = extent_extrusion(
        6,
        false,
        &[(10, 20), (17, 21), (13, 22), (18, 23)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            parametric_surface(21, 73),
            test_property(22, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            parametric_surface(23, 73),
        ],
    );
    assert!(feature.is_none());
}

#[test]
fn parses_parametric_surfaces_by_form_and_parameter_length() {
    for (form, bytes) in [(29u32, 116usize), (73, 156)] {
        for (version, stamps, extended) in [
            (19, &[][..], false),
            (23, &[-1][..], true),
            (25, &[-1, -1][..], true),
        ] {
            let build = |length: usize| {
                let mut surface =
                    crate::test_support::test_fixtures::content_at(11, version, stamps);
                surface.extend_from_slice(&[0; 12]);
                if extended {
                    surface.extend_from_slice(&0u32.to_le_bytes());
                }
                surface.push(1);
                surface.extend_from_slice(&form.to_le_bytes());
                surface.extend_from_slice(&1u32.to_le_bytes());
                surface.extend_from_slice(&vec![0u8; length]);
                surface
            };
            let parsed = parse(&build(bytes), |_, source| {
                crate::feature::parse_surface(source, version).expect("surface")
            })
            .expect("parametric surface");
            let PmDcFeaturePropertyKind::ParametricSurface {
                extension,
                state,
                surface_form,
                value,
                ..
            } = parsed.kind
            else {
                panic!("parametric surface kind");
            };
            assert_eq!(extension, extended.then_some(0));
            assert_eq!((state, surface_form, value), (1, form, 1));
            for length in [bytes - 4, bytes + 4] {
                assert!(parse(&build(length), |_, source| {
                    crate::feature::parse_surface(source, version)
                })
                .is_err());
            }
        }
    }
}
