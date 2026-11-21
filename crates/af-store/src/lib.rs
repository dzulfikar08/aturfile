//! aturfile Pro. Mac App Store builds (feature `store`) sell one
//! non-consumable, [`PRO_PRODUCT`], through StoreKit 2: a small Swift bridge
//! (`swift/StoreKitBridge.swift`, compiled by build.rs, ported from Voltrus
//! Tools' `vt-store`). Every other build is fully unlocked and sells nothing.
//!
//! The StoreKit calls block until the App Store answers (a purchase waits for
//! the user): call them from a background thread, never the UI thread.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The App Store product id of the Pro unlock (a non-consumable in App
/// Store Connect). Follows the bundle id; see docs/release.md.
pub const PRO_PRODUCT: &str = "com.atur.aturfile.pro";

/// Whether this build sells Pro (otherwise Pro is always on).
pub const STORE_BUILD: bool = cfg!(feature = "store");

/// What the App Store said last, kept so Pro is known at launch, offline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    pub pro: bool,
    /// The displayed price, e.g. "$9.99".
    pub price: Option<String>,
}

impl Cache {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn update(&mut self, fresh: &Snapshot) {
        self.pro = fresh.pro;
        if fresh.price.is_some() {
            self.price = fresh.price.clone();
        }
    }
}

/// The App Store's answer about Pro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub pro: bool,
    pub price: Option<String>,
}

#[derive(Deserialize)]
struct RawEntitlement {
    product: String,
}

#[derive(Deserialize)]
struct RawSnapshot {
    #[serde(default)]
    entitlements: Vec<RawEntitlement>,
    #[serde(default)]
    prices: std::collections::HashMap<String, String>,
    error: Option<String>,
}

impl Snapshot {
    /// Parse the bridge's `{"entitlements":[...],"prices":{...}}` or
    /// `{"error": ...}`.
    pub fn from_bridge(json: &str) -> Result<Self, String> {
        let raw: RawSnapshot =
            serde_json::from_str(json).map_err(|e| format!("unreadable App Store answer: {e}"))?;
        if let Some(e) = raw.error {
            return Err(e);
        }
        Ok(Self {
            pro: raw.entitlements.iter().any(|e| e.product == PRO_PRODUCT),
            price: raw.prices.get(PRO_PRODUCT).cloned(),
        })
    }
}

/// How a purchase ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Purchased,
    /// Waiting for approval (Ask to Buy).
    Pending,
    Cancelled,
}

impl Outcome {
    pub fn from_bridge(json: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Raw {
            result: String,
            message: Option<String>,
        }
        let raw: Raw =
            serde_json::from_str(json).map_err(|e| format!("unreadable App Store answer: {e}"))?;
        match raw.result.as_str() {
            "purchased" => Ok(Self::Purchased),
            "pending" => Ok(Self::Pending),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(raw.message.unwrap_or_else(|| "The purchase failed".into())),
        }
    }
}

fn products_json() -> String {
    serde_json::to_string(&[PRO_PRODUCT]).unwrap_or_else(|_| "[]".to_owned())
}

/// Whether Pro is owned, and its price. Works offline (prices unknown then).
pub fn snapshot() -> Result<Snapshot, String> {
    Snapshot::from_bridge(&bridge::snapshot(&products_json()))
}

/// Buy Pro.
pub fn purchase() -> Result<Outcome, String> {
    Outcome::from_bridge(&bridge::purchase(PRO_PRODUCT))
}

/// Restore purchases (may ask the user to sign in), then answer like
/// [`snapshot`].
pub fn restore() -> Result<Snapshot, String> {
    Snapshot::from_bridge(&bridge::restore(&products_json()))
}

/// Call `on_update` (from a StoreKit thread) when a transaction arrives on
/// its own: Ask to Buy approved, bought on another Mac, refunded. Once per
/// process.
pub fn listen(on_update: extern "C" fn()) {
    bridge::listen(on_update);
}

#[cfg(all(feature = "store", target_os = "macos"))]
#[allow(unsafe_code)]
mod bridge {
    use std::ffi::{CStr, CString, c_char};

    unsafe extern "C" {
        fn af_sk_snapshot(ids_json: *const c_char) -> *mut c_char;
        fn af_sk_purchase(id: *const c_char) -> *mut c_char;
        fn af_sk_restore(ids_json: *const c_char) -> *mut c_char;
        fn af_sk_listen(callback: extern "C" fn());
        fn af_sk_free(pointer: *mut c_char);
    }

    fn call(function: unsafe extern "C" fn(*const c_char) -> *mut c_char, arg: &str) -> String {
        let Ok(arg) = CString::new(arg) else {
            return r#"{"error":"bad argument"}"#.to_owned();
        };
        // SAFETY: `arg` is NUL-terminated and outlives the call; the bridge
        // returns a strdup'd string, copied and freed exactly once.
        unsafe {
            let answer = function(arg.as_ptr());
            if answer.is_null() {
                return r#"{"error":"no answer from the App Store"}"#.to_owned();
            }
            let text = CStr::from_ptr(answer).to_string_lossy().into_owned();
            af_sk_free(answer);
            text
        }
    }

    pub fn snapshot(ids_json: &str) -> String {
        call(af_sk_snapshot, ids_json)
    }

    pub fn purchase(product: &str) -> String {
        call(af_sk_purchase, product)
    }

    pub fn restore(ids_json: &str) -> String {
        call(af_sk_restore, ids_json)
    }

    pub fn listen(callback: extern "C" fn()) {
        // SAFETY: a plain function pointer, valid for the whole process.
        unsafe { af_sk_listen(callback) }
    }
}

/// Source builds and other systems: nothing is sold.
#[cfg(not(all(feature = "store", target_os = "macos")))]
mod bridge {
    const UNAVAILABLE: &str = "This build of aturfile sells nothing: everything is unlocked.";

    pub fn snapshot(_: &str) -> String {
        format!(r#"{{"error":"{UNAVAILABLE}"}}"#)
    }

    pub fn purchase(_: &str) -> String {
        format!(r#"{{"result":"error","message":"{UNAVAILABLE}"}}"#)
    }

    pub fn restore(_: &str) -> String {
        snapshot("")
    }

    pub fn listen(_: extern "C" fn()) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snapshots() {
        let s = Snapshot::from_bridge(&format!(
            r#"{{"entitlements":[{{"product":"{PRO_PRODUCT}","original":1,"signed":2}}],"prices":{{"{PRO_PRODUCT}":"$9.99"}}}}"#
        ))
        .unwrap();
        assert!(s.pro);
        assert_eq!(s.price.as_deref(), Some("$9.99"));
        let none =
            Snapshot::from_bridge(r#"{"entitlements":[{"product":"other"}],"prices":{}}"#).unwrap();
        assert!(!none.pro && none.price.is_none());
        assert_eq!(
            Snapshot::from_bridge(r#"{"error":"cancelled"}"#),
            Err("cancelled".into())
        );
        assert!(Snapshot::from_bridge("junk").is_err());
    }

    #[test]
    fn parses_outcomes() {
        assert_eq!(
            Outcome::from_bridge(r#"{"result":"purchased","original":3}"#),
            Ok(Outcome::Purchased)
        );
        assert_eq!(
            Outcome::from_bridge(r#"{"result":"pending"}"#),
            Ok(Outcome::Pending)
        );
        assert_eq!(
            Outcome::from_bridge(r#"{"result":"cancelled"}"#),
            Ok(Outcome::Cancelled)
        );
        assert_eq!(
            Outcome::from_bridge(r#"{"result":"error","message":"nope"}"#),
            Err("nope".into())
        );
    }

    #[test]
    fn cache_round_trips_and_keeps_known_price() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pro.json");
        let mut c = Cache::default();
        c.update(&Snapshot {
            pro: true,
            price: Some("$9.99".into()),
        });
        c.update(&Snapshot {
            pro: true,
            price: None,
        });
        assert_eq!(c.price.as_deref(), Some("$9.99"));
        c.save(&file).unwrap();
        assert_eq!(Cache::load(&file), c);
        assert_eq!(Cache::load(&dir.path().join("missing")), Cache::default());
    }

    #[test]
    #[cfg(not(feature = "store"))]
    fn source_builds_sell_nothing() {
        assert!(snapshot().is_err());
        assert!(purchase().is_err());
        assert_eq!(products_json(), format!(r#"["{PRO_PRODUCT}"]"#));
    }
}
