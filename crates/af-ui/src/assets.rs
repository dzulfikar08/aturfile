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
    ]
);

/// Register with `gpui_kit::application().with_assets(AppAssets)`.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
