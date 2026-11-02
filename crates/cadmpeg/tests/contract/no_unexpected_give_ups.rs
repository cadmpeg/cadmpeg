// SPDX-License-Identifier: Apache-2.0
//! The §4.5 no-unexpected-give-ups contract (#215): no golden outside a
//! `damaged/` directory charges `record.unreadable`.

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
}
