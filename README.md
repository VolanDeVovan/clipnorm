# clipnorm

A tiny background daemon that normalizes the Wayland clipboard so screenshots and copied images paste into terminal AI agents (Claude Code, Codex, Aider, …) without ceremony.

The pain it scratches: TUI agents running in Alacritty / Foot / WezTerm / Kitty can't read raw image bytes from the clipboard. They only attach an image when the clipboard exposes a `text/plain` absolute path. So copying a screenshot from Telegram or Firefox and hitting paste does nothing — the image is right there, the agent just can't see it.

clipnorm sits between the source app and the terminal:

1. Watches the Wayland clipboard via `wlr-data-control` / `ext-data-control`.
2. When an image-only clipboard appears, saves the bytes to `$XDG_RUNTIME_DIR/clipnorm/Pasted from <ts>.png`.
3. Republishes the clipboard with the original image bytes **plus** a `text/plain` path, `text/uri-list`, and `x-special/gnome-copied-files`.
4. Anything that pastes — TUI agents, Nautilus, image editors, browsers, chats — gets the format it knows.

Files live in `XDG_RUNTIME_DIR` (tmpfs, RAM-backed) and are pruned after **1 hour**, so nothing accumulates on disk.

## Install

### Nix (flake)

```sh
nix run github:VolanDeVovan/clipnorm
```

For a NixOS / home-manager setup, add it as a flake input and wire it as a systemd user service:

```nix
# flake.nix
inputs.clipnorm.url = "github:VolanDeVovan/clipnorm";

# home-manager module
{ inputs, pkgs, lib, ... }:
let clipnorm = inputs.clipnorm.packages.${pkgs.stdenv.hostPlatform.system}.default;
in {
  home.packages = [ clipnorm ];

  systemd.user.services.clipnorm = {
    Unit = {
      Description = "Wayland clipboard image normalizer";
      After = [ "graphical-session.target" ];
      PartOf = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = lib.getExe clipnorm;
      Restart = "on-failure";
      RestartSec = 5;
      MemoryMax = "256M";
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };
}
```

### Shell installer

Static musl binary, drops itself into `~/.local/bin/`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/VolanDeVovan/clipnorm/releases/latest/download/clipnorm-installer.sh \
  | sh
```

Linux x86_64 / aarch64. Built and tagged via [cargo-dist](https://github.com/axodotdev/cargo-dist) on every `v*` tag.

### From source

```sh
git clone https://github.com/VolanDeVovan/clipnorm.git
cd clipnorm
cargo install --path . --root ~/.local
```

## Run as a service

Once the binary is in `~/.local/bin/`, drop the unit file in place and enable it:

```sh
mkdir -p ~/.config/systemd/user
cp examples/clipnorm.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now clipnorm.service
```

Verify:

```sh
systemctl --user status clipnorm.service
journalctl --user -u clipnorm -f
```

## Configuration

All optional, all environment variables.

| Variable | Default | Notes |
|---|---|---|
| `CLIPNORM_OUTPUT_DIR` | `$XDG_RUNTIME_DIR/clipnorm` (or `/tmp/clipnorm`) | Where saved images go. |

## Compatibility

- **Compositor**: anything that exposes `wlr-data-control-v1` or `ext-data-control-v1` — niri, Sway, Hyprland, KDE, GNOME (via mutter ≥ 47), Wayfire, river, Cosmic, …
- **Terminal**: any that pipes raw clipboard text on paste. Tested with Alacritty; Kitty / WezTerm / Ghostty / Foot work the same way for the path-paste flow.
- **TUI agents that benefit**: Claude Code, Codex CLI, Aider, Gemini CLI, Continue CLI, opencode — anything that auto-attaches files when pasted as an absolute path.

## Related projects

- [clipaste](https://github.com/hqhq1025/clipaste) — same idea, but for **macOS / Windows / WSL2** and SSH bridging. clipnorm fills the Wayland Linux gap; if you cross platforms, run both.
- [wl-clip-persist](https://github.com/Linus789/wl-clip-persist) — keeps the clipboard alive after the source app closes, no transformation.
- [cliphist](https://github.com/sentriz/cliphist) / [stash](https://github.com/NotAShelf/stash) — clipboard history managers; complementary, different problem.

## How it stays out of trouble

- **Loop guard**: clipnorm advertises a sentinel MIME (`application/x-clipnorm-wrapped`) on its own publication. When the watch loop sees that, or any `text/plain*` / `text/uri-list`, it skips — so its own writes don't trigger another wrap, and ordinary text copies are left alone.
- **No disk pressure**: `XDG_RUNTIME_DIR` is tmpfs (RAM); files are deleted after 1h, inline after each event. Zero background timers.
- **Tiny**: ~2 MB resident, 0% CPU when idle.

## License

MIT — see [LICENSE](./LICENSE).
