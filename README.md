# Real-ESRGAN GUI

[![build](https://github.com/TransparentLC/realesrgan-gui/actions/workflows/build.yml/badge.svg)](https://github.com/TransparentLC/realesrgan-gui/actions/workflows/build.yml)
[![download](https://img.shields.io/github/downloads/TransparentLC/realesrgan-gui/total.svg)](https://github.com/TransparentLC/realesrgan-gui/releases)

实用、美观的图片放大工具 [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) 的图形界面，参考 [waifu2x-caffe](https://github.com/lltcggie/waifu2x-caffe) 设计。

<details>

<summary>README translations</summary>

* [简体中文 (Simplified Chinese)](https://github.com/TransparentLC/realesrgan-gui/blob/master/README.md)
* [English](https://github.com/TransparentLC/realesrgan-gui/blob/master/README.en-US.md)
* [Ukrainian (Українська)](https://github.com/TransparentLC/realesrgan-gui/blob/master/README.uk-UA.md) Translated by [@kirill0ermakov](https://github.com/kirill0ermakov)
* [Türkçe (Turkish)](https://github.com/TransparentLC/realesrgan-gui/blob/master/README.tr-TR.md) Translated by [@NandeMD](https://github.com/NandeMD)

</details>

<picture>
    <source media="(prefers-color-scheme:dark)" srcset="https://user-images.githubusercontent.com/47057319/219046038-0edbb076-a4b3-4c6b-884e-46895f38bae8.png">
    <img src="https://user-images.githubusercontent.com/47057319/219045988-f1515ffa-a190-499d-9cf0-ec044cb478ef.png">
</picture>

## 基本介绍

这个程序是 Real-ESRGAN 的命令行程序 [Real-ESRGAN-ncnn-vulkan](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan) 的图形界面，使用 [Rust](https://www.rust-lang.org/) 和 [Slint](https://slint.dev/) 编写，界面为 Fluent Design 风格，同时支持 Windows、Ubuntu 和 macOS 平台。

> [!NOTE]
> 本项目原先使用 Python 和 tkinter（Sun-Valley 主题）实现，现已完全重构为 Rust + Slint，Python 版本的代码已删除。原有的配置格式（`config.ini`）和翻译文件（`i18n.ini`）保持兼容。

快速上手：

* ![Windows 10+](https://img.shields.io/badge/Windows-10+-06b?logo=windows) 在 Release 中下载最新的 `realesrgan-gui-windows-bundled-v*.7z`，解压后打开 `realesrgan-gui.exe` 即可使用。
* ![Ubuntu 22.04+](https://img.shields.io/badge/Ubuntu-22.04+-e52?logo=ubuntu) 在 Release 中下载最新的 `realesrgan-gui-ubuntu-bundled-v*.tar.xz`，解压后打开 `realesrgan-gui` 即可使用。
* ![macOS Monterey+](https://img.shields.io/badge/macOS-Monterey+-111?logo=apple) 在 Release 中下载最新的 `realesrgan-gui-macos-appbundle-v*.tar.xz`，解压后在终端中执行 `chmod u+x "Real-ESRGAN GUI.app/Contents/MacOS/realesrgan-gui"`、`chmod u+x "Real-ESRGAN GUI.app/Contents/MacOS/realesrgan-ncnn-vulkan"` 和 `xattr -cr "Real-ESRGAN GUI.app"`，打开 `Real-ESRGAN GUI` 即可使用。

> [!TIP]
> Real-ESRGAN-ncnn-vulkan 从 2022 年 4 月开始就没有更新了，可以使用 [upscayl/upscayl-ncnn](https://github.com/upscayl/upscayl-ncnn) 这个仍然在维护的分支作为替代。
>
> 下载最新的 Release 并将压缩包中的 `upscayl-bin[.exe]` 解压到 GUI 的主程序所在的目录，它将会被优先使用。

> [!TIP]
> 你也可以在 Real-ESRGAN GUI 中使用 [Real-CUGAN](https://github.com/bilibili/ailab/tree/main/Real-CUGAN) 进行放大！详细配置方法参见[这里](#我觉得-real-cugan-的放大效果比-real-esrgan-更好)。

<details>

<summary>其它的运行方式和说明</summary>

* Release 中的 `realesrgan-gui-windows.7z` 和 `realesrgan-gui-ubuntu.tar.xz` 不包含 Real-ESRGAN-ncnn-vulkan 的主程序和官方模型，请自行在[这里](https://github.com/xinntao/Real-ESRGAN/releases)下载后解压到 GUI 的主程序所在的目录。
* Actions 中上传的是根据最新提交的代码打包的版本（相当于 Nightly），同样不包含 Real-ESRGAN-ncnn-vulkan 的主程序和官方模型。
* 如果需要从源代码运行，请安装 [Rust](https://rustup.rs/) 1.82 或以上版本，将 Real-ESRGAN-ncnn-vulkan 解压到项目目录，然后执行 `cargo run --release`，编译生成的可执行文件位于 `target/release/`。
* 在其它的 Linux 发行版中可能也可以运行本项目，不过我没有进行测试。

</details>

如果你想要贡献 Real-ESRGAN GUI，请查看 [CONTRIBUTING.md](https://github.com/TransparentLC/realesrgan-gui/blob/master/CONTRIBUTING.md)。

### 从源代码构建

需要 [Rust](https://rustup.rs/) 1.82 或以上版本。

```shell
# 调试运行
cargo run

# 发布构建，可执行文件位于 target/release/
cargo build --release
```

首次构建需要联网下载依赖（Slint、image、rfd、libwebp 等）。在 Linux 上还需要安装 Slint / winit 所需的系统库，例如 `libxkbcommon`、`libwayland-client`、`fontconfig` 和 OpenGL 相关的开发包。

发布时把 `target/release/realesrgan-gui[.exe]` 和 Real-ESRGAN-ncnn-vulkan 的主程序、模型放在同一目录即可。翻译文件已经内嵌在可执行文件中，不需要额外附带 `i18n.ini`。

如果需要让「关于」页面显示构建时间，可以在构建时设置 `REGUI_BUILD_TIME` 环境变量（已格式化的时间字符串，例如 `2024-06-02 14:06:22`）。

> [!NOTE]
> 在 macOS 上构建为 `.app` 时，需要自行准备 `Info.plist` 和图标；本项目不再附带 Tk 版本的打包脚本。

### 相关项目

* 在 Android 上使用 Real-ESRGAN：[tumuyan/RealSR-NCNN-Android](https://github.com/tumuyan/RealSR-NCNN-Android)
* 通过 Vapoursynth 对视频使用 Real-ESRGAN：[HolyWu/vs-realesrgan](https://github.com/HolyWu/vs-realesrgan)

## 功能介绍

在原版支持的功能的基础上，增加了以下功能：

* 任意尺寸放大
    * Real-ESRGAN 只能将输入的图片以固定的 2-4x 倍率（和选用的模型有关）放大。
    * 这一功能通过多次调用 Real-ESRGAN 后使用常规缩放算法降采样实现。
    * 例如将 640x360 的图片使用 2x 的模型放大到宽度 1600，实际操作为先放大到 1280x720，再放大到 2560x1440，最后降采样到 1600x900。
    * 默认使用 Lanczos 进行降采样，也可以选择其它算法。
* 对 GIF 的处理
    * 将 GIF 的各个帧拆分出来并记录时长，逐个放大后再进行合并。
* 拖拽支持
    * 拖拽文件或目录到窗口设定输入和输出路径
    * **注意：** 重构为 Slint 后暂时无法接收操作系统层面的文件拖放，详见[已知限制](#已知限制)。
    * 根据拖拽时选择的放大尺寸计算方式，在输出路径中会自动添加形如 x4、w1280、h1080 的后缀。
* 深色模式界面
    * 由 Slint 的 Fluent Design 主题根据系统设定自动选择浅色或深色模式。
* 多语言支持
    * 界面语言和系统语言一致（启动时通过系统区域设置判断）。
    * 目前支持简繁中文、英语、乌克兰语、土耳其语、西班牙语和法语。在缺少某个语言的翻译文本的情况下，默认会回退到英语。
    * **欢迎添加更多语言的翻译，或对现有的翻译进行改进～**（参见 [`i18n.ini`](https://github.com/TransparentLC/realesrgan-gui/blob/master/i18n.ini)）
    * 新增语言时，除了在 `i18n.ini` 中添加对应的翻译段，还需要在 [`src/i18n.rs`](https://github.com/TransparentLC/realesrgan-gui/blob/master/src/i18n.rs) 的 `LOCALES_MAP` 中补充「语言显示名称 → 语言代码」的映射（两者顺序需保持一致）。

## 效果对比

| Nearest Neighbor | Lanczos | waifu2x-caffe | Real-ESRGAN |
| --- | --- | --- | --- |
| ![](https://user-images.githubusercontent.com/47057319/166262181-cf1e6c02-a8d2-4d49-88d9-1dfe65107c18.png) | ![](https://user-images.githubusercontent.com/47057319/166262508-32010b72-76b1-4edb-ba8a-f850283873ea.png) | ![](https://user-images.githubusercontent.com/47057319/166262200-a350b33b-9ebb-4159-889c-38d9d5bba386.png) | ![](https://user-images.githubusercontent.com/47057319/166262192-735fb21b-7452-48fe-b99d-ed8233af6d31.png) |

| Nearest Neighbor | Lanczos | waifu2x-caffe | Real-ESRGAN |
| --- | --- | --- | --- |
| ![](https://user-images.githubusercontent.com/47057319/166262217-7623a30d-e4e9-46e4-a869-1dcabdbbd74e.png) | ![](https://user-images.githubusercontent.com/47057319/166262210-a836ed72-b197-4f5f-bcfd-3e459ebf5776.png) | ![](https://user-images.githubusercontent.com/47057319/166262243-810b894d-657d-4a84-84bb-88e76845404f.png) | ![](https://user-images.githubusercontent.com/47057319/166262229-6bc75e4b-9980-4c14-b4e4-4c0d53642a35.png) |

| Nearest Neighbor | Real-ESRGAN |
| --- | --- |
| ![](https://user-images.githubusercontent.com/47057319/168476063-28a142d4-87ef-491e-b50e-6c981236133f.gif) | ![](https://user-images.githubusercontent.com/47057319/168476067-68e76ed6-9589-44f8-ada8-2792dda0ded4.gif) |

| Nearest Neighbor | Real-ESRGAN |
| --- | --- |
| ![](https://user-images.githubusercontent.com/47057319/170270314-dce674be-e1d3-433f-a71f-763983b33e97.gif) | ![](https://user-images.githubusercontent.com/47057319/170273963-4b11551b-44e7-42f8-b0fd-5b2599087a95.gif) |

* waifu2x-caffe 使用 `UpResNet10` 和 `UpPhoto` 模型，降噪等级 3，开启 TTA。
* Real-ESRGAN 使用 `realesrgan-x4plus-anime` 和 `realesrgan-x4plus` 模型，开启 TTA。
* 放大倍率均为 4x。
* 为了减小文件大小，展示的 GIF 进行了有损压缩处理。

## 可能遇到的问题

### 如何选择模型

对于三次元图片建议使用 `realesrgan-x4plus`，对于二次元图片建议使用 `realesrgan-x4plus-anime`。

对于同一系列模型的不同倍率版本，建议选择等于或大于想要将图片放大的倍率的版本。例如在想要将图片放大到 3x 但是只有 x2 和 x4 模型的情况下，应该选择 x4 的模型。

带有 `animevideo` 的几个模型是针对二次元视频使用的，特点是模型文件较小以及处理速度较快（我自己的测试是 `realesrgan-x4plus-anime` 的 1.5x 到 3x 不等）。不过这个 GUI 并不会考虑加入视频处理功能。

你也可以在[这里](https://github.com/TransparentLC/realesrgan-gui/releases/tag/additional-models)下载使用第三方的附加模型，对于某些图片（特别是三次元图片）可能会有比官方模型更好的效果，请根据实际情况自由尝试。将模型的文件名相同的 `bin` 和 `param` 文件放在 `models` 目录，主程序在启动后会自动识别到这些模型。

### 拆分大小的作用

对应原版的 `-t tile-size` 参数。“自动设定”已经可以满足日常使用了，但是如果想要自己设定的话，在显存充足的情况下建议使用较大的值，处理速度更快，放大后的图片质量更好，细节更多（虽然可能不太明显）。

将 Real-ESRGAN-ncnn-vulkan 自带的 [256x256 的测试图](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan/blob/master/images/input2.jpg)使用 `realesrgan-x4plus` 模型在 TTA 模式下放大到 4x，选择不同的拆分大小的效果：[32](https://user-images.githubusercontent.com/47057319/168460056-1aaf420a-c2d0-4bbf-a350-703f69cd947f.png)，[256 或以上](https://user-images.githubusercontent.com/47057319/168460053-0c34296f-a5c7-447c-9f34-e86b6ebc7035.png)。

[#32](https://github.com/TransparentLC/realesrgan-gui/issues/32#issuecomment-1547148843) 有更详细一些的解释。

### TTA 模式的作用

理论上可以稍微提升放大后的图片的质量，但是实际上效果非常不明显，还会使处理时间增加数倍，因此一般情况下没有开启的必要。

我自己选择了几张 1200px 以上的高清二次元图片进行实验：先将原图缩小到 1/4，再使用 `realesrgan-x4plus-anime` 模型在使用或不使用 TTA 的情况下放大 4x，比较放大后图片和原图的 SSIM（范围为 0-1，值越大表示两张图越相似）。结果使用 TTA 的 SSIM 仅比不使用高出 0.002 左右，目视就更看不出差异了。

### 高级设定中的“针对 GIF 的透明色进行额外处理”是什么？

GIF 只支持最多 256 种 RGB 颜色的调色板并设定其中一种颜色为透明色（可选），也就是说不存在半透明的情况。对于存在透明部分的 GIF，这就出现了两个问题：

* 图像的 Alpha 通道只有 0 和 255 两个值，可以用只有黑白两色的图像表示，有严重的锯齿。
* 将 GIF 的每一帧拆出来保存为 PNG、WebP 等格式以后，透明部分在 RGB 通道上的颜色会变得不可预料。例如 GIF 中被设为透明色的颜色原本是 `#FFFFFF`，将帧另存为后可能会变成 `#000000`，虽然只看图片的话并没有区别。

对于使用 Real-ESRGAN 直接放大 GIF 的每一帧的做法（[示例](https://user-images.githubusercontent.com/47057319/170273973-d9743d66-d6df-42c2-8fe8-b123fa6edb98.gif)），上面两个问题的影响是：

* Real-ESRGAN 对 Alpha 通道放大的效果非常不理想，和使用常规缩放算法几乎没有区别，导致放大后的帧周围会出现一圈锯齿比较明显的杂边。
* 杂边的颜色是不可预料的，比如有些情况下是黑色，会显得非常难看。

这个选项就是针对这两个问题而添加的，启用后会添加以下操作：

* 在拆出 GIF 的每一帧时，强制把透明部分的颜色设为白色，这样可以将放大后的 GIF 的杂边颜色固定为白色，比较美观。
* 对于每一帧的 Alpha 通道，先添加半径 3px 的高斯模糊以平滑锯齿，然后应用一个增加对比度的曲线（或者是 LUT）以尽可能减小杂边的影响，再通过仿色算法处理为只有 0 和 255 两个值的黑白图像。

这个选项是实验性的，建议在放大存在透明部分的 GIF 时手动开启，在放大不存在透明部分的 GIF 时关闭。可能是由于这里的实现或 GIF 编码器对透明色的处理存在问题，在开启时处理后者会出现一些奇怪的问题（主要是出现不该出现的透明色以及仿色效果非常差）。也许会有更好的处理方法。

### 高级设定中的“使用有损压缩”、“有损压缩质量”和“自定义压缩/后期处理命令”是什么？

开启“使用有损压缩”以后，如果输出的文件是 JPEG 或 WebP 格式，就可以根据设定的值（0-100 表示从低质量到高质量）控制输出的文件的压缩质量了。如果输入的是文件夹，则放大文件夹中 JPEG 或 WebP 格式的图片时输出的压缩质量也会受这个选项影响。压缩使用 `image` crate 和 libwebp 完成。

不开启这个选项的话，输出为 WebP 格式时使用的是无损压缩。

如果设定了“自定义压缩/后期处理命令”，则不会进行上面的压缩操作。在这里你可以输入一条命令对放大后的图片进行压缩或其他的处理，还可以自定义命令中的参数。

* `{input}` 表示输入文件的路径。
* `{output}` 表示输出文件的路径。
* `{output:ext}` 表示输出文件的路径，但把扩展名修改为 `ext`。
* 命令示例：
    * 使用 [avifenc (libavif)](https://github.com/AOMediaCodec/libavif/blob/main/doc/avifenc.1.md) 转换为 AVIF 格式：`avifenc --speed 6 --jobs all --depth 8 --yuv 420 --min 0 --max 63 -a end-usage=q -a cq-level=30 -a enable-chroma-deltaq=1 --autotiling --ignore-icc --ignore-xmp --ignore-exif {input} {output:avif}`
    * 使用 [cjxl (libjxl)](https://github.com/libjxl/libjxl#usage) 转换为 JPEG XL 格式：`cjxl {input} {output:jxl} --quality=80 --effort=9 --progressive --verbose`
    * 使用 [gif2webp (libwebp)](https://developers.google.com/speed/webp/docs/gif2webp) 将输出的 GIF 转换为 WebP 格式：`gif2webp -lossy -q 80 -m 6 -min_size -mt -v {input} -o {output:webp}`
    * 使用 [ImageMagick](https://imagemagick.org/) 在右下角添加文字水印，然后转换为 AVIF 格式：`magick convert -fill white -pointsize 24 -gravity SouthEast -draw "text 16 16 'https://github.com/TransparentLC/realesrgan-gui'" -quality 80 {input} {output:avif}`

请忽略“基本设定”的“输出”的扩展名，实际的输出文件扩展名由设定的命令决定。

### 配置文件的保存位置

项目目录或打包后的可执行文件所在目录下的 `config.ini`，没有这个文件的情况下会使用默认的配置。在退出程序时会自动保存配置。

如果因为配置文件的问题导致程序不能运行的话，可以先尝试将配置文件删除。

### 已知限制

* Slint 的 winit 后端目前不支持接收操作系统层面的文件拖放，因此不能像 Tk 版本那样把文件或文件夹拖进窗口来设定输入路径。请使用「浏览」按钮选择文件，或直接在输入框中输入、粘贴文件（文件夹）的路径。
* 「浏览」按钮只能选择文件；需要处理整个文件夹时，请手动输入文件夹路径。
* 有损压缩输出为 WebP 时使用 libwebp，与 Pillow 的编码结果可能存在细微差异。
* 降采样方式中的 `Hamming` 会使用 `image` crate 中最接近的 `Triangle` 滤波器（`image` 没有提供 Hamming 滤波器）。
* 合并 GIF 时不再逐个调色板重排透明色索引（这是 Pillow 的浏览器兼容性变通做法），透明通道的量化由 GIF 编码器处理。

### 我觉得 Real-CUGAN 的放大效果比 Real-ESRGAN 更好

有些用户是这么认为的，所以我决定添加对 Real-CUGAN 的支持。

想要使用 Real-CUGAN 而不是 Real-ESRGAN 进行放大的话，请遵循以下步骤：

* 下载 [Real-CUGAN-ncnn-vulkan](https://github.com/nihui/realcugan-ncnn-vulkan)
* 在 `config.ini` 中将 `upscaler` 设定为 `realcugan-ncnn-vulkan[.exe]` 的路径
* 将 Real-CUGAN 的模型（`models-{nose,pro,se}` 三个文件夹）放入 `models` 文件夹（或是 `config.ini` 的 `modeldir` 指定的目录）

### 为什么不使用（其他功能类似的 GUI）？

Real-ESRGAN 的 GUI 当然不止一个，这里列出了其中的一部分，并且附上了我没有使用它们而是选择自己写一个 GUI 的理由。

#### [Waifu2x-Extension-GUI](https://github.com/AaronFeng753/Waifu2x-Extension-GUI) ![](https://img.shields.io/github/stars/AaronFeng753/Waifu2x-Extension-GUI)

这是一个“全家桶”类型的工具箱，它不仅整合了用于图像放大的 waifu2x、Anime4k、Real-SR、SRMD、Real-ESRGAN、Real-CUGAN……还包含了用于视频插帧的 CAIN、DAIN、RIFE，以及 ffmpeg、ImageMagick、gifsicle、nircmd、wget 等辅助工具。只支持 Windows。

不过，功能繁多也使它的界面和设定都变得非常复杂，但其中的大部分功能并不是我会用到的。在它是开源软件的时候我曾经是它的用户，但作者从 2021 年 5 月的 v3.41.01 开始[修改了 LICENSE](https://github.com/AaronFeng753/Waifu2x-Extension-GUI/commit/37a4b53d754bf58f740df0d9043f726c062efa71) 转为[闭源](https://github.com/AaronFeng753/Waifu2x-Extension-GUI/commit/38b13ed886f50d861798e8cad3cc056f04173415)，并会在每次启动和处理完成时显示购买高级版的广告。

虽然我并不依赖那些高级版限定的功能，但这些改变还是使我产生了编写符合自己的需求的轻量级 GUI（也就是这个项目）的想法。

#### [upscayl](https://github.com/upscayl/upscayl) ![](https://img.shields.io/github/stars/upscayl/upscayl)

用 Electron 实现的 GUI，同样实现了跨平台。由于使用的是前端技术，界面和交互都很容易做得非常出色，甚至还有将原图和放大后的图片进行对比的功能。文档也非常详细。

不过它仍然缺少了一些功能，例如处理 GIF、自定义后处理命令等。另外，它目前[还没有多语言支持](https://github.com/upscayl/upscayl/issues/91)。

另外，因为它是使用 Electron 实现的，所以它的用户又要[多安装一个 Chromium 内核了](https://github.com/ShirasawaSama/CefDetector/raw/master/screenshot.png)😂 Upscayl 的大小约为 400 MB，相比之下 Real-ESRGAN GUI 仅有 10 MB 左右（Windows 版，排除 Real-ESRGAN-ncnn-vulkan 主程序和模型）。

#### [tsukumijima/Real-ESRGAN-GUI](https://github.com/tsukumijima/Real-ESRGAN-GUI) ![](https://img.shields.io/github/stars/tsukumijima/Real-ESRGAN-GUI)、[net2cn/Real-ESRGAN_GUI](https://github.com/net2cn/Real-ESRGAN_GUI) ![](https://img.shields.io/github/stars/net2cn/Real-ESRGAN_GUI)、[upscale-rs](https://github.com/oloko64/upscale-rs) ![](https://img.shields.io/github/stars/oloko64/upscale-rs)、[Real-ESRGAN-EGUI](https://github.com/WGzeyu/Real-ESRGAN-EGUI) ![](https://img.shields.io/github/stars/WGzeyu/Real-ESRGAN-EGUI) ……

这些 GUI 基本上都是对 CLI 参数的简单封装，没有多少额外的功能。

不过，我很喜欢 tsukumijima/Real-ESRGAN-GUI 使用的 Material Design 风格，它也支持 Real-CUGAN。

## 借物表

* [Slint](https://slint.dev/) 跨平台 GUI 框架，使用其内置的 Fluent Design 控件风格
* [image](https://github.com/image-rs/image) 图片编解码与缩放
* [webp](https://github.com/jaredforth/webp) libwebp 绑定，用于有损 WebP 编码
* [rfd](https://github.com/PolyMeilex/rfd) 原生文件选择对话框
* [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) 原版工具

## 致谢

感谢 [@blacklein](https://github.com/blacklein) 和 [@hyrulelinks](https://github.com/hyrulelinks) 提供在 macOS 下使用这个 GUI 和打包为应用程序的帮助，还有其他的贡献者们！

[![Contributors](https://contrib.rocks/image?repo=TransparentLC/realesrgan-gui)](https://github.com/TransparentLC/realesrgan-gui/graphs/contributors)

## Star history

<a href="https://star-history.com/#TransparentLC/realesrgan-gui&Date">
  <picture>
    <source media="(prefers-color-scheme:dark)" srcset="https://api.star-history.com/svg?repos=TransparentLC/realesrgan-gui&type=Date&theme=dark" />
    <source media="(prefers-color-scheme:light)" srcset="https://api.star-history.com/svg?repos=TransparentLC/realesrgan-gui&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=TransparentLC/realesrgan-gui&type=Date" />
  </picture>
</a>
