//! File operations with undo: conflict-free names, recursive copy, move
//! with a cross-device fallback, and the undo/redo stack. Trashing goes
//! through a callback so the app can route it via its guarded engine.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `name` split into stem and extension. Folders and dot-files ("x.app"
/// excepted) keep their whole name as the stem.
pub fn split_name(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir
        && !crate::kind::PACKAGE_EXTENSIONS.contains(&crate::kind::extension_of(name).as_str())
    {
        return (name, "");
    }
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i..]),
    }
}

/// A name for `name` in `dir` that is not taken: "Report.pdf", then
/// "Report 2.pdf", "Report 3.pdf". A name that already ends in a number
/// ("Report 2.pdf") continues from it.
pub fn unique_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let first = dir.join(name);
    if first.symlink_metadata().is_err() {
        return first;
    }
    let (stem, ext) = split_name(name, is_dir);
    let (base, start) = match stem.rsplit_once(' ') {
        Some((b, n)) if !b.is_empty() && n.parse::<u32>().is_ok_and(|n| n >= 2) => {
            (b, n.parse::<u32>().unwrap_or(1) + 1)
        }
        _ => (stem, 2),
    };
    (start..)
        .map(|n| dir.join(format!("{base} {n}{ext}")))
        .find(|p| p.symlink_metadata().is_err())
        .expect("some number is free")
}

/// Names a user may give an item.
pub fn validate_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("A name can't be empty".into());
    }
    if n == "." || n == ".." {
        return Err("That name is reserved".into());
    }
    if n.contains('/') || n.contains('\0') {
        return Err("Names can't contain \u{201c}/\u{201d}".into());
    }
    Ok(n)
}

/// Copy `src` to `dst` (which must not exist): files, folders recursively,
/// symlinks as links. `progress` gets the bytes of each file copied.
pub fn copy_path(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
    if dst.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", dst.display()),
        ));
    }
    if dst.starts_with(src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "can't copy a folder into itself",
        ));
    }
    copy_inner(src, dst, progress)
}

fn copy_inner(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    let ft = md.file_type();
    if ft.is_symlink() {
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dst)?;
        #[cfg(not(unix))]
        fs::copy(src, dst).map(|_| ())?;
    } else if ft.is_dir() {
        fs::create_dir(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_inner(&entry.path(), &dst.join(entry.file_name()), progress)?;
        }
        let _ = fs::set_permissions(dst, md.permissions());
    } else {
        let n = fs::copy(src, dst)?;
        progress(n);
    }
    Ok(())
}

/// Bytes under `path` (for a progress total).
pub fn total_bytes(path: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(path) else {
        return 0;
    };
    if md.is_dir() {
        fs::read_dir(path)
            .map(|rd| rd.flatten().map(|e| total_bytes(&e.path())).sum())
            .unwrap_or(0)
    } else {
        md.len()
    }
}

/// Move `src` to `dst` (which must not exist). A rename when both are on
/// one volume; otherwise copy, then remove the original.
pub fn move_path(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
    if dst.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", dst.display()),
        ));
    }
    if dst.starts_with(src) && dst != src {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "can't move a folder into itself",
        ));
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            copy_inner(src, dst, progress)?;
            remove_path(src)
        }
        Err(e) => Err(e),
    }
}

fn is_cross_device(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18) // EXDEV
}

fn remove_path(path: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// One undoable change, recorded after it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// A new, empty folder.
    Created(PathBuf),
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
    /// Copies made: `(source, copy)`.
    Copied(Vec<(PathBuf, PathBuf)>),
    /// Items moved: `(from, to)`.
    Moved(Vec<(PathBuf, PathBuf)>),
    /// Items moved to the Trash: `(original, where it is in the Trash)`.
    Trashed(Vec<(PathBuf, PathBuf)>),
}

impl Op {
    /// "Undo Rename", for the status bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created(_) => "New Folder",
            Self::Renamed { .. } => "Rename",
            Self::Copied(_) => "Copy",
            Self::Moved(_) => "Move",
            Self::Trashed(_) => "Move to Trash",
        }
    }

    /// Paths this op touched, to refresh the views showing them.
    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Created(p) => vec![p.clone()],
            Self::Renamed { from, to } => vec![from.clone(), to.clone()],
            Self::Copied(v) | Self::Moved(v) | Self::Trashed(v) => {
                v.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect()
            }
        }
    }
}

/// Moves an item to the Trash and says where it went.
pub type TrashFn<'a> = &'a dyn Fn(&Path) -> io::Result<Option<PathBuf>>;

/// Reverse `op`. Copies and new folders go to the Trash (never deleted
/// outright), moves and renames go back, trashed items come back out.
pub fn undo(op: &Op, trash: TrashFn) -> io::Result<()> {
    let mut ignore = |_: u64| {};
    match op {
        Op::Created(p) => trash(p).map(|_| ()),
        Op::Renamed { from, to } => move_path(to, from, &mut ignore),
        Op::Copied(pairs) => pairs
            .iter()
            .try_for_each(|(_, copy)| trash(copy).map(|_| ())),
        Op::Moved(pairs) | Op::Trashed(pairs) => pairs
            .iter()
            .rev()
            .try_for_each(|(from, to)| move_path(to, from, &mut ignore)),
    }
}

/// Do `op` again after an undo. Returns the op as now recorded (a re-trash
/// lands somewhere new in the Trash).
pub fn redo(op: &Op, trash: TrashFn) -> io::Result<Op> {
    let mut ignore = |_: u64| {};
    match op {
        Op::Created(p) => fs::create_dir(p).map(|_| op.clone()),
        Op::Renamed { from, to } => move_path(from, to, &mut ignore).map(|_| op.clone()),
        Op::Copied(pairs) => {
            for (src, copy) in pairs {
                copy_path(src, copy, &mut ignore)?;
            }
            Ok(op.clone())
        }
        Op::Moved(pairs) => {
            for (from, to) in pairs {
                move_path(from, to, &mut ignore)?;
            }
            Ok(op.clone())
        }
        Op::Trashed(pairs) => {
            let mut now = Vec::new();
            for (orig, _) in pairs {
                if let Some(at) = trash(orig)? {
                    now.push((orig.clone(), at));
                }
            }
            Ok(Op::Trashed(now))
        }
    }
}

/// Undo and redo history. New changes clear the redo side.
#[derive(Debug, Default)]
pub struct UndoStack<T> {
    done: Vec<T>,
    undone: Vec<T>,
    limit: usize,
}

impl<T> UndoStack<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            limit: limit.max(1),
        }
    }

    pub fn push(&mut self, item: T) {
        self.undone.clear();
        self.done.push(item);
        if self.done.len() > self.limit {
            self.done.remove(0);
        }
    }

    /// The change to undo; call [`Self::undone`] once it is reversed.
    pub fn take_undo(&mut self) -> Option<T> {
        self.done.pop()
    }

    /// Record that `item` was undone (it can be redone).
    pub fn undone(&mut self, item: T) {
        self.undone.push(item);
    }

    /// The change to redo; call [`Self::redone`] once it is applied again.
    pub fn take_redo(&mut self) -> Option<T> {
        self.undone.pop()
    }

    /// Record that `item` was redone, keeping the redo side intact.
    pub fn redone(&mut self, item: T) {
        self.done.push(item);
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn peek_undo(&self) -> Option<&T> {
        self.done.last()
    }

    pub fn peek_redo(&self) -> Option<&T> {
        self.undone.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_names() {
        assert_eq!(split_name("a.tar.gz", false), ("a.tar", ".gz"));
        assert_eq!(split_name(".zshrc", false), (".zshrc", ""));
        assert_eq!(split_name("v1.2", true), ("v1.2", ""));
        assert_eq!(split_name("Safari.app", true), ("Safari", ".app"));
    }

    #[test]
    fn conflict_names_count_up() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(unique_name(d, "a.txt", false), d.join("a.txt"));
        fs::write(d.join("a.txt"), "").unwrap();
        assert_eq!(unique_name(d, "a.txt", false), d.join("a 2.txt"));
        fs::write(d.join("a 2.txt"), "").unwrap();
        assert_eq!(unique_name(d, "a.txt", false), d.join("a 3.txt"));
        // Continues from a number already there.
        assert_eq!(unique_name(d, "a 2.txt", false), d.join("a 3.txt"));
        fs::create_dir(d.join("untitled folder")).unwrap();
        assert_eq!(
            unique_name(d, "untitled folder", true),
            d.join("untitled folder 2")
        );
        // "Plan 1" is not a counter.
        fs::write(d.join("Plan 1"), "").unwrap();
        assert_eq!(unique_name(d, "Plan 1", false), d.join("Plan 1 2"));
    }

    #[test]
    fn validates_names() {
        assert_eq!(validate_name("  x "), Ok("x"));
        assert!(validate_name("").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
    }

    #[test]
    fn copies_and_moves_trees() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::create_dir_all(d.join("src/inner")).unwrap();
        fs::write(d.join("src/inner/f.txt"), "hello").unwrap();
        let mut bytes = 0;
        copy_path(&d.join("src"), &d.join("copy"), &mut |n| bytes += n).unwrap();
        assert_eq!(bytes, 5);
        assert_eq!(
            fs::read_to_string(d.join("copy/inner/f.txt")).unwrap(),
            "hello"
        );
        assert!(copy_path(&d.join("src"), &d.join("copy"), &mut |_| {}).is_err());
        assert!(copy_path(&d.join("src"), &d.join("src/inner/x"), &mut |_| {}).is_err());
        move_path(&d.join("copy"), &d.join("moved"), &mut |_| {}).unwrap();
        assert!(!d.join("copy").exists());
        assert!(d.join("moved/inner/f.txt").exists());
        assert_eq!(total_bytes(&d.join("moved")), 5);
    }

    fn fake_trash(bin: PathBuf) -> impl Fn(&Path) -> io::Result<Option<PathBuf>> {
        move |p: &Path| {
            fs::create_dir_all(&bin)?;
            let to = unique_name(&bin, &p.file_name().unwrap().to_string_lossy(), p.is_dir());
            fs::rename(p, &to)?;
            Ok(Some(to))
        }
    }

    #[test]
    fn undo_and_redo_each_op() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let trash = fake_trash(d.join(".bin"));
        fs::write(d.join("a"), "1").unwrap();

        let rename = Op::Renamed {
            from: d.join("a"),
            to: d.join("b"),
        };
        fs::rename(d.join("a"), d.join("b")).unwrap();
        undo(&rename, &trash).unwrap();
        assert!(d.join("a").exists() && !d.join("b").exists());
        redo(&rename, &trash).unwrap();
        assert!(d.join("b").exists());

        copy_path(&d.join("b"), &d.join("b 2"), &mut |_| {}).unwrap();
        let copied = Op::Copied(vec![(d.join("b"), d.join("b 2"))]);
        undo(&copied, &trash).unwrap();
        assert!(!d.join("b 2").exists());
        assert!(d.join(".bin/b 2").exists(), "copies go to the Trash");

        let at = trash(&d.join("b")).unwrap().unwrap();
        let trashed = Op::Trashed(vec![(d.join("b"), at)]);
        undo(&trashed, &trash).unwrap();
        assert!(d.join("b").exists());
        let again = redo(&trashed, &trash).unwrap();
        assert!(!d.join("b").exists());
        assert!(matches!(again, Op::Trashed(ref v) if v.len() == 1));

        fs::create_dir(d.join("new")).unwrap();
        let created = Op::Created(d.join("new"));
        undo(&created, &trash).unwrap();
        assert!(!d.join("new").exists());
        redo(&created, &trash).unwrap();
        assert!(d.join("new").is_dir());
    }

    #[test]
    fn stack_order_and_limit() {
        let mut s = UndoStack::new(2);
        s.push(1);
        s.push(2);
        s.push(3);
        assert_eq!(s.take_undo(), Some(3));
        s.undone(3);
        assert_eq!(s.peek_redo(), Some(&3));
        assert_eq!(s.take_undo(), Some(2));
        s.undone(2);
        assert_eq!(s.take_undo(), None, "the oldest fell off");
        assert_eq!(s.take_redo(), Some(2));
        s.redone(2);
        assert!(s.can_redo());
        s.push(9);
        assert!(!s.can_redo(), "a new change clears redo");
        assert_eq!(s.peek_undo(), Some(&9));
    }
}
