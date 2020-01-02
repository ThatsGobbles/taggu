use std::path::Path;
use std::path::PathBuf;
use std::borrow::Cow;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;

use crate::config::selection::Selection;
use crate::config::serialize_format::SerializeFormat;

#[derive(Debug)]
pub enum Error {
    InvalidItemDirPath(PathBuf),
    CannotAccessItemPath(PathBuf, IoError),
    NoItemPathParent(PathBuf),
    CannotReadItemDir(IoError),
    CannotReadItemDirEntry(IoError),

    InvalidMetaFilePath(PathBuf),
    CannotAccessMetaPath(PathBuf, IoError),
    NoMetaPathParent(PathBuf),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Self::InvalidItemDirPath(ref p) => write!(f, "invalid item directory path: {}", p.display()),
            Self::CannotAccessItemPath(ref p, ref err) => write!(f, r#"cannot access item path "{}", error: {}"#, p.display(), err),
            Self::NoItemPathParent(ref p) => write!(f, "item path does not have a parent and/or is filesystem root: {}", p.display()),
            Self::CannotReadItemDir(ref err) => write!(f, "unable to read entries in item directory: {}", err),
            Self::CannotReadItemDirEntry(ref err) => write!(f, "unable to read item directory entry: {}", err),

            Self::InvalidMetaFilePath(ref p) => write!(f, "invalid meta file path: {}", p.display()),
            Self::CannotAccessMetaPath(ref p, ref err) => write!(f, r#"cannot access meta path "{}", error: {}"#, p.display(), err),
            Self::NoMetaPathParent(ref p) => write!(f, "meta path does not have a parent and/or is filesystem root: {}", p.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::CannotAccessItemPath(_, ref err) => Some(err),
            Self::CannotAccessMetaPath(_, ref err) => Some(err),
            Self::CannotReadItemDir(ref err) => Some(err),
            Self::CannotReadItemDirEntry(ref err) => Some(err),
            _ => None,
        }
    }
}

impl Error {
    pub(crate) fn is_fatal(&self) -> bool {
        match self {
            Self::CannotAccessMetaPath(_, io_error) => {
                match io_error.kind() {
                    IoErrorKind::NotFound => false,
                    _ => true,
                }
            },
            Self::InvalidItemDirPath(..) | Self::NoItemPathParent(..) => false,
            _ => true,
        }
    }
}

/// Represents the target location of the item files that a given metadata file
/// provides metadata for, relative to the location of the metadata file itself.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy, EnumIter)]
pub enum Target {
    Siblings,
    Parent,
}

impl Target {
    /// Provides the meta file path that provides metadata for an item file for
    /// this target.
    // NOTE: This always returns a `PathBuf`, since joining paths is required.
    pub fn get_meta_path<'a, P>(
        &'a self,
        item_path: P,
        serialize_format: SerializeFormat,
    ) -> Result<PathBuf, Error>
    where
        P: Into<Cow<'a, Path>>,
    {
        let item_path = item_path.into();

        // Get filesystem stat for item path.
        // This step is always done, even if the file/dir status does not need to be checked,
        // as it provides useful error information about permissions and non-existence.
        let item_fs_stat = match std::fs::metadata(&item_path) {
            Err(err) => return Err(Error::CannotAccessItemPath(item_path.into(), err)),
            Ok(item_fs_stat) => item_fs_stat,
        };

        let meta_path_parent_dir = match self {
            Self::Parent => {
                if !item_fs_stat.is_dir() {
                    return Err(Error::InvalidItemDirPath(item_path.into()))
                }

                item_path.as_ref()
            },
            Self::Siblings => {
                match item_path.as_ref().parent() {
                    Some(item_path_parent) => item_path_parent,
                    None => Err(Error::NoItemPathParent(item_path.into()))?,
                }
            }
        };

        // Create the target meta file name.
        let target_fn = format!("{}.{}", self.default_file_name(), serialize_format.file_extension());
        let meta_path = meta_path_parent_dir.join(target_fn);

        // Get filesystem stat for meta path.
        // This step is always done, even if the file/dir status does not need to be checked,
        // as it provides useful error information about permissions and non-existence.
        let meta_fs_stat = match std::fs::metadata(&meta_path) {
            Err(err) => return Err(Error::CannotAccessMetaPath(meta_path.into(), err)),
            Ok(meta_fs_stat) => meta_fs_stat,
        };

        // Ensure that the meta path is indeed a file.
        if !meta_fs_stat.is_file() {
            // Found a directory with the meta file name, this would be an unusual error case.
            Err(Error::InvalidMetaFilePath(meta_path))
        }
        else {
            Ok(meta_path)
        }
    }

    /// Provides the possible owned item paths of this target.
    /// This is a listing of the file paths that this meta target could/should provide metadata for.
    /// Note that this does NOT parse meta files, it only uses file system locations and presence.
    /// Also, no filtering or sorting of the returned item paths is performed.
    pub fn get_item_paths<'a, P>(&'a self, meta_path: P) -> Result<Vec<PathBuf>, Error>
    where
        P: Into<Cow<'a, Path>>,
    {
        let meta_path = meta_path.into();

        let meta_fs_stat = match std::fs::metadata(&meta_path) {
            Err(err) => return Err(Error::CannotAccessMetaPath(meta_path.into(), err)),
            Ok(meta_fs_stat) => meta_fs_stat,
        };

        if !meta_fs_stat.is_file() {
            return Err(Error::InvalidMetaFilePath(meta_path.into()))
        }

        // Get the parent directory of the meta file.
        // NOTE: This is only outside the pattern match because all branches currently use it.
        if let Some(meta_parent_dir_path) = meta_path.parent() {
            let mut po_item_paths = vec![];

            match self {
                Self::Parent => {
                    // This is just the passed-in path, just push it on unchanged.
                    po_item_paths.push(meta_parent_dir_path.into());
                },
                Self::Siblings => {
                    // Return all children of this directory.
                    for entry in std::fs::read_dir(&meta_parent_dir_path).map_err(Error::CannotReadItemDir)? {
                        po_item_paths.push(entry.map_err(Error::CannotReadItemDirEntry)?.path());
                    }
                },
            }

            Ok(po_item_paths)
        }
        else {
            // This should never happen!
            Err(Error::NoMetaPathParent(meta_path.into()))?
        }
    }

    // NOTE: No sorting is performed, sorting only occurs if needed during plexing.
    pub fn get_selected_item_paths<'a, P>(
        &'a self,
        meta_path: P,
        selection: &'a Selection,
        ) -> Result<Vec<PathBuf>, Error>
    where
        P: Into<Cow<'a, Path>>,
    {
        let mut item_paths = self.get_item_paths(meta_path)?;

        item_paths.retain(|p| selection.is_selected(p));

        Ok(item_paths)
    }

    pub fn default_file_name(&self) -> &'static str {
        match self {
            Self::Parent => "self",
            Self::Siblings => "item",
        }
    }
}

// enum ItemPaths<'a> {
//     Parent(Option<&'a Path>),
//     Siblings(ReadDir),
// }

// impl<'a> Iterator for ItemPaths<'a> {
//     type Item = Result<Cow<'a, Path>, IoError>;

//     fn next(&mut self) -> Option<Self::Item> {
//         match self {
//             Self::Parent(o) => o.take().map(Cow::Borrowed).map(Result::Ok),
//             Self::Siblings(rd) => rd.next().map(|dir_res| {
//                 dir_res.map(|entry| Cow::Owned(entry.path()))
//             }),
//         }
//     }
// }

// struct SelectedItemPaths<'a>(&'a Selection, ItemPaths<'a>);

// impl<'a> Iterator for SelectedItemPaths<'a> {
//     type Item = Result<Cow<'a, Path>, IoError>;

//     fn next(&mut self) -> Option<Self::Item> {
//         let selection = &self.0;
//         self.1.find(|res| match res {
//             Ok(p) => selection.is_selected(p),
//             Err(_) => true,
//         })
//     }
// }
