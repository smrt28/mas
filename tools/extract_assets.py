"""Extract the original printed artwork; requires poppler and Pillow.
Run from the repository root. Coordinates refer to 1500px page renders.
"""
import pathlib
import subprocess
import tempfile
from PIL import Image

out = pathlib.Path('assets')
out.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory() as tmp:
    subprocess.run(['pdftoppm', '-scale-to', '1500', '-png', 'mas.pdf', tmp+'/page'], check=True)
    def page(n): return Image.open(f'{tmp}/page-{n:02}.png')
    # Rank symbols from the rules, in their original order. No generated artwork.
    p = page(5)
    names = ['marshal','general','captain','cadet','corporal','rifleman','scout','miner','spy','mine','flag']
    bounds = [(197,255,249,306),(252,255,302,306),(311,255,358,306),(370,255,417,306),
              (430,255,479,306),(490,255,535,306),(544,255,594,306),(602,255,653,306),
              (661,255,711,306),(744,255,791,306),(800,255,847,306)]
    for name, box in zip(names,bounds):
        p.crop(box).resize((100,100)).save(out/f'{name}.png')
    page(7).crop((185,310,620,898)).save(out/'terrain.png')
    page(1).crop((125,109,936,1318)).rotate(90,expand=True).save(out/'cover.png')
print('Extracted 11 rank symbols, terrain and cover from mas.pdf')
