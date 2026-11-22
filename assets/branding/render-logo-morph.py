"""Render the onboarding logo paths as the GitHub hero GIF and MP4.

Run with Pillow and imageio-ffmpeg installed. The source artwork stays in assets/
and ui/src/assets/. The path keyframes are adapted from WinX onboarding.
"""
from math import sin, pi
from pathlib import Path
import os
import subprocess

from PIL import Image, ImageDraw, ImageFont
import imageio_ffmpeg

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
WIDTH, HEIGHT = 800, 344
FPS, SECONDS, SCALE = 20, 4.5, 2
CANVAS = "#f8f9fb"
INK = "#191a1b"
SILVER = "#cbcdcf"
RED = "#e3181c"
FONTS = Path(os.environ.get("WINX_BRAND_FONTS", Path(os.environ.get("SystemRoot", "C:/Windows")) / "Fonts"))

INITIAL_SILVER = ((91, 164), (164, 91), (164, 91), (164, 91))
FINAL_SILVER = ((82, 80), (169, 168), (174, 80), (89, 168))
INITIAL_INK = ((91, 91), (128, 128), (165, 165), (91, 91), (128, 128), (165, 165))
FINAL_INK = ((47, 108), (85, 172), (128, 128), (209, 108), (171, 172), (128, 128))
INITIAL_RED = ((165, 137), (193, 137), (193, 165), (193, 193), (165, 193),
               (137, 193), (137, 165), (137, 137), (165, 137))
FINAL_RED = ((128, 103), (140, 115), (152, 127), (140, 139), (128, 151),
             (116, 139), (104, 127), (116, 115), (128, 103))


def mix(a, b, t):
    return a + (b - a) * t


def ease(t, start, end):
    x = min(1, max(0, (t - start) / (end - start)))
    return x * x * (3 - 2 * x)


def shape(start, end, amount):
    return [(mix(a, c, amount), mix(b, d, amount)) for (a, b), (c, d) in zip(start, end)]


def capsule(draw, points, width, color):
    radius = width / 2
    draw.line(points, fill=color, width=round(width), joint="curve")
    for x, y in points:
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=color)


def accent(points):
    trace = []
    for index in range(0, len(points) - 2, 2):
        a, b, c = points[index:index + 3]
        for step in range(16):
            t = step / 16
            trace.append(tuple(mix(mix(x, y, t), mix(y, z, t), t) for x, y, z in zip(a, b, c)))
    return trace


def opacity(image, amount):
    result = image.copy()
    result.putalpha(result.getchannel("A").point(lambda alpha: round(alpha * amount)))
    return result


def frame(t, original, final, title, tagline):
    size = (WIDTH * SCALE, HEIGHT * SCALE)
    canvas = Image.new("RGBA", size, CANVAS)
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle((1, 1, size[0] - 2, size[1] - 2),
                           radius=24 * SCALE, outline="#dedee1", width=2)
    motion = ease(t, .88, 2.65)
    reveal = ease(t, 2.42, 3.35)
    settlement = 1 + .026 * sin(pi * ease(t, 2.40, 3.15))
    mark_size = round(212 * SCALE * settlement)
    left = (size[0] - mark_size) // 2
    top = round(mix(65, 24, reveal) * SCALE)
    vector = Image.new("RGBA", (mark_size, mark_size))
    strokes = ImageDraw.Draw(vector)
    unit = mark_size / 256
    def scale(points):
        return [(x * unit, y * unit) for x, y in points]

    silver = scale(shape(INITIAL_SILVER, FINAL_SILVER, motion))
    ink = scale(shape(INITIAL_INK, FINAL_INK, motion))
    stroke = mix(73, 33, ease(t, .9, 2.35)) * unit
    for points in (silver[:2], silver[2:]):
        capsule(strokes, points, stroke, SILVER)
    for points in (ink[:3], ink[3:]):
        capsule(strokes, points, stroke, INK)
    if motion > .35:
        tips = shape(((91, 164), (93, 162), (164, 91), (162, 93)),
                     ((85, 173), (93, 164), (171, 173), (163, 164)), motion)
        tips = scale(tips)
        tip_width = 32 * unit * ease(t, 1.42, 2.30)
        for points in (tips[:2], tips[2:]):
            capsule(strokes, points, tip_width, SILVER)
    strokes.polygon(scale(accent(shape(INITIAL_RED, FINAL_RED, motion))), fill=RED)

    start = 1 - ease(t, .58, .90)
    end = ease(t, 2.56, 2.92)
    # The geometric paths carry the movement. Blend only at the endpoints to keep
    # the original raster artwork as the opening and final frames.
    tile = Image.alpha_composite(Image.new("RGBA", vector.size, CANVAS), vector)
    if start:
        tile = Image.blend(tile, original.resize(tile.size, Image.Resampling.LANCZOS), start)
    if end:
        tile = Image.blend(tile, final.resize(tile.size, Image.Resampling.LANCZOS), end)
    canvas.alpha_composite(tile, (left, top))

    wording = Image.new("RGBA", size)
    draw = ImageDraw.Draw(wording)
    shift = (1 - reveal) * 8 * SCALE
    draw.text((size[0] / 2, 251 * SCALE + shift), "WinX", font=title, fill=INK, anchor="mm")
    draw.text((size[0] / 2, 292 * SCALE + shift), "Your Nothing experience. Now on Windows.",
              font=tagline, fill="#4f5053", anchor="mm")
    canvas.alpha_composite(opacity(wording, reveal))
    return canvas.convert("RGB").resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)


def main():
    original = Image.open(ROOT / "ui/src/assets/nothingx-logo.png").convert("RGBA")
    original = Image.alpha_composite(Image.new("RGBA", original.size, CANVAS), original)
    final = Image.open(ROOT / "assets/winx-logo.png").convert("RGBA")
    title = ImageFont.truetype(str(FONTS / "segoeuib.ttf"), 52 * SCALE)
    tagline = ImageFont.truetype(str(FONTS / "segoeui.ttf"), 19 * SCALE)
    frames = [frame(i / FPS, original, final, title, tagline) for i in range(round(FPS * SECONDS))]
    pixels = b"".join(image.tobytes() for image in frames)
    ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    common = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo",
              "-pixel_format", "rgb24", "-video_size", f"{WIDTH}x{HEIGHT}",
              "-framerate", str(FPS), "-i", "-"]
    subprocess.run([*common, "-an", "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p",
                    "-movflags", "+faststart", HERE / "winx-logo-morph.mp4"], input=pixels, check=True)
    subprocess.run([*common, "-filter_complex",
                    "split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
                    "-loop", "0", HERE / "winx-logo-morph.gif"], input=pixels, check=True)


if __name__ == "__main__":
    main()
