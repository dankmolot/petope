{
  description = "petope dev env";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        tex = (
          pkgs.texliveSmall.withPackages (
            ps: with ps; [
              # https://github.com/James-Yu/LaTeX-Workshop/wiki/Install#installation
              latexmk # making files from latex
              chktex # linting
              latexindent # formatting
              xurl # \url line breaking
              minted # for code highlighting
              tcolorbox # for blockquotes
              helvetic # for Helvetica font
              inconsolata # as monoscape font
              titlesec # For modifying titles
            ]
          )
        );
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            # Rust
            rust
            pkgs.rust-analyzer

            # LaTeX
            tex
          ];
        };
      }
    );
}
