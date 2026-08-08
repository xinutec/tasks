//! What is served for a path that is not the API.
//!
//! ⚠ **The wrong answer here is a `200`.** A SPA fallback that hands
//! `index.html` to a request for `main-ABC123.js` or a `.woff2` is not an error
//! anywhere: the browser gets HTML where it asked for a font, renders broken
//! icons, and reports nothing. It was measured on this app's own live
//! deployment before it was fixed, and memview's console shipped it too.

use tasks::routes::spa;

/// The bytes do not matter — only which branch was taken — so the index is a
/// path that cannot be read. A 500 therefore means "tried to serve the page".
const NO_INDEX: &str = "/nonexistent/index.html";

fn status(path: &str) -> u16 {
    spa(NO_INDEX, path).status().as_u16()
}

#[test]
fn a_path_that_names_a_file_is_not_found() {
    for path in [
        "/main-ABC123.js",
        "/styles-7HENECVC.css",
        "/media/roboto-latin-400.woff2",
        "/icon.svg",
        "/favicon.ico",
    ] {
        assert_eq!(status(path), 404, "{path} fell back to the page");
    }
}

#[test]
fn a_route_gets_the_page() {
    // 500 rather than 200 because the fixture index cannot be read; what is
    // being asserted is that it TRIED, which is the other branch.
    for path in ["/", "/new", "/t/1", "/t/12345"] {
        assert_eq!(status(path), 500, "{path} was refused as a file");
    }
}

#[test]
fn the_dot_that_counts_is_in_the_last_segment() {
    // A route under a directory with a dot in its name is still a route, and a
    // file at the root is still a file.
    assert_eq!(status("/v1.2/settings"), 500);
    assert_eq!(status("/t/1/notes.md"), 404);
}
