"""Extract the supplied PDF artwork. Run from the repository root.

Requires Poppler (pdftoppm), librsvg (rsvg-convert), and Pillow. The board comes from tmp/mas-plan.pdf; the original mas.pdf supplies the cover.
Preserve the restored cover when present; extract an upright fallback if missing.
Piece PNGs are rasterized from the committed SVG masters, preserving their traces.
Coordinates below refer to a 2000px preview and are scaled to a 4912px render,
matching the new plan's native scanned resolution rather than enlarging thumbnails.
"""
import pathlib
import subprocess
import tempfile
import sys
from PIL import Image

OUT = pathlib.Path('assets')
PLAN = pathlib.Path('tmp/mas-plan.pdf')
# Printed gridline centres: 10 columns, 11 rows. Cropping to these centres makes
# the printed cell centres agree with the client's evenly spaced hit targets.
BOARD = (10, 235, 1410, 1775)



def extract():
    OUT.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(['pdftoppm', '-scale-to', '4912', '-png', '-singlefile',
                        str(PLAN), tmp+'/plan'], check=True)
        plan = Image.open(tmp+'/plan.png').convert('RGB')
        scale = plan.height / 2000

        def crop(box):
            return plan.crop(tuple(round(v * scale) for v in box))

        # Store at 2x the UI's resolution, retaining the printed red/white grid,
        # landscape and both uninterrupted 1x3 flag panels.
        crop(BOARD).resize((1200, 1320), Image.Resampling.LANCZOS).save(OUT/'board.png')
        # cover.png is a manually reviewed AI restoration. Do not overwrite it
        # with the raw scan. A fresh extraction must rotate clockwise to place
        # the rider upright on the left and the title on the right.
        if not (OUT/'cover.png').exists():
            subprocess.run(['pdftoppm', '-f', '1', '-l', '1', '-scale-to', '1500',
                            '-png', '-singlefile', 'mas.pdf', tmp+'/cover'], check=True)
            Image.open(tmp+'/cover.png').crop((125,109,936,1318)).transpose(
                Image.Transpose.ROTATE_270).save(OUT/'cover.png')
    subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name('trace_pieces.py')),
                    '--export-only'], check=True)
    print(f'Extracted 10×11 board from {PLAN}; retained/restored upright cover; exported piece SVG masters')


if __name__ == '__main__':
    extract()
