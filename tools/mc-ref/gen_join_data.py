#!/usr/bin/env python3
"""
Build Hyperion join-time registry + tag tables from vanilla 26.2.

Inputs:
  - tools/mc-ref/server-inner-26.2.jar  (datapack JSON)
  - tools/mc-ref/datagen/generated/reports/registries.json  (built-in IDs)

Outputs:
  - crates/hyperion_server/src/network/join_data/generated.rs
"""

from __future__ import annotations

import json
import zipfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
JAR = Path(__file__).with_name("server-inner-26.2.jar")
REG_REPORT = Path(__file__).with_name("datagen") / "generated" / "reports" / "registries.json"
OUT_RS = ROOT / "crates" / "hyperion_server" / "src" / "network" / "join_data" / "generated.rs"

# Folder under data/minecraft → synchronized registry id
SYNC_FOLDERS: dict[str, str] = {
    "banner_pattern": "minecraft:banner_pattern",
    "cat_sound_variant": "minecraft:cat_sound_variant",
    "cat_variant": "minecraft:cat_variant",
    "chat_type": "minecraft:chat_type",
    "chicken_sound_variant": "minecraft:chicken_sound_variant",
    "chicken_variant": "minecraft:chicken_variant",
    "cow_sound_variant": "minecraft:cow_sound_variant",
    "cow_variant": "minecraft:cow_variant",
    "damage_type": "minecraft:damage_type",
    "dialog": "minecraft:dialog",
    "dimension_type": "minecraft:dimension_type",
    "enchantment": "minecraft:enchantment",
    "frog_variant": "minecraft:frog_variant",
    "instrument": "minecraft:instrument",
    "jukebox_song": "minecraft:jukebox_song",
    "painting_variant": "minecraft:painting_variant",
    "pig_sound_variant": "minecraft:pig_sound_variant",
    "pig_variant": "minecraft:pig_variant",
    "sulfur_cube_archetype": "minecraft:sulfur_cube_archetype",
    "test_environment": "minecraft:test_environment",
    "test_instance": "minecraft:test_instance",
    "timeline": "minecraft:timeline",
    "trim_material": "minecraft:trim_material",
    "trim_pattern": "minecraft:trim_pattern",
    "wolf_sound_variant": "minecraft:wolf_sound_variant",
    "wolf_variant": "minecraft:wolf_variant",
    "world_clock": "minecraft:world_clock",
    "zombie_nautilus_variant": "minecraft:zombie_nautilus_variant",
    "worldgen/biome": "minecraft:worldgen/biome",
}

# Tag folder under data/minecraft/tags → registry used for Update Tags
# Includes built-ins that synchronized definitions reference.
TAG_FOLDERS: dict[str, str] = {
    **{folder: reg for folder, reg in SYNC_FOLDERS.items()},
    "block": "minecraft:block",
    "item": "minecraft:item",
    "fluid": "minecraft:fluid",
    "entity_type": "minecraft:entity_type",
    "game_event": "minecraft:game_event",
    "instrument": "minecraft:instrument",
    "point_of_interest_type": "minecraft:point_of_interest_type",
    "painting_variant": "minecraft:painting_variant",
    "banner_pattern": "minecraft:banner_pattern",
    "cat_variant": "minecraft:cat_variant",
    "frog_variant": "minecraft:frog_variant",
    "wolf_variant": "minecraft:wolf_variant",
    "pig_variant": "minecraft:pig_variant",
    "cow_variant": "minecraft:cow_variant",
    "chicken_variant": "minecraft:chicken_variant",
    "damage_type": "minecraft:damage_type",
    "enchantment": "minecraft:enchantment",
    "instrument": "minecraft:instrument",
    "jukebox_song": "minecraft:jukebox_song",
    "trim_material": "minecraft:trim_material",
    "trim_pattern": "minecraft:trim_pattern",
    "dialog": "minecraft:dialog",
    "timeline": "minecraft:timeline",
    "world_clock": "minecraft:world_clock",
    "worldgen/biome": "minecraft:worldgen/biome",
    "worldgen/structure": "minecraft:worldgen/structure",
    "worldgen/world_preset": "minecraft:worldgen/world_preset",
    "worldgen/flat_level_generator_preset": "minecraft:worldgen/flat_level_generator_preset",
    "worldgen/structure_set": "minecraft:worldgen/structure_set",
    "worldgen/configured_feature": "minecraft:worldgen/configured_feature",
    "worldgen/placed_feature": "minecraft:worldgen/placed_feature",
    "worldgen/processor_list": "minecraft:worldgen/processor_list",
    "worldgen/template_pool": "minecraft:worldgen/template_pool",
    "worldgen/noise": "minecraft:worldgen/noise",
    "worldgen/density_function": "minecraft:worldgen/density_function",
    "worldgen/biome": "minecraft:worldgen/biome",
}


def list_entries(z: zipfile.ZipFile, folder: str) -> list[str]:
    prefix = f"data/minecraft/{folder}/"
    out: list[str] = []
    for name in z.namelist():
        if name.startswith(prefix) and name.endswith(".json"):
            rest = name[len(prefix) : -5]
            if "/" not in rest:
                out.append(rest)
    out.sort()
    return out


def prioritize(entries: list[str], first: str) -> list[str]:
    if first in entries:
        entries = [e for e in entries if e != first]
        entries.insert(0, first)
    return entries


def load_builtin_ids(report: dict) -> dict[str, dict[str, int]]:
    """registry_id → { path_without_ns_or_full_id → protocol_id }"""
    result: dict[str, dict[str, int]] = {}
    for reg_id, body in report.items():
        entries = body.get("entries", {})
        mapping: dict[str, int] = {}
        for full_name, meta in entries.items():
            pid = meta["protocol_id"]
            mapping[full_name] = pid
            if full_name.startswith("minecraft:"):
                mapping[full_name[len("minecraft:") :]] = pid
        result[reg_id] = mapping
    return result


def parse_tag_values(raw_values: list) -> tuple[list[str], list[str]]:
    """Return (direct_ids, nested_tag_refs) from a tag JSON values array."""
    directs: list[str] = []
    nested: list[str] = []
    for v in raw_values:
        if isinstance(v, dict):
            v = v.get("id", "")
            # optional required flag ignored
        if not isinstance(v, str) or not v:
            continue
        if v.startswith("#"):
            nested.append(v[1:])
        else:
            directs.append(v)
    return directs, nested


def expand_tag(
    reg: str,
    tag_name: str,
    graph: dict[str, dict[str, tuple[list[str], list[str]]]],
    cache: dict[tuple[str, str], list[str]],
    stack: set[tuple[str, str]] | None = None,
) -> list[str]:
    key = (reg, tag_name)
    if key in cache:
        return cache[key]
    if stack is None:
        stack = set()
    if key in stack:
        return []
    stack.add(key)
    directs, nested = graph.get(reg, {}).get(tag_name, ([], []))
    out: list[str] = list(directs)
    for n in nested:
        # nested tags are same-registry usually as minecraft:foo
        nested_name = n if n.startswith("minecraft:") or ":" in n else f"minecraft:{n}"
        # also try path-only key
        out.extend(expand_tag(reg, nested_name, graph, cache, stack))
        if not nested_name.startswith("minecraft:"):
            out.extend(expand_tag(reg, f"minecraft:{nested_name}", graph, cache, stack))
    # de-dupe
    seen: set[str] = set()
    uniq: list[str] = []
    for x in out:
        if x not in seen:
            seen.add(x)
            uniq.append(x)
    cache[key] = uniq
    stack.remove(key)
    return uniq


def to_path(entry_id: str) -> str:
    if entry_id.startswith("minecraft:"):
        return entry_id[len("minecraft:") :]
    if ":" in entry_id:
        return entry_id  # keep foreign ns as full for lookup attempts
    return entry_id


def resolve_id(
    reg: str,
    entry_id: str,
    sync_index: dict[str, dict[str, int]],
    builtin: dict[str, dict[str, int]],
) -> int | None:
    path = to_path(entry_id)
    full = entry_id if ":" in entry_id else f"minecraft:{entry_id}"
    if reg in sync_index:
        m = sync_index[reg]
        if path in m:
            return m[path]
        if full in m:
            return m[full]
        if full.startswith("minecraft:") and full[len("minecraft:") :] in m:
            return m[full[len("minecraft:") :]]
    if reg in builtin:
        m = builtin[reg]
        if full in m:
            return m[full]
        if path in m:
            return m[path]
    return None


def rust_str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def main() -> None:
    report = json.loads(REG_REPORT.read_text(encoding="utf-8"))
    builtin = load_builtin_ids(report)

    registries: dict[str, list[str]] = {}
    with zipfile.ZipFile(JAR) as z:
        for folder, reg_id in SYNC_FOLDERS.items():
            entries = list_entries(z, folder)
            if reg_id == "minecraft:worldgen/biome":
                entries = prioritize(entries, "plains")
            if reg_id == "minecraft:dimension_type":
                entries = prioritize(entries, "overworld")
            registries[reg_id] = entries

        # tag graph: reg -> tag_name(minecraft:...) -> (directs, nested)
        tag_graph: dict[str, dict[str, tuple[list[str], list[str]]]] = defaultdict(dict)
        for name in z.namelist():
            if not name.startswith("data/minecraft/tags/") or not name.endswith(".json"):
                continue
            rel = name[len("data/minecraft/tags/") : -5]
            matched = None
            for folder in sorted(TAG_FOLDERS.keys(), key=len, reverse=True):
                if rel == folder or rel.startswith(folder + "/"):
                    # only if the rest is the tag path
                    if rel == folder:
                        continue
                    matched = folder
                    break
            if not matched:
                continue
            reg = TAG_FOLDERS[matched]
            tag_path = rel[len(matched) + 1 :]
            try:
                data = json.loads(z.read(name))
            except Exception:
                continue
            directs, nested = parse_tag_values(data.get("values", []))
            tag_graph[reg][f"minecraft:{tag_path}"] = (directs, nested)

    # index maps for synchronized registries (path -> index)
    sync_index: dict[str, dict[str, int]] = {
        reg: {path: i for i, path in enumerate(entries)} for reg, entries in registries.items()
    }

    # expand all tags we have for registries we can resolve (sync or builtin)
    resolvable = set(sync_index) | set(builtin)
    expanded: dict[str, dict[str, list[int]]] = {}
    cache: dict[tuple[str, str], list[str]] = {}
    missing = 0
    total_tags = 0
    for reg, tags in tag_graph.items():
        if reg not in resolvable:
            continue
        expanded[reg] = {}
        for tag_name in tags:
            total_tags += 1
            ids_str = expand_tag(reg, tag_name, tag_graph, cache)
            ids: list[int] = []
            for e in ids_str:
                rid = resolve_id(reg, e, sync_index, builtin)
                if rid is None:
                    missing += 1
                    continue
                ids.append(rid)
            # de-dupe ints preserving order
            seen: set[int] = set()
            uniq: list[int] = []
            for i in ids:
                if i not in seen:
                    seen.add(i)
                    uniq.append(i)
            expanded[reg][tag_name] = uniq

    print(f"registries={len(registries)} tags={total_tags} missing_entries={missing}")

    # emit Rust
    priority = [
        "minecraft:dimension_type",
        "minecraft:worldgen/biome",
        "minecraft:world_clock",
        "minecraft:timeline",
        "minecraft:damage_type",
        "minecraft:banner_pattern",
        "minecraft:instrument",
        "minecraft:jukebox_song",
        "minecraft:trim_material",
        "minecraft:trim_pattern",
        "minecraft:painting_variant",
        "minecraft:sulfur_cube_archetype",
        "minecraft:chat_type",
        "minecraft:enchantment",
        "minecraft:dialog",
        "minecraft:test_environment",
        "minecraft:test_instance",
    ]
    ordered = priority + sorted(k for k in registries if k not in priority)

    lines: list[str] = []
    lines.append("// @generated by tools/mc-ref/gen_join_data.py — DO NOT EDIT.")
    lines.append("// Vanilla 26.2 synchronized registries + fully expanded Update Tags.")
    lines.append("")
    lines.append("/// Known pack the vanilla client ships (`SharedConstants` id = \"26.2\").")
    lines.append('pub const CORE_KNOWN_PACK: (&str, &str, &str) = ("minecraft", "core", "26.2");')
    lines.append("")
    lines.append("/// Synchronized registry id → entry paths (minecraft namespace).")
    lines.append("/// Entry order defines numeric network IDs starting at 0.")
    lines.append("pub const VANILLA_REGISTRIES: &[(&str, &[&str])] = &[")
    for reg in ordered:
        lines.append(f"    ({rust_str(reg)}, &[")
        for e in registries[reg]:
            lines.append(f"        {rust_str(e)},")
        lines.append("    ]),")
    lines.append("];")
    lines.append("")
    lines.append("/// Full Update Tags payload data: registry → (tag name → entry numeric IDs).")
    lines.append("#[allow(clippy::type_complexity)]")
    lines.append("pub const VANILLA_TAGS: &[(&str, &[(&str, &[i32])])] = &[")
    for reg in sorted(expanded.keys()):
        tags = expanded[reg]
        if not tags:
            continue
        lines.append(f"    ({rust_str(reg)}, &[")
        for tag_name in sorted(tags.keys()):
            ids = tags[tag_name]
            id_list = ", ".join(str(i) for i in ids)
            lines.append(f"        ({rust_str(tag_name)}, &[{id_list}] as &[i32]),")
        lines.append("    ]),")
    lines.append("];")
    lines.append("")

    OUT_RS.parent.mkdir(parents=True, exist_ok=True)
    OUT_RS.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {OUT_RS} ({OUT_RS.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
