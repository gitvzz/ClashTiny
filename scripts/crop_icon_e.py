#!/usr/bin/env python3
"""Crop img-*.png to tray icon size.

Usage:
  python3 crop_icon_e.py [size]
  python3 crop_icon_e.py        # 默认 size=650
  python3 crop_icon_e.py 700    # 从中心裁出 700×700 区域
"""

from PIL import Image
import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(SCRIPT_DIR)
ICONS_DIR = os.path.join(PROJECT_DIR, 'src-tauri', 'icons')

VARIANTS = [
    ('img-e.png', 'tray-e'),
    ('img-n.png', 'tray-n'),
    ('img-s.png', 'tray-s'),
    ('img-t.png', 'tray-t'),
]


def crop_center(img, size):
    """从中心裁出 size×size 的正方形区域。"""
    w, h = img.size
    side = min(size, w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    return img.crop((left, top, left + side, top + side))


def main(size):
    for inp, name in VARIANTS:
        path = os.path.join(ICONS_DIR, inp)
        if not os.path.exists(path):
            print(f'  Skip {inp} (not found)')
            continue
        print(f'  {name}...')
        img = Image.open(path).convert('RGBA')
        cropped = crop_center(img, size)
        img_2x = cropped.resize((44, 44), Image.LANCZOS)
        img_1x = cropped.resize((22, 22), Image.LANCZOS)
        img_2x.save(os.path.join(ICONS_DIR, f'{name}@2x.png'))
        img_1x.save(os.path.join(ICONS_DIR, f'{name}.png'))
        print(f'    -> {name}.png, {name}@2x.png')

    print('Done.')


if __name__ == '__main__':
    size = 650
    if len(sys.argv) >= 2:
        try:
            size = int(sys.argv[1])
        except ValueError:
            print('Error: size must be an integer (e.g. 650)')
            sys.exit(1)
        if size < 1:
            print('Error: size must be positive')
            sys.exit(1)

    print(f'Crop size: {size}×{size}')
    main(size)
