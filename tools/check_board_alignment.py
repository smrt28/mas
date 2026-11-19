"""Check restored grid registration against the original PDF without changing it.

Requires Poppler and Pillow. Reports drift in game pixels at a 600px board width.
The full-page normalized positions are used, so different image resolutions do
not affect the comparison. The two 1x3 gaps do not interrupt vertical grid edges.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
from PIL import Image


def grid_lines(path):
    image = Image.open(path).convert('RGB')
    width, height = image.size
    pixels = image.load()

    def red(x, y):
        r, g, b = pixels[x, y]
        return r > 90 and r > 1.7*g and r > 1.7*b and g < 110

    result = {}
    for axis, count, start, step, total in [
            ('x', 11, 10, 140, 1428), ('y', 12, 235, 140, 2000)]:
        extent = width if axis == 'x' else height
        centers = []
        for i in range(count):
            expected = (start+step*i)/total
            scores = []
            for at in range(max(0, round((expected-.012)*extent)),
                            min(extent, round((expected+.012)*extent)+1)):
                if axis == 'x':
                    score = sum(red(at, y) for y in range(round(height*.125), round(height*.885)))
                else:
                    score = sum(red(x, at) for x in range(round(width*.01), round(width*.985)))
                scores.append((at, score))
            peak = max(score for _, score in scores)
            if peak == 0:
                raise ValueError(f'Missing {axis} gridline {i}')
            plateau = [at for at, score in scores if score >= peak*.92]
            centers.append(sum(plateau)/len(plateau)/extent)
        result[axis] = centers
    return result


def check(image_path, report_path):
    with tempfile.TemporaryDirectory() as temp:
        reference = Path(temp)/'original'
        subprocess.run(['pdftoppm', '-scale-to', '2000', '-png', '-singlefile',
                        'tmp/mas-plan.pdf', str(reference)], check=True)
        original = grid_lines(reference.with_suffix('.png'))
    restored = grid_lines(image_path)
    game_extent = {'x': 600, 'y': 600*1178.88/841.68}
    drift = {axis: [abs(a-b)*game_extent[axis]
                    for a, b in zip(original[axis], restored[axis])]
             for axis in ('x', 'y')}
    maximum = max(value for values in drift.values() for value in values)
    report = {'source': 'tmp/mas-plan.pdf', 'restored': str(image_path),
              'comparison_board_width': 600, 'gridline_count': 23,
              'maximum_drift_pixels': maximum, 'gridline_drift_pixels': drift,
              'passes_one_pixel_tolerance': maximum <= 1.0}
    report_path.write_text(json.dumps(report, indent=2)+'\n')
    print(f'Checked 23 gridlines: maximum drift {maximum:.3f} pixels at 600px board width.')
    if maximum > 1.0:
        raise SystemExit('Restored grid exceeds one-pixel registration tolerance.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', type=Path, default=Path('assets/board.png'))
    parser.add_argument('--report', type=Path, default=Path('assets/board-alignment.json'))
    args = parser.parse_args()
    check(args.image, args.report)
