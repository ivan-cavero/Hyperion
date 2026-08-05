//! Convert vanilla datapack JSON → packed network NBT for every sync registry entry.
//!
//! ```text
//! cargo run -p hyperion_tools --bin gen-registry-nbt
//! ```
//!
//! Output: `crates/hyperion_server/src/network/join_data/registry_nbt.bin`
//!
//! Blob format (big-endian):
//! ```text
//! u32 count
//! repeated:
//!   u16 reg_len + reg_utf8
//!   u16 path_len + path_utf8
//!   u32 nbt_len + nbt bytes (root compound, nameless)
//! ```

use std::io::Read;
use std::process::ExitCode;

use hyperion_protocol::{NbtTag, encode_compound_tag};
use hyperion_tools::{
    SYNC_FOLDERS, list_entries, open_server_jar, prioritize_registry, workspace_root, write_bytes,
};
use serde_json::Value;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gen-registry-nbt: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = workspace_root()?;
    let mut archive = open_server_jar(&root)?;
    let mut records: Vec<(String, String, Vec<u8>)> = Vec::new();

    for &(folder, reg_id) in SYNC_FOLDERS {
        let mut entries = list_entries(&mut archive, folder)?;
        prioritize_registry(reg_id, &mut entries);
        for path in entries {
            let jname = format!("data/minecraft/{folder}/{path}.json");
            let mut file = archive
                .by_name(&jname)
                .map_err(|e| format!("open {jname}: {e}"))?;
            let mut raw = String::new();
            file.read_to_string(&mut raw)
                .map_err(|e| format!("read {jname}: {e}"))?;
            let obj: Value =
                serde_json::from_str(&raw).map_err(|e| format!("parse {jname}: {e}"))?;
            let Value::Object(map) = obj else {
                return Err(format!("expected object in {jname}"));
            };
            let nbt = json_object_to_network_nbt(&map).map_err(|e| format!("nbt {jname}: {e}"))?;
            records.push((reg_id.to_owned(), path, nbt));
        }
    }

    let mut blob = Vec::new();
    blob.extend_from_slice(&(records.len() as u32).to_be_bytes());
    for (reg_id, path, nbt) in &records {
        let rb = reg_id.as_bytes();
        let pb = path.as_bytes();
        if rb.len() > u16::MAX as usize || pb.len() > u16::MAX as usize {
            return Err("registry or path string too long".to_owned());
        }
        blob.extend_from_slice(&(rb.len() as u16).to_be_bytes());
        blob.extend_from_slice(rb);
        blob.extend_from_slice(&(pb.len() as u16).to_be_bytes());
        blob.extend_from_slice(pb);
        blob.extend_from_slice(&(nbt.len() as u32).to_be_bytes());
        blob.extend_from_slice(nbt);
    }

    let out = root.join("crates/hyperion_server/src/network/join_data/registry_nbt.bin");
    write_bytes(&out, &blob)?;
    println!(
        "wrote {} records={} bytes={}",
        out.display(),
        records.len(),
        blob.len()
    );

    for (reg_id, path, nbt) in &records {
        if reg_id.ends_with("dimension_type") && path == "overworld" {
            if nbt.first() != Some(&0x0a) {
                return Err("overworld NBT must start with compound tag 0x0a".to_owned());
            }
            println!("overworld nbt len {}", nbt.len());
            break;
        }
    }
    Ok(())
}

fn json_object_to_network_nbt(map: &serde_json::Map<String, Value>) -> Result<Vec<u8>, String> {
    let entries = json_map_to_compound(map)?;
    encode_compound_tag(&entries).map_err(|e| e.to_string())
}

fn json_map_to_compound(
    map: &serde_json::Map<String, Value>,
) -> Result<Vec<(String, NbtTag)>, String> {
    let mut entries = Vec::with_capacity(map.len());
    for (k, v) in map {
        if v.is_null() {
            continue;
        }
        entries.push((k.clone(), json_to_nbt(v)?));
    }
    Ok(entries)
}

fn json_to_nbt(value: &Value) -> Result<NbtTag, String> {
    match value {
        Value::Null => Err("null is not a valid NBT value".to_owned()),
        Value::Bool(b) => Ok(NbtTag::Byte(if *b { 1 } else { 0 })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if (i32::MIN as i64..=i32::MAX as i64).contains(&i) {
                    Ok(NbtTag::Int(i as i32))
                } else {
                    Ok(NbtTag::Long(i))
                }
            } else if let Some(f) = n.as_f64() {
                Ok(NbtTag::Double(f))
            } else {
                Err(format!("unsupported number {n}"))
            }
        }
        Value::String(s) => Ok(NbtTag::String(s.clone())),
        Value::Array(arr) => {
            if arr.is_empty() {
                return Ok(NbtTag::List(Vec::new()));
            }
            let et = tag_type_of(&arr[0])?;
            let mut items = Vec::with_capacity(arr.len());
            for item in arr {
                let coerced = coerce_for_list(item, et)?;
                items.push(coerced);
            }
            Ok(NbtTag::List(items))
        }
        Value::Object(map) => Ok(NbtTag::Compound(json_map_to_compound(map)?)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonNbtKind {
    Byte,
    Int,
    Long,
    Double,
    String,
    List,
    Compound,
}

fn tag_type_of(value: &Value) -> Result<JsonNbtKind, String> {
    match value {
        Value::Bool(_) => Ok(JsonNbtKind::Byte),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if (i32::MIN as i64..=i32::MAX as i64).contains(&i) {
                    Ok(JsonNbtKind::Int)
                } else {
                    Ok(JsonNbtKind::Long)
                }
            } else {
                Ok(JsonNbtKind::Double)
            }
        }
        Value::String(_) => Ok(JsonNbtKind::String),
        Value::Array(_) => Ok(JsonNbtKind::List),
        Value::Object(_) => Ok(JsonNbtKind::Compound),
        Value::Null => Err("null list element".to_owned()),
    }
}

fn coerce_for_list(item: &Value, et: JsonNbtKind) -> Result<NbtTag, String> {
    match (et, item) {
        (JsonNbtKind::Double, Value::Number(n)) if n.as_i64().is_some() => {
            Ok(NbtTag::Double(n.as_i64().unwrap() as f64))
        }
        (JsonNbtKind::Int, Value::Number(n)) if n.as_f64().is_some_and(|f| f.fract() == 0.0) => {
            Ok(NbtTag::Int(n.as_f64().unwrap() as i32))
        }
        _ => json_to_nbt(item),
    }
}
