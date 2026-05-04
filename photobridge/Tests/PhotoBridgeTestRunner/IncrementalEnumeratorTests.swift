import Foundation
import Photos
import PhotoBridgeLib

func runIncrementalEnumeratorTests() {
    suite("IncrementalEnumerator") {
        test("tokenData round-trip: encode then decode preserves bytes") {
            // PHPersistentChangeToken is NSCoding — we only test the
            // Data wrapper helpers here without a real token object.
            let original = Data([0x01, 0x02, 0x03, 0xFF])
            // Encode: wrap raw bytes in a keyed-archiver plist
            let archived = try NSKeyedArchiver.archivedData(
                withRootObject: original as NSData,
                requiringSecureCoding: true
            )
            let unarchived = try NSKeyedUnarchiver.unarchivedObject(
                ofClass: NSData.self,
                from: archived
            ).map { Data($0) }
            try expect(unarchived == original, "round-trip mismatch")
        }

        test("nil token → defaultStateURL is in Application Support") {
            let url = IncrementalEnumerator.defaultStateURL
            let appSupport = FileManager.default.urls(
                for: .applicationSupportDirectory, in: .userDomainMask
            ).first!
            try expect(url.path.hasPrefix(appSupport.path), "state URL should be in Application Support")
        }

        test("defaultStateURL ends with state.json") {
            let url = IncrementalEnumerator.defaultStateURL
            try expect(url.lastPathComponent == "state.json", "expected state.json filename")
        }
    }
}
