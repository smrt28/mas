# Board restoration

The game uses `assets/board.png`, restored from the full sheet in `tmp/mas-plan.pdf` with the built-in imagegen tool. The original PDF is retained unchanged. `tmp/mas-plan-restored.pdf` contains the restored board at the original 841.68×1178.88-point page size, with lossless embedding of the game PNG.

The restoration removes the center fold and scan noise, reconstructs the interrupted white KAPRAL drawing using the intact red drawing, lightly sharpens the printed art and lettering, and erases all 22 printed inventory numbers and multiplication signs. Both rank strips retain their illustrations and labels; the game draws its own remaining reserve counts directly on the clean card backgrounds.

The full-page grid has been checked against the original source using `tools/check_board_alignment.py`. The report in `assets/board-alignment.json` measures all 23 gridline centers in normalized page coordinates. No board geometry or click targets were changed. The source is a scan, so this is a restoration rather than a claim of recovering missing original print data perfectly.

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
