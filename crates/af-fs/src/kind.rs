//! What a file is, from its name: drives icons, colours, the Kind column and
//! which preview to show.

/// Broad file kinds (one icon and colour each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Folder,
    /// A macOS bundle that opens as a document (`.pages`, `.photoslibrary`).
    Package,
    Application,
    Image,
    Video,
    Audio,
    Archive,
    DiskImage,
    Code,
    Markdown,
    Text,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Other,
}

/// Bundles Finder opens as documents, not folders.
pub const PACKAGE_EXTENSIONS: &[&str] = &[
    "app",
    "bundle",
    "framework",
    "pages",
    "numbers",
    "key",
    "photoslibrary",
    "xcodeproj",
    "xcworkspace",
    "playground",
    "rtfd",
    "pkg",
    "plugin",
    "kext",
];

const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "svg", "ico",
    "icns", "raw", "cr2", "nef", "arw", "dng", "psd",
];
const VIDEO: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg",
];
const AUDIO: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "flac", "aiff", "aif", "ogg", "opus", "wma", "caf",
];
const ARCHIVE: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "lz", "lzma", "cpio", "jar", "xip",
];
const DISK_IMAGE: &[&str] = &["dmg", "iso", "img", "sparseimage", "sparsebundle", "vmdk"];
const CODE: &[&str] = &[
    "rs",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "m",
    "mm",
    "swift",
    "go",
    "py",
    "rb",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "java",
    "kt",
    "kts",
    "scala",
    "cs",
    "php",
    "lua",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "pl",
    "r",
    "dart",
    "zig",
    "nim",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "clj",
    "sql",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "vue",
    "svelte",
    "astro",
    "json",
    "jsonc",
    "toml",
    "yaml",
    "yml",
    "xml",
    "plist",
    "ini",
    "cfg",
    "conf",
    "gradle",
    "cmake",
    "mk",
    "dockerfile",
    "proto",
    "graphql",
    "wgsl",
    "glsl",
    "metal",
    "lock",
];
const TEXT: &[&str] = &[
    "txt", "text", "log", "csv", "tsv", "rtf", "env", "diff", "patch",
];
const DOCUMENT: &[&str] = &["doc", "docx", "odt", "epub", "tex"];
const SPREADSHEET: &[&str] = &["xls", "xlsx", "ods"];
const PRESENTATION: &[&str] = &["ppt", "pptx", "odp"];
/// Names without an extension that are still source files.
const CODE_NAMES: &[&str] = &[
    "makefile",
    "dockerfile",
    "justfile",
    "rakefile",
    "gemfile",
    "procfile",
    "license",
    "readme",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".zshrc",
    ".bashrc",
    ".profile",
];

/// Lowercased extension of `name` ("" when there is none). A leading dot
/// alone (".zshrc") is not an extension.
pub fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        Some(0) | None => String::new(),
        Some(i) => name[i + 1..].to_lowercase(),
    }
}

/// Classify by name. `is_dir` decides between folders and bundles.
pub fn classify(name: &str, is_dir: bool) -> FileKind {
    let ext = extension_of(name);
    if is_dir {
        return match ext.as_str() {
            "app" => FileKind::Application,
            e if PACKAGE_EXTENSIONS.contains(&e) => FileKind::Package,
            _ => FileKind::Folder,
        };
    }
    let e = ext.as_str();
    let has = |list: &[&str]| list.contains(&e);
    if e.is_empty() {
        let lower = name.to_lowercase();
        return if CODE_NAMES.contains(&lower.as_str()) {
            FileKind::Code
        } else {
            FileKind::Other
        };
    }
    match e {
        "md" | "markdown" | "mdx" => FileKind::Markdown,
        "pdf" => FileKind::Pdf,
        "app" => FileKind::Application,
        _ if has(IMAGE) => FileKind::Image,
        _ if has(VIDEO) => FileKind::Video,
        _ if has(AUDIO) => FileKind::Audio,
        _ if has(ARCHIVE) => FileKind::Archive,
        _ if has(DISK_IMAGE) => FileKind::DiskImage,
        _ if has(CODE) => FileKind::Code,
        _ if has(TEXT) => FileKind::Text,
        _ if has(DOCUMENT) => FileKind::Document,
        _ if has(SPREADSHEET) => FileKind::Spreadsheet,
        _ if has(PRESENTATION) => FileKind::Presentation,
        _ => FileKind::Other,
    }
}

impl FileKind {
    /// The generic word for the Kind column ("PNG image").
    fn noun(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Package => "Package",
            Self::Application => "Application",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Archive => "archive",
            Self::DiskImage => "disk image",
            Self::Code => "source",
            Self::Markdown => "Markdown",
            Self::Text => "text",
            Self::Pdf => "PDF document",
            Self::Document => "document",
            Self::Spreadsheet => "spreadsheet",
            Self::Presentation => "presentation",
            Self::Other => "Document",
        }
    }

    /// Whether the preview pane can show the file's text.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Code | Self::Markdown | Self::Text)
    }
}

/// Readable names for common source and text formats.
fn language(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust source",
        "py" => "Python script",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" => "TypeScript",
        "tsx" | "jsx" => "React source",
        "swift" => "Swift source",
        "go" => "Go source",
        "c" => "C source",
        "h" | "hpp" => "C header",
        "cc" | "cpp" => "C++ source",
        "m" | "mm" => "Objective-C source",
        "java" => "Java source",
        "kt" | "kts" => "Kotlin source",
        "rb" => "Ruby script",
        "php" => "PHP script",
        "sh" | "bash" | "zsh" | "fish" => "Shell script",
        "html" | "htm" => "HTML document",
        "css" | "scss" | "sass" | "less" => "Stylesheet",
        "json" | "jsonc" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "xml" => "XML",
        "plist" => "Property list",
        "sql" => "SQL",
        "lock" => "Lock file",
        "txt" | "text" => "Plain text",
        "log" => "Log file",
        "csv" => "CSV",
        "tsv" => "TSV",
        "rtf" => "Rich text",
        "diff" | "patch" => "Patch",
        _ => return None,
    })
}

/// The Kind column's text: "Folder", "PNG image", "Markdown", "Document".
pub fn kind_label(name: &str, is_dir: bool) -> String {
    let kind = classify(name, is_dir);
    if let Some(lang) = language(&extension_of(name)).filter(|_| !is_dir) {
        return lang.to_owned();
    }
    match kind {
        FileKind::Folder
        | FileKind::Package
        | FileKind::Application
        | FileKind::Markdown
        | FileKind::Pdf => kind.noun().to_owned(),
        FileKind::Other => {
            let ext = extension_of(name);
            if ext.is_empty() {
                "Document".to_owned()
            } else {
                format!("{} file", ext.to_uppercase())
            }
        }
        _ => format!("{} {}", extension_of(name).to_uppercase(), kind.noun())
            .trim()
            .to_owned(),
    }
}

/// Images GPUI can decode cheaply enough for thumbnails and previews.
pub fn is_decodable_image(name: &str) -> bool {
    matches!(
        extension_of(name).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_files() {
        assert_eq!(classify("photo.JPG", false), FileKind::Image);
        assert_eq!(classify("main.rs", false), FileKind::Code);
        assert_eq!(classify("README.md", false), FileKind::Markdown);
        assert_eq!(classify("Makefile", false), FileKind::Code);
        assert_eq!(classify(".zshrc", false), FileKind::Code);
        assert_eq!(classify("archive.tar.gz", false), FileKind::Archive);
        assert_eq!(classify("Safari.app", true), FileKind::Application);
        assert_eq!(classify("Doc.pages", true), FileKind::Package);
        assert_eq!(classify("src", true), FileKind::Folder);
        assert_eq!(classify("data.bin", false), FileKind::Other);
    }

    #[test]
    fn labels() {
        assert_eq!(kind_label("a.png", false), "PNG image");
        assert_eq!(kind_label("notes.md", false), "Markdown");
        assert_eq!(kind_label("x", true), "Folder");
        assert_eq!(kind_label("data.bin", false), "BIN file");
        assert_eq!(kind_label("main.rs", false), "Rust source");
        assert_eq!(kind_label("notes.txt", false), "Plain text");
        assert_eq!(kind_label("noext", false), "Document");
    }

    #[test]
    fn extensions() {
        assert_eq!(extension_of(".hidden"), "");
        assert_eq!(extension_of("a.B"), "b");
        assert_eq!(extension_of("plain"), "");
        assert!(is_decodable_image("x.webp"));
        assert!(!is_decodable_image("x.heic"));
    }
}
