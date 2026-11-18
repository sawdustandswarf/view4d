# AGENTS.md

Guidance for AI coding assistants (and people) working in this repository.

## What this is

view4d shows 3D scenes that change over time in a pane of termmd (https://github.com/sawdustandswarf/termmd). It can show a model that follows its file (STL, OBJ, 3MF, primlib `.prim`) or an orrery. `README.md` covers usage and keys.

## Commands

```sh
cargo build --workspace
cargo test --workspace
cargo test -p view4d-prim                    # the .prim reader, against primlib's fixtures
cargo clippy --workspace --all-targets
cargo fmt --all
cargo run -- part.stl --png part.png         # one frame to a PNG; works outside termmd
```

## Dependencies on other repositories

- **termmd's crates** (`termmd-surface`, `termmd-vulkan`, `termmd-deflate`, `termmd-font`) are **git dependencies** in the workspace `Cargo.toml`, and `Cargo.lock` pins the commit. To work on them side by side, clone termmd beside this repository and put a `[patch."https://github.com/sawdustandswarf/termmd"]` table with `path` entries in a `.cargo/config.toml` *above* both repositories, never in this one.
- **primlib** (https://github.com/sawdustandswarf/primlib) owns the `.prim` format (`docs/FORMAT.md`) and the expression language (`docs/EXPRESSIONS.md`). `view4d-prim` must read exactly what primlib writes. It is tested against fixtures primlib generates into `prim/tests/data/`. Don't edit those by hand: with primlib cloned beside this repository, regenerate them with `python3 ../primlib/tools/view4d_fixtures.py prim/tests/data`.

## Layout

| Crate | Folder | What |
|---|---|---|
| `view4d-scene` | `scene/` | maths, camera, rasterizer, edges, text, PNG, the `Scene` trait |
| `view4d-model` | `model/` | models from files: STL, OBJ + MTL, 3MF (own zip and XML readers), `.prim` |
| `view4d-orrery` | `orrery/` | planet positions from approximate elements |
| `view4d-prim` | `prim/` | `.prim` files: JSON reader, expressions, operations as closed meshes (BSP booleans), recipes and assemblies |
| `view4d` | `app/` | the binary: arguments, the termmd connection, the loop, file watching |

## Docs map

| Read | For |
|---|---|
| `docs/GLOSSARY.md` | the meaning of every term; **use these words** |
| `docs/DESIGN.md` | architecture; read before a structural change |
| `docs/PLAN.md` | milestones: what's done and what's next |
| `TODO.md` | smaller open work, as a checklist |
| primlib's `docs/FORMAT.md`, `docs/EXPRESSIONS.md` | the file format and language `view4d-prim` implements |
| termmd's `docs/SURFACE.md` | the embedded-app protocol |

## Rules

- **No third-party crates.** Only std and termmd's crates.
- **`view4d-prim` follows primlib.** When the two disagree, primlib's reference (`primlib/expr.py`, `recipe.py`'s `layout`) is right. Change view4d to match it, not the fixtures.
- **Unsafe:** every `unsafe` block carries a `// SAFETY:` comment (clippy `undocumented_unsafe_blocks`).
- **`app/src/gpu.rs` is shared in spirit with specan.** Keep the two alike; with a third app it moves into `termmd-vulkan`.
- **Keep docs current:** a change to keys or behaviour updates `README.md` and ticks `docs/PLAN.md`.
- **Commits:** only when asked.
