//! Standalone HTML5 bundle export: one self-contained `.html` file that plays
//! the deck in any browser, fully offline.
//!
//! Every slide becomes a `<section>` laid out at 96 CSS px per inch (1:1 with
//! the EMU geometry), shapes are absolutely positioned divs, text keeps its
//! per-run font/size/weight/color, and pictures reuse their data-URI payloads.
//! The embedded script handles keyboard/click navigation, fit-to-window
//! scaling, a slide counter and a notes toggle — no network, no external
//! assets, no build step.

use std::path::Path;

use crate::document::PptxDocument;
use crate::error::Error;
use crate::inspect::{ShapeInfo, TextRunInfo};

/// 96 CSS px per inch, 914 400 EMU per inch.
const PX_PER_EMU: f64 = 96.0 / 914_400.0;
/// Points to CSS px at 96 dpi.
const PX_PER_PT: f64 = 96.0 / 72.0;
const DEFAULT_FONT_PT: f64 = 18.0;
/// PowerPoint's default text-body insets (0.1" left/right, 0.05" top/bottom).
const TEXT_PAD_X_PX: f64 = 0.1 * 96.0;
const TEXT_PAD_Y_PX: f64 = 0.05 * 96.0;

const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__ · OmaDeck</title>
<style>
html,body{height:100%;margin:0;background:#14151a;overflow:hidden}
#wrap{position:fixed;inset:0;display:flex;align-items:center;justify-content:center}
#stage{position:relative;flex:none;transform-origin:center center}
.slide{position:absolute;inset:0;background:#fff;visibility:hidden}
.slide.active{visibility:visible}
.shape{position:absolute;box-sizing:border-box;overflow:hidden}
.shape p{margin:0;line-height:1.2;white-space:pre-wrap}
img.pic{position:absolute;display:block}
.notes{display:none;position:absolute;left:0;right:0;bottom:0;max-height:30%;overflow:auto;
  background:rgba(20,21,26,.94);color:#e8e8ee;padding:10px 14px;
  font:14px/1.5 system-ui,-apple-system,sans-serif}
body.notes-on .notes{display:block}
#counter{position:fixed;right:14px;bottom:10px;color:#8f95a3;font:12px/1 system-ui,sans-serif}
#hint{position:fixed;left:14px;bottom:10px;color:#5b606c;font:11px/1 system-ui,sans-serif}
</style>
</head>
<body>
<div id="wrap"><div id="stage" style="width:__W_PX__px;height:__H_PX__px">
__SLIDES__
</div></div>
<div id="counter">1 / 1</div>
<div id="hint">&larr;/&rarr; navigate &middot; F fullscreen &middot; N notes</div>
<script>
(function () {
  var slides = Array.prototype.slice.call(document.querySelectorAll('.slide'));
  var stage = document.getElementById('stage');
  var counter = document.getElementById('counter');
  var cur = 0;
  function fit() {
    var s = stage.offsetWidth, t = stage.offsetHeight;
    var sc = Math.min(window.innerWidth / s, window.innerHeight / t) * 0.97;
    stage.style.transform = 'scale(' + sc + ')';
  }
  function show(n) {
    cur = Math.max(0, Math.min(slides.length - 1, n));
    slides.forEach(function (sl, i) { sl.classList.toggle('active', i === cur); });
    counter.textContent = (cur + 1) + ' / ' + slides.length;
  }
  window.addEventListener('keydown', function (e) {
    if (e.key === 'ArrowRight' || e.key === ' ' || e.key === 'PageDown' || e.key === 'ArrowDown') { show(cur + 1); e.preventDefault(); }
    else if (e.key === 'ArrowLeft' || e.key === 'Backspace' || e.key === 'PageUp' || e.key === 'ArrowUp') { show(cur - 1); e.preventDefault(); }
    else if (e.key === 'Home') { show(0); }
    else if (e.key === 'End') { show(slides.length - 1); }
    else if (e.key === 'f' || e.key === 'F') {
      if (document.fullscreenElement) document.exitFullscreen();
      else document.documentElement.requestFullscreen().catch(function () {});
    } else if (e.key === 'n' || e.key === 'N') {
      document.body.classList.toggle('notes-on');
    }
  });
  window.addEventListener('resize', fit);
  document.addEventListener('click', function (e) {
    if (e.clientX > window.innerWidth * 0.65) show(cur + 1);
    else if (e.clientX < window.innerWidth * 0.35) show(cur - 1);
  });
  fit();
  show(0);
})();
</script>
</body>
</html>
"#;

impl PptxDocument {
    /// Render the whole deck to a standalone HTML file: one `<section>` per
    /// slide, 1:1 EMU-to-CSS-px geometry, all assets inline.
    pub fn export_html(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let bytes = self.export_html_bytes()?;
        std::fs::write(path.as_ref(), bytes)?;
        Ok(())
    }

    /// Render the whole deck to a standalone HTML page and return the bytes.
    pub fn export_html_bytes(&self) -> Result<Vec<u8>, Error> {
        let dims = self.slide_dimensions();
        let model = crate::model_of(&self.pres);
        let w_px = emu_px(dims.width_emu);
        let h_px = emu_px(dims.height_emu);

        let mut slides = String::new();
        for i in 0..self.slide_count() {
            let shapes = self.get_slide_shapes(i)?;
            let notes = model.slides[i].notes.clone();
            slides.push_str(&render_slide(&shapes, notes.as_deref()));
        }

        let html = TEMPLATE
            .replace("__TITLE__", &escape_html(&model.title))
            .replace("__W_PX__", &px(w_px))
            .replace("__H_PX__", &px(h_px))
            .replace("__SLIDES__", &slides);
        Ok(html.into_bytes())
    }
}

fn emu_px(emu: i64) -> f64 {
    emu as f64 * PX_PER_EMU
}

fn px(v: f64) -> String {
    format!("{:.2}", v)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Fill values that are paintable CSS colors (as opposed to the tokens
/// `"none"` / `"gradient"` / `"pattern"` / `"image"` reported by inspect).
fn paintable(color: &str) -> bool {
    !matches!(
        color.to_ascii_lowercase().as_str(),
        "none" | "gradient" | "pattern" | "image"
    )
}

/// CSS `font-family` for a run: the deck's family first, then a stack that
/// matches its generic class (serif / mono / sans).
fn font_stack(family: Option<&str>) -> String {
    let fallback = match family.map(|f| f.to_ascii_lowercase()).as_deref() {
        Some(f)
            if f.contains("times")
                || f.contains("georgia")
                || f.contains("garamond")
                || f.contains("serif") =>
        {
            "\"Times New Roman\", Georgia, serif"
        }
        Some(f)
            if f.contains("mono")
                || f.contains("courier")
                || f.contains("consolas") =>
        {
            "Consolas, \"Courier New\", monospace"
        }
        _ => "\"Segoe UI\", Arial, Helvetica, sans-serif",
    };
    match family {
        Some(f) if !f.is_empty() => format!("{:?}, {}", f, fallback),
        _ => fallback.to_string(),
    }
}

fn render_slide(shapes: &[ShapeInfo], notes: Option<&str>) -> String {
    let mut out = String::from("<section class=\"slide\">");
    for sh in shapes {
        out.push_str(&render_shape(sh));
    }
    if let Some(notes) = notes {
        if !notes.trim().is_empty() {
            out.push_str("<div class=\"notes\">");
            out.push_str(&escape_html(notes));
            out.push_str("</div>");
        }
    }
    out.push_str("</section>");
    out
}

fn render_shape(sh: &ShapeInfo) -> String {
    if let Some(children) = &sh.children {
        let mut out = String::new();
        for c in children {
            out.push_str(&render_shape(c));
        }
        return out;
    }
    match sh.kind {
        "picture" => {
            if let (Some(b), Some(pic)) = (&sh.bounds, &sh.pic) {
                format!(
                    "<img class=\"pic\" style=\"left:{}px;top:{}px;width:{}px;height:{}px\" src=\"{}\" alt=\"{}\">",
                    px(emu_px(b.x_emu)),
                    px(emu_px(b.y_emu)),
                    px(emu_px(b.width_emu)),
                    px(emu_px(b.height_emu)),
                    pic.data_uri,
                    escape_html(&sh.name)
                )
            } else {
                String::new()
            }
        }
        "autoshape" => {
            let Some(b) = &sh.bounds else {
                return String::new();
            };
            let mut style = format!(
                "left:{}px;top:{}px;width:{}px;height:{}px",
                px(emu_px(b.x_emu)),
                px(emu_px(b.y_emu)),
                px(emu_px(b.width_emu)),
                px(emu_px(b.height_emu))
            );
            if let Some(fill) = sh.fill.as_deref().filter(|f| paintable(f)) {
                style.push_str(&format!(";background:{}", fill));
            }
            if let Some(line) = sh.line.as_ref() {
                if let Some(color) = line.color.as_deref().filter(|c| paintable(c)) {
                    let w_px = line.width_emu.map(emu_px).unwrap_or(12_700.0 * PX_PER_EMU);
                    style.push_str(&format!(";border:{:.2}px solid {}", w_px.max(0.5), color));
                }
            }
            let text = render_text(sh);
            if text.is_empty() {
                format!("<div class=\"shape\" style=\"{style}\"></div>")
            } else {
                style.push_str(&format!(
                    ";padding:{}px {}px",
                    px(TEXT_PAD_Y_PX),
                    px(TEXT_PAD_X_PX)
                ));
                format!("<div class=\"shape\" style=\"{style}\">{text}</div>")
            }
        }
        _ => String::new(),
    }
}

/// The shape's text as `<p>` paragraphs (runs grouped by paragraph index,
/// explicit line breaks as `<br>`), each run a styled `<span>`.
fn render_text(sh: &ShapeInfo) -> String {
    let mut paras: Vec<(usize, String, Option<String>)> = Vec::new();
    for r in &sh.runs {
        if r.text.is_empty() {
            continue;
        }
        let entry = paras.iter_mut().find(|(p, _, _)| *p == r.paragraph);
        let entry = match entry {
            Some(e) => e,
            None => {
                paras.push((r.paragraph, String::new(), r.alignment.clone()));
                paras.last_mut().unwrap()
            }
        };
        if r.text == "\n" {
            entry.1.push_str("<br>");
            continue;
        }
        entry.1.push_str(&run_span(r));
    }
    let mut out = String::new();
    for (p, inner, align) in paras {
        let _ = p;
        let align = align
            .as_deref()
            .filter(|a| matches!(*a, "left" | "center" | "right" | "justify"))
            .unwrap_or("left");
        out.push_str(&format!("<p style=\"text-align:{align}\">{inner}</p>"));
    }
    out
}

fn run_span(r: &TextRunInfo) -> String {
    let size = r.font_size_pt.unwrap_or(DEFAULT_FONT_PT) * PX_PER_PT;
    let mut style = format!(
        "font-family:{};font-size:{:.1}px",
        font_stack(r.font_family.as_deref()),
        size
    );
    if r.bold {
        style.push_str(";font-weight:700");
    }
    if r.italic {
        style.push_str(";font-style:italic");
    }
    if let Some(color) = r.color.as_deref().filter(|c| paintable(c)) {
        style.push_str(&format!(";color:{color}"));
    }
    format!("<span style=\"{style}\">{}</span>", escape_html(&r.text))
}
