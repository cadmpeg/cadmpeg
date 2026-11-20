#!/usr/bin/env python3
"""Attribute LLVM IR lines of one crate to serde instantiation categories."""
import collections, re, subprocess, sys

per_fn = {}
current = None
count = 0
for line in open(sys.argv[1], errors="replace"):
    if line.startswith("define "):
        match = re.search(r'@"?([^"(\s]+)"?\(', line)
        current = match.group(1) if match else "?"
        count = 0
    elif current is not None:
        count += 1
        if line.startswith("}"):
            per_fn[current] = per_fn.get(current, 0) + count
            current = None
names = list(per_fn)
out = subprocess.run(["rustfilt"], input="\n".join(names), capture_output=True, text=True).stdout.split("\n")
demangled = dict(zip(names, out))
total = sum(per_fn.values())

RULES = [
    ("schemars", r"schemars|json_schema|subschema_for"),
    ("de: serde Content buffering", r"private::de::content|ContentDeserializer|ContentRefDeserializer|TaggedContentVisitor|AdjacentlyTagged.*Visitor|FlatMapDeserializer|MapDeserializer<|SeqDeserializer<|visit_content_"),
    ("de: serde_json text (StrRead/SliceRead)", r"serde_json::de::|StrRead|SliceRead|MapAccess<|SeqAccess<|MapKey<"),
    ("de: serde_json Value", r"serde_json::value::de|Value as serde_core::de::Deserializer|ValueVisitor"),
    ("ser: FiniteSerializer", r"FiniteSerializer|FiniteValue"),
    ("ser: RawValueStrEmitter", r"RawValueStrEmitter"),
    ("ser: TaggedSerializer", r"TaggedSerializer"),
    ("ser: FlatMapSerializer", r"FlatMapSerialize"),
    ("ser: serde_json::Value serializer (to_value)", r"serde_json::value::ser"),
    ("ser: LimitedProjection", r"LimitedProjection"),
    ("ser: Projector", r"Projector"),
    ("ser: serde_json text Serializer", r"serde_json::ser::"),
    ("ser: other serialize", r"::serialize|Serializer|serialize_"),
    ("de: other deserialize", r"deserialize|Deserializer|Visitor|visit_"),
    ("drop/clone/eq/debug/hash", r"drop_in_place|drop_glue|::clone|::eq|::fmt|::hash"),
]
cats = collections.Counter()
for name, lines in per_fn.items():
    text = demangled.get(name, name)
    for label, pattern in RULES:
        if re.search(pattern, text):
            cats[label] += lines
            break
    else:
        cats["other (non-serde)"] += lines
print(f"total lines {total} functions {len(per_fn)}")
for label, lines in cats.most_common():
    print(f"{lines:10} {100*lines/total:5.1f}%  {label}")

# Self types with the most serde code
selfs = collections.Counter()
for name, lines in per_fn.items():
    text = demangled.get(name, name)
    match = re.match(r"<(cadmpeg_ir::[\w:]+)(?:<[^>]*>)? as (serde_core::(?:ser::Serialize|de::Deserialize)|schemars::JsonSchema)>", text)
    if match:
        selfs[(match.group(1), match.group(2).split("::")[-1])] += lines
    else:
        match = re.search(r"<(cadmpeg_ir::[\w:]+)(?:<[^>]*>)? as serde_core::de::Deserialize>::deserialize::", text)
        if match:
            selfs[(match.group(1), "Deserialize(inner)")] += lines
print("\ntypes with the most direct serde/schemars lines:")
for (ty, tr), lines in selfs.most_common(30):
    print(f"{lines:9} {100*lines/total:5.1f}%  {tr:20} {ty}")
