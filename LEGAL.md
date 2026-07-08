# Legal and provenance policy

cadmpeg is a clean-room CAD transcoding project. This document states how contributors may derive format knowledge, which sources are forbidden, and how to report a concern or request a takedown. Contributors must follow it. If any part of it conflicts with a contribution you want to make, **do not make the contribution**; open an issue and ask first.

Engineers, not lawyers, wrote this document; it is not legal advice. It describes the project's operating rules.

---

## Clean-room posture

Every format spec and every decoder in cadmpeg must come from analyzing CAD files that the analyst is **legally entitled to possess**, using only observation of those files' bytes and publicly available information.

Allowed sources:

- Files you authored yourself in the relevant CAD application, or files whose license/terms permit inspection and redistribution of derived knowledge.
- Observation of file bytes: hex analysis, entropy profiling, structural diffing across files you made yourself with controlled changes.
- Publicly published information: open standards, published papers, vendor documentation that is public and unrestricted, and prior open-source work under compatible licenses.

---

## Forbidden sources

Contributions must **not** be derived from, and must not include, any of the following:

1. **Vendor SDKs, toolkits, or libraries** (e.g. proprietary geometry-kernel SDKs or format read/write toolkits), including their headers, symbol names, sample code, or any material distributed under their license. Copying structure or constants out of an SDK is not clean-room.
2. **Decompiled, disassembled, or reverse-engineered vendor binaries.** Do not decompile CAD applications or their DLLs/shared libraries and transcribe what you find. You may inspect output files. You may not inspect the implementation that produced them.
3. **Confidential, NDA-covered, or leaked material**: internal specs, format documentation shared under confidentiality, or anything obtained through a breach of an agreement or of access controls.
4. **Files you are not entitled to possess or share**, including customer or third-party proprietary parts used without permission.
5. **Code or specs under license terms incompatible** with Apache-2.0 (code) or CC-BY-4.0 (docs).
6. **Circumvention of technical protection measures.** cadmpeg does not decode content protected by encryption, DRM, license enforcement, or other access controls, and does not accept tooling, keys, or format knowledge whose purpose or derivation is the bypassing of such a measure. Analyzing unprotected bytes in files you may possess is in scope; defeating a protection measure is not.

If you have ever been under NDA with a CAD vendor, or have had access to a vendor's format SDK source, and you are unsure whether your knowledge of a format is affected by that access, **do not contribute to that format's decoder or spec.** See the conflict rule below.

---

## NDA and conflict-of-knowledge rule

You may not contribute decoder code or format-spec content for a format if your knowledge of that format could reasonably have come from a source on the forbidden list, for example if you are or were under NDA covering that format, or worked with the vendor's format SDK. Independent re-derivation by the same contributor does not cure forbidden-source access for that format; each format claim must remain traceable to allowed sources rather than to a contributor's memory.

Contributing to _other_ parts of the project (unrelated formats, tooling, docs, the IR, exporters) is permitted.

---

## Provenance declaration

Every pull request that adds or changes a **decoder** or a **format specification** must include a provenance declaration in the PR description. Use the following text only if accurate:

> I derived this contribution solely by analyzing CAD files I am legally entitled to possess and from publicly available information. I did not use vendor SDKs, decompiled binaries, confidential or NDA-covered material, or any source listed as forbidden in LEGAL.md.

If you cannot sign that sentence, the contribution is not acceptable in its current form. This is in addition to the DCO sign-off required on all commits (see [CONTRIBUTING.md](CONTRIBUTING.md)).

---

## Trademarks

Format names and CAD application names (SolidWorks, Fusion 360, Creo, NX, and others) are trademarks of their respective owners. cadmpeg uses them only nominatively, to say which files a decoder targets. cadmpeg is not affiliated with, endorsed by, or sponsored by any of these vendors.

---

## Reporting a concern / takedown requests

If you believe any content in cadmpeg was derived from a forbidden source, infringes a right you hold, or violates this policy, report it to the maintainers.

- Open a GitHub issue describing the concern, **or**, if the matter is sensitive, contact the maintainers privately via the security/contact address listed in the repository's `SECURITY.md` or repository metadata.
- Identify the specific file(s), commit(s), or spec section(s) at issue and the basis for the concern.
- Maintainers will review the report. Where a concern is substantiated, maintainers will remove or revert the affected material pending provenance review. If provenance is unclear, maintainers will remove the material and re-derive it from allowed sources.

The project accepts good-faith reports without retaliation by maintainers.
