import Cocoa

class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            let task = Process()
            task.executableURL = URL(fileURLWithPath: NSHomeDirectory() + "/Applications/Boo.app/Contents/MacOS/boo")
            task.arguments = [url.absoluteString]
            try? task.run()
        }
        NSApp.terminate(nil)
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
