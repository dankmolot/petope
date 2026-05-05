## Config format
By default `petope` loads a config file `config.toml`
(which can be customized by providing `-c`/`--config` flag)
and generates a private key if needed.

Here is how config looks:
```toml
# you shall keep this value in secret!
private_key = "base64 encoded 32 bytes of 25519 private key"

# You can add as many peers as you want
[[peer]]
id = "z32 encoded public key, can be seen after launching petope"

[[peer]]
id = "fe3nwzmjph4s7k4enggeygeqsus46se9nffbarndui6amsms688o"
```

## Rust development
[Rust Analyzer] wont work from a get go in Nix environment. You need also to specify `"rust-analyzer.server.path": "rust-analyzer"` in your VSCode `settings.json` for it to use not bundled one.

## LaTeX development
[TexLive] is used as tool provider, and [LaTeX Workshop] extension for VSCode.

Check out [this guide](https://paulwintz.com/latex-in-vscode/) by Paul Wintz to configure [LaTeX Workshop]

**NB!** Configure out dir for LaTeX artifacts to `latex_dist`

[TexLive]: https://wiki.nixos.org/wiki/TexLive
[LaTex Workshop]: https://github.com/James-Yu/LaTeX-Workshop
[Rust Analyzer]: https://rust-analyzer.github.io/
