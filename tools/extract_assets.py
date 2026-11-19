"""Extract the supplied PDF artwork. Run from the repository root.

Requires Poppler (pdftoppm) and Pillow. The complete board and sharp rank symbols
come from tmp/mas-plan.pdf; the original mas.pdf supplies the cover illustration.
Coordinates below refer to a 2000px preview and are scaled to a 4912px render,
matching the new plan's native scanned resolution rather than enlarging thumbnails.
"""
import pathlib
import subprocess
import tempfile
from PIL import Image, ImageOps

OUT = pathlib.Path('assets')
PLAN = pathlib.Path('tmp/mas-plan.pdf')
# Printed gridline centres: 10 columns, 11 rows. Cropping to these centres makes
# the printed cell centres agree with the client's evenly spaced hit targets.
BOARD = (10, 235, 1410, 1775)
RANKS = ['mine', 'spy', 'miner', 'rifleman', 'scout', 'corporal',
         'cadet', 'captain', 'general', 'marshal', 'flag']
# Tight icon columns exclude paper edges and printed labels; these are the
# original white-side symbols, which are upside-down along the top of the plan.
ICON_COLUMNS = [(30, 106), (162, 254), (290, 384), (420, 515), (550, 642),
                (682, 760), (807, 885), (937, 1015), (1066, 1154),
                (1194, 1290), (1325, 1410)]


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
        for rank, (left, right) in zip(RANKS, ICON_COLUMNS):
            icon = crop((left, 99, right, 186)).rotate(180)
            # Isolate the original dark ink from the cream paper. This preserves
            # the scanned symbol, including its antialiased edges, while avoiding
            # a square paper patch around each piece on the board.
            alpha = ImageOps.grayscale(icon).point(
                lambda value: max(0, min(255, round((175-value)*255/115))))
            bounds = alpha.getbbox()
            if bounds is None:
                raise ValueError(f'No ink found for {rank}')
            alpha = alpha.crop(bounds)
            alpha.thumbnail((224, 224), Image.Resampling.LANCZOS)
            piece = Image.new('RGBA', (256, 256), (43, 38, 31, 0))
            piece.paste(Image.new('RGBA', alpha.size, (43, 38, 31, 255)),
                        ((256-alpha.width)//2, (256-alpha.height)//2), alpha)
            piece.save(OUT/f'{rank}.png')

        subprocess.run(['pdftoppm', '-f', '1', '-l', '1', '-scale-to', '1500',
                        '-png', '-singlefile', 'mas.pdf', tmp+'/cover'], check=True)
        Image.open(tmp+'/cover.png').crop((125,109,936,1318)).rotate(
            90, expand=True).save(OUT/'cover.png')
    print(f'Extracted 10×11 board and 11 rank symbols from {PLAN}; cover from mas.pdf')


if __name__ == '__main__':
    extract()
