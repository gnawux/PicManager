import Foundation
import Security

public enum GarminKeychain {
    private static let service = "io.picmanager.garmin"
    public static func password() throws -> String? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecReturnData as String: true]
        let status = SecItemCopyMatching(query as CFDictionary, nil)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw NSError(domain: "GarminKeychain", code: Int(status)) }
        var value: CFTypeRef?; SecItemCopyMatching(query as CFDictionary, &value)
        return (value as? Data).flatMap { String(data: $0, encoding: .utf8) }
    }
    public static func save(password: String) throws {
        let data = Data(password.utf8)
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service]
        SecItemDelete(query as CFDictionary)
        var value = query; value[kSecValueData as String] = data; value[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let status = SecItemAdd(value as CFDictionary, nil)
        guard status == errSecSuccess else { throw NSError(domain: "GarminKeychain", code: Int(status)) }
    }
}
