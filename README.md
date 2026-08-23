# PRC Editor (Rust)

A Windows desktop editor for Smash Ultimate parameter files (`.prc`, `.prcx`, `.stdat`, and related formats), written in Rust with [egui](https://github.com/emilk/egui).

## Credits

This project is a Rust rewrite that **heavily uses the original PRC editor** as its reference for file format, UI layout, and editing workflow.

- Original library and **prcEditor**: [BenHall-7/paracobNET](https://github.com/BenHall-7/paracobNET) by Benjamin Hall
- Parameter labels: [ultimate-research/param-labels](https://github.com/ultimate-research/param-labels)
- This Rust version was built with the use of AI (Cursor)

The original `prcEditor` is the Windows C# TreeView / DataGrid tool this version is based on. Hash40 parsing, label handling, and most of the editor behavior were modeled directly after that project. This repo would not exist without that work.

## Features

- Tree navigation of parameter structs and lists
- Field table with Hash40 autocomplete from `ParamLabels.csv`
- Checkbox editing for boolean fields
- Label editor with search and pagination
- Add child items to the selected category (for example a new entry under `flip_bones`)
- Undo / redo, copy / paste, and in-place save

## Download

Grab the Windows executable from [Releases](https://github.com/CrusherD2/prc-editor-rust/releases).

You also need [ParamLabels.csv](https://github.com/ultimate-research/param-labels) for readable names. The editor will prompt you to load it.

## Build from source

Requires a recent Rust toolchain.

```bash
git clone https://github.com/CrusherD2/prc-editor-rust.git
cd prc-editor-rust
cargo build --release
```

The binary is written to `target/release/prc-editor-rust.exe`.

## Usage

1. Load `ParamLabels.csv` when prompted (or from the Labels menu).
2. Open a `.prc` / `.prcx` / `.stdat` file from **File > Open**.
3. Browse the parameter tree on the left and edit fields on the right.

## License

MIT, same as [paracobNET](https://github.com/BenHall-7/paracobNET). See [LICENSE](LICENSE).
