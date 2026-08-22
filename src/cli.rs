//! Turns argv into an intent, before anything touches the filesystem.
//!
//! Keeping the two apart is what lets `mhr --version` answer the question
//! instead of trying to open a file with that name, and it makes [`parse`]
//! testable without a disk.

use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub const USAGE: &str = "\
mhr, a read-only viewer for GitHub-flavoured markdown. It opens a window and
re-renders whenever the file changes on disk. It never writes to the file.

Usage:
  mhr <file.md>

Options:
  -h, --help     Print this message
  -V, --version  Print the version";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Open this argument. It is an `OsString` rather than a `String` because a
    /// path is whatever bytes the operating system accepts, not necessarily
    /// UTF-8, and refusing to open a file over its encoding would be absurd.
    Open(OsString),
    Help,
    Version,
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Request> {
    let mut args = args.into_iter();

    let Some(arg) = args.next() else {
        bail!("no file given\n\n{USAGE}");
    };

    // Only the argument's flag-ness is a UTF-8 question. `to_str` returning
    // None means it cannot be any of the options below, so it falls through to
    // being treated as a path.
    match arg.to_str() {
        Some("-h" | "--help") => return Ok(Request::Help),
        Some("-V" | "--version") => return Ok(Request::Version),
        // A file may legally be named `-weird.md`, but a leading dash reads as
        // an option to every person and every shell, so it is refused with a
        // way out rather than guessed at.
        Some(other) if other.starts_with('-') => {
            bail!("unknown option {other}, use ./{other} to open a file by that name\n\n{USAGE}");
        }
        _ => {}
    }

    if args.next().is_some() {
        bail!("mhr opens one file at a time\n\n{USAGE}");
    }

    Ok(Request::Open(arg))
}

/// Resolves an opened argument to a path worth watching.
///
/// Canonicalising here rather than at the point of use matters for more than
/// tidiness: `watch` derives the directory it watches from this path, and a
/// relative path would make that directory depend on the working directory at
/// the time of each call.
pub fn open(arg: &OsStr) -> Result<PathBuf> {
    let path = std::fs::canonicalize(arg)
        .with_context(|| format!("cannot open {}", PathBuf::from(arg).display()))?;

    // Without this, a directory argument reaches the window as a "Cannot read:
    // Is a directory" notice, which answers a question nobody asked.
    if path.is_dir() {
        bail!("{} is a directory, not a markdown file", path.display());
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Request, open, parse};
    use std::ffi::OsString;

    fn parse_args(args: &[&str]) -> super::Result<Request> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn reads_a_file_argument() {
        assert_eq!(
            parse_args(&["notes.md"]).unwrap(),
            Request::Open(OsString::from("notes.md"))
        );
    }

    #[test]
    fn reads_both_spellings_of_each_option() {
        assert_eq!(parse_args(&["-h"]).unwrap(), Request::Help);
        assert_eq!(parse_args(&["--help"]).unwrap(), Request::Help);
        assert_eq!(parse_args(&["-V"]).unwrap(), Request::Version);
        assert_eq!(parse_args(&["--version"]).unwrap(), Request::Version);
    }

    /// The whole reason this module exists: these used to be read as filenames.
    #[test]
    fn does_not_treat_an_option_as_a_filename() {
        let error = format!("{:#}", parse_args(&["-x"]).unwrap_err());
        assert!(error.contains("unknown option -x"), "{error}");
        assert!(error.contains("./-x"), "no way out offered: {error}");
    }

    #[test]
    fn refuses_an_empty_or_crowded_command_line() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["a.md", "b.md"]).is_err());
    }

    /// A path is bytes, not text, so an argument that is not UTF-8 is still a
    /// file to open rather than an error to report.
    #[cfg(unix)]
    #[test]
    fn accepts_a_filename_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let arg = OsString::from_vec(vec![b'a', 0xff, b'.', b'm', b'd']);
        assert_eq!(parse([arg.clone()]).unwrap(), Request::Open(arg));
    }

    #[test]
    fn rejects_a_directory_before_a_window_opens() {
        let error = format!("{:#}", open("fixtures".as_ref()).unwrap_err());
        assert!(error.contains("is a directory"), "{error}");
    }

    #[test]
    fn resolves_a_real_file_to_an_absolute_path() {
        let path =
            open("fixtures/kitchen-sink.md".as_ref()).expect("the fixture ships with the repo");
        assert!(path.is_absolute(), "{}", path.display());
        assert!(path.ends_with("kitchen-sink.md"), "{}", path.display());
    }

    #[test]
    fn reports_the_name_it_could_not_open() {
        let error = format!("{:#}", open("no-such-file.md".as_ref()).unwrap_err());
        assert!(error.contains("no-such-file.md"), "{error}");
    }
}
