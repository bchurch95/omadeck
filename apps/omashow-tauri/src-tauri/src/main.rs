use std::sync::Mutex;
use tauri::{Manager, State, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use omashow_core::{detect_morph, model_of, MorphResult, PptxDocument, SlideDimensions, ZOrderAction};
use serde::Serialize;

/// The in-memory deck — the single source of truth. `PptxDocument` holds both the
/// editable model and the original file's parts, so saving after edits stays lossless.
/// The frontend only ever sees a lightweight JSON projection of the model.
#[derive(Default)]
struct Deck {
    doc: Option<PptxDocument>,
    path: Option<String>,
}

/// Path passed as `omashow-tauri <file.pptx>`; the frontend pulls it once on
/// load and opens it, so no event can be missed before the webview boots.
#[derive(Default)]
struct InitialDeck(Option<String>);
#[derive(Default)]
struct CurrentSlide(std::sync::atomic::AtomicUsize);

fn main() {
    let open_path = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(Deck::default()))
        .manage(InitialDeck(open_path))
        .manage(CurrentSlide::default())
        .invoke_handler(tauri::generate_handler![
            initial_deck_path,
            new_presentation,
            open_pptx,
            open_presentation,
            save_pptx,
            save_as,
            save_presentation,
            export_pdf,
            export_html,
            get_slide_content,
            morph_between,
            set_title,
            set_notes,
            add_slide,
            add_slide_at,
            delete_slide,
            move_slide,
            reorder_slides,
            apply_theme,
            update_text_run,
            move_shape,
            delete_shape,
            set_text_run,
            save_presentation_edited,
            undo_presentation,
            redo_presentation,
            undo_state,
            open_file_dialog,
            save_file_dialog,
            list_monitors,
            reorder_shape_z_order,
            set_current_slide,
            get_current_slide,
            open_audience_window,
            close_audience_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn project(doc: &PptxDocument) -> Result<String, String> {
    serde_json::to_string(&omashow_core::model_of(&doc.pres)).map_err(|e| e.to_string())
}

#[tauri::command]
fn initial_deck_path(state: State<'_, InitialDeck>) -> Option<String> {
    state.0.clone()
}

#[tauri::command]
fn set_current_slide(state: State<'_, CurrentSlide>, slide: usize) {
    state.0.store(slide, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn get_current_slide(state: State<'_, CurrentSlide>) -> usize {
    state.0.load(std::sync::atomic::Ordering::Relaxed)
}

/// Deck overview returned by `open_presentation`: metadata, the slide list,
/// and speaker notes (no shapes — those come per-slide via `get_slide_content`).
#[derive(Serialize)]
struct PresentationSummary {
    title: String,
    slide_count: usize,
    slide_dimensions: SlideDimensions,
    slides: Vec<SlideSummary>,
}

#[derive(Serialize)]
struct SlideSummary {
    index: usize,
    title: Option<String>,
    notes: Option<String>,
}

/// Full content of one slide for the canvas: shapes (with runs, colors,
/// bounding boxes) plus the slide dimensions for EMU-to-pixel scaling.
#[derive(Serialize)]
struct SlideContent {
    index: usize,
    slide_dimensions: SlideDimensions,
    shapes: Vec<omashow_core::ShapeInfo>,
    background: Option<String>,
}

fn summary(doc: &PptxDocument) -> PresentationSummary {
    let model = model_of(&doc.pres);
    let slides = (0..doc.slide_count())
        .map(|i| SlideSummary {
            index: i,
            title: model.slides[i].title.clone(),
            notes: model.slides[i].notes.clone(),
        })
        .collect();
    PresentationSummary {
        title: model.title,
        slide_count: doc.slide_count(),
        slide_dimensions: doc.slide_dimensions(),
        slides,
    }
}

#[tauri::command]
fn new_presentation(state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    deck.doc = Some(PptxDocument::new());
    deck.path = None;
    project(deck.doc.as_ref().unwrap())
}

#[tauri::command]
fn open_pptx(path: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = PptxDocument::open(&path).map_err(|e| e.to_string())?;
    deck.doc = Some(doc);
    deck.path = Some(path);
    project(deck.doc.as_ref().unwrap())
}

#[tauri::command]
fn open_presentation(
    path: String,
    state: State<'_, Mutex<Deck>>,
) -> Result<PresentationSummary, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = PptxDocument::open(&path).map_err(|e| e.to_string())?;
    let out = summary(&doc);
    deck.doc = Some(doc);
    deck.path = Some(path);
    Ok(out)
}

#[tauri::command]
fn get_slide_content(slide: usize, state: State<'_, Mutex<Deck>>) -> Result<SlideContent, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    let shapes = doc.get_slide_shapes(slide).map_err(|e| e.to_string())?;
    let bg = doc.slide_background(slide).map_err(|e| e.to_string())?;
    Ok(SlideContent {
        index: slide,
        slide_dimensions: doc.slide_dimensions(),
        background: bg,
        shapes,
    })
}

/// Which shapes morph between two slides (Keynote Magic Move detection).
/// The frontend drives the transition animation from the returned boxes.
#[tauri::command]
fn morph_between(
    before: usize,
    after: usize,
    state: State<'_, Mutex<Deck>>,
) -> Result<MorphResult, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    let a = doc.get_slide_shapes(before).map_err(|e| e.to_string())?;
    let b = doc.get_slide_shapes(after).map_err(|e| e.to_string())?;
    Ok(detect_morph(&a, &b))
}

#[tauri::command]
fn save_presentation(path: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    doc.save(&path).map_err(|e| e.to_string())?;
    deck.path = Some(path.clone());
    Ok(path)
}

#[tauri::command]
fn save_pptx(state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    let path = deck.path.as_ref().ok_or("no file path set — use save_as")?;
    doc.save(path).map_err(|e| e.to_string())?;
    Ok(path.clone())
}

#[tauri::command]
fn save_as(path: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    doc.save(&path).map_err(|e| e.to_string())?;
    deck.path = Some(path.clone());
    Ok(path)
}

#[tauri::command]
fn export_pdf(path: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    doc.export_pdf(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
fn export_html(path: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    doc.export_html(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
fn set_title(slide: usize, title: String, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.set_title(slide, &title).map_err(|e| e.to_string())?;
    project(doc)
}

#[tauri::command]
fn set_notes(slide: usize, notes: Option<String>, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.set_notes(slide, notes).map_err(|e| e.to_string())?;
    project(doc)
}

#[tauri::command]
fn add_slide(title: Option<String>, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.add_slide(title).map_err(|e| e.to_string())?;
    project(doc)
}

#[tauri::command]
fn delete_slide(slide: usize, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.delete_slide(slide).map_err(|e| e.to_string())?;
    project(doc)
}

#[tauri::command]
fn reorder_shape_z_order(
    slide: usize,
    shape_id: u32,
    action: String,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let action = match action.as_str() {
        "bring_to_front" => ZOrderAction::BringToFront,
        "send_to_back" => ZOrderAction::SendToBack,
        "bring_forward" => ZOrderAction::BringForward,
        "send_backward" => ZOrderAction::SendBackward,
        other => return Err(format!("unknown z-order action: {other}")),
    };
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.reorder_shape_z_order(slide, shape_id, action)
        .map_err(|e| e.to_string())?;
    project(doc)
}

/// Insert a new slide at `index` (one past the end appends).
#[tauri::command]
fn add_slide_at(index: usize, title: Option<String>, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.add_slide_at(index, title).map_err(|e| e.to_string())?;
    project(doc)
}

/// Move the slide at `from` so it ends up at position `to`.
#[tauri::command]
fn move_slide(from: usize, to: usize, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.move_slide(from, to).map_err(|e| e.to_string())?;
    project(doc)
}

/// Reorder all slides to match `order`, a permutation of `0..slide_count`.
#[tauri::command]
fn reorder_slides(order: Vec<usize>, state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.reorder_slides(order).map_err(|e| e.to_string())?;
    project(doc)
}

/// Apply a deck-wide color theme (source hex -> target hex pairs), persisting
/// the file immediately. The in-memory model is unchanged; the remap lives on
/// the document and is rewritten into every XML part on save.
#[tauri::command]
fn apply_theme(
    map: Vec<(String, String)>,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let path = deck.path.clone();
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.apply_theme(map).map_err(|e| e.to_string())?;
    if let Some(path) = path {
        doc.save(path).map_err(|e| e.to_string())?;
    }
    project(doc)
}

/// Replace the text of the shape `shape_id` on `slide`, keeping its base formatting.
#[tauri::command]
fn update_text_run(
    slide: usize,
    shape_id: u32,
    new_text: String,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.update_text_run(slide, shape_id, &new_text).map_err(|e| e.to_string())?;
    project(doc)
}

/// Shift the shape `shape_id` on `slide` by `dx_emu`/`dy_emu` EMU.
#[tauri::command]
fn move_shape(
    slide: usize,
    shape_id: u32,
    dx_emu: i64,
    dy_emu: i64,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.move_shape(slide, shape_id, dx_emu, dy_emu).map_err(|e| e.to_string())?;
    project(doc)
}

/// Remove the shape `shape_id` from `slide`.
#[tauri::command]
fn delete_shape(
    slide: usize,
    shape_id: u32,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.delete_shape(slide, shape_id).map_err(|e| e.to_string())?;
    project(doc)
}

/// Replace the text of the run at `run_idx` in the shape `shape_id` on `slide`.
#[tauri::command]
fn set_text_run(
    slide: usize,
    shape_id: u32,
    run_idx: usize,
    text: String,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.set_text_run(slide, shape_id, run_idx, &text).map_err(|e| e.to_string())?;
    project(doc)
}

/// Save the current (possibly edited) presentation to `path` and return the
/// fresh model so the frontend can confirm the write.
#[tauri::command]
fn save_presentation_edited(
    path: String,
    state: State<'_, Mutex<Deck>>,
) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.save(&path).map_err(|e| e.to_string())?;
    let model = project(doc)?;
    deck.path = Some(path.clone());
    Ok(model)
}

/// Undoes the most recent edit; returns the updated model.
#[tauri::command]
fn undo_presentation(state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.undo().ok_or("nothing to undo")?;
    project(doc)
}

/// Re-applies the most recently undone edit.
#[tauri::command]
fn redo_presentation(state: State<'_, Mutex<Deck>>) -> Result<String, String> {
    let mut deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_mut().ok_or("no presentation open")?;
    doc.redo().ok_or("nothing to redo")?;
    project(doc)
}

/// Undo/redo availability plus the last action's label, for toolbar state.
#[derive(Serialize)]
struct UndoState {
    can_undo: bool,
    can_redo: bool,
    last: Option<String>,
}

#[tauri::command]
fn undo_state(state: State<'_, Mutex<Deck>>) -> Result<UndoState, String> {
    let deck = state.lock().map_err(|e| e.to_string())?;
    let doc = deck.doc.as_ref().ok_or("no presentation open")?;
    Ok(UndoState {
        can_undo: doc.can_undo(),
        can_redo: doc.can_redo(),
        last: doc.undo_description(),
    })
}

#[tauri::command]
async fn open_file_dialog(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app_handle
        .dialog()
        .file()
        .set_title("Open Presentation")
        .add_filter("PowerPoint", &["pptx"])
        .pick_file(move |file| {
            let _ = tx.try_send(file);
        });
    let file = rx.recv().await.flatten();
    Ok(file.and_then(|p| p.as_path().map(|path| path.to_string_lossy().to_string())))
}

#[tauri::command]
async fn save_file_dialog(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app_handle
        .dialog()
        .file()
        .set_title("Save Presentation")
        .add_filter("PowerPoint", &["pptx"])
        .save_file(move |file| {
            let _ = tx.try_send(file);
        });
    let file = rx.recv().await.flatten();
    Ok(file.and_then(|p| p.as_path().map(|path| path.to_string_lossy().to_string())))
}

const AUDIENCE_LABEL: &str = "audience";

/// Projection-target affinity for a monitor name. AirPlay receivers (Apple
/// TV, the AirPlay Receiver app) and TV displays show up as external
/// monitors; scoring by name lets secondary-display routing prefer them over
/// ordinary second displays.
pub fn airplay_affinity(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    let compact: String = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if compact.contains("airplay") {
        100
    } else if compact.contains("appletv") {
        90
    } else if tv_word(&lower) {
        70
    } else if lower.contains("display") {
        40
    } else {
        0
    }
}

/// True when "tv" stands as a word on its own ("Samsung TV", "Apple TV")
/// but not inside an identifier ("DTV-1", "eDP-1").
fn tv_word(lower: &str) -> bool {
    lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == "tv")
}

/// Among the non-primary monitors, the one most likely to be the projection
/// target: highest AirPlay affinity first, ties broken by connection order.
/// `None` when no secondary display exists (single-display setup).
fn pick_external_index(monitors: &[(String, bool)]) -> Option<usize> {
    let mut best: Option<(u8, usize)> = None;
    for (i, (name, primary)) in monitors.iter().enumerate() {
        if *primary {
            continue;
        }
        let aff = airplay_affinity(name);
        if best.map_or(true, |(b, _)| aff > b) {
            best = Some((aff, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Audience display selection policy: an explicitly requested monitor name
/// wins; otherwise the best AirPlay / external monitor (an Apple TV or
/// AirPlay receiver beats a plain second display, and the first secondary
/// display wins on ties); then the single (primary) monitor. Returns `None`
/// when nothing matches or no display exists.
fn pick_audience_index(monitors: &[(String, bool)], requested: Option<&str>) -> Option<usize> {
    match requested {
        Some(name) => monitors.iter().position(|(n, _)| n == name),
        None => pick_external_index(monitors).or_else(|| (!monitors.is_empty()).then_some(0)),
    }
}

/// Window placement and chrome for the audience window, in logical (CSS)
/// pixels. Secondary target: borderless, always-on-top fullscreen covering the
/// monitor exactly (the position at the monitor origin is what makes the
/// runtime resolve fullscreen for *this* display). Primary target (single
/// display): a floating, decorated 16:9 preview at ~65% of the display width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudienceLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub decorations: bool,
    pub fullscreen: bool,
    pub always_on_top: bool,
}

/// Pure geometry decision so secondary-display routing is unit-testable
/// without hardware. `monitor` is already in logical pixels.
pub fn audience_layout(monitor_x: f64, monitor_y: f64, monitor_w: f64, monitor_h: f64, target_is_primary: bool) -> AudienceLayout {
    if !target_is_primary {
        AudienceLayout {
            x: monitor_x,
            y: monitor_y,
            width: monitor_w,
            height: monitor_h,
            decorations: false,
            fullscreen: true,
            always_on_top: true,
        }
    } else {
        let width = (monitor_w * 0.65).clamp(480.0, 960.0);
        let height = (width * 9.0 / 16.0).round();
        AudienceLayout {
            x: f64::NAN, // OS-placed (no explicit position)
            y: f64::NAN,
            width,
            height,
            decorations: true,
            fullscreen: false,
            always_on_top: true,
        }
    }
}

/// A display attached to the system, in logical (CSS) pixels.
#[derive(Serialize, Clone)]
struct MonitorInfo {
    name: String,
    is_primary: bool,
    /// A secondary display (not the built-in / primary screen).
    is_external: bool,
    /// Looks like an AirPlay / TV projection target by name (Apple TV,
    /// AirPlay receiver, "… TV"). Heuristic — the name comes from the OS.
    is_airplay: bool,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

#[tauri::command]
fn list_monitors(app: tauri::AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let primary = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .and_then(|m| m.name().cloned());
    Ok(app
        .available_monitors()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|m| {
            let scale = m.scale_factor();
            let size = *m.size();
            let pos = *m.position();
            let name = m.name().cloned().unwrap_or_else(|| "monitor".to_string());
            let is_primary = Some(name.clone()) == primary;
            MonitorInfo {
                is_primary,
                is_external: !is_primary,
                is_airplay: airplay_affinity(&name) >= 70,
                name,
                width: (size.width as f64 / scale).round() as u32,
                height: (size.height as f64 / scale).round() as u32,
                x: (pos.x as f64 / scale).round() as i32,
                y: (pos.y as f64 / scale).round() as i32,
            }
        })
        .collect())
}

/// Opens an audience projection window. When a secondary monitor exists (or is
/// explicitly requested) — including AirPlay receivers such as Apple TV or the
/// AirPlay Receiver app, which the OS reports as external displays — the
/// window is borderless, fullscreen, and always on top on that display, so
/// focus changes on the primary monitor never hide or move it while the
/// presenter multitasks in the console. On a single display (e.g. testing
/// locally), it opens as a floating, decorated preview window so both views
/// are visible.
#[tauri::command]
fn open_audience_window(app: tauri::AppHandle, monitor_name: Option<String>) -> Result<String, String> {
    if let Some(existing) = app.get_webview_window(AUDIENCE_LABEL) {
        existing.close().map_err(|e| e.to_string())?;
    }
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    let primary_name = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .and_then(|m| m.name().cloned());
    let specs: Vec<(String, bool)> = monitors
        .iter()
        .map(|m| {
            let name = m.name().cloned().unwrap_or_default();
            (name, m.name().cloned() == primary_name)
        })
        .collect();
    let index = pick_audience_index(&specs, monitor_name.as_deref()).ok_or_else(|| {
        match monitor_name.as_deref() {
            Some(name) => format!("monitor '{name}' not found"),
            None => "no display found".to_string(),
        }
    })?;
    let monitor = &monitors[index];
    let is_primary = specs[index].1;
    let scale = monitor.scale_factor().max(0.1);
    let size = *monitor.size();
    let pos = *monitor.position();

    let layout = audience_layout(
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
        is_primary,
    );

    let mut builder = WebviewWindowBuilder::new(&app, AUDIENCE_LABEL, tauri::WebviewUrl::App("audience.html".into()));
    builder = builder
        .title(if layout.fullscreen { "Omashow — Audience" } else { "Omashow — Audience Preview" })
        .inner_size(layout.width, layout.height)
        .decorations(layout.decorations)
        .fullscreen(layout.fullscreen)
        .minimizable(!layout.fullscreen)
        .maximizable(!layout.fullscreen)
        .resizable(!layout.fullscreen)
        .always_on_top(layout.always_on_top);
    if layout.x.is_finite() {
        builder = builder.position(layout.x, layout.y);
    }
    builder.build().map_err(|e| e.to_string())?;

    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.set_focus();
    }

    Ok(monitor.name().cloned().unwrap_or_default())
}

#[tauri::command]
fn close_audience_window(app: tauri::AppHandle) -> Result<(), String> {
    match app.get_webview_window(AUDIENCE_LABEL) {
        Some(win) => win.close().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{airplay_affinity, audience_layout, pick_audience_index, pick_external_index};

    fn names(names: &[&str], primary: usize) -> Vec<(String, bool)> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), i == primary))
            .collect()
    }

    #[test]
    fn requested_monitor_wins() {
        let m = names(&["eDP-1", "HDMI-A-1"], 0);
        assert_eq!(pick_audience_index(&m, Some("eDP-1")), Some(0));
        assert_eq!(pick_audience_index(&m, Some("HDMI-A-1")), Some(1));
    }

    #[test]
    fn unspecified_prefers_first_non_primary() {
        let m = names(&["eDP-1", "HDMI-A-1", "DP-2"], 0);
        assert_eq!(pick_audience_index(&m, None), Some(1));
    }

    #[test]
    fn single_monitor_is_used() {
        let m = names(&["eDP-1"], 0);
        assert_eq!(pick_audience_index(&m, None), Some(0));
    }

    #[test]
    fn unknown_or_empty_is_none() {
        let m = names(&["eDP-1"], 0);
        assert_eq!(pick_audience_index(&m, Some("DP-9")), None);
        let empty: Vec<(String, bool)> = vec![];
        assert_eq!(pick_audience_index(&empty, None), None);
    }

    #[test]
    fn airplay_names_score_higher_than_plain_displays() {
        assert_eq!(airplay_affinity("AirPlay Display"), 100);
        assert_eq!(airplay_affinity("Kitchen Apple TV"), 90);
        assert_eq!(airplay_affinity("AppleTV"), 90);
        assert_eq!(airplay_affinity("Apple-TV 4K"), 90);
        assert_eq!(airplay_affinity("Samsung TV"), 70);
        assert_eq!(airplay_affinity("Display 2"), 40);
        assert_eq!(airplay_affinity("HDMI-A-1"), 0);
        assert_eq!(airplay_affinity("eDP-1"), 0);
        // "tv" must not match inside identifiers
        assert_eq!(airplay_affinity("DTV-1"), 0);
    }

    #[test]
    fn airplay_receiver_wins_over_other_external_displays() {
        let m = names(&["eDP-1", "HDMI-A-1", "Kitchen Apple TV"], 0);
        assert_eq!(pick_external_index(&m), Some(2));
        assert_eq!(pick_audience_index(&m, None), Some(2));

        let m = names(&["eDP-1", "DP-2", "AirPlay Display"], 0);
        assert_eq!(pick_audience_index(&m, None), Some(2));
    }

    #[test]
    fn external_pick_falls_back_to_first_secondary_on_tie() {
        let m = names(&["eDP-1", "HDMI-A-1", "DP-2"], 0);
        assert_eq!(pick_external_index(&m), Some(1));

        let single = names(&["eDP-1"], 0);
        assert_eq!(pick_external_index(&single), None);
        assert_eq!(pick_audience_index(&single, None), Some(0));
    }

    #[test]
    fn secondary_monitor_covers_display_exactly() {
        let l = audience_layout(1920.0, 0.0, 2560.0, 1440.0, false);
        assert_eq!((l.x, l.y), (1920.0, 0.0));
        assert_eq!((l.width, l.height), (2560.0, 1440.0));
        assert!(!l.decorations);
        assert!(l.fullscreen);
        assert!(l.always_on_top);
    }

    #[test]
    fn secondary_monitor_left_of_primary_keeps_negative_origin() {
        let l = audience_layout(-1920.0, 0.0, 1920.0, 1080.0, false);
        assert_eq!((l.x, l.y), (-1920.0, 0.0));
        assert_eq!((l.width, l.height), (1920.0, 1080.0));
        assert!(l.fullscreen);
    }

    #[test]
    fn secondary_hidpi_uses_logical_coordinates() {
        // 4K monitor at 2x scale: physical (3840,0) 3840x2160 -> logical (1920,0) 1920x1080.
        let l = audience_layout(1920.0, 0.0, 1920.0, 1080.0, false);
        assert_eq!((l.x, l.y), (1920.0, 0.0));
        assert_eq!((l.width, l.height), (1920.0, 1080.0));
    }

    #[test]
    fn single_display_preview_is_16_9_and_bounded() {
        // 1920x1080 primary: 65% width would be 1248, clamped to 960 -> 540 tall.
        let l = audience_layout(0.0, 0.0, 1920.0, 1080.0, true);
        assert_eq!((l.width, l.height), (960.0, 540.0));
        assert!(l.decorations);
        assert!(!l.fullscreen);
        assert!(l.always_on_top);
        assert!(!l.x.is_finite()); // OS-placed, no explicit position
    }

    #[test]
    fn small_display_preview_respects_minimum() {
        // 640x480: 65% = 416 -> clamped up to 480.
        let l = audience_layout(0.0, 0.0, 640.0, 480.0, true);
        assert_eq!((l.width, l.height), (480.0, 270.0));
    }

    #[test]
    fn ultrawide_preview_stays_16_9() {
        let l = audience_layout(0.0, 0.0, 3840.0, 1080.0, true);
        assert_eq!((l.width, l.height), (960.0, 540.0));
    }
}
