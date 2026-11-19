"""Trace original game symbols into SVG masters and solid black transparent PNGs.

Requires Poppler (pdfimages), Potrace 1.16+, librsvg (rsvg-convert), and Pillow.
Run from the repository root. No image-generation model is used: all paths follow
ink in the supplied plan. Override POTRACE to use a locally installed executable.
"""
import argparse
from collections import deque
import os
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from PIL import Image, ImageOps

PLAN = Path('tmp/mas-plan.pdf')
OUT = Path('assets')
VECTOR = OUT / 'vectors'
SIZE = 1024
INK_SIZE = 896
# Source coordinates refer to the 2000px-high plan preview; native source is
# extracted losslessly from the PDF (3507x4912). The top white symbols are inverted.
WHITE_COLUMNS = {
    'mine': (30, 106), 'spy': (162, 254), 'miner': (290, 384),
    'rifleman': (420, 515), 'scout': (550, 642), 'cadet': (807, 885),
    'captain': (937, 1015), 'general': (1066, 1154),
    'marshal': (1194, 1290), 'flag': (1325, 1410),
}
SVG = 'http://www.w3.org/2000/svg'
ET.register_namespace('', SVG)


def remove_specks(ink, maximum=3):
    """Remove only isolated <=3-pixel scan dust; retain disconnected design detail."""
    pixels = ink.load()
    visited = set()
    for y in range(ink.height):
        for x in range(ink.width):
            if not pixels[x, y] or (x, y) in visited:
                continue
            component = []
            pending = deque([(x, y)])
            visited.add((x, y))
            while pending:
                px, py = pending.popleft()
                component.append((px, py))
                for nx in range(max(0, px-1), min(ink.width, px+2)):
                    for ny in range(max(0, py-1), min(ink.height, py+2)):
                        if pixels[nx, ny] and (nx, ny) not in visited:
                            visited.add((nx, ny))
                            pending.append((nx, ny))
            if len(component) <= maximum:
                for px, py in component:
                    pixels[px, py] = 0
    return ink


def source_icon(plan, name):
    scale = plan.height / 2000
    if name == 'corporal':
        # The white corporal has a printing defect through the face. The complete
        # red-side corporal is the requested reference, including its curled
        # moustache, chin outline, hat brim and narrow light band across the hat.
        box = (655, 1818, 758, 1920)
    else:
        left, right = WHITE_COLUMNS[name]
        box = (left, 99, right, 186)
    crop = plan.crop(tuple(round(v*scale) for v in box))
    if name != 'corporal':
        crop = crop.transpose(Image.Transpose.ROTATE_180)
    # Red channel separates black ink from red paper without outlining that paper.
    gray = crop.getchannel('R') if name == 'corporal' else ImageOps.grayscale(crop)
    cutoff = {'corporal': 130, 'mine': 120}.get(name, 145)
    ink = gray.point(lambda value: 255 if value < cutoff else 0)
    if name == 'corporal':
        # The narrow hat-band is lighter than the rest of the black ink. Preserve
        # its scanned contour with a local threshold instead of filling it in.
        band = (694, 1859, 734, 1863)
        local = tuple(round(v*scale)-round(box[i % 2]*scale)
                      for i, v in enumerate(band))
        detail = gray.crop(local).point(lambda value: 255 if value < 82 else 0)
        ink.paste(detail, local[:2])
    ink = remove_specks(ink)
    bounds = ink.getbbox()
    if not bounds:
        raise ValueError(f'No original ink found for {name}')
    return crop.crop(bounds), ink.crop(bounds)


def trace(name, ink, temporary, potrace):
    bitmap = temporary / f'{name}.pbm'
    raw_svg = temporary / f'{name}.svg'
    # Conservative smoothing: suppress tiny isolated dust, keep intentional sharp
    # corners, and optimize curves to within 0.1 of a native source pixel.
    ImageOps.invert(ink).convert('1').save(bitmap)
    subprocess.run([potrace, str(bitmap), '--svg', '--flat', '--turdsize', '0',
                    '--alphamax', '1.0', '--opttolerance', '0.1', '--unit', '100',
                    '--output', str(raw_svg)], check=True)
    source = ET.parse(raw_svg).getroot()
    _, _, width, height = map(float, source.attrib['viewBox'].split())
    scale = INK_SIZE/max(width, height)
    dx, dy = (SIZE-width*scale)/2, (SIZE-height*scale)/2
    master = ET.Element(f'{{{SVG}}}svg', {
        'width': str(SIZE), 'height': str(SIZE), 'viewBox': f'0 0 {SIZE} {SIZE}',
        'fill': '#000000',
    })
    title = ET.SubElement(master, f'{{{SVG}}}title')
    title.text = f'{name.capitalize()} — traced from original Maršál a špión plan'
    group = ET.SubElement(master, f'{{{SVG}}}g', {
        'transform': f'translate({dx:.8f} {dy:.8f}) scale({scale:.8f})',
    })
    for element in source:
        if element.tag == f'{{{SVG}}}g':
            group.append(element)
    svg_path = VECTOR / f'{name}.svg'
    ET.indent(master)
    ET.ElementTree(master).write(svg_path, encoding='utf-8', xml_declaration=True)
    render_png(svg_path, OUT / f'{name}.png', temporary)


def render_png(svg_path, png_path, temporary):
    large = temporary / 'render.png'
    subprocess.run(['rsvg-convert', '--width', str(SIZE*2), '--height', str(SIZE*2),
                    '--output', str(large), str(svg_path)], check=True)
    alpha = Image.open(large).convert('RGBA').getchannel('A').resize(
        (SIZE, SIZE), Image.Resampling.LANCZOS)
    # All RGB samples, even fully transparent ones, are exactly black. Only alpha
    # carries antialiasing. No paper colours or white matte can create a halo.
    result = Image.new('RGBA', (SIZE, SIZE), (0, 0, 0, 0))
    result.putalpha(alpha)
    result.save(png_path, optimize=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-only', action='store_true',
                        help='Rasterize the committed SVG masters without retracing the PDF')
    args = parser.parse_args()
    OUT.mkdir(exist_ok=True)
    VECTOR.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        temporary = Path(temp)
        if args.export_only:
            expected = set(WHITE_COLUMNS) | {'corporal'}
            if {path.stem for path in VECTOR.glob('*.svg')} != expected:
                raise ValueError('Expected all eleven committed SVG masters in assets/vectors')
            for svg_path in sorted(VECTOR.glob('*.svg')):
                render_png(svg_path, OUT / f'{svg_path.stem}.png', temporary)
            return
        subprocess.run(['pdfimages', '-f', '1', '-l', '1', '-j', str(PLAN),
                        str(temporary/'source')], check=True)
        plan = Image.open(temporary/'source-000.jpg').convert('RGB')
        for name in [*WHITE_COLUMNS, 'corporal']:
            reference, ink = source_icon(plan, name)
            trace(name, ink, temporary, os.environ.get('POTRACE', 'potrace'))
            print(f'{name}: traced {ink.width}×{ink.height} source pixels → SVG + {SIZE}×{SIZE} PNG')


if __name__ == '__main__':
    main()
