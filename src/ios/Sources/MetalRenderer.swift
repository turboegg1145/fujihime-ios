import CoreGraphics
import Foundation
import Metal
import MetalKit
import simd

struct ViewportUniforms {
    var scale: SIMD2<Float>
    var offset: SIMD2<Float>
}

final class MetalRenderer: NSObject, MTKViewDelegate {
    let device: MTLDevice
    private let commandQueue: MTLCommandQueue
    private let pipelineState: MTLRenderPipelineState
    private let gameTexture: MTLTexture
    private var pixelBuffer: [UInt8]
    private var uniforms = ViewportUniforms(scale: SIMD2<Float>(1.0, 1.0), offset: SIMD2<Float>(0.0, 0.0))

    init?(mtkView: MTKView) {
        guard let dev = mtkView.device ?? MTLCreateSystemDefaultDevice(),
              let queue = dev.makeCommandQueue(),
              let library = dev.makeDefaultLibrary(),
              let vertexFn = library.makeFunction(name: "fujihimeVertex"),
              let fragmentFn = library.makeFunction(name: "fujihimeFragment") else {
            return nil
        }

        self.device = dev
        self.commandQueue = queue
        self.pixelBuffer = [UInt8](repeating: 0, count: EngineBridge.presentBGRA8Length)

        let pipelineDesc = MTLRenderPipelineDescriptor()
        pipelineDesc.vertexFunction = vertexFn
        pipelineDesc.fragmentFunction = fragmentFn
        pipelineDesc.colorAttachments[0].pixelFormat = .bgra8Unorm

        do {
            self.pipelineState = try dev.makeRenderPipelineState(descriptor: pipelineDesc)
        } catch {
            NSLog("[FujihimeMetal] Failed to create pipeline state: %@", error.localizedDescription)
            return nil
        }

        let texDesc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm,
            width: EngineBridge.gameWidth,
            height: EngineBridge.gameHeight,
            mipmapped: false
        )
        texDesc.usage = [.shaderRead]
        texDesc.storageMode = .shared

        guard let texture = dev.makeTexture(descriptor: texDesc) else {
            return nil
        }
        self.gameTexture = texture

        super.init()

        mtkView.device = dev
        mtkView.colorPixelFormat = .bgra8Unorm
        mtkView.clearColor = MTLClearColor(red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0)
        mtkView.framebufferOnly = true
        mtkView.isPaused = true
        mtkView.enableSetNeedsDisplay = false
        mtkView.delegate = self

        updateUniforms(forDrawableSize: mtkView.drawableSize)
    }

    func updateTexture(from bridge: EngineBridge) {
        guard bridge.copyPresentBGRA8(into: &pixelBuffer) else { return }
        let region = MTLRegionMake2D(0, 0, EngineBridge.gameWidth, EngineBridge.gameHeight)
        let bytesPerRow = EngineBridge.gameWidth * 4
        pixelBuffer.withUnsafeBytes { rawBuf in
            guard let base = rawBuf.baseAddress else { return }
            gameTexture.replace(region: region, mipmapLevel: 0, withBytes: base, bytesPerRow: bytesPerRow)
        }
    }

    func gameViewportRect(in bounds: CGRect) -> CGRect {
        guard bounds.width > 0, bounds.height > 0 else { return .zero }
        let targetAspect = CGFloat(EngineBridge.gameWidth) / CGFloat(EngineBridge.gameHeight)
        let viewAspect = bounds.width / bounds.height

        if viewAspect > targetAspect {
            let rectH = bounds.height
            let rectW = rectH * targetAspect
            let rectX = bounds.minX + (bounds.width - rectW) * 0.5
            return CGRect(x: rectX, y: bounds.minY, width: rectW, height: rectH)
        } else {
            let rectW = bounds.width
            let rectH = rectW / targetAspect
            let rectY = bounds.minY + (bounds.height - rectH) * 0.5
            return CGRect(x: bounds.minX, y: rectY, width: rectW, height: rectH)
        }
    }

    func viewPointToGamePoint(_ pt: CGPoint, in bounds: CGRect) -> (Int32, Int32)? {
        let rect = gameViewportRect(in: bounds)
        guard rect.width > 0, rect.height > 0, rect.contains(pt) else {
            return nil
        }
        return mapPoint(pt, inGameRect: rect)
    }

    func clampedViewPointToGamePoint(_ pt: CGPoint, in bounds: CGRect) -> (Int32, Int32) {
        let rect = gameViewportRect(in: bounds)
        guard rect.width > 0, rect.height > 0 else { return (0, 0) }
        return mapPoint(pt, inGameRect: rect)
    }

    private func mapPoint(_ pt: CGPoint, inGameRect rect: CGRect) -> (Int32, Int32) {
        let normX = (pt.x - rect.minX) / rect.width
        let normY = (pt.y - rect.minY) / rect.height
        let rawX = Int32(normX * CGFloat(EngineBridge.gameWidth))
        let rawY = Int32(normY * CGFloat(EngineBridge.gameHeight))
        let gx = max(0, min(Int32(EngineBridge.gameWidth - 1), rawX))
        let gy = max(0, min(Int32(EngineBridge.gameHeight - 1), rawY))
        return (gx, gy)
    }

    private func updateUniforms(forDrawableSize size: CGSize) {
        guard size.width > 0, size.height > 0 else {
            uniforms = ViewportUniforms(scale: SIMD2<Float>(1.0, 1.0), offset: SIMD2<Float>(0.0, 0.0))
            return
        }
        let targetAspect = Float(EngineBridge.gameWidth) / Float(EngineBridge.gameHeight)
        let viewAspect = Float(size.width / size.height)

        if viewAspect > targetAspect {
            uniforms = ViewportUniforms(
                scale: SIMD2<Float>(targetAspect / viewAspect, 1.0),
                offset: SIMD2<Float>(0.0, 0.0)
            )
        } else {
            uniforms = ViewportUniforms(
                scale: SIMD2<Float>(1.0, viewAspect / targetAspect),
                offset: SIMD2<Float>(0.0, 0.0)
            )
        }
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        updateUniforms(forDrawableSize: size)
    }

    func draw(in view: MTKView) {
        updateUniforms(forDrawableSize: view.drawableSize)
        guard let renderPassDesc = view.currentRenderPassDescriptor,
              let drawable = view.currentDrawable,
              let commandBuffer = commandQueue.makeCommandBuffer(),
              let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: renderPassDesc) else {
            return
        }

        encoder.setRenderPipelineState(pipelineState)
        encoder.setVertexBytes(&uniforms, length: MemoryLayout<ViewportUniforms>.stride, index: 0)
        encoder.setFragmentTexture(gameTexture, index: 0)
        encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        encoder.endEncoding()

        commandBuffer.present(drawable)
        commandBuffer.commit()
    }
}
