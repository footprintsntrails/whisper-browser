//! Icons, the README banner and the Windows installer artwork, rendered from the logo SVGs
//! with resvg. The results are committed; rerun these after changing the logo.

use std::fs;
use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};

use crate::fsutil::{self, err};
use crate::{Opts, root};

const BLACK: &str = "#111316";
const PLATINUM: &str = "#D3D9DF";
const MUTED: &str = "#8E98A3";
const SLOGAN: &str = "Whisper can\u{2019}t make you invisible. It just won\u{2019}t snitch.";

fn logo(name: &str) -> Result<String, String> {
    let path = root().join("assets/logo").join(name);
    fs::read_to_string(&path).map_err(err(path.display()))
}

/// Renders SVG markup to a `width`×`height` pixmap, with fonts from `font_dir`.
fn render(
    svg: &str,
    width: u32,
    height: u32,
    font_dir: Option<&Path>,
) -> Result<tiny_skia::Pixmap, String> {
    let mut opt = usvg::Options::default();
    if let Some(dir) = font_dir {
        for entry in fs::read_dir(dir).map_err(err(dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "ttf" || e == "otf") {
                opt.fontdb_mut()
                    .load_font_data(fs::read(&path).map_err(err(path.display()))?);
            }
        }
        if opt.fontdb.faces().count() == 0 {
            return Err(format!("no fonts found in {}", dir.display()));
        }
    }
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("bad size")?;
    let size = tree.size();
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

/// Straight (not premultiplied) RGBA bytes.
fn rgba(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn save_png(pixmap: &tiny_skia::Pixmap, path: &Path) -> Result<(), String> {
    fsutil::write(path, pixmap.encode_png().map_err(|e| e.to_string())?)
}

/// assets/icon: PNGs, whisper.ico (Windows), whisper.icns (macOS) and whisper.svg (Linux).
/// The 16 and 24 px sizes use the hand-drawn small SVGs, not a downscale.
pub fn icons() -> Result<(), String> {
    let out = root().join("assets/icon");
    let master = logo("master.svg")?;
    let size_svg = |s: u32| -> Result<String, String> {
        match s {
            16 => logo("small-16.svg"),
            24 => logo("small-24.svg"),
            _ => Ok(master.clone()),
        }
    };
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for s in [16, 24, 32, 48, 64, 128, 256] {
        let pixmap = render(&size_svg(s)?, s, s, None)?;
        let image = ico::IconImage::from_rgba_data(s, s, rgba(&pixmap));
        // Classic layout: bitmaps for the small sizes, PNG for 256 px
        let entry = if s < 256 {
            ico::IconDirEntry::encode_as_bmp(&image)
        } else {
            ico::IconDirEntry::encode_as_png(&image)
        }
        .map_err(|e| e.to_string())?;
        icon_dir.add_entry(entry);
    }
    let mut bytes = Vec::new();
    icon_dir.write(&mut bytes).map_err(|e| e.to_string())?;
    fsutil::write(&out.join("whisper.ico"), bytes)?;

    let mut family = icns::IconFamily::new();
    for s in [16, 32, 64, 128, 256, 512, 1024] {
        let pixmap = render(&size_svg(s)?, s, s, None)?;
        let image = icns::Image::from_data(icns::PixelFormat::RGBA, s, s, rgba(&pixmap))
            .map_err(|e| e.to_string())?;
        family.add_icon(&image).map_err(|e| format!("{s}px: {e}"))?;
    }
    let mut bytes = Vec::new();
    family.write(&mut bytes).map_err(|e| e.to_string())?;
    fsutil::write(&out.join("whisper.icns"), bytes)?;

    for s in [256, 512] {
        save_png(
            &render(&master, s, s, None)?,
            &out.join(format!("whisper-{s}.png")),
        )?;
    }
    fsutil::write(&out.join("whisper.svg"), &master)?;
    println!("icons written to {}", out.display());
    Ok(())
}

/// The logo as an element that can be placed inside a larger SVG.
fn embedded_logo(x: f32, y: f32, size: f32) -> Result<String, String> {
    let master = logo("master.svg")?;
    let start = master.find("<svg").ok_or("master.svg has no <svg>")?;
    let open_end = master[start..].find('>').ok_or("bad master.svg")? + start;
    let inner = &master[open_end + 1..master.rfind("</svg>").ok_or("bad master.svg")?];
    Ok(format!(
        r#"<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 256 256">{inner}</svg>"#
    ))
}

fn font_dir(opts: &Opts) -> Result<PathBuf, String> {
    let dir = opts.path("font-dir")?;
    for weight in ["Regular", "SemiBold"] {
        if !dir.join(format!("Inter-{weight}.ttf")).is_file() {
            return Err(format!("Inter-{weight}.ttf is not in {}", dir.display()));
        }
    }
    Ok(dir)
}

/// assets/readme/banner.png: the logo, the name and the slogan on Whisper black.
pub fn banner(opts: &Opts) -> Result<(), String> {
    let fonts = font_dir(opts)?;
    let (w, h) = (1280.0, 400.0);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
  <defs>
    <radialGradient id="glow" cx="0.22" cy="0.5" r="0.62">
      <stop offset="0" stop-color="#1D2126"/>
      <stop offset="1" stop-color="{BLACK}"/>
    </radialGradient>
  </defs>
  <rect width="{w}" height="{h}" rx="28" fill="url(#glow)"/>
  <rect x="0.75" y="0.75" width="{iw}" height="{ih}" rx="27.25" fill="none" stroke="#FFFFFF" stroke-opacity="0.07" stroke-width="1.5"/>
  {logo}
  <text x="372" y="214" font-family="Inter" font-weight="600" font-size="112" letter-spacing="-3" fill="{PLATINUM}">Whisper</text>
  <text x="376" y="276" font-family="Inter" font-weight="400" font-size="31" letter-spacing="-0.2" fill="{MUTED}">{SLOGAN}</text>
</svg>"##,
        iw = w - 1.5,
        ih = h - 1.5,
        logo = embedded_logo(104.0, 80.0, 240.0)?,
    );
    let pixmap = render(&svg, w as u32, h as u32, Some(&fonts))?;
    let out = root().join("assets/readme/banner.png");
    save_png(&pixmap, &out)?;
    println!("{}", out.display());
    Ok(())
}

/// The Windows installer's welcome/finish sidebar (164×314) and page header (150×57),
/// as 24-bit BMP files, which is what NSIS needs.
pub fn installer_art(opts: &Opts) -> Result<(), String> {
    let fonts = font_dir(opts)?;
    let sidebar = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="164" height="314" viewBox="0 0 164 314">
  <defs>
    <radialGradient id="glow" cx="0.5" cy="0.32" r="0.7">
      <stop offset="0" stop-color="#1D2126"/>
      <stop offset="1" stop-color="{BLACK}"/>
    </radialGradient>
  </defs>
  <rect width="164" height="314" fill="url(#glow)"/>
  {logo}
  <text x="82" y="178" text-anchor="middle" font-family="Inter" font-weight="600" font-size="27" letter-spacing="-0.6" fill="{PLATINUM}">Whisper</text>
  <text x="82" y="270" text-anchor="middle" font-family="Inter" font-weight="400" font-size="10.5" fill="{MUTED}">It can&#8217;t make you invisible.</text>
  <text x="82" y="285" text-anchor="middle" font-family="Inter" font-weight="400" font-size="10.5" fill="{MUTED}">It just won&#8217;t snitch.</text>
</svg>"##,
        logo = embedded_logo(42.0, 56.0, 80.0)?,
    );
    let header = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="150" height="57" viewBox="0 0 150 57">
  <rect width="150" height="57" fill="#FFFFFF"/>
  {logo}
</svg>"##,
        logo = embedded_logo(98.0, 6.0, 45.0)?,
    );
    let out = root().join("assets/installer");
    for (svg, w, h, name) in [
        (sidebar, 164, 314, "sidebar.bmp"),
        (header, 150, 57, "header.bmp"),
    ] {
        let pixmap = render(&svg, w, h, Some(&fonts))?;
        fsutil::write(&out.join(name), bmp24(&pixmap))?;
        save_png(&pixmap, &crate::build().join(name.replace(".bmp", ".png")))?;
    }
    println!("installer artwork written to {}", out.display());
    Ok(())
}

/// An uncompressed 24-bit BMP (opaque images only).
fn bmp24(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&((row * h) as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    let pixels = pixmap.pixels();
    for y in (0..h).rev() {
        for x in 0..w {
            let c = pixels[y * w + x].demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red()]);
        }
        out.extend(std::iter::repeat_n(0, row - w * 3));
    }
    out
}
