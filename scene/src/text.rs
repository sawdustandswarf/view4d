//! Text on a frame, in a 5×7 bitmap font of printable ASCII built in: for
//! the overlay, where a font file and a rasterizer would be a lot for a
//! line of numbers. Anything else shows as `?`.

use crate::raster::Frame;

/// A glyph's size in font pixels, and the advance and line height.
pub const GLYPH: (u32, u32) = (5, 7);
const ADVANCE: u32 = 6;
const LINE: u32 = 9;

/// `' '` to `'~'`, 7 rows of 5 each, top first: `#` set, `.` clear.
const GLYPHS: [&str; 95] = [
    "..... ..... ..... ..... ..... ..... .....", // space
    "..#.. ..#.. ..#.. ..#.. ..#.. ..... ..#..", // !
    ".#.#. .#.#. .#.#. ..... ..... ..... .....", // "
    ".#.#. .#.#. ##### .#.#. ##### .#.#. .#.#.", // #
    "..#.. .#### #.#.. .###. ..#.# ####. ..#..", // $
    "##... ##..# ...#. ..#.. .#... #..## ...##", // %
    ".##.. #..#. #.#.. .#... #.#.# #..#. .##.#", // &
    "..#.. ..#.. ..... ..... ..... ..... .....", // '
    "...#. ..#.. .#... .#... .#... ..#.. ...#.", // (
    ".#... ..#.. ...#. ...#. ...#. ..#.. .#...", // )
    "..... ..#.. #.#.# .###. #.#.# ..#.. .....", // *
    "..... ..#.. ..#.. ##### ..#.. ..#.. .....", // +
    "..... ..... ..... ..... .##.. ..#.. .#...", // ,
    "..... ..... ..... ##### ..... ..... .....", // -
    "..... ..... ..... ..... ..... .##.. .##..", // .
    "..... ....# ...#. ..#.. .#... #.... .....", // /
    ".###. #...# #..## #.#.# ##..# #...# .###.", // 0
    "..#.. .##.. ..#.. ..#.. ..#.. ..#.. .###.", // 1
    ".###. #...# ....# ...#. ..#.. .#... #####", // 2
    "##### ...#. ..#.. ...#. ....# #...# .###.", // 3
    "...#. ..##. .#.#. #..#. ##### ...#. ...#.", // 4
    "##### #.... ####. ....# ....# #...# .###.", // 5
    "..##. .#... #.... ####. #...# #...# .###.", // 6
    "##### ....# ...#. ..#.. .#... .#... .#...", // 7
    ".###. #...# #...# .###. #...# #...# .###.", // 8
    ".###. #...# #...# .#### ....# ...#. .##..", // 9
    "..... .##.. .##.. ..... .##.. .##.. .....", // :
    "..... .##.. .##.. ..... .##.. ..#.. .#...", // ;
    "...#. ..#.. .#... #.... .#... ..#.. ...#.", // <
    "..... ..... ##### ..... ##### ..... .....", // =
    ".#... ..#.. ...#. ....# ...#. ..#.. .#...", // >
    ".###. #...# ....# ...#. ..#.. ..... ..#..", // ?
    ".###. #...# ....# .##.# #.#.# #.#.# .###.", // @
    ".###. #...# #...# ##### #...# #...# #...#", // A
    "####. #...# #...# ####. #...# #...# ####.", // B
    ".###. #...# #.... #.... #.... #...# .###.", // C
    "###.. #..#. #...# #...# #...# #..#. ###..", // D
    "##### #.... #.... ####. #.... #.... #####", // E
    "##### #.... #.... ####. #.... #.... #....", // F
    ".###. #...# #.... #.### #...# #...# .####", // G
    "#...# #...# #...# ##### #...# #...# #...#", // H
    ".###. ..#.. ..#.. ..#.. ..#.. ..#.. .###.", // I
    "..### ...#. ...#. ...#. ...#. #..#. .##..", // J
    "#...# #..#. #.#.. ##... #.#.. #..#. #...#", // K
    "#.... #.... #.... #.... #.... #.... #####", // L
    "#...# ##.## #.#.# #.#.# #...# #...# #...#", // M
    "#...# #...# ##..# #.#.# #..## #...# #...#", // N
    ".###. #...# #...# #...# #...# #...# .###.", // O
    "####. #...# #...# ####. #.... #.... #....", // P
    ".###. #...# #...# #...# #.#.# #..#. .##.#", // Q
    "####. #...# #...# ####. #.#.. #..#. #...#", // R
    ".#### #.... #.... .###. ....# ....# ####.", // S
    "##### ..#.. ..#.. ..#.. ..#.. ..#.. ..#..", // T
    "#...# #...# #...# #...# #...# #...# .###.", // U
    "#...# #...# #...# #...# #...# .#.#. ..#..", // V
    "#...# #...# #...# #.#.# #.#.# #.#.# .#.#.", // W
    "#...# #...# .#.#. ..#.. .#.#. #...# #...#", // X
    "#...# #...# .#.#. ..#.. ..#.. ..#.. ..#..", // Y
    "##### ....# ...#. ..#.. .#... #.... #####", // Z
    ".###. .#... .#... .#... .#... .#... .###.", // [
    "..... #.... .#... ..#.. ...#. ....# .....", // \
    ".###. ...#. ...#. ...#. ...#. ...#. .###.", // ]
    "..#.. .#.#. #...# ..... ..... ..... .....", // ^
    "..... ..... ..... ..... ..... ..... #####", // _
    ".#... ..#.. ..... ..... ..... ..... .....", // `
    "..... ..... .###. ....# .#### #...# .####", // a
    "#.... #.... #.##. ##..# #...# #...# ####.", // b
    "..... ..... .###. #.... #.... #...# .###.", // c
    "....# ....# .##.# #..## #...# #...# .####", // d
    "..... ..... .###. #...# ##### #.... .###.", // e
    "..##. .#..# .#... ###.. .#... .#... .#...", // f
    "..... .#### #...# #...# .#### ....# .###.", // g
    "#.... #.... #.##. ##..# #...# #...# #...#", // h
    "..#.. ..... .##.. ..#.. ..#.. ..#.. .###.", // i
    "...#. ..... ..##. ...#. ...#. #..#. .##..", // j
    "#.... #.... #..#. #.#.. ##... #.#.. #..#.", // k
    ".##.. ..#.. ..#.. ..#.. ..#.. ..#.. .###.", // l
    "..... ..... ##.#. #.#.# #.#.# #...# #...#", // m
    "..... ..... #.##. ##..# #...# #...# #...#", // n
    "..... ..... .###. #...# #...# #...# .###.", // o
    "..... ..... ####. #...# ####. #.... #....", // p
    "..... ..... .##.# #..## .#### ....# ....#", // q
    "..... ..... #.##. ##..# #.... #.... #....", // r
    "..... ..... .###. #.... .###. ....# ####.", // s
    ".#... .#... ###.. .#... .#... .#..# ..##.", // t
    "..... ..... #...# #...# #...# #..## .##.#", // u
    "..... ..... #...# #...# #...# .#.#. ..#..", // v
    "..... ..... #...# #...# #.#.# #.#.# .#.#.", // w
    "..... ..... #...# .#.#. ..#.. .#.#. #...#", // x
    "..... ..... #...# #...# .#### ....# .###.", // y
    "..... ..... ##### ...#. ..#.. .#... #####", // z
    "...#. ..#.. ..#.. .#... ..#.. ..#.. ...#.", // {
    "..#.. ..#.. ..#.. ..#.. ..#.. ..#.. ..#..", // |
    ".#... ..#.. ..#.. ...#. ..#.. ..#.. .#...", // }
    "..... ..... .#... #.#.# ...#. ..... .....", // ~
];

/// `c`'s rows, top first, bit 4 the leftmost column.
fn glyph(c: char) -> [u8; 7] {
    let i = if (' '..='~').contains(&c) { c as usize - ' ' as usize } else { '?' as usize - ' ' as usize };
    let mut rows = [0u8; 7];
    for (row, bits) in rows.iter_mut().zip(GLYPHS[i].split(' ')) {
        *row = bits.bytes().fold(0, |r, b| r << 1 | (b == b'#') as u8);
    }
    rows
}

/// The size `text` takes at `scale` screen pixels a font pixel: lines
/// split at `\n`, no space after the last line or glyph.
pub fn size(text: &str, scale: u32) -> (u32, u32) {
    let lines = text.split('\n');
    let cols = lines.clone().map(|l| l.chars().count() as u32).max().unwrap_or(0);
    let rows = lines.count() as u32;
    let w = (cols * ADVANCE).saturating_sub(ADVANCE - GLYPH.0);
    let h = (rows * LINE).saturating_sub(LINE - GLYPH.1);
    (w * scale, h * scale)
}

/// Lines `scale` apart, top to top.
pub fn line_height(scale: u32) -> u32 {
    LINE * scale
}

/// How many glyphs fit in `width` pixels at `scale`.
pub fn columns(width: u32, scale: u32) -> usize {
    ((width / scale.max(1) + ADVANCE - GLYPH.0) / ADVANCE) as usize
}

/// `text` in lines of at most `cols` glyphs, broken after a space where
/// there is one, else wherever it must.
pub fn wrap(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut lines = Vec::new();
    let mut rest: Vec<char> = text.chars().collect();
    while rest.len() > cols {
        let cut = rest[..=cols].iter().rposition(|&c| c == ' ').filter(|&i| i > 0).unwrap_or(cols);
        lines.push(rest[..cut].iter().collect::<String>().trim_end().to_string());
        rest.drain(..cut);
        while rest.first() == Some(&' ') {
            rest.remove(0);
        }
    }
    lines.push(rest.into_iter().collect());
    lines
}

impl Frame {
    /// Write `text` with its top left at `(x, y)`, `scale` pixels a font
    /// pixel, in `rgb`; clipped to the frame.
    pub fn text(&mut self, x: i32, y: i32, scale: u32, text: &str, rgb: [u8; 3]) {
        let s = scale.max(1) as i32;
        for (n, line) in text.split('\n').enumerate() {
            let top = y + n as i32 * LINE as i32 * s;
            for (k, c) in line.chars().enumerate() {
                let left = x + k as i32 * ADVANCE as i32 * s;
                for (r, bits) in glyph(c).iter().enumerate() {
                    for col in 0..GLYPH.0 as i32 {
                        if bits >> (4 - col) & 1 != 0 {
                            self.rect(left + col * s, top + r as i32 * s, s as u32, s as u32, rgb, 255);
                        }
                    }
                }
            }
        }
    }

    /// Fill a `w`×`h` rectangle at `(x, y)` with `rgb`, `alpha` of 255
    /// over what is there; clipped to the frame.
    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, rgb: [u8; 3], alpha: u8) {
        let x0 = x.clamp(0, self.width as i32) as u32;
        let y0 = y.clamp(0, self.height as i32) as u32;
        let x1 = (x + w as i32).clamp(0, self.width as i32) as u32;
        let y1 = (y + h as i32).clamp(0, self.height as i32) as u32;
        let a = alpha as u32;
        let bgr = [rgb[2], rgb[1], rgb[0]];
        for yy in y0..y1 {
            let row = (yy * self.width) as usize * 4;
            for xx in x0..x1 {
                let p = &mut self.pixels[row + xx as usize * 4..][..3];
                for (d, s) in p.iter_mut().zip(bgr) {
                    *d = ((s as u32 * a + *d as u32 * (255 - a) + 127) / 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_seven_rows_of_five() {
        for (i, g) in GLYPHS.iter().enumerate() {
            let rows: Vec<&str> = g.split(' ').collect();
            let c = (b' ' + i as u8) as char;
            assert_eq!(rows.len(), 7, "{c:?}");
            assert!(rows.iter().all(|r| r.len() == 5 && r.bytes().all(|b| b == b'#' || b == b'.')), "{c:?}");
        }
        assert_eq!(glyph(' '), [0; 7]);
        assert_eq!(glyph('é'), glyph('?'));
        assert_eq!(glyph('T')[0], 0b11111);
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap("short", 10), ["short"]);
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("ab  cd", 3), ["ab", "cd"]);
        assert_eq!(columns(11, 1), 2, "two glyphs and the space between");
        assert_eq!(columns(10, 1), 1);
        assert_eq!(columns(22, 2), 2);
    }

    #[test]
    fn sizes() {
        assert_eq!(size("", 1), (0, 7));
        assert_eq!(size("ab", 1), (11, 7));
        assert_eq!(size("abc\nd", 2), (34, 32));
    }

    #[test]
    fn text_lands_where_asked_and_clips() {
        let mut f = Frame::new(20, 10);
        let ground = f.rgb(0, 0);
        f.text(1, 1, 1, "|", [255, 0, 0]);
        assert_eq!(f.rgb(3, 1), [255, 0, 0], "the bar of |, in its middle column");
        assert_eq!(f.rgb(2, 1), ground, "beside it, the ground");
        // Off every edge: nothing panics.
        f.text(-3, -3, 2, "Hello", [255; 3]);
        f.text(18, 8, 3, "W", [255; 3]);
        f.rect(5, 5, 100, 100, [0, 0, 0], 128);
        assert!(f.rgb(19, 9)[0] < 200, "half over");
    }
}
