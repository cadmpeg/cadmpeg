# Form constructions

`FormConstruction` retains native inputs, cage observations, and membership. `default()` creates unknown membership. `complete()` admits an ordered list of distinct subdivision cage IDs. `try_new()` checks an explicit construction and returns an error when its identities, positions, resource states, or membership disagree.

An observation records a decoded cage, an undecoded resource, or an unresolved binding. Membership selects observations as confirmed members. Other decoded observations are candidates. `committed_cages()` returns decoded members. `referenced_cages()` returns members and candidates. A known empty complete construction remains distinct from unknown membership.

`FormInputs` retains native parameters, direct references, counted lists, and supplemental reference, position, cardinality, and resource evidence. A reference names its source and role and retains its local or foreign target. Its optional run groups slots from one source record. Position evidence refers to a canonical observation; an optional source identifies its native list. Complete membership must agree with every supplemental position and cardinality claim. Conflicting claims remain evidence while the selected membership stays partial.

Complete cage arrays have distinct values. When `cage_inputs` is present, the array agrees with its selected decoded observations. The schema describes structural variants and array uniqueness. The constructor and deserializer enforce cross-field agreement, valid observation indices and positions, distinct observations, and compatible resource states.

Identity rewriting visits all typed native and subdivision identities, including absent native endpoints. Literal parameter, resource, link, and version text remains unchanged.
