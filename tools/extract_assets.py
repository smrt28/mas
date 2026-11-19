"""Extract the supplied PDF artwork. Run from the repository root.

Requires Poppler (pdftoppm), librsvg (rsvg-convert), and Pillow. The board comes from tmp/mas-plan.pdf; the original mas.pdf supplies the cover.
Preserve the restored board and cover. Recover a missing board from its lossless
restored PDF master; never replace it with the creased scan or printed counts.
Piece PNGs are rasterized from the committed SVG masters, preserving their traces.
Coordinates below refer to a 2000px preview and are scaled to a 4912px render,
matching the new plan's native scanned resolution rather than enlarging thumbnails.
"""
import pathlib
import subprocess
import tempfile
import sys
import shutil
from PIL import Image

OUT = pathlib.Path('assets')
PLAN = pathlib.Path('tmp/mas-plan-restored.pdf')
def extract():
    OUT.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        if not (OUT/'board.png').exists():
            if not PLAN.exists():
                raise SystemExit('Restore assets/board.png or tmp/mas-plan-restored.pdf from version control; the original scan must not overwrite the cleaned artwork.')
            subprocess.run(['pdfimages', '-f', '1', '-l', '1', '-png', str(PLAN),
                            tmp+'/restored'], check=True)
            shutil.copyfile(tmp+'/restored-000.png', OUT/'board.png')
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
    print('Preserved/restored cleaned board and upright cover; exported piece SVG masters.')


if __name__ == '__main__':
    extract()
