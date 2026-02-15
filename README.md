# insert-dylib (Rust)

A Rust rewrite of `insert_dylib`: injects a new `LC_LOAD_DYLIB` (or `LC_LOAD_WEAK_DYLIB`) load command into Mach-O binaries.

> This tool modifies binary files directly. Always keep backups and validate outputs before distribution.

## Platform Support

- Supported platform: **macOS aarch64 (Apple Silicon)**
- Unsupported targets fail at compile time by design.

## Features

- Inject `LC_LOAD_DYLIB` or `LC_LOAD_WEAK_DYLIB`
- Works with thin Mach-O and fat binaries
- Optional in-place patching or writing to a new output file
- Optional code-signature load command stripping (with related `__LINKEDIT` adjustments)
- Interactive safety prompts, with `--all-yes` for non-interactive runs

## Build

```bash
cargo build --release --target aarch64-apple-darwin
```

The binary will be at:

- `target/aarch64-apple-darwin/release/insert-dylib`

## Usage

```bash
insert-dylib [options] <dylib_path> <binary_path> [new_binary_path]
```

If `new_binary_path` is omitted (and `--inplace` is not used), output defaults to:

- `<binary_path>_patched`

### Options

- `--inplace` patch the input binary in place
- `--weak` use `LC_LOAD_WEAK_DYLIB` instead of `LC_LOAD_DYLIB`
- `--overwrite` allow overwriting output without prompt
- `--strip-codesig` force removing `LC_CODE_SIGNATURE` when possible
- `--no-strip-codesig` never remove `LC_CODE_SIGNATURE`
- `--all-yes` auto-answer `yes` to all interactive prompts
- `--ios` rewrite dylib Mach-O platform markers from `macOS` to `iOS` (requires `--dylib-path`)
- `--dylib-path <path>` local dylib file path used by `--ios` for Mach-O platform rewrite

## Examples

More case studies are in [`examples/`](./examples/README.md).

Inject a dylib and write to default output:

```bash
insert-dylib @executable_path/libHook.dylib MyApp
```

Inject weak dylib in place:

```bash
insert-dylib --weak --inplace @rpath/libHook.dylib MyApp
```

Force code-signature stripping and overwrite output:

```bash
insert-dylib --strip-codesig --overwrite @loader_path/libHook.dylib MyApp MyApp.patched
```

Inject iOS install name while rewriting a local dylib file to iOS platform metadata:

```bash
insert-dylib --ios --dylib-path libarcaea_function.dylib @executable_path/Frameworks/libarcaea_function.dylib Arc-mobile
```

## Code Signing Note

If `LC_CODE_SIGNATURE` is removed, the binary's signature is invalidated. Re-sign the patched binary if needed:

```bash
codesign --force --sign - MyApp.patched
```

## Legal / Safety

Use this tool only on binaries you are authorized to modify.
