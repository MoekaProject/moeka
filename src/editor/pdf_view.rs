use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use gpui::{
    AnyElement, Bounds, Context, Corners, Entity, FocusHandle, Hsla, MouseButton, Pixels, Point,
    Render, RenderImage, Task, Window, canvas, div, fill, point, prelude::*, px, size,
};
use hayro::kurbo::Point as PdfPoint;

use super::{EditorView, PdfViewState, pdf};
use crate::i18n::{UiLanguage, runtime::RuntimeText};

const CACHE_BYTES: usize = 128 * 1024 * 1024;
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static DOCUMENT_ID: AtomicUsize = AtomicUsize::new(1);
type RasterKey = (usize, u32, u32, u32, u8);

fn page_size(page: &pdf::PageInfo, rotation: u8) -> (f32, f32) {
    if rotation.is_multiple_of(2) {
        (page.width, page.height)
    } else {
        (page.height, page.width)
    }
}

fn thumbnail_size(page: &pdf::PageInfo, rotation: u8) -> (f32, f32, f32) {
    let (width, height) = page_size(page, rotation);
    let scale = (104.0 / width).min(112.0 / height);
    (width * scale, height * scale, scale)
}

fn thumbnail_label_origin(rect: Bounds<Pixels>, text_width: Pixels) -> Point<Pixels> {
    point(rect.center().x - text_width * 0.5, rect.bottom() + px(3.0))
}

// Lower-resolution tiles remain in the bounded shared cache, not in a second
// unaccounted image store. Paint them underneath newly arriving sharp tiles.
fn fallback_tiles(
    document: usize,
    page: usize,
    rotation: u8,
    dimensions: (f32, f32),
    rect: Bounds<Pixels>,
) -> Vec<(Bounds<Pixels>, Option<Arc<RenderImage>>)> {
    CACHE.with_borrow_mut(|cache| {
        let mut keys = cache
            .images
            .keys()
            .filter_map(|&(owner, key)| {
                (owner == document && key.0 == page && key.4 == rotation).then_some(key)
            })
            .collect::<Vec<_>>();
        keys.sort_by_key(|key| key.1);
        keys.into_iter()
            .map(|key| {
                let width = (dimensions.0 * key.1 as f32 / 100.0).ceil();
                let height = (dimensions.1 * key.1 as f32 / 100.0).ceil();
                let x = key.2 as f32 * pdf::TILE_SIZE as f32;
                let y = key.3 as f32 * pdf::TILE_SIZE as f32;
                let tile = Bounds::new(
                    rect.origin
                        + point(
                            rect.size.width * (x / width),
                            rect.size.height * (y / height),
                        ),
                    size(
                        rect.size.width * ((width - x).min(pdf::TILE_SIZE as f32) / width),
                        rect.size.height * ((height - y).min(pdf::TILE_SIZE as f32) / height),
                    ),
                );
                cache.clock = cache.clock.wrapping_add(1);
                let used = cache.clock;
                let image = cache
                    .images
                    .get_mut(&(document, key))
                    .expect("cached PDF tile");
                image.used = used;
                (tile, Some(image.image.clone()))
            })
            .collect()
    })
}

enum PdfSource {
    Local(PathBuf),
    Remote(
        Arc<moeka_ssh::filesystem::SshFilesystem>,
        moeka_fs::path::NativePath,
    ),
}

#[derive(Default)]
struct RasterCache {
    images: HashMap<(usize, RasterKey), CachedPage>,
    clock: usize,
    retired: Vec<(Option<gpui::AnyWindowHandle>, Arc<RenderImage>)>,
}

thread_local! {
    static CACHE: std::cell::RefCell<RasterCache> = std::cell::RefCell::default();
}

fn cached(document: usize, key: RasterKey) -> Option<Arc<RenderImage>> {
    CACHE.with_borrow_mut(|cache| {
        cache.clock = cache.clock.wrapping_add(1);
        let used = cache.clock;
        cache.images.get_mut(&(document, key)).map(|image| {
            image.used = used;
            image.image.clone()
        })
    })
}

fn cache_image(
    document: usize,
    key: RasterKey,
    image: Arc<RenderImage>,
    bytes: usize,
    window: Option<gpui::AnyWindowHandle>,
) {
    CACHE.with_borrow_mut(|cache| {
        cache.clock = cache.clock.wrapping_add(1);
        let used = cache.clock;
        cache.images.insert(
            (document, key),
            CachedPage {
                image,
                bytes,
                used,
                window,
            },
        );
        while cache
            .images
            .values()
            .map(|image| image.bytes * 2)
            .sum::<usize>()
            > CACHE_BYTES
        {
            let key = cache
                .images
                .iter()
                .min_by_key(|(_, image)| image.used)
                .map(|(key, _)| *key);
            if let Some(key) = key {
                if let Some(image) = cache.images.remove(&key) {
                    cache.retired.push((image.window, image.image));
                }
            } else {
                break;
            }
        }
    });
}

struct Permit;

fn retire_document(document: usize) {
    CACHE.with_borrow_mut(|cache| {
        let keys = cache
            .images
            .keys()
            .filter(|(owner, _)| *owner == document)
            .copied()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(image) = cache.images.remove(&key) {
                cache.retired.push((image.window, image.image));
            }
        }
    });
}

fn retire_images(window: &mut Window, cx: &mut gpui::App) {
    use gpui::AppContext;
    let retired = CACHE.with_borrow_mut(|cache| std::mem::take(&mut cache.retired));
    for (owner, image) in retired {
        let Some(owner) = owner else {
            continue;
        };
        if owner == window.window_handle() {
            if let Err(error) = window.drop_image(image) {
                eprintln!("PDF: {error}");
            }
        } else if cx.windows().contains(&owner) {
            match cx.update_window(owner, |_, window, _| window.drop_image(image)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) | Err(error) => eprintln!("PDF: {error}"),
            }
        }
    }
}

fn release_images(cx: &mut gpui::App) {
    use gpui::AppContext;
    let retired = CACHE.with_borrow_mut(|cache| std::mem::take(&mut cache.retired));
    for (owner, image) in retired {
        if let Some(owner) = owner
            && cx.windows().contains(&owner)
        {
            match cx.update_window(owner, |_, window, _| window.drop_image(image)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) | Err(error) => eprintln!("{}: {error}", RuntimeText::RenderFailed),
            }
        }
    }
}

impl Permit {
    fn acquire() -> Option<Self> {
        Self::acquire_below(2)
    }

    fn acquire_background() -> Option<Self> {
        Self::acquire_below(1)
    }

    fn acquire_below(limit: usize) -> Option<Self> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |count| {
                (count < limit).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::Release);
    }
}

#[derive(Clone)]
struct CachedPage {
    image: Arc<RenderImage>,
    bytes: usize,
    used: usize,
    window: Option<gpui::AnyWindowHandle>,
}

pub(super) struct PdfView {
    pub info: Option<Arc<pdf::DocumentInfo>>,
    pub text: Option<Arc<Vec<pdf::PageText>>>,
    bytes: Option<Arc<[u8]>>,
    cancelled: Arc<AtomicBool>,
    task: Option<Task<()>>,
    raster_task: Option<Task<()>>,
    raster_requested: Arc<std::sync::Mutex<HashSet<RasterKey>>>,
    document_id: usize,
    content_id: usize,
    image_task: Option<Task<()>>,
    failed: HashSet<RasterKey>,
    pub error: Option<String>,
    pub search_matches: Vec<(usize, std::ops::Range<usize>)>,
    pub search_current: Option<usize>,
    search_task: Option<Task<()>>,
    search_epoch: usize,
    pub searching: bool,
    pub search_error: Option<String>,
    pub page: usize,
    scroll: f32,
    horizontal_scroll: f32,
    thumb_scroll: f32,
    thumbnail_offsets: Vec<f32>,
    thumbnail_focus: FocusHandle,
    zoom: f32,
    rotation: u8,
    fit_page: bool,
    page_input: Option<Entity<crate::text_input::TextInput>>,
    zoom_input: Option<Entity<crate::text_input::TextInput>>,
    toolbar_error: Option<String>,
    toolbar_editor: Option<gpui::WeakEntity<EditorView>>,
    toolbar_button: crate::style::ControlStyle,
    toolbar_input: crate::style::TextInputStyles,
    ui_scale: f32,
    viewport: Bounds<Pixels>,
    layouts: Vec<(usize, Bounds<Pixels>)>,
    offsets: Vec<f32>,
    layout_width: f32,
    layout_zoom: f32,
    selection: Option<((usize, usize), (usize, usize))>,
    down: Option<Point<Pixels>>,
    selection_anchor: Option<((usize, usize), (usize, usize))>,
    focus: FocusHandle,
    background: Hsla,
    foreground: Hsla,
    accent: Hsla,
    selection_color: Hsla,
    search_color: Hsla,
    language: UiLanguage,
    pending_search: Option<(String, bool, bool, bool)>,
    query: Option<(String, bool, bool, bool)>,
    restore_anchor: Option<PdfViewState>,
    pending_position: Option<(usize, PdfPoint, Point<Pixels>)>,
    pub collapsed: HashSet<usize>,
    search_cancelled: Arc<AtomicBool>,
    window: Option<gpui::AnyWindowHandle>,
}

impl Drop for PdfView {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled.store(true, Ordering::Relaxed);
        retire_document(self.document_id);
    }
}

impl PdfView {
    fn new_with_source(
        source: PdfSource,
        focus: FocusHandle,
        theme: &crate::theme::Theme,
        language: UiLanguage,
        state: PdfViewState,
        cx: &mut Context<Self>,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let task = cx.spawn(async move |view, cx| {
            let request = token.clone();
            let scheduler = match &source {
                PdfSource::Local(path) => path.clone(),
                PdfSource::Remote(_, _) => super::remote::scheduler_key(),
            };
            let result = crate::platform::fs_task::run("read PDF", scheduler, move || {
                let bytes = match source {
                    PdfSource::Local(path) => {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        moeka_fs::local::native::File::open(path)?
                            .take(256 * 1024 * 1024 + 1)
                            .read_to_end(&mut bytes)?;
                        if bytes.len() > 256 * 1024 * 1024 {
                            return Err(std::io::Error::other(RuntimeText::PdfMemoryLimit));
                        }
                        bytes
                    }
                    PdfSource::Remote(filesystem, path) => {
                        filesystem
                            .read_file(&path, 256 * 1024 * 1024)
                            .map_err(|error| std::io::Error::other(error.to_string()))?
                            .0
                    }
                };
                Ok(Arc::<[u8]>::from(bytes))
            })
            .await;
            let bytes = match result {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = view.update(cx, |view, cx| {
                        if !Arc::ptr_eq(&view.cancelled, &request) {
                            return;
                        }
                        view.error = Some(error.to_string());
                        view.task = None;
                        cx.notify();
                    });
                    return;
                }
            };
            let permit = loop {
                if token.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(permit) = Permit::acquire() {
                    break permit;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            };
            let work_bytes = bytes.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _permit = permit;
                    let pdf = pdf::parse(work_bytes)?;
                    let info = pdf::inspect(&pdf)?;
                    Ok::<_, anyhow::Error>(info)
                })
                .await;
            let info = match result {
                Ok(info) => info,
                Err(error) => {
                    let _ = view.update(cx, |view, cx| {
                        if !Arc::ptr_eq(&view.cancelled, &request) {
                            return;
                        }
                        view.error = Some(error.to_string());
                        view.task = None;
                        cx.notify();
                    });
                    return;
                }
            };
            let work_bytes = bytes.clone();
            let _ = view.update(cx, |view, cx| {
                if !Arc::ptr_eq(&view.cancelled, &request) {
                    return;
                }
                retire_document(view.document_id);
                view.content_id = DOCUMENT_ID.fetch_add(1, Ordering::Relaxed);
                view.image_task = None;
                view.failed.clear();
                view.text = None;
                view.thumbnail_offsets.clear();
                view.selection = None;
                view.search_cancelled.store(true, Ordering::Relaxed);
                view.search_epoch = view.search_epoch.wrapping_add(1);
                view.search_task = None;
                view.search_matches.clear();
                view.search_current = None;
                view.pending_position = None;
                view.pending_search = view.query.clone();
                view.searching = view.pending_search.is_some();
                view.bytes = Some(bytes);
                view.info = Some(Arc::new(info));
                cx.notify();
            });
            let permit = loop {
                if token.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(permit) = Permit::acquire_background() {
                    break permit;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _permit = permit;
                    pdf::extract(&pdf::parse(work_bytes)?, &token)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if !Arc::ptr_eq(&view.cancelled, &request) {
                    return;
                }
                view.task = None;
                match result {
                    Ok(text) => {
                        if text.iter().any(|page| page.unmapped > 0) {
                            view.error =
                                Some(RuntimeText::PdfTextLimited.text(view.language).to_string());
                        } else if text.iter().all(|page| page.text.is_empty()) {
                            view.error =
                                Some(RuntimeText::PdfNoText.text(view.language).to_string());
                        }
                        view.text = Some(Arc::new(text));
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                if let Some((query, case, word, regex)) = view.pending_search.take() {
                    view.search(query, case, word, regex, cx);
                }
                cx.notify();
            });
        });
        let styles = crate::style::Styles::from_theme(theme, theme);
        Self {
            info: None,
            text: None,
            bytes: None,
            cancelled,
            task: Some(task),
            raster_task: None,
            raster_requested: Arc::new(std::sync::Mutex::new(HashSet::new())),
            document_id: DOCUMENT_ID.fetch_add(1, Ordering::Relaxed),
            content_id: 0,
            image_task: None,
            failed: HashSet::new(),
            error: None,
            search_matches: Vec::new(),
            search_current: None,
            search_task: None,
            search_epoch: 0,
            searching: false,
            search_error: None,
            page: state.page,
            scroll: 0.0,
            horizontal_scroll: 0.0,
            thumb_scroll: state.thumbnail_scroll.max(0.0),
            thumbnail_offsets: Vec::new(),
            thumbnail_focus: cx.focus_handle(),
            zoom: if state.zoom.is_finite() {
                state.zoom.clamp(0.25, 8.0)
            } else {
                1.0
            },
            rotation: 0,
            fit_page: false,
            page_input: None,
            zoom_input: None,
            toolbar_error: None,
            toolbar_editor: None,
            toolbar_button: styles.toolbar_button(26.0),
            toolbar_input: styles.input_styles(crate::style::InputRole::Stepper, 1.0),
            ui_scale: 1.0,
            viewport: Bounds::default(),
            layouts: Vec::new(),
            selection: None,
            offsets: Vec::new(),
            layout_width: 0.0,
            layout_zoom: 0.0,
            down: None,
            selection_anchor: None,
            focus,
            background: theme.bg_primary,
            foreground: theme.text_primary,
            accent: theme.text_accent,
            selection_color: theme.selection,
            search_color: theme.highlight_bg,
            language,
            pending_search: None,
            query: None,
            restore_anchor: Some(state),
            pending_position: None,
            collapsed: HashSet::new(),
            search_cancelled: Arc::new(AtomicBool::new(false)),
            window: None,
        }
    }

    pub fn reload(
        &mut self,
        path: PathBuf,
        theme: &crate::theme::Theme,
        language: UiLanguage,
        cx: &mut Context<Self>,
    ) {
        self.reload_source(PdfSource::Local(path), theme, language, cx);
    }

    pub(super) fn reload_remote(
        &mut self,
        filesystem: Arc<moeka_ssh::filesystem::SshFilesystem>,
        path: moeka_fs::path::NativePath,
        theme: &crate::theme::Theme,
        language: UiLanguage,
        cx: &mut Context<Self>,
    ) {
        self.reload_source(PdfSource::Remote(filesystem, path), theme, language, cx);
    }

    fn reload_source(
        &mut self,
        source: PdfSource,
        theme: &crate::theme::Theme,
        language: UiLanguage,
        cx: &mut Context<Self>,
    ) {
        let mut replacement = Self::new_with_source(
            source,
            self.focus.clone(),
            theme,
            language,
            self.state(),
            cx,
        );
        replacement.info = self.info.clone();
        replacement.text = self.text.clone();
        replacement.bytes = self.bytes.clone();
        replacement.content_id = self.content_id;
        replacement.query = self.query.clone();
        replacement.pending_search = self.query.clone();
        replacement.selection_color = self.selection_color;
        replacement.search_color = self.search_color;
        replacement.rotation = self.rotation;
        replacement.fit_page = self.fit_page;
        replacement.toolbar_editor = self.toolbar_editor.clone();
        replacement.thumbnail_focus = self.thumbnail_focus.clone();
        let old = self.document_id;
        let new = replacement.document_id;
        CACHE.with_borrow_mut(|cache| {
            let keys = cache
                .images
                .keys()
                .filter(|(document, _)| *document == old)
                .copied()
                .collect::<Vec<_>>();
            for key in keys {
                if let Some(image) = cache.images.remove(&key) {
                    cache.images.insert((new, key.1), image);
                }
            }
        });
        *self = replacement;
    }

    pub fn state(&self) -> PdfViewState {
        if let Some(anchor) = self.restore_anchor {
            return PdfViewState {
                zoom: self.zoom,
                thumbnail_scroll: self.thumb_scroll,
                ..anchor
            };
        }
        if self.info.is_none() || self.viewport.size.width <= px(0.0) {
            return self.restore_anchor.unwrap_or_default();
        }
        let info = self.info.as_ref().expect("PDF info loaded");
        let width = (f32::from(self.viewport.size.width) - 152.0).max(1.0);
        let page_index = self
            .offsets
            .partition_point(|offset| *offset + 8.0 <= self.scroll)
            .saturating_sub(1)
            .min(info.pages.len() - 1);
        let offset = self.offsets.get(page_index).copied().unwrap_or(0.0) + 8.0;
        let (page_width, page_height) = page_size(&info.pages[page_index], self.rotation);
        let height = page_height * width * self.zoom / page_width;
        let fraction = ((self.scroll - offset) / height).clamp(0.0, 1.0);
        PdfViewState {
            page: page_index,
            fraction,
            zoom: self.zoom,
            thumbnail_scroll: self.thumb_scroll,
        }
    }

    fn zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        if !zoom.is_finite() {
            return;
        }
        self.pending_position = None;
        self.fit_page = false;
        self.toolbar_error = None;
        self.restore_anchor = Some(self.state());
        self.zoom = zoom.clamp(0.25, 8.0);
        cx.notify();
    }

    fn zoom_at(&mut self, factor: f32, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self
            .layouts
            .iter()
            .min_by(|(_, a), (_, b)| {
                let y = position.y;
                let distance =
                    |rect: &Bounds<Pixels>| (rect.top() - y).max(px(0.0)).max(y - rect.bottom());
                f32::from(distance(a)).total_cmp(&f32::from(distance(b)))
            })
            .and_then(|&(page, rect)| {
                let info = self.info.as_ref()?.pages.get(page)?;
                let scale = f32::from(rect.size.width) / page_size(info, self.rotation).0;
                let transform = pdf::view_transform(info.width, info.height, self.rotation);
                Some((
                    page,
                    transform.inverse()
                        * PdfPoint::new(
                            f64::from(f32::from(position.x - rect.origin.x) / scale),
                            f64::from(f32::from(position.y - rect.origin.y) / scale),
                        ),
                    position - self.viewport.origin,
                ))
            });
        self.zoom(self.zoom * factor, cx);
        if let Some(anchor) = anchor {
            self.restore_anchor = None;
            self.pending_position = Some(anchor);
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let pages = self.text.as_ref()?;
        let mut selected = String::new();
        for page in start.0..=end.0 {
            let text = &pages.get(page)?.text;
            let from = if page == start.0 { start.1 } else { 0 };
            let to = if page == end.0 { end.1 } else { text.len() };
            if !selected.is_empty() {
                selected.push('\n');
            }
            selected.push_str(text.get(from..to)?);
        }
        (!selected.is_empty()).then_some(selected)
    }

    pub(super) fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(a, b)| a != b)
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if let Some(pages) = &self.text
            && let Some(last) = pages.last()
        {
            self.selection = Some(((0, 0), (pages.len() - 1, last.text.len())));
            cx.notify();
        }
    }

    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        if position.x < self.viewport.left() + px(128.0) {
            return None;
        }
        let pages = self.text.as_ref()?;
        for &(page, bounds) in &self.layouts {
            if !bounds.contains(&position) {
                continue;
            }
            let text = pages.get(page)?;
            let info = self.info.as_ref()?.pages.get(page)?;
            let scale = f64::from(f32::from(bounds.size.width) / page_size(info, self.rotation).0);
            let point = pdf::view_transform(info.width, info.height, self.rotation).inverse()
                * PdfPoint::new(
                    f64::from(f32::from(position.x - bounds.left())) / scale,
                    f64::from(f32::from(position.y - bounds.top())) / scale,
                );
            if text.hit(point).is_some() {
                return None;
            }
            if let Some((a, b)) = self.selection {
                let (start, end) = if a <= b { (a, b) } else { (b, a) };
                if start.0 <= page && page <= end.0 {
                    let from = if start.0 == page { start.1 } else { 0 };
                    let to = if end.0 == page {
                        end.1
                    } else {
                        text.text.len()
                    };
                    if text
                        .highlight_rects(&(from..to))
                        .iter()
                        .any(|rect| rect.contains(point))
                    {
                        return None;
                    }
                }
            }
            return text
                .images
                .iter()
                .rev()
                .find(|image| image.contains(point))
                .map(|image| (page, image.index));
        }
        None
    }

    pub(super) fn image_exporting(&self) -> bool {
        self.image_task.is_some()
    }

    pub(super) fn export_image(
        &mut self,
        page: usize,
        image: usize,
        save: bool,
        cx: &mut Context<Self>,
    ) {
        if self.image_task.is_some() {
            return;
        }
        let Some(bytes) = self.bytes.clone().filter(|_| {
            self.text
                .as_ref()
                .and_then(|pages| pages.get(page))
                .is_some_and(|page| page.images.iter().any(|entry| entry.index == image))
        }) else {
            self.error = Some(RuntimeText::DocumentUnavailable.text(self.language).into());
            cx.notify();
            return;
        };
        let content_id = self.content_id;
        let cancelled = self.cancelled.clone();
        let default_name = format!("image-{}-{}.png", page + 1, image + 1);
        #[cfg(not(target_os = "macos"))]
        let title =
            crate::i18n::translate(self.language, crate::i18n::TextKey::SaveFileDialog).to_string();
        self.image_task = Some(cx.spawn(async move |view, cx| {
            let result: anyhow::Result<()> = async {
                let destination = if save {
                    #[cfg(target_os = "macos")]
                    let path = {
                        let directory = crate::platform::paths::home_dir().unwrap_or_default();
                        let receiver = view.update(cx, |_, cx| {
                            cx.prompt_for_new_path(&directory, Some(&default_name))
                        })?;
                        receiver.await??
                    };
                    #[cfg(not(target_os = "macos"))]
                    let path =
                        cx.background_executor()
                            .spawn(async move {
                                super::helpers::save_file_blocking(&default_name, &title)
                            })
                            .await;
                    let Some(path) = path else {
                        return Ok(());
                    };
                    Some(path)
                } else {
                    None
                };
                let permit = loop {
                    if cancelled.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if let Some(permit) = Permit::acquire() {
                        break permit;
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                };
                let token = cancelled.clone();
                let png = cx
                    .background_executor()
                    .spawn(async move {
                        let _permit = permit;
                        let pdf = pdf::parse(bytes)?;
                        pdf::image_png(&pdf, page, image, &token)
                    })
                    .await?;
                if cancelled.load(Ordering::Relaxed)
                    || !view.update(cx, |view, _| view.content_id == content_id)?
                {
                    return Ok(());
                }
                if let Some(path) = destination {
                    let destination = path.clone();
                    crate::platform::fs_task::run("save image as", path, move || {
                        super::file_ops::atomic_write(&destination, &png)
                    })
                    .await?;
                } else {
                    view.update(cx, |_, cx| {
                        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png);
                        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
                    })?;
                }
                Ok(())
            }
            .await;
            let _ = view.update(cx, |view, cx| {
                view.image_task = None;
                if let Err(error) = result {
                    view.error = Some(error.to_string());
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn navigate(&mut self, target: &pdf::LinkTarget, cx: &mut Context<Self>) {
        if matches!(target, pdf::LinkTarget::Page { page, .. } | pdf::LinkTarget::Fit { page, .. } if self.info.as_ref().is_some_and(|info| *page >= info.pages.len()))
        {
            self.error = Some(
                RuntimeText::PdfUnsupportedDestination
                    .text(self.language)
                    .to_string(),
            );
            cx.notify();
            return;
        }
        self.pending_position = None;
        match target {
            pdf::LinkTarget::Page { page, point, zoom } => {
                if let Some(info) = &self.info
                    && let Some(info) = info.pages.get(*page)
                {
                    let (page_width, page_height) = page_size(info, self.rotation);
                    let transform = pdf::view_transform(info.width, info.height, self.rotation);
                    let point = point.map(|point| transform * point);
                    if let Some(zoom) = zoom {
                        self.fit_page = false;
                        self.zoom = (*zoom * page_width
                            / (f32::from(self.viewport.size.width) - 152.0).max(1.0))
                        .clamp(0.25, 8.0);
                    }
                    self.horizontal_scroll = point.map_or(0.0, |point| {
                        point.x.max(0.0) as f32
                            * (f32::from(self.viewport.size.width) - 152.0).max(1.0)
                            * self.zoom
                            / page_width
                    });
                    self.restore_anchor = Some(PdfViewState {
                        page: *page,
                        fraction: point.map_or(0.0, |point| point.y as f32 / page_height),
                        zoom: self.zoom,
                        thumbnail_scroll: self.thumb_scroll,
                    });
                    self.page = *page;
                }
            }
            pdf::LinkTarget::Fit {
                page,
                rectangle,
                width,
                height,
                point,
            } => {
                if let Some(info) = &self.info
                    && let Some(info) = info.pages.get(*page)
                {
                    self.fit_page = false;
                    let (page_width, page_height) = page_size(info, self.rotation);
                    let transform = pdf::view_transform(info.width, info.height, self.rotation);
                    let rectangle = rectangle.map(|rect| transform.transform_rect_bbox(rect));
                    let point = point.map(|point| transform * point);
                    let available_width = (f32::from(self.viewport.size.width) - 152.0).max(1.0);
                    let available_height = (f32::from(self.viewport.size.height) - 16.0).max(1.0);
                    let target_width = rectangle.map_or(page_width, |rect| rect.width() as f32);
                    let target_height = rectangle.map_or(page_height, |rect| rect.height() as f32);
                    let scale = match (*width, *height) {
                        (true, true) => {
                            (available_width / target_width).min(available_height / target_height)
                        }
                        (true, false) => available_width / target_width,
                        _ => available_height / target_height,
                    };
                    self.zoom = (scale * page_width / available_width).clamp(0.25, 8.0);
                    self.restore_anchor = Some(PdfViewState {
                        page: *page,
                        fraction: rectangle.map_or_else(
                            || point.map_or(0.0, |point| point.y as f32 / page_height),
                            |rect| rect.y0 as f32 / page_height,
                        ),
                        zoom: self.zoom,
                        thumbnail_scroll: self.thumb_scroll,
                    });
                    self.page = *page;
                }
            }
            pdf::LinkTarget::Url(url) => {
                if let Err(error) = crate::platform::shell::open_url(url) {
                    self.error = Some(error.to_string());
                }
            }
            pdf::LinkTarget::Unsupported => {
                self.error = Some(
                    RuntimeText::PdfUnsupportedDestination
                        .text(self.language)
                        .to_string(),
                )
            }
        }
        cx.notify();
    }

    pub fn search(
        &mut self,
        query: String,
        case: bool,
        word: bool,
        regex: bool,
        cx: &mut Context<Self>,
    ) {
        self.pending_position = None;
        self.query = (!query.is_empty()).then(|| (query.clone(), case, word, regex));
        self.search_cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.search_cancelled.clone();
        self.pending_search = None;
        self.search_epoch = self.search_epoch.wrapping_add(1);
        self.search_task = None;
        self.search_matches.clear();
        self.search_current = None;
        self.search_error = None;
        self.searching = !query.is_empty();
        let epoch = self.search_epoch;
        if query.is_empty() {
            self.searching = false;
            cx.notify();
            return;
        }
        let Some(text) = self.text.clone() else {
            if self.task.is_some() {
                self.pending_search = Some((query, case, word, regex));
            } else {
                self.searching = false;
                self.search_error = Some(RuntimeText::PdfNoText.text(self.language).to_string());
            }
            cx.notify();
            return;
        };
        self.search_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let permit = loop {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(permit) = Permit::acquire_background() {
                    break permit;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _permit = permit;
                    let pattern = if regex { query } else { regex::escape(&query) };
                    let matcher = regex::RegexBuilder::new(&pattern)
                        .case_insensitive(!case)
                        .build()?;
                    let mut found = Vec::new();
                    for (page, text) in text.iter().enumerate() {
                        if cancelled.load(Ordering::Relaxed) {
                            return Err(anyhow::anyhow!(RuntimeText::DocumentUnavailable));
                        }
                        for matched in matcher
                            .find_iter(&text.text)
                            .filter(|matched| !matched.is_empty())
                            .filter(|matched| {
                                !word
                                    || (super::search::is_word_boundary(
                                        &text.text,
                                        matched.start(),
                                    ) && super::search::is_word_boundary(
                                        &text.text,
                                        matched.end(),
                                    ))
                            })
                        {
                            if found.len() == 100_000 {
                                return Err(anyhow::anyhow!(RuntimeText::PdfSearchLimit));
                            }
                            found.push((page, matched.range()));
                        }
                    }
                    Ok::<_, anyhow::Error>(found)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.search_epoch != epoch {
                    return;
                }
                view.searching = false;
                match result {
                    Ok(matches) => {
                        view.search_matches = matches;
                        view.search_navigate(true, cx);
                    }
                    Err(error) => view.search_error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn search_navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.search_matches.is_empty() {
            return;
        }
        let count = self.search_matches.len();
        let index = self.search_current.map_or(0, |index| {
            if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            }
        });
        self.search_current = Some(index);
        let (page, range) = &self.search_matches[index];
        let rectangles = self
            .text
            .as_ref()
            .map(|pages| pages[*page].highlight_rects(range))
            .unwrap_or_default();
        let point = rectangles.first().map(|rect| rect.center());
        let page = *page;
        self.navigate(
            &pdf::LinkTarget::Page {
                page,
                point,
                zoom: None,
            },
            cx,
        );
        if let Some(point) = point {
            self.restore_anchor = None;
            self.pending_position = Some((
                page,
                point,
                gpui::point(
                    px(128.0) + (self.viewport.size.width - px(128.0)) * 0.5,
                    self.viewport.size.height * 0.5,
                ),
            ));
        }
    }

    fn queue(&mut self, requested: Vec<RasterKey>, cx: &mut Context<Self>) {
        *self
            .raster_requested
            .lock()
            .expect("PDF raster request mutex poisoned") = requested.iter().copied().collect();
        if self.raster_task.is_some() {
            return;
        }
        let mut seen = HashSet::new();
        let requested = requested
            .into_iter()
            .filter(|key| {
                seen.insert(*key)
                    && cached(self.document_id, *key).is_none()
                    && !self.failed.contains(key)
            })
            .take(4)
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return;
        }
        let Some(bytes) = self.bytes.clone() else {
            return;
        };
        let cancelled = self.cancelled.clone();
        let visible = self.raster_requested.clone();
        self.raster_task = Some(cx.spawn(async move |view, cx| {
            let request = cancelled.clone();
            let permit = loop {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(permit) = Permit::acquire() {
                    break permit;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _permit = permit;
                    let document = pdf::parse(bytes)?;
                    let cache = hayro::RenderCache::new();
                    let mut rendered = Vec::new();
                    for key in requested {
                        if cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        if !visible
                            .lock()
                            .expect("PDF raster request mutex poisoned")
                            .contains(&key)
                        {
                            continue;
                        }
                        rendered.push((
                            key,
                            pdf::raster_tile_rotated(
                                &document,
                                &cache,
                                key.0,
                                key.1 as f32 / 100.0,
                                key.2,
                                key.3,
                                key.4,
                            ),
                        ));
                    }
                    Ok::<_, anyhow::Error>(rendered)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if !Arc::ptr_eq(&view.cancelled, &request) {
                    return;
                }
                view.raster_task = None;
                match result {
                    Ok(rendered) => {
                        for (key, result) in rendered {
                            match result {
                                Ok((mut pixels, warnings)) => {
                                    if !warnings.is_empty() {
                                        view.error = Some(warnings.join("; "));
                                    }
                                    let bytes = pixels.len();
                                    for pixel in pixels.chunks_exact_mut(4) {
                                        pixel.swap(0, 2);
                                    }
                                    cache_image(
                                        view.document_id,
                                        key,
                                        Arc::new(RenderImage::new([image::Frame::new(pixels)])),
                                        bytes,
                                        view.window,
                                    );
                                }
                                Err(error) => {
                                    view.failed.insert(key);
                                    view.error = Some(error.to_string());
                                }
                            }
                        }
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
    }

    fn hit(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        if position.x < self.viewport.origin.x + px(128.0) {
            return None;
        }
        let pages = self.text.as_ref()?;
        self.layouts
            .iter()
            .filter_map(|&(page, bounds)| {
                let info = self.info.as_ref()?.pages.get(page)?;
                let scale = f32::from(bounds.size.width) / page_size(info, self.rotation).0;
                let point = pdf::view_transform(info.width, info.height, self.rotation).inverse()
                    * PdfPoint::new(
                        f32::from(position.x - bounds.origin.x) as f64 / f64::from(scale),
                        f32::from(position.y - bounds.origin.y) as f64 / f64::from(scale),
                    );
                pages
                    .get(page)?
                    .nearest_hit(point)
                    .map(|(cluster, distance)| {
                        (
                            (page, cluster.range.start),
                            distance * f64::from(scale).powi(2),
                        )
                    })
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(hit, _)| hit)
    }
}

impl PdfView {
    fn center_page(&mut self, cx: &mut Context<Self>) {
        if let Some(info) = self
            .info
            .as_ref()
            .and_then(|info| info.pages.get(self.page))
        {
            self.restore_anchor = None;
            self.pending_position = Some((
                self.page,
                PdfPoint::new(f64::from(info.width) * 0.5, f64::from(info.height) * 0.5),
                point(
                    px(128.0) + (self.viewport.size.width - px(128.0)) * 0.5,
                    self.viewport.size.height * 0.5,
                ),
            ));
        }
        cx.notify();
    }

    fn toggle_fit(&mut self, cx: &mut Context<Self>) {
        let fit_page = !self.fit_page;
        self.zoom(1.0, cx);
        self.fit_page = fit_page;
        self.center_page(cx);
    }

    fn rotate_left(&mut self, cx: &mut Context<Self>) {
        self.toolbar_error = None;
        self.rotation = (self.rotation + 1) % 4;
        self.offsets.clear();
        self.thumbnail_offsets.clear();
        self.center_page(cx);
    }

    fn layout_thumbnails(&mut self) {
        let Some(info) = &self.info else { return };
        if self.thumbnail_offsets.len() != info.pages.len() + 1 {
            self.thumbnail_offsets.clear();
            self.thumbnail_offsets.push(0.0);
            for page in &info.pages {
                let height = thumbnail_size(page, self.rotation).1;
                self.thumbnail_offsets
                    .push(self.thumbnail_offsets.last().copied().unwrap_or(0.0) + height + 34.0);
            }
        }
    }

    fn thumbnail_at(&self, position: Point<Pixels>) -> Option<usize> {
        if !self.viewport.contains(&position) || position.x >= self.viewport.left() + px(128.0) {
            return None;
        }
        let y = f32::from(position.y - self.viewport.top()) + self.thumb_scroll;
        let index = self
            .thumbnail_offsets
            .partition_point(|offset| *offset <= y)
            .checked_sub(1)?;
        (index + 1 < self.thumbnail_offsets.len()).then_some(index)
    }

    fn step_thumbnail(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(last) = self
            .info
            .as_ref()
            .and_then(|info| info.pages.len().checked_sub(1))
        else {
            return;
        };
        let page = if forward {
            self.page.saturating_add(1).min(last)
        } else {
            self.page.saturating_sub(1)
        };
        self.navigate(
            &pdf::LinkTarget::Page {
                page,
                point: None,
                zoom: None,
            },
            cx,
        );
        self.layout_thumbnails();
        let top = self.thumbnail_offsets[page];
        let bottom = self.thumbnail_offsets[page + 1];
        let height = f32::from(self.viewport.size.height);
        if top < self.thumb_scroll {
            self.thumb_scroll = top;
        } else if bottom > self.thumb_scroll + height {
            self.thumb_scroll = (bottom - height).max(0.0);
        }
        cx.notify();
    }

    fn commit_toolbar_input(&mut self, page: bool, value: &str, cx: &mut Context<Self>) -> bool {
        use crate::i18n::{PdfControlText, pdf_control_label};
        let valid = if page {
            value
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|number| {
                    *number > 0
                        && self
                            .info
                            .as_ref()
                            .is_some_and(|info| *number <= info.pages.len())
                })
                .map(|number| {
                    self.page = number - 1;
                    self.restore_anchor = Some(PdfViewState {
                        page: self.page,
                        fraction: 0.0,
                        zoom: self.zoom,
                        thumbnail_scroll: self.thumb_scroll,
                    });
                    self.pending_position = None;
                })
                .is_some()
        } else {
            value
                .trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|number| number.is_finite() && (25.0..=800.0).contains(number))
                .map(|number| self.zoom(number / 100.0, cx))
                .is_some()
        };
        self.toolbar_error = (!valid).then(|| {
            pdf_control_label(
                self.language,
                if page {
                    PdfControlText::InvalidPage
                } else {
                    PdfControlText::InvalidZoom
                },
            )
            .to_string()
        });
        cx.notify();
        valid
    }

    fn toolbar_tip(&self, child: impl IntoElement, title: &'static str) -> AnyElement {
        if let Some(editor) = self
            .toolbar_editor
            .as_ref()
            .and_then(|editor| editor.upgrade())
        {
            super::tooltip::with_tooltip(child, editor, title, None::<String>)
        } else {
            child.into_any_element()
        }
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::i18n::{PdfControlText, pdf_control_label};
        use crate::text_input::{TextInput, TextInputEvent};
        for page in [true, false] {
            if (if page {
                &self.page_input
            } else {
                &self.zoom_input
            })
            .is_none()
            {
                let input = cx.new(|cx| TextInput::new(self.toolbar_input.clone(), "", cx));
                cx.subscribe_in(
                    &input,
                    window,
                    move |view, input, event, window, cx| match event {
                        TextInputEvent::Submit | TextInputEvent::SubmitBackward => {
                            let text = input.read(cx).text.clone();
                            if view.commit_toolbar_input(page, &text, cx) {
                                window.focus(&view.focus);
                            }
                        }
                        TextInputEvent::Escape => {
                            view.toolbar_error = None;
                            window.focus(&view.focus);
                            cx.notify();
                        }
                        _ => {}
                    },
                )
                .detach();
                if page {
                    self.page_input = Some(input);
                } else {
                    self.zoom_input = Some(input);
                }
            }
        }
        let page_input = self.page_input.as_ref().expect("PDF page input").clone();
        let zoom_input = self.zoom_input.as_ref().expect("PDF zoom input").clone();
        let total = self.info.as_ref().map_or(0, |info| info.pages.len());
        for (input, value) in [
            (
                &page_input,
                if total == 0 {
                    "0".to_string()
                } else {
                    (self.page + 1).to_string()
                },
            ),
            (&zoom_input, format!("{:.0}", self.zoom * 100.0)),
        ] {
            input.update(cx, |input, cx| {
                input.styles = self.toolbar_input.clone();
                input.styles.text_input_height_px = 26.0 * self.ui_scale;
                if !input.focus_handle.is_focused(window) && input.text != value {
                    input.set_text(&value, cx);
                }
            });
        }
        let button = |id: &'static str, path: &'static str| {
            crate::ui::button::icon_button(
                id,
                self.toolbar_button,
                crate::style::ControlState {
                    disabled: total == 0,
                    ..Default::default()
                },
                self.ui_scale,
            )
            .debug_selector(move || id.to_string())
            .child(
                gpui::svg()
                    .path(path)
                    .size(px(16.0 * self.ui_scale))
                    .text_color(self.foreground),
            )
        };
        let fit_title = pdf_control_label(
            self.language,
            if self.fit_page {
                PdfControlText::FitWidth
            } else {
                PdfControlText::FitPage
            },
        );
        let fit_icon = if self.fit_page {
            super::assets::PREVIEW_FULLSCREEN_EXIT_SVG_PATH
        } else {
            super::assets::PREVIEW_FULLSCREEN_SVG_PATH
        };
        let controls = div()
            .id("pdf-toolbar-controls")
            .debug_selector(|| "pdf-toolbar-controls".into())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.0 * self.ui_scale))
            .child(
                self.toolbar_tip(
                    div()
                        .id("pdf-page-input")
                        .debug_selector(|| "pdf-page-input".into())
                        .w(px(
                            (total.to_string().len().max(3) as f32 * 8.0 + 8.0) * self.ui_scale
                        ))
                        .child(page_input),
                    pdf_control_label(self.language, PdfControlText::Page),
                ),
            )
            .child(div().whitespace_nowrap().child(format!("/ {total}")))
            .child(
                div()
                    .mx(px(6.0 * self.ui_scale))
                    .w(px(1.0))
                    .h(px(16.0 * self.ui_scale))
                    .bg(crate::theme::with_alpha(self.foreground, 0.15)),
            )
            .child(self.toolbar_tip(
                button("pdf-zoom-out", super::assets::PREVIEW_ZOOM_OUT_SVG_PATH).on_click(
                    cx.listener(|view, _, window, cx| {
                        window.focus(&view.focus);
                        view.zoom(view.zoom / 1.2, cx);
                    }),
                ),
                crate::i18n::preview_action_label(self.language, "zoom_out"),
            ))
            .child(
                self.toolbar_tip(
                    div()
                        .id("pdf-zoom-input")
                        .debug_selector(|| "pdf-zoom-input".into())
                        .w(px(44.0 * self.ui_scale))
                        .child(zoom_input),
                    pdf_control_label(self.language, PdfControlText::Zoom),
                ),
            )
            .child("%")
            .child(self.toolbar_tip(
                button("pdf-zoom-in", super::assets::PREVIEW_ZOOM_IN_SVG_PATH).on_click(
                    cx.listener(|view, _, window, cx| {
                        window.focus(&view.focus);
                        view.zoom(view.zoom * 1.2, cx);
                    }),
                ),
                crate::i18n::preview_action_label(self.language, "zoom_in"),
            ))
            .child(
                div()
                    .mx(px(6.0 * self.ui_scale))
                    .w(px(1.0))
                    .h(px(16.0 * self.ui_scale))
                    .bg(crate::theme::with_alpha(self.foreground, 0.15)),
            )
            .child(self.toolbar_tip(
                button("pdf-fit-page", fit_icon).on_click(cx.listener(|view, _, window, cx| {
                    window.focus(&view.focus);
                    view.toggle_fit(cx);
                })),
                fit_title,
            ))
            .child(self.toolbar_tip(
                button("pdf-rotate-left", "assets/icons/preview/rotate-left.svg").on_click(
                    cx.listener(|view, _, window, cx| {
                        window.focus(&view.focus);
                        view.rotate_left(cx);
                    }),
                ),
                pdf_control_label(self.language, PdfControlText::Rotate),
            ));
        div()
            .cursor(gpui::CursorStyle::Arrow)
            .ml(px(128.0))
            .min_w(px(0.0))
            .h(px(34.0 * self.ui_scale))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0 * self.ui_scale))
            .child(controls)
            .into_any_element()
    }
}

impl Render for PdfView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(window, cx);
        let view = cx.entity();
        let paint_view = view.clone();
        let info = self.info.clone();
        let loading = self.task.is_some() && info.is_none();
        let message = self
            .toolbar_error
            .clone()
            .or_else(|| self.error.clone())
            .or_else(|| self.search_error.clone())
            .unwrap_or_else(|| {
                if loading {
                    RuntimeText::PdfLoading.text(self.language).to_string()
                } else {
                    String::new()
                }
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(self.background)
            .text_color(self.foreground)
            .child(toolbar)
            .when(!message.is_empty(), |element| {
                element.child(
                    div()
                        .ml(px(128.0))
                        .px_2()
                        .text_size(px(12.0))
                        .child(message),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .overflow_hidden()
                    .on_scroll_wheel(cx.listener(|view, event: &gpui::ScrollWheelEvent, _, cx| {
                        let delta = f32::from(event.delta.pixel_delta(px(16.0)).y);
                        if event.position.x < view.viewport.origin.x + px(128.0) {
                            view.thumb_scroll = (view.thumb_scroll - delta).max(0.0);
                        } else if event.modifiers.platform || event.modifiers.control {
                            if let Some(factor) = super::preview_overlay::wheel_zoom_factor(
                                f32::from(event.delta.pixel_delta(px(40.0)).y),
                            ) {
                                view.zoom_at(factor, event.position, cx);
                            }
                        } else {
                            view.pending_position = None;
                            view.restore_anchor = None;
                            view.scroll = (view.scroll - delta).max(0.0);
                            view.horizontal_scroll = (view.horizontal_scroll
                                - f32::from(event.delta.pixel_delta(px(16.0)).x))
                            .max(0.0);
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&view.focus);
                            view.down = Some(event.position);
                            view.selection_anchor = None;
                            if let Some(hit) = view.hit(event.position) {
                                view.selection = Some((hit, hit));
                                if let Some(cluster) = view.text.as_ref().and_then(|pages| {
                                    pages[hit.0]
                                        .clusters
                                        .iter()
                                        .find(|cluster| cluster.range.start == hit.1)
                                }) {
                                    view.selection_anchor = Some((hit, (hit.0, cluster.range.end)));
                                }
                                if event.click_count >= 2
                                    && let Some(pages) = &view.text
                                {
                                    use unicode_segmentation::UnicodeSegmentation;
                                    if let Some((start, word)) = pages[hit.0]
                                        .text
                                        .split_word_bound_indices()
                                        .find(|(start, word)| {
                                            *start <= hit.1 && *start + word.len() > hit.1
                                        })
                                    {
                                        view.selection =
                                            Some(((hit.0, start), (hit.0, start + word.len())));
                                        view.down = None;
                                    }
                                }
                            } else {
                                view.selection = None;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|view, event: &gpui::MouseMoveEvent, _, cx| {
                        if view.down.is_some()
                            && let Some(hit) = view.hit(event.position)
                            && let Some((start, anchor_end)) = view.selection_anchor
                        {
                            let end = view
                                .text
                                .as_ref()
                                .and_then(|pages| {
                                    pages[hit.0]
                                        .clusters
                                        .iter()
                                        .find(|cluster| cluster.range.start == hit.1)
                                })
                                .map_or(hit.1, |cluster| cluster.range.end);
                            view.selection = Some(if hit < start {
                                (anchor_end, hit)
                            } else {
                                (start, (hit.0, end))
                            });
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, event: &gpui::MouseUpEvent, _, cx| {
                            let click = view
                                .down
                                .take()
                                .is_some_and(|down| (event.position - down).magnitude() < 4.0);
                            if click && let Some(info) = &view.info {
                                let mut target = None;
                                for &(page, bounds) in &view.layouts {
                                    if bounds.contains(&event.position) {
                                        let page_info = &info.pages[page];
                                        let scale = f32::from(bounds.size.width)
                                            / page_size(page_info, view.rotation).0;
                                        let point = pdf::view_transform(
                                            page_info.width,
                                            page_info.height,
                                            view.rotation,
                                        )
                                        .inverse()
                                            * PdfPoint::new(
                                                f32::from(event.position.x - bounds.origin.x)
                                                    as f64
                                                    / scale as f64,
                                                f32::from(event.position.y - bounds.origin.y)
                                                    as f64
                                                    / scale as f64,
                                            );
                                        target = info.pages[page]
                                            .links
                                            .iter()
                                            .find(|link| link.contains(point))
                                            .map(|link| link.target.clone());
                                    }
                                }
                                if let Some(target) = target {
                                    view.navigate(&target, cx);
                                }
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            view.down = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                retire_images(window, cx);
                                let hitbox = window.insert_hitbox(
                                    Bounds::new(
                                        bounds.origin + point(px(128.0), px(0.0)),
                                        size(
                                            (bounds.size.width - px(128.0)).max(px(0.0)),
                                            bounds.size.height,
                                        ),
                                    ),
                                    gpui::HitboxBehavior::Normal,
                                );
                                let paint = view.update(cx, |view, cx| {
                                    view.window = Some(window.window_handle());
                                    if view.viewport.size.width != bounds.size.width
                                        && view.info.is_some()
                                        && view.viewport.size.width > px(0.0)
                                        && view.restore_anchor.is_none()
                                    {
                                        view.restore_anchor = Some(view.state());
                                    }
                                    let resized = view.viewport.size != bounds.size;
                                    view.viewport = bounds;
                                    let Some(info) = view.info.clone() else {
                                        return Vec::new();
                                    };
                                    let width = (f32::from(bounds.size.width) - 152.0).max(1.0);
                                    let height = f32::from(bounds.size.height);
                                    let previous_page = view.page;
                                    let previous_zoom = view.zoom;
                                    if resized && view.fit_page {
                                        view.center_page(cx);
                                    }
                                    if view.fit_page
                                        && (resized
                                            || view.pending_position.is_some()
                                            || view.restore_anchor.is_some())
                                    {
                                        let (pw, ph) = page_size(
                                            &info.pages[view.page.min(info.pages.len() - 1)],
                                            view.rotation,
                                        );
                                        view.zoom =
                                            ((height - 16.0).max(1.0) * pw / (width * ph)).min(1.0);
                                    }
                                    if view.layout_width != width
                                        || view.layout_zoom != view.zoom
                                        || view.offsets.len() != info.pages.len() + 1
                                    {
                                        view.offsets.clear();
                                        view.offsets.push(0.0);
                                        for page in &info.pages {
                                            let (page_width, page_height) =
                                                page_size(page, view.rotation);
                                            view.offsets.push(
                                                view.offsets.last().copied().unwrap_or(0.0)
                                                    + page_height * width * view.zoom / page_width
                                                    + 16.0,
                                            );
                                        }
                                        view.layout_width = width;
                                        view.layout_zoom = view.zoom;
                                    }
                                    if let Some(state) = view.restore_anchor.take() {
                                        let page = state.page.min(info.pages.len() - 1);
                                        let (pw, ph) = page_size(&info.pages[page], view.rotation);
                                        view.scroll = 8.0
                                            + view.offsets[page]
                                            + ph * width * view.zoom / pw
                                                * if state.fraction.is_finite() {
                                                    state.fraction.clamp(0.0, 1.0)
                                                } else {
                                                    0.0
                                                };
                                    }
                                    let repositioned = view.pending_position.is_some();
                                    if let Some((page, target, position)) =
                                        view.pending_position.take()
                                    {
                                        let page_info = &info.pages[page];
                                        let target = pdf::view_transform(
                                            page_info.width,
                                            page_info.height,
                                            view.rotation,
                                        ) * target;
                                        let scale = width * view.zoom
                                            / page_size(page_info, view.rotation).0;
                                        view.scroll =
                                            8.0 + view.offsets[page] + target.y as f32 * scale
                                                - f32::from(position.y);
                                        view.horizontal_scroll = 140.0
                                            + (width - width * view.zoom).max(0.0) * 0.5
                                            + target.x as f32 * scale
                                            - f32::from(position.x);
                                    }
                                    view.horizontal_scroll = view
                                        .horizontal_scroll
                                        .clamp(0.0, (width * view.zoom - width).max(0.0));
                                    let total =
                                        *view.offsets.last().expect("PDF page offsets populated");
                                    view.scroll = view.scroll.clamp(0.0, (total - height).max(0.0));
                                    view.layout_thumbnails();
                                    view.thumb_scroll = view.thumb_scroll.clamp(
                                        0.0,
                                        (view.thumbnail_offsets.last().copied().unwrap_or(0.0)
                                            - height)
                                            .max(0.0),
                                    );
                                    let mut requested = Vec::new();
                                    let mut paint = Vec::new();
                                    view.layouts.clear();
                                    let first = view
                                        .offsets
                                        .partition_point(|offset| *offset + 8.0 <= view.scroll)
                                        .saturating_sub(1);
                                    let end = view
                                        .offsets
                                        .partition_point(|offset| *offset < view.scroll + height)
                                        .min(info.pages.len());
                                    for index in first..end {
                                        let page = &info.pages[index];
                                        let (page_width, page_height) =
                                            page_size(page, view.rotation);
                                        let y = view.offsets[index] + 8.0 - view.scroll;
                                        let scale = width * view.zoom / page_width;
                                        let rect = Bounds::new(
                                            point(
                                                bounds.origin.x
                                                    + px(140.0
                                                        + (width - page_width * scale).max(0.0)
                                                            * 0.5
                                                        - view.horizontal_scroll),
                                                bounds.origin.y + px(y),
                                            ),
                                            size(px(page_width * scale), px(page_height * scale)),
                                        );
                                        view.layouts.push((index, rect));
                                        if y < height && y + page_height * scale > 0.0 {
                                            let factor = (scale * window.scale_factor() * 100.0)
                                                .ceil()
                                                .max(1.0)
                                                as u32;
                                            let raster_width =
                                                (page_width * factor as f32 / 100.0).ceil();
                                            let raster_height =
                                                (page_height * factor as f32 / 100.0).ceil();
                                            let unit_x = f32::from(rect.size.width) / raster_width;
                                            let unit_y =
                                                f32::from(rect.size.height) / raster_height;
                                            let visible = rect.intersect(&bounds);
                                            let first_column =
                                                (f32::from(visible.origin.x - rect.origin.x)
                                                    / unit_x
                                                    / pdf::TILE_SIZE as f32)
                                                    .floor()
                                                    .max(0.0)
                                                    as u32;
                                            let last_column = (f32::from(
                                                visible.origin.x + visible.size.width
                                                    - rect.origin.x,
                                            ) / unit_x
                                                / pdf::TILE_SIZE as f32)
                                                .ceil()
                                                .max(0.0)
                                                as u32;
                                            let first_row =
                                                (f32::from(visible.origin.y - rect.origin.y)
                                                    / unit_y
                                                    / pdf::TILE_SIZE as f32)
                                                    .floor()
                                                    .max(0.0)
                                                    as u32;
                                            let last_row = (f32::from(
                                                visible.origin.y + visible.size.height
                                                    - rect.origin.y,
                                            ) / unit_y
                                                / pdf::TILE_SIZE as f32)
                                                .ceil()
                                                .max(0.0)
                                                as u32;
                                            let mut tiles = Vec::new();
                                            let preview = (
                                                index,
                                                (51200.0 / page_width.max(page_height))
                                                    .floor()
                                                    .max(1.0)
                                                    as u32,
                                                0,
                                                0,
                                                view.rotation,
                                            );
                                            requested.push(preview);
                                            for row in first_row..last_row {
                                                for column in first_column..last_column {
                                                    let key =
                                                        (index, factor, column, row, view.rotation);
                                                    requested.push(key);
                                                    let x = column as f32 * pdf::TILE_SIZE as f32;
                                                    let y = row as f32 * pdf::TILE_SIZE as f32;
                                                    let tile = Bounds::new(
                                                        point(
                                                            rect.origin.x + px(x * unit_x),
                                                            rect.origin.y + px(y * unit_y),
                                                        ),
                                                        size(
                                                            px((raster_width - x)
                                                                .min(pdf::TILE_SIZE as f32)
                                                                * unit_x),
                                                            px((raster_height - y)
                                                                .min(pdf::TILE_SIZE as f32)
                                                                * unit_y),
                                                        ),
                                                    );
                                                    tiles.push((
                                                        tile,
                                                        cached(view.document_id, key),
                                                    ));
                                                }
                                            }
                                            if tiles.iter().any(|(_, image)| image.is_none()) {
                                                let mut fallback = fallback_tiles(
                                                    view.document_id,
                                                    index,
                                                    view.rotation,
                                                    (page_width, page_height),
                                                    rect,
                                                );
                                                fallback.extend(tiles);
                                                tiles = fallback;
                                            }
                                            paint.push((rect, tiles, index, false));
                                        }
                                    }
                                    if !view.layouts.iter().any(|&(page, rect)| {
                                        page == view.page
                                            && rect.top() >= bounds.top()
                                            && rect.bottom() <= bounds.bottom()
                                    }) && let Some((page, _)) =
                                        view.layouts.iter().max_by(|(_, a), (_, b)| {
                                            f32::from(a.intersect(&bounds).size.height).total_cmp(
                                                &f32::from(b.intersect(&bounds).size.height),
                                            )
                                        })
                                    {
                                        view.page = *page;
                                    }
                                    let first_thumbnail = view
                                        .thumbnail_offsets
                                        .partition_point(|offset| *offset <= view.thumb_scroll)
                                        .saturating_sub(1);
                                    let last_thumbnail =
                                        view.thumbnail_offsets.partition_point(|offset| {
                                            *offset < view.thumb_scroll + height
                                        });
                                    for index in
                                        first_thumbnail..last_thumbnail.min(info.pages.len())
                                    {
                                        let page = &info.pages[index];
                                        let (thumbnail_width, thumbnail_height, ts) =
                                            thumbnail_size(page, view.rotation);
                                        let ty =
                                            view.thumbnail_offsets[index] - view.thumb_scroll + 8.0;
                                        if ty < height && ty + thumbnail_height + 18.0 > 0.0 {
                                            let rect = Bounds::new(
                                                point(
                                                    bounds.origin.x
                                                        + px((128.0 - thumbnail_width) * 0.5),
                                                    bounds.origin.y + px(ty),
                                                ),
                                                size(px(thumbnail_width), px(thumbnail_height)),
                                            );
                                            let key = (
                                                index,
                                                (ts * window.scale_factor() * 100.0).ceil().max(1.0)
                                                    as u32,
                                                0,
                                                0,
                                                view.rotation,
                                            );
                                            requested.push(key);
                                            paint.push((
                                                rect,
                                                vec![(rect, cached(view.document_id, key))],
                                                index,
                                                true,
                                            ));
                                        }
                                    }
                                    if repositioned
                                        || previous_page != view.page
                                        || previous_zoom != view.zoom
                                    {
                                        cx.notify();
                                    }
                                    view.queue(requested, cx);
                                    paint
                                });
                                (paint, hitbox)
                            },
                            move |bounds, (paint, hitbox), window, cx| {
                                let magnify_view = paint_view.clone();
                                window.on_mouse_event(
                                    move |event: &gpui::MagnifyEvent, phase, window, cx| {
                                        if phase == gpui::DispatchPhase::Bubble
                                            && hitbox.is_hovered(window)
                                            && event.delta.is_finite()
                                            && event.delta.abs() >= 0.0001
                                        {
                                            magnify_view.update(cx, |view, cx| {
                                                view.zoom_at(
                                                    (1.0 + event.delta).clamp(0.5, 2.0),
                                                    event.position,
                                                    cx,
                                                )
                                            });
                                            cx.stop_propagation();
                                        }
                                    },
                                );
                                let mut labels = Vec::new();
                                let mut paint_error = None;
                                window.with_content_mask(
                                    Some(gpui::ContentMask { bounds }),
                                    |window| {
                                        let view = paint_view.read(cx);
                                        for (rect, tiles, index, thumbnail) in paint {
                                            let content_bounds = if thumbnail {
                                                bounds
                                            } else {
                                                Bounds::new(
                                                    point(
                                                        bounds.origin.x + px(128.0),
                                                        bounds.origin.y,
                                                    ),
                                                    size(
                                                        (bounds.size.width - px(128.0))
                                                            .max(px(0.0)),
                                                        bounds.size.height,
                                                    ),
                                                )
                                            };
                                            if thumbnail && index == view.page {
                                                window.paint_quad(fill(
                                                    Bounds::new(
                                                        rect.origin - point(px(2.0), px(2.0)),
                                                        rect.size + size(px(4.0), px(4.0)),
                                                    ),
                                                    view.accent,
                                                ));
                                            }
                                            window.paint_quad(fill(
                                                rect.intersect(&content_bounds),
                                                gpui::white(),
                                            ));
                                            for (tile, image) in tiles {
                                                if let Some(image) = image {
                                                    if let Err(error) = window.with_content_mask(
                                                        Some(gpui::ContentMask {
                                                            bounds: content_bounds,
                                                        }),
                                                        |window| {
                                                            window.paint_image(
                                                                tile,
                                                                Corners::all(px(0.0)),
                                                                image,
                                                                0,
                                                                false,
                                                            )
                                                        },
                                                    ) {
                                                        paint_error = Some(format!(
                                                            "{}: {error}",
                                                            RuntimeText::RenderFailed
                                                        ));
                                                    }
                                                }
                                            }
                                            if thumbnail {
                                                labels.push((index, rect, view.foreground));
                                                continue;
                                            }
                                            let Some(pages) = &view.text else {
                                                continue;
                                            };
                                            let info =
                                                &view.info.as_ref().expect("PDF info loaded").pages
                                                    [index];
                                            let scale = f32::from(rect.size.width)
                                                / page_size(info, view.rotation).0;
                                            let transform = pdf::view_transform(
                                                info.width,
                                                info.height,
                                                view.rotation,
                                            );
                                            let start = view
                                                .search_matches
                                                .partition_point(|(page, _)| *page < index);
                                            let end = view
                                                .search_matches
                                                .partition_point(|(page, _)| *page <= index);
                                            let matches = &view.search_matches[start..end];
                                            let mut highlights = Vec::new();
                                            for (offset, (_, range)) in matches.iter().enumerate() {
                                                let active =
                                                    view.search_current == Some(start + offset);
                                                let color = crate::theme::with_alpha(
                                                    view.search_color,
                                                    if active { 0.5 } else { 0.28 },
                                                );
                                                highlights.extend(
                                                    pages[index]
                                                        .highlight_rects(range)
                                                        .into_iter()
                                                        .map(|rect| (rect, color)),
                                                );
                                            }
                                            if let Some((a, b)) = view.selection {
                                                let (start, end) =
                                                    if a <= b { (a, b) } else { (b, a) };
                                                if start.0 <= index && index <= end.0 {
                                                    let from =
                                                        if start.0 == index { start.1 } else { 0 };
                                                    let to = if end.0 == index {
                                                        end.1
                                                    } else {
                                                        pages[index].text.len()
                                                    };
                                                    highlights.extend(
                                                        pages[index]
                                                            .highlight_rects(&(from..to))
                                                            .into_iter()
                                                            .map(|rect| {
                                                                (rect, view.selection_color)
                                                            }),
                                                    );
                                                }
                                            }
                                            for (geometry, color) in highlights {
                                                let geometry =
                                                    transform.transform_rect_bbox(geometry);
                                                let highlight = Bounds::new(
                                                    point(
                                                        rect.origin.x
                                                            + px(geometry.x0 as f32 * scale),
                                                        rect.origin.y
                                                            + px(geometry.y0 as f32 * scale),
                                                    ),
                                                    size(
                                                        px(geometry.width() as f32 * scale),
                                                        px(geometry.height() as f32 * scale),
                                                    ),
                                                );
                                                window.paint_quad(fill(
                                                    highlight.intersect(&content_bounds),
                                                    color,
                                                ));
                                            }
                                        }
                                    },
                                );
                                window.with_content_mask(
                                    Some(gpui::ContentMask { bounds }),
                                    |window| {
                                        for (index, rect, color) in labels {
                                            let text = (index + 1).to_string();
                                            let len = text.len();
                                            let font = window.text_style().font();
                                            let line = window.text_system().shape_line(
                                                text.into(),
                                                px(11.0),
                                                &[gpui::TextRun {
                                                    len,
                                                    font,
                                                    color,
                                                    background_color: None,
                                                    underline: None,
                                                    strikethrough: None,
                                                }],
                                                None,
                                            );
                                            if let Err(error) = line.paint(
                                                thumbnail_label_origin(rect, line.width),
                                                px(15.0),
                                                window,
                                                cx,
                                            ) {
                                                paint_error = Some(format!(
                                                    "{}: {error}",
                                                    RuntimeText::RenderFailed
                                                ));
                                            }
                                        }
                                    },
                                );
                                if let Some(error) = paint_error {
                                    paint_view.update(cx, |view, cx| {
                                        if view.error.is_none() {
                                            view.error = Some(error);
                                            cx.notify();
                                        }
                                    });
                                }
                            },
                        )
                        .size_full(),
                    )
                    .child(
                        div()
                            .id("pdf-thumbnails")
                            .debug_selector(|| "pdf-thumbnails".into())
                            .track_focus(&self.thumbnail_focus)
                            .key_context("MarkdownEditor")
                            .on_action(cx.listener(|view, _: &crate::actions::Up, _, cx| {
                                view.step_thumbnail(false, cx);
                                cx.stop_propagation();
                            }))
                            .on_action(cx.listener(|view, _: &crate::actions::Down, _, cx| {
                                view.step_thumbnail(true, cx);
                                cx.stop_propagation();
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                                    window.focus(&view.thumbnail_focus);
                                    view.down = None;
                                    view.selection_anchor = None;
                                    if let Some(page) = view.thumbnail_at(event.position) {
                                        view.navigate(
                                            &pdf::LinkTarget::Page {
                                                page,
                                                point: None,
                                                zoom: None,
                                            },
                                            cx,
                                        );
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(128.0))
                            .cursor(gpui::CursorStyle::Arrow),
                    ),
            )
    }
}

impl EditorView {
    pub(super) fn open_pdf_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.active_pdf_view() else {
            return;
        };
        let view = view.read(cx);
        if !view.viewport.contains(&position) || position.x < view.viewport.left() + px(128.0) {
            return;
        }
        let kind = super::ContextMenuKind::Pdf {
            tab_id: self.active_tab_id.clone().expect("PDF tab active"),
            content_id: view.content_id,
            image: view.image_at(position),
        };
        window.focus(&self.focus_handle);
        self.open_context_menu(super::ContextMenu { position, kind }, true, cx);
        cx.stop_propagation();
    }

    pub(super) fn pdf_menu_view(
        &self,
        tab_id: &str,
        content_id: usize,
        cx: &gpui::App,
    ) -> Option<&Entity<PdfView>> {
        self.pdf_views.get(tab_id).filter(|view| {
            self.all_tabs().any(|tab| tab.id == tab_id) && view.read(cx).content_id == content_id
        })
    }

    pub(super) fn prune_pdf_views(&mut self) {
        if self.pdf_views.is_empty() {
            return;
        }
        let live = self
            .all_tabs()
            .map(|tab| tab.id.clone())
            .collect::<HashSet<_>>();
        self.pdf_views.retain(|id, _| live.contains(id));
        self.pdf_search_counts.retain(|id, _| live.contains(id));
    }

    pub(super) fn scroll_pdf(
        &mut self,
        amount: f32,
        edge: Option<bool>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.active_tab_is_pdf()
            || self.focused_sidebar_path.is_some()
            || self.focused_workspace_root.is_some()
        {
            return false;
        }
        if let Some(view) = self.active_pdf_view().cloned() {
            view.update(cx, |view, cx| {
                view.restore_anchor = None;
                view.pending_position = None;
                view.scroll = match edge {
                    Some(false) => 0.0,
                    Some(true) => (view.offsets.last().copied().unwrap_or(0.0)
                        - f32::from(view.viewport.size.height))
                    .max(0.0),
                    None => (view.scroll + amount * f32::from(view.viewport.size.height).max(40.0))
                        .max(0.0),
                };
                cx.notify();
            });
        }
        true
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::AppContext;
    use hayro::kurbo::Rect;

    struct Workspace(Entity<EditorView>, gpui::Subscription);

    impl Render for Workspace {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let _ = &self.1;
            let (content, menu) = self.0.update(cx, |editor, cx| {
                editor.last_window_width = f32::from(window.viewport_size().width);
                editor.last_window_height = f32::from(window.viewport_size().height);
                (
                    editor.render_workspace_content_column(window, cx),
                    editor.render_context_menu(cx),
                )
            });
            div()
                .key_context("MarkdownEditor")
                .track_focus(&self.0.read(cx).focus_handle)
                .on_action(
                    cx.listener(|this, action: &crate::actions::Up, window, cx| {
                        this.0
                            .update(cx, |editor, cx| editor.on_up(action, window, cx));
                    }),
                )
                .on_action(
                    cx.listener(|this, action: &crate::actions::Down, window, cx| {
                        this.0
                            .update(cx, |editor, cx| editor.on_down(action, window, cx));
                    }),
                )
                .size_full()
                .relative()
                .flex()
                .child(content)
                .child(menu)
        }
    }

    fn pdf_editor(reader: &Entity<PdfView>, cx: &mut gpui::TestAppContext) -> Entity<EditorView> {
        cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.embedded_editor = false;
                editor.styles.ui.spec.animations_enabled = false;
                editor.active_file_kind_cache = Some(super::super::FileKind::Pdf);
                editor.tabs[0].file_kind = Some(super::super::FileKind::Pdf);
                editor
                    .pdf_views
                    .insert(editor.active_tab_id.clone().unwrap(), reader.clone());
                reader.update(cx, |reader, _| reader.focus = editor.focus_handle.clone());
                editor
            })
        })
    }

    fn reader(cx: &mut gpui::TestAppContext) -> Entity<PdfView> {
        cx.update(|cx| {
            let editor = cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false));
            let focus = editor.read(cx).focus_handle.clone();
            let theme = editor.read(cx).theme.clone();
            cx.new(|cx| {
                let mut reader = PdfView::new_with_source(
                    PdfSource::Local(PathBuf::from("pdf-test-missing.pdf")),
                    focus,
                    &theme,
                    UiLanguage::English,
                    PdfViewState::default(),
                    cx,
                );
                reader.cancelled.store(true, Ordering::Relaxed);
                reader.task = None;
                reader.restore_anchor = None;
                reader.info = Some(Arc::new(pdf::DocumentInfo {
                    pages: vec![
                        pdf::PageInfo {
                            width: 200.0,
                            height: 100.0,
                            links: Vec::new()
                        };
                        2
                    ],
                    headings: Vec::new(),
                    outline_error: None,
                }));
                reader.text = Some(Arc::new(vec![
                    pdf::PageText {
                        text: "a中b".into(),
                        clusters: vec![
                            pdf::TextCluster {
                                range: 0..1,
                                bounds: Rect::new(0.0, 0.0, 5.0, 10.0),
                            },
                            pdf::TextCluster {
                                range: 1..4,
                                bounds: Rect::new(5.0, 0.0, 15.0, 10.0),
                            },
                            pdf::TextCluster {
                                range: 4..5,
                                bounds: Rect::new(15.0, 0.0, 20.0, 10.0),
                            },
                        ],
                        unmapped: 0,
                        images: Vec::new(),
                    },
                    pdf::PageText {
                        text: "next".into(),
                        ..Default::default()
                    },
                ]));
                reader.viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(552.0), px(300.0)));
                reader.offsets = vec![0.0, 216.0, 432.0];
                reader
            })
        })
    }

    #[gpui::test]
    fn pdf_thumbnails_use_page_proportions_and_center_page_numbers(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        reader.update(cx, |view, _| {
            let info = Arc::make_mut(view.info.as_mut().unwrap());
            info.pages[1].width = 100.0;
            info.pages[1].height = 200.0;
            view.layout_thumbnails();
            assert_eq!(view.thumbnail_offsets, vec![0.0, 86.0, 232.0]);
            for (index, page) in view.info.as_ref().unwrap().pages.iter().enumerate() {
                let (width, height, _) = thumbnail_size(page, 0);
                let rect = Bounds::new(
                    point(
                        px((128.0 - width) * 0.5),
                        px(view.thumbnail_offsets[index] + 8.0),
                    ),
                    size(px(width), px(height)),
                );
                for text_width in [px(7.0), px(21.0)] {
                    let origin = thumbnail_label_origin(rect, text_width);
                    assert_eq!(origin.x + text_width * 0.5, px(64.0));
                    assert_eq!(origin.y, rect.bottom() + px(3.0));
                    assert_eq!(
                        px(view.thumbnail_offsets[index + 1] + 8.0) - (origin.y + px(15.0)),
                        px(16.0)
                    );
                }
            }
        });
        reader.update(cx, |view, cx| {
            view.rotate_left(cx);
            view.layout_thumbnails();
            assert_eq!(view.thumbnail_offsets, vec![0.0, 146.0, 232.0]);
        });
    }

    #[gpui::test]
    fn pdf_thumbnail_focus_switches_pages_while_body_focus_scrolls(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([
                gpui::KeyBinding::new("up", crate::actions::Up, Some("MarkdownEditor")),
                gpui::KeyBinding::new("down", crate::actions::Down, Some("MarkdownEditor")),
            ])
        });
        let reader = reader(cx);
        reader.update(cx, |view, _| {
            let info = Arc::make_mut(view.info.as_mut().unwrap());
            info.pages.resize(40, info.pages[0].clone());
            Arc::make_mut(view.text.as_mut().unwrap()).resize(40, pdf::PageText::default());
        });
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let thumbnails = cx.debug_bounds("pdf-thumbnails").unwrap();
        cx.simulate_click(
            thumbnails.origin + point(px(64.0), px(110.0)),
            gpui::Modifiers::default(),
        );
        cx.update(|window, cx| {
            let view = reader.read(cx);
            assert!(view.thumbnail_focus.is_focused(window));
            assert_eq!(view.page, 1);
        });
        cx.simulate_keystrokes("up up");
        cx.update(|_, cx| assert_eq!(reader.read(cx).page, 0));
        cx.simulate_keystrokes("down down down down down down down down down down down down");
        cx.update(|window, cx| {
            let view = reader.read(cx);
            assert_eq!(view.page, 12);
            assert!(view.thumbnail_focus.is_focused(window));
            assert!(view.thumb_scroll > 0.0);
            assert!(view.thumbnail_offsets[12] >= view.thumb_scroll);
            assert!(
                view.thumbnail_offsets[13]
                    <= view.thumb_scroll + f32::from(view.viewport.size.height)
            );
        });
        let body = cx.update(|_, cx| {
            let view = reader.read(cx);
            view.viewport.origin + point(px(240.0), px(50.0))
        });
        cx.simulate_click(body, gpui::Modifiers::default());
        let before = cx.update(|window, cx| {
            let view = reader.read(cx);
            assert!(view.focus.is_focused(window));
            (view.scroll, view.page)
        });
        cx.simulate_keystrokes("down");
        cx.update(|_, cx| {
            let view = reader.read(cx);
            assert!(
                (view.scroll - before.0 - f32::from(view.viewport.size.height) * 0.08).abs() < 0.1
            );
            assert_eq!(view.page, before.1);
        });
        cx.simulate_keystrokes("up");
        cx.update(|_, cx| assert!((reader.read(cx).scroll - before.0).abs() < 0.1));
        reader.update(cx, |view, cx| {
            view.page = 39;
            view.step_thumbnail(true, cx);
            assert_eq!(view.page, 39);
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.simulate_click(
            thumbnails.origin + point(px(64.0), px(5.0)),
            gpui::Modifiers::default(),
        );
        cx.update(|window, cx| {
            let view = reader.read(cx);
            assert!(view.thumbnail_focus.is_focused(window));
            let expected = view
                .thumbnail_at(thumbnails.origin + point(px(64.0), px(5.0)))
                .unwrap();
            assert_eq!(view.page, expected);
        });
    }

    #[gpui::test]
    fn pdf_toolbar_inputs_fit_rotation_and_alignment(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "enter",
                crate::text_input::TextInputSubmit,
                Some("TextInput"),
            )])
        });
        let reader = reader(cx);
        reader.update(cx, |reader, _| {
            for page in &mut Arc::make_mut(reader.info.as_mut().unwrap()).pages {
                page.height = 400.0;
            }
        });
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let controls = cx.debug_bounds("pdf-toolbar-controls").unwrap();
        cx.update(|_, cx| {
            let view = reader.read(cx);
            assert!(
                (f32::from(controls.center().x - (view.viewport.center().x + px(64.0)))).abs()
                    < 1.0
            );
        });
        for (id, value, page) in [
            ("pdf-page-input", "2", true),
            ("pdf-zoom-input", "175%", false),
        ] {
            let bounds = cx.debug_bounds(id).unwrap();
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            cx.update(|window, cx| {
                let view = reader.read(cx);
                let input = (if page {
                    &view.page_input
                } else {
                    &view.zoom_input
                })
                .clone()
                .unwrap();
                assert!(input.read(cx).focus_handle.is_focused(window));
                input.update(cx, |input, cx| input.select_all(cx));
            });
            cx.simulate_input(value);
            cx.simulate_keystrokes("enter");
            cx.update(|window, cx| {
                let view = reader.read(cx);
                assert!(view.toolbar_error.is_none(), "{:?}", view.toolbar_error);
                assert!(view.focus.is_focused(window));
                if page {
                    assert_eq!(view.page, 1);
                } else {
                    assert!((view.zoom - 1.75).abs() < 0.001);
                }
            });
        }
        reader.update(cx, |view, cx| {
            assert!(!view.commit_toolbar_input(true, "0", cx));
            assert!(!view.commit_toolbar_input(true, "3", cx));
            assert!(!view.commit_toolbar_input(false, "NaN", cx));
            assert!(!view.commit_toolbar_input(false, "801%", cx));
            assert_eq!(view.page, 1);
            assert_eq!(view.zoom, 1.75);
            view.toolbar_error = None;
            view.page = 0;
            view.center_page(cx);
        });
        let fit = cx.debug_bounds("pdf-fit-page").unwrap().center();
        cx.simulate_click(fit, gpui::Modifiers::default());
        cx.update(|_, cx| {
            let view = reader.read(cx);
            let rect = view.layouts.iter().find(|(page, _)| *page == 0).unwrap().1;
            assert!(view.fit_page);
            assert!(rect.size.height <= view.viewport.size.height - px(15.9));
            assert!(rect.size.width <= view.viewport.size.width - px(151.9));
        });
        let rotate = cx.debug_bounds("pdf-rotate-left").unwrap().center();
        for turns in [1, 2, 3, 0] {
            cx.simulate_click(rotate, gpui::Modifiers::default());
            cx.update(|_, cx| {
                let view = reader.read(cx);
                assert_eq!(view.rotation, turns);
                let rect = view.layouts.iter().find(|(page, _)| *page == 0).unwrap().1;
                let info = &view.info.as_ref().unwrap().pages[0];
                let transform = pdf::view_transform(info.width, info.height, turns);
                let target = transform * PdfPoint::new(10.0, 5.0);
                let scale = f32::from(rect.size.width) / page_size(info, turns).0;
                assert_eq!(
                    view.hit(
                        rect.origin
                            + point(px(target.x as f32 * scale), px(target.y as f32 * scale))
                    ),
                    Some((0, 1))
                );
            });
        }
        cx.simulate_click(fit, gpui::Modifiers::default());
        cx.update(|_, cx| {
            let view = reader.read(cx);
            assert!(!view.fit_page);
            assert_eq!(view.zoom, 1.0);
        });
    }

    #[gpui::test]
    fn pdf_page_jump_keeps_the_last_page_selected_at_the_scroll_limit(
        cx: &mut gpui::TestAppContext,
    ) {
        let reader = reader(cx);
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        reader.update(cx, |view, cx| {
            assert!(view.commit_toolbar_input(true, "2", cx))
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.update(|_, cx| {
            let view = reader.read(cx);
            assert_eq!(view.page, 1);
            assert!(
                view.layouts
                    .iter()
                    .any(|(page, rect)| *page == 1 && rect.bottom() <= view.viewport.bottom())
            );
        });
    }

    #[gpui::test]
    fn pdf_zoom_keeps_cached_pixels_until_new_tiles_arrive(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        let document = cx.update(|cx| reader.read(cx).document_id);
        let image = Arc::new(RenderImage::new([image::Frame::new(
            image::RgbaImage::from_pixel(200, 100, image::Rgba([20, 80, 160, 255])),
        )]));
        cache_image(
            document,
            (0, 100, 0, 0, 0),
            image.clone(),
            200 * 100 * 4,
            None,
        );
        for zoom in [0.5, 1.2, 2.0, 8.0] {
            let rect = Bounds::new(
                point(px(140.0), px(10.0)),
                size(px(200.0 * zoom), px(100.0 * zoom)),
            );
            assert!(cached(document, (0, (zoom * 100.0) as u32, 0, 0, 0)).is_none());
            let fallback = fallback_tiles(document, 0, 0, (200.0, 100.0), rect);
            assert_eq!(fallback.len(), 1);
            assert_eq!(fallback[0].0, rect);
            assert!(Arc::ptr_eq(fallback[0].1.as_ref().unwrap(), &image));
        }
        assert!(fallback_tiles(document, 0, 1, (100.0, 200.0), Bounds::default()).is_empty());
        retire_document(document);
    }

    #[gpui::test]
    fn pdf_selection_copies_reverse_utf8_and_cross_page_ranges(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        reader.update(cx, |reader, cx| {
            reader.layouts = vec![(
                0,
                Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(100.0))),
            )];
            assert_eq!(reader.hit(point(px(5.0), px(5.0))), None);
            reader.selection = Some(((0, 4), (0, 1)));
            assert_eq!(reader.selected_text().as_deref(), Some("中"));
            reader.selection = Some(((1, 2), (0, 1)));
            assert_eq!(reader.selected_text().as_deref(), Some("中b\nne"));
            reader.select_all(cx);
            assert_eq!(reader.selected_text().as_deref(), Some("a中b\nnext"));
        });
    }

    #[gpui::test]
    fn pdf_navigation_preserves_pending_anchor_before_layout(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        reader.update(cx, |reader, cx| {
            reader.navigate(
                &pdf::LinkTarget::Page {
                    page: 1,
                    point: Some(PdfPoint::new(0.0, 25.0)),
                    zoom: None,
                },
                cx,
            );
            assert_eq!(reader.state().page, 1);
            assert_eq!(reader.state().fraction, 0.25);
            reader.zoom(2.0, cx);
            assert_eq!(reader.state().page, 1);
            assert_eq!(reader.state().zoom, 2.0);
            let before = reader.state();
            reader.navigate(
                &pdf::LinkTarget::Page {
                    page: 99,
                    point: None,
                    zoom: None,
                },
                cx,
            );
            assert_eq!(reader.state(), before);
        });
    }

    #[gpui::test]
    fn pdf_clearing_query_cancels_pending_search_and_highlights(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        reader.update(cx, |reader, cx| {
            reader.search("中".into(), false, false, false, cx);
            let token = reader.search_cancelled.clone();
            reader.search(String::new(), false, false, false, cx);
            assert!(token.load(Ordering::Relaxed));
            assert!(!reader.searching);
            assert!(reader.search_matches.is_empty());
        });
        cx.run_until_parked();
        reader.read_with(cx, |reader, _| assert!(reader.search_matches.is_empty()));
    }

    #[gpui::test]
    fn pdf_pages_center_at_fit_and_reduced_zoom_and_remain_pannable(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        let cx = cx.add_empty_window();
        for zoom in [1.0, 0.5, 2.0] {
            cx.update(|_, cx| reader.update(cx, |reader, cx| reader.zoom(zoom, cx)));
            cx.draw(
                point(px(30.0), px(20.0)),
                size(px(600.0), px(400.0)),
                |_, _| reader.clone(),
            );
            cx.update(|_, cx| {
                let reader = reader.read(cx);
                let page = reader.layouts[0].1;
                if zoom <= 1.0 {
                    let left = page.left() - reader.viewport.left() - px(128.0);
                    let right = reader.viewport.right() - page.right();
                    assert!((f32::from(left - right)).abs() < 0.01);
                    let position = page.origin + point(px(10.0), px(5.0));
                    assert!(reader.hit(position).is_some());
                } else {
                    assert_eq!(page.left(), reader.viewport.left() + px(140.0));
                    assert!(page.right() > reader.viewport.right());
                }
            });
        }
        cx.update(|_, cx| reader.update(cx, |reader, _| reader.horizontal_scroll = 10_000.0));
        cx.draw(
            point(px(30.0), px(20.0)),
            size(px(600.0), px(400.0)),
            |_, _| reader.clone(),
        );
        cx.update(|_, cx| {
            let reader = reader.read(cx);
            assert_eq!(
                reader.layouts[0].1.right(),
                reader.viewport.right() - px(12.0)
            );
        });
    }

    #[gpui::test]
    fn pdf_find_bar_is_visible_and_searches_with_light_page_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        let reader = reader(cx);
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.embedded_editor = false;
                editor.active_file_kind_cache = Some(super::super::FileKind::Pdf);
                editor.tabs[0].file_kind = Some(super::super::FileKind::Pdf);
                editor
                    .pdf_views
                    .insert(editor.active_tab_id.clone().unwrap(), reader.clone());
                editor.theme_kind = crate::theme::ThemeKind::OneDark;
                editor.theme = editor.theme_kind.build();
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.open_search(true, window, cx));
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let bar = cx
            .debug_bounds("search-bar")
            .expect("PDF find bar rendered");
        let content = cx.debug_bounds("editor-content-0").unwrap();
        assert!(bar.size.width > px(0.0) && bar.size.height > px(0.0));
        assert!(content.contains(&bar.center()));
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert!(!editor.search_replace_visible);
            assert!(
                editor
                    .search_query_editor
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .focus_handle
                    .is_focused(window)
            );
            let light = crate::theme::apply_editor_theme_mode(
                &editor.app_settings.light_theme_kind().build(),
                editor.app_settings.editor_theme_mode_for_appearance(false),
                false,
            );
            assert_eq!(reader.read(cx).selection_color, light.selection);
            assert_eq!(reader.read(cx).search_color, light.highlight_bg);
            assert_ne!(reader.read(cx).selection_color, editor.theme.selection);
        });
        cx.simulate_input("中");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(reader.read(cx).search_matches, vec![(0, 1..4)]);
            assert!(!reader.read(cx).searching);
        });
    }

    #[gpui::test]
    fn pdf_canvas_virtualizes_long_documents_and_dispatches_zoom(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        reader.update(cx, |reader, _| {
            let mut info = (**reader.info.as_ref().unwrap()).clone();
            info.pages.resize(10_000, info.pages[0].clone());
            reader.info = Some(Arc::new(info));
            reader.text = None;
            reader.restore_anchor = Some(PdfViewState {
                page: 2345,
                fraction: 0.4,
                ..Default::default()
            });
        });
        let cx = cx.add_empty_window();
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(600.0), px(400.0)),
            |_, _| reader.clone(),
        );
        cx.update(|_, cx| {
            let reader = reader.read(cx);
            assert!(!reader.layouts.is_empty());
            assert!(reader.layouts.len() < 5, "{}", reader.layouts.len());
            assert!(reader.page > 1000, "{}", reader.page);
        });
        cx.simulate_mouse_move(
            point(px(300.0), px(150.0)),
            None,
            gpui::Modifiers::default(),
        );
        cx.simulate_event(gpui::MagnifyEvent {
            position: point(px(300.0), px(150.0)),
            delta: 0.2,
            modifiers: gpui::Modifiers::default(),
        });
        cx.update(|_, cx| {
            let reader = reader.read(cx);
            assert!((reader.zoom - 1.2).abs() < 0.001);
            assert!(reader.state().page > 1000);
        });
    }

    #[gpui::test]
    fn pdf_drag_from_page_margin_selects_nearest_text(cx: &mut gpui::TestAppContext) {
        let reader = reader(cx);
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let (start, end) = cx.update(|_, cx| {
            let reader = reader.read(cx);
            let page = reader.layouts[0].1;
            (
                point(reader.viewport.left() + px(130.0), page.top() + px(40.0)),
                point(page.right() - px(10.0), page.top() + px(40.0)),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), gpui::Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, gpui::Modifiers::default());
        cx.update(|_, cx| {
            assert_eq!(reader.read(cx).selected_text().as_deref(), Some("a中b"));
        });
    }

    #[gpui::test]
    fn pdf_context_menu_preserves_selection_and_dispatches_copy_and_select_all(
        cx: &mut gpui::TestAppContext,
    ) {
        let reader = reader(cx);
        reader.update(cx, |reader, _| reader.selection = Some(((0, 1), (0, 4))));
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let position =
            cx.update(|_, cx| reader.read(cx).viewport.origin + point(px(200.0), px(60.0)));
        cx.simulate_mouse_down(position, MouseButton::Right, gpui::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        let copy = cx
            .debug_bounds("pdf-menu-copy")
            .expect("Copy menu item")
            .center();
        cx.simulate_click(copy, gpui::Modifiers::default());
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("中")
        );
        cx.simulate_mouse_down(position, MouseButton::Right, gpui::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        let all = cx
            .debug_bounds("pdf-menu-select-all")
            .expect("Select All menu item")
            .center();
        cx.simulate_click(all, gpui::Modifiers::default());
        cx.update(|_, cx| {
            assert_eq!(
                reader.read(cx).selected_text().as_deref(),
                Some("a中b\nnext")
            )
        });
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            let id = editor.active_tab_id.as_deref().unwrap();
            assert!(editor.pdf_menu_view(id, 0, cx).is_some());
            assert!(editor.pdf_menu_view(id, 1, cx).is_none());
            assert!(editor.pdf_menu_view("closed-pdf", 0, cx).is_none());
            assert!(!editor.tabs[0].is_dirty);
        });
    }

    #[gpui::test]
    async fn pdf_image_context_menu_copies_embedded_png(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        let reader = reader(cx);
        let bytes: Arc<[u8]> = pdf::tests::image_document("").into();
        let parsed = pdf::parse(bytes.clone()).unwrap();
        reader.update(cx, |reader, _| {
            reader.info = Some(Arc::new(pdf::inspect(&parsed).unwrap()));
            reader.text = Some(Arc::new(
                pdf::extract(&parsed, &AtomicBool::new(false)).unwrap(),
            ));
            reader.bytes = Some(bytes);
            reader.cancelled = Arc::new(AtomicBool::new(false));
        });
        let editor = pdf_editor(&reader, cx);
        let (_, cx) = cx.add_window_view(|_, cx| {
            Workspace(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let position = cx.update(|_, cx| {
            let reader = reader.read(cx);
            let rect = reader.layouts[0].1;
            let center = reader.text.as_ref().unwrap()[0].images[0].bounds.center();
            let scale = f32::from(rect.size.width) / 200.0;
            rect.origin + point(px(center.x as f32 * scale), px(center.y as f32 * scale))
        });
        cx.simulate_mouse_down(position, MouseButton::Right, gpui::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("pdf-menu-save-image").is_some());
        let copy = cx
            .debug_bounds("pdf-menu-copy-image")
            .expect("Copy Image menu item")
            .center();
        cx.simulate_click(copy, gpui::Modifiers::default());
        cx.condition(&reader, |reader, _| !reader.image_exporting())
            .await;
        let clipboard = cx.read_from_clipboard().expect("image clipboard");
        assert!(matches!(
            clipboard.entries(),
            [gpui::ClipboardEntry::Image(_)]
        ));
        cx.update(|_, cx| {
            assert!(
                reader.read(cx).error.is_none(),
                "{:?}",
                reader.read(cx).error
            );
            assert!(!reader.read(cx).image_exporting());
        });
        #[cfg(target_os = "macos")]
        {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("extracted.png");
            cx.simulate_mouse_down(position, MouseButton::Right, gpui::Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear());
            let save = cx.debug_bounds("pdf-menu-save-image").unwrap().center();
            cx.simulate_click(save, gpui::Modifiers::default());
            cx.simulate_new_path_selection(|_| Some(path.clone()));
            cx.condition(&reader, |reader, _| !reader.image_exporting())
                .await;
            let saved = image::load_from_memory(&moeka_fs::local::native::read(path).unwrap())
                .unwrap()
                .into_rgba8();
            assert_eq!(saved.dimensions(), (2, 1));
            assert_eq!(saved.get_pixel(1, 0).0, [0, 0, 255, 128]);
        }
    }

    #[gpui::test]
    fn pdf_search_centers_matches_and_gestures_preserve_the_pointer_anchor(
        cx: &mut gpui::TestAppContext,
    ) {
        let reader = reader(cx);
        reader.update(cx, |reader, _| {
            let info = Arc::make_mut(reader.info.as_mut().unwrap());
            for page in &mut info.pages {
                page.height = 400.0;
            }
            let text = Arc::make_mut(reader.text.as_mut().unwrap());
            text[1].clusters.push(pdf::TextCluster {
                range: 0..4,
                bounds: Rect::new(60.0, 180.0, 100.0, 200.0),
            });
        });
        let cx = cx.add_empty_window();
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(600.0), px(400.0)),
            |_, _| reader.clone(),
        );
        cx.update(|_, cx| {
            reader.update(cx, |reader, cx| {
                reader.search_matches = vec![(1, 0..4)];
                reader.search_navigate(true, cx);
            });
        });
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(600.0), px(400.0)),
            |_, _| reader.clone(),
        );
        let position = cx.update(|_, cx| {
            let reader = reader.read(cx);
            let rect = reader
                .layouts
                .iter()
                .find(|(page, _)| *page == 1)
                .unwrap()
                .1;
            let y = rect.top() + px(190.0 * f32::from(rect.size.width) / 200.0);
            assert!(f32::from(y - reader.viewport.center().y).abs() < 0.01);
            point(px(320.0), y)
        });
        for modifiers in [
            gpui::Modifiers {
                platform: true,
                ..Default::default()
            },
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
        ] {
            let (page, source, old_zoom) = cx.update(|_, cx| {
                let reader = reader.read(cx);
                let &(page, rect) = reader
                    .layouts
                    .iter()
                    .find(|(_, rect)| rect.contains(&position))
                    .unwrap();
                (page, (position - rect.origin) / reader.zoom, reader.zoom)
            });
            cx.simulate_mouse_move(position, None, gpui::Modifiers::default());
            cx.simulate_event(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(30.0))),
                modifiers,
                ..Default::default()
            });
            cx.draw(
                point(px(0.0), px(0.0)),
                size(px(600.0), px(400.0)),
                |_, _| reader.clone(),
            );
            cx.update(|_, cx| {
                let reader = reader.read(cx);
                let rect = reader
                    .layouts
                    .iter()
                    .find(|(index, _)| *index == page)
                    .unwrap()
                    .1;
                assert!((reader.zoom - old_zoom * 1.1).abs() < 0.001);
                assert!((rect.origin + source * reader.zoom - position).magnitude() < 0.01);
            });
        }
        let before = cx.update(|_, cx| {
            let reader = reader.read(cx);
            (reader.zoom, reader.scroll, reader.horizontal_scroll)
        });
        cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(-5.0), px(-20.0))),
            ..Default::default()
        });
        cx.update(|_, cx| {
            let reader = reader.read(cx);
            assert_eq!(reader.zoom, before.0);
            assert_eq!(reader.scroll, before.1 + 20.0);
            assert_eq!(reader.horizontal_scroll, before.2 + 5.0);
        });
    }
}
impl EditorView {
    pub(super) fn active_pdf_view(&self) -> Option<&Entity<PdfView>> {
        self.pdf_views.get(self.active_tab_id.as_ref()?)
    }

    pub(super) fn render_pdf_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let id = self.active_tab_id.clone().expect("PDF tab active");
        if !self.pdf_views.contains_key(&id) {
            let tab = self
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .expect("PDF tab exists");
            let source = if tab.is_remote {
                let Some(workspace) = self.remote_workspace.as_ref() else {
                    return div()
                        .child(crate::i18n::remote_text(self.ui_language, "not_connected"))
                        .into_any_element();
                };
                let Some(resource) = tab
                    .remote
                    .as_ref()
                    .and_then(|state| state.resource.as_ref())
                    .filter(|resource| {
                        !tab.remote_binding_lost
                            && resource.filesystem == *workspace.filesystem.identity()
                    })
                else {
                    return div()
                        .child(crate::i18n::remote_text(
                            self.ui_language,
                            "remote_binding_lost",
                        ))
                        .into_any_element();
                };
                PdfSource::Remote(workspace.filesystem.clone(), resource.path.clone())
            } else {
                PdfSource::Local(self.current_file_path.clone().expect("PDF tab has path"))
            };
            let focus = self.focus_handle.clone();
            let theme = self.theme.clone();
            let language = self.ui_language;
            let state = self
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.pdf_view)
                .unwrap_or_default();
            let view = cx.new(|cx| {
                cx.on_release(|view: &mut PdfView, cx| {
                    retire_document(view.document_id);
                    cx.defer(release_images);
                })
                .detach();
                PdfView::new_with_source(source, focus, &theme, language, state, cx)
            });
            let tab_id = id.clone();
            cx.observe(&view, move |editor, view, cx| {
                let state = view.read(cx).state();
                editor
                    .pdf_search_counts
                    .insert(tab_id.clone(), view.read(cx).search_matches.len());
                if let Some(tab) = editor.all_tabs_mut().find(|tab| tab.id == tab_id) {
                    tab.pdf_view = state;
                }
                cx.notify();
            })
            .detach();
            if self.search_visible {
                let query = self.search_query.clone();
                let case = self.search_case_sensitive;
                let word = self.search_whole_word;
                let regex = self.search_use_regex;
                view.update(cx, |view, cx| view.search(query, case, word, regex, cx));
            }
            self.pdf_views.insert(id.clone(), view);
        }
        let page_theme = crate::theme::apply_editor_theme_mode(
            &self.app_settings.light_theme_kind().build(),
            self.app_settings.editor_theme_mode_for_appearance(false),
            false,
        );
        let toolbar_editor = cx.entity().downgrade();
        self.pdf_views[&id].update(cx, |view, _| {
            view.background = self.theme.bg_primary;
            view.foreground = self.theme.text_primary;
            view.accent = self.theme.text_accent;
            view.selection_color = page_theme.selection;
            view.search_color = page_theme.highlight_bg;
            view.language = self.ui_language;
            view.toolbar_editor = Some(toolbar_editor);
            view.toolbar_button = self.styles.toolbar_button(26.0);
            view.toolbar_input = self
                .styles
                .input_styles(crate::style::InputRole::Stepper, self.ui_scale);
            view.ui_scale = self.ui_scale;
        });
        self.pdf_views[&id].clone().into_any_element()
    }
}
