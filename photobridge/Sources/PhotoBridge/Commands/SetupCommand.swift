import ArgumentParser
import Foundation

@available(macOS 13, *)
struct SetupCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "setup",
        abstract: "Print first-time setup guide and optionally install a launchd sync job"
    )

    @Flag(name: .long, help: "Generate and install a launchd plist for automatic sync")
    var installLaunchd: Bool = false

    @Option(name: .long, help: "Sync interval in hours when installing launchd job (default: 6)")
    var intervalHours: Int = 6

    func run() async throws {
        if installLaunchd {
            try installLaunchdPlist(intervalHours: intervalHours)
            return
        }
        printSetupGuide()
    }

    private func printSetupGuide() {
        print("""
        PhotoBridge First-Time Setup
        ════════════════════════════

        Step 1: Move Photos Library to an external drive (recommended)
          ① Open Photos.app → Settings → General
          ② Click "Change…" and choose a path on an external drive
          ③ Photos will move the library automatically
             (iCloud content stays in the cloud — no data loss)

        Step 2: Grant Photos library access
          ① Run:  photobridge status
          ② A system dialog will appear; choose "Full Access"
          ③ Re-run the command — it will work immediately (no restart needed)

          Note: The permission dialog shows the name of your terminal app
          (e.g. "iTerm2"), not "photobridge". This is normal macOS behaviour.
          If you previously denied access, reset it with:
            tccutil reset Photos com.googlecode.iterm2   # iTerm2
            tccutil reset Photos com.apple.Terminal      # Terminal.app

        Step 3: First full export
          Run:  photobridge export --batch-size 200
          (Large libraries may take hours; you can interrupt and resume.)

        Step 4: Daily incremental sync
          Run:  photobridge sync
          Or install a launchd job to sync automatically:
            photobridge setup --install-launchd --interval-hours 6
        """)
    }

    private func installLaunchdPlist(intervalHours: Int) throws {
        guard let picmanagerURL = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("picmanager")
            .absoluteURL
            ?? URL(string: "/usr/local/bin/picmanager") else {
            fputs("Could not determine photobridge path.\n", stderr)
            throw ExitCode.failure
        }

        let photobridgePath = CommandLine.arguments[0]
        let intervalSeconds = intervalHours * 3600
        let label = "com.picmanager.photobridge-sync"
        let plistPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/\(label).plist")

        let plistContent = """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
          "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>Label</key>
            <string>\(label)</string>
            <key>ProgramArguments</key>
            <array>
                <string>\(photobridgePath)</string>
                <string>sync</string>
                <string>--picmanager</string>
                <string>\(picmanagerURL.path)</string>
            </array>
            <key>StartInterval</key>
            <integer>\(intervalSeconds)</integer>
            <key>StandardOutPath</key>
            <string>\(FileManager.default.homeDirectoryForCurrentUser.path)/Library/Logs/photobridge-sync.log</string>
            <key>StandardErrorPath</key>
            <string>\(FileManager.default.homeDirectoryForCurrentUser.path)/Library/Logs/photobridge-sync-error.log</string>
            <key>RunAtLoad</key>
            <false/>
        </dict>
        </plist>
        """

        try FileManager.default.createDirectory(
            at: plistPath.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try plistContent.write(to: plistPath, atomically: true, encoding: .utf8)

        print("""
        ✓ Installed: \(plistPath.path)

        To activate (runs sync every \(intervalHours) hours):
          launchctl load \(plistPath.path)

        To deactivate:
          launchctl unload \(plistPath.path)

        Logs:
          ~/Library/Logs/photobridge-sync.log
          ~/Library/Logs/photobridge-sync-error.log
        """)
    }
}
