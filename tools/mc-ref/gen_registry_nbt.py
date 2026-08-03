#!/usr/bin/env python3
"""Convert vanilla datapack JSON → network NBT bytes for every synchronized registry entry."""
from __future__ import annotations

import json
import struct
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
JAR = Path(__file__).with_name("server-inner-26.2.jar")
OUT_BIN = ROOT / "crates" / "hyperion_server" / "src" / "network" / "join_data" / "registry_nbt.bin"
OUT_IDX = ROOT / "crates" / "hyperion_server" / "src" / "network" / "join_data" / "nbt_index.rs"

# Must match gen_join_data.py SYNC_FOLDERS + prioritization
SYNC = {
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

TAG_END = 0
TAG_BYTE = 1
TAG_SHORT = 2
TAG_INT = 3
TAG_LONG = 4
TAG_FLOAT = 5
TAG_DOUBLE = 6
TAG_BYTE_ARRAY = 7
TAG_STRING = 8
TAG_LIST = 9
TAG_COMPOUND = 10
TAG_INT_ARRAY = 11
TAG_LONG_ARRAY = 12


def write_string(buf: bytearray, s: str) -> None:
    raw = s.encode("utf-8")
    if len(raw) > 65535:
        raise ValueError(f"string too long: {len(raw)}")
    buf.extend(struct.pack(">H", len(raw)))
    buf.extend(raw)


def tag_type_of(value) -> int:
    if isinstance(value, bool):
        return TAG_BYTE
    if isinstance(value, int):
        if -2147483648 <= value <= 2147483647:
            return TAG_INT
        return TAG_LONG
    if isinstance(value, float):
        return TAG_DOUBLE
    if isinstance(value, str):
        return TAG_STRING
    if isinstance(value, list):
        return TAG_LIST
    if isinstance(value, dict):
        return TAG_COMPOUND
    raise TypeError(f"unsupported JSON type: {type(value)}")


def write_payload(buf: bytearray, value) -> None:
    if isinstance(value, bool):
        buf.append(1 if value else 0)
    elif isinstance(value, int):
        if -2147483648 <= value <= 2147483647:
            buf.extend(struct.pack(">i", value))
        else:
            buf.extend(struct.pack(">q", value))
    elif isinstance(value, float):
        buf.extend(struct.pack(">d", value))
    elif isinstance(value, str):
        write_string(buf, value)
    elif isinstance(value, list):
        if not value:
            buf.append(TAG_END)
            buf.extend(struct.pack(">i", 0))
            return
        # Minecraft requires homogeneous lists; coerce by first element type
        et = tag_type_of(value[0])
        buf.append(et)
        buf.extend(struct.pack(">i", len(value)))
        for item in value:
            # light coercion for ints/floats mixed
            if et == TAG_DOUBLE and isinstance(item, int) and not isinstance(item, bool):
                item = float(item)
            if et == TAG_INT and isinstance(item, float) and item.is_integer():
                item = int(item)
            write_payload(buf, item)
    elif isinstance(value, dict):
        for k, v in value.items():
            if v is None:
                continue
            buf.append(tag_type_of(v))
            write_string(buf, k)
            write_payload(buf, v)
        buf.append(TAG_END)
    else:
        raise TypeError(type(value))


def encode_root_compound(obj: dict) -> bytes:
    """Network NBT: root compound with no name."""
    buf = bytearray()
    buf.append(TAG_COMPOUND)
    for k, v in obj.items():
        if v is None:
            continue
        buf.append(tag_type_of(v))
        write_string(buf, k)
        write_payload(buf, v)
    buf.append(TAG_END)
    return bytes(buf)


def list_entries(z: zipfile.ZipFile, folder: str) -> list[str]:
    prefix = f"data/minecraft/{folder}/"
    out = []
    for name in z.namelist():
        if name.startswith(prefix) and name.endswith(".json"):
            rest = name[len(prefix):-5]
            if "/" not in rest:
                out.append(rest)
    out.sort()
    return out


def prioritize(entries: list[str], first: str) -> list[str]:
    if first in entries:
        entries = [e for e in entries if e != first]
        entries.insert(0, first)
    return entries


def main() -> None:
    # Build blob: sequence of (registry_id, entry_path, nbt_bytes)
    # File format:
    #   u32be count
    #   repeated:
    #     u16be reg_len + reg_utf8
    #     u16be path_len + path_utf8
    #     u32be nbt_len + nbt
    records: list[tuple[str, str, bytes]] = []
    with zipfile.ZipFile(JAR) as z:
        for folder, reg_id in SYNC.items():
            entries = list_entries(z, folder)
            if reg_id == "minecraft:worldgen/biome":
                entries = prioritize(entries, "plains")
            if reg_id == "minecraft:dimension_type":
                entries = prioritize(entries, "overworld")
            for path in entries:
                jname = f"data/minecraft/{folder}/{path}.json"
                obj = json.loads(z.read(jname))
                if not isinstance(obj, dict):
                    raise SystemExit(f"expected object in {jname}")
                nbt = encode_root_compound(obj)
                records.append((reg_id, path, nbt))

    blob = bytearray()
    blob.extend(struct.pack(">I", len(records)))
    for reg_id, path, nbt in records:
        rb = reg_id.encode()
        pb = path.encode()
        blob.extend(struct.pack(">H", len(rb)))
        blob.extend(rb)
        blob.extend(struct.pack(">H", len(pb)))
        blob.extend(pb)
        blob.extend(struct.pack(">I", len(nbt)))
        blob.extend(nbt)

    OUT_BIN.parent.mkdir(parents=True, exist_ok=True)
    OUT_BIN.write_bytes(blob)
    print(f"wrote {OUT_BIN} records={len(records)} bytes={len(blob)}")

    # smoke: overworld nbt starts with 0x0a
    for reg_id, path, nbt in records:
        if reg_id.endswith("dimension_type") and path == "overworld":
            assert nbt[0] == 0x0A
            print("overworld nbt len", len(nbt))
            break


if __name__ == "__main__":
    main()
