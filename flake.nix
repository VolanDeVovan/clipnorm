{
  description = "Watch the Wayland clipboard, save image-only entries to disk, republish multi-MIME so the path pastes into TUI agents like Claude Code";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (system: pkgs: {
        default = self.packages.${system}.clipnorm;

        clipnorm = pkgs.rustPlatform.buildRustPackage {
          pname = "clipnorm";
          version = "0.1.0";

          src = pkgs.lib.cleanSource ./.;

          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            description = "Wayland clipboard image normalizer for TUI AI agents (Claude Code, Codex, Aider, …)";
            homepage = "https://github.com/VolanDeVovan/clipnorm";
            license = pkgs.lib.licenses.mit;
            mainProgram = "clipnorm";
            platforms = pkgs.lib.platforms.linux;
          };
        };
      });

      devShells = forAllSystems (
        _: pkgs: {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt
              wl-clipboard
            ];
          };
        }
      );

      formatter = forAllSystems (_: pkgs: pkgs.nixfmt-rfc-style);
    };
}
