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

/// The tile colour of Whisper Canary's icon: the inverse logo (light tile, black face)
/// with the platinum tile in pastel yellow.
const CANARY_TILE: &str = "#EADCA0";
/// The face of the inverse logo.
const INVERSE_FACE: &str = "#0B0D0F";

/// One look of the icon: the 256 px master and the hand-drawn 16 and 24 px versions.
struct Logos {
    master: String,
    small16: String,
    small24: String,
}

impl Logos {
    fn release() -> Result<Logos, String> {
        Ok(Logos {
            master: logo("master.svg")?,
            small16: logo("small-16.svg")?,
            small24: logo("small-24.svg")?,
        })
    }

    /// Whisper Canary: master-inv.svg, and the small sizes inverted the same way
    /// (platinum tile, black face), then the platinum tile colours (#D3D9DF and the
    /// gradient stops #DDE2E7 and #C7CED5) replaced by the canary yellow.
    fn canary() -> Result<Logos, String> {
        let master = recolour(
            &logo("master-inv.svg")?,
            &[
                ("#DDE2E7", CANARY_TILE),
                ("#C7CED5", CANARY_TILE),
                ("#D3D9DF", CANARY_TILE),
            ],
        )?
        .replace("(inverse)", "(Whisper Canary)");
        let small = |name: &str, size: &str| -> Result<String, String> {
            // small-*.svg: black tile (#111316), platinum face (#D3D9DF).
            Ok(recolour(
                &logo(name)?,
                &[("#D3D9DF", INVERSE_FACE), ("#111316", CANARY_TILE)],
            )?
            .replace(
                &format!("v3, {size}"),
                &format!("v3 (Whisper Canary), {size}"),
            ))
        };
        let logos = Logos {
            master,
            small16: small("small-16.svg", "16 px")?,
            small24: small("small-24.svg", "24 px")?,
        };
        for svg in [&logos.master, &logos.small16, &logos.small24] {
            for platinum in ["#D3D9DF", "#DDE2E7", "#C7CED5", "#111316"] {
                if svg.contains(platinum) {
                    return Err(format!("the canary logo still has {platinum}"));
                }
            }
        }
        Ok(logos)
    }

    /// The SVG drawn at `size` px: the hand-drawn small ones at 16 and 24 px.
    fn at(&self, size: u32) -> &str {
        match size {
            16 => &self.small16,
            24 => &self.small24,
            _ => &self.master,
        }
    }
}

/// Replaces each colour (every occurrence, which must exist) in order.
fn recolour(svg: &str, swaps: &[(&str, &str)]) -> Result<String, String> {
    let mut out = svg.to_string();
    for (from, to) in swaps {
        if !out.contains(from) {
            return Err(format!("{from} is not in the logo"));
        }
        out = out.replace(from, to);
    }
    Ok(out)
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

/// A Windows .ico: bitmaps for the small sizes, PNG for 256 px (the classic layout).
fn ico(logos: &Logos) -> Result<Vec<u8>, String> {
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for s in [16, 24, 32, 48, 64, 128, 256] {
        let pixmap = render(logos.at(s), s, s, None)?;
        let image = ico::IconImage::from_rgba_data(s, s, rgba(&pixmap));
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
    Ok(bytes)
}

/// assets/icon: PNGs, whisper.ico (Windows), whisper.icns (macOS) and whisper.svg (Linux).
/// The 16 and 24 px sizes use the hand-drawn small SVGs, not a downscale.
pub fn icons() -> Result<(), String> {
    let out = root().join("assets/icon");
    let logos = Logos::release()?;
    fsutil::write(&out.join("whisper.ico"), ico(&logos)?)?;

    let mut family = icns::IconFamily::new();
    for s in [16, 32, 64, 128, 256, 512, 1024] {
        let pixmap = render(logos.at(s), s, s, None)?;
        let image = icns::Image::from_data(icns::PixelFormat::RGBA, s, s, rgba(&pixmap))
            .map_err(|e| e.to_string())?;
        family.add_icon(&image).map_err(|e| format!("{s}px: {e}"))?;
    }
    let mut bytes = Vec::new();
    family.write(&mut bytes).map_err(|e| e.to_string())?;
    fsutil::write(&out.join("whisper.icns"), bytes)?;

    for s in [256, 512] {
        save_png(
            &render(&logos.master, s, s, None)?,
            &out.join(format!("whisper-{s}.png")),
        )?;
    }
    fsutil::write(&out.join("whisper.svg"), &logos.master)?;
    println!("icons written to {}", out.display());
    Ok(())
}

/// The logo as an element that can be placed inside a larger SVG.
fn embedded_logo(x: f32, y: f32, size: f32) -> Result<String, String> {
    embedded(&logo("master.svg")?, x, y, size)
}

/// A 256×256 logo SVG (master.svg or master-inv.svg) as a nested element.
fn embedded(svg: &str, x: f32, y: f32, size: f32) -> Result<String, String> {
    let start = svg.find("<svg").ok_or("logo has no <svg>")?;
    let open_end = svg[start..].find('>').ok_or("bad logo SVG")? + start;
    let inner = &svg[open_end + 1..svg.rfind("</svg>").ok_or("bad logo SVG")?];
    Ok(format!(
        r#"<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 256 256">{inner}</svg>"#
    ))
}

/// "Whisper" in Inter SemiBold, converted to outlines so the browser shows it the same
/// everywhere without the font. `fill` is written as given (for example `context-fill`,
/// which lets Firefox's CSS choose the colour).
fn wordmark(fonts: &Path, fill: &str) -> Result<String, String> {
    let mut opt = usvg::Options::default();
    let path = fonts.join("Inter-SemiBold.ttf");
    opt.fontdb_mut()
        .load_font_data(fs::read(&path).map_err(err(path.display()))?);
    let text = r##"<svg xmlns="http://www.w3.org/2000/svg" width="460" height="140" viewBox="0 0 460 140"><text x="10" y="110" font-family="Inter" font-weight="600" font-size="112" letter-spacing="-3" fill="#000000">Whisper</text></svg>"##;
    let tree = usvg::Tree::from_str(text, &opt).map_err(|e| e.to_string())?;
    let written = tree.to_string(&usvg::WriteOptions::default());
    // Keep only the outline paths, then give them a tight viewBox and the wanted fill.
    // usvg writes absolute M/L/Q/C/Z commands, so every pair of numbers is a point.
    let mut paths = String::new();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (i, _) in written.match_indices("<path ") {
        let end = written[i..].find("/>").ok_or("unterminated <path>")? + i + 2;
        let element = &written[i..end];
        if element.contains("transform=") {
            return Err("unexpected transform on a wordmark path".into());
        }
        let d = element
            .split(" d=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .ok_or("path without d")?;
        let numbers: Vec<f32> = d
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().map_err(|_| format!("bad number {s:?}")))
            .collect::<Result<_, _>>()?;
        for p in numbers.as_chunks::<2>().0 {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        paths.push_str(&element.replace("fill=\"#000000\"", &format!("fill=\"{fill}\"")));
        paths.push('\n');
    }
    if paths.is_empty() {
        return Err("Inter did not produce any outlines".into());
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{x:.1} {y:.1} {w:.1} {h:.1}\">\n<title>Whisper</title>\n{paths}</svg>\n",
        x = x0.floor(),
        y = y0.floor(),
        w = (x1 - x0.floor()).ceil(),
        h = (y1 - y0.floor()).ceil(),
    ))
}

/// gecko/overlay/browser/branding/whisper: the images Firefox's branding directory needs
/// for the Whisper build (PNG icons, Start menu tiles, about dialog logo and wordmarks).
/// The .ico files and the installer BMPs are not repeated here: gecko/scripts/prepare.sh
/// copies them from assets/.
pub fn gecko_branding(opts: &Opts) -> Result<(), String> {
    let fonts = font_dir(opts)?;
    let out = root().join("gecko/overlay/browser/branding/whisper");
    let inverse = logo("master-inv.svg")?;
    branding_images(&Logos::release()?, &out, &fonts, "Whisper", 40.0)?;

    for (name, svg, px) in [
        ("PrivateBrowsing_70.png", &inverse, 142),
        ("PrivateBrowsing_150.png", &inverse, 300),
    ] {
        save_png(&render(&tile(svg, px)?, px, px, None)?, &out.join(name))?;
    }
    let content = out.join("content");
    for (name, svg, px) in [
        ("about-logo-private.png", &inverse, 192),
        ("about-logo-private@2x.png", &inverse, 384),
    ] {
        save_png(&render(svg, px, px, None)?, &content.join(name))?;
    }

    // The about dialog and the new tab page colour the wordmark through context-fill.
    fsutil::write(
        &content.join("about-wordmark.svg"),
        wordmark(&fonts, "context-fill")?,
    )?;
    fsutil::write(
        &content.join("firefox-wordmark.svg"),
        wordmark(&fonts, "context-fill light-dark(#111316, #d3d9df)")?,
    )?;
    println!("Firefox branding images written to {}", out.display());
    Ok(())
}

/// A Windows Start menu tile: the logo on a transparent square, the tile colour comes
/// from the VisualElements manifest.
fn tile(svg: &str, px: u32) -> Result<String, String> {
    let p = px as f32;
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{px}" height="{px}" viewBox="0 0 {px} {px}">{}</svg>"#,
        embedded(svg, p * 0.2, p * 0.2, p * 0.6)?
    ))
}

/// The images of a branding directory that show the program's own icon: the PNG icons
/// (default*.png, also the Linux window and menu icons), the Start menu tiles, the
/// about dialog and new tab logo, and about.png (logo over `name`).
fn branding_images(
    logos: &Logos,
    out: &Path,
    fonts: &Path,
    name: &str,
    name_size: f32,
) -> Result<(), String> {
    for s in [16, 22, 24, 32, 48, 64, 128, 256] {
        save_png(
            &render(logos.at(s), s, s, None)?,
            &out.join(format!("default{s}.png")),
        )?;
    }
    for (file, px) in [
        ("VisualElements_70.png", 142),
        ("VisualElements_150.png", 300),
    ] {
        save_png(
            &render(&tile(&logos.master, px)?, px, px, None)?,
            &out.join(file),
        )?;
    }
    let content = out.join("content");
    for (file, px) in [("about-logo.png", 192), ("about-logo@2x.png", 384)] {
        save_png(&render(&logos.master, px, px, None)?, &content.join(file))?;
    }
    fsutil::write(&content.join("about-logo.svg"), &logos.master)?;

    // about.png (300×236): logo over the name on a Whisper-black card, for pages that
    // show a single brand image (about:logo); readable on light and dark pages alike.
    let about = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="236" viewBox="0 0 300 236">
  <rect width="300" height="236" rx="24" fill="{BLACK}"/>
  {logo}
  <text x="150" y="206" text-anchor="middle" font-family="Inter" font-weight="600" font-size="{name_size}" letter-spacing="-1" fill="{PLATINUM}">{name}</text>
</svg>"##,
        logo = embedded(&logos.master, 94.0, 16.0, 112.0)?,
    );
    save_png(
        &render(&about, 300, 236, Some(fonts))?,
        &content.join("about.png"),
    )
}

/// gecko/variants/canary: Whisper Canary's artwork, made from the release logo (see
/// Logos::canary): the derived logo SVGs (logo/), the Windows icon (icon/), the
/// installer artwork (installer/) and the images of its branding directory (branding/,
/// copied over the release branding by gecko/scripts/prepare.sh). The private browsing
/// images and the wordmarks stay the release ones.
pub fn canary_art(opts: &Opts) -> Result<(), String> {
    let fonts = font_dir(opts)?;
    let out = root().join("gecko/variants/canary");
    let logos = Logos::canary()?;
    fsutil::write(&out.join("logo/master.svg"), &logos.master)?;
    fsutil::write(&out.join("logo/small-16.svg"), &logos.small16)?;
    fsutil::write(&out.join("logo/small-24.svg"), &logos.small24)?;
    fsutil::write(&out.join("icon/whisper-canary.ico"), ico(&logos)?)?;
    save_png(
        &render(&logos.master, 256, 256, None)?,
        &out.join("icon/whisper-canary-256.png"),
    )?;
    installer_images(
        &logos,
        &out.join("installer"),
        &fonts,
        Some("Canary"),
        "canary-",
    )?;
    branding_images(
        &logos,
        &out.join("branding"),
        &fonts,
        "Whisper Canary",
        30.0,
    )?;
    println!("Whisper Canary artwork written to {}", out.display());
    Ok(())
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
    let out = root().join("assets/installer");
    installer_images(&Logos::release()?, &out, &fonts, None, "")?;
    println!("installer artwork written to {}", out.display());
    Ok(())
}

/// sidebar.bmp and header.bmp into `out`; `edition` (for example "Canary") goes under
/// the name, in the canary yellow. PNG previews go to build/, named with `preview`.
fn installer_images(
    logos: &Logos,
    out: &Path,
    fonts: &Path,
    edition: Option<&str>,
    preview: &str,
) -> Result<(), String> {
    let edition = edition.map_or(String::new(), |e| {
        format!(
            r##"<text x="82" y="204" text-anchor="middle" font-family="Inter" font-weight="600" font-size="19" letter-spacing="-0.3" fill="{CANARY_TILE}">{e}</text>
  "##
        )
    });
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
  {edition}<text x="82" y="270" text-anchor="middle" font-family="Inter" font-weight="400" font-size="10.5" fill="{MUTED}">It can&#8217;t make you invisible.</text>
  <text x="82" y="285" text-anchor="middle" font-family="Inter" font-weight="400" font-size="10.5" fill="{MUTED}">It just won&#8217;t snitch.</text>
</svg>"##,
        logo = embedded(&logos.master, 42.0, 56.0, 80.0)?,
    );
    let header = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="150" height="57" viewBox="0 0 150 57">
  <rect width="150" height="57" fill="#FFFFFF"/>
  {logo}
</svg>"##,
        logo = embedded(&logos.master, 98.0, 6.0, 45.0)?,
    );
    for (svg, w, h, name) in [
        (sidebar, 164, 314, "sidebar.bmp"),
        (header, 150, 57, "header.bmp"),
    ] {
        let pixmap = render(&svg, w, h, Some(fonts))?;
        fsutil::write(&out.join(name), bmp24(&pixmap))?;
        save_png(
            &pixmap,
            &crate::build().join(format!("{preview}{}", name.replace(".bmp", ".png"))),
        )?;
    }
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
