#!/usr/bin/env python3
"""Generate tray icon variants from AI-generated base image.

Pipeline:
  1. Load the AI-generated shield+lightning PNG
  2. Convert white background to transparent (template image)
  3. Crop to icon bounds, position on canvas with room for letter
  4. Add status letter (N/S/T/E) at bottom-right
  5. Export 44×44 (2x) and 22×22 (1x) PNGs
"""

from PIL import Image, ImageDraw, ImageFont
import numpy as np
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(SCRIPT_DIR)
ASSET_DIR = os.path.join(
    os.path.expanduser('~'), '.cursor', 'projects',
    'Users-netfere-project2026-ClashTiny', 'assets')
OUTPUT_DIR = os.path.join(PROJECT_DIR, 'src-tauri', 'icons')

BASE_IMAGE = os.path.join(ASSET_DIR, 'tray-base-v4.png')

# Final icon canvas: 440×440 supersampled, resized to 44×44 / 22×22
CANVAS = 440


def find_bold_font(size):
    candidates = [
        '/System/Library/Fonts/Helvetica.ttc',
        '/Library/Fonts/Arial Bold.ttf',
        '/System/Library/Fonts/SFNSDisplay.ttf',
    ]
    for path in candidates:
        try:
            return ImageFont.truetype(path, size)
        except (OSError, IOError):
            continue
    return ImageFont.load_default()


def to_template_image(img):
    """Convert a black-on-white image to black-on-transparent (template)."""
    img = img.convert('RGBA')
    data = np.array(img)
    r, g, b, a = data[:, :, 0], data[:, :, 1], data[:, :, 2], data[:, :, 3]
    brightness = (r.astype(int) + g.astype(int) + b.astype(int)) // 3
    # Black pixels become opaque black; white pixels become transparent
    new_alpha = (255 - brightness).clip(0, 255).astype(np.uint8)
    data[:, :, 0] = 0
    data[:, :, 1] = 0
    data[:, :, 2] = 0
    data[:, :, 3] = new_alpha
    return Image.fromarray(data)


def crop_to_content(img, padding=0):
    """Crop image to bounding box of non-transparent content."""
    bbox = img.getbbox()
    if bbox:
        left, top, right, bottom = bbox
        return img.crop((
            max(0, left - padding),
            max(0, top - padding),
            min(img.width, right + padding),
            min(img.height, bottom + padding),
        ))
    return img


def crop_to_square(img):
    """Crop image to a centered square based on the shorter dimension."""
    w, h = img.size
    side = min(w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    return img.crop((left, top, left + side, top + side))


def create_base_canvas():
    """Load AI image, crop to square, convert to template, place on canvas."""
    raw = Image.open(BASE_IMAGE)
    square = crop_to_square(raw)
    template = to_template_image(square)
    cropped = crop_to_content(template, padding=2)
    # Re-crop to square after trimming transparent edges
    cropped = crop_to_square(cropped)

    # Shield occupies upper-left portion, leaving bottom-right for letter
    shield_size = int(CANVAS * 0.78)
    shield = cropped.resize((shield_size, shield_size), Image.LANCZOS)

    canvas = Image.new('RGBA', (CANVAS, CANVAS), (0, 0, 0, 0))
    offset = int(CANVAS * 0.02)
    canvas.paste(shield, (offset, offset), shield)
    return canvas


def add_letter(base, letter, font):
    """Add a bold letter as a badge at the bottom-right corner."""
    img = base.copy()
    draw = ImageDraw.Draw(img)

    bbox = draw.textbbox((0, 0), letter, font=font)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    ty_offset = bbox[1]

    # Bottom-right corner, snug against the canvas edge
    margin = 8
    lx = CANVAS - tw - margin
    ly = CANVAS - th - margin - ty_offset

    draw.text((lx, ly), letter, fill=(0, 0, 0, 255), font=font)
    return img


def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)

    print('Loading base image...')
    base = create_base_canvas()

    font = find_bold_font(130)
    print(f'Font loaded, canvas size: {CANVAS}×{CANVAS}')

    # Save base preview
    base_preview = base.resize((176, 176), Image.LANCZOS)
    base_preview.save(os.path.join(OUTPUT_DIR, 'tray-base-preview.png'))

    variants = [
        ('N', 'tray-n'),
        ('S', 'tray-s'),
        ('T', 'tray-t'),
        ('E', 'tray-e'),
    ]

    for letter, name in variants:
        icon = add_letter(base, letter, font)

        # 2x Retina (44×44)
        img_2x = icon.resize((44, 44), Image.LANCZOS)
        img_2x.save(os.path.join(OUTPUT_DIR, f'{name}@2x.png'))

        # 1x (22×22)
        img_1x = icon.resize((22, 22), Image.LANCZOS)
        img_1x.save(os.path.join(OUTPUT_DIR, f'{name}.png'))

        # Preview (176×176)
        preview = icon.resize((176, 176), Image.LANCZOS)
        preview.save(os.path.join(OUTPUT_DIR, f'{name}-preview.png'))

        print(f'  {name}: 22×22 + 44×44 + preview')

    print('Done!')


if __name__ == '__main__':
    main()
