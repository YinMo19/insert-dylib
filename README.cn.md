# insert-dylib（Rust 版）

这是 `insert_dylib` 的 Rust 重写版本，用于向 Mach-O 二进制注入新的 `LC_LOAD_DYLIB`（或 `LC_LOAD_WEAK_DYLIB`）load command。

> 该工具会直接修改二进制文件。请务必先备份，并在分发前自行验证结果。

## 平台支持

- 仅支持：**macOS aarch64（Apple Silicon）**
- 其它目标会在编译期直接报错（设计如此）。

## 功能

- 注入 `LC_LOAD_DYLIB` 或 `LC_LOAD_WEAK_DYLIB`
- 支持 thin Mach-O 与 fat binary
- 支持原地修改（in-place）或输出到新文件
- 支持可选移除代码签名 load command（并尝试调整相关 `__LINKEDIT` 信息）
- 交互式安全确认，支持 `--all-yes` 无人值守

## 构建

```bash
cargo build --release --target aarch64-apple-darwin
```

生成的可执行文件位于：

- `target/aarch64-apple-darwin/release/insert-dylib`

## 用法

```bash
insert-dylib [options] <dylib_path> <binary_path> [new_binary_path]
```

若未提供 `new_binary_path`（且未使用 `--inplace`），默认输出为：

- `<binary_path>_patched`

### 选项

- `--inplace` 直接修改输入文件
- `--weak` 使用 `LC_LOAD_WEAK_DYLIB`，而不是 `LC_LOAD_DYLIB`
- `--overwrite` 允许无提示覆盖输出文件
- `--strip-codesig` 在可行时强制移除 `LC_CODE_SIGNATURE`
- `--no-strip-codesig` 不移除 `LC_CODE_SIGNATURE`
- `--all-yes` 对所有交互提示自动回答 `yes`
- `--ios` 将 dylib 的 Mach-O 平台字段从 `macOS` 改为 `iOS`（必须配合 `--dylib-path`）
- `--dylib-path <path>` 配合 `--ios` 使用的本地 dylib 文件路径，用于平台字段改写

## 示例

更多案例见 [`examples/`](./examples/README.md)。

注入 dylib，输出到默认文件：

```bash
insert-dylib @executable_path/libHook.dylib MyApp
```

使用弱依赖并原地修改：

```bash
insert-dylib --weak --inplace @rpath/libHook.dylib MyApp
```

强制剥离代码签名并覆盖输出：

```bash
insert-dylib --strip-codesig --overwrite @loader_path/libHook.dylib MyApp MyApp.patched
```

写入 iOS install name，并对本地 dylib 文件执行平台改写：

```bash
insert-dylib --ios --dylib-path libarcaea_function.dylib @executable_path/Frameworks/libarcaea_function.dylib Arc-mobile
```

## 代码签名说明

若移除了 `LC_CODE_SIGNATURE`，原签名会失效。需要的话请对补丁后的文件重新签名：

```bash
codesign --force --sign - MyApp.patched
```

## 法律与安全

请仅对你有权限修改的二进制使用本工具。
