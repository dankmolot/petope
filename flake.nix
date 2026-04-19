{
  description = "petope dev env";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
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
            ]
          )
        );
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer

            # LaTeX
            tex
          ];
        };
      }
    );
}
