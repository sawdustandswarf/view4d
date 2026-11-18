//! `view4d-model`: models from files (docs/GLOSSARY.md, "Model").

pub mod obj;
pub mod prim;
pub mod stl;
pub mod three_mf;
pub mod xml;
pub mod zip;

use std::path::Path;

use view4d_scene::{Mesh, Vec3};

/// Why a file isn't a model we can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A named piece of a model in one colour (docs/GLOSSARY.md, "Part"). An
/// STL file is one part, named by the file; OBJ and 3MF name their own.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub mesh: Mesh,
    /// Its feature edges, when the reader has them already (a recipe's
    /// part, placed); otherwise the viewer finds them.
    pub edges: Option<Vec<[Vec3; 2]>>,
}

/// The kinds of file a model is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Stl,
    Obj,
    ThreeMf,
    /// primlib's `.prim`: a primitive, built here.
    Prim,
}

impl Format {
    /// A zip is 3MF, whatever its name; otherwise by the file's extension
    /// (`.obj`, `.3mf`, `.prim`, any case), STL if none of them.
    pub fn of(path: &Path, bytes: &[u8]) -> Format {
        if bytes.starts_with(b"PK\x03\x04") {
            return Format::ThreeMf;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("obj") => Format::Obj,
            Some(e) if e.eq_ignore_ascii_case("3mf") => Format::ThreeMf,
            Some(e) if e.eq_ignore_ascii_case("prim") => Format::Prim,
            _ => Format::Stl,
        }
    }
}

/// The parts of the model in `path`, or why not, saying which file. An
/// OBJ's `mtllib` files are read from beside it.
pub fn read(path: &Path) -> Result<Vec<Part>, String> {
    read_at(path, &[])
}

/// As `read`; a `.prim` file is built at `values` (name → expression) over
/// its own, which other files can't take.
pub fn read_at(path: &Path, values: &[(String, String)]) -> Result<Vec<Part>, String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let bytes = std::fs::read(path).map_err(|e| at(&e))?;
    let name = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let format = Format::of(path, &bytes);
    if !values.is_empty() && format != Format::Prim {
        return Err(at(&"only a .prim file takes values"));
    }
    match format {
        Format::Prim => prim::parse(&String::from_utf8_lossy(&bytes), values, &name).map_err(|e| at(&e)),
        Format::Stl => Ok(vec![Part { name, mesh: stl::parse(&bytes).map_err(|e| at(&e))?, edges: None }]),
        Format::Obj => {
            let dir = path.parent().unwrap_or(Path::new(""));
            obj::parse(&String::from_utf8_lossy(&bytes), |f| std::fs::read_to_string(dir.join(f)).ok())
                .map_err(|e| at(&e))
        }
        Format::ThreeMf => three_mf::parse(&bytes).map_err(|e| at(&e)),
    }
}
