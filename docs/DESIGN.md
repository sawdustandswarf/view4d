# view4d — Design

A viewer for 3D scenes that change over time, shown in a pane of termmd. Terms
are in [GLOSSARY.md](GLOSSARY.md); the work, step by step, in [PLAN.md](PLAN.md).

## Principles

1. **Our own code**, as in termmd: zero third-party crates. termmd's
   `termmd-surface` and `termmd-vulkan` are ours.
2. **A guest in termmd.** view4d is an embedded app: its own process, drawing
   into a surface termmd composites (termmd's docs/SURFACE.md). termmd knows
   nothing of 3D.
3. **One viewer, many scenes.** Everything that is about looking (the camera,
   input, drawing, the overlay, `--png`) is the [scene viewer](GLOSSARY.md#scene-viewer);
   what is looked at is a [scene](GLOSSARY.md#scene).
4. **Simple first.** Draw on the CPU until that is too slow (PLAN.md M4).

## Workspace

```
scene/   view4d-scene   the viewer's parts: maths, camera, rasterizer, the Scene trait
model/   view4d-model   models from files: STL, OBJ (+ MTL), 3MF (its own zip and XML readers), .prim
orrery/  view4d-orrery  where the planets are: JPL's approximate elements, Kepler
prim/    view4d-prim    primlib's .prim files: its JSON reader, the expression language, the
                        operations built as closed meshes (BSP-tree booleans), recipes and assemblies of them
                        laid out, text from
                        termmd-font (termmd's, a git dependency)
app/     view4d         the binary: arguments, the termmd connection, the loop
```

## A scene

```rust
pub trait Scene {                       // view4d-scene
    fn meshes(&self) -> &[Mesh];        // solid things, shaded
    fn lines(&self) -> &[Lines];        // edges, orbits
    fn bounds(&self) -> Aabb;           // for fitting the camera
    fn status(&self) -> String;         // the overlay's line
    fn error(&self) -> Option<&str> { None }      // in red under it
    fn key(&mut self, key: char) -> bool { false } // keys the viewer leaves
    fn poll(&mut self) -> bool { false }           // twice a second: changed?
    fn animates(&self) -> bool { false }           // frames keep coming; poll each
    fn labels(&self) -> &[Label] { &[] }           // names beside points
}
```

The viewer (`app/src/viewer.rs`) has the camera, the mouse, the standard keys
and the overlay, and draws whatever scene it holds. A model (`app/src/model.rs`)
watches its file in `poll` and has `w` for its wireframe; the orrery
(`view4d-orrery`) has a clock of its own, real time or faster, paused or
stepped; faster than real time it animates.

A model changes only when its file does; the orrery every frame.

## Drawing

Each frame is drawn into BGRA pixels on the CPU (flat shading, a depth buffer),
then the GPU copies them into a dma-buf termmd shows, with the barriers that hand
the image over. That is termmd's surface example, with a real picture. The same
pixels make `--png`.
