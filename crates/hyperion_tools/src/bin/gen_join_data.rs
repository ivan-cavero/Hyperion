//! Build join-time registry + tag tables from vanilla 26.2 datapack + reports.
//!
//! ```text
//! cargo run -p hyperion_tools --bin gen-join-data
//! ```
//!
//! Inputs:
//! - `tools/mc-ref/server-inner-26.2.jar`
//! - `tools/mc-ref/datagen/generated/reports/registries.json`
//!
//! Output:
//! - `crates/hyperion_server/src/network/join_data/generated.rs`

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::process::ExitCode;

use hyperion_tools::{
    SYNC_FOLDERS, list_entries, open_server_jar, prioritize_registry, reports_dir,
    rust_string_literal, workspace_root, write_text,
};
use serde_json::Value;
use zip::ZipArchive;

/// Tag folder under `data/minecraft/tags/` → registry for Update Tags.
const TAG_FOLDERS: &[(&str, &str)] = &[
    ("banner_pattern", "minecraft:banner_pattern"),
    ("block", "minecraft:block"),
    ("cat_variant", "minecraft:cat_variant"),
    ("chicken_variant", "minecraft:chicken_variant"),
    ("cow_variant", "minecraft:cow_variant"),
    ("damage_type", "minecraft:damage_type"),
    ("dialog", "minecraft:dialog"),
    ("enchantment", "minecraft:enchantment"),
    ("entity_type", "minecraft:entity_type"),
    ("fluid", "minecraft:fluid"),
    ("frog_variant", "minecraft:frog_variant"),
    ("game_event", "minecraft:game_event"),
    ("instrument", "minecraft:instrument"),
    ("item", "minecraft:item"),
    ("jukebox_song", "minecraft:jukebox_song"),
    ("painting_variant", "minecraft:painting_variant"),
    ("pig_variant", "minecraft:pig_variant"),
    ("point_of_interest_type", "minecraft:point_of_interest_type"),
    ("timeline", "minecraft:timeline"),
    ("trim_material", "minecraft:trim_material"),
    ("trim_pattern", "minecraft:trim_pattern"),
    ("wolf_variant", "minecraft:wolf_variant"),
    ("world_clock", "minecraft:world_clock"),
    ("worldgen/biome", "minecraft:worldgen/biome"),
    (
        "worldgen/configured_feature",
        "minecraft:worldgen/configured_feature",
    ),
    (
        "worldgen/density_function",
        "minecraft:worldgen/density_function",
    ),
    (
        "worldgen/flat_level_generator_preset",
        "minecraft:worldgen/flat_level_generator_preset",
    ),
    ("worldgen/noise", "minecraft:worldgen/noise"),
    (
        "worldgen/placed_feature",
        "minecraft:worldgen/placed_feature",
    ),
    (
        "worldgen/processor_list",
        "minecraft:worldgen/processor_list",
    ),
    ("worldgen/structure", "minecraft:worldgen/structure"),
    ("worldgen/structure_set", "minecraft:worldgen/structure_set"),
    ("worldgen/template_pool", "minecraft:worldgen/template_pool"),
    ("worldgen/world_preset", "minecraft:worldgen/world_preset"),
    // Also cover sync-only folders that may have tags.
    ("cat_sound_variant", "minecraft:cat_sound_variant"),
    ("chicken_sound_variant", "minecraft:chicken_sound_variant"),
    ("cow_sound_variant", "minecraft:cow_sound_variant"),
    ("pig_sound_variant", "minecraft:pig_sound_variant"),
    ("sulfur_cube_archetype", "minecraft:sulfur_cube_archetype"),
    ("test_environment", "minecraft:test_environment"),
    ("test_instance", "minecraft:test_instance"),
    ("wolf_sound_variant", "minecraft:wolf_sound_variant"),
    (
        "zombie_nautilus_variant",
        "minecraft:zombie_nautilus_variant",
    ),
    ("chat_type", "minecraft:chat_type"),
    ("dimension_type", "minecraft:dimension_type"),
];

const REGISTRY_PRIORITY: &[&str] = &[
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
];

type TagNode = (Vec<String>, Vec<String>);
type TagGraph = HashMap<String, HashMap<String, TagNode>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gen-join-data: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = workspace_root()?;
    let reg_report_path = reports_dir(&root).join("registries.json");
    if !reg_report_path.is_file() {
        return Err(format!(
            "missing {}\nRun Mojang data generators first.",
            reg_report_path.display()
        ));
    }
    let report_raw = fs::read_to_string(&reg_report_path)
        .map_err(|e| format!("read {}: {e}", reg_report_path.display()))?;
    let report: Value =
        serde_json::from_str(&report_raw).map_err(|e| format!("parse registries.json: {e}"))?;
    let builtin = load_builtin_ids(&report)?;

    let mut archive = open_server_jar(&root)?;
    let mut registries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &(folder, reg_id) in SYNC_FOLDERS {
        let mut entries = list_entries(&mut archive, folder)?;
        prioritize_registry(reg_id, &mut entries);
        registries.insert(reg_id.to_owned(), entries);
    }

    let tag_graph = load_tag_graph(&mut archive)?;

    let sync_index: HashMap<String, HashMap<String, i32>> = registries
        .iter()
        .map(|(reg, entries)| {
            let map = entries
                .iter()
                .enumerate()
                .map(|(i, path)| (path.clone(), i as i32))
                .collect();
            (reg.clone(), map)
        })
        .collect();

    let resolvable: HashSet<String> = sync_index
        .keys()
        .cloned()
        .chain(builtin.keys().cloned())
        .collect();

    let mut expanded: BTreeMap<String, BTreeMap<String, Vec<i32>>> = BTreeMap::new();
    let mut cache: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut missing = 0usize;
    let mut total_tags = 0usize;

    for (reg, tags) in &tag_graph {
        if !resolvable.contains(reg) {
            continue;
        }
        let mut reg_tags = BTreeMap::new();
        for tag_name in tags.keys() {
            total_tags += 1;
            let ids_str = expand_tag(reg, tag_name, &tag_graph, &mut cache, &mut HashSet::new());
            let mut ids = Vec::new();
            for e in ids_str {
                match resolve_id(reg, &e, &sync_index, &builtin) {
                    Some(rid) => ids.push(rid),
                    None => missing += 1,
                }
            }
            let mut seen = HashSet::new();
            ids.retain(|i| seen.insert(*i));
            reg_tags.insert(tag_name.clone(), ids);
        }
        expanded.insert(reg.clone(), reg_tags);
    }

    println!(
        "registries={} tags={total_tags} missing_entries={missing}",
        registries.len()
    );

    let mut ordered: Vec<String> = REGISTRY_PRIORITY
        .iter()
        .filter(|k| registries.contains_key(**k))
        .map(|s| (*s).to_owned())
        .collect();
    let mut rest: Vec<String> = registries
        .keys()
        .filter(|k| !REGISTRY_PRIORITY.contains(&k.as_str()))
        .cloned()
        .collect();
    rest.sort();
    ordered.extend(rest);

    let mut lines = vec![
        "// @generated by cargo run -p hyperion_tools --bin gen-join-data — DO NOT EDIT."
            .to_owned(),
        "// Vanilla 26.2 synchronized registries + fully expanded Update Tags.".to_owned(),
        String::new(),
        "/// Known pack the vanilla client ships (`SharedConstants` id = \"26.2\").".to_owned(),
        "pub const CORE_KNOWN_PACK: (&str, &str, &str) = (\"minecraft\", \"core\", \"26.2\");"
            .to_owned(),
        String::new(),
        "/// Synchronized registry id → entry paths (minecraft namespace).".to_owned(),
        "/// Entry order defines numeric network IDs starting at 0.".to_owned(),
        "pub const VANILLA_REGISTRIES: &[(&str, &[&str])] = &[".to_owned(),
    ];
    for reg in &ordered {
        lines.push(format!("    ({}, &[", rust_string_literal(reg)));
        for e in &registries[reg] {
            lines.push(format!("        {},", rust_string_literal(e)));
        }
        lines.push("    ]),".to_owned());
    }
    lines.push("];".to_owned());
    lines.push(String::new());
    lines.push(
        "/// Full Update Tags payload data: registry → (tag name → entry numeric IDs).".to_owned(),
    );
    lines.push("#[allow(clippy::type_complexity)]".to_owned());
    lines.push("pub const VANILLA_TAGS: &[(&str, &[(&str, &[i32])])] = &[".to_owned());
    for (reg, tags) in &expanded {
        if tags.is_empty() {
            continue;
        }
        lines.push(format!("    ({}, &[", rust_string_literal(reg)));
        for (tag_name, ids) in tags {
            let id_list = ids
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "        ({}, &[{id_list}] as &[i32]),",
                rust_string_literal(tag_name)
            ));
        }
        lines.push("    ]),".to_owned());
    }
    lines.push("];".to_owned());
    lines.push(String::new());

    let out = root.join("crates/hyperion_server/src/network/join_data/generated.rs");
    let body = lines.join("\n");
    write_text(&out, &body)?;
    println!("wrote {} ({} bytes)", out.display(), body.len());
    Ok(())
}

fn load_builtin_ids(report: &Value) -> Result<HashMap<String, HashMap<String, i32>>, String> {
    let obj = report
        .as_object()
        .ok_or_else(|| "registries.json root must be an object".to_owned())?;
    let mut result = HashMap::new();
    for (reg_id, body) in obj {
        let entries = body
            .get("entries")
            .and_then(|e| e.as_object())
            .ok_or_else(|| format!("registry {reg_id} missing entries"))?;
        let mut mapping = HashMap::new();
        for (full_name, meta) in entries {
            let pid = meta
                .get("protocol_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| format!("missing protocol_id for {full_name}"))?
                as i32;
            mapping.insert(full_name.clone(), pid);
            if let Some(rest) = full_name.strip_prefix("minecraft:") {
                mapping.insert(rest.to_owned(), pid);
            }
        }
        result.insert(reg_id.clone(), mapping);
    }
    Ok(result)
}

fn load_tag_graph<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<TagGraph, String> {
    let mut folders: Vec<&str> = TAG_FOLDERS.iter().map(|(f, _)| *f).collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.len()));

    let mut graph: TagGraph = HashMap::new();
    let names: Vec<String> = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_owned())
                .map_err(|e| format!("zip entry {i}: {e}"))
        })
        .collect::<Result<_, _>>()?;

    for name in names {
        if !name.starts_with("data/minecraft/tags/") || !name.ends_with(".json") {
            continue;
        }
        let rel = &name["data/minecraft/tags/".len()..name.len() - 5];
        let mut matched: Option<&str> = None;
        for folder in &folders {
            if rel == *folder {
                continue;
            }
            if rel.starts_with(&format!("{folder}/")) {
                matched = Some(folder);
                break;
            }
        }
        let Some(matched) = matched else {
            continue;
        };
        let reg = TAG_FOLDERS
            .iter()
            .find(|(f, _)| *f == matched)
            .map(|(_, r)| *r)
            .expect("folder in map");
        let tag_path = &rel[matched.len() + 1..];
        let mut file = archive
            .by_name(&name)
            .map_err(|e| format!("open {name}: {e}"))?;
        let mut raw = String::new();
        if file.read_to_string(&mut raw).is_err() {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let values = data
            .get("values")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let (directs, nested) = parse_tag_values(&values);
        graph
            .entry(reg.to_owned())
            .or_default()
            .insert(format!("minecraft:{tag_path}"), (directs, nested));
    }
    Ok(graph)
}

fn parse_tag_values(raw_values: &[Value]) -> (Vec<String>, Vec<String>) {
    let mut directs = Vec::new();
    let mut nested = Vec::new();
    for v in raw_values {
        let s = match v {
            Value::String(s) => s.as_str(),
            Value::Object(map) => map.get("id").and_then(|x| x.as_str()).unwrap_or(""),
            _ => "",
        };
        if s.is_empty() {
            continue;
        }
        if let Some(rest) = s.strip_prefix('#') {
            nested.push(rest.to_owned());
        } else {
            directs.push(s.to_owned());
        }
    }
    (directs, nested)
}

fn expand_tag(
    reg: &str,
    tag_name: &str,
    graph: &TagGraph,
    cache: &mut HashMap<(String, String), Vec<String>>,
    stack: &mut HashSet<(String, String)>,
) -> Vec<String> {
    let key = (reg.to_owned(), tag_name.to_owned());
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    if !stack.insert(key.clone()) {
        return Vec::new();
    }
    let (directs, nested) = graph
        .get(reg)
        .and_then(|t| t.get(tag_name))
        .cloned()
        .unwrap_or_default();
    let mut out = directs;
    for n in nested {
        let nested_name = if n.contains(':') {
            n.clone()
        } else {
            format!("minecraft:{n}")
        };
        out.extend(expand_tag(reg, &nested_name, graph, cache, stack));
        if !nested_name.starts_with("minecraft:") {
            out.extend(expand_tag(
                reg,
                &format!("minecraft:{nested_name}"),
                graph,
                cache,
                stack,
            ));
        }
    }
    let mut seen = HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    stack.remove(&key);
    cache.insert(key, out.clone());
    out
}

fn to_path(entry_id: &str) -> String {
    entry_id
        .strip_prefix("minecraft:")
        .unwrap_or(entry_id)
        .to_owned()
}

fn resolve_id(
    reg: &str,
    entry_id: &str,
    sync_index: &HashMap<String, HashMap<String, i32>>,
    builtin: &HashMap<String, HashMap<String, i32>>,
) -> Option<i32> {
    let path = to_path(entry_id);
    let full = if entry_id.contains(':') {
        entry_id.to_owned()
    } else {
        format!("minecraft:{entry_id}")
    };
    if let Some(m) = sync_index.get(reg) {
        if let Some(id) = m.get(&path) {
            return Some(*id);
        }
        if let Some(id) = m.get(&full) {
            return Some(*id);
        }
        if let Some(rest) = full.strip_prefix("minecraft:")
            && let Some(id) = m.get(rest)
        {
            return Some(*id);
        }
    }
    if let Some(m) = builtin.get(reg) {
        if let Some(id) = m.get(&full) {
            return Some(*id);
        }
        if let Some(id) = m.get(&path) {
            return Some(*id);
        }
    }
    None
}
