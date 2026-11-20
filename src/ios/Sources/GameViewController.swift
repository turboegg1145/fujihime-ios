import Foundation
import MetalKit
import UIKit

final class GameViewController: UIViewController {
    private static let longPressDelaySeconds: TimeInterval = 0.45
    private static let longPressMoveTolerancePt: CGFloat = 16.0
    private static let panWheelThresholdPt: CGFloat = 28.0

    private let engineBridge = EngineBridge()
    private var audioOutput: AudioOutput?
    private var mtkView: MTKView!
    private var renderer: MetalRenderer?
    private var displayLink: CADisplayLink?
    private var lastFrameTimestamp: CFTimeInterval = 0.0

    private var activeTouch: UITouch?
    private var touchStartPoint: CGPoint = .zero
    private var touchSentDown = false
    private var multiTouchActive = false
    private var longPressFired = false
    private var longPressWorkItem: DispatchWorkItem?
    private var panAccumulatedY: CGFloat = 0.0

    private var isPresentingAlert = false
    private var missingAssetsAlertShown = false
    private var isPaused = false

    override var prefersStatusBarHidden: Bool { true }
    override var prefersHomeIndicatorAutoHidden: Bool { true }
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask { .landscape }
    override var shouldAutorotate: Bool { true }
    override var canBecomeFirstResponder: Bool { true }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        let metalView = MTKView(frame: view.bounds, device: MTLCreateSystemDefaultDevice())
        metalView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        metalView.backgroundColor = .black
        metalView.isMultipleTouchEnabled = true
        view.addSubview(metalView)
        self.mtkView = metalView

        if let r = MetalRenderer(mtkView: metalView) {
            self.renderer = r
        } else {
            NSLog("[Fujihime] Metal device unavailable on this device")
        }

        setupGestureRecognizers(on: metalView)

        if engineBridge.hasGameAssets {
            let audio = AudioOutput(bridge: engineBridge)
            self.audioOutput = audio
            audio.start()
            startDisplayLink()
        }
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        becomeFirstResponder()

        if !engineBridge.hasGameAssets && !missingAssetsAlertShown {
            missingAssetsAlertShown = true
            presentMissingAssetsAlert()
        }
    }

    deinit {
        shutdown()
    }

    // MARK: - Lifecycle

    func pauseForBackground() {
        guard !isPaused else { return }
        isPaused = true
        displayLink?.isPaused = true
        audioOutput?.pause()
        cancelLongPressAndReleaseSkip()
        if engineBridge.hasGameAssets {
            _ = engineBridge.saveGlobal()
        }
        UIApplication.shared.isIdleTimerDisabled = false
    }

    func resumeFromBackground() {
        guard isPaused else { return }
        isPaused = false
        lastFrameTimestamp = 0.0
        if engineBridge.hasGameAssets {
            audioOutput?.resume()
            displayLink?.isPaused = false
        }
        becomeFirstResponder()
    }

    func shutdown() {
        displayLink?.invalidate()
        displayLink = nil
        audioOutput?.stop()
        audioOutput = nil
        cancelLongPressAndReleaseSkip()
        if engineBridge.hasGameAssets {
            _ = engineBridge.saveGlobal()
        }
        engineBridge.destroy()
        UIApplication.shared.isIdleTimerDisabled = false
    }

    private func startDisplayLink() {
        guard displayLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(onDisplayLink(_:)))
        link.preferredFramesPerSecond = 60
        link.add(to: .main, forMode: .common)
        self.displayLink = link
    }

    @objc private func onDisplayLink(_ link: CADisplayLink) {
        guard !isPaused, engineBridge.hasGameAssets else { return }

        let now = link.timestamp
        let deltaMs: UInt32
        if lastFrameTimestamp <= 0.0 {
            deltaMs = 16
        } else {
            let rawMs = (now - lastFrameTimestamp) * 1000.0
            deltaMs = UInt32(max(1.0, min(100.0, rawMs)))
        }
        lastFrameTimestamp = now

        _ = engineBridge.tick(deltaMs: deltaMs)

        if let r = renderer {
            r.updateTexture(from: engineBridge)
            mtkView.draw()
        }

        if let state = engineBridge.getState() {
            UIApplication.shared.isIdleTimerDisabled = state.is_playing_video
            if state.exit_requested {
                _ = engineBridge.saveGlobal()
                _ = engineBridge.startTitle()
            }
        }

        pollAndPresentEngineDialog()
    }

    // MARK: - Gestures & Single-Finger Touch

    private func setupGestureRecognizers(on targetView: UIView) {
        let threeFingerTap = UITapGestureRecognizer(target: self, action: #selector(handleThreeFingerTap(_:)))
        threeFingerTap.numberOfTouchesRequired = 3
        threeFingerTap.cancelsTouchesInView = false
        threeFingerTap.delaysTouchesBegan = false
        threeFingerTap.delaysTouchesEnded = false
        targetView.addGestureRecognizer(threeFingerTap)

        let twoFingerTap = UITapGestureRecognizer(target: self, action: #selector(handleTwoFingerTap(_:)))
        twoFingerTap.numberOfTouchesRequired = 2
        twoFingerTap.cancelsTouchesInView = false
        twoFingerTap.delaysTouchesBegan = false
        twoFingerTap.delaysTouchesEnded = false
        twoFingerTap.require(toFail: threeFingerTap)
        targetView.addGestureRecognizer(twoFingerTap)

        let twoFingerPan = UIPanGestureRecognizer(target: self, action: #selector(handleTwoFingerPan(_:)))
        twoFingerPan.minimumNumberOfTouches = 2
        twoFingerPan.maximumNumberOfTouches = 2
        twoFingerPan.cancelsTouchesInView = false
        twoFingerPan.delaysTouchesBegan = false
        twoFingerPan.delaysTouchesEnded = false
        targetView.addGestureRecognizer(twoFingerPan)
    }

    @objc private func handleTwoFingerTap(_ recognizer: UITapGestureRecognizer) {
        guard recognizer.state == .ended, engineBridge.hasGameAssets else { return }
        engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_RIGHT_UP, x: 400, y: 300)
    }

    @objc private func handleThreeFingerTap(_ recognizer: UITapGestureRecognizer) {
        guard recognizer.state == .ended else { return }
        presentQuickMenu()
    }

    @objc private func handleTwoFingerPan(_ recognizer: UIPanGestureRecognizer) {
        guard engineBridge.hasGameAssets else { return }
        switch recognizer.state {
        case .began:
            panAccumulatedY = 0.0
        case .changed:
            let translation = recognizer.translation(in: mtkView)
            recognizer.setTranslation(.zero, in: mtkView)
            panAccumulatedY += translation.y
            while panAccumulatedY >= Self.panWheelThresholdPt {
                panAccumulatedY -= Self.panWheelThresholdPt
                engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_WHEEL_UP, x: 400, y: 300)
            }
            while panAccumulatedY <= -Self.panWheelThresholdPt {
                panAccumulatedY += Self.panWheelThresholdPt
                engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_WHEEL_DOWN, x: 400, y: 300)
            }
        case .ended, .cancelled, .failed:
            panAccumulatedY = 0.0
        default:
            break
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard engineBridge.hasGameAssets else { return }
        let totalTouches = event?.allTouches?.count ?? touches.count

        if totalTouches >= 2 {
            cancelActiveSingleTouchForMultiTouch()
            return
        }

        guard touches.count == 1, let touch = touches.first, let r = renderer else { return }
        let pt = touch.location(in: mtkView)
        guard let (gx, gy) = r.viewPointToGamePoint(pt, in: mtkView.bounds) else {
            return
        }

        activeTouch = touch
        touchStartPoint = pt
        touchSentDown = true
        multiTouchActive = false
        longPressFired = false

        engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_MOVE, x: gx, y: gy)
        engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_DOWN, x: gx, y: gy)
        scheduleLongPressTimer(for: touch)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard engineBridge.hasGameAssets else { return }
        if (event?.allTouches?.count ?? 1) >= 2 {
            cancelActiveSingleTouchForMultiTouch()
            return
        }

        guard !multiTouchActive,
              let tracked = activeTouch,
              touches.contains(tracked),
              let r = renderer else {
            return
        }

        let pt = tracked.location(in: mtkView)
        let dx = pt.x - touchStartPoint.x
        let dy = pt.y - touchStartPoint.y
        if hypot(dx, dy) >= Self.longPressMoveTolerancePt {
            longPressWorkItem?.cancel()
            longPressWorkItem = nil
        }

        let (gx, gy) = r.clampedViewPointToGamePoint(pt, in: mtkView.bounds)
        engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_MOVE, x: gx, y: gy)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        finishTouches(touches, with: event, cancelled: false)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        finishTouches(touches, with: event, cancelled: true)
    }

    private func finishTouches(_ touches: Set<UITouch>, with event: UIEvent?, cancelled: Bool) {
        if let tracked = activeTouch, touches.contains(tracked) {
            longPressWorkItem?.cancel()
            longPressWorkItem = nil

            if longPressFired {
                engineBridge.keyEvent(vkCode: 0x11, isDown: false) // VK_CONTROL up
            } else if touchSentDown && !multiTouchActive {
                if !cancelled, let r = renderer {
                    let pt = tracked.location(in: mtkView)
                    let (gx, gy) = r.clampedViewPointToGamePoint(pt, in: mtkView.bounds)
                    engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_UP, x: gx, y: gy)
                } else {
                    engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_UP, x: -1, y: -1)
                }
            }

            activeTouch = nil
            touchSentDown = false
            longPressFired = false
        }

        let allDone = event?.allTouches?.allSatisfy {
            $0.phase == .ended || $0.phase == .cancelled
        } ?? true
        if allDone {
            multiTouchActive = false
        }
    }

    private func cancelActiveSingleTouchForMultiTouch() {
        multiTouchActive = true
        longPressWorkItem?.cancel()
        longPressWorkItem = nil

        if longPressFired {
            engineBridge.keyEvent(vkCode: 0x11, isDown: false)
            longPressFired = false
        }
        if touchSentDown {
            engineBridge.pointerEvent(kind: FUJIHIME_POINTER_KIND_UP, x: -1, y: -1)
            touchSentDown = false
        }
        activeTouch = nil
    }

    private func scheduleLongPressTimer(for touch: UITouch) {
        longPressWorkItem?.cancel()
        let item = DispatchWorkItem { [weak self, weak touch] in
            guard let self = self,
                  let touch = touch,
                  self.activeTouch === touch,
                  !self.multiTouchActive else {
                return
            }
            let pt = touch.location(in: self.mtkView)
            let dist = hypot(pt.x - self.touchStartPoint.x, pt.y - self.touchStartPoint.y)
            guard dist < Self.longPressMoveTolerancePt else { return }

            self.longPressFired = true
            self.engineBridge.keyEvent(vkCode: 0x11, isDown: true) // VK_CONTROL down
        }
        longPressWorkItem = item
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.longPressDelaySeconds, execute: item)
    }

    private func cancelLongPressAndReleaseSkip() {
        longPressWorkItem?.cancel()
        longPressWorkItem = nil
        if longPressFired {
            engineBridge.keyEvent(vkCode: 0x11, isDown: false)
            longPressFired = false
        }
        activeTouch = nil
        touchSentDown = false
        multiTouchActive = false
    }

    // MARK: - External Hardware Keyboard

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handledAny = false
        for press in presses {
            if let key = press.key, let vk = mapHIDUsageToVK(key.keyCode) {
                engineBridge.keyEvent(vkCode: vk, isDown: true)
                handledAny = true
            }
        }
        if !handledAny {
            super.pressesBegan(presses, with: event)
        }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handledAny = false
        for press in presses {
            if let key = press.key, let vk = mapHIDUsageToVK(key.keyCode) {
                engineBridge.keyEvent(vkCode: vk, isDown: false)
                handledAny = true
            }
        }
        if !handledAny {
            super.pressesEnded(presses, with: event)
        }
    }

    private func mapHIDUsageToVK(_ usage: UIKeyboardHIDUsage) -> UInt16? {
        switch usage {
        case .keyboardReturnOrEnter, .keypadEnter:
            return 0x0D // VK_RETURN
        case .keyboardSpacebar:
            return 0x20 // VK_SPACE
        case .keyboardEscape:
            return 0x1B // VK_ESCAPE
        case .keyboardTab:
            return 0x09 // VK_TAB
        case .keyboardLeftControl, .keyboardRightControl:
            return 0x11 // VK_CONTROL
        case .keyboardLeftShift, .keyboardRightShift:
            return 0x10 // VK_SHIFT
        case .keyboardUpArrow, .keyboardPageUp:
            return 0x26 // VK_UP
        case .keyboardDownArrow, .keyboardPageDown:
            return 0x28 // VK_DOWN
        case .keyboardF1:
            return 0x70 // VK_F1
        default:
            return nil
        }
    }

    // MARK: - Native Dialogs & Quick Menu

    private func pollAndPresentEngineDialog() {
        guard !isPresentingAlert, presentedViewController == nil else { return }
        guard let dlg = engineBridge.pollPendingDialog() else { return }

        isPresentingAlert = true
        let alert = UIAlertController(title: dlg.title, message: dlg.message, preferredStyle: .alert)

        if dlg.kind == FUJIHIME_DIALOG_KIND_OK_ALERT {
            alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
                self?.isPresentingAlert = false
                self?.engineBridge.confirmDialogResponse(dialogId: dlg.id, accepted: true)
            })
        } else {
            let isChs = (engineBridge.currentLanguageMode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
            let noTitle = isChs ? "否" : "いいえ"
            let yesTitle = isChs ? "是" : "はい"

            alert.addAction(UIAlertAction(title: noTitle, style: .cancel) { [weak self] _ in
                self?.isPresentingAlert = false
                self?.engineBridge.confirmDialogResponse(dialogId: dlg.id, accepted: false)
            })
            alert.addAction(UIAlertAction(title: yesTitle, style: .default) { [weak self] _ in
                self?.isPresentingAlert = false
                self?.engineBridge.confirmDialogResponse(dialogId: dlg.id, accepted: true)
            })
        }

        present(alert, animated: true, completion: nil)
    }

    private func presentQuickMenu() {
        guard !isPresentingAlert, presentedViewController == nil else { return }
        guard engineBridge.hasGameAssets else {
            presentMissingAssetsAlert()
            return
        }

        isPresentingAlert = true
        let isChs = (engineBridge.currentLanguageMode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
        let title = isChs ? "腐姫 快捷菜单" : "腐姫 クイックメニュー"
        let langDesc = isChs ? "当前模式：简体中文 V1.0\n字体：\(engineBridge.loadedFontSummary)" : "現在モード：オリジナル日本語\nフォント：\(engineBridge.loadedFontSummary)"

        let sheet = UIAlertController(title: title, message: langDesc, preferredStyle: .actionSheet)

        let switchLangTitle = isChs ? "切换至原版日文 (Original JP)" : "簡体字中国語 V1.0 へ切替"
        sheet.addAction(UIAlertAction(title: switchLangTitle, style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
            let nextMode = isChs ? FUJIHIME_LANGUAGE_MODE_JAPANESE : FUJIHIME_LANGUAGE_MODE_CHINESE_V1
            _ = self?.engineBridge.setLanguageMode(nextMode)
        })

        let actions: [(String, FujihimeSystemAction)] = isChs ? [
            ("存档 (Save)", FUJIHIME_SYSTEM_ACTION_SAVE),
            ("读档 (Load)", FUJIHIME_SYSTEM_ACTION_LOAD),
            ("系统设置 (Config)", FUJIHIME_SYSTEM_ACTION_CONFIG),
            ("自动播放 (Auto)", FUJIHIME_SYSTEM_ACTION_AUTO),
            ("快进模式 (Skip)", FUJIHIME_SYSTEM_ACTION_SKIP),
            ("回看历史 (Backlog)", FUJIHIME_SYSTEM_ACTION_BACKLOG),
            ("一步回退 (Rollback)", FUJIHIME_SYSTEM_ACTION_ROLLBACK),
            ("隐藏 / 显示文本框", FUJIHIME_SYSTEM_ACTION_HIDE_TEXTBOX),
            ("回到标题画面", FUJIHIME_SYSTEM_ACTION_TITLE)
        ] : [
            ("セーブ (Save)", FUJIHIME_SYSTEM_ACTION_SAVE),
            ("ロード (Load)", FUJIHIME_SYSTEM_ACTION_LOAD),
            ("環境設定 (Config)", FUJIHIME_SYSTEM_ACTION_CONFIG),
            ("オート (Auto)", FUJIHIME_SYSTEM_ACTION_AUTO),
            ("スキップ (Skip)", FUJIHIME_SYSTEM_ACTION_SKIP),
            ("バックログ (Backlog)", FUJIHIME_SYSTEM_ACTION_BACKLOG),
            ("一つ戻る (Rollback)", FUJIHIME_SYSTEM_ACTION_ROLLBACK),
            ("ウィンドウ消去 / 表示", FUJIHIME_SYSTEM_ACTION_HIDE_TEXTBOX),
            ("タイトルに戻る", FUJIHIME_SYSTEM_ACTION_TITLE)
        ]

        for (label, sysAction) in actions {
            sheet.addAction(UIAlertAction(title: label, style: .default) { [weak self] _ in
                self?.isPresentingAlert = false
                self?.engineBridge.performAction(sysAction)
            })
        }

        let importTitle = isChs ? "导入 2002 CD 存档 (KhDLsave_2002CD.dat)" : "2002 CD セーブ取込 (KhDLsave_2002CD.dat)"
        sheet.addAction(UIAlertAction(title: importTitle, style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
            self?.handleManualImport2002Save()
        })

        let helpTitle = isChs ? "手势与外接键盘说明" : "ジェスチャー・キーボード操作"
        sheet.addAction(UIAlertAction(title: helpTitle, style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
            self?.presentGestureHelpAlert()
        })

        let cancelTitle = isChs ? "取消" : "キャンセル"
        sheet.addAction(UIAlertAction(title: cancelTitle, style: .cancel) { [weak self] _ in
            self?.isPresentingAlert = false
        })

        if let popover = sheet.popoverPresentationController {
            popover.sourceView = view
            popover.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }

        present(sheet, animated: true, completion: nil)
    }

    private func handleManualImport2002Save() {
        let isChs = (engineBridge.currentLanguageMode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
        let candidatePath = (engineBridge.saveDirPath as NSString).appendingPathComponent("KhDLsave_2002CD.dat")
        let fm = FileManager.default

        let title: String
        let message: String
        if fm.fileExists(atPath: candidatePath),
           let data = try? Data(contentsOf: URL(fileURLWithPath: candidatePath)),
           engineBridge.import2002CdSave(data: data) {
            let importedPath = (engineBridge.saveDirPath as NSString).appendingPathComponent("KhDLsave_2002CD.dat.imported")
            try? fm.removeItem(atPath: importedPath)
            try? fm.moveItem(atPath: candidatePath, toPath: importedPath)
            title = isChs ? "导入成功" : "取込完了"
            message = isChs ? "已将 KhDLsave_2002CD.dat 迁移至 2007 DL 格式并写入存档。" : "KhDLsave_2002CD.dat を 2007 DL 形式へ変換し適用しました。"
        } else {
            title = isChs ? "未找到有效的 2002 CD 存档" : "2002 CD セーブが見つかりません"
            message = isChs
                ? "请将 206,024 字节的 KhDLsave_2002CD.dat 放入「文件」App -> 腐姫 -> save/ 目录后再试。"
                : "206,024 バイトの KhDLsave_2002CD.dat を「ファイル」App -> 腐姫 -> save/ に配置してください。"
        }

        isPresentingAlert = true
        let alert = UIAlertController(title: title, message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
        })
        present(alert, animated: true, completion: nil)
    }

    private func presentGestureHelpAlert() {
        let isChs = (engineBridge.currentLanguageMode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
        let title = isChs ? "操作说明" : "操作ガイド"
        let msg = isChs
            ? """
            • 单指点击：左键推进 / 确定
            • 单指拖动：悬停高亮 / 设置页滑块拖拽
            • 单指长按 (≥0.45s)：按住快进 (Ctrl)
            • 双指轻点：右键 (隐藏文本框 / 返回)
            • 双指下滑 / 上滑：呼出并翻阅 Backlog
            • 三指轻点：呼出本快捷菜单 (切换日文/汉化)
            • 硬件键盘：Enter / Space / Esc / Ctrl / Shift / Tab / 方向键 / PageUp / PageDown / F1
            """
            : """
            • 1本指タップ：左クリック (文章送り・決定)
            • 1本指ドラッグ：カーソル移動・スライダー操作
            • 1本指長押し (0.45秒以上)：Ctrl 早送り
            • 2本指タップ：右クリック (枠消去・戻る)
            • 2本指上下スワイプ：バックログ表示・スクロール
            • 3本指タップ：クイックメニュー表示 (日本語/簡体字切替)
            • ハードウェアキーボード：Enter / Space / Esc / Ctrl / Shift / Tab / 矢印 / PageUp / PageDown / F1
            """

        isPresentingAlert = true
        let alert = UIAlertController(title: title, message: msg, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
        })
        present(alert, animated: true, completion: nil)
    }

    private func presentMissingAssetsAlert() {
        guard !isPresentingAlert, presentedViewController == nil else { return }
        isPresentingAlert = true
        let msg = """
        未在 App Bundle 中检测到游戏资源（game/scr.xfl 等）。
        请通过携带游戏资产的 IPA 构建，或使用「文件」App 将原版资源放入「腐姫/game/」目录后重启应用。

        Game assets not found in AppBundle/game/.
        """
        let alert = UIAlertController(title: "缺少游戏资源 / Missing Assets", message: msg, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
            self?.isPresentingAlert = false
        })
        present(alert, animated: true, completion: nil)
    }
}
