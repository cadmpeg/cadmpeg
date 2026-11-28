# Format authoring

Write dense, direct technical prose in ASD-STE100. State each fact once and affirmatively.

Specifications state the settled format model as fact: byte semantics and invariants. They carry no corpus, sample, experiment, or provenance qualifiers, research history, project status, implementation bugs, or export behavior.

Put genuine unknowns in `*-open-items.md` with the fields Question, Known, Need, Conflict, and Note. Finite evidence is not an unknown. When an item is settled, write the answer into the specification and delete the item in the same change.

An open item is self-contained. Describe the stored form in Known: the record or frame, its length, fields and offsets, their values, and relationships between records. State a form's facts without naming a file, design, or sample or describing one in place of its name.

Do not locate a fact by an absolute record index, stream offset, entity identity, or feature label that has meaning only in one file. State a relation between records when the specification, layout table, or decoder confirms it. Otherwise give the stored values as an instance of the form without a locator. State what the form stores and the values it takes rather than counting files or records.

Write Need as the save that would settle the question: the model to build, the control to change, and the records to compare.
