# view4d — Glossary

The words view4d's documents use, each with one meaning. termmd's own words
(pane, surface, embedded app, focus) mean what termmd's GLOSSARY.md says.

| Avoid | When you mean | Say |
| ----- | ------------- | --- |
| orbit | the camera turning about its target | orbit (the camera) |
| orbit | a planet's path | a planet's orbit |
| 3D image, image | a file of triangles | [model](#model) |
| class | a Rust struct, enum or trait | type, trait |

### view4d
The program: a [scene viewer](#scene-viewer) showing one [scene](#scene).

### Scene viewer
Everything about looking at a scene: the [camera](#camera), input, drawing, the
[overlay](#overlay), `--png`. Code: `view4d-scene`.

### Scene
What is shown: triangles and how they move, its bounds, any keys of its own and
its status. A [model](#model) or the [orrery](#orrery). Code: `trait Scene` in
`view4d-scene`.

### Model
A scene loaded from a file (STL, OBJ, 3MF, or a [primitive](#primitive)'s
`.prim`), followed as the file changes.
It is one or more [parts](#part). Code: `view4d-model`.

### Part
A named piece of a [model](#model) in one colour: an STL file is one, named by
the file; an OBJ names its own (`o`, `g`), a 3MF its objects with meshes; a part
in several colours is one per colour. Code: `view4d_model::Part`. One part can
be selected (clicked, or `[` `]`), and parts hidden, by name.

### Primitive
A parametric part from primlib (https://github.com/sawdustandswarf/primlib), read from its `.prim` file:
parameters, checks, and a tree of operations whose sizes are expressions over
its names, so it can be built at any values. Code: `view4d-prim` (M5).

### Bed
A printer's bed, outlined under a model at the size `--bed` gives.

### Orrery
The scene of the Sun and planets, where they are now by their orbital elements,
time running at a speed you set. (An orrery is a geared model of the solar
system.) Code: `view4d-orrery` (M2).

### Camera
Where the scene is seen from: it orbits a target point (yaw, pitch, distance),
pans and zooms, and has standard views (front, top, right, isometric).

### Overlay
Text drawn over the scene in a corner: a model's size in mm, the orrery's date.
