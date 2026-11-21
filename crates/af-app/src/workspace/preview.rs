//! The preview pane: images, text and source (first 1 MiB, monospace),
//! rendered Markdown, folder totals, and a metadata block for every item.
//! Everything is read on a background thread.

use std::path::PathBuf;
use std::sync::Arc;

use af_fs::details::{Details, FolderSummary};
use af_fs::{Entry, FileKind};
use af_ui::components::{IconButtonExt as _, icon_button, section_label};
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Workspace;
use super::list::file_icon;
use crate::format;
use crate::tab::is_folder;

/// How much of a file the preview reads.
const TEXT_CAP: usize = 1024 * 1024;
/// Lines shown from that.
const MAX_LINES: usize = 20_000;
/// Markdown is laid out whole; keep it to a size that stays quick.
const MARKDOWN_CAP: usize = 200 * 1024;
/// Entries a folder summary walks before it stops counting.
const FOLDER_WALK_LIMIT: u64 = 50_000;
const TEXT_LINE_HEIGHT: Pixels = px(18.);

#[derive(Default)]
pub(crate) enum Content {
    #[default]
    Empty,
    Loading,
    Text {
        lines: Arc<Vec<SharedString>>,
        truncated: bool,
    },
    Markdown(SharedString),
    Image(PathBuf),
    Folder(FolderSummary),
    Binary,
    Error(SharedString),
}

#[derive(Default)]
pub(crate) struct Preview {
    /// The item shown (or loading).
    key: Option<PathBuf>,
    details: Option<Details>,
    content: Content,
    text_scroll: UniformListScrollHandle,
    _task: Option<Task<()>>,
}

fn load(entry: &Entry) -> (Option<Details>, Content) {
    let details = af_fs::details::details(&entry.path).ok();
    let content = if is_folder(entry) {
        match af_fs::details::summarize(&entry.path, FOLDER_WALK_LIMIT) {
            Ok(s) => Content::Folder(s),
            Err(e) => Content::Error(e.to_string().into()),
        }
    } else if af_fs::kind::is_decodable_image(&entry.name) {
        Content::Image(entry.path.clone())
    } else {
        let kind = entry.kind();
        let small_unknown = kind == FileKind::Other && entry.size <= TEXT_CAP as u64;
        if kind.is_text() || small_unknown {
            match af_fs::details::read_text(&entry.path, TEXT_CAP) {
                Ok(t) if t.binary => Content::Binary,
                Ok(t) if kind == FileKind::Markdown && t.text.len() <= MARKDOWN_CAP => {
                    Content::Markdown(t.text.into())
                }
                Ok(t) => {
                    let lines: Vec<SharedString> = t
                        .text
                        .lines()
                        .take(MAX_LINES)
                        .map(|l| SharedString::from(l.replace('\t', "    ")))
                        .collect();
                    Content::Text {
                        truncated: t.truncated || lines.len() >= MAX_LINES,
                        lines: Arc::new(lines),
                    }
                }
                Err(e) => Content::Error(e.to_string().into()),
            }
        } else {
            Content::Empty
        }
    };
    (details, content)
}

impl Workspace {
    /// Start loading the preview of the item under the cursor if it changed.
    pub(super) fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let tab = self.tab();
        let entry = if tab.selection.count() <= 1 {
            tab.current().cloned()
        } else {
            None
        };
        let key = entry.as_ref().map(|e| e.path.clone());
        if key == self.preview.key {
            return;
        }
        self.preview.key = key;
        self.preview.details = None;
        self.preview.text_scroll = UniformListScrollHandle::new();
        let Some(entry) = entry else {
            self.preview.content = Content::Empty;
            self.preview._task = None;
            return;
        };
        self.preview.content = Content::Loading;
        let path = entry.path.clone();
        let work = cx.background_executor().spawn(async move { load(&entry) });
        self.preview._task = Some(cx.spawn(async move |this, cx| {
            let (details, content) = work.await;
            let _ = this.update(cx, |ws, cx| {
                if ws.preview.key.as_ref() == Some(&path) {
                    ws.preview.details = details;
                    ws.preview.content = content;
                    cx.notify();
                }
            });
        }));
    }

    /// Forget the loaded preview (the item changed on disk).
    pub(crate) fn invalidate_preview(&mut self) {
        self.preview.key = None;
    }

    pub(super) fn render_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let tab = self.tab();
        let n = tab.selection.count();
        if n > 1 {
            let bytes = tab.selected_bytes();
            let folders = tab.selected().iter().filter(|e| e.is_dir).count();
            let mut lines = vec![format!("{n} items selected")];
            if bytes > 0 {
                lines.push(format!("{} in files", af_fs::format_size(bytes)));
            }
            if folders > 0 {
                lines.push(format!(
                    "{folders} folder{}",
                    if folders == 1 { "" } else { "s" }
                ));
            }
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(SPACE_3)
                .child(
                    Icon::new(IconName::Files)
                        .size(px(48.))
                        .text_color(t.fg_dim),
                )
                .children(lines.into_iter().enumerate().map(|(i, l)| {
                    div()
                        .text_size(if i == 0 { FONT_EMPHASIS } else { FONT_SMALL })
                        .text_color(if i == 0 { t.fg } else { t.fg_muted })
                        .child(l)
                }))
                .into_any_element();
        }
        let current = tab.current().cloned();
        match current {
            Some(entry) => self.render_preview_for(&entry, false, cx),
            None => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(SPACE_3)
                .text_color(t.fg_dim)
                .child(Icon::new(IconName::Eye).size(px(32.)).text_color(t.fg_dim))
                .child(
                    div()
                        .text_size(FONT_SMALL)
                        .child("Select an item to preview it"),
                )
                .into_any_element(),
        }
    }

    /// The preview of `entry`. `compact` is the Miller columns' last column.
    pub(super) fn render_preview_for(
        &mut self,
        entry: &Entry,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens(cx);
        let (icon, color) = file_icon(entry, &t);
        let mono = cx.theme().mono_font_family.clone();
        let showing = self.preview.key.as_ref() == Some(&entry.path);
        let content: AnyElement = match (&self.preview.content, showing) {
            (Content::Image(path), true) => div()
                .w_full()
                .h(if compact { px(200.) } else { px(260.) })
                .p(SPACE_6)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .rounded(RADIUS_MD),
                )
                .into_any_element(),
            (Content::Text { lines, truncated }, true) => {
                let lines = lines.clone();
                let count = lines.len();
                let truncated = *truncated;
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .m(SPACE_5)
                    .rounded(RADIUS_MD)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.bg)
                    .overflow_hidden()
                    .child(
                        uniform_list("preview-lines", count, move |range, _, _| {
                            range
                                .map(|i| {
                                    div()
                                        .h(TEXT_LINE_HEIGHT)
                                        .px(SPACE_4)
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .child(lines[i].clone())
                                })
                                .collect::<Vec<_>>()
                        })
                        .track_scroll(&self.preview.text_scroll)
                        .flex_1()
                        .py(SPACE_3)
                        .font_family(mono)
                        .text_size(FONT_SMALL)
                        .text_color(t.fg),
                    )
                    .when(truncated, |d| {
                        d.child(
                            div()
                                .px(SPACE_4)
                                .py(SPACE_2)
                                .border_t_1()
                                .border_color(t.border)
                                .text_size(FONT_CAPTION)
                                .text_color(t.fg_dim)
                                .child("Showing the first 1 MB"),
                        )
                    })
                    .into_any_element()
            }
            (Content::Markdown(text), true) => div()
                .id("markdown")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(SPACE_7)
                .py(SPACE_5)
                .child(TextView::markdown("markdown-preview", text.clone()).selectable(true))
                .into_any_element(),
            (Content::Folder(sum), true) => {
                let mut lines = vec![format::items(sum.items as usize)];
                let at_least = if sum.truncated { "at least " } else { "" };
                lines.push(format!(
                    "{at_least}{} in {} files",
                    af_fs::format_size(sum.bytes),
                    sum.files
                ));
                big_icon(icon, color)
                    .children(
                        lines
                            .into_iter()
                            .map(|l| div().text_size(FONT_SMALL).text_color(t.fg_muted).child(l)),
                    )
                    .into_any_element()
            }
            (Content::Error(e), true) => big_icon(icon, color)
                .child(
                    div()
                        .text_size(FONT_SMALL)
                        .text_color(t.danger)
                        .child(e.clone()),
                )
                .into_any_element(),
            (Content::Binary, true) => big_icon(icon, color)
                .child(
                    div()
                        .text_size(FONT_SMALL)
                        .text_color(t.fg_dim)
                        .child("No text preview"),
                )
                .into_any_element(),
            _ => big_icon(icon, color).into_any_element(),
        };
        let grows = matches!(
            (&self.preview.content, showing),
            (Content::Text { .. } | Content::Markdown(_), true)
        );

        let details = if showing {
            self.preview.details.clone()
        } else {
            None
        };
        let kind = af_fs::kind::kind_label(&entry.name, entry.is_dir);
        let mut rows: Vec<(&str, String)> = vec![("Kind", kind)];
        if !entry.is_dir {
            rows.push(("Size", af_fs::format_size(entry.size)));
        }
        if let Some(d) = &details {
            if let Some(c) = d.created {
                rows.push(("Created", format::long_date(c)));
            }
            if let Some(m) = d.modified {
                rows.push(("Modified", format::long_date(m)));
            }
            if let Some(mode) = d.mode {
                rows.push((
                    "Permissions",
                    format!(
                        "{}  {:o}",
                        af_fs::details::permissions_string(mode, d.is_dir),
                        mode & 0o7777
                    ),
                ));
            }
            if let Some(target) = &d.link_target {
                rows.push(("Links to", target.display().to_string()));
            }
        } else if let Some(m) = entry.modified {
            rows.push(("Modified", format::long_date(m)));
        }
        let path = entry.path.clone();
        let copy = icon_button("copy-path", IconName::Copy, false, cx)
            .tip("Copy path", Some("Cmd-Alt-C"))
            .on_click(
                cx.listener(move |ws, _, _, cx| ws.copy_paths(std::slice::from_ref(&path), cx)),
            );
        let meta = v_flex()
            .flex_none()
            .px(SPACE_7)
            .py(SPACE_6)
            .gap(SPACE_3)
            .border_t_1()
            .border_color(t.border)
            .child(
                div()
                    .text_size(FONT_TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.fg)
                    .line_clamp(3)
                    .child(entry.name.clone()),
            )
            .child(div().pt(SPACE_3).child(section_label("Information", cx)))
            .children(rows.into_iter().map(|(label, value)| {
                h_flex()
                    .items_start()
                    .gap(SPACE_4)
                    .text_size(FONT_SMALL)
                    .child(
                        div()
                            .flex_none()
                            .w(px(80.))
                            .text_color(t.fg_dim)
                            .child(label),
                    )
                    .child(div().flex_1().min_w_0().text_color(t.fg).child(value))
            }))
            .child(
                h_flex()
                    .items_start()
                    .gap(SPACE_4)
                    .text_size(FONT_SMALL)
                    .child(
                        div()
                            .flex_none()
                            .w(px(80.))
                            .text_color(t.fg_dim)
                            .child("Where"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(t.fg_muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(FONT_CAPTION)
                            .child(entry.path.display().to_string()),
                    )
                    .child(copy),
            );

        let body = if grows {
            content
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .justify_center()
                .child(content)
                .into_any_element()
        };
        v_flex()
            .id("preview")
            .size_full()
            .child(body)
            .child(meta)
            .into_any_element()
    }
}

fn big_icon(icon: IconName, color: Hsla) -> Div {
    v_flex()
        .w_full()
        .py(SPACE_9)
        .items_center()
        .gap(SPACE_3)
        .child(Icon::new(icon).size(px(64.)).text_color(color))
}
