# NIST MBE PMI producer evidence

Curated subset of the NIST Model-Based Engineering (MBE) Product and Manufacturing Information (PMI) Validation and Conformance Testing system. These files are producer evidence for decoder and encoder work: semantic PMI, native CAD containers, and the two PMI-definition spreadsheets that name the expected annotations.

cadmpeg did not author these files and does not dedicate them CC0. They are U.S. government works in the public domain.

This is not a dump of the NIST archives. Whales, overlapping STC natives, drawing PDFs, and McMaster fastener SLDPRTs from the MTC zip are omitted. The files here are producer evidence, not default `cargo test` goldens. A scheduled workflow inspects and decodes them without committing IR snapshots: [nist-pmi-goldens.yml](../../.github/workflows/nist-pmi-goldens.yml) (`#[ignore]` in `cargo test-fast`).

## Disclaimer (NIST)

This test system (test cases, CAD models, model verification and validation results) was developed at the National Institute of Standards and Technology by employees of the Federal Government in the course of their official duties. Pursuant to Title 17 Section 105 of the United States Code this test system is not subject to copyright protection and is in the public domain. NIST assumes no responsibility for the components of the test system for use by other parties and makes no guarantees, expressed or implied, about their quality, reliability, or any other characteristic. https://www.nist.gov/disclaimer

The test cases are not intended to represent best practice in how to apply GD&T to a part. Simpler GD&T strategies could have been used. The test cases are intended to exercise valid presentations of GD&T defined in the ASME Y14 standards.

The test cases and CAD models can be used without any restrictions. Their use in other software or hardware products does not imply a recommendation or endorsement by NIST of those products.

Acknowledgement is appreciated. Use of the NIST logo is not allowed.

See also https://www.nist.gov/open/license and https://www.nist.gov/disclaimer.

## Project pages

- Landing: https://www.nist.gov/ctl/smart-connected-systems-division/smart-connected-manufacturing-systems-group/mbe-pmi-0
- Project: https://www.nist.gov/ctl/smart-connected-systems-division/smart-connected-manufacturing-systems-group/mbe-pmi-validation
- Short URL used in NIST's own README files: https://go.usa.gov/mGVm
- Test-case browser: https://pages.nist.gov/CAD-PMI-Testing/models.html
- CAx-IF republish: https://www.mbx-if.org/home/cax/resources/

## Acquisition

Downloaded 2026-10-30 from the NIST document pages (each page serves the zip). Bytes were taken from those zips; they were not copied from a private cache.

| Archive | Document page | Direct zip | SHA-256 of the zip |
| --- | --- | --- | --- |
| CTC definitions | https://www.nist.gov/document/nist-ctc-test-case-definitions | https://www.nist.gov/system/files/documents/noindex/2022/07/19/NIST_MBE_PMI_CTC_Definitions.zip | `79ebc74d99eb05c0d23de1c1d8730651fa2a24def56f647bac21dd439ec40690` |
| FTC definitions | https://www.nist.gov/document/nist-ftc-test-case-definitions | https://www.nist.gov/system/files/documents/noindex/2022/07/19/NIST_MBE_PMI_FTC_Definitions.zip | `3762211e6c5e6c75055f56366abe59c43d5824468fb313d7818ba8ee6b777c15` |
| Native FTC/CTC CAD | https://www.nist.gov/document/nist-ftc-ctc-pmi-cad-models | https://www.nist.gov/system/files/documents/noindex/2024/05/07/NIST-FTC-CTC-PMI-CAD-models.zip | `17e14dbf207f447f16e0182a7eda5254ee19afff591dd409b8b510103dd05d10` |
| STEP AP203/AP242 | https://www.nist.gov/document/nist-pmi-step-files | https://www.nist.gov/system/files/documents/noindex/2024/06/19/NIST-PMI-STEP-Files.zip | `8fa78429e6d8d9b0d7681d223b6aa9ec98c3772185c55b1a0e3679b21c181911` |
| MTC assembly | https://www.nist.gov/document/nist-cad-models-mtc-assembly | https://www.nist.gov/system/files/documents/noindex/2025/09/04/NIST-MTC-Assembly.zip | `9aeb53e54f682ea1732857d06a7f0513c71667a2d84407396325fa6ce5340bbc` |

The two PMI-definition workbooks also appear inside the native CAD zip; the copies here were taken from the dedicated definition zips. `nist_ftc_09_asme1_rd.stp` is the geometry-only STEP from the native CAD zip's `FTC Definitions/` folder (263934 bytes), not the slightly different AP203 copy in the STEP zip.

NIST stripped preprocessor / authoring-application names from the STEP headers. Native `authoring_app` values below follow the folder names in the publisher zip.

## What is in this subset

Spreadsheets (not `[[file]]` entries: `corpus_manifest` classifies through a codec):

- `definitions/NIST-CTC-PMI-Definitions.xlsx` — CTC-01..05 expected PMI
- `definitions/NIST-FTC-PMI-Definitions.xlsx` — FTC-06..11 expected PMI

Semantic AP242 STEP (CTC-01..05, FTC-06, 07, 10):

- `step/nist_ctc_01_asme1_ap242-e1.stp` — smallest CTC AP242; baseline semantic PMI
- `step/nist_ctc_02_asme1_ap242-e2.stp` — line-profile and dense NURBS
- `step/nist_ctc_03_asme1_ap242-e2.stp` — highest CTC PMI density
- `step/nist_ctc_04_asme1_ap242-e1.stp` — holes, countersink, counterbore
- `step/nist_ctc_05_asme1_ap242-e1.stp` — runout, circularity, concentricity
- `step/nist_ftc_06_asme1_ap242-e2.stp` — spheres, tapers, dense FTC PMI
- `step/nist_ftc_07_asme1_ap242-e2.stp` — mixed NURBS and analytics
- `step/nist_ftc_10_asme1_ap242-e2.stp` — multi-body, cylindricity, symmetry

Geometry-only STEP:

- `step/nist_ftc_09_asme1_rd.stp` — FTC-09 slots without the tessellated AP242 whale

Natives (one design per format except SolidWorks, which takes two distinct FTC cases):

- `catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart` — CATIA V5-6R2019; spheres, tori, NURBS
- `nx/nist_ctc_03_asme1_nx1980_rc.prt` — NX 1980; multi-body feature tree (`format = "nx"`)
- `nx/NIST mtc crada assembly.prt` — NX MTC box assembly placements, no fastener library
- `creo/nist_ctc_01_asme1_rd_cr10030.prt.5` — Creo 10; CTC-01 twin of the Inventor native
- `inventor/INV_nist_ctc_01_asme1_2021.ipt` — Inventor 2021; CTC-01 twin of the Creo native
- `sldprt/nist_ftc_06_asme1_rd_sw1802.SLDPRT` — SolidWorks MBD 2018; semantic PMI in `model.pmi`
- `sldprt/nist_ftc_08_asme1_rc_sw1802.SLDPRT` — SolidWorks MBD 2018; free-state / tangent-plane PMI

The three publisher `README.txt` files from the definition and STEP zips are folded into this document rather than committed as extra copies.

## Manifest

CAD files are listed in [`../manifest.toml`](../manifest.toml) with `license = "LicenseRef-NIST-PD"`, `author = "National Institute of Standards and Technology"`, the document-page `source_url`, and `acquisition_date = "2026-10-30"`. Dialect pins are derived by `UPDATE_CORPUS_DIALECTS=1 cargo test -p cadmpeg --test corpus_manifest`.
