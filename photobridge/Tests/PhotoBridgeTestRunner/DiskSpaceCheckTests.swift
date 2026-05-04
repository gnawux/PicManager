import Foundation
import PhotoBridgeLib

func runDiskSpaceCheckTests() {
    suite("DiskSpaceCheck") {
        test("formatBytes under 1 GB shows MB") {
            let s = formatBytes(52_428_800)  // 50 MB
            try expect(s.contains("MB"), "should show MB: \(s)")
        }

        test("formatBytes 1 GB+ shows GB") {
            let s = formatBytes(2_147_483_648)  // 2 GB
            try expect(s.contains("GB"), "should show GB: \(s)")
        }

        test("checkDiskSpace nil when plenty of free space") {
            // estimated 1 MB, free 10 GB → no warning
            let warning = checkDiskSpace(
                stagingDir: URL(fileURLWithPath: "/tmp"),
                assets: [],
                estimatedBytesOverride: 1_048_576,
                freeBytesOverride: 10_737_418_240
            )
            try expect(warning == nil, "no warning when plenty of space")
        }

        test("checkDiskSpace warns when estimated > 80% of free") {
            // estimated 900 MB, free 1 GB → warn (90%)
            let warning = checkDiskSpace(
                stagingDir: URL(fileURLWithPath: "/tmp"),
                assets: [],
                estimatedBytesOverride: 943_718_400,
                freeBytesOverride: 1_073_741_824
            )
            try expect(warning != nil, "should warn when > 80% of free space")
        }

        test("checkDiskSpace warns for system volume path") {
            let warning = checkDiskSpace(
                stagingDir: URL(fileURLWithPath: "/tmp"),
                assets: [],
                photosLibraryPath: "/System/Volumes/Data/Users/me/Pictures/Photos Library.photoslibrary",
                estimatedBytesOverride: 0,
                freeBytesOverride: Int64.max
            )
            try expect(warning != nil, "should warn for system volume")
            try expect(warning?.isSystemVolume == true, "isSystemVolume flag set")
        }

        test("checkDiskSpace no system-volume warning for external drive path") {
            let warning = checkDiskSpace(
                stagingDir: URL(fileURLWithPath: "/tmp"),
                assets: [],
                photosLibraryPath: "/Volumes/MyDrive/Photos Library.photoslibrary",
                estimatedBytesOverride: 0,
                freeBytesOverride: Int64.max
            )
            try expect(warning == nil, "no warning for external drive")
        }
    }
}
