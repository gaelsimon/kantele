//! The folders a library is read from, and how a path under one of them is named.

use std::path::{Path, PathBuf};

use super::fold::nfc;

/// One music folder, and the name it goes by inside every relative path when there are several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub label: String,
    pub path: PathBuf,
}

/// The music folders. With one, a relative path is simply under it; with several, it starts with
/// its folder's label, so two folders holding `Bach/Cantatas` name two things.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roots {
    roots: Vec<Root>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RootsError {
    #[error("no music folder")]
    None,
    #[error("two music folders are both called {label}; folders are told apart by their names")]
    SameName { label: String },
    #[error("{inner} is inside {outer}, so it would be read twice")]
    Nested { inner: PathBuf, outer: PathBuf },
}

impl Roots {
    pub fn one(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            roots: vec![Root {
                label: label_of(&path),
                path,
            }],
        }
    }

    /// Several folders, each labelled by its name. Names are published in NFC, so two differing only
    /// in their spelling on disk are refused.
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, RootsError> {
        let mut roots: Vec<Root> = Vec::new();
        for path in paths {
            let label = label_of(&path);
            if let Some(other) = roots.iter().find(|root| nfc(&root.label) == nfc(&label)) {
                if other.path == path {
                    continue;
                }
                return Err(RootsError::SameName { label });
            }
            for other in &roots {
                if path.starts_with(&other.path) {
                    return Err(RootsError::Nested {
                        inner: path,
                        outer: other.path.clone(),
                    });
                }
                if other.path.starts_with(&path) {
                    return Err(RootsError::Nested {
                        inner: other.path.clone(),
                        outer: path,
                    });
                }
            }
            roots.push(Root { label, path });
        }
        if roots.is_empty() {
            return Err(RootsError::None);
        }
        Ok(Self { roots })
    }

    pub fn len(&self) -> usize {
        self.roots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Whether relative paths carry no label, which is the case that keeps an existing store valid.
    pub fn is_one(&self) -> bool {
        self.roots.len() == 1
    }

    pub fn iter(&self) -> impl Iterator<Item = &Root> {
        self.roots.iter()
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.roots.iter().map(|root| root.path.as_path())
    }

    /// The relative form of a path on disk, or nothing where it is under no music folder.
    pub fn relative(&self, path: &Path) -> Option<PathBuf> {
        let root = self
            .roots
            .iter()
            .find(|root| path.starts_with(&root.path))?;
        let inside = path.strip_prefix(&root.path).ok()?;
        Some(match self.is_one() {
            true => inside.to_path_buf(),
            false => Path::new(&root.label).join(inside),
        })
    }

    /// The relative form, or the path itself where it is under no music folder.
    pub fn relative_or_self(&self, path: &Path) -> PathBuf {
        self.relative(path).unwrap_or_else(|| path.to_path_buf())
    }

    /// Where a relative path lives on disk, or nothing where it starts with no folder's label. A row
    /// written when there was one folder names nothing once there are several, and is stale.
    pub fn absolute(&self, relative: &Path) -> Option<PathBuf> {
        match self.roots.as_slice() {
            [] => Some(relative.to_path_buf()),
            [only] => Some(only.path.join(relative)),
            several => {
                let mut parts = relative.components();
                let label = parts.next().and_then(|first| first.as_os_str().to_str())?;
                let root = several.iter().find(|root| root.label == label)?;
                Some(root.path.join(parts.as_path()))
            }
        }
    }

    /// The folders on disk a relative folder stands for: all of them for the empty path when there
    /// are several, one otherwise, none where the path names no folder.
    pub fn starts(&self, relative: &Path) -> Vec<PathBuf> {
        if relative.as_os_str().is_empty() && !self.is_one() {
            return self.roots.iter().map(|root| root.path.clone()).collect();
        }
        self.absolute(relative).into_iter().collect()
    }

    /// Whether a path on disk is one of the music folders itself.
    pub fn is_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| root.path == path)
    }

    /// What the library is called.
    pub fn name(&self) -> String {
        match self.roots.as_slice() {
            [only] => super::library::library_name(&only.path),
            _ => "Music".to_owned(),
        }
    }

    /// One line naming every folder, for a log.
    pub fn describe(&self) -> String {
        self.roots
            .iter()
            .map(|root| root.path.display().to_string())
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// What the store writes down to recognise the library it holds. One folder writes its path
    /// alone, so an older store still reads.
    pub fn meta(&self) -> Option<String> {
        self.roots
            .iter()
            .map(|root| root.path.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .map(|paths| paths.join("\n"))
    }
}

fn label_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Music".to_owned())
}

impl From<&Path> for Roots {
    fn from(path: &Path) -> Self {
        Self::one(path)
    }
}

impl From<&PathBuf> for Roots {
    fn from(path: &PathBuf) -> Self {
        Self::one(path.clone())
    }
}

impl From<PathBuf> for Roots {
    fn from(path: PathBuf) -> Self {
        Self::one(path)
    }
}

impl From<&Roots> for Roots {
    fn from(roots: &Roots) -> Self {
        roots.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_folder_names_its_files_as_it_always_did() {
        let roots = Roots::one("/volume1/music");
        assert_eq!(
            roots.relative(Path::new("/volume1/music/Bach/1.flac")),
            Some(PathBuf::from("Bach/1.flac"))
        );
        assert_eq!(
            roots.absolute(Path::new("Bach/1.flac")),
            Some(PathBuf::from("/volume1/music/Bach/1.flac"))
        );
        assert_eq!(roots.meta().as_deref(), Some("/volume1/music"));
        assert_eq!(roots.name(), "music");
        assert_eq!(roots.relative(Path::new("/elsewhere/1.flac")), None);
    }

    #[test]
    fn several_folders_are_told_apart_by_their_names_in_every_relative_path() {
        let roots = Roots::new([
            PathBuf::from("/volume1/music"),
            PathBuf::from("/volume2/more"),
        ])
        .expect("two folders");
        assert_eq!(
            roots.relative(Path::new("/volume2/more/Bach/1.flac")),
            Some(PathBuf::from("more/Bach/1.flac"))
        );
        assert_eq!(
            roots.absolute(Path::new("more/Bach/1.flac")),
            Some(PathBuf::from("/volume2/more/Bach/1.flac"))
        );
        assert_eq!(
            roots.absolute(Path::new("music/Bach/1.flac")),
            Some(PathBuf::from("/volume1/music/Bach/1.flac"))
        );
        assert_eq!(
            roots.absolute(Path::new("Bach/1.flac")),
            None,
            "a path written when there was one folder names nothing now"
        );
        assert_eq!(
            roots.starts(Path::new("")),
            [
                PathBuf::from("/volume1/music"),
                PathBuf::from("/volume2/more")
            ]
        );
        assert_eq!(
            roots.starts(Path::new("more/Bach")),
            [PathBuf::from("/volume2/more/Bach")]
        );
        assert!(roots.starts(Path::new("Bach")).is_empty());
        assert_eq!(
            roots.meta().as_deref(),
            Some("/volume1/music\n/volume2/more")
        );
        assert_eq!(roots.name(), "Music");
    }

    #[test]
    fn folders_that_cannot_be_told_apart_or_would_be_read_twice_are_refused() {
        assert_eq!(Roots::new([]), Err(RootsError::None));
        assert!(matches!(
            Roots::new([PathBuf::from("/a/music"), PathBuf::from("/b/music")]),
            Err(RootsError::SameName { .. })
        ));
        assert!(
            matches!(
                Roots::new([PathBuf::from("/a/Björk"), PathBuf::from("/b/Bjo\u{308}rk")]),
                Err(RootsError::SameName { .. })
            ),
            "one name a Mac writes decomposed is still one name"
        );
        assert!(matches!(
            Roots::new([PathBuf::from("/a"), PathBuf::from("/a/inside")]),
            Err(RootsError::Nested { .. })
        ));
        assert!(matches!(
            Roots::new([PathBuf::from("/a/inside"), PathBuf::from("/a")]),
            Err(RootsError::Nested { .. })
        ));
        let same =
            Roots::new([PathBuf::from("/a"), PathBuf::from("/a")]).expect("one folder twice");
        assert!(same.is_one(), "the same folder named twice is one folder");
    }
}
