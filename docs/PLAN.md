# view4d — Implementation plan

Status: `[x]` done · `[~]` in progress · `[ ]` not started.
How it works is in [DESIGN.md](DESIGN.md); words in [GLOSSARY.md](GLOSSARY.md).

view4d shows 3D scenes that change over time (the fourth dimension) in a pane
of [termmd](https://github.com/sawdustandswarf/termmd/blob/main/docs/DESIGN.md): a [model](GLOSSARY.md#model) from a file,
which follows the file as it is rewritten, or the [orrery](GLOSSARY.md#orrery). It
is an embedded app, built on termmd's `termmd-surface` and `termmd-vulkan`
(termmd's PLAN.md M15), and not part of termmd.

## M1 — Models on screen

Goal: `view4d part.stl &` beside the shell shows the part, turns with the mouse,
and redraws when FreeCAD writes the file again.

- [x] 1.1 Workspace (`scene`, `model`, `app`), docs, zero third-party crates
- [x] 1.2 STL: binary and ASCII, facet normals worked out from the winding (files
  often leave them zero), bounds; tests with files built in the test
  - Binary or ASCII by size, not by `solid` (binary headers often start with it); a file's own normal
    only for a triangle too thin to have one. `scene::{Vec3, Aabb, Mesh}` came with it
- [x] 1.3 Maths and the camera: vectors and 4×4 matrices; a camera that orbits a
  target (yaw, pitch, distance), pans and zooms; fit to bounds; the standard views
  (front, top, right, isometric), third angle as in the fc-doc drawings
  - No matrices after all: points go to camera space by the camera's axes and are projected from there,
    which is where near-plane clipping happens too. Z up as in FreeCAD; front looks along +Y, isometric
    from (+1, −1, +1). Pitch stops straight above and below, where up follows the yaw
- [x] 1.4 The rasterizer, on the CPU: triangles into BGRA pixels with a depth
  buffer, flat shading by a light at the camera, edges drawn where faces meet at
  an angle; `--png FILE` writes one frame. Tests: small scenes checked pixel by pixel
  - The light is a direction, not a point (a point lit each triangle of a flat face differently),
    and sits a little above and left of the eye: straight from the eye, a box's three faces seen
    isometrically are equally bright. Both sides of a face are lit, for open surfaces
  - FreeCAD's meshes have T-junctions along seams, so a lone edge overlapped on its line by lone
    edges of level faces is inside the surface, not drawn; a triangle a file holds twice counts
    once (latch.stl holds every triangle twice: each edge had four faces)
  - The PNG is stored, not compressed: no deflate compressor to write. 55,000 triangles at
    800×600 in 0.1 s in a release build
- [x] 1.5 The viewer in termmd: a surface placed by `--place KEYS` (default beside
  the shell), `--focus=no`; drag orbits, Shift+drag pans, the wheel zooms, `f`
  fits, `r` resets, `1`–`4` the standard views, `w` wireframe, `q` quits; frames
  copied into dma-bufs as termmd's surface example does
  - Shift and the mouse are termmd's (selection), so a right or middle drag, or Ctrl and a drag,
    pans. Default place `place=right w=50% h=50%`. A frame is drawn only when something has
    changed, after everything termmd has sent so far, and one at a time (the next after `frame`)
  - `app/src/gpu.rs` is the surface example's staging buffer and copy; if a second app needs
    them, they belong in termmd-vulkan
  - Checked in termmd's headless host: placed, a frame at the pane's size, `w` and `1` redraw,
    `q` quits
- [x] 1.6 Reload on change (the file's size and time checked twice a second, read
  again once stable), and the overlay: size in mm and the triangle count, in a
  small built-in bitmap font
  - A file that can't be read again (half written, gone) keeps the last model on screen, with the
    error in red under the overlay until it can. The camera stays where it was across a reload
  - The font is 5×7, printable ASCII (`scene/src/text.rs`), at 1–3× by the frame's height; the
    overlay wraps to the frame's width; `o` hides it, and `--png` leaves it out
  - Checked in termmd's headless host: the file replaced by another, then truncated

## M2 — The orrery

- [x] 2.1 Planet positions from JPL's approximate Keplerian elements (valid
  1800–2050) and the clock: Kepler's equation solved; tests against positions JPL
  gives for known dates
  - `orrery/` (`view4d-orrery`), f64 throughout. Tested against Horizons at J2000 and
    2025-06-01 within 1.5× JPL's nominal errors (nominal, not worst: Saturn's distance is 1.15×)
  - Uranus and Neptune match positions from the solar system's barycentre, not the Sun's centre
    (from the Sun, Neptune is 4.5× out: the Sun's own wobble); the rest match the Sun's centre.
    At the orrery's scale neither shows
- [x] 2.2 The scene: the Sun and planets as spheres, orbits as lines, the view
  from above the ecliptic; `view4d --orrery`
  - `trait Scene` as DESIGN.md has it; the model became one (its file watch and `w` with it)
  - Distances on a log scale (`ln(1 + r/0.2 au)`) for now, with true and even written for 2.3;
    sizes against Neptune's distance as drawn: every planet 1.4%, and up to 3% more by the
    square root of its radius. Following the clock, it is built again when the minute changes
  - Checked against the sky: November 2026 has the Earth at about 50° heliocentric longitude,
    Saturn and Neptune near 0°, Uranus about 60°, as the picture does
- [x] 2.3 Time: pause, faster and slower, a day or a year at a step, back to now;
  scales for size and distance (true, log, even), since no one scale shows it all
  - Space pauses; `+`/`-` step through real time, a minute, hour, day, week, month, year and ten
    years a second; `.`/`,` a day, `>`/`<` a year; `n` now; `d` log, even, true distances. Faster
    than real time it animates: `Scene::animates`, and a frame whenever termmd takes one
  - Sizes stay drawn big at every scale, but no bigger than a share of Mercury's distance, so at
    true distances the Sun doesn't swallow the inner planets (and the outer ones are specks: zoom)
  - Outside 1800–2050 a warning shows under the status: the elements drift there
  - A scene key that changes its extent (the scale) fits the camera again
- [x] 2.4 Planet names; an idle mode, slow and real time, to leave in a corner
  with `--focus=no`
  - Names are `Scene::labels`: points with text, which the viewer projects and writes beside them
    in the overlay's font, in the planet's colour lightened to read on the dark; `l` hides them
  - `--idle`: `fixed=top-right w=36 h=14` and `--focus=no` unless given otherwise. In real time
    it draws once a minute, when the minute shown changes, and otherwise sleeps
  - Checked in termmd's headless host: with `--idle &` the shell keeps the keyboard

## M3 — More formats

- [x] 3.1 OBJ + MTL: named parts and colours
  - `view4d_model::{Part, read}`: a model is parts, each a name and a mesh in one colour; STL is one
    part named by the file, `.obj` (any case) is OBJ. A part is what `o` or `g` names, one mesh per
    material it uses; `usemtl` carries across parts as OBJ has it. Colour is the MTL's `Kd`; a
    material in no MTL file, or an MTL that can't be read, keeps the default colour
  - Polygons split into a fan; negative indices, `\` continuations; `vn` and `vt` ignored (normals
    from the winding, as for STL). Feature edges are found per part, so parts meeting are outlined
  - The overlay counts parts when there is more than one. Only the OBJ is watched: an MTL
    rewritten alone is read with the OBJ's next change
- [x] 3.2 3MF: colours, parts and units (a zip of XML: inflate, a zip directory
  reader and a small XML reader; termmd-core's inflate may be shared)
  - Decided 2026-11-14: inflate from `termmd-deflate`, split out of termmd-core for it (termmd's M15.7)
  - `model/src/zip.rs` (the central directory, Zip64, stored or deflated), `xml.rs` (elements and
    attributes by local name, entities decoded; text skipped), `three_mf.rs`: the root model named by
    `_rels/.rels`, the build's items through components and their transforms (a mirroring one turns
    triangles back outwards), objects in other model files (`p:path`, as Bambu Studio and Orca write),
    units to mm. Colours from base materials and colour groups, by triangle or object
  - A part is an object with a mesh, named by itself or the nearest object above it with a name
    (else `object N`). A zip is taken as 3MF whatever its name
  - 524 of the 525 3MF files in a local collection read (slicer projects from Bambu Studio, Orca, Prusa, and
    exports; the other is an autosave missing its object file); 3 million triangles in 2 s. Slicers'
    filament colours (in their own config files) not read: those parts show in the default colour
- [x] 3.3 Parts shown and hidden by name; a print bed outline at the printer's size
  - A click selects the part under the pointer (a ray from the eye, the nearest shown triangle; a
    click on nothing selects nothing), `]` and `[` the next and previous; its edges are drawn in
    amber and its name is in the overlay. `h` hides or shows it, `H` shows it alone (again: all),
    `a` shows all; `--only NAME` and `--hide NAME` (repeatable; a name there is none of is an error
    listing the parts). Hiding and the selection go by name, so they last across a reload
  - `--bed WxD` (mm) outlines a bed centred under the model at its lowest point; `b` toggles it
  - Decided 2026-11-14: only what the 3D model files hold. Slicers' own settings in a 3MF (Bambu's
    and Orca's bed, object names and filament colours, Prusa's `bed_shape`) are not read for now
  - Keys reach a scene as typed: Shift+`.` is `>` (it came as `.`, so the orrery's `>`, `<` and `+`
    did nothing in termmd)

## M4 — On the GPU

When models or panes outgrow the CPU: a Vulkan pipeline drawing straight into
the dma-buf (vertex buffers, a depth buffer, shaders compiled by `glslc` into
checked-in SPIR-V, as termmd does), the CPU rasterizer kept for `--png` and tests.

## M5 — primlib primitives (`view4d-prim`)

primlib (https://github.com/sawdustandswarf/primlib) writes primitives as `.prim` files (its P1): parameters,
checks and a tree of operations whose sizes are expressions. view4d draws them.

- [x] 5.1 A JSON reader and the expression evaluator, tested against primlib's
  - `prim/` (`view4d-prim`): `json.rs` (keys kept in order, line and column in errors, nesting
    limited), `expr.rs` (`primlib/expr.py`'s tokens, precedence, degrees and errors; a parsed
    `Expr` with `uses` and `refs` for the builder), `Prim::read`/`parse` refusing what primlib's
    `loads` refuses, `names` (given values, then the file's, then defaults) and `failed_checks`
  - `prim/tests/data`: the 19 primitives primlib exports and `cases.json`, written by primlib's
    `tools/view4d_fixtures.py`: 2,159 names and expression values at defaults and other values
    (series indices 0–2, failing checks), all within 1e-9 of Python's; its error cases refused
  - A name can be another name's text (stencil's `font1`): names are values, sizes numbers
- [x] 5.2 The geometry kernel: each operation a closed f64 mesh, booleans by BSP-tree
  mesh CSG; coplanar faces (the chamfer cuts) snapped to a tolerance; each primitive
  within 0.1% of FreeCAD's volume at its defaults
  - `csg.rs`: convex polygons that keep their plane through every split, points within 1e-5 mm of
    a plane on it, the trees walked with stacks (no recursion to overflow); solids whose bounds
    don't meet are put together without a tree. `build.rs` evaluates the operations (memoised,
    per series index where one is used), `profile.rs` makes the solids from profiles
  - Curves are polygons to 0.01 mm (16 to 256 sides), radii grown so each section or wedge has the
    true area: cylinders, cones, revolves and spheres have their true volumes; rounded corners are
    chords (a little under: 0.02% of a 2 mm round's corner)
  - `tests/freecad.rs`: FreeCAD's volumes and bounds (primlib's baseline, written into
    `tests/data/freecad.json` by its `tools/view4d_fixtures.py`). 17 of 19 within 0.1% (most within
    0.001%; rotary_latch 0.08%, its many rounds), bounds within 0.01 mm, every one closed; 2 s in all
    in a release build (hinge 97,000 polygons in 0.14 s)
- [x] 5.3 Operations as the primitives need them: box, cylinder, cone, beyond, array,
  polar, fuse, cut, common, compound (box_edge, box_corner, hinge); then revolve,
  prism, loft, slot, sphere, extrude; text last (glyph outlines: termmd's font code?).
  An operation not yet there is named, and nothing half drawn
  - Every primitive needed a profile, so the operations came together. `loft_about` is a Catmull-Rom sweep through its sections in (r, z, angle), in a
    revolve's steps; a twisted face is four triangles about its centre (on the ruled surface), not
    two, which left 1.4% out of a rising sweep
  - Closed is measured as the faces' area vectors summing to nothing: twist_plug is out by 7e-4 mm²
    in 4,600, the rest by 1e-12
  - `text` (`text.rs`) as Draft ShapeString sets it: glyphs from termmd's `termmd-font` crate
    (split out of termmd-core for this, M15.8 there, with the `kern` table), kerning, scaled so the
    capital M is the size, justified on the cap height and the tight width. FreeCAD's FT2FC loads
    glyphs hinted at 480 ppem, so heights, advances and kerning are whole pixels: done here too, it
    took 0.14% of area away (a cap height of 1408 units, not 1409). The font's own y hinting is
    not done (no bytecode interpreter): up to 0.06% of a line's area. `extrude` makes prisms of the
    outlines less those of the holes (even-odd). A text's bounds are its outlines' (`xmin(@6)`)
  - stencil within 0.0004% of FreeCAD's volume, its two templates (the same plate, cut two ways)
    measured each, as FreeCAD sums a compound; 18 of 19 primitives' meshes closed
  - `Solid::mesh()`: vertices welded within 2e-5 mm (twice the booleans' tolerance: two splits
    near one point left three just over 1e-5 apart in a row in stencil), each vertex lying
    along another face's edge put into it (no T-junctions), faces fanned from the vertex that
    leaves fewest flat triangles, so every edge of a face is a triangle's. All but twist_plug have
    no open edge. twist_plug has 91, where its lead (a loft about z) meets the revolve carrying on from it:
    their shared end is cut by different steps and radii grown by different factors, so slivers.
    Quantising the steps (powers of two) made it worse elsewhere; for M8, where a print needs it
- [x] 5.4 `view4d part.prim`: parts in their colours, followed as the file changes
  - `view4d-model` reads `.prim` (by extension) through `view4d-prim`: built at the file's values,
    or `--set NAME=VALUE` (repeatable) over them, every time it is read. A compound's parts take
    their operations' names and colours from a palette; a single solid is one part named by the
    file. A failed check is the error, every message given (and, on a reload, red under the
    overlay while the last model stays); so is a name the primitive hasn't, or a font not found
  - Normals are the faces' planes', not the triangles' windings (slivers'); drawn from the welded
    mesh, faces meet edge to edge and only real edges are drawn (the T-junctions had drawn stray
    lines across flat faces). hinge in 0.3 s, rotary_latch in 0.6 s, release build
- [~] 5.5 Recipes and assemblies (primlib's P2), live: counts follow the parameters
  - Text in expressions (primlib's P2.1a): `<<…>>`, `+`, `==`/`!=`, `name[a:b]`, `name[k]`,
    `mod`, `len`, `count`, `repeat`; tested against primlib's cases
  - Recipes (`.prim` version 2, `recipe.rs`): names and checks as a primitive's (the parts'
    checks too), parts each a primitive at bound values, placed by `instance`, `row` (by key),
    `series`, `choose`, `group` and `move`; laid out as primlib's `layout` lays them out
    (`tests/recipes.rs`: every placed part, at defaults and other values, from primlib's
    `recipes.json`). `Document::parse` reads either kind
  - `view4d edge_run.prim --set "key='LMSF"`: each part built and meshed once, placed by copying;
    a mirrored part's triangles turned over so they still face out; parts in their own colours
  - Assemblies (P2.2a, `kind: assembly`): parts that are recipes or assemblies, laid out where
    placed and named `outer/inner`; `pieces` (some of a compound: a hinge's `side_a`); `print`;
    a `joint` placement turning what it holds about a line by an angle expression.
    `Recipe::leaves` gives each primitive part by path. `view4d hinged_lid.prim --set open_angle=60`
    shows the lid side up on its hinge (`tests/recipes.rs`, against primlib's layouts)
  - The hybrid case (P2.2c): corners, panels and keyed edges, the lid on its joint;
    `view4d hybrid_case.prim --set open_angle=60` in 0.8 s, 46 parts (release)
  - Its rail (P2.2d): `turn` operation (`Solid::turned`); `rail_end` within 0.0005% of FreeCAD
  - Its hinge row and latches (P2.2e): the whole case, 57 parts, in 16 s (release; rotary_latch
    built once per piece it gives: a part's build could be shared between parts of one
    primitive at the same values)

## M6 — Time

- [x] `--animate NAME=FROM:TO[:SECONDS]`: one value of a `.prim` file swept from FROM to TO and
  back, eased, read again every frame. A recipe's part built the same as in the last read is not
  built again, its feature edges with it (found once, placed with the part): `hinged_lid.prim
  --animate open_angle=0:110` reads again in 15 ms, not 0.85 s. `--speed N` starts the orrery
  N steps faster. `--orbit SECONDS` takes the eye round the scene, rising and falling a little, until a
  drag. A swept model is fitted to its whole sweep
- [ ] Motion tracks: a parameter or a joint angle over time (a hinge opening, a latch
  turning, a part resizing), played with the orrery's clock keys; overlaps between
  moving parts found at sampled times

## M7 — The editor

- [ ] Parameters picked and changed in view4d, checks live, written back to the file;
  editing the operation tree its own plan

## M8 — For printing

- [ ] STL and 3MF writers (parts, colours, mm); closed and manifold, volumes as
  FreeCAD's, slicing in Orca as FreeCAD's export does

## M9 — Slicer and G-code

- [ ] Its own plan when it comes: slices, perimeters, infill, supports, G-code for the
  printer, sent straight to it

## Later

- [ ] glTF, if something needs it
- [ ] A section plane; measuring between two points
- [ ] Outside termmd: kitty graphics or sixel frames, or a window of its own
