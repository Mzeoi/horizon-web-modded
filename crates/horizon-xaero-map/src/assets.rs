#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::OnceLock;

const GUIS_PNG: &[u8] = include_bytes!("../assets/guis.png");
const MINIMAP_FRAME_PNG: &[u8] = include_bytes!("../assets/minimap_frame.png");
const WORLDMAP_GUI_PNG: &[u8] = include_bytes!("../assets/gui.png");

const MINIMAP_ZH_CN: &str = include_str!("../assets/lang/minimap_zh_cn.json");
const MINIMAP_EN_US: &str = include_str!("../assets/lang/minimap_en_us.json");
const WORLDMAP_EN_US: &str = include_str!("../assets/lang/worldmap_en_us.json");

#[derive(Clone, Debug)]
pub struct RgbaTexture {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 pixels (`width * height * 4`).
    pub rgba: Vec<u8>,
}

impl RgbaTexture {
    pub fn decode_png(bytes: &[u8]) -> Option<Self> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        let w = info.width;
        let h = info.height;
        let raw = &buf[..info.buffer_size()];
        let rgba = match info.color_type {
            png::ColorType::Rgba => raw.to_vec(),
            png::ColorType::Rgb => raw
                .chunks_exact(3)
                .flat_map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => raw
                .chunks_exact(2)
                .flat_map(|c| [c[0], c[0], c[0], c[1]])
                .collect(),
            png::ColorType::Grayscale => raw
                .iter()
                .flat_map(|&g| [g, g, g, 255])
                .collect(),
            _ => return None,
        };
        Some(Self {
            width: w,
            height: h,
            rgba,
        })
    }

    /// Blit a sub-rectangle `[sx, sy, sw, sh]` from this texture onto `dst` (`dst_w x dst_h`)
    /// at `[dx, dy]`, alpha-blended and optionally tinted by `argb`.
    pub fn blit_into(
        &self,
        src: [u32; 4],
        dst: &mut [u8],
        dst_w: u32,
        dst_h: u32,
        dx: i32,
        dy: i32,
        argb: u32,
    ) {
        let [sx, sy, sw, sh] = src;
        let [ta, tr, tg, tb] = argb.to_be_bytes();
        for row in 0..sh {
            let ty = dy + row as i32;
            if ty < 0 || ty >= dst_h as i32 || sy + row >= self.height {
                continue;
            }
            for col in 0..sw {
                let tx = dx + col as i32;
                if tx < 0 || tx >= dst_w as i32 || sx + col >= self.width {
                    continue;
                }
                let si = (((sy + row) * self.width + (sx + col)) * 4) as usize;
                let sr = (self.rgba[si] as u32 * tr as u32) / 255;
                let sg = (self.rgba[si + 1] as u32 * tg as u32) / 255;
                let sb = (self.rgba[si + 2] as u32 * tb as u32) / 255;
                let sa = (self.rgba[si + 3] as u32 * ta as u32) / 255;
                if sa == 0 {
                    continue;
                }
                let di = ((ty as u32 * dst_w + tx as u32) * 4) as usize;
                if sa == 255 {
                    dst[di] = sr as u8;
                    dst[di + 1] = sg as u8;
                    dst[di + 2] = sb as u8;
                    dst[di + 3] = 255;
                } else {
                    let inv = 255 - sa;
                    dst[di] = ((sr * sa + dst[di] as u32 * inv) / 255) as u8;
                    dst[di + 1] = ((sg * sa + dst[di + 1] as u32 * inv) / 255) as u8;
                    dst[di + 2] = ((sb * sa + dst[di + 2] as u32 * inv) / 255) as u8;
                    dst[di + 3] = (sa + (dst[di + 3] as u32 * inv) / 255).min(255) as u8;
                }
            }
        }
    }
}

pub struct XaeroAssets {
    pub guis: RgbaTexture,
    pub minimap_frame: RgbaTexture,
    pub worldmap_gui: RgbaTexture,
    lang_zh: HashMap<String, String>,
    lang_en: HashMap<String, String>,
}

static ASSETS: OnceLock<XaeroAssets> = OnceLock::new();

pub fn assets() -> &'static XaeroAssets {
    ASSETS.get_or_init(|| {
        let guis = RgbaTexture::decode_png(GUIS_PNG).expect("valid guis.png");
        let minimap_frame =
            RgbaTexture::decode_png(MINIMAP_FRAME_PNG).expect("valid minimap_frame.png");
        let worldmap_gui =
            RgbaTexture::decode_png(WORLDMAP_GUI_PNG).expect("valid worldmap gui.png");

        let lang_zh: HashMap<String, String> =
            serde_json::from_str(MINIMAP_ZH_CN).unwrap_or_default();
        let mut lang_en: HashMap<String, String> =
            serde_json::from_str(MINIMAP_EN_US).unwrap_or_default();
        if let Ok(wm_en) = serde_json::from_str::<HashMap<String, String>>(WORLDMAP_EN_US) {
            for (k, v) in wm_en {
                lang_en.entry(k).or_insert(v);
            }
        }


        XaeroAssets {
            guis,
            minimap_frame,
            worldmap_gui,
            lang_zh,
            lang_en,
        }
    })
}

/// Translate a Xaero lang key using English (`en_us`), falling back to the key itself.
pub fn tr(key: &str) -> &str {
    let a = assets();
    let _ = &a.lang_zh;
    a.lang_en
        .get(key)
        .map(String::as_str)
        .unwrap_or(key)
}
