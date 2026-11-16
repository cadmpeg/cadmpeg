use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_work_axis;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn work_axis_partial_evidence_and_literal_names_survive_real_f3z_merge() {
    for geometry in [false, true] {
        let smbh = if geometry {
            synthetic_geometry_smbh()
        } else {
            Vec::new()
        };
        let first = f3d_with_work_axis(&smbh, "source", false);
        let first = crate::test_support::archive_session::decode(
            &mut Cursor::new(first),
            &DecodeOptions::default(),
        )
        .unwrap();
        let axis = first
            .ir()
            .model
            .features
            .iter()
            .find(|f| {
                matches!(
                    f.evaluation.definition().operation(),
                    FeatureOperation::DatumAxis { .. }
                )
            })
            .expect("literal synthetic carrier produces a datum axis");
        let feature_id = axis.id.as_str();
        let literal = axis.native_ref.as_deref().expect("native scope owner");
        let component = f3d_with_work_axis(&smbh, literal, true);
        let alone = EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(component.clone()),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        let root = f3d_without_brep(
            "assembly-design",
            "root.f3d",
            &[("comp.f3d", XREF_ROLE), ("comp.f3d", XREF_ROLE)],
        );
        let archive = f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", root.as_slice()),
                ("comp.f3d", component.as_slice()),
            ],
        );
        let merged = EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(archive),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        let axes = merged
            .ir()
            .model
            .features
            .iter()
            .filter(|f| {
                matches!(
                    f.evaluation.definition().operation(),
                    FeatureOperation::DatumAxis { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            axes.len(),
            1,
            "repeated placements share one member definition"
        );
        assert_ne!(axes[0].id.as_str(), feature_id);
        assert_ne!(axes[0].native_ref.as_deref(), Some(literal));
        let parameters = &merged.ir().model.parameters;
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "Literal");
        assert_eq!(parameters[0].expression, literal);
        assert_eq!(parameters[0].owner.as_ref(), Some(&axes[0].id));

        let FeatureOperation::DatumAxis { axis } = axes[0].evaluation.definition().operation()
        else {
            unreachable!()
        };
        assert!(axis.geometry().is_some());
        let native =
            crate::native::F3dNative::load(merged.ir().native.namespace("f3d").unwrap()).unwrap();
        assert!(native
            .design_parameter_scopes
            .iter()
            .any(|scope| Some(scope.id.as_str()) == axes[0].native_ref.as_deref()));
        assert_eq!(native.design_parameters.len(), 1);
        assert_eq!(native.design_parameters[0].expression(), literal);
        let names = native
            .design_parameter_scopes
            .iter()
            .filter_map(|s| s.work_axis_construction())
            .flat_map(|a| &a.candidates)
            .filter_map(|c| c.prefix.name.value())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![literal],
            "native literal exactly equal to a source scope identity survives namespacing"
        );
        let unreadable = |result: &EditableDecodeResult| {
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
                .count()
        };
        assert_eq!(unreadable(&alone), 1);
        assert_eq!(unreadable(&merged), 1);
        let stubs = merged.ir().native_unknowns("f3d").unwrap();
        let stub = stubs
            .iter()
            .find(|stub| {
                merged
                    .source_fidelity()
                    .retained_record(stub.id.as_str())
                    .is_some_and(|record| record.data() == Some(f64::NAN.to_le_bytes().as_slice()))
            })
            .expect("the failed scalar has its own retained byte image");
        assert_eq!(stub.links.len(), 1);
        assert!(native
            .design_parameter_scopes
            .iter()
            .any(|scope| scope.id == stub.links[0].as_str()));

        assert_eq!(merged.ir().geometry.bodies.is_empty(), !geometry);
    }
}
