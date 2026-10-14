# Free games used for demos and CI

| File | Game | SHA-256 | License / provenance |
|---|---|---|---|
| `rkanoid-v3.00.gba` | Rkanoid v3.00 (homebrew Arkanoid-style breakout) | `99ae825ffb1b7e8b48b33cd2715ceb8ca524858f99fee4b22938489460083d94` | Declared copyright-free / freely redistributable by the repository owner on 2026-10-14. The ROM itself carries no license or author string; add the original source URL here when available. |

These are the only game images allowed in the repository. Commercial ROMs (for example under `roms/`, which is git-ignored) must never be committed, and packages built with `gba-recomp bundle` from them must not be redistributed.

The CI builds and releases a recompiled package of every game in this folder for each supported platform.
