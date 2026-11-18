//! Writing a frame as PNG: RGB, deflate's stored blocks (no compression,
//! so no compressor to write), with the CRCs and Adler-32 the format wants.

use crate::raster::Frame;

/// `frame` as a PNG file.
pub fn encode(frame: &Frame) -> Vec<u8> {
    let (w, h) = (frame.width, frame.height);
    // Each row: filter type 0, then RGB.
    let mut raw = Vec::with_capacity(h as usize * (1 + w as usize * 3));
    for row in frame.pixels.chunks_exact(w as usize * 4) {
        raw.push(0);
        for p in row.as_chunks::<4>().0 {
            raw.extend([p[2], p[1], p[0]]);
        }
    }
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]); // 8-bit RGB, deflate, no filter choice, no interlace
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// `data` as a zlib stream of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if data.is_empty() { vec![&[]] } else { data.chunks(0xffff).collect() };
    for (i, b) in blocks.iter().enumerate() {
        out.push((i + 1 == blocks.len()) as u8); // BFINAL, BTYPE 00
        let len = b.len() as u16;
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend(*b);
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn a_frame_reads_back() {
        let mut f = Frame::new(3, 2);
        f.pixels[..4].copy_from_slice(&[1, 2, 3, 255]); // BGRA
        let png = encode(&f);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
        assert!(png.ends_with(&[0xae, 0x42, 0x60, 0x82]), "IEND's CRC");
        // The one stored block holds the rows as written.
        let idat = 8 + 25;
        assert_eq!(&png[idat + 4..idat + 8], b"IDAT");
        let data = &png[idat + 8..];
        assert_eq!(&data[..3], &[0x78, 0x01, 1]);
        assert_eq!(u16::from_le_bytes([data[3], data[4]]), 2 * (1 + 3 * 3));
        assert_eq!(&data[7..11], &[0, 3, 2, 1], "filter 0, then the first pixel as RGB");
    }
}
