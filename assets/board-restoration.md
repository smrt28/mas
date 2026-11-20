# Board restoration

The game uses `assets/board.png`, restored from the full sheet in `tmp/mas-plan.pdf` with the built-in imagegen tool. The original PDF is retained unchanged. `tmp/mas-plan-restored.pdf` contains the restored board at the original 841.68×1178.88-point page size, with lossless embedding of the game PNG.

The restoration removes the center fold and scan noise, reconstructs the interrupted white KAPRAL drawing using the intact red drawing, lightly sharpens the printed art and lettering, and erases all 22 printed inventory numbers and multiplication signs. Both rank strips retain their illustrations and labels; the game draws its own remaining reserve counts directly on the clean card backgrounds.

The full-page grid has been checked against the original source using `tools/check_board_alignment.py`. The report in `assets/board-alignment.json` measures all 23 gridline centers in normalized page coordinates. That initial restoration left board geometry and click targets unchanged; the deployment-strip adjustment below updates only the legend targets. The source is a scan, so this is a restoration rather than a claim of recovering missing original print data perfectly.

## Maintenance

- `python3 tools/extract_assets.py` preserves the restored PNG. If it is missing, it can recover the exact pixels from `tmp/mas-plan-restored.pdf`; it never overwrites the restoration with the creased source scan.
- `python3 tools/export_board_pdf.py` updates the restored PDF from the reviewed PNG using lossless image compression.
- `python3 tools/check_board_alignment.py` checks registration against the original PDF and updates the alignment report. It fails if drift exceeds one game pixel at a 600px board width.

## Final image-edit prompt

Use case: precise-object-edit / archival scan restoration.
Image 1 is the EDIT TARGET: the complete original Czech Maršál a špión game plan. Image 2 is ONLY a reference for the intact KAPRAL/corporal drawing.
Restore Image 1 conservatively, keeping its original style, warm parchment colors, muted red cards, cream cards, black artwork, labels and proportions.
Edits:
1. Remove the visible fold/crease through the middle, its seam, shadow and discoloration, blending the intact neighboring paper/map texture. Repair any interrupted lines. Repair the crease-damaged KAPRAL drawing in the center of the TOP white strip: preserve its surviving silhouette, tall hat, narrow hat band, hair/brim, curled moustache and curved chin outline. Use the intact red KAPRAL at the BOTTOM and Image 2 to reconstruct only the missing/interrupted details. The TOP white KAPRAL remains upside down like the other top pieces.
2. Erase ALL twenty-two printed piece counts and their multiplication signs from BOTH strips (all 1×, 2×, 3×, 4×, 5×, 6× and 8×, including upside-down ones at the top). Inpaint the original cream or red card background beneath each count. Leave a clean blank count area on every card. Keep EVERY piece illustration and Czech rank label intact and in exactly its original position. No new numbers, symbols, text, labels or buttons.
3. Reduce scan noise and dust and lightly sharpen printed artwork, lettering and grid edges. Preserve the original printing character, map texture and palette. No heavy sharpening halos, new decorative texture, modernization, changed typography, or replacement symbols.
ABSOLUTE GEOMETRY LOCK: keep the entire original page framing and aspect ratio. Do not crop, rotate, pad, shift, enlarge, respace or redraw the layout. Preserve the EXACT gridline locations, line thickness and card positions because interactive click targets already match them. The board is TEN columns by ELEVEN rows. The two flag gaps are ONE column wide by THREE rows tall at original columns D and G, rows 5–7, and contain the SAME flag pictures in the SAME positions. Keep all red/white board markings and all terrain patches in place. There must still be exactly eleven original upside-down white piece cards along the TOP and eleven original upright red piece cards along the BOTTOM, in their original order. Do not add playable pieces to the empty grid. Full original sheet, opaque background.

## Deployment strip alignment

The deployment strips in `assets/board.png` now use 96-pixel center spacing on
both sides. Red card slots are 88×132 pixels, starting at (5, 1344); the white
strip mirrors their horizontal positions and starts at y=16. Side margins differ
by only one raster pixel. Each original card illustration and label was moved
together without scaling or redrawing. Original differences in card widths remain.
The PNG remains 1059×1485, and rows 169–1326 (the entire playing grid) are
pixel-identical to the previous restoration.

The built-in imagegen tool supplied a clean parchment background plate. Only its
outer margins were used; ImageMagick composited the original cards and the exact
original grid pixels back into place. `plan::card` converts these PNG slot bounds
to the shared board coordinate system; `plan::count_position` anchors counts
inside the same bounds. Picking, selection borders and counts all use `Layout`.
Client tests exercise every rank at 360×480, 600×881, 1200×900 and 400×1600,
for both commanders at 1×/2× DPI, checking input, rendered border extents and
count transforms. A subsequent 1600×500 resize also checks placement.

Background-plate prompt (built-in tool):

> Use case: precise-object-edit. Edit target: original game board. Produce a clean background plate for archival retouching: remove all eleven red rounded cards together with their black illustrations and labels from the bottom margin below the grid, and all eleven cream cards with their illustrations and labels from the top margin above the grid. Inpaint those two margins with the adjacent warm golden parchment/map paper texture, seamlessly and conservatively. No cards, symbols, letters, shadows or ghosts should remain in these two margins. Keep full original 1059 x 1485 framing and proportions. Preserve the entire playing grid, both flag panels, colors and all central artwork unchanged. This is a background-only intermediate; original cards will be composited back without altering their pixels.

Validation: `cargo test --offline` passes all 50 tests; the grid check still reports
0.652px maximum registration drift relative to the source PDF. A headless GPU
screenshot was attempted, but this environment has no GPU adapter (`/dev/dri` is
absent). The edited PNG was visually inspected; client overlay validation uses
the actual spawned sprite and text transforms without a GPU.
