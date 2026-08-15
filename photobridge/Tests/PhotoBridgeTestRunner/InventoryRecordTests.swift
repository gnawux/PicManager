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

        test("incremental records preserve before and after checkpoint roles") {
            let boundary = InventoryChangeBoundaryRecord(
                recordType: "changes_end",
                checkpointBefore: Data([1]),
                checkpointAfter: Data([2]),
                assetCount: 3,
                removedCount: 1
            )
            let data = try inventoryEncoder().encode(boundary)
            let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]
            try expect(json["mode"] as? String, equals: "incremental")
            try expect(json["checkpoint_before"] as? String, equals: "AQ==")
            try expect(json["checkpoint_after"] as? String, equals: "Ag==")
            try expect(json["removed_count"] as? Int, equals: 1)
        }

        test("removed asset record retains provider identity") {
            let data = try inventoryEncoder().encode(
                InventoryRemovalRecord(localIdentifier: "UUID/L0/001")
            )
            let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]
            try expect(json["record_type"] as? String, equals: "removed_asset")
            try expect(json["local_identifier"] as? String, equals: "UUID/L0/001")
        }
    }
}
