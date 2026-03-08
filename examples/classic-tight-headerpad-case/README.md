# Classic `calc` Injection Case (Tight Headerpad)

This folder is a reproducible failure/success comparison for Mach-O dylib injection:

- The target binary (`calc`) has very limited header padding (about 32 bytes).
- Injecting a long dylib path can corrupt the binary after re-signing.
- Injecting a short dylib path with the right flags can persist the hook safely.

## Directory Layout

- `calc/calc.c`: target program source
- `hook/`: Rust hook payload project (`sighook` based)

## Prerequisites

- This example itself requires macOS aarch64 (Apple Silicon)
- `codesign` available in your environment

## Build and Run (Step-by-Step)

Run from repository root:

```bash
# 1) Build insert-dylib
cargo build --release

# 2) Enter this example folder
cd examples/classic-tight-headerpad-case

# 3) Build the target binary
cc -O2 -fno-inline calc/calc.c -o calc_bin

# 4) Build the hook dylib
cargo build --release --manifest-path hook/Cargo.toml
```

### Baseline Check (`DYLD_INSERT_LIBRARIES`)

```bash
DYLD_INSERT_LIBRARIES="$PWD/hook/target/release/libclassic_hook.dylib" ./calc_bin
```

Expected output:

```text
[+] hooked: now x0 is 99.
Result: 99
```

## Failure Repro (Long Dylib Name)

Use a longer filename such as `libh.dylib`:

```bash
cp hook/target/release/libclassic_hook.dylib libh.dylib
../../target/release/insert-dylib libh.dylib calc_bin calc_bad
codesign -f -s - ./calc_bad
./calc_bad
```

Common symptom:

- Process exits unexpectedly and does not print the expected final result line.

## Success Repro (Short Dylib Name)

Use a short filename such as `h.dylib`:

```bash
cp hook/target/release/libclassic_hook.dylib h.dylib
../../target/release/insert-dylib --no-strip-codesig h.dylib calc_bin calc_ok
codesign -f -s - ./calc_ok
./calc_ok
```

Expected output:

```text
[+] hooked: now x0 is 99.
Result: 99
```

## Why This Happens

For `LC_LOAD_DYLIB`, command size is:

- `cmdsize = 24 + align8(path_len + 1)`

Where:

- `24` is the fixed size of `struct dylib_command`
- `path_len + 1` includes the trailing `\0`

In this case:

- available header space is about 32 bytes
- `libh.dylib` needs 40 bytes (overflow)
- `h.dylib` needs 32 bytes (fits exactly)

That is why long path injection fails while short path injection succeeds.

## Practical Notes

- When header padding is tight, prefer short install names.
- In this case, prefer `--no-strip-codesig`.
- If you can rebuild the target, add `-Wl,-headerpad_max_install_names`.
