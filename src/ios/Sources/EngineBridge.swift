import Foundation

struct PendingDialogInfo {
    let id: UInt64
    let kind: FujihimeDialogKind
    let title: String
    let message: String
}

final class EngineBridge {
    static let gameWidth = Int(GAME_WIDTH)
    static let gameHeight = Int(GAME_HEIGHT)
    static let presentBGRA8Length = gameWidth * gameHeight * 4
    static let sampleRate = Double(AUDIO_SAMPLE_RATE)

    private static let languageDefaultsKey = "fujihime.language_mode"

    private let lock = NSLock()
    private var engine: OpaquePointer?

    let gameRootPath: String
    let origBackupPath: String
    let saveDirPath: String
    let overrideDirPath: String
    let bundleFontsDirPath: String
    let documentsFontsDirPath: String
    let hasGameAssets: Bool

    private(set) var currentLanguageMode: FujihimeLanguageMode
    private(set) var loadedFontSummary: String = "none"

    init() {
        let fm = FileManager.default
        let bundlePath = Bundle.main.bundlePath
        let docsURL = fm.urls(for: .documentDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)

        let saveURL = docsURL.appendingPathComponent("save", isDirectory: true)
        let overrideURL = docsURL.appendingPathComponent("override", isDirectory: true)
        let docsFontsURL = docsURL.appendingPathComponent("Fonts", isDirectory: true)

        try? fm.createDirectory(at: saveURL, withIntermediateDirectories: true, attributes: nil)
        try? fm.createDirectory(at: overrideURL, withIntermediateDirectories: true, attributes: nil)

        let bundleGamePath = (bundlePath as NSString).appendingPathComponent("game")
        let docsGamePath = docsURL.appendingPathComponent("game", isDirectory: true).path
        let bundleHasScr = fm.fileExists(atPath: (bundleGamePath as NSString).appendingPathComponent("scr.xfl"))
            || fm.fileExists(atPath: (bundleGamePath as NSString).appendingPathComponent("orig_backup/scr.xfl"))
        let docsHasScr = fm.fileExists(atPath: (docsGamePath as NSString).appendingPathComponent("scr.xfl"))

        if bundleHasScr {
            self.gameRootPath = bundleGamePath
            self.hasGameAssets = true
        } else if docsHasScr {
            self.gameRootPath = docsGamePath
            self.hasGameAssets = true
        } else {
            self.gameRootPath = bundleGamePath
            self.hasGameAssets = false
        }

        self.origBackupPath = (self.gameRootPath as NSString).appendingPathComponent("orig_backup")
        self.saveDirPath = saveURL.path
        self.overrideDirPath = overrideURL.path
        self.bundleFontsDirPath = (bundlePath as NSString).appendingPathComponent("Fonts")
        self.documentsFontsDirPath = docsFontsURL.path

        let savedLangRaw = UserDefaults.standard.integer(forKey: Self.languageDefaultsKey)
        if savedLangRaw == Int(FUJIHIME_LANGUAGE_MODE_CHINESE_V1.rawValue) {
            self.currentLanguageMode = FUJIHIME_LANGUAGE_MODE_CHINESE_V1
        } else {
            self.currentLanguageMode = FUJIHIME_LANGUAGE_MODE_JAPANESE
        }

        if self.hasGameAssets {
            self.initializeEngine()
        }
    }

    deinit {
        destroy()
    }

    private func initializeEngine() {
        let mode = currentLanguageMode
        let repairLabels = (mode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
        let created: OpaquePointer? = gameRootPath.withCString { gPtr in
            saveDirPath.withCString { sPtr in
                overrideDirPath.withCString { oPtr in
                    origBackupPath.withCString { bPtr in
                        var cfg = FujihimeEngineConfig(
                            game_root: gPtr,
                            save_dir: sPtr,
                            override_dir: oPtr,
                            orig_backup_dir: bPtr,
                            language_mode: mode,
                            repair_chs_recall_labels: repairLabels
                        )
                        return fujihime_engine_create(&cfg)
                    }
                }
            }
        }

        lock.lock()
        self.engine = created
        lock.unlock()

        if created != nil {
            loadFontsForLanguage(mode)
            _ = import2002CdSaveIfPresentOnStartup()
            startTitle()
        }
    }

    func destroy() {
        lock.lock()
        let ptr = engine
        engine = nil
        lock.unlock()

        if let ptr = ptr {
            fujihime_engine_destroy(ptr)
        }
    }

    private func findFontData(candidates: [String]) -> (name: String, data: Data)? {
        let fm = FileManager.default
        let dirs = [documentsFontsDirPath, bundleFontsDirPath]
        for dir in dirs {
            for name in candidates {
                let full = (dir as NSString).appendingPathComponent(name)
                if fm.fileExists(atPath: full), let data = try? Data(contentsOf: URL(fileURLWithPath: full)), !data.isEmpty {
                    return (name, data)
                }
            }
        }
        return nil
    }

    private func registerFont(_ data: Data, role: FujihimeFontRole, faceIndex: Int32 = 0) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine, !data.isEmpty else { return false }
        return data.withUnsafeBytes { rawBuf in
            guard let base = rawBuf.bindMemory(to: UInt8.self).baseAddress else { return false }
            return fujihime_engine_load_font(ptr, role, base, UInt(rawBuf.count), faceIndex) == FUJIHIME_OK
        }
    }

    private func loadFontsForLanguage(_ mode: FujihimeLanguageMode) {
        let jpGothic = findFontData(candidates: [
            "jp_gothic.ttf", "jp_gothic.ttc", "jp_gothic.otf", "msgothic.ttc", "ipag.ttf"
        ])
        let jpMincho = findFontData(candidates: [
            "jp_mincho.ttf", "jp_mincho.ttc", "jp_mincho.otf", "msmincho.ttc", "ipam.ttf"
        ])
        let chsHei = findFontData(candidates: [
            "chs_hei.ttf", "chs_hei.ttc", "chs_hei.otf", "wqy-zenhei.ttc", "simhei.ttf"
        ])

        var summaryParts: [String] = []
        if mode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1 {
            if let primary = chsHei ?? jpGothic {
                if registerFont(primary.data, role: FUJIHIME_FONT_ROLE_GOTHIC) {
                    summaryParts.append("gothic=\(primary.name)")
                }
            }
            if let secondary = chsHei ?? jpMincho {
                if registerFont(secondary.data, role: FUJIHIME_FONT_ROLE_MINCHO) {
                    summaryParts.append("mincho=\(secondary.name)")
                }
            }
            if let fallback = jpGothic {
                if registerFont(fallback.data, role: FUJIHIME_FONT_ROLE_FALLBACK) {
                    summaryParts.append("fallback=\(fallback.name)")
                }
            }
        } else {
            if let primary = jpGothic ?? chsHei {
                if registerFont(primary.data, role: FUJIHIME_FONT_ROLE_GOTHIC) {
                    summaryParts.append("gothic=\(primary.name)")
                }
            }
            if let secondary = jpMincho {
                if registerFont(secondary.data, role: FUJIHIME_FONT_ROLE_MINCHO) {
                    summaryParts.append("mincho=\(secondary.name)")
                }
            }
            if let fallback = chsHei {
                if registerFont(fallback.data, role: FUJIHIME_FONT_ROLE_FALLBACK) {
                    summaryParts.append("fallback=\(fallback.name)")
                }
            }
        }
        loadedFontSummary = summaryParts.isEmpty ? "none" : summaryParts.joined(separator: ", ")
    }

    @discardableResult
    func startTitle() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return false }
        return fujihime_engine_start_title(ptr) == FUJIHIME_OK
    }

    @discardableResult
    func tick(deltaMs: UInt32) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return false }
        return fujihime_engine_tick(ptr, deltaMs) == FUJIHIME_OK
    }

    @discardableResult
    func copyPresentBGRA8(into buffer: inout [UInt8]) -> Bool {
        guard buffer.count >= Self.presentBGRA8Length else { return false }
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return false }
        return buffer.withUnsafeMutableBytes { rawBuf in
            guard let base = rawBuf.bindMemory(to: UInt8.self).baseAddress else { return false }
            return fujihime_engine_copy_present_bgra8(ptr, base, UInt(rawBuf.count)) == FUJIHIME_OK
        }
    }

    func renderAudioF32(into interleavedOut: UnsafeMutablePointer<Float>, frameCount: Int) -> Int {
        guard frameCount > 0 else { return 0 }
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return 0 }
        return Int(fujihime_engine_render_audio_f32(ptr, interleavedOut, UInt(frameCount)))
    }

    func pointerEvent(kind: FujihimePointerKind, x: Int32, y: Int32) {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return }
        _ = fujihime_engine_pointer_event(ptr, kind, x, y)
    }

    func keyEvent(vkCode: UInt16, isDown: Bool) {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return }
        _ = fujihime_engine_key_event(ptr, vkCode, isDown)
    }

    func performAction(_ action: FujihimeSystemAction) {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return }
        _ = fujihime_engine_perform_action(ptr, action)
    }

    @discardableResult
    func setLanguageMode(_ mode: FujihimeLanguageMode) -> Bool {
        let repair = (mode == FUJIHIME_LANGUAGE_MODE_CHINESE_V1)
        lock.lock()
        let ok: Bool
        if let ptr = engine {
            ok = origBackupPath.withCString { bPtr in
                fujihime_engine_set_language_mode(ptr, mode, bPtr, repair) == FUJIHIME_OK
            }
        } else {
            ok = false
        }
        lock.unlock()

        if ok {
            currentLanguageMode = mode
            UserDefaults.standard.set(Int(mode.rawValue), forKey: Self.languageDefaultsKey)
            loadFontsForLanguage(mode)
        }
        return ok
    }

    @discardableResult
    func saveGlobal() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return false }
        return fujihime_engine_save_global(ptr) == FUJIHIME_OK
    }

    func getState() -> FujihimeEngineState? {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return nil }
        var state = FujihimeEngineState()
        if fujihime_engine_get_state(ptr, &state) == FUJIHIME_OK {
            return state
        }
        return nil
    }

    func pollPendingDialog() -> PendingDialogInfo? {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return nil }
        var rawDialog = FujihimePendingDialog()
        guard fujihime_engine_poll_pending_dialog(ptr, &rawDialog) else {
            return nil
        }
        var titleBuf = rawDialog.title_utf8
        let title = withUnsafeBytes(of: &titleBuf) { bytes -> String in
            guard let cStr = bytes.bindMemory(to: CChar.self).baseAddress else { return "" }
            return String(cString: cStr)
        }
        var msgBuf = rawDialog.message_utf8
        let message = withUnsafeBytes(of: &msgBuf) { bytes -> String in
            guard let cStr = bytes.bindMemory(to: CChar.self).baseAddress else { return "" }
            return String(cString: cStr)
        }
        return PendingDialogInfo(
            id: rawDialog.dialog_id,
            kind: rawDialog.kind,
            title: title,
            message: message
        )
    }

    func confirmDialogResponse(dialogId: UInt64, accepted: Bool) {
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return }
        _ = fujihime_engine_confirm_dialog_response(ptr, dialogId, accepted)
    }

    @discardableResult
    func import2002CdSaveIfPresentOnStartup() -> Bool {
        let fm = FileManager.default
        let candidatePath = (saveDirPath as NSString).appendingPathComponent("KhDLsave_2002CD.dat")
        guard fm.fileExists(atPath: candidatePath) else { return false }
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: candidatePath)),
              data.count == Int(CD_2002_SAVE_SIZE) else {
            return false
        }
        let ok = import2002CdSave(data: data)
        if ok {
            let importedPath = (saveDirPath as NSString).appendingPathComponent("KhDLsave_2002CD.dat.imported")
            try? fm.removeItem(atPath: importedPath)
            try? fm.moveItem(atPath: candidatePath, toPath: importedPath)
        }
        return ok
    }

    func import2002CdSave(data: Data) -> Bool {
        guard data.count == Int(CD_2002_SAVE_SIZE) else { return false }
        lock.lock()
        defer { lock.unlock() }
        guard let ptr = engine else { return false }
        return data.withUnsafeBytes { rawBuf in
            guard let base = rawBuf.bindMemory(to: UInt8.self).baseAddress else { return false }
            return fujihime_engine_import_2002_cd_save(ptr, base, UInt(rawBuf.count)) == FUJIHIME_OK
        }
    }
}
