import Foundation
import PhotoBridgeLib

func runInventoryRecordTests() {
    suite("Inventory records") {
        test("encodes source identity separately from original filename") {
            let record = InventoryAssetRecord(
                localIdentifier: "A-UUID/L0/001",
                originalFilename: "IMG_0042.HEIC",
                mediaType: "image",
                pixelWidth: 4032,
                pixelHeight: 3024,
                creationDate: Date(timeIntervalSince1970: 0),
                modificationDate: nil,
                favorite: true,
                hidden: false,
                livePhoto: true,
                adjusted: false,
                burstIdentifier: nil,
                excludedReason: nil,
                resources: [InventoryResourceRecord(
                    type: "photo",
                    originalFilename: "IMG_0042.HEIC",
                    uniformTypeIdentifier: "public.heic"
                )]
            )
            let data = try inventoryEncoder().encode(record)
            let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]
            try expect(json["local_identifier"] as? String, equals: "A-UUID/L0/001")
            try expect(json["original_filename"] as? String, equals: "IMG_0042.HEIC")
            try expect(json["record_type"] as? String, equals: "asset")
            try expect(json["schema_version"] as? Int, equals: 1)
        }

        test("boundary records carry durable checkpoints and completion counts") {
            let boundary = InventoryBoundaryRecord(
                recordType: "inventory_end",
                checkpoint: Data([1, 2, 3]),
                assetCount: 42
            )
            let data = try inventoryEncoder().encode(boundary)
            let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]
            try expect(json["record_type"] as? String, equals: "inventory_end")
            try expect(json["checkpoint"] as? String, equals: "AQID")
            try expect(json["asset_count"] as? Int, equals: 42)
        }
    }
}
