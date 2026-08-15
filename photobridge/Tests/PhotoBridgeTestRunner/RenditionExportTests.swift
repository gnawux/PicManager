import Foundation
import PhotoBridgeLib

func runRenditionExportTests() {
    suite("Rendition export contract") {
        test("keeps original filename inside an isolated package") {
            let paths = renditionPackagePaths(
                originalFilename: "IMG_0042.HEIC",
                currentUniformTypeIdentifier: "public.jpeg"
            )
            try expect(paths.originalRelativePath, equals: "original/IMG_0042.HEIC")
            try expect(paths.currentRelativePath, equals: "current/current.jpg")
            try expect(paths.manifestRelativePath, equals: "manifest.json")
        }

        test("strips path traversal from provider filenames") {
            let paths = renditionPackagePaths(originalFilename: "../../private/IMG_0042.HEIC")
            try expect(paths.originalRelativePath, equals: "original/IMG_0042.HEIC")
        }

        test("manifest distinguishes byte-preserved original from current appearance") {
            let original = RenditionFileManifest(
                role: "original", relativePath: "original/IMG.HEIC",
                originalFilename: "IMG.HEIC", uniformTypeIdentifier: "public.heic",
                mimeType: "image/heic", sha256: "abc", byteSize: 100,
                width: 100, height: 80, provenance: "photokit_resource",
                bytePreserved: true, orientationMode: "metadata", sourceOrientation: 6,
                displayOrientation: 6, colorSpace: "RGB", generationKey: nil
            )
            let current = RenditionFileManifest(
                role: "current", relativePath: "current/current.jpg",
                originalFilename: nil, uniformTypeIdentifier: "public.jpeg",
                mimeType: "image/jpeg", sha256: "def", byteSize: 80,
                width: 80, height: 60, provenance: "photokit_current",
                bytePreserved: false, orientationMode: "metadata", sourceOrientation: 1,
                displayOrientation: 1, colorSpace: "RGB", generationKey: "v2"
            )
            let manifest = RenditionPackageManifest(
                sourceIdentifier: "UUID/L0/001", originalFilename: "IMG.HEIC",
                adjusted: true, original: original, current: current
            )
            let data = try renditionManifestEncoder().encode(manifest)
            let text = String(decoding: data, as: UTF8.self)
            try expect(text.contains("\"byte_preserved\" : true"))
            try expect(text.contains("\"provenance\" : \"photokit_current\""))
            try expect(text.contains("\"source_identifier\" : \"UUID\\/L0\\/001\""))
        }

        test("maps known rendition UTIs to explicit media types") {
            try expect(mimeType(forUTI: "public.heic"), equals: "image/heic")
            try expect(mimeType(forUTI: "public.jpeg"), equals: "image/jpeg")
            try expect(mimeType(forUTI: "vendor.unknown"), equals: "application/octet-stream")
        }
    }
}
