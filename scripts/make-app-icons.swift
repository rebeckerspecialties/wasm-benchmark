// Renders the app icons into the platform asset catalogs under apps/:
// iOS and watchOS (one opaque 1024 px icon), tvOS (layered App Icon and App
// Store icon, Top Shelf images) and visionOS (three-layer solid image stack).
//
// The icon is a speedometer gauge on WebAssembly purple, drawn in three
// layers — background, dial, needle — which tvOS and visionOS use for their
// parallax and depth effects; iOS and watchOS get the layers flattened.
//
// Usage: swift scripts/make-app-icons.swift   (from the repository root)

import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

enum Layer: CaseIterable { case back, middle, front }

let apps = URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("apps")

func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r, green: g, blue: b, alpha: a)
}

let purpleLight = rgb(0.494, 0.404, 1.000)  // #7E67FF
let purple = rgb(0.396, 0.310, 0.941)       // #654FF0, WebAssembly purple
let purpleDeep = rgb(0.165, 0.098, 0.478)   // #2A197A
let white = rgb(1, 1, 1)

/// Draws `layers` of the gauge into `ctx` over `rect`; the dial is sized
/// from the rect's shorter side and centered at `center`.
func drawGauge(_ ctx: CGContext, rect: CGRect, center: CGPoint, radius r: CGFloat, layers: Set<Layer>) {
    if layers.contains(.back) {
        let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                                  colors: [purpleLight, purple, purpleDeep] as CFArray,
                                  locations: [0, 0.45, 1])!
        ctx.drawLinearGradient(gradient,
                               start: CGPoint(x: rect.minX, y: rect.maxY),
                               end: CGPoint(x: rect.maxX, y: rect.minY),
                               options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
    }
    // Angles are CoreGraphics radians (counterclockwise from +x, y up).
    let start = CGFloat.pi * 1.25   // 225°, lower left
    let end = -CGFloat.pi * 0.25    // -45°, lower right
    let reading = CGFloat.pi * 0.2  // needle at 36°: fast
    if layers.contains(.middle) {
        ctx.setLineCap(.round)
        ctx.setLineWidth(r * 0.2)
        ctx.setStrokeColor(rgb(1, 1, 1, 0.22))
        ctx.addArc(center: center, radius: r, startAngle: start, endAngle: end, clockwise: true)
        ctx.strokePath()
        ctx.setStrokeColor(white)
        ctx.addArc(center: center, radius: r, startAngle: start, endAngle: reading, clockwise: true)
        ctx.strokePath()
        // Ticks inside the arc.
        ctx.setLineWidth(r * 0.045)
        ctx.setStrokeColor(rgb(1, 1, 1, 0.75))
        for i in 0...8 {
            let a = start - (start - end) * CGFloat(i) / 8
            let inner = r * 0.62, outer = r * 0.74
            ctx.move(to: CGPoint(x: center.x + cos(a) * inner, y: center.y + sin(a) * inner))
            ctx.addLine(to: CGPoint(x: center.x + cos(a) * outer, y: center.y + sin(a) * outer))
        }
        ctx.strokePath()
    }
    if layers.contains(.front) {
        // Tapered needle with a hub.
        let tip = CGPoint(x: center.x + cos(reading) * r * 0.86, y: center.y + sin(reading) * r * 0.86)
        let side = reading + .pi / 2
        let w = r * 0.085
        ctx.setShadow(offset: CGSize(width: 0, height: -r * 0.03), blur: r * 0.08, color: rgb(0.08, 0.03, 0.25, 0.45))
        ctx.setFillColor(white)
        ctx.move(to: tip)
        ctx.addLine(to: CGPoint(x: center.x + cos(side) * w, y: center.y + sin(side) * w))
        ctx.addLine(to: CGPoint(x: center.x - cos(reading) * r * 0.1, y: center.y - sin(reading) * r * 0.1))
        ctx.addLine(to: CGPoint(x: center.x - cos(side) * w, y: center.y - sin(side) * w))
        ctx.closePath()
        ctx.fillPath()
        ctx.fillEllipse(in: CGRect(x: center.x - r * 0.15, y: center.y - r * 0.15, width: r * 0.3, height: r * 0.3))
        ctx.setShadow(offset: .zero, blur: 0, color: nil)
        ctx.setFillColor(purple)
        ctx.fillEllipse(in: CGRect(x: center.x - r * 0.065, y: center.y - r * 0.065, width: r * 0.13, height: r * 0.13))
    }
}

func drawTitle(_ ctx: CGContext, _ text: String, size: CGFloat, weight: CGFloat, at point: CGPoint, alpha: CGFloat = 1) {
    let base = CTFontCreateUIFontForLanguage(.system, size, nil)!
    let traits = [kCTFontWeightTrait: weight] as CFDictionary
    let descriptor = CTFontDescriptorCreateWithAttributes([kCTFontTraitsAttribute: traits] as CFDictionary)
    let font = CTFontCreateCopyWithAttributes(base, size, nil, descriptor)
    let attributes = [kCTFontAttributeName: font, kCTForegroundColorAttributeName: rgb(1, 1, 1, alpha)] as CFDictionary
    let line = CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, text as CFString, attributes)!)
    ctx.textPosition = point
    CTLineDraw(line, ctx)
}

/// Renders `width` x `height` pixels; opaque images carry no alpha channel,
/// as App Store icons must not.
func render(_ width: Int, _ height: Int, opaque: Bool, _ draw: (CGContext, CGRect) -> Void) -> CGImage {
    let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: (opaque ? CGImageAlphaInfo.noneSkipLast : .premultipliedLast).rawValue)!
    ctx.interpolationQuality = .high
    ctx.setShouldAntialias(true)
    draw(ctx, CGRect(x: 0, y: 0, width: width, height: height))
    return ctx.makeImage()!
}

/// PNG, or JPEG for a `.jpg` URL (the Top Shelf images: their gradients
/// run to megabytes as PNG).
func write(_ image: CGImage, _ url: URL) {
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let jpeg = url.pathExtension == "jpg"
    let type = jpeg ? UTType.jpeg : UTType.png
    let dest = CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil)!
    let options = jpeg ? [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary : nil
    CGImageDestinationAddImage(dest, image, options)
    precondition(CGImageDestinationFinalize(dest), "could not write \(url.path)")
}

func json(_ object: Any, _ url: URL) {
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    try! (data + Data("\n".utf8)).write(to: url)
}

let info = ["author": "xcode", "version": 1] as [String: Any]

/// Square icon layers: the dial fills most of the canvas.
func squareIcon(_ size: Int, layers: Set<Layer>, opaque: Bool) -> CGImage {
    render(size, size, opaque: opaque) { ctx, rect in
        let s = CGFloat(size)
        drawGauge(ctx, rect: rect, center: CGPoint(x: s * 0.5, y: s * 0.47), radius: s * 0.3, layers: layers)
    }
}

// iOS and watchOS: one flattened, opaque 1024 px icon each.
for (dir, platform) in [("WasmBenchmarkIOS", "ios"), ("WasmBenchmarkWatch", "watchos")] {
    let set = apps.appendingPathComponent("\(dir)/Assets.xcassets/AppIcon.appiconset")
    write(squareIcon(1024, layers: Set(Layer.allCases), opaque: true), set.appendingPathComponent("AppIcon.png"))
    json(["images": [["filename": "AppIcon.png", "idiom": "universal", "platform": platform, "size": "1024x1024"]],
          "info": info], set.appendingPathComponent("Contents.json"))
    json(["info": info], apps.appendingPathComponent("\(dir)/Assets.xcassets/Contents.json"))
}

// visionOS: a three-layer solid image stack, 1024 px per layer; the back
// layer is opaque.
do {
    let stack = apps.appendingPathComponent("WasmBenchmarkVision/Assets.xcassets/AppIcon.solidimagestack")
    let names: [(Layer, String)] = [(.front, "Front"), (.middle, "Middle"), (.back, "Back")]
    for (layer, name) in names {
        let dir = stack.appendingPathComponent("\(name).solidimagestacklayer")
        write(squareIcon(1024, layers: [layer], opaque: layer == .back), dir.appendingPathComponent("Content.imageset/\(name).png"))
        json(["images": [["filename": "\(name).png", "idiom": "vision", "scale": "2x"]], "info": info],
             dir.appendingPathComponent("Content.imageset/Contents.json"))
        json(["info": info], dir.appendingPathComponent("Contents.json"))
    }
    json(["info": info, "layers": names.map { ["filename": "\($0.1).solidimagestacklayer"] }],
         stack.appendingPathComponent("Contents.json"))
    json(["info": info], apps.appendingPathComponent("WasmBenchmarkVision/Assets.xcassets/Contents.json"))
}

// tvOS: layered 400x240 pt App Icon (1x, 2x), layered 1280x768 App Store
// icon, and the Top Shelf images.
do {
    let brand = apps.appendingPathComponent("WasmBenchmarkTV/Assets.xcassets/App Icon & Top Shelf Image.brandassets")
    func tvLayer(_ w: Int, _ h: Int, _ layer: Layer) -> CGImage {
        render(w, h, opaque: layer == .back) { ctx, rect in
            let hh = CGFloat(h)
            drawGauge(ctx, rect: rect, center: CGPoint(x: rect.midX, y: hh * 0.46), radius: hh * 0.3, layers: [layer])
        }
    }
    let names: [(Layer, String)] = [(.front, "Front"), (.middle, "Middle"), (.back, "Back")]
    for (stackName, w, h, scales) in [("App Icon", 400, 240, [1, 2]), ("App Icon - App Store", 1280, 768, [1])] {
        let stack = brand.appendingPathComponent("\(stackName).imagestack")
        for (layer, name) in names {
            let dir = stack.appendingPathComponent("\(name).imagestacklayer")
            var images: [[String: Any]] = []
            for scale in scales {
                let file = scale == 1 ? "\(name).png" : "\(name)@\(scale)x.png"
                write(tvLayer(w * scale, h * scale, layer), dir.appendingPathComponent("Content.imageset/\(file)"))
                images.append(["filename": file, "idiom": "tv", "scale": "\(scale)x"])
            }
            json(["images": images, "info": info], dir.appendingPathComponent("Content.imageset/Contents.json"))
            json(["info": info], dir.appendingPathComponent("Contents.json"))
        }
        json(["info": info, "layers": names.map { ["filename": "\($0.1).imagestacklayer"] }],
             stack.appendingPathComponent("Contents.json"))
    }
    for (setName, w, h) in [("Top Shelf Image", 1920, 720), ("Top Shelf Image Wide", 2320, 720)] {
        let set = brand.appendingPathComponent("\(setName).imageset")
        var images: [[String: Any]] = []
        for scale in [1, 2] {
            let file = scale == 1 ? "TopShelf.jpg" : "TopShelf@2x.jpg"
            let image = render(w * scale, h * scale, opaque: true) { ctx, rect in
                let hh = CGFloat(h * scale)
                drawGauge(ctx, rect: rect, center: CGPoint(x: rect.width * 0.2, y: hh * 0.46), radius: hh * 0.27, layers: Set(Layer.allCases))
                drawTitle(ctx, "WasmBench", size: hh * 0.17, weight: 0.4, at: CGPoint(x: rect.width * 0.36, y: hh * 0.5))
                drawTitle(ctx, "WebAssembly interpreters, head to head", size: hh * 0.065, weight: 0.0,
                          at: CGPoint(x: rect.width * 0.36 + hh * 0.01, y: hh * 0.36), alpha: 0.8)
            }
            write(image, set.appendingPathComponent(file))
            images.append(["filename": file, "idiom": "tv", "scale": "\(scale)x"])
        }
        json(["images": images, "info": info], set.appendingPathComponent("Contents.json"))
    }
    json(["assets": [
        ["filename": "App Icon - App Store.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "1280x768"],
        ["filename": "App Icon.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "400x240"],
        ["filename": "Top Shelf Image Wide.imageset", "idiom": "tv", "role": "top-shelf-image-wide", "size": "2320x720"],
        ["filename": "Top Shelf Image.imageset", "idiom": "tv", "role": "top-shelf-image", "size": "1920x720"],
    ], "info": info], brand.appendingPathComponent("Contents.json"))
    json(["info": info], apps.appendingPathComponent("WasmBenchmarkTV/Assets.xcassets/Contents.json"))
}

print("icons written under \(apps.path)")
