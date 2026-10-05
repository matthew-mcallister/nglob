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

## Usage

### Configuration

### Choosing a search method

Which search method you should use depends on your use case:

- `nglob::walker::sync::glob`: Synchronous, single-threaded directory search.
- `nglob::walker::tokio::glob`: Requires the `tokio` feature. Parallel,
  asynchronous search based on tokio.
- `nglob::walker::string::glob`: Matches against a list of file path strings
  without interacting with the filesystem. This is ideal for searching
  filepaths inside applications that deal with archives, HTTP directories,
  sandboxes, and the like.

### Synchronous matching

Here's an example of synchronous directory matching.

```rust
use nglob::GlobConfig;
use nglob::walker::sync::glob;

let config = GlobConfig::default();
let pattern =
let result = glob(&config, "", );
```

### Asynchronous matching

The asynchronous matcher in

If you are already running async code, the usage is

```rust

```

### String matching

nglob can do matching against bare filepaths without touching the filesystem.
Currently, paths are treated according to the same semantics as the host
operating system, so be aware of minute differences.

### Error handling

Each

## Pattern syntax

###

## Limitations

- UTF-8 only currently
- glob patterns only, no regex
- Character classes not supported currently
- Can't search multiple drives in parallel on Windows
- String filepath matching semantics depend on host operating system
