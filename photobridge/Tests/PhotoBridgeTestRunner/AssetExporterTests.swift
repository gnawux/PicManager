import Photos
import PhotoBridgeLib

func runAssetExporterTests() {
    suite("fileExtension(forUTI:)") {
        test("public.jpeg → .jpg") {
            try expect(fileExtension(forUTI: "public.jpeg"), equals: "jpg")
        }
        test("public.jpg → .jpg") {
            try expect(fileExtension(forUTI: "public.jpg"), equals: "jpg")
        }
        test("public.heic → .heic") {
            try expect(fileExtension(forUTI: "public.heic"), equals: "heic")
        }
        test("public.heif → .heif") {
            try expect(fileExtension(forUTI: "public.heif"), equals: "heif")
        }
        test("public.png → .png") {
            try expect(fileExtension(forUTI: "public.png"), equals: "png")
        }
        test("public.tiff → .tiff") {
            try expect(fileExtension(forUTI: "public.tiff"), equals: "tiff")
        }
        test("unknown UTI → .data (fallback)") {
            try expect(fileExtension(forUTI: "com.unknown.format"), equals: "data")
        }
    }

    suite("exportDestinationURL") {
        test("builds URL from stagingDir + localIdentifier + extension") {
            let dir = URL(fileURLWithPath: "/tmp/staging")
            let url = exportDestinationURL(
                stagingDir: dir,
                localIdentifier: "ABC/L0/001",
                uti: "public.heic"
            )
            // localIdentifier slashes replaced with underscores
            try expect(url.lastPathComponent, equals: "ABC_L0_001.heic")
            try expect(url.deletingLastPathComponent().path, equals: "/tmp/staging")
        }

        test("JPEG UTI produces .jpg extension") {
            let url = exportDestinationURL(
                stagingDir: URL(fileURLWithPath: "/tmp"),
                localIdentifier: "XYZ",
                uti: "public.jpeg"
            )
            try expect(url.pathExtension, equals: "jpg")
        }
    }
}
