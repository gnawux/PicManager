import Photos
@testable import PhotoBridgeLib

struct MockResource: AssetResourceInfo {
    let type: PHAssetResourceType
    let uniformTypeIdentifier: String
}

func makeResources(_ specs: [(PHAssetResourceType, String)]) -> [any AssetResourceInfo] {
    specs.map { MockResource(type: $0.0, uniformTypeIdentifier: $0.1) }
}
