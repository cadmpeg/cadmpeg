// SPDX-License-Identifier: Apache-2.0
//! The §4.5 no-unexpected-give-ups contract (#215): give-ups match the
//! pinned F3D decode, while other undamaged fixtures give up no part.

use super::support;

fn check_format(format: &str) {
    let fixtures = support::sampled_fixtures(format);
    let mut findings = Vec::new();
    for fixture in fixtures.iter().filter(|fixture| !fixture.damaged) {
        let decoded = match support::decode(format, &fixture.bytes) {
            Ok(decoded) => decoded,
            Err(_) if support::pins_decode_error(&fixture.path) => continue,
            Err(error) => {
                findings.push(format!(
                    "{format}: {} refuses its input: {error}",
                    fixture.path.display()
                ));
                continue;
            }
        };
        if format == "f3d" {
            let golden = fixture.path.parent().and_then(|directory| {
                fixture.path.file_stem().map(|stem| {
                    directory
                        .with_file_name("decode")
                        .join(stem)
                        .with_extension("json")
                })
            });
            let expected = golden
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|golden| {
                    golden
                        .pointer("/report/losses")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                });
            let actual: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == support::UNREADABLE)
                .map(|loss| serde_json::to_value(loss).expect("serialize give-up"))
                .collect();
            let expected: Option<Vec<_>> = expected.map(|losses| {
                losses
                    .into_iter()
                    .filter(|loss| {
                        loss.pointer("/code/code")
                            .and_then(serde_json::Value::as_str)
                            == Some(support::UNREADABLE)
                    })
                    .collect()
            });
            if expected.as_ref() != Some(&actual) {
                findings.push(format!("{format}: {} give-ups differ from its pinned decode: expected {expected:?}, got {actual:?}", fixture.path.display()));
            }
        } else {
            for loss in &decoded.report().losses {
                if loss.code.local_code() == super::support::UNREADABLE {
                    findings.push(format!(
                        "{format}: {} charges record.unreadable outside a damaged directory: {}",
                        fixture.path.display(),
                        loss.message
                    ));
                }
            }
        }
    }
    support::assert_no_findings(&findings);
}

macro_rules! no_unexpected_give_ups {
    ($($module:ident => $format:literal, $test:ident);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn $test() {
                super::check_format($format);
            }
        })*
    };
}

no_unexpected_give_ups! {
    step_no_unexpected_give_ups => "step", step_charges_no_unexpected_give_up;
    iges_no_unexpected_give_ups => "iges", iges_charges_no_unexpected_give_up;
    rhino_no_unexpected_give_ups => "rhino", rhino_charges_no_unexpected_give_up;
    freecad_no_unexpected_give_ups => "fcstd", freecad_charges_no_unexpected_give_up;
    f3d_no_unexpected_give_ups => "f3d", f3d_charges_no_unexpected_give_up;
    sldprt_no_unexpected_give_ups => "sldprt", sldprt_charges_no_unexpected_give_up;
    catia_no_unexpected_give_ups => "catia", catia_charges_no_unexpected_give_up;
    creo_no_unexpected_give_ups => "creo", creo_charges_no_unexpected_give_up;
    usd_no_unexpected_give_ups => "usd", usd_charges_no_unexpected_give_up;
    inventor_no_unexpected_give_ups => "inventor", inventor_charges_no_unexpected_give_up;
    sat_no_unexpected_give_ups => "sat", sat_charges_no_unexpected_give_up;
    nx_no_unexpected_give_ups => "nx", nx_charges_no_unexpected_give_up;
}
