# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 3. Persistent topology identity

## 4. Exact-topology transfer

## 5. Design projection

## 6. Semantic annotations

## 7. TechDraw projection

### DG-06. Non-page template relationship admission

**Question.** Which drawing runtime types may populate the neutral page-template field?

**Known.** `drawing.rs:32-45` extracts typed `Views` and `Template` carriers only for page
objects, but `drawing.rs:66-70` retains every link-valued property as a relationship. Neutral
transfer at `drawing.rs:175-190` then reads any relationship named `Template` for every registered
drawing record. The specification limits page template carriers to pages and states that other
runtime types do not supply them.

**Need.** Gate neutral `Drawing.template` extraction on the page runtime kind. Retain a non-page
`Template` relationship as native relationship data without interpreting it as page membership.

**Conflict.** A registered non-page view with an `App::PropertyLink` named `Template` targeting a
registered template populates `Drawing.template`, although the source type cannot own a page
template carrier. An unrelated link property therefore changes a neutral page field without a
refusal or loss.

**Note.** New hostile-sweep finding.

## 9. Assembly joints

## 10. Attachment and assembly

## 11. Persistent graph admission
