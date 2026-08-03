# mc-ref — vanilla 26.2 join data generators

Scripts that rebuild the embedded Configuration data used by Hyperion.

## Prerequisites

- Java 21+ (for the Mojang data generator)
- Python 3.10+
- Vanilla `server.jar` for **26.2**

## Setup (once per machine)

```powershell
# 1) Download server.jar 26.2 into this folder as server-26.2.jar
# 2) Extract the inner jar from the bundler:
#    META-INF/versions/26.2/server-26.2.jar → server-inner-26.2.jar

java "-DbundlerMainClass=net.minecraft.data.Main" `
  -jar server-26.2.jar `
  --reports --output datagen/generated
```

## Regenerate embedded assets

```powershell
python tools/mc-ref/gen_join_data.py
python tools/mc-ref/gen_registry_nbt.py
```

Writes into `crates/hyperion_server/src/network/join_data/`:

| File | Purpose |
|---|---|
| `generated.rs` | Known pack id, registry entry lists, full Update Tags |
| `registry_nbt.bin` | Fallback network NBT for every synchronized entry |

Jars, `datagen/`, `classes/` and extracts are gitignored.
