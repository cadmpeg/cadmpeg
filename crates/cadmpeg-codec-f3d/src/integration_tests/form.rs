use cadmpeg_ir::features::form::{
    FormCageBinding, FormNativeReferenceTarget, FormResourceAvailability,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn form_cage_program_failures_retain_the_active_resource_and_native_parser_loss() {
    for (valid, program) in [
        (true, crate::test_support::zip_test::generated_form_tsm()),
        (false, b"invalid synthetic cage program".to_vec()),
    ] {
        let bytes = crate::test_support::zip_test::f3d_with_form_body_resource(&program);
        let decoded = super::decode(bytes);
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Form"))
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Form(construction)) =
            feature.evaluation.definition()
        else {
            panic!("Form");
        };
        assert_eq!(construction.membership_is_complete(), valid);
        assert_eq!(decoded.ir().geometry.subds.len(), usize::from(valid));
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .any(crate::test_support::tspline_cage_given_up),
            !valid
        );
        let observation = &construction.observations()[0];
        let resource = match &observation.binding {
            FormCageBinding::Decoded {
                resource: Some(resource),
                ..
            } if valid => resource,
            FormCageBinding::Undecoded {
                resource,
                availability: FormResourceAvailability::ActiveUndecoded,
            } if !valid => resource,
            _ => panic!("active Form resource"),
        };
        assert_eq!(resource.entry.as_str(), "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm");
    }
}

#[test]
fn f3z_form_evidence_survives_shared_and_distinct_occurrences() {
    use crate::test_support::assembly_test::{
        f3d_with_reference_table_and_identity_xrefs, f3z_archive, redirections_json, XREF_ROLE,
    };
    const SECOND_ROLE: &str = "12345678-1234-4234-8234-123456789abc";
    let program = crate::test_support::zip_test::generated_form_tsm();
    let probe = super::decode(
        crate::test_support::zip_test::f3d_with_partial_form_body_resource_aliases(
            &program,
            "literal:probe",
        ),
    );
    let collision = probe
        .ir()
        .native
        .namespace("f3d")
        .unwrap()
        .arenas()
        .values()
        .flatten()
        .map(cadmpeg_ir::NativeRecord::id)
        .filter(|id| id.starts_with("f3d:"))
        .min_by_key(|id| id.len())
        .unwrap()
        .to_owned();
    assert!(
        collision.len() <= 64,
        "shortest native ID is {} bytes",
        collision.len()
    );
    let child = crate::test_support::zip_test::f3d_with_partial_form_body_resource_aliases(
        &program, &collision,
    );
    let unmerged = super::decode(child.clone());
    let original_form = unmerged
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Form(form)) => Some(form),
            _ => None,
        })
        .unwrap();
    let original_foreign = original_form.inputs().reference_evidence.iter().find(|reference| {
        matches!(&reference.target, FormNativeReferenceTarget::Document { asset_guid, .. } if asset_guid == &collision)
    }).unwrap();
    let original_group = original_foreign.group.expect("grouped foreign reference");
    let original_absent = original_form.inputs().reference_evidence.iter().find(|reference| {
        matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with("#9999"))
    }).unwrap();
    let absent_group = original_absent.group.expect("grouped absent reference");
    assert!(unmerged
        .ir()
        .native
        .namespace("f3d")
        .unwrap()
        .arenas()
        .values()
        .flatten()
        .any(|record| record.id() == collision));
    let redirections = redirections_json(
        "root.f3d",
        &[("child-a.f3d", XREF_ROLE), ("child-b.f3d", SECOND_ROLE)],
    );
    let root = f3d_with_reference_table_and_identity_xrefs(
        "assembly-design",
        "root.f3d",
        &[
            ("child-a.f3d", XREF_ROLE),
            ("child-a.f3d", XREF_ROLE),
            ("child-b.f3d", SECOND_ROLE),
        ],
        redirections.as_bytes(),
    );
    let decoded = super::decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("child-a.f3d", &child),
            ("child-b.f3d", &child),
        ],
    ));
    let forms = decoded
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Form(construction)) => {
                Some(construction)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(forms.len(), 2);
    let mut definitions = std::collections::BTreeMap::new();
    let model = &decoded.ir().model;
    for occurrence in &model.occurrences {
        let definition = &occurrence.prototype;
        assert!(
            model
                .product_definitions
                .iter()
                .any(|row| &row.id == definition
                    && !matches!(row.storage, cadmpeg_ir::products::Storage::External(_))),
            "local prototype"
        );
        *definitions.entry(definition).or_insert(0usize) += 1;
    }
    assert_eq!(definitions.len(), 2);
    assert_eq!(
        definitions
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        [1, 2].into_iter().collect()
    );
    let mut all_cages = std::collections::BTreeSet::new();
    let mut foreign_sources = std::collections::HashSet::new();
    for form in forms {
        let prefix = ["child-a.f3d@", "child-b.f3d@"]
            .into_iter()
            .find(|prefix| {
                form.inputs()
                    .references
                    .iter()
                    .any(|id| id.as_str().starts_with(&format!("f3d:{prefix}")))
            })
            .unwrap();
        assert!(!form.membership_is_complete());
        assert_eq!(form.committed_cages().count(), 2);
        assert_eq!(form.referenced_cages().count(), 2);
        for id in form.referenced_cages() {
            assert!(id.as_str().contains(prefix));
            assert!(decoded
                .ir()
                .geometry
                .subds
                .iter()
                .any(|cage| cage.id == *id));
            assert!(all_cages.insert(id.as_str().to_owned()));
        }
        let association = form
            .inputs()
            .resource_associations
            .iter()
            .find(|association| association.resource.source.as_str().ends_with("#1720"))
            .unwrap();
        assert!(association.cage.as_str().contains(prefix));
        assert!(association.resource.source.as_str().contains(prefix));
        assert!(form
            .referenced_cages()
            .any(|cage| cage == &association.cage));
        assert_eq!(association.resource.entry.as_str(), "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm");
        let foreign = form.inputs().reference_evidence.iter().find(|reference| matches!(&reference.target, FormNativeReferenceTarget::Document { asset_guid, target_type_guid: Some(type_guid), link_name, version: Some(version), .. } if asset_guid == &collision && type_guid == "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA" && link_name == &collision && version.key == "11111111-2222-3333-4444-555555555555" && version.urn == collision)).unwrap();
        assert!(foreign.source.as_str().contains(prefix));
        assert_eq!(foreign.group, Some(original_group));
        assert!(foreign_sources.insert(foreign.source.clone()));
        assert!(decoded
            .ir()
            .native
            .namespace("f3d")
            .unwrap()
            .arenas()
            .values()
            .flatten()
            .any(|record| record.id()
                == format!("f3d:{prefix}{}", collision.strip_prefix("f3d:").unwrap())));
        let absent = form
            .inputs()
            .reference_evidence
            .iter()
            .find_map(|reference| match &reference.target {
                FormNativeReferenceTarget::Local { target }
                    if target.as_str().ends_with("#9999") =>
                {
                    Some((target, reference.group))
                }
                _ => None,
            })
            .unwrap();
        let (absent, group) = absent;
        assert_eq!(group, Some(absent_group));
        assert!(absent.as_str().contains(prefix));
        assert!(!decoded
            .ir()
            .native
            .namespace("f3d")
            .unwrap()
            .arenas()
            .values()
            .flatten()
            .any(|record| record.id() == absent.as_str()));
        assert!(form.observations().iter().any(|observation| observation
            .paths
            .iter()
            .any(|path| path.references.contains(absent))));
        let resources = form
            .observations()
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Decoded {
                    resource: Some(resource),
                    ..
                } => Some(resource.entry.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(resources, [
            "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm",
            "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000100.tsm",
        ]);
    }
    assert_eq!(all_cages.len(), 4);
    assert_eq!(foreign_sources.len(), 2);
}

/// A radial map is the source's native map, so a map that repeats a source
/// decodes with the cage and is kept whole.
#[test]
fn a_radial_map_that_repeats_a_source_decodes_and_keeps_the_map() {
    let mut program = crate::test_support::zip_test::generated_form_tsm();
    program.extend_from_slice(
        b"105sym 1\n105plane 0.5 0.5 0 1 1 0 0 0 0 1 0 0\n\
          105r segments 4\n105r sweep 1\n105r ef 0 1 0 2\n105r er 1 0\n\
          105r ff 0 0\n105r fr 0 0\n105r vf 0 1\n105r vr 1 0\n",
    );
    let decoded = super::decode(crate::test_support::zip_test::f3d_with_form_body_resource(
        &program,
    ));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(crate::test_support::tspline_cage_given_up));
    let [subd] = decoded.ir().geometry.subds.as_slice() else {
        panic!("one cage");
    };
    let [symmetry] = subd.cage.symmetries() else {
        panic!("one symmetry");
    };
    let cadmpeg_ir::SubdSymmetryKind::Radial(radial) = symmetry.kind() else {
        panic!("radial symmetry");
    };
    let ef = radial
        .radial_maps
        .iter()
        .find(|map| map.selector == cadmpeg_ir::SubdRadialMapSelector::Ef)
        .expect("ef map");
    assert_eq!(ef.pairs, vec![[0, 1], [0, 2]]);
}

/// A repeated radial source over live edges: a pair is a neutral
/// correspondence only where its target has one source and the reverse map
/// holds its inverse. The native maps stay whole, and each affected pair is
/// charged as unresolved.
#[test]
fn a_repeated_radial_source_over_live_edges_projects_only_pairs_with_an_inverse() {
    for (reverse_edges, edge_pairs, unresolved) in [
        ("1 0", vec![[0_u32, 1]], 1),
        ("1 0 2 0", Vec::new(), 4_usize),
    ] {
        let mut program = crate::test_support::zip_test::generated_form_tsm();
        program.extend_from_slice(
            format!(
                "105sym 1\n105plane 0.5 0.5 0 1 1 0 0 0 0 1 0 0\n\
                 105r segments 4\n105r sweep 1\n105r ef 0 1 0 2\n105r er {reverse_edges}\n\
                 105r ff 0 0\n105r fr 0 0\n105r vf 0 1\n105r vr 1 0\n"
            )
            .as_bytes(),
        );
        let decoded = super::decode(crate::test_support::zip_test::f3d_with_form_body_resource(
            &program,
        ));
        let losses = &decoded.report().losses;
        assert!(!losses
            .iter()
            .any(crate::test_support::tspline_cage_given_up));
        let charged = losses
            .iter()
            .find(|loss| loss.code == crate::loss::F3dLossCode::TsplineRadialMapUnresolved.kind())
            .expect("unresolved radial pairs are charged");
        assert!(
            charged
                .message
                .starts_with(&format!("{unresolved} radial symmetry pair(s)")),
            "{}",
            charged.message
        );
        let [subd] = decoded.ir().geometry.subds.as_slice() else {
            panic!("one cage");
        };
        let [symmetry] = subd.cage.symmetries() else {
            panic!("one symmetry");
        };
        assert_eq!(symmetry.edge_pairs(), edge_pairs.as_slice());
        assert_eq!(symmetry.face_pairs(), [[0_u32, 0]]);
        assert_eq!(symmetry.vertex_pairs(), [[0_u32, 1]]);
        let cadmpeg_ir::SubdSymmetryKind::Radial(radial) = symmetry.kind() else {
            panic!("radial symmetry");
        };
        let ef = radial
            .radial_maps
            .iter()
            .find(|map| map.selector == cadmpeg_ir::SubdRadialMapSelector::Ef)
            .expect("ef map");
        assert_eq!(ef.pairs, vec![[0, 1], [0, 2]]);
    }
}
