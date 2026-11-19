# Original game piece traces

These eleven SVG files are the vector masters for the pieces used by the game.
Each uses filled black paths, transparent negative space, a 1024×1024 view box,
and a uniformly scaled silhouette with a 64-unit minimum margin. The PNGs in the
parent directory are 1024×1024 exports of these masters.

References are the original embedded 3507×4912 scan in `tmp/mas-plan.pdf`:

- Corporal: the intact **red** piece at the bottom of the plan, preserving the
  curled moustache, chin, hat brim, and narrow light band across the hat.
- The other ten: the white pieces across the top of the plan, rotated upright.

The extraction uses the embedded JPEG directly rather than an enlarged preview.
Black ink is segmented from the paper, isolated scan dust of at most three native
pixels is removed, and Potrace fits Bézier paths with conservative smoothing.
The mine uses a lighter threshold to retain the coiled fuse's small openings;
a local threshold retains the corporal's lighter hat-band detail. No generative
redrawing or replacement symbols are used. The trace follows the supplied scan;
it cannot recover detail that the original scan never captured.

All exported PNG RGB channels are exactly zero, including pixels with zero alpha.
Interior artwork is opaque; only the outline antialiasing uses partial alpha.
There is no paper texture, tinted ink, or white matte to produce a background halo.
The client draws the square images without changing their aspect ratio.

Re-export the PNGs from the committed vectors (Pillow and librsvg required):

```sh
python3 tools/trace_pieces.py --export-only
```

Reproduce the vector traces and PNGs from the source PDF (also requires Poppler
and [Potrace](https://potrace.sourceforge.net/), tested with version 1.16):

```sh
python3 tools/trace_pieces.py
# If Potrace is installed outside PATH:
POTRACE=/path/to/potrace python3 tools/trace_pieces.py
```

`tools/extract_assets.py` also exports these committed masters while preserving
the restored board and cover. It does not replace the traces with soft scan crops.
