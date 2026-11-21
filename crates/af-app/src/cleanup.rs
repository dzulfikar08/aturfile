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
use gpui::{
    ClickEvent, Context, ScrollStrategy, SharedString, Task, UniformListScrollHandle, div,
    prelude::*, px, uniform_list,
};

use crate::browser::{Workspace, tool_button};
use crate::theme;

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

fn safety_badge(s: Safety) -> impl IntoElement {
    let (label, color) = match s {
        Safety::Safe => ("SAFE", theme::safe()),
        Safety::Review => ("REVIEW", theme::review()),
        Safety::Info => ("INFO", theme::muted()),
    };
    div()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(color)
        .text_color(color)
        .text_xs()
        .child(label)
}

impl Workspace {
    pub fn render_cleanup(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

        let header = div()
            .pt(px(10.0))
            .h(px(64.0))
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_lg().child("Clean Up"))
                    .child(div().text_xs().text_color(theme::muted()).child(summary)),
            )
            .child(div().flex_1())
            .when(safe_total > 0, |s| {
                s.child(
                    tool_button(
                        "stage-safe",
                        &format!("Stage all safe ({})", format_size(safe_total)),
                        true,
                    )
                    .border_1()
                    .border_color(theme::safe())
                    .on_click(
                        cx.listener(|ws, _: &ClickEvent, _, cx| ws.cleanup.stage_all_safe(cx)),
                    ),
                )
            })
            .child(
                tool_button(
                    "rescan",
                    "Rescan",
                    !matches!(c.phase, Phase::Scanning { .. }),
                )
                .border_1()
                .border_color(theme::border())
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| Cleanup::start_scan(ws, cx))),
            );

        let list = if c.rows.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
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
            .track_scroll(c.scroll.clone())
            .flex_1()
            .into_any_element()
        };

        let queue_total = c.queue.total();
        let mut queue = div()
            .w(px(300.0))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme::border())
            .bg(theme::sidebar())
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(theme::muted()).text_xs().child("QUEUE"))
                    .child(div().text_lg().child(format_size(queue_total)))
                    .child(div().text_xs().text_color(theme::muted()).child(format!(
                        "{} items. Enter or \u{2318}S stages the selected row.",
                        c.queue.items.len()
                    ))),
            );
        let mut items = div().id("queue-items").flex_1().overflow_y_scroll().px_2();
        for (i, item) in c.queue.items.iter().enumerate() {
            let path = item.path.clone();
            items = items.child(
                div()
                    .id(("queued", i))
                    .flex()
                    .gap_2()
                    .py_1()
                    .px_1()
                    .rounded_sm()
                    .text_xs()
                    .hover(|s| s.bg(theme::hover()))
                    .child(div().flex_1().min_w_0().truncate().child(
                        af_clean::paths::display_path(&item.path, &af_clean::paths::home()),
                    ))
                    .child(
                        div()
                            .text_color(theme::muted())
                            .child(format_size(item.logical)),
                    )
                    .child(
                        div()
                            .id(("unstage", i))
                            .cursor_pointer()
                            .text_color(theme::muted())
                            .hover(|s| s.text_color(theme::danger()))
                            .child("\u{00d7}")
                            .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| {
                                ws.cleanup.queue.unstage(&path);
                                cx.notify();
                            })),
                    ),
            );
        }
        queue = queue.child(items).child(
            div().p_3().child(
                tool_button(
                    "trash-queue",
                    &format!("Move {} to Trash", format_size(queue_total)),
                    queue_total > 0,
                )
                .w_full()
                .h(px(32.0))
                .when(queue_total > 0, |s| s.bg(theme::accent()))
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| Cleanup::trash_queue(ws, cx))),
            ),
        );

        div()
            .flex_1()
            .min_w_0()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(list)
                    .child(
                        div()
                            .h(px(24.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .text_xs()
                            .text_color(theme::muted())
                            .border_t_1()
                            .border_color(theme::border())
                            .children(c.message.clone()),
                    ),
            )
            .child(queue)
    }

    fn cleanup_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let c = &self.cleanup;
        let base = div()
            .id(("clean", ix))
            .w_full()
            .h(px(30.0))
            .flex()
            .items_center()
            .px_4()
            .gap_2();
        match &c.rows[ix] {
            Row::Header {
                title,
                hint,
                safety,
                total,
            } => base
                .mt_1()
                .child(safety_badge(*safety))
                .child(div().child(title.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(hint.clone()),
                )
                .child(div().text_color(theme::muted()).child(format_size(*total))),
            Row::Item {
                label, size, path, ..
            } => {
                let staged = c.is_staged(path);
                let selected = ix == c.selected;
                base.mx_1()
                    .pl(px(28.0))
                    .rounded_md()
                    .when(selected, |s| s.bg(theme::selected()))
                    .when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
                    .child(
                        div()
                            .w(px(14.0))
                            .text_color(if staged {
                                theme::safe()
                            } else {
                                theme::border()
                            })
                            .child(if staged { "\u{2713}" } else { "\u{25cb}" }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                    .child(div().text_color(theme::muted()).child(format_size(*size)))
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
