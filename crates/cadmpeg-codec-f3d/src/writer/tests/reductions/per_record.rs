// SPDX-License-Identifier: Apache-2.0
//! A record the generator cannot write as it stands is degraded or renamed
//! with a loss naming it, and the rest of the document is written.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::ProceduralCurve;

use crate::loss::F3dLossCode;
use crate::records::configuration::DesignConfiguration;
use crate::test_support::configuration_table::project;
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::test_support::smbh_curves_test::synthetic_geometry_with_subset_curve_smbh;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

fn is_subset(procedural: &ProceduralCurve) -> bool {
    matches!(
        procedural.definition(),
        ProceduralCurveDefinition::Subset(_)
    )
}

#[test]
fn a_subset_that_cannot_be_restated_on_its_source_is_written_as_its_cache() {
    let mut ir = super::source_less(&synthetic_geometry_with_subset_curve_smbh());
    let index = ir
        .geometry
        .procedural_curves
        .iter()
        .position(is_subset)
        .expect("a subset construction");
    let construction = ir.geometry.procedural_curves[index].id.clone();
    let carrier = ir
        .geometry
        .procedural_curve_owner(&construction)
        .expect("the subset's carrier")
        .clone();
    let ProceduralCurveDefinition::Subset(subset) =
        ir.geometry.procedural_curves[index].definition()
    else {
        panic!("a subset construction");
    };
    let cyclic = SubsetCurveConstruction::try_new(
        carrier.clone(),
        subset.parameter_range().endpoints(),
        *subset.sense(),
        subset.legacy_cache(),
    )
    .expect("a subset of its own carrier");
    ir.geometry.procedural_curves[index] = ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Subset(cyclic),
    );

    let (report, decoded) = super::write(&ir);
    let reduced = report
        .losses
        .iter()
        .find(|loss| {
            loss.code == F3dLossCode::WriterProceduralReduced.kind()
                && loss.message.contains(construction.as_str())
        })
        .unwrap_or_else(|| panic!("{:#?}", report.losses));
    assert!(
        reduced.message.contains(carrier.as_str()),
        "{}",
        reduced.message
    );
    assert!(
        reduced.message.contains("cyclic source chain"),
        "{}",
        reduced.message
    );
    assert!(!decoded.geometry.procedural_curves.iter().any(is_subset));
    assert_eq!(decoded.geometry.bodies.len(), ir.geometry.bodies.len());
    assert_eq!(decoded.geometry.edges.len(), ir.geometry.edges.len());
}

/// The source-less document of a design holding the configuration rule
/// `name`.
fn with_configuration_rule(name: &str) -> CadIr {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                br#"{"when":"width > 20 mm","activate":"wide"}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("a configuration rule decodes");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "f3d",
        &[],
    )
    .expect("no native unknowns");
    ir
}

#[test]
fn a_configuration_entry_name_outside_the_design_folder_is_renamed_into_it() {
    let stored = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.456.dsgcfgrule";
    let outside = "../outside/DesignConfigurationRule.456.dsgcfgrule";
    let mut ir = with_configuration_rule(stored);
    let payload = update_f3d_native(&mut ir, |native| {
        let configuration = &mut native.design_configurations[0];
        let payload = configuration.payload().clone();
        *configuration =
            DesignConfiguration::try_new(outside.into(), configuration.kind(), payload.clone())
                .expect("a configuration rule name");
        payload
    });
    ir.model.configurations = project(&ir);

    let (report, decoded) = super::write(&ir);
    let native = f3d_native(&decoded);
    assert_eq!(native.design_configurations.len(), 1);
    assert_eq!(native.design_configurations[0].entry_name(), stored);
    assert_eq!(native.design_configurations[0].payload(), &payload);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains(outside)
                && loss.message.contains(stored)),
        "{:#?}",
        report.losses
    );
}
