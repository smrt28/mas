# Cover restoration

`assets/cover.png` is the upright, lightly restored game-cover scan from `mas.pdf`, edited with the built-in imagegen tool. The rider is on the left, the original Czech title on the right, and the PAN logo at bottom right. The reviewed output is 1532×1027 pixels. The source PDF remains available for comparison.

`tools/extract_assets.py` preserves this reviewed asset. If it is missing, the script extracts a correctly oriented raw fallback from the PDF. Rerunning extraction does not regenerate the AI restoration.

The following are the final prompts used for the restoration and its targeted logo-lettering correction.

## Restoration prompt

Edit target: the supplied original Maršál a špión board-game cover scan.
First rotate the entire cover exactly 180 degrees so the rider and horse stand upright on the LEFT, the title reads normally on the RIGHT, and the circular PAN logo is in the BOTTOM RIGHT.
Then perform restrained archival scan restoration: gently sharpen the existing printed lines and lettering, reduce scanner grain/halftone noise and minor dust, and slightly improve legibility and local contrast. Preserve the original vintage warm parchment palette and illustrated/printed style.
Critical invariants: keep the full original composition, framing, aspect ratio, proportions, rider, horse, uniform, tack, map background, diagonal white stripe and original circular PAN logo. Preserve the original typography and exact Czech title “MARŠÁL A ŠPIÓN”, including every accent; the two-line layout must read “MARŠÁL” above “A ŠPIÓN”. Preserve the small original logo lettering as closely as possible. Do not redesign, replace the illustration, add objects or words, crop, modernize, oversaturate, invent texture or produce sharpening halos. Keep the background opaque. This is a mild cleanup of the same original cover, not a new cover.

## Logo correction prompt

Precise local correction to Image 1 (the upright restored cover). Image 2 is the original upside-down source, supplied only to verify the bottom-right PAN logo after rotating its reference mentally 180 degrees. Keep the entirety of Image 1 unchanged except the small circular logo's perimeter lettering. Restore the exact original curved Czech inscription: “NOVÁ ŘADA SPOLEČENSKÝCH HER”. Do not substitute “NOVÁ SPOLEČENSKÁ HRA”. Preserve the original PAN mark/cartoon, circular red/black/cream logo, and lettering design as faithfully as possible. Keep Image 1's upright rider on the left, exact title “MARŠÁL A ŠPIÓN” on the right, parchment background, cleaned illustrated details, palette, dimensions, framing and composition identical. No other edits. Opaque background.
