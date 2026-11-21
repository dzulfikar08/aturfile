//! The app's asset source: GPUI Kit's default component icons plus the
//! Lucide icons aturfile uses (embedded one by one, not the whole catalog).
//! Name them with `gpui_kit::assets::IconName` (e.g. `IconName::Folder`).

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    AppIcons,
    [
        // Places and devices
        House,
        Monitor,
        FileText,
        Download,
        AppWindow,
        HardDrive,
        Usb,
        Server,
        Pin,
        PinOff,
        BrushCleaning,
        // File kinds
        Folder,
        FolderOpen,
        FolderPlus,
        File,
        FileCode,
        FileImage,
        FileVideoCamera,
        FileMusic,
        FileArchive,
        FileSpreadsheet,
        FileTerminal,
        FileSymlink,
        Package,
        Presentation,
        // Toolbar and shell
        ChevronLeft,
        ChevronRight,
        ChevronUp,
        ChevronDown,
        List,
        Columns3,
        LayoutGrid,
        PanelLeft,
        PanelRight,
        Search,
        Plus,
        X,
        Command,
        // Context menu, preview, palette
        Copy,
        Trash,
        Pencil,
        Eye,
        ExternalLink,
        Info,
        Settings,
        SunMoon,
        Rows3,
        RotateCcw,
        RotateCw,
        Clipboard,
        ClipboardPaste,
        Scissors,
        CopyPlus,
        EyeOff,
        RefreshCw,
        CircleCheck,
        Circle,
        Check,
        Files,
    ]
);

/// Register with `gpui_kit::application().with_assets(AppAssets)`.
pub struct AppAssets;

/// The app icon, for About and the Pro sheet: `img(APP_ICON)`.
pub const APP_ICON: &str = "brand/app-icon.png";
const APP_ICON_PNG: &[u8] = include_bytes!("../../../packaging/AppIcon.png");

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(APP_ICON_PNG)));
        }
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        if APP_ICON.starts_with(path) {
            paths.push(APP_ICON.into());
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
