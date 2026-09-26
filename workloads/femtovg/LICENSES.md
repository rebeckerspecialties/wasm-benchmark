# femtovg E2E scenes: provenance and licenses

Both files are used unmodified as benchmark input. The guest
(`workloads-rs-cargo/femtovg-guest`) embeds them with `include_bytes!`.

| scene | file | source | license |
|---|---|---|---|
| 0 | `Ghostscript_Tiger.svg` (68 629 B, sha256 `eab0c137…5035`) | femtovg's own SVG example asset, `examples/assets/Ghostscript_Tiger.svg` in [femtovg](https://github.com/femtovg/femtovg), last changed in femtovg commit `1c1cb0b7` (2020-01-23) | femtovg ships it without a separate license notice (femtovg itself is MIT OR Apache-2.0). The artwork is the tiger from Ghostscript's example files (`examples/tiger.eps`), which Ghostscript distributes under the GNU AGPL v3. Check that term before redistributing this repository outside its current use. |
| 1 | `combined-linking.svg` (198 460 B, sha256 `20434c94…dbea`) | WebAssembly component-model design repository, `design/mvp/examples/images/combined-linking.svg`, last changed in commit `79817785` (2024-07-01); vendored in this repo's `wasmtime` submodule at `tests/component-model` (`7c676115`) | Apache-2.0: the repository's `LICENSE` puts every file under Apache 2.0 unless a subdirectory says otherwise, and this one's does not. |

Scene 1 is the heavier real-world file: a published diagram whose text was
exported as glyph outlines, so it is 190 paths with far more curve segments
than the tiger. Its root group is clipped (`clip-path`), which exercises
femtovg's clip-path commands (`ClipFill` / `ClipReset`) end to end.
