"""Package the restored board PNG losslessly on the original PDF page size.

Requires Pillow. The supplied tmp/mas-plan.pdf is never modified. Image samples
are embedded with Flate compression, avoiding an additional JPEG encoding pass.
"""
from pathlib import Path
import argparse
import zlib
from PIL import Image


def export(image_path, output_path):
    with Image.open(image_path) as source:
        image = source.convert('RGB')
    width, height = image.size
    pixels = zlib.compress(image.tobytes(), 9)
    content = b'q\n841.68 0 0 1178.88 0 0 cm\n/Board Do\nQ\n'
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 841.68 1178.88] '
        b'/Resources << /XObject << /Board 4 0 R >> >> /Contents 5 0 R >>',
        (f'<< /Type /XObject /Subtype /Image /Width {width} /Height {height} '
         f'/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode '
         f'/Length {len(pixels)} >>\nstream\n').encode()+pixels+b'\nendstream',
        f'<< /Length {len(content)} >>\nstream\n'.encode()+content+b'endstream',
    ]
    pdf = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf.extend(f'{number} 0 obj\n'.encode()+obj+b'\nendobj\n')
    xref = len(pdf)
    pdf.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        pdf.extend(f'{offset:010} 00000 n \n'.encode())
    pdf.extend((f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\n'
                f'startxref\n{xref}\n%%EOF\n').encode())
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_bytes(pdf)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', type=Path, default=Path('assets/board.png'))
    parser.add_argument('--output', type=Path, default=Path('tmp/mas-plan-restored.pdf'))
    args = parser.parse_args()
    export(args.image, args.output)
    print(f'Exported {args.output} at 841.68×1178.88 points (lossless image embedding).')
