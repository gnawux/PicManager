import Photos
import PhotoBridgeLib

func runAssetFilterTests() {
    suite("selectExportResource") {
        test("JPEG-only asset → returns .photo resource") {
            let resources = makeResources([(.photo, "public.jpeg")])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result != nil, "expected a resource")
            try expect(result!.type == .photo, "expected .photo type")
        }

        test("HEIC-only asset → returns .photo resource") {
            let resources = makeResources([(.photo, "public.heic")])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result != nil, "expected a resource")
            try expect(result!.type == .photo, "expected .photo type")
        }

        test("RAW+JPEG asset → returns .photo (JPEG), skips .alternatePhoto (RAW)") {
            let resources = makeResources([
                (.photo, "public.jpeg"),
                (.alternatePhoto, "com.adobe.raw-image"),
            ])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result != nil, "expected a resource")
            try expect(result!.type == .photo, "expected .photo, not .alternatePhoto")
        }

        test("Live Photo → returns .photo, skips .pairedVideo") {
            let resources = makeResources([
                (.photo, "public.heic"),
                (.pairedVideo, "com.apple.quicktime-movie"),
            ])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result != nil, "expected a resource")
            try expect(result!.type == .photo, "expected .photo, not .pairedVideo")
        }

        test("RAW-only asset (no JPEG) → returns nil (skip)") {
            let resources = makeResources([(.alternatePhoto, "com.adobe.raw-image")])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result == nil, "expected nil for RAW-only asset")
        }

        test("empty resources → returns nil") {
            let result = selectExportResource(from: [], isBurst: false, isUserPick: false)
            try expect(result == nil, "expected nil for empty resources")
        }

        test("burst non-userPick → returns nil (skip non-selected burst frames)") {
            let resources = makeResources([(.photo, "public.jpeg")])
            let result = selectExportResource(from: resources, isBurst: true, isUserPick: false)
            try expect(result == nil, "expected nil for non-userPick burst frame")
        }

        test("burst userPick → returns resource") {
            let resources = makeResources([(.photo, "public.jpeg")])
            let result = selectExportResource(from: resources, isBurst: true, isUserPick: true)
            try expect(result != nil, "expected resource for userPick burst frame")
        }

        test("non-burst (representsBurst=false) ignores isUserPick") {
            let resources = makeResources([(.photo, "public.jpeg")])
            let result = selectExportResource(from: resources, isBurst: false, isUserPick: false)
            try expect(result != nil, "non-burst assets should always be exported")
        }
    }
}
