use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// The Finder icon of a file or app bundle.
    File(PathBuf),
    /// An SF Symbol drawn on a colored tile.
    Symbol { name: &'static str, tint: Tint },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tint {
    Blue,
    Red,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Open(PathBuf),
    OpenUrl(String),
    Quit,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub title: String,
    pub kind: &'static str,
    pub icon: Icon,
    pub action: Action,
}

const APP_DIRS: &[&str] = &[
    "/Applications",
    "/Applications/Utilities",
    "/System/Applications",
    "/System/Applications/Utilities",
];

/// Apps that live outside the usual Applications folders.
const EXTRA_APPS: &[&str] = &["/System/Library/CoreServices/Finder.app"];

/// Scans the installed apps and adds the built-in commands.
pub fn load() -> Vec<Entry> {
    let home_apps = std::env::var_os("HOME").map(|home| Path::new(&home).join("Applications"));
    let mut bundles: Vec<PathBuf> = APP_DIRS
        .iter()
        .map(PathBuf::from)
        .chain(home_apps)
        .flat_map(|dir| app_bundles_in(&dir))
        .collect();
    bundles.extend(
        EXTRA_APPS
            .iter()
            .map(PathBuf::from)
            .filter(|path| path.exists()),
    );

    let mut seen = HashSet::new();
    let mut entries: Vec<Entry> = bundles
        .into_iter()
        .filter_map(|path| {
            let title = path.file_stem()?.to_str()?.to_owned();
            seen.insert(title.clone()).then(|| Entry {
                title,
                kind: "Application",
                icon: Icon::File(path.clone()),
                action: Action::Open(path),
            })
        })
        .collect();

    entries.push(Entry {
        title: "Quit Rust Overlay".to_owned(),
        kind: "Command",
        icon: Icon::Symbol {
            name: "power",
            tint: Tint::Red,
        },
        action: Action::Quit,
    });
    entries
}

fn app_bundles_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read_dir
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .collect()
}

/// Returns the entries matching `query`, best match first, followed by a web search for it.
pub fn search(entries: &[Entry], query: &str) -> Vec<Entry> {
    let query = query.trim();
    let mut matches: Vec<(u32, &Entry)> = entries
        .iter()
        .filter_map(|entry| Some((score(query, &entry.title)?, entry)))
        .collect();
    matches.sort_by_cached_key(|(score, entry)| (Reverse(*score), entry.title.to_lowercase()));

    let mut results: Vec<Entry> = matches
        .into_iter()
        .map(|(_, entry)| entry.clone())
        .collect();
    if !query.is_empty() {
        results.push(web_search(query));
    }
    results
}

/// Ranks how well `candidate` matches `query`, or `None` if it doesn't match at all.
pub fn score(query: &str, candidate: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let query = query.to_lowercase();
    let candidate = candidate.to_lowercase();
    let words: Vec<&str> = candidate
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let initials: String = words
        .iter()
        .filter_map(|word| word.chars().next())
        .collect();

    if candidate.starts_with(&query) {
        Some(400)
    } else if query.chars().count() > 1 && initials.starts_with(&query) {
        Some(300)
    } else if words.iter().any(|word| word.starts_with(&query)) {
        Some(200)
    } else if candidate.contains(&query) {
        Some(100)
    } else if is_subsequence(&query, &candidate) {
        Some(50)
    } else {
        None
    }
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut haystack = haystack.chars();
    needle.chars().all(|c| haystack.any(|h| h == c))
}

fn web_search(query: &str) -> Entry {
    Entry {
        title: format!("Search Google for “{query}”"),
        kind: "Web Search",
        icon: Icon::Symbol {
            name: "magnifyingglass",
            tint: Tint::Blue,
        },
        action: Action::OpenUrl(format!(
            "https://www.google.com/search?q={}",
            percent_encode(query)
        )),
    }
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(title: &str) -> Entry {
        Entry {
            title: title.to_owned(),
            kind: "Application",
            icon: Icon::File(PathBuf::from(format!("/Applications/{title}.app"))),
            action: Action::Open(PathBuf::from(format!("/Applications/{title}.app"))),
        }
    }

    fn titles(results: &[Entry]) -> Vec<&str> {
        results.iter().map(|entry| entry.title.as_str()).collect()
    }

    #[test]
    fn ranks_prefix_then_initials_then_word_then_substring_then_subsequence() {
        assert_eq!(score("saf", "Safari"), Some(400));
        assert_eq!(score("vsc", "Visual Studio Code"), Some(300));
        assert_eq!(score("stu", "Visual Studio Code"), Some(200));
        assert_eq!(score("ari", "Safari"), Some(100));
        assert_eq!(score("sfr", "Safari"), Some(50));
        assert_eq!(score("xyz", "Safari"), None);
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(score("SAF", "safari"), Some(400));
    }

    #[test]
    fn empty_query_lists_everything_alphabetically_without_web_search() {
        let entries = [app("Xcode"), app("calendar"), app("Books")];
        assert_eq!(
            titles(&search(&entries, "  ")),
            ["Books", "calendar", "Xcode"]
        );
    }

    #[test]
    fn best_matches_come_first_and_web_search_comes_last() {
        let entries = [app("Xcode"), app("Code"), app("Calendar")];
        assert_eq!(
            titles(&search(&entries, "code")),
            ["Code", "Xcode", "Search Google for “code”"]
        );
    }

    #[test]
    fn web_search_url_is_percent_encoded() {
        let results = search(&[], "rust & swift?");
        assert_eq!(
            results[0].action,
            Action::OpenUrl("https://www.google.com/search?q=rust%20%26%20swift%3F".to_owned())
        );
    }
}
