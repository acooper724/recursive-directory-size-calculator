# recursive-directory-size-calculator

Walks a directory tree and returns the total byte count and file count, following symlinks while guarding against cycles.

```rust
use recursive_directory_size_calculator::calculate;

fn main() -> std::io::Result<()> {
    let stats = calculate(".")?;
    println!("{} bytes in {} files", stats.total_bytes, stats.file_count);
    Ok(())
}
```

The public API is a single function `calculate` and a struct `DirectoryStats` with fields `total_bytes: u64` and `file_count: u64`.

## Why this exists

Recursively sizing a directory looks trivial until symlinks are involved. A naive walk that follows symlinks will loop forever on a cycle (a directory containing a symlink back to an ancestor). A walk that does not follow symlinks silently under-reports for the common case of a symlinked subdirectory that the user expects to be included.

This library takes the middle path: it follows symlinks, but canonicalises each directory before descending and keeps a set of visited canonical paths. A canonical path is visited at most once, so cycles terminate and duplicate views of the same directory are not double-counted. The trade-off is that a directory reachable by two distinct symlink paths is counted once, not twice, which matches the intuition of "how many bytes live under this tree" rather than "how many bytes would `find -L` print".

## Edge cases

- **Broken symlinks** are skipped entirely; they contribute neither bytes nor a file count.
- **Symlinks to files** are followed and counted like regular files.
- **Unreadable entries** are skipped so a single permission error does not abort the walk. The root itself must be a readable directory, otherwise `calculate` returns an `io::Error` with kind `InvalidInput` when the root is not a directory.
- **Hard links** are counted once per directory entry, matching `du` semantics.

The walk is single-threaded and uses only the standard library.
