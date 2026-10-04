// Text: system fonts (Segoe UI and friends) rasterised with fontdue and blended
// straight into the pixmap. No font ships with the app.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use ttf_parser::{Face as TtfFace, GlyphId, OutlineBuilder};

use crate::gfx::Gfx;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    Regular,
    Medium,
    Bold,
    Mono,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Every font that has been loaded, in one arena. Faces and fallbacks load the
/// first time a glyph needs them: parsing a CJK collection costs hundreds of
/// megabytes, so it must never happen unless a character really requires it.
struct Fonts {
    fonts: Vec<TtfFace<'static>>,
    by_face: HashMap<Face, Option<usize>>,
    /// Fallback fonts by position in `FALLBACKS`: None = not tried yet, Some(None) = unavailable.
    fallback: Vec<Option<Option<usize>>>,
}

const FALLBACKS: [(&str, u32); 4] = [("seguisym.ttf", 0), ("msyh.ttc", 0), ("YuGothM.ttc", 0), ("malgun.ttf", 0)];

thread_local! {
    static FONTS: RefCell<Fonts> = RefCell::new(Fonts {
        fonts: Vec::new(),
        by_face: HashMap::new(),
        fallback: vec![None; FALLBACKS.len()],
    });
    static CACHE: RefCell<HashMap<(usize, u16, u32), Rc<(GlyphMetrics, Vec<u8>)>>> =
        RefCell::new(HashMap::new());
    /// Advance widths by (face, size×4, char): measuring is called far more often than drawing.
    static ADVANCE: RefCell<HashMap<(Face, u32, char), f32>> = RefCell::new(HashMap::new());
}

/// Placement of a rasterised glyph, same convention as most rasterisers:
/// `xmin` from the pen, `ymin` from the baseline to the bitmap's bottom edge.
#[derive(Clone, Copy, Default)]
struct GlyphMetrics {
    xmin: i32,
    ymin: i32,
    width: usize,
    height: usize,
    advance_width: f32,
}

/// Font files are read once and kept for the life of the process, so the parsed
/// face can borrow them. Nothing is expanded: `ttf-parser` reads tables in place.
fn load(file: &str, index: u32) -> Option<TtfFace<'static>> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let bytes = std::fs::read(format!(r"{windir}\Fonts\{file}")).ok()?;
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    TtfFace::parse(leaked, index).ok()
}

fn glyph_index(f: &TtfFace<'static>, c: char) -> u16 {
    f.glyph_index(c).map(|g| g.0).unwrap_or(0)
}

fn advance_of(f: &TtfFace<'static>, gid: u16, size: f32) -> f32 {
    let upem = f.units_per_em() as f32;
    f.glyph_hor_advance(GlyphId(gid)).unwrap_or(0) as f32 * size / upem
}

struct Outline(PathBuilder);

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// Coverage bitmap for one glyph at `px` pixels, drawn with tiny-skia.
fn rasterize(f: &TtfFace<'static>, gid: u16, px: f32) -> (GlyphMetrics, Vec<u8>) {
    let upem = f.units_per_em() as f32;
    let sc = px / upem;
    let advance_width = f.glyph_hor_advance(GlyphId(gid)).unwrap_or(0) as f32 * sc;
    let mut out = Outline(PathBuilder::new());
    let Some(b) = f.outline_glyph(GlyphId(gid), &mut out) else {
        return (GlyphMetrics { advance_width, ..Default::default() }, Vec::new());
    };
    let xmin = (b.x_min as f32 * sc).floor() as i32 - 1;
    let ymin = (b.y_min as f32 * sc).floor() as i32 - 1;
    let xmax = (b.x_max as f32 * sc).ceil() as i32 + 1;
    let ymax = (b.y_max as f32 * sc).ceil() as i32 + 1;
    let (w, h) = ((xmax - xmin).max(1) as u32, (ymax - ymin).max(1) as u32);
    let (Some(path), Some(mut pm)) = (out.0.finish(), Pixmap::new(w, h)) else {
        return (GlyphMetrics { advance_width, ..Default::default() }, Vec::new());
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    // Font units (y up) → bitmap pixels (y down), origin at the bitmap's top-left.
    let tf = Transform::from_row(sc, 0.0, 0.0, -sc, -xmin as f32, ymax as f32);
    pm.fill_path(&path, &paint, FillRule::Winding, tf, None);
    let cov: Vec<u8> = pm.data().chunks_exact(4).map(|p| p[3]).collect();
    (GlyphMetrics { xmin, ymin, width: w as usize, height: h as usize, advance_width }, cov)
}

fn face_files(face: Face) -> &'static [&'static str] {
    match face {
        Face::Regular => &["segoeui.ttf", "arial.ttf"],
        Face::Medium => &["seguisb.ttf", "segoeuib.ttf", "arial.ttf"],
        Face::Bold => &["segoeuib.ttf", "arialbd.ttf"],
        Face::Mono => &["consola.ttf", "cour.ttf", "segoeui.ttf"],
    }
}

impl Fonts {
    fn face_font(&mut self, face: Face) -> Option<usize> {
        if let Some(id) = self.by_face.get(&face) {
            return *id;
        }
        let mut id = None;
        for name in face_files(face) {
            if let Some(f) = load(name, 0) {
                self.fonts.push(f);
                id = Some(self.fonts.len() - 1);
                break;
            }
        }
        self.by_face.insert(face, id);
        id
    }

    fn fallback_font(&mut self, i: usize) -> Option<usize> {
        if let Some(v) = self.fallback[i] {
            return v;
        }
        let (file, idx) = FALLBACKS[i];
        let id = load(file, idx).map(|f| {
            self.fonts.push(f);
            self.fonts.len() - 1
        });
        self.fallback[i] = Some(id);
        id
    }

    /// The font and glyph that draw `c` in `face`.
    fn resolve(&mut self, face: Face, c: char) -> Option<(usize, u16)> {
        let primary = self.face_font(face)?;
        let g = glyph_index(&self.fonts[primary], c);
        if g != 0 || c.is_whitespace() {
            return Some((primary, g));
        }
        for i in 0..FALLBACKS.len() {
            if let Some(id) = self.fallback_font(i) {
                let g = glyph_index(&self.fonts[id], c);
                if g != 0 {
                    return Some((id, g));
                }
            }
        }
        Some((primary, 0))
    }
}

/// Advance of `text` in logical pixels at `size`.
pub fn measure(text: &str, face: Face, size: f32) -> f32 {
    let q = (size * 4.0).round() as u32;
    let mut w = 0.0;
    for c in text.chars() {
        let hit = ADVANCE.with(|a| a.borrow().get(&(face, q, c)).copied());
        let adv = match hit {
            Some(v) => v,
            None => {
                let v = FONTS.with(|f| {
                    let mut f = f.borrow_mut();
                    f.resolve(face, c).map(|(id, g)| advance_of(&f.fonts[id], g, size)).unwrap_or(0.0)
                });
                ADVANCE.with(|a| a.borrow_mut().insert((face, q, c), v));
                v
            }
        };
        w += adv;
    }
    w
}

/// Greedy word wrap to `max_w` logical pixels. Explicit newlines are honoured.
pub fn wrap(text: &str, face: Face, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if measure(&candidate, face, size) <= max_w || line.is_empty() {
                line = candidate;
                // A single word wider than the line is broken by character.
                while measure(&line, face, size) > max_w && line.chars().count() > 1 {
                    let mut cut = line.chars().count() - 1;
                    while cut > 1 && measure(&line.chars().take(cut).collect::<String>(), face, size) > max_w {
                        cut -= 1;
                    }
                    let head: String = line.chars().take(cut).collect();
                    let tail: String = line.chars().skip(cut).collect();
                    lines.push(head);
                    line = tail;
                }
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        lines.push(line);
    }
    lines
}

/// Truncates with an ellipsis so that the result fits `max_w`.
pub fn ellipsize(text: &str, face: Face, size: f32, max_w: f32) -> String {
    if measure(text, face, size) <= max_w {
        return text.to_string();
    }
    let mut out: Vec<char> = text.chars().collect();
    while !out.is_empty() {
        out.pop();
        let s: String = out.iter().collect::<String>() + "…";
        if measure(&s, face, size) <= max_w {
            return s;
        }
    }
    "…".into()
}

/// Draws one line. `y` is the vertical centre of the line.
pub fn draw(g: &mut Gfx, text: &str, x: f32, y: f32, face: Face, size: f32, color: Color, align: Align) {
    draw_with(g, text, x, y, face, size, align, |_| color);
}

/// Like `draw`, but each glyph asks `color_at(fraction along the line)` for its colour
/// — the shimmer on the ticker is done this way.
pub fn draw_with(
    g: &mut Gfx, text: &str, x: f32, y: f32, face: Face, size: f32, align: Align,
    color_at: impl Fn(f32) -> Color,
) {
    if text.is_empty() {
        return;
    }
    let width = measure(text, face, size);
    let x0 = match align {
        Align::Left => x,
        Align::Center => x - width / 2.0,
        Align::Right => x - width,
    };
    let tf = g.transform();
    let scale = tf.sx.max(0.01);
    let px_size = size * scale;
    let alpha = g.alpha();
    if alpha <= 0.0 {
        return;
    }

    // Device position of the pen and the baseline.
    let mut pen = x0 * tf.sx + tf.tx;
    let baseline = ((y + size * 0.36) * tf.sy + tf.ty).round();
    let (data, pw, ph, mask) = g.pixels_and_mask();
    let (pw, ph) = (pw as i32, ph as i32);

    let mut adv = 0.0f32;
    for c in text.chars() {
        let resolved = FONTS.with(|f| f.borrow_mut().resolve(face, c));
        let Some((fi, gi)) = resolved else { continue };
        let color = color_at(if width > 0.0 { adv / width } else { 0.0 });
        let (r, gr, b) = (color.red(), color.green(), color.blue());
        let a = color.alpha() * alpha;
        let key = (fi, gi, (px_size * 4.0).round() as u32);
        let glyph = CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            // Animated scales produce a stream of odd sizes; keep the cache bounded.
            if cache.len() > 4000 {
                cache.clear();
            }
            if let Some(g) = cache.get(&key) {
                return g.clone();
            }
            let g = Rc::new(FONTS.with(|f| rasterize(&f.borrow().fonts[fi], gi, px_size)));
            cache.insert(key, g.clone());
            g
        });
        let (metrics, bitmap) = (&glyph.0, &glyph.1);
        let gx = (pen + metrics.xmin as f32).round() as i32;
        let gy = baseline as i32 - metrics.ymin - metrics.height as i32;
        blit(data, pw, ph, bitmap, metrics.width as i32, metrics.height as i32, gx, gy, (r, gr, b), a, mask);
        pen += metrics.advance_width;
        adv += metrics.advance_width / scale;
    }
}

#[allow(clippy::too_many_arguments)]
fn blit(
    dst: &mut [u8], pw: i32, ph: i32,
    cov: &[u8], w: i32, h: i32, x0: i32, y0: i32,
    rgb: (f32, f32, f32), alpha: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    for row in 0..h {
        let y = y0 + row;
        if y < 0 || y >= ph {
            continue;
        }
        for col in 0..w {
            let x = x0 + col;
            if x < 0 || x >= pw {
                continue;
            }
            let mut c = cov[(row * w + col) as usize] as f32 / 255.0;
            if c <= 0.0 {
                continue;
            }
            if let Some(m) = mask {
                c *= m.data()[(y as u32 * m.width() + x as u32) as usize] as f32 / 255.0;
            }
            let sa = c * alpha;
            if sa <= 0.0 {
                continue;
            }
            let i = ((y * pw + x) * 4) as usize;
            let inv = 1.0 - sa;
            dst[i] = (rgb.0 * sa * 255.0 + dst[i] as f32 * inv).round().min(255.0) as u8;
            dst[i + 1] = (rgb.1 * sa * 255.0 + dst[i + 1] as f32 * inv).round().min(255.0) as u8;
            dst[i + 2] = (rgb.2 * sa * 255.0 + dst[i + 2] as f32 * inv).round().min(255.0) as u8;
            dst[i + 3] = (sa * 255.0 + dst[i + 3] as f32 * inv).round().min(255.0) as u8;
        }
    }
}
