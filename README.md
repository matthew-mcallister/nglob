# nglob

nglob is a Rust library for file globbing. It supports synchronous and parallel
async directory search, string matching, filetype filters, and both Windows and
Unix-like filesystems. It is based on a bespoke search algorithm using tries
and finite automata which handles complex patterns while having less surprising
behavior than standard shell glob. The API is also intentionally simple to use.

## Installation

To add the latest version to your project, run `cargo add nglob`. nglob is
compatible with stable rustc.

## Cargo features

To enable all features, change the dependency in your `Cargo.toml` to this:

```
nglob = { version = "...", features = ["tokio"] }
```

Substitute the desired value for the version number.

List of features:

- `tokio`: Enables parallel search with the `nglob::walker::tokio` module.

## Example

```rust
use nglob::GlobConfig;
use nglob::walker::sync::glob;

let config = GlobConfig::default();
let pattern = Pattern::compile("src/**.rs").unwrap();
let output = glob(&config, &pattern);
for result in output.results() {
    match result {
        Ok(e) => println!("{}", e.path),
        Err(e) => println!("error: {}", e),
    }
}
```

See crate documentation for more detailed usage.

## Limitations

- UTF-8 only currently
- glob patterns only, no regex
- Character classes not supported currently
- Can't search multiple drives in parallel on Windows
- String filepath matching semantics depend on host operating system
