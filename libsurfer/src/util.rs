//! Utility functions.
use crate::displayed_item_tree::VisibleItemIndex;
use camino::Utf8PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// This function takes a number and converts it's digits into the range
/// a-p. This is nice because it makes for some easily typed ids.
/// The function first formats the number as a hex digit and then performs
/// the mapping.
pub fn uint_idx_to_alpha_idx(idx: VisibleItemIndex, nvariables: usize) -> String {
    // this calculates how many hex digits we need to represent nvariables
    // unwrap because the result should always fit into usize and because
    // we are not going to display millions of character ids.
    let width = usize::try_from(nvariables.ilog(16)).unwrap() + 1;
    format!("{:0width$x}", idx.0)
        .chars()
        .map(|c| match c {
            '0' => 'a',
            '1' => 'b',
            '2' => 'c',
            '3' => 'd',
            '4' => 'e',
            '5' => 'f',
            '6' => 'g',
            '7' => 'h',
            '8' => 'i',
            '9' => 'j',
            'a' => 'k',
            'b' => 'l',
            'c' => 'm',
            'd' => 'n',
            'e' => 'o',
            'f' => 'p',
            _ => '?',
        })
        .collect()
}

/// This is the reverse function to uint_idx_to_alpha_idx.
pub fn alpha_idx_to_uint_idx(idx: String) -> Option<VisibleItemIndex> {
    let mapped = idx
        .chars()
        .map(|c| match c {
            'a' => '0',
            'b' => '1',
            'c' => '2',
            'd' => '3',
            'e' => '4',
            'f' => '5',
            'g' => '6',
            'h' => '7',
            'i' => '8',
            'j' => '9',
            'k' => 'a',
            'l' => 'b',
            'm' => 'c',
            'n' => 'd',
            'o' => 'e',
            'p' => 'f',
            _ => '?',
        })
        .collect::<String>();
    usize::from_str_radix(&mapped, 16)
        .ok()
        .map(VisibleItemIndex)
}

/// This function searches upward from `start` for directories or files matching `item`. It returns
/// a `Vec<PathBuf>` to all found instances in order of closest to furthest away. The function only
/// searches up within subdirectories of `end`.
#[cfg(not(target_arch = "wasm32"))]
pub fn search_upward(
    start: impl AsRef<Path>,
    end: impl AsRef<Path>,
    item: impl AsRef<Path>,
) -> Vec<PathBuf> {
    start
        .as_ref()
        .ancestors()
        .take_while(|p| p.starts_with(end.as_ref()))
        .map(|p| p.join(&item))
        .filter(|p| p.try_exists().is_ok_and(std::convert::identity))
        .collect()
}

pub fn get_multi_extension_from_filename(filename: &str) -> Option<String> {
    filename
        .as_bytes()
        .iter()
        .position(|&c| c == b'.')
        .map(|pos| {
            let iter = filename.chars().skip(pos + 1);
            iter.collect::<String>()
        })
}

/// Get the full extension of a path, including all extensions.
/// For example, for "foo.tar.gz", this function returns "tar.gz", and not just "gz",
/// like path.extension() would.
pub fn get_multi_extension(path: &Utf8PathBuf) -> Option<String> {
    // Find the first . in the path, if any. Return the rest of the path.
    if let Some(filename) = path.file_name() {
        return get_multi_extension_from_filename(filename);
    }
    None
}
