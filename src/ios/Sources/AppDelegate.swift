import UIKit

@UIApplicationMain
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    private weak var gameViewController: GameViewController?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let win = UIWindow(frame: UIScreen.main.bounds)
        win.backgroundColor = .black

        let vc = GameViewController()
        self.gameViewController = vc
        win.rootViewController = vc
        win.makeKeyAndVisible()

        self.window = win
        return true
    }

    func applicationWillResignActive(_ application: UIApplication) {
        gameViewController?.pauseForBackground()
    }

    func applicationDidEnterBackground(_ application: UIApplication) {
        gameViewController?.pauseForBackground()
    }

    func applicationDidBecomeActive(_ application: UIApplication) {
        gameViewController?.resumeFromBackground()
    }

    func applicationWillTerminate(_ application: UIApplication) {
        gameViewController?.shutdown()
    }
}
