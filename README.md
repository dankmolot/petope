## Rust development
[Rust Analyzer] wont work from a get go in Nix environment. You need also to specify `"rust-analyzer.server.path": "rust-analyzer"` in your VSCode `settings.json` for it to use not bundled one.

## LaTeX development
[TexLive] is used as tool provider, and [LaTeX Workshop] extension for VSCode.

Check out [this guide](https://paulwintz.com/latex-in-vscode/) by Paul Wintz to configure [LaTeX Workshop]

**NB!** Configure out dir for LaTeX artifacts to `latex_dist`

[TexLive]: https://wiki.nixos.org/wiki/TexLive
[LaTex Workshop]: https://github.com/James-Yu/LaTeX-Workshop
[Rust Analyzer]: https://rust-analyzer.github.io/