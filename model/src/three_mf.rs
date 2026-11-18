//! 3MF: a zip of XML. The model file the package's relationships name
//! (`3D/3dmodel.model` as a rule) holds objects, each a mesh or components
//! placing other objects, and a build of items placing objects; what the
//! build places is shown, by every transform on the way. Objects in other
//! model files of the package (the production extension, `p:path`, as
//! Bambu Studio writes) are read too.
//!
//! A part is an object with a mesh, named by its own `name` or else by the
//! nearest object above it with one. Colours are base materials'
//! `displaycolor` or the materials extension's colour groups, by the
//! triangle's `pid` and `p1` or else its object's `pid` and `pindex`; a
//! part in several colours is one mesh per colour. Units are made mm.

use std::collections::HashMap;
use std::rc::Rc;

use view4d_scene::{Mesh, Vec3};

use crate::xml::{Event, Reader, Start};
use crate::zip::Zip;
use crate::{Error, Part};

/// How deep components may nest: deeper is taken as a cycle.
const DEPTH: usize = 32;

/// The parts of the 3MF package in `bytes`.
pub fn parse(bytes: &[u8]) -> Result<Vec<Part>, Error> {
    let zip = Zip::open(bytes)?;
    let root = root_model(&zip)?;
    let mut files = Files { zip: &zip, loaded: HashMap::new() };
    let model = files.get(&root)?;
    let mut parts = Parts::default();
    for item in &model.build {
        let path = item.path.clone().unwrap_or_else(|| root.clone());
        files.walk(&path, item.object, item.transform, "", 0, &mut parts)?;
    }
    Ok(parts.finish(model.unit))
}

/// The root model's path in the package: what `_rels/.rels` names as the
/// 3D model, or the usual place.
fn root_model(zip: &Zip<'_>) -> Result<String, Error> {
    if let Some(rels) = zip.read("_rels/.rels") {
        let rels = rels?;
        for e in Reader::new(&String::from_utf8_lossy(&rels)) {
            if let Event::Start(s) = e?
                && s.name == "Relationship"
                && s.attr("Type").is_some_and(|t| t.ends_with("/3dmodel"))
                && let Some(t) = s.attr("Target")
            {
                return Ok(t.trim_start_matches('/').to_string());
            }
        }
    }
    Ok("3D/3dmodel.model".into())
}

/// An affine transform as 3MF writes it: a point (x y z 1) times four rows
/// of three.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transform([f32; 12]);

impl Transform {
    const IDENTITY: Transform = Transform([1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.]);

    fn parse(s: Option<&str>) -> Result<Transform, Error> {
        let Some(s) = s else { return Ok(Transform::IDENTITY) };
        let n: Vec<f32> = s.split_ascii_whitespace().map_while(|w| w.parse().ok()).collect();
        n.try_into().map(Transform).map_err(|_| Error(format!("a transform that isn't twelve numbers: `{s}`")))
    }

    fn apply(&self, p: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            p.x * m[0] + p.y * m[3] + p.z * m[6] + m[9],
            p.x * m[1] + p.y * m[4] + p.z * m[7] + m[10],
            p.x * m[2] + p.y * m[5] + p.z * m[8] + m[11],
        )
    }

    /// This, then `outer`.
    fn then(&self, outer: &Transform) -> Transform {
        let (a, b) = (&self.0, &outer.0);
        let mut m = [0f32; 12];
        for r in 0..4 {
            for c in 0..3 {
                m[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
            }
            if r == 3 {
                for c in 0..3 {
                    m[9 + c] += b[9 + c];
                }
            }
        }
        Transform(m)
    }

    /// Whether it mirrors, so triangles' winding turns inside out.
    fn mirrors(&self) -> bool {
        let m = &self.0;
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        det < 0.0
    }
}

/// An object placed: by a component or a build item.
#[derive(Debug, Clone)]
struct Placed {
    object: u32,
    transform: Transform,
    /// Another model file of the package (`p:path`).
    path: Option<String>,
}

#[derive(Debug, Default)]
struct Object {
    name: String,
    vertices: Vec<Vec3>,
    triangles: Vec<([usize; 3], Option<[u8; 3]>)>,
    components: Vec<Placed>,
    /// The colour its triangles have unless they say.
    pid: Option<u32>,
    pindex: Option<usize>,
}

/// One model file.
#[derive(Debug)]
struct Model {
    /// mm per unit.
    unit: f32,
    objects: HashMap<u32, Object>,
    build: Vec<Placed>,
}

fn number<T: std::str::FromStr>(s: &Start<'_>, name: &str) -> Result<Option<T>, Error> {
    match s.attr(name) {
        None => Ok(None),
        Some(v) => v.trim().parse().map(Some).map_err(|_| Error(format!("<{} {name}=\"{v}\">: not a number", s.name))),
    }
}

fn required<T: std::str::FromStr>(s: &Start<'_>, name: &str) -> Result<T, Error> {
    number(s, name)?.ok_or_else(|| Error(format!("<{}> without `{name}`", s.name)))
}

/// `#RRGGBB` or `#RRGGBBAA`.
fn color(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().strip_prefix('#').filter(|h| matches!(h.len(), 6 | 8))?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn placed(s: &Start<'_>) -> Result<Placed, Error> {
    Ok(Placed {
        object: required(s, "objectid")?,
        transform: Transform::parse(s.attr("transform"))?,
        path: s.attr("path").map(|p| p.trim_start_matches('/').to_string()),
    })
}

impl Model {
    fn parse(text: &str) -> Result<Model, Error> {
        let mut model = Model { unit: 1.0, objects: HashMap::new(), build: Vec::new() };
        let mut colors: HashMap<u32, Vec<Option<[u8; 3]>>> = HashMap::new();
        let mut group: Option<u32> = None;
        let mut object: Option<(u32, Object)> = None;
        for e in Reader::new(text) {
            match e? {
                Event::Start(s) => match s.name {
                    "model" => {
                        model.unit = match s.attr("unit").unwrap_or("millimeter") {
                            "micron" => 0.001,
                            "millimeter" => 1.0,
                            "centimeter" => 10.0,
                            "inch" => 25.4,
                            "foot" => 304.8,
                            "meter" => 1000.0,
                            u => return Err(Error(format!("a unit of `{u}`"))),
                        }
                    }
                    "basematerials" | "colorgroup" => {
                        let id = required(&s, "id")?;
                        colors.insert(id, Vec::new());
                        group = (!s.empty).then_some(id);
                    }
                    "base" | "color" => {
                        let c = s.attr(if s.name == "base" { "displaycolor" } else { "color" }).and_then(color);
                        if let Some(g) = group.and_then(|g| colors.get_mut(&g)) {
                            g.push(c);
                        }
                    }
                    "object" => {
                        let o = Object {
                            name: s.attr("name").unwrap_or("").trim().to_string(),
                            pid: number(&s, "pid")?,
                            pindex: number(&s, "pindex")?,
                            ..Object::default()
                        };
                        object = Some((required(&s, "id")?, o));
                    }
                    "vertex" => {
                        if let Some((_, o)) = &mut object {
                            o.vertices.push(Vec3::new(required(&s, "x")?, required(&s, "y")?, required(&s, "z")?));
                        }
                    }
                    "triangle" => {
                        if let Some((_, o)) = &mut object {
                            let v = [required(&s, "v1")?, required(&s, "v2")?, required(&s, "v3")?];
                            let pid = number(&s, "pid")?.or(o.pid);
                            let index = number(&s, "p1")?.or(o.pindex).unwrap_or(0);
                            let c = pid.and_then(|p| colors.get(&p)).and_then(|g| g.get(index).copied().flatten());
                            o.triangles.push((v, c));
                        }
                    }
                    "component" => {
                        if let Some((_, o)) = &mut object {
                            o.components.push(placed(&s)?);
                        }
                    }
                    "item" => model.build.push(placed(&s)?),
                    _ => {}
                },
                Event::End("basematerials" | "colorgroup") => group = None,
                Event::End("object") => {
                    if let Some((id, o)) = object.take() {
                        if let Some(&([a, b, c], _)) =
                            o.triangles.iter().find(|(v, _)| v.iter().any(|&i| i >= o.vertices.len()))
                        {
                            let n = o.vertices.len();
                            return Err(Error(format!("object {id}: a triangle {a} {b} {c} of {n} vertices")));
                        }
                        model.objects.insert(id, o);
                    }
                }
                Event::End(_) => {}
            }
        }
        Ok(model)
    }
}

/// The package's model files, read when first needed.
struct Files<'z, 'a> {
    zip: &'z Zip<'a>,
    loaded: HashMap<String, Rc<Model>>,
}

impl Files<'_, '_> {
    fn get(&mut self, path: &str) -> Result<Rc<Model>, Error> {
        if let Some(m) = self.loaded.get(path) {
            return Ok(m.clone());
        }
        let bytes = self.zip.read(path).ok_or_else(|| Error(format!("no {path} in the package")))??;
        let m = Rc::new(Model::parse(&String::from_utf8_lossy(&bytes)).map_err(|e| Error(format!("{path}: {e}")))?);
        self.loaded.insert(path.to_string(), m.clone());
        Ok(m)
    }

    /// Object `id` of the file at `path`, and what it places, into `parts`.
    fn walk(
        &mut self,
        path: &str,
        id: u32,
        t: Transform,
        name: &str,
        depth: usize,
        parts: &mut Parts,
    ) -> Result<(), Error> {
        if depth > DEPTH {
            return Err(Error(format!("components nested more than {DEPTH} deep (a cycle?)")));
        }
        let model = self.get(path)?;
        let o = model.objects.get(&id).ok_or_else(|| Error(format!("{path}: no object {id}")))?;
        let name = match (o.name.as_str(), name) {
            ("", "") => format!("object {id}"),
            ("", n) => n.to_string(),
            (n, _) => n.to_string(),
        };
        let mirrors = t.mirrors();
        for &([a, b, c], color) in &o.triangles {
            let p = |i: usize| t.apply(o.vertices[i]);
            let tri = if mirrors { [p(a), p(c), p(b)] } else { [p(a), p(b), p(c)] };
            parts.push(&name, color, tri);
        }
        for c in &o.components {
            let file = c.path.as_deref().unwrap_or(path);
            self.walk(file, c.object, c.transform.then(&t), &name, depth + 1, parts)?;
        }
        Ok(())
    }
}

/// A part's name, and its colour if it has one.
type Key = (String, Option<[u8; 3]>);

/// Triangles by part and colour, in the order they first come.
#[derive(Default)]
struct Parts {
    list: Vec<(Key, Vec<[Vec3; 3]>)>,
    index: HashMap<Key, usize>,
}

impl Parts {
    fn push(&mut self, name: &str, color: Option<[u8; 3]>, tri: [Vec3; 3]) {
        let list = &mut self.list;
        let i = *self.index.entry((name.to_string(), color)).or_insert_with(|| {
            list.push(((name.to_string(), color), Vec::new()));
            list.len() - 1
        });
        self.list[i].1.push(tri);
    }

    /// The parts, scaled by `unit` to mm.
    fn finish(self, unit: f32) -> Vec<Part> {
        self.list
            .into_iter()
            .map(|((name, color), mut triangles)| {
                if unit != 1.0 {
                    triangles.iter_mut().flatten().for_each(|p| *p = *p * unit);
                }
                let mut mesh = Mesh::from_triangles(triangles);
                mesh.color = color.unwrap_or(Mesh::DEFAULT_COLOR);
                Part { name, mesh, edges: None }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::archive;

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/box.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;

    /// A unit tetrahedron, wound outwards, as an object's mesh.
    fn tetrahedron(attrs: &str, triangle: &str) -> String {
        format!(
            r#"<object id="{{ID}}" type="model" {attrs}><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/>
</vertices><triangles>
<triangle v1="0" v2="2" v3="1" {triangle}/><triangle v1="0" v2="1" v3="3"/>
<triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/>
</triangles></mesh></object>"#
        )
    }

    fn package(model: &str, more: &[(&str, &str)]) -> Vec<u8> {
        let mut files: Vec<(&str, &[u8])> = vec![("_rels/.rels", RELS.as_bytes()), ("3D/box.model", model.as_bytes())];
        files.extend(more.iter().map(|(n, d)| (*n, d.as_bytes())));
        archive(&files)
    }

    fn summary(parts: &[Part]) -> Vec<(&str, usize, [u8; 3])> {
        parts.iter().map(|p| (p.name.as_str(), p.mesh.triangles.len(), p.mesh.color)).collect()
    }

    #[test]
    fn objects_components_colours_and_units() {
        let model = format!(
            r##"<?xml version="1.0"?>
<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"
       xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
 <resources>
  <basematerials id="1"><base name="Red" displaycolor="#FF0000"/><base name="Green" displaycolor="#00FF00FF"/></basematerials>
  <m:colorgroup id="2"><m:color color="#0000FF"/></m:colorgroup>
  {body}
  {lid}
  <object id="5" name="case"><components>
   <component objectid="3"/>
   <component objectid="4" transform="1 0 0 0 1 0 0 0 1 10 0 0"/>
  </components></object>
 </resources>
 <build><item objectid="5" transform="1 0 0 0 1 0 0 0 1 0 0 5"/></build>
</model>"##,
            body = tetrahedron(r#"name="body" pid="1" pindex="1""#, r#"pid="2" p1="0""#).replace("{ID}", "3"),
            lid = tetrahedron("", "").replace("{ID}", "4"),
        );
        let parts = parse(&package(&model, &[])).unwrap();
        assert_eq!(
            summary(&parts),
            [("body", 1, [0, 0, 255]), ("body", 3, [0, 255, 0]), ("case", 4, Mesh::DEFAULT_COLOR)],
            "the lid has no name of its own"
        );
        let bounds = parts.iter().fold(view4d_scene::Aabb::EMPTY, |b, p| b.union(p.mesh.bounds()));
        assert_eq!((bounds.min, bounds.max), (Vec3::new(0., 0., 50.), Vec3::new(110., 10., 60.)), "in mm");
        assert_eq!(parts[1].mesh.normals[0], Vec3::new(0., -1., 0.));
    }

    #[test]
    fn objects_in_other_files_and_mirroring() {
        let model = r#"<model unit="millimeter"><resources>
 <object id="1" name="assembly"><components>
  <component p:path="/3D/Objects/object_1.model" objectid="7" transform="-1 0 0 0 1 0 0 0 1 0 0 0"/>
 </components></object>
</resources><build><item objectid="1"/></build></model>"#;
        let object = format!("<model><resources>{}</resources></model>", tetrahedron("", "").replace("{ID}", "7"));
        let parts = parse(&package(model, &[("3D/Objects/object_1.model", &object)])).unwrap();
        assert_eq!(summary(&parts), [("assembly", 4, Mesh::DEFAULT_COLOR)]);
        let m = &parts[0].mesh;
        // Mirrored in x, still wound outwards: the base faces down, the slant face out to -x.
        assert_eq!(m.normals[0], Vec3::new(0., 0., -1.));
        assert!(m.normals[3].x < 0.0 && m.normals[3].y > 0.0);
    }

    #[test]
    fn transforms_compose() {
        let turn = Transform([0., 1., 0., -1., 0., 0., 0., 0., 1., 0., 0., 0.]); // a quarter turn about z
        let shift = Transform::parse(Some("1 0 0 0 1 0 0 0 1 5 6 7")).unwrap();
        let p = Vec3::new(1., 0., 0.);
        assert_eq!(turn.apply(p), Vec3::new(0., 1., 0.));
        assert_eq!(turn.then(&shift).apply(p), Vec3::new(5., 7., 7.));
        assert_eq!(shift.then(&turn).apply(p), Vec3::new(-6., 6., 7.));
        assert!(!turn.mirrors() && Transform::parse(Some("1 0 0 0 1 0 0 0 -1 0 0 0")).unwrap().mirrors());
    }

    #[test]
    fn errors() {
        let e = |model: &str| parse(&package(model, &[])).unwrap_err().0;
        let item = r#"<build><item objectid="1"/></build>"#;
        assert_eq!(e(&format!("<model><resources/>{item}</model>")), "3D/box.model: no object 1");
        let tri = r#"<object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/></vertices>
                     <triangles><triangle v1="0" v2="0" v3="1"/></triangles></mesh></object>"#;
        assert_eq!(
            e(&format!("<model><resources>{tri}</resources></model>")),
            "3D/box.model: object 1: a triangle 0 0 1 of 1 vertices"
        );
        let cycle = r#"<object id="1"><components><component objectid="1"/></components></object>"#;
        assert!(e(&format!("<model><resources>{cycle}</resources>{item}</model>")).contains("a cycle"));
        assert_eq!(e(r#"<model unit="furlong"/>"#), "3D/box.model: a unit of `furlong`");
        assert_eq!(e(r#"<model><build><item/></build></model>"#), "3D/box.model: <item> without `objectid`");
        assert_eq!(
            e(
                r#"<model><resources><object id="1"><mesh><vertices><vertex x="a" y="0" z="0"/></vertices></mesh></object></resources></model>"#
            ),
            "3D/box.model: <vertex x=\"a\">: not a number"
        );
        let no_model = archive(&[("_rels/.rels", RELS.as_bytes())]);
        assert_eq!(parse(&no_model).unwrap_err().0, "no 3D/box.model in the package");
        assert!(parse(&package("<model/>", &[])).unwrap().is_empty());
    }
}
