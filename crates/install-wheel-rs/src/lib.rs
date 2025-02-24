//! Takes a wheel and installs it into a venv.

use std::io;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::str::FromStr;

use platform_info::PlatformInfoError;
use thiserror::Error;
use zip::result::ZipError;
use zip::ZipArchive;

use distribution_filename::WheelFilename;
use pep440_rs::Version;
use platform_host::{Arch, Os};
pub use uninstall::{uninstall_wheel, Uninstall};
use uv_fs::Simplified;
use uv_normalize::PackageName;

pub mod linker;
mod record;
mod script;
mod uninstall;
mod wheel;

/// The layout of the target environment into which a wheel can be installed.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The Python interpreter, as returned by `sys.executable`.
    pub sys_executable: PathBuf,
    /// The `purelib` directory, as returned by `sysconfig.get_paths()`.
    pub purelib: PathBuf,
    /// The `platlib` directory, as returned by `sysconfig.get_paths()`.
    pub platlib: PathBuf,
    /// The `include` directory, as returned by `sysconfig.get_paths()`.
    pub include: PathBuf,
    /// The `scripts` directory, as returned by `sysconfig.get_paths()`.
    pub scripts: PathBuf,
    /// The `data` directory, as returned by `sysconfig.get_paths()`.
    pub data: PathBuf,
    /// The Python version, as returned by `sys.version_info`.
    pub python_version: (u8, u8),
    /// The `os.name` value for the current platform.
    pub os_name: String,
}

/// Note: The caller is responsible for adding the path of the wheel we're installing.
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Custom error type to add a path to error reading a file from a zip
    #[error("Failed to reflink {} to {}", from.simplified_display(), to.simplified_display())]
    Reflink {
        from: PathBuf,
        to: PathBuf,
        #[source]
        err: io::Error,
    },
    /// Tags/metadata didn't match platform
    #[error("The wheel is incompatible with the current platform {os} {arch}")]
    IncompatibleWheel { os: Os, arch: Arch },
    /// The wheel is broken
    #[error("The wheel is invalid: {0}")]
    InvalidWheel(String),
    /// Doesn't follow file name schema
    #[error(transparent)]
    InvalidWheelFileName(#[from] distribution_filename::WheelFilenameError),
    /// The caller must add the name of the zip file (See note on type).
    #[error("Failed to read {0} from zip file")]
    Zip(String, #[source] ZipError),
    #[error("Failed to run Python subcommand")]
    PythonSubcommand(#[source] io::Error),
    #[error("Failed to move data files")]
    WalkDir(#[from] walkdir::Error),
    #[error("RECORD file doesn't match wheel contents: {0}")]
    RecordFile(String),
    #[error("RECORD file is invalid")]
    RecordCsv(#[from] csv::Error),
    #[error("Broken virtualenv: {0}")]
    BrokenVenv(String),
    #[error("Unable to create Windows launch for {0} (only x64_64 is supported)")]
    UnsupportedWindowsArch(&'static str),
    #[error("Unable to create Windows launcher on non-Windows platform")]
    NotWindows,
    #[error("Failed to detect the current platform")]
    PlatformInfo(#[source] PlatformInfoError),
    #[error("Invalid version specification, only none or == is supported")]
    Pep440,
    #[error("Invalid direct_url.json")]
    DirectUrlJson(#[from] serde_json::Error),
    #[error("No .dist-info directory found")]
    MissingDistInfo,
    #[error("Cannot uninstall package; RECORD file not found at: {}", _0.simplified_display())]
    MissingRecord(PathBuf),
    #[error("Multiple .dist-info directories found: {0}")]
    MultipleDistInfo(String),
    #[error("Invalid wheel size")]
    InvalidSize,
    #[error("Invalid package name")]
    InvalidName(#[from] uv_normalize::InvalidNameError),
    #[error("Invalid package version")]
    InvalidVersion(#[from] pep440_rs::VersionParseError),
    #[error("Wheel package name does not match filename: {0} != {1}")]
    MismatchedName(PackageName, PackageName),
    #[error("Wheel version does not match filename: {0} != {1}")]
    MismatchedVersion(Version, Version),
}

/// Returns `true` if the file is a `METADATA` file in a `dist-info` directory that matches the
/// wheel filename.
pub fn is_metadata_entry(path: &str, filename: &WheelFilename) -> bool {
    let Some((dist_info_dir, file)) = path.split_once('/') else {
        return false;
    };
    if file != "METADATA" {
        return false;
    }
    let Some(dir_stem) = dist_info_dir.strip_suffix(".dist-info") else {
        return false;
    };
    let Some((name, version)) = dir_stem.rsplit_once('-') else {
        return false;
    };
    let Ok(name) = PackageName::from_str(name) else {
        return false;
    };
    if name != filename.name {
        return false;
    }
    let Ok(version) = Version::from_str(version) else {
        return false;
    };
    if version != filename.version {
        return false;
    }
    true
}

/// Find the `dist-info` directory from a list of files.
///
/// The metadata name may be uppercase, while the wheel and dist info names are lowercase, or
/// the metadata name and the dist info name are lowercase, while the wheel name is uppercase.
/// Either way, we just search the wheel for the name.
///
/// Returns the dist info dir prefix without the `.dist-info` extension.
///
/// Reference implementation: <https://github.com/pypa/packaging/blob/2f83540272e79e3fe1f5d42abae8df0c14ddf4c2/src/packaging/utils.py#L146-L172>
pub fn find_dist_info<'a, T: Copy>(
    filename: &WheelFilename,
    files: impl Iterator<Item = (T, &'a str)>,
) -> Result<(T, &'a str), Error> {
    let metadatas: Vec<_> = files
        .filter_map(|(payload, path)| {
            let (dist_info_dir, file) = path.split_once('/')?;
            if file != "METADATA" {
                return None;
            }

            let dir_stem = dist_info_dir.strip_suffix(".dist-info")?;
            let (name, version) = dir_stem.rsplit_once('-')?;
            if PackageName::from_str(name).ok()? != filename.name {
                return None;
            }

            if Version::from_str(version).ok()? != filename.version {
                return None;
            }

            Some((payload, dir_stem))
        })
        .collect();
    let (payload, dist_info_prefix) = match metadatas[..] {
        [] => {
            return Err(Error::MissingDistInfo);
        }
        [(payload, path)] => (payload, path),
        _ => {
            return Err(Error::MultipleDistInfo(
                metadatas
                    .into_iter()
                    .map(|(_, dist_info_dir)| dist_info_dir.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
    };
    Ok((payload, dist_info_prefix))
}

/// Given an archive, read the `dist-info` metadata into a buffer.
pub fn read_dist_info(
    filename: &WheelFilename,
    archive: &mut ZipArchive<impl Read + Seek + Sized>,
) -> Result<Vec<u8>, Error> {
    let dist_info_prefix =
        find_dist_info(filename, archive.file_names().map(|name| (name, name)))?.1;

    let mut file = archive
        .by_name(&format!("{dist_info_prefix}.dist-info/METADATA"))
        .map_err(|err| Error::Zip(filename.to_string(), err))?;

    #[allow(clippy::cast_possible_truncation)]
    let mut buffer = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buffer)?;

    Ok(buffer)
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use distribution_filename::WheelFilename;

    use crate::find_dist_info;

    #[test]
    fn test_dot_in_name() {
        let files = [
            "mastodon/Mastodon.py",
            "mastodon/__init__.py",
            "mastodon/streaming.py",
            "Mastodon.py-1.5.1.dist-info/DESCRIPTION.rst",
            "Mastodon.py-1.5.1.dist-info/metadata.json",
            "Mastodon.py-1.5.1.dist-info/top_level.txt",
            "Mastodon.py-1.5.1.dist-info/WHEEL",
            "Mastodon.py-1.5.1.dist-info/METADATA",
            "Mastodon.py-1.5.1.dist-info/RECORD",
        ];
        let filename = WheelFilename::from_str("Mastodon.py-1.5.1-py2.py3-none-any.whl").unwrap();
        let (_, dist_info_prefix) =
            find_dist_info(&filename, files.into_iter().map(|file| (file, file))).unwrap();
        assert_eq!(dist_info_prefix, "Mastodon.py-1.5.1");
    }
}
