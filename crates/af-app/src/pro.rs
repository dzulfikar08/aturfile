//! Pro in Mac App Store builds (feature `store`): multiple tabs and Clean
//! Up's Move to Trash. Every other build is fully unlocked. The App Store's
//! last answer is cached so Pro is known at launch, offline too.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use af_store::{Cache, Outcome, STORE_BUILD};
use gpui_kit::*;

pub struct Pro {
    pub unlocked: bool,
    pub price: Option<String>,
    /// A purchase or restore is running.
    pub busy: bool,
    /// The last purchase or restore message.
    pub message: Option<SharedString>,
}

impl Global for Pro {}

/// Set by StoreKit's transaction listener (any thread).
static CHANGED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_transaction() {
    CHANGED.store(true, Ordering::Relaxed);
}

fn cache_file() -> std::path::PathBuf {
    af_mac::app_data_dir().join("pro.json")
}

pub fn unlocked(cx: &App) -> bool {
    !STORE_BUILD || cx.try_global::<Pro>().is_some_and(|p| p.unlocked)
}

pub fn state(cx: &App) -> Option<&Pro> {
    cx.try_global::<Pro>()
}

pub fn init(cx: &mut App) {
    if !STORE_BUILD {
        cx.set_global(Pro {
            unlocked: true,
            price: None,
            busy: false,
            message: None,
        });
        return;
    }
    let cache = Cache::load(&cache_file());
    cx.set_global(Pro {
        unlocked: cache.pro,
        price: cache.price.clone(),
        busy: false,
        message: None,
    });
    af_store::listen(on_transaction);
    refresh(cx);
    // Transactions that arrive on their own (Ask to Buy, refunds).
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            if CHANGED.swap(false, Ordering::Relaxed) {
                cx.update(refresh);
            }
        }
    })
    .detach();
}

fn apply(result: Result<af_store::Snapshot, String>, cx: &mut App) {
    let pro = cx.global_mut::<Pro>();
    pro.busy = false;
    match result {
        Ok(snap) => {
            let mut cache = Cache::load(&cache_file());
            cache.update(&snap);
            let _ = cache.save(&cache_file());
            pro.unlocked = snap.pro;
            if snap.price.is_some() {
                pro.price = snap.price;
            }
        }
        Err(e) if e != "cancelled" => pro.message = Some(e.into()),
        Err(_) => {}
    }
    cx.refresh_windows();
}

/// Ask the App Store again (quietly).
pub fn refresh(cx: &mut App) {
    let work = cx
        .background_executor()
        .spawn(async { af_store::snapshot() });
    cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| apply(result, cx));
    })
    .detach();
}

pub fn purchase(cx: &mut App) {
    if !STORE_BUILD {
        return;
    }
    cx.global_mut::<Pro>().busy = true;
    cx.global_mut::<Pro>().message = None;
    let work = cx
        .background_executor()
        .spawn(async { af_store::purchase() });
    cx.spawn(async move |cx| {
        let outcome = work.await;
        cx.update(|cx| match outcome {
            Ok(Outcome::Purchased) => refresh(cx),
            Ok(Outcome::Pending) => {
                let p = cx.global_mut::<Pro>();
                p.busy = false;
                p.message = Some("Waiting for approval (Ask to Buy).".into());
                cx.refresh_windows();
            }
            Ok(Outcome::Cancelled) => {
                cx.global_mut::<Pro>().busy = false;
                cx.refresh_windows();
            }
            Err(e) => apply(Err(e), cx),
        });
    })
    .detach();
}

pub fn restore(cx: &mut App) {
    if !STORE_BUILD {
        return;
    }
    cx.global_mut::<Pro>().busy = true;
    let work = cx
        .background_executor()
        .spawn(async { af_store::restore() });
    cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| {
            let none = matches!(&result, Ok(s) if !s.pro);
            apply(result, cx);
            if none {
                cx.global_mut::<Pro>().message =
                    Some("No Pro purchase found for this Apple Account.".into());
            }
        });
    })
    .detach();
}
