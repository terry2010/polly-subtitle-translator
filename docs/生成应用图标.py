#!/usr/bin/env python3
"""
生成软件图标脚本（macOS + Windows 全分辨率全格式）

用法:
    python3 docs/生成应用图标.py <源图片路径> <输出目录>

示例:
    python3 docs/生成应用图标.py ~/Desktop/icon.png src-tauri/icons

依赖:
    pip3 install Pillow

功能:
    从一张 PNG 图片生成 macOS 和 Windows 所需的所有图标：
    - macOS: .icns 文件 + 各分辨率 PNG
    - Windows: .ico 文件 + 各分辨率 PNG
    - Tauri 所需的图标配置
"""

import sys
import os
import shutil
import struct
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    print("错误: 需要安装 Pillow")
    print("    pip3 install Pillow")
    sys.exit(1)


# === macOS 图标规格 ===
# .icns 文件包含的分辨率（像素）
MAC_ICNS_SIZES = [16, 32, 64, 128, 256, 512, 1024]

# macOS PNG 图标（Tauri 配置用）
MAC_PNG_SIZES = [32, 128, 256, 512, 1024]

# === Windows 图标规格 ===
# .ico 文件包含的分辨率
WIN_ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

# Windows Store / NSIS 方形 Logo（像素）
WIN_STORE_LOGOS = {
    "Square30x30Logo": 30,
    "Square44x44Logo": 44,
    "Square71x71Logo": 71,
    "Square89x89Logo": 89,
    "Square107x107Logo": 107,
    "Square142x142Logo": 142,
    "Square150x150Logo": 150,
    "Square284x284Logo": 284,
    "Square310x310Logo": 310,
    "StoreLogo": 50,
}

# === Tauri 配置中引用的图标 ===
TAURI_ICONS = [
    "32x32.png",
    "128x128.png",
    "128x128@2x.png",
    "icon.icns",
    "icon.ico",
]

# === 网站 favicon / PWA 图标规格 ===
# 参考 public/ 目录下已有的文件
WEB_FAVICON_SIZES = [16, 32, 48, 180, 192, 512]
WEB_FAVICON_ICO_SIZES = [16, 32, 48]


def resize_image(img: Image.Image, size: int) -> Image.Image:
    """将图片缩放到指定尺寸（正方形），使用高质量 LANCZOS 重采样"""
    return img.resize((size, size), Image.LANCZOS)


# === SECTION 1 END ===


def generate_macos_pngs(img: Image.Image, output_dir: Path):
    """生成 macOS 所需的各分辨率 PNG 文件"""
    print("\n[macOS] 生成 PNG 图标...")
    for size in MAC_PNG_SIZES:
        resized = resize_image(img, size)
        filename = f"{size}x{size}.png"
        resized.save(output_dir / filename, "PNG")
        print(f"  ✓ {filename} ({size}x{size})")

    # 生成 128x128@2x.png（即 256x256，文件名标记为 @2x）
    resized_2x = resize_image(img, 256)
    resized_2x.save(output_dir / "128x128@2x.png", "PNG")
    print(f"  ✓ 128x128@2x.png (256x256)")

    # 生成 icon.png（512x512，通用图标）
    icon_512 = resize_image(img, 512)
    icon_512.save(output_dir / "icon.png", "PNG")
    print(f"  ✓ icon.png (512x512)")


def generate_icns(img: Image.Image, output_dir: Path):
    """生成 macOS .icns 文件"""
    print("\n[macOS] 生成 icon.icns ...")
    # icns 支持的 OSType 标识和对应尺寸
    # 参考: https://en.wikipedia.org/wiki/Apple_Icon_Image_format
    icns_types = [
        (16,  b"is32", b"s8mk"),    # 16x16  RGB + mask
        (32,  b"il32", b"l8mk"),    # 32x32
        (64,  b"ih32", b"h8mk"),    # 48x48 -> 用 64
        (128, b"it32", b"t8mk"),    # 128x128
    ]
    # 现代 icns 用 ARGB 格式（ic07, ic08, ic09, ic10, ic11, ic12, ic13, ic14）
    icns_modern = [
        (128,  b"ic07"),   # 128x128
        (256,  b"ic08"),   # 256x256
        (512,  b"ic09"),   # 512x512
        (1024, b"ic10"),   # 1024x1024
        (16,   b"ic11"),   # 16x16 (retina 32)
        (32,   b"ic12"),   # 32x32 (retina 64)
        (128,  b"ic13"),   # 128x128 (retina 256)
        (256,  b"ic14"),   # 256x256 (retina 512)
    ]

    # 使用 Pillow 直接生成 icns（Pillow 原生支持）
    # Pillow 的 icns 保存需要特定尺寸
    icns_img = img.convert("RGBA")
    icns_img.save(output_dir / "icon.icns", format="ICNS")
    print(f"  ✓ icon.icns (macOS icon bundle)")


def generate_windows_pngs(img: Image.Image, output_dir: Path):
    """生成 Windows Store / NSIS 所需的方形 Logo PNG"""
    print("\n[Windows] 生成 Store Logo PNG...")
    for name, size in WIN_STORE_LOGOS.items():
        resized = resize_image(img, size)
        filename = f"{name}.png"
        resized.save(output_dir / filename, "PNG")
        print(f"  ✓ {filename} ({size}x{size})")


def generate_ico(img: Image.Image, output_dir: Path):
    """生成 Windows .ico 文件（包含多分辨率）"""
    print("\n[Windows] 生成 icon.ico ...")
    sizes = WIN_ICO_SIZES
    # Pillow 原生支持保存 .ico，会自动包含多分辨率
    ico_img = img.convert("RGBA")
    ico_img.save(output_dir / "icon.ico", format="ICO", sizes=[(s, s) for s in sizes])
    print(f"  ✓ icon.ico (包含 {', '.join(str(s) for s in sizes)} 尺寸)")


def generate_web_favicons(img: Image.Image, output_dir: Path):
    """生成网站 favicon 和 PWA 图标"""
    print("\n[Web] 生成网站 favicon / PWA 图标...")
    for size in WEB_FAVICON_SIZES:
        resized = resize_image(img, size)
        filename = f"favicon-{size}.png"
        resized.save(output_dir / filename, "PNG")
        print(f"  ✓ {filename} ({size}x{size})")

    # apple-touch-icon.png（180x180，iOS 添加到主屏幕图标）
    apple_icon = resize_image(img, 180)
    apple_icon.save(output_dir / "apple-touch-icon.png", "PNG")
    print(f"  ✓ apple-touch-icon.png (180x180)")

    # favicon.ico（16/32/48 多分辨率，浏览器标签栏用）
    favicon_img = img.convert("RGBA")
    favicon_img.save(
        output_dir / "favicon.ico",
        format="ICO",
        sizes=[(s, s) for s in WEB_FAVICON_ICO_SIZES],
    )
    print(f"  ✓ favicon.ico (包含 {', '.join(str(s) for s in WEB_FAVICON_ICO_SIZES)} 尺寸)")


# === SECTION 2 END ===


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)

    source_path = Path(sys.argv[1]).expanduser().resolve()
    output_dir = Path(sys.argv[2]).expanduser().resolve()

    if not source_path.exists():
        print(f"错误: 源图片不存在: {source_path}")
        sys.exit(1)

    if not source_path.suffix.lower() == ".png":
        print(f"错误: 源图片必须是 PNG 格式，当前: {source_path.suffix}")
        sys.exit(1)

    # 创建输出目录
    output_dir.mkdir(parents=True, exist_ok=True)

    # 加载源图片
    print(f"源图片: {source_path}")
    img = Image.open(source_path).convert("RGBA")
    print(f"原始尺寸: {img.size[0]}x{img.size[1]}")
    if img.size[0] != img.size[1]:
        print("警告: 源图片不是正方形，将按居中裁剪处理")
        size = min(img.size)
        left = (img.size[0] - size) // 2
        top = (img.size[1] - size) // 2
        img = img.crop((left, top, left + size, top + size))
        print(f"裁剪后尺寸: {img.size[0]}x{img.size[1]}")

    if min(img.size) < 1024:
        print(f"警告: 源图片尺寸小于 1024x1024，放大后可能模糊。建议使用 1024x1024 或更大的图片。")

    print(f"输出目录: {output_dir}")

    # === 生成所有图标 ===
    generate_macos_pngs(img, output_dir)
    generate_icns(img, output_dir)
    generate_windows_pngs(img, output_dir)
    generate_ico(img, output_dir)
    generate_web_favicons(img, output_dir)

    # === 汇总 ===
    print("\n" + "=" * 50)
    print("  ✅ 所有图标生成完成！")
    print("=" * 50)
    print(f"\n输出目录: {output_dir}")
    print(f"\nTauri 配置中引用的图标（tauri.conf.json）:")
    for icon in TAURI_ICONS:
        path = output_dir / icon
        status = "✓" if path.exists() else "✗"
        print(f"  {status} icons/{icon}")
    print(f"\n请将生成的文件复制到 src-tauri/icons/ 目录，")
    print(f"或直接将输出目录设为 src-tauri/icons/。")


if __name__ == "__main__":
    main()


# === SECTION 3 END ===
