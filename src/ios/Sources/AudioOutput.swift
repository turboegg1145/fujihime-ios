import AVFoundation
import Foundation

final class AudioOutput {
    private static let renderChunkFrames = 4096

    private let audioEngine = AVAudioEngine()
    private var sourceNode: AVAudioSourceNode?
    private let bridge: EngineBridge
    private var isPausedByApp = false

    private var interruptionObserver: NSObjectProtocol?
    private var routeChangeObserver: NSObjectProtocol?

    init(bridge: EngineBridge) {
        self.bridge = bridge
        setupNotifications()
    }

    deinit {
        stop()
        if let obs = interruptionObserver {
            NotificationCenter.default.removeObserver(obs)
        }
        if let obs = routeChangeObserver {
            NotificationCenter.default.removeObserver(obs)
        }
    }

    func start() {
        configureAudioSession()

        guard let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: EngineBridge.sampleRate,
            channels: 2,
            interleaved: false
        ) else {
            return
        }

        if let existing = sourceNode {
            audioEngine.detach(existing)
            sourceNode = nil
        }

        // Pre-allocated scratch buffer captured by the render callback to avoid per-callback heap allocations.
        var scratchInterleaved = [Float](repeating: 0.0, count: Self.renderChunkFrames * 2)
        let weakBridge = bridge

        let node = AVAudioSourceNode(format: format) { [weak weakBridge] _, _, frameCount, audioBufferList -> OSStatus in
            let totalFrames = Int(frameCount)
            let ablPointer = UnsafeMutableAudioBufferListPointer(audioBufferList)

            guard let engineBridge = weakBridge, totalFrames > 0 else {
                for buf in ablPointer {
                    if let mData = buf.mData {
                        memset(mData, 0, Int(buf.mDataByteSize))
                    }
                }
                return noErr
            }

            if ablPointer.count >= 2,
               let leftBase = ablPointer[0].mData?.assumingMemoryBound(to: Float.self),
               let rightBase = ablPointer[1].mData?.assumingMemoryBound(to: Float.self) {
                var offset = 0
                while offset < totalFrames {
                    let chunk = min(Self.renderChunkFrames, totalFrames - offset)
                    let rendered = scratchInterleaved.withUnsafeMutableBufferPointer { bufPtr -> Int in
                        guard let base = bufPtr.baseAddress else { return 0 }
                        return engineBridge.renderAudioF32(into: base, frameCount: chunk)
                    }
                    let valid = min(rendered, chunk)
                    for i in 0..<valid {
                        leftBase[offset + i] = scratchInterleaved[i * 2]
                        rightBase[offset + i] = scratchInterleaved[i * 2 + 1]
                    }
                    if valid < chunk {
                        for i in valid..<chunk {
                            leftBase[offset + i] = 0.0
                            rightBase[offset + i] = 0.0
                        }
                    }
                    offset += chunk
                }
            } else if ablPointer.count == 1,
                      let monoOrInterleaved = ablPointer[0].mData?.assumingMemoryBound(to: Float.self) {
                let channels = Int(ablPointer[0].mNumberChannels)
                var offset = 0
                while offset < totalFrames {
                    let chunk = min(Self.renderChunkFrames, totalFrames - offset)
                    let rendered = scratchInterleaved.withUnsafeMutableBufferPointer { bufPtr -> Int in
                        guard let base = bufPtr.baseAddress else { return 0 }
                        return engineBridge.renderAudioF32(into: base, frameCount: chunk)
                    }
                    let valid = min(rendered, chunk)
                    if channels >= 2 {
                        for i in 0..<valid {
                            monoOrInterleaved[(offset + i) * channels] = scratchInterleaved[i * 2]
                            monoOrInterleaved[(offset + i) * channels + 1] = scratchInterleaved[i * 2 + 1]
                        }
                    } else {
                        for i in 0..<valid {
                            monoOrInterleaved[offset + i] = 0.5 * (scratchInterleaved[i * 2] + scratchInterleaved[i * 2 + 1])
                        }
                    }
                    if valid < chunk {
                        for i in valid..<chunk {
                            for ch in 0..<max(channels, 1) {
                                monoOrInterleaved[(offset + i) * max(channels, 1) + ch] = 0.0
                            }
                        }
                    }
                    offset += chunk
                }
            }

            return noErr
        }

        sourceNode = node
        audioEngine.attach(node)
        audioEngine.connect(node, to: audioEngine.mainMixerNode, format: format)

        isPausedByApp = false
        startEngineIfNeeded()
    }

    func pause() {
        isPausedByApp = true
        if audioEngine.isRunning {
            audioEngine.pause()
        }
    }

    func resume() {
        isPausedByApp = false
        configureAudioSession()
        startEngineIfNeeded()
    }

    func stop() {
        isPausedByApp = true
        if audioEngine.isRunning {
            audioEngine.stop()
        }
        if let node = sourceNode {
            audioEngine.detach(node)
            sourceNode = nil
        }
    }

    private func configureAudioSession() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default, options: [])
            try session.setPreferredSampleRate(EngineBridge.sampleRate)
            try session.setActive(true)
        } catch {
            NSLog("[FujihimeAudio] AVAudioSession config error: %@", error.localizedDescription)
        }
    }

    private func startEngineIfNeeded() {
        guard !isPausedByApp, !audioEngine.isRunning else { return }
        do {
            try audioEngine.start()
        } catch {
            NSLog("[FujihimeAudio] AVAudioEngine start error: %@", error.localizedDescription)
        }
    }

    private func setupNotifications() {
        let nc = NotificationCenter.default
        interruptionObserver = nc.addObserver(
            forName: AVAudioSession.interruptionNotification,
            object: AVAudioSession.sharedInstance(),
            queue: .main
        ) { [weak self] notification in
            self?.handleInterruption(notification)
        }

        routeChangeObserver = nc.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: AVAudioSession.sharedInstance(),
            queue: .main
        ) { [weak self] notification in
            self?.handleRouteChange(notification)
        }
    }

    private func handleInterruption(_ notification: Notification) {
        guard let userInfo = notification.userInfo,
              let typeRaw = userInfo[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: typeRaw) else {
            return
        }

        switch type {
        case .began:
            if audioEngine.isRunning {
                audioEngine.pause()
            }
        case .ended:
            let optionsRaw = userInfo[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            let options = AVAudioSession.InterruptionOptions(rawValue: optionsRaw)
            if options.contains(.shouldResume) && !isPausedByApp {
                configureAudioSession()
                startEngineIfNeeded()
            }
        @unknown default:
            break
        }
    }

    private func handleRouteChange(_ notification: Notification) {
        guard !isPausedByApp else { return }
        if !audioEngine.isRunning {
            configureAudioSession()
            startEngineIfNeeded()
        }
    }
}
