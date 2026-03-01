# PoE Leveling Guide

A lightweight overlay tool for Path of Exile that displays leveling notes and zone diagrams without leaving the game.

Ported from [PoE-Leveling-Guide (AHK)](https://github.com/JusKillmeQik/PoE-Leveling-Guide) to Rust due to significant performance issues with AutoHotkey. Many auxiliary features from the original (builds, gems screenshot references) have been removed as they are better handled by [Awakened PoE Trade](https://github.com/SnosMe/awakened-poe-trade) and other tools.

## How It Works

As you progress through leveling zones, the tool monitors your `Client.txt` log file to detect zone transitions. Only the zone name is logged by the game, so the tool infers the correct act and zone based on which part of the campaign you are in. If you go backwards, you may need to manually update the part, which will cause it to recheck your location.

Based on your current zone, notes and diagrams from [this leveling guide](https://docs.google.com/document/d/1sExA-AnTbroJ-HN2neZiij5G4X9u2ENlC7m_zf1tqP8/edit) are shown in a transparent overlay so you never have to alt-tab out of Path of Exile.

## Features

- Transparent, always-on-top overlay with click-through support
- Zone detection via Client.txt log parsing
- Leveling notes (Default, Abbreviated, Detailed) with color-coded text
- Zone layout diagrams
- Custom notes support — drop your own notes in a `Custom Notes/` folder
- EXP penalty indicator
- Configurable opacity, image sizing, and window positions
- Crash recovery — restores your last zone/act on restart
- Hide when PoE is not focused

## Building

Requires [Rust](https://rustup.rs/) (Windows target).

```
make build       # debug build
make release     # optimized release build
make run         # build and run (debug)
make run-release # build and run (release)
```

Or directly with cargo:

```
cargo build --release
```

## Configuration

On first run, a `config.ini` file is created next to the executable with default settings. You can configure:

- **Client.txt path** — location of your PoE log file
- **Overlay opacity** — transparency of the overlay panels
- **Note type** — Default, Abbreviated, Detailed, or any custom notes
- **Image width/spacing** — size and spacing of zone diagrams
- **Hide when unfocused** — automatically hide when PoE is not the active window

## Custom Notes

To create custom notes, click "Create Custom" in the settings panel. This copies the Default notes to a `Custom Notes/` folder next to the executable, which you can then edit freely. Custom notes override the built-in notes when selected.

### Note Format

Each act has two files: `guide.txt` (zone checklist) and `notes.txt` (detailed tips). Lines can be color-coded using either a letter-comma or symbol-space prefix:

| Code | Symbol | Color  | Typical Usage        |
|------|--------|--------|----------------------|
| `R,` | `< `   | Red    | Danger / warnings    |
| `G,` | `+ `   | Green  | Trials / gem rewards |
| `B,` | `> `   | Blue   | Quests / passives    |
| `Y,` | `- `   | Yellow | Important reminders  |
| `W,` |        | White  | Explicit default     |

Lines without a recognized prefix render in white. Both formats are interchangeable:

```
B,Dweller of the Deep: Passive
> Lower Prison:        Trial / Support GEM
Kill Merveil
< RECOMMEND high cold resist
+ TRIAL - Spike traps
- Remember to grab waypoint
```

## License

[CC BY-NC 4.0](LICENSE) — Free to use and modify for non-commercial purposes with attribution.
