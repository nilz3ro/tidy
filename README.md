# tidy 🧹

## Summary

A CLI tool to automatically clean up cluttered directories on your filesystem.
`tidy` is built with [Tokio](https://tokio.rs) tasks and async filesystem operations.

This allows `tidy` to start sorting files before all of the files have even been enumerated in the source directory (the directory to tidy). Some files will be moved to their destination before others have even been found in the source directory. This architecture makes it possible for `tidy` to organize thousands of files very quickly and with a very low memory overhead.

(benchmarks and resource consumption analysis coming soon)

## Todos

- improved cli
- config system
- sorting strategies - preset and custom
- watch mode
