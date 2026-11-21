//! Clean Up: scan Home, show what's taking space with an honest safety label,
//! stage items into a queue, then move the queue to Trash. Nothing is removed
//! until the user presses the button, and the engine re-checks every path.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use af_clean::cleanup::{CleanupQueue, Disposal, StagedItem};
use af_clean::other::Safety;
use af_clean::{Progress, ScanOptions};
use af_fs::format_size;
use af_ui::components::{IconButtonExt as _, badge, icon_button, meter, section_label};
use af_ui::metrics::*;
use af_ui::{Tokens, tokens};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::workspace::Workspace;

const TOP_FOLDERS: usize = 15;

#[derive(Clone)]
enum Row {
    Header {
        title: SharedString,
        hint: SharedString,
        safety: Safety,
        total: u64,
    },
    Item {
        label: SharedString,
        path: PathBuf,
        category: String,
        safety: Safety,
        size: u64,
    },
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Scanning {
        progress: Arc<Progress>,
        started: Instant,
    },
    Ready {
        scanned: u64,
        files: u64,
        secs: f64,
        errors: u64,
    },
    Trashing,
}

#[derive(Default)]
pub struct Cleanup {
    phase: Phase,
    /// The biggest category, for the size bars.
    max_total: u64,
    rows: Vec<Row>,
    selected: usize,
    queue: CleanupQueue,
    message: Option<SharedString>,
    scroll: UniformListScrollHandle,
    _scan: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

impl Cleanup {
    pub fn started(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    pub fn start_scan(ws: &mut Workspace, cx: &mut Context<Workspace>) {
        let progress = Arc::new(Progress::default());
        let started = Instant::now();
        let shared = progress.clone();
        let work = cx.background_executor().spawn(async move {
            let home = af_clean::paths::home();
            let opts = ScanOptions {
                keep_file_nodes: false,
                ..ScanOptions::default()
            };
            let scan = af_clean::scan(&home, opts, Some(shared))?;
            let now = scan.taken_at;
            let report = af_clean::other::other_report(Some(&scan), now);
            Ok::<_, af_clean::DbError>((scan, report))
        });
        let scan_task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |ws, cx| {
                let c = &mut ws.cleanup;
                match result {
                    Ok((scan, report)) => {
                        c.rows = build_rows(&scan, &report);
                        c.refresh_max();
                        c.phase = Phase::Ready {
                            scanned: scan.node.logical,
                            files: scan.node.files,
                            secs: scan.duration_ms as f64 / 1000.0,
                            errors: scan.errors,
                        };
                        c.selected = c.first_item().unwrap_or(0);
                        c.message = None;
                    }
                    Err(e) => {
                        c.phase = Phase::Idle;
                        c.message = Some(format!("Scan failed: {e}").into());
                    }
                }
                c._poll = None;
                cx.notify();
            });
        });
        // Repaint the counters while the walk runs.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        // Replacing an older scan's tasks cancels it.
        let c = &mut ws.cleanup;
        c.phase = Phase::Scanning { progress, started };
        c.rows.clear();
        c.message = None;
        c._scan = Some(scan_task);
        c._poll = Some(poll);
        cx.notify();
    }

    fn refresh_max(&mut self) {
        self.max_total = self
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { total, .. } => Some(*total),
                Row::Item { .. } => None,
            })
            .max()
            .unwrap_or(0);
    }

    fn first_item(&self) -> Option<usize> {
        self.rows.iter().position(|r| matches!(r, Row::Item { .. }))
    }

    pub fn move_by(&mut self, delta: isize, cx: &mut Context<Workspace>) {
        let mut ix = self.selected as isize;
        loop {
            ix += delta;
            if ix < 0 || ix as usize >= self.rows.len() {
                return;
            }
            if matches!(self.rows[ix as usize], Row::Item { .. }) {
                break;
            }
        }
        self.selected = ix as usize;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    fn is_staged(&self, path: &PathBuf) -> bool {
        self.queue.items.iter().any(|i| &i.path == path)
    }

    pub fn toggle_selected(&mut self, cx: &mut Context<Workspace>) {
        self.toggle(self.selected, cx);
    }

    fn toggle(&mut self, ix: usize, cx: &mut Context<Workspace>) {
        let Some(Row::Item {
            path,
            category,
            safety,
            size,
            label,
        }) = self.rows.get(ix).cloned()
        else {
            return;
        };
        if self.is_staged(&path) {
            self.queue.unstage(&path);
            self.message = None;
        } else if safety == Safety::Info {
            self.message = Some("System-managed: shown for context, not removable here".into());
        } else {
            let item = StagedItem {
                path,
                logical: size,
                category,
                reason: label.to_string(),
            };
            self.message = self.queue.stage(item).err().map(SharedString::from);
        }
        cx.notify();
    }

    fn stage_all_safe(&mut self, cx: &mut Context<Workspace>) {
        let mut refused = 0;
        for row in &self.rows {
            if let Row::Item {
                path,
                category,
                safety: Safety::Safe,
                size,
                label,
            } = row
            {
                if self.queue.items.iter().any(|i| &i.path == path) {
                    continue;
                }
                let item = StagedItem {
                    path: path.clone(),
                    logical: *size,
                    category: category.clone(),
                    reason: label.to_string(),
                };
                if self.queue.stage(item).is_err() {
                    refused += 1;
                }
            }
        }
        self.message = (refused > 0)
            .then(|| format!("{refused} items skipped (protected or overlapping)").into());
        cx.notify();
    }

    pub fn trash_queue(ws: &mut Workspace, cx: &mut Context<Workspace>) {
        if ws.cleanup.queue.items.is_empty() || matches!(ws.cleanup.phase, Phase::Trashing) {
            return;
        }
        if let Err(e) = ws.cleanup.queue.validate() {
            ws.cleanup.message = Some(format!("Queue changed on disk: {e}").into());
            cx.notify();
            return;
        }
        let items = ws.cleanup.queue.items.clone();
        let task = cx
            .background_executor()
            .spawn(async move { af_clean::cleanup::apply(&items, Disposal::Trash, false) });
        let job = cx.spawn(async move |this, cx| {
            let outcomes = task.await;
            let _ = this.update(cx, |ws, cx| {
                let c = &mut ws.cleanup;
                let freed: u64 = outcomes.iter().map(|o| o.freed).sum();
                let failed: Vec<_> = outcomes.iter().filter(|o| !o.ok).collect();
                for o in outcomes.iter().filter(|o| o.ok) {
                    c.queue.unstage(&o.path);
                    c.rows
                        .retain(|r| !matches!(r, Row::Item { path, .. } if path == &o.path));
                }
                c.message = Some(if failed.is_empty() {
                    format!(
                        "Moved to Trash: {}. Empty the Trash to get the space back.",
                        format_size(freed)
                    )
                    .into()
                } else {
                    format!(
                        "Moved {}; {} failed: {}",
                        format_size(freed),
                        failed.len(),
                        failed[0].message
                    )
                    .into()
                });
                c.phase = Phase::Ready {
                    scanned: 0,
                    files: 0,
                    secs: 0.0,
                    errors: 0,
                };
                c.selected = c.selected.min(c.rows.len().saturating_sub(1));
                c.refresh_max();
                cx.notify();
            });
        });
        ws.cleanup.phase = Phase::Trashing;
        ws.cleanup._scan = Some(job);
        cx.notify();
    }
}

fn build_rows(scan: &af_clean::ScanResult, report: &af_clean::other::OtherReport) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut cats: Vec<_> = report
        .categories
        .iter()
        .filter(|c| c.logical > 0 && c.paths.iter().any(|p| p.exists && p.logical > 0))
        .collect();
    // Safe first, then review, then info; biggest first inside each.
    cats.sort_by_key(|c| (safety_rank(c.safety), std::cmp::Reverse(c.logical)));
    for cat in cats {
        rows.push(Row::Header {
            title: cat.title.clone().into(),
            hint: cat.hint.clone().into(),
            safety: cat.safety,
            total: cat.logical,
        });
        let mut paths: Vec<_> = cat
            .paths
            .iter()
            .filter(|p| p.exists && p.logical > 0)
            .collect();
        paths.sort_by_key(|p| std::cmp::Reverse(p.logical));
        for p in paths {
            rows.push(Row::Item {
                label: af_clean::paths::display_path(&p.path, &scan.root).into(),
                path: p.path.clone(),
                category: cat.id.clone(),
                safety: cat.safety,
                size: p.logical,
            });
        }
    }
    let top: Vec<_> = scan
        .node
        .children
        .iter()
        .filter(|n| n.is_dir() && n.logical > 0)
        .take(TOP_FOLDERS)
        .collect();
    if !top.is_empty() {
        rows.push(Row::Header {
            title: "Largest folders in Home".into(),
            hint: "Your own files. Open them in Files to decide what to keep.".into(),
            safety: Safety::Review,
            total: top.iter().map(|n| n.logical).sum(),
        });
        for n in top {
            rows.push(Row::Item {
                label: format!("~/{}", n.name).into(),
                path: n.path.clone(),
                category: "manual".into(),
                safety: Safety::Review,
                size: n.logical,
            });
        }
    }
    rows
}

fn safety_rank(s: Safety) -> u8 {
    match s {
        Safety::Safe => 0,
        Safety::Review => 1,
        Safety::Info => 2,
    }
}

fn safety_color(s: Safety, t: &Tokens) -> Hsla {
    match s {
        Safety::Safe => t.success,
        Safety::Review => t.warning,
        Safety::Info => t.fg_muted,
    }
}

fn safety_badge(s: Safety, t: &Tokens) -> Div {
    let label = match s {
        Safety::Safe => "Safe",
        Safety::Review => "Review",
        Safety::Info => "Info",
    };
    badge(label, safety_color(s, t))
}

/// Every row has one height (`uniform_list`): headers and items alike.
const ROW_HEIGHT: Pixels = px(32.);

impl Workspace {
    pub fn render_cleanup(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let c = &self.cleanup;
        let summary: SharedString = match &c.phase {
            Phase::Idle => "Not scanned yet".into(),
            Phase::Scanning { progress, started } => {
                let (files, bytes, _) = progress.snapshot();
                format!(
                    "Scanning Home\u{2026} {} files, {} ({:.0}s)",
                    files,
                    format_size(bytes),
                    started.elapsed().as_secs_f64()
                )
                .into()
            }
            Phase::Ready {
                scanned,
                files,
                secs,
                errors,
            } if *scanned > 0 => {
                let mut s = format!(
                    "Home: {} in {} files, scanned in {:.1}s",
                    format_size(*scanned),
                    files,
                    secs
                );
                if *errors > 0 {
                    s.push_str(&format!(" ({errors} unreadable)"));
                }
                s.into()
            }
            Phase::Ready { .. } => "Done".into(),
            Phase::Trashing => "Moving to Trash\u{2026}".into(),
        };
        let scanning = matches!(c.phase, Phase::Scanning { .. });
        let safe_total: u64 = c
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Item {
                    safety: Safety::Safe,
                    size,
                    ..
                } => Some(*size),
                _ => None,
            })
            .sum();

        let header = h_flex()
            .flex_none()
            .h(px(64.))
            .px(SPACE_8)
            .gap(SPACE_5)
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .size(px(34.))
                    .flex_none()
                    .rounded(RADIUS_LG)
                    .bg(t.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::BrushCleaning)
                            .size(ICON_LG)
                            .text_color(t.accent),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(SPACE_1)
                    .child(
                        div()
                            .text_size(FONT_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Clean Up"),
                    )
                    .child(
                        div()
                            .text_size(FONT_SMALL)
                            .text_color(t.fg_muted)
                            .truncate()
                            .child(summary),
                    ),
            )
            .when(safe_total > 0, |d| {
                d.child(
                    Button::new("stage-safe")
                        .small()
                        .outline()
                        .label(format!("Stage all safe ({})", format_size(safe_total)))
                        .on_click(cx.listener(|ws, _, _, cx| ws.cleanup.stage_all_safe(cx))),
                )
            })
            .child(
                Button::new("rescan")
                    .small()
                    .outline()
                    .icon(IconName::RefreshCw)
                    .label("Rescan")
                    .disabled(scanning)
                    .on_click(cx.listener(|ws, _, _, cx| Cleanup::start_scan(ws, cx))),
            );

        let list = if c.rows.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(SPACE_4)
                .text_color(t.fg_muted)
                .child(
                    Icon::new(IconName::HardDrive)
                        .size(px(40.))
                        .text_color(t.fg_dim),
                )
                .child(match c.phase {
                    Phase::Scanning { .. } => {
                        "Reading every folder in Home once. Large disks take a minute."
                    }
                    _ => "Nothing to show",
                })
                .into_any_element()
        } else {
            uniform_list(
                "cleanup-rows",
                c.rows.len(),
                cx.processor(|ws, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|ix| ws.cleanup_row(ix, cx).into_any_element())
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&c.scroll)
            .flex_1()
            .py(SPACE_3)
            .into_any_element()
        };

        let queue_total = c.queue.total();
        let mut items = v_flex()
            .id("queue-items")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(SPACE_4)
            .gap(px(1.));
        if c.queue.items.is_empty() {
            items = items.child(
                div()
                    .pt(SPACE_7)
                    .px(SPACE_3)
                    .text_size(FONT_SMALL)
                    .text_color(t.fg_dim)
                    .child("Nothing staged. Double-click a row, or select it and press Return."),
            );
        }
        for (i, item) in c.queue.items.iter().enumerate() {
            let path = item.path.clone();
            items = items.child(
                h_flex()
                    .id(("queued", i))
                    .h(px(28.))
                    .px(SPACE_3)
                    .gap(SPACE_3)
                    .rounded(RADIUS_MD)
                    .text_size(FONT_SMALL)
                    .hover(|s| s.bg(t.hover))
                    .child(div().flex_1().min_w_0().truncate().child(
                        af_clean::paths::display_path(&item.path, &af_clean::paths::home()),
                    ))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.fg_muted)
                            .child(format_size(item.logical)),
                    )
                    .child(
                        icon_button(("unstage", i), IconName::X, false, cx)
                            .tip("Unstage", None)
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                ws.cleanup.queue.unstage(&path);
                                cx.notify();
                            })),
                    ),
            );
        }
        let queue = v_flex()
            .flex_none()
            .w(px(300.))
            .h_full()
            .border_l_1()
            .border_color(t.border)
            .bg(t.chrome)
            .child(
                v_flex()
                    .p(SPACE_7)
                    .gap(SPACE_2)
                    .child(section_label("Queue", cx))
                    .child(
                        div()
                            .text_size(FONT_DISPLAY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format_size(queue_total)),
                    )
                    .child(
                        div()
                            .text_size(FONT_SMALL)
                            .text_color(t.fg_muted)
                            .child(format!("{} items staged", c.queue.items.len())),
                    ),
            )
            .child(items)
            .child(
                div().p(SPACE_6).border_t_1().border_color(t.border).child(
                    Button::new("trash-queue")
                        .primary()
                        .w_full()
                        .icon(IconName::Trash)
                        .label(format!("Move {} to Trash", format_size(queue_total)))
                        .disabled(queue_total == 0)
                        .on_click(cx.listener(|ws, _, _, cx| Cleanup::trash_queue(ws, cx))),
                ),
            );

        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(header)
                    .child(list)
                    .when_some(c.message.clone(), |d, msg| {
                        d.child(
                            div()
                                .flex_none()
                                .px(SPACE_8)
                                .py(SPACE_3)
                                .border_t_1()
                                .border_color(t.border)
                                .bg(t.accent_soft)
                                .text_size(FONT_SMALL)
                                .text_color(t.fg)
                                .child(msg),
                        )
                    }),
            )
            .child(queue)
    }

    fn cleanup_row(&mut self, ix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = tokens(cx);
        let c = &self.cleanup;
        match &c.rows[ix] {
            Row::Header {
                title,
                hint,
                safety,
                total,
            } => {
                let fraction = *total as f32 / c.max_total.max(1) as f32;
                h_flex()
                    .id(("clean", ix))
                    .w_full()
                    .h(ROW_HEIGHT)
                    .px(SPACE_6)
                    .child(
                        h_flex()
                            .size_full()
                            .px(SPACE_4)
                            .gap(SPACE_4)
                            .rounded(RADIUS_MD)
                            .bg(t.surface)
                            .child(safety_badge(*safety, &t))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(FONT_SMALL)
                                    .text_color(t.fg_dim)
                                    .child(hint.clone()),
                            )
                            .child(div().flex_none().w(px(110.)).child(meter(
                                fraction,
                                safety_color(*safety, &t),
                                t.bar_track,
                                px(4.),
                            )))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(64.))
                                    .flex()
                                    .justify_end()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format_size(*total)),
                            ),
                    )
            }
            Row::Item {
                label, size, path, ..
            } => {
                let staged = c.is_staged(path);
                let selected = ix == c.selected;
                h_flex()
                    .id(("clean", ix))
                    .w_full()
                    .h(ROW_HEIGHT)
                    .px(SPACE_6)
                    .child(
                        h_flex()
                            .size_full()
                            .pl(SPACE_6)
                            .pr(SPACE_4)
                            .gap(SPACE_4)
                            .rounded(RADIUS_MD)
                            .when_else(
                                selected,
                                |d| d.bg(t.selected),
                                |d| d.hover(|s| s.bg(t.hover)),
                            )
                            .child(
                                Icon::new(if staged {
                                    IconName::CircleCheck
                                } else {
                                    IconName::Circle
                                })
                                .size(ICON_MD)
                                .text_color(if staged {
                                    t.success
                                } else {
                                    t.fg_dim
                                }),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(64.))
                                    .flex()
                                    .justify_end()
                                    .text_color(t.fg_muted)
                                    .child(format_size(*size)),
                            ),
                    )
                    .on_click(cx.listener(move |ws, e: &ClickEvent, _, cx| {
                        ws.cleanup.selected = ix;
                        if e.click_count() >= 2 || e.modifiers().platform {
                            ws.cleanup.toggle(ix, cx);
                        }
                        cx.notify();
                    }))
            }
        }
    }
}
