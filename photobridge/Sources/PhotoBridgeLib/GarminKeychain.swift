import Foundation
import Security

/// The macOS shell depends on this narrow interface instead of on `SecItem` directly.
/// It makes credential ownership explicit (one Garmin account per item) and lets the
/// presentation layer be exercised with a fake store without touching a user's keychain.
public protocol GarminCredentialStore {
    func password(for account: String) throws -> String?
    func save(password: String, for account: String) throws
}

public struct GarminKeychain: GarminCredentialStore {
    public static let defaultService = "io.picmanager.garmin"

    private let service: String

    public init(service: String = GarminKeychain.defaultService) {
        self.service = service
    }

    public func password(for account: String) throws -> String? {
        let normalizedAccount = try validatedAccount(account)
        let exactQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: normalizedAccount,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(exactQuery as CFDictionary, &result)
        if status == errSecSuccess {
            return try decodePassword(result)
        }
        guard status == errSecItemNotFound else { throw keychainError(status) }

        // PicManager 1.0 stored a service-only item. Read it only when the requested
        // account has no dedicated credential, then make an account-scoped copy. The
        // old item is intentionally retained: deleting a query without an account can
        // erase another account's credentials on machines that already upgraded.
        guard let legacy = try legacyPassword() else { return nil }
        try save(password: legacy, for: normalizedAccount)
        return legacy
    }

    public func save(password: String, for account: String) throws {
        let normalizedAccount = try validatedAccount(account)
        guard !password.isEmpty else { throw GarminKeychainError.emptyPassword }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: normalizedAccount,
        ]
        let data = Data(password.utf8)
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecSuccess { return }
        guard status == errSecItemNotFound else { throw keychainError(status) }

        var attributes = query
        attributes[kSecValueData as String] = data
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let addStatus = SecItemAdd(attributes as CFDictionary, nil)
        guard addStatus == errSecSuccess else { throw keychainError(addStatus) }
    }

    private func legacyPassword() throws -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnData as String: true,
            kSecReturnAttributes as String: true,
            kSecMatchLimit as String: kSecMatchLimitAll,
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw keychainError(status) }
        let matches = (result as? [[String: Any]]) ?? []
        guard let legacy = matches.first(where: { $0[kSecAttrAccount as String] == nil }) else { return nil }
        return try decodePassword(legacy[kSecValueData as String])
    }

    private func validatedAccount(_ account: String) throws -> String {
        let normalized = account.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !normalized.isEmpty else { throw GarminKeychainError.emptyAccount }
        return normalized
    }

    private func decodePassword(_ value: Any?) throws -> String {
        guard let data = value as? Data,
              let password = String(data: data, encoding: .utf8),
              !password.isEmpty else {
            throw GarminKeychainError.invalidStoredPassword
        }
        return password
    }

    private func keychainError(_ status: OSStatus) -> NSError {
        NSError(domain: NSOSStatusErrorDomain, code: Int(status))
    }
}

public enum GarminKeychainError: LocalizedError {
    case emptyAccount
    case emptyPassword
    case invalidStoredPassword

    public var errorDescription: String? {
        switch self {
        case .emptyAccount: "A Garmin account is required."
        case .emptyPassword: "A Garmin password is required."
        case .invalidStoredPassword: "The saved Garmin credential is invalid."
        }
    }
}
