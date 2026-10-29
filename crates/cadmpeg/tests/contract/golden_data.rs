// SPDX-License-Identifier: Apache-2.0
//! Data checks 5, 6 and 7 (#246 IV.2) over every committed decode golden.
//!
//! `FreeCAD` decode goldens pin native arenas by digest, so its checks
//! decode the corpus fixtures those goldens snapshot.

use cadmpeg_test_support::data_checks::properties::property_map_findings;
use cadmpeg_test_support::data_checks::{
    decode_goldens, equation_identities, expression_text_findings, feature_ordinal_findings,
    freecad_stale_findings, never_stale_findings, solidworks_stale_findings, DecodeGolden,
};
use serde_json::Value;

fn assert_documents_clean(
    label: &str,
    documents: &[DecodeGolden],
    check: impl Fn(&DecodeGolden) -> Vec<String>,
) {
    assert!(!documents.is_empty(), "{label} has no decoded documents");
    let findings: Vec<String> = documents
        .iter()
        .flat_map(|golden| {
            check(golden)
                .into_iter()
                .map(move |finding| format!("{}: {finding}", golden.path))
        })
        .collect();
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

fn assert_clean(crate_name: &str, check: impl Fn(&Value) -> Vec<String>) {
    let goldens = decode_goldens(crate_name);
    assert!(!goldens.is_empty(), "{crate_name} has no decode goldens");
    let findings: Vec<String> = goldens
        .iter()
        .flat_map(|golden| {
            check(&golden.ir)
                .into_iter()
                .map(move |finding| format!("{}: {finding}", golden.path))
        })
        .collect();
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

macro_rules! golden_data_checks {
    ($($codec:ident => $crate_name:literal, $namespace:literal),* $(,)?) => {
        $(mod $codec {
            const NAMESPACE: &str = $namespace;

            #[test]
            fn property_maps_hold_no_native_fact() {
                super::assert_clean($crate_name, |document| super::property_map_findings(NAMESPACE, document));
            }

            #[test]
            fn feature_ordinals_are_dense() {
                super::assert_clean($crate_name, super::feature_ordinal_findings);
            }

            #[test]
            fn expression_text_names_no_parameter() {
                super::assert_clean($crate_name, |document| {
                    super::expression_text_findings(document, &super::equation_identities(document))
                });
            }
        })*
    };
}

macro_rules! never_stale {
    ($($module:ident => $crate_name:literal),* $(,)?) => {
        $(mod $module {
            #[test]
            fn needs_recompute_is_false() {
                super::assert_clean($crate_name, super::never_stale_findings);
            }
        })*
    };
}

golden_data_checks! {
    sat => "cadmpeg-codec-sat", "sat",
    step => "cadmpeg-codec-step", "step",
    iges => "cadmpeg-codec-iges", "iges",
    rhino => "cadmpeg-codec-rhino", "rhino",
    freecad => "cadmpeg-codec-freecad", "fcstd",
    f3d => "cadmpeg-codec-f3d", "f3d",
    sldprt => "cadmpeg-codec-sldprt", "sldprt",
    inventor => "cadmpeg-codec-inventor", "inventor",
    catia => "cadmpeg-codec-catia", "catia",
    creo => "cadmpeg-codec-creo", "creo",
    nx => "cadmpeg-codec-nx", "nx",
}

never_stale! {
    sat_rule_e => "cadmpeg-codec-sat",
    step_rule_e => "cadmpeg-codec-step",
    iges_rule_e => "cadmpeg-codec-iges",
    rhino_rule_e => "cadmpeg-codec-rhino",
    f3d_rule_e => "cadmpeg-codec-f3d",
    inventor_rule_e => "cadmpeg-codec-inventor",
    catia_rule_e => "cadmpeg-codec-catia",
    nx_rule_e => "cadmpeg-codec-nx",
}

mod sldprt_rule_e {
    #[test]
    fn needs_recompute_follows_the_active_configuration() {
        super::assert_clean("cadmpeg-codec-sldprt", super::solidworks_stale_findings);
    }
}

#[cfg(feature = "fcstd")]
mod freecad_corpus {
    use std::io::Cursor;
    use std::path::Path;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_test_support::data_checks::DecodeGolden;

    fn corpus_decodes() -> Vec<DecodeGolden> {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
        let mut paths: Vec<_> = std::fs::read_dir(&directory)
            .expect("FreeCAD corpus fixtures")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "FCStd")
            })
            .collect();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).expect("FreeCAD fixture bytes");
                let decoded = cadmpeg_codec_freecad::FcstdCodec
                    .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                DecodeGolden {
                    path: path.display().to_string(),
                    ir: serde_json::to_value(decoded.ir()).expect("CADIR JSON"),
                    fidelity: serde_json::to_value(decoded.source_fidelity())
                        .expect("fidelity JSON"),
                }
            })
            .collect()
    }

    #[test]
    fn property_maps_hold_no_native_fact_on_every_corpus_fixture() {
        super::assert_documents_clean("FreeCAD corpus", &corpus_decodes(), |golden| {
            super::property_map_findings("fcstd", &golden.ir)
        });
    }

    #[test]
    fn needs_recompute_follows_touched_on_every_corpus_fixture() {
        super::assert_documents_clean("FreeCAD corpus", &corpus_decodes(), |golden| {
            super::freecad_stale_findings(&golden.ir)
        });
    }
}
