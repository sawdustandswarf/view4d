//! Reading files out of a zip archive, as 3MF is: the central directory
//! found from its end record (Zip64 too), files stored or deflated
//! (termmd-deflate's inflate). Nothing is written, nor encrypted files read.

use crate::Error;

/// The most a file may inflate to: more is taken as a broken archive.
const LIMIT: usize = 1 << 30;

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    /// Where its local header is.
    offset: usize,
}

/// An archive's directory, over its bytes.
#[derive(Debug)]
pub struct Zip<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn broken(what: &str) -> Error {
    Error(format!("a broken zip: {what}"))
}

impl<'a> Zip<'a> {
    /// The directory of the archive in `bytes`.
    pub fn open(bytes: &'a [u8]) -> Result<Zip<'a>, Error> {
        let (count, at) = directory(bytes).ok_or_else(|| Error("not a zip: no end of central directory".into()))?;
        let mut entries = Vec::with_capacity(count.min(1 << 16));
        let mut p = at;
        for _ in 0..count {
            entries.push(entry(bytes, &mut p).ok_or_else(|| broken("a central directory entry"))?);
        }
        Ok(Zip { bytes, entries })
    }

    /// The names of its files, in the directory's order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The file called `name`, matched exactly or else ignoring ASCII case;
    /// none if there is no such file.
    pub fn read(&self, name: &str) -> Option<Result<Vec<u8>, Error>> {
        let e = self
            .entries
            .iter()
            .find(|e| e.name == name)
            .or_else(|| self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)))?;
        Some(self.data(e))
    }

    fn data(&self, e: &Entry) -> Result<Vec<u8>, Error> {
        let b = self.bytes;
        let at = |what: &dyn std::fmt::Display| broken(&format!("{}'s {what}", e.name));
        if u32_at(b, e.offset) != Some(0x0403_4b50) {
            return Err(at(&"local header"));
        }
        let header = |o| u16_at(b, e.offset + o).ok_or_else(|| at(&"local header"));
        let (n, x) = (header(26)?, header(28)?);
        let start = e.offset + 30 + n as usize + x as usize;
        let data = start.checked_add(e.compressed).and_then(|end| b.get(start..end)).ok_or_else(|| at(&"data"))?;
        let out = match e.method {
            0 => data.to_vec(),
            8 => termmd_deflate::inflate::inflate(data, e.size.min(LIMIT))
                .map_err(|err| at(&format_args!("{err:?} data")))?,
            m => return Err(Error(format!("{}: compressed by method {m}, not stored or deflated", e.name))),
        };
        if out.len() != e.size {
            return Err(at(&"size"));
        }
        Ok(out)
    }
}

/// The number of entries and where the central directory starts.
fn directory(b: &[u8]) -> Option<(usize, usize)> {
    // The end record is 22 bytes and a comment of up to 64 KiB.
    let from = b.len().checked_sub(22)?;
    let end = (from.saturating_sub(0xffff)..=from).rev().find(|&i| u32_at(b, i) == Some(0x0605_4b50))?;
    let (count, at) = (u16_at(b, end + 10)?, u32_at(b, end + 16)?);
    if count != 0xffff && at != 0xffff_ffff {
        return Some((count as usize, at as usize));
    }
    // Zip64: a locator just before the end record says where its own end record is.
    if u32_at(b, end.checked_sub(20)?) != Some(0x0706_4b50) {
        return None;
    }
    let end64 = usize::try_from(u64_at(b, end - 20 + 8)?).ok()?;
    if u32_at(b, end64) != Some(0x0606_4b50) {
        return None;
    }
    Some((usize::try_from(u64_at(b, end64 + 32)?).ok()?, usize::try_from(u64_at(b, end64 + 48)?).ok()?))
}

/// The central directory entry at `*p`, moving past it.
fn entry(b: &[u8], p: &mut usize) -> Option<Entry> {
    let e = *p;
    if u32_at(b, e)? != 0x0201_4b50 {
        return None;
    }
    let (n, x, c) = (u16_at(b, e + 28)? as usize, u16_at(b, e + 30)? as usize, u16_at(b, e + 32)? as usize);
    let name = String::from_utf8_lossy(b.get(e + 46..e + 46 + n)?).into_owned();
    let mut size = u64::from(u32_at(b, e + 24)?);
    let mut compressed = u64::from(u32_at(b, e + 20)?);
    let mut offset = u64::from(u32_at(b, e + 42)?);
    // Zip64 sizes: in the extra field, those that are all ones, in this order.
    let extra = b.get(e + 46 + n..e + 46 + n + x)?;
    let mut q = 0;
    while q + 4 <= extra.len() {
        let (id, len) = (u16_at(extra, q)?, u16_at(extra, q + 2)? as usize);
        if id == 1 {
            let mut f = q + 4;
            for v in [&mut size, &mut compressed, &mut offset] {
                if *v == 0xffff_ffff {
                    *v = u64_at(extra, f)?;
                    f += 8;
                }
            }
        }
        q += 4 + len;
    }
    if u16_at(b, e + 8)? & 1 != 0 {
        return None; // encrypted
    }
    *p = e + 46 + n + x + c;
    Some(Entry {
        name,
        method: u16_at(b, e + 10)?,
        compressed: usize::try_from(compressed).ok()?,
        size: usize::try_from(size).ok()?,
        offset: usize::try_from(offset).ok()?,
    })
}

/// An archive of `files`, deflated where it makes them smaller: for tests,
/// here and in the 3MF reader's.
#[cfg(test)]
pub(crate) fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let packed = termmd_deflate::deflate::deflate(data);
        let (method, body): (u16, &[u8]) = if packed.len() < data.len() { (8, &packed) } else { (0, data) };
        let fields = |out: &mut Vec<u8>| {
            out.extend(20u16.to_le_bytes()); // version needed
            out.extend(0u16.to_le_bytes()); // flags
            out.extend(method.to_le_bytes());
            out.extend([0u8; 8]); // time, date, CRC (not checked)
            out.extend((body.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes()); // extra
        };
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // made by
        fields(&mut central);
        central.extend([0u8; 10]); // comment, disk, internal and external attributes
        central.extend((out.len() as u32).to_le_bytes());
        central.extend(name.as_bytes());
        out.extend(0x0403_4b50u32.to_le_bytes());
        fields(&mut out);
        out.extend(name.as_bytes());
        out.extend(body);
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disks
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(3u16.to_le_bytes());
    out.extend(b"end");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_and_deflated() {
        let long = "vertex ".repeat(100);
        let a = archive(&[("[Content_Types].xml", b"<x/>"), ("3D/3dmodel.model", long.as_bytes())]);
        let z = Zip::open(&a).unwrap();
        assert_eq!(z.names().collect::<Vec<_>>(), ["[Content_Types].xml", "3D/3dmodel.model"]);
        assert_eq!(z.entries[1].method, 8, "deflated");
        assert_eq!(z.read("[Content_Types].xml").unwrap().unwrap(), b"<x/>");
        assert_eq!(z.read("3d/3DModel.model").unwrap().unwrap(), long.as_bytes(), "any case");
        assert!(z.read("missing").is_none());
    }

    #[test]
    fn broken() {
        assert_eq!(Zip::open(b"PK not really").unwrap_err().0, "not a zip: no end of central directory");
        let a = archive(&[("a", b"hello")]);
        let mut cut = a.clone();
        cut[0] = b'X'; // the local header's signature
        assert_eq!(Zip::open(&cut).unwrap().read("a").unwrap().unwrap_err().0, "a broken zip: a's local header");
        let mut cut = a.clone();
        let at = a.len() - 25 - 47; // the central entry's signature, before the end record and itself
        cut[at] = b'X';
        assert!(Zip::open(&cut).is_err());
    }
}
