use std::path::{Component, Path};

fn normalize_path(path: &str) -> String {
    let s = path.replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    let s = s.strip_prefix('/').unwrap_or(s);
    s.to_string()
}

fn main() {
    let paths = vec![
        "foo/bar",
        "./foo/bar",
        "/foo/bar",
        "foo//bar",
        "foo/./bar",
        "foo/../bar",
    ];
    for p in paths {
        println!("'{}' -> '{}'", p, normalize_path(p));
    }
}
